//! DEC-942: the execution plan the order builder produced for an asked order is bound under the
//! content hash, so the grant that repeats the hash approves exactly that plan. Each expected
//! object is the test's own, built member by member, never through `execution_object`.

mod common;

use common::{answer, content, price};
use mandate_approval::{
    AssetClass, ExecutionTerms, ProtectionTerms, TimeInForce, content_hash, content_object,
    execution_object,
};
use mandate_canon::{Key, Object, Value};
use proptest::prelude::*;

fn text(s: &str) -> Value {
    Value::Str(s.to_owned())
}

fn object(members: &[(&str, Value)]) -> Value {
    let mut out = Object::new();
    for (key, value) in members {
        out.insert(Key::new(key).unwrap(), value.clone());
    }
    Value::Object(out)
}

fn bracket() -> ExecutionTerms {
    ExecutionTerms {
        asset_class: AssetClass::UsEquity,
        tif: TimeInForce::Gtc,
        protection_required: true,
        protection: Some(ProtectionTerms {
            stop: price("177.89"),
            take_profit: Some(price("205.98")),
        }),
    }
}

/// The oracle's own `execution` member: DEC-942 item 2's members, in its spellings.
fn expected(terms: &ExecutionTerms) -> Value {
    let class = match terms.asset_class {
        AssetClass::UsEquity => "us_equity",
        AssetClass::Crypto => "crypto",
    };
    let tif = match terms.tif {
        TimeInForce::Day => "day",
        TimeInForce::Gtc => "gtc",
        TimeInForce::Ioc => "ioc",
    };
    let protection = terms.protection.map_or(Value::Null, |p| {
        object(&[
            ("stop", text(&p.stop.to_string())),
            (
                "take_profit",
                p.take_profit
                    .map_or(Value::Null, |tp| text(&tp.to_string())),
            ),
        ])
    });
    object(&[
        ("asset_class", text(class)),
        ("tif", text(tif)),
        (
            "protection_required",
            Value::Bool(terms.protection_required),
        ),
        ("protection", protection),
    ])
}

#[test]
fn a_plan_is_a_member_of_the_content_object() {
    let terms = bracket();
    let planned = mandate_approval::RequestContent {
        execution: Some(terms),
        ..content()
    };
    let value = answer("content_object", content_object(&planned));
    assert_eq!(value.get("execution"), Some(&expected(&terms)));
    assert_eq!(
        answer("execution_object", execution_object(&terms)),
        expected(&terms)
    );
}

#[test]
fn a_request_without_a_plan_keeps_the_version_one_content() {
    let value = answer("content_object", content_object(&content()));
    assert_eq!(value.get("execution"), None, "no plan, no member");
}

#[test]
fn the_hash_covers_the_plan() {
    let unplanned = answer("content_hash", content_hash(&content()));
    let planned = |terms: ExecutionTerms| {
        answer(
            "content_hash",
            content_hash(&mandate_approval::RequestContent {
                execution: Some(terms),
                ..content()
            }),
        )
    };
    let bracket_hash = planned(bracket());
    assert_ne!(bracket_hash, unplanned);
    let moved_stop = ExecutionTerms {
        protection: Some(ProtectionTerms {
            stop: price("177.88"),
            take_profit: Some(price("205.98")),
        }),
        ..bracket()
    };
    assert_ne!(
        planned(moved_stop),
        bracket_hash,
        "a moved stop is a new hash"
    );
    let day = ExecutionTerms {
        tif: TimeInForce::Day,
        ..bracket()
    };
    assert_ne!(
        planned(day),
        bracket_hash,
        "another time in force is a new hash"
    );
}

fn tif() -> impl Strategy<Value = TimeInForce> {
    prop_oneof![
        Just(TimeInForce::Day),
        Just(TimeInForce::Gtc),
        Just(TimeInForce::Ioc)
    ]
}

fn cents() -> impl Strategy<Value = mandate_num::Price> {
    (1u64..10_000_000).prop_map(|c| {
        let written = format!("{}.{:02}", c / 100, c % 100);
        price(written.trim_end_matches('0').trim_end_matches('.'))
    })
}

fn terms() -> impl Strategy<Value = ExecutionTerms> {
    (
        prop_oneof![Just(AssetClass::UsEquity), Just(AssetClass::Crypto)],
        tif(),
        any::<bool>(),
        proptest::option::of((cents(), proptest::option::of(cents()))),
    )
        .prop_map(
            |(asset_class, tif, protection_required, protection)| ExecutionTerms {
                asset_class,
                tif,
                protection_required,
                protection: protection
                    .map(|(stop, take_profit)| ProtectionTerms { stop, take_profit }),
            },
        )
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// DEC-942: every plan is written as the oracle writes it, and two plans that differ give two
    /// hashes, so no plan can stand in for another under one grant.
    #[test]
    fn every_plan_is_written_exactly_and_hashed_apart(a in terms(), b in terms()) {
        prop_assert_eq!(answer("execution_object", execution_object(&a)), expected(&a));
        let hash = |t: ExecutionTerms| answer("content_hash", content_hash(
            &mandate_approval::RequestContent { execution: Some(t), ..content() },
        ));
        prop_assert_eq!(hash(a) == hash(b), a == b);
    }
}
