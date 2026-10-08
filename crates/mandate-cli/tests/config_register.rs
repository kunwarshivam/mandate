//! D1 (E10-16, DEC-526): `config register` and `model register` store their object and commit one
//! `ConfigSnapshotRegistered`. Each committed draft is read back through `mandate_journal`'s own
//! schema check (§9.2) and compared with a payload written out here. The instrument snapshot is
//! held to DEC-523, the model's content and hash come from `mandate_modelhost::content`, and
//! nothing is registered outside paper.

mod common;

use std::collections::BTreeMap;

use common::{CONTROL, Journal, at, owner, stream};
use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_cli::config::{ConfigKind, register, register_model};
use mandate_cli::control::{ControlError, ControlJournal, Owner};
use mandate_journal::{Draft, Environment};

type Store = BTreeMap<Digest, Vec<u8>>;

const NOW: i64 = 1_790_000_000;
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"SPY"}"#;

/// The control stream's one event, accepted by the journal's schema check, and its payload.
fn registered(journal: &Journal) -> (Draft, Value) {
    let rows = journal.rows(&stream(CONTROL)).unwrap();
    let [row] = rows.as_slice() else {
        panic!("one event on the control stream: {rows:?}")
    };
    let draft = Draft::parse(&row.body).unwrap();
    (
        draft,
        parse(&row.body).unwrap().get("payload").unwrap().clone(),
    )
}

/// The payload §9.2 gives a registration of a non-model `kind` naming `canonical`.
fn plain_payload(kind: &str, canonical: &[u8]) -> Value {
    let hash = Digest::of(canonical).to_hex();
    let text = format!(
        r#"{{"admits_instruments":null,"content_hash":"sha256:{hash}","kind":"{kind}","model_id":null,"model_version":null,"params":[]}}"#
    );
    parse(text.as_bytes()).unwrap()
}

/// SPY's snapshot with `change` applied to its members.
fn spy_with(change: impl FnOnce(&mut BTreeMap<Key, Value>)) -> Vec<u8> {
    let Value::Object(mut members) = parse(SPY.as_bytes()).unwrap() else {
        panic!("SPY is an object")
    };
    change(&mut members);
    to_canonical(&Value::Object(members))
}

fn refused(result: Result<impl std::fmt::Debug, ControlError>) -> &'static str {
    match result {
        Err(ControlError::Refused { reason }) => reason,
        other => panic!("not refused: {other:?}"),
    }
}

#[test]
#[ignore = "pending E10-16"]
fn an_instrument_snapshot_of_dec_523_is_stored_registered_and_found_on_a_rerun() {
    for exchange in ["arca", "nasdaq"] {
        let object = spy_with(|m| {
            m.insert(
                Key::new("exchange").unwrap(),
                Value::Str(exchange.to_owned()),
            );
        });
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let kind = ConfigKind::InstrumentSnapshot;
        let first = register(&mut journal, &mut store, &owner(), kind, &object, at(NOW)).unwrap();
        let canonical = to_canonical(&parse(&object).unwrap());
        assert_eq!(
            store.get(&Digest::of(&canonical)),
            Some(&canonical),
            "{exchange}"
        );
        let (draft, payload) = registered(&journal);
        let shape = (draft.event_type(), draft.schema_version());
        assert_eq!(shape, ("ConfigSnapshotRegistered", 1));
        assert_eq!(payload, plain_payload("instrument_snapshot", &canonical));
        let again = register(
            &mut journal,
            &mut store,
            &owner(),
            kind,
            &object,
            at(NOW + 60),
        );
        assert_eq!(
            again.unwrap(),
            first,
            "a re-run finds the registration (DEC-290)"
        );
        registered(&journal);
    }
}

#[test]
#[ignore = "pending E10-16"]
fn a_snapshot_outside_dec_523_is_refused_before_anything_is_stored() {
    let set = |name: &'static str, value: Value| {
        move |m: &mut BTreeMap<Key, Value>| {
            m.insert(Key::new(name).unwrap(), value);
        }
    };
    let text = |s: &str| Value::Str(s.to_owned());
    let mut cases: Vec<(String, Vec<u8>)> = vec![
        ("an extra member".into(), spy_with(set("note", text("x")))),
        (
            "etp_source".into(),
            spy_with(set("etp_source", text("manual"))),
        ),
        ("exchange".into(), spy_with(set("exchange", text("nyse")))),
        (
            "asset_class".into(),
            spy_with(set("asset_class", text("crypto"))),
        ),
        ("etp".into(), spy_with(set("etp", text("leveraged")))),
        (
            "increment".into(),
            spy_with(set("increment", text("fractional"))),
        ),
        ("empty id".into(), spy_with(set("instrument_id", text("")))),
        ("empty symbol".into(), spy_with(set("symbol", text("")))),
        (
            "lower-case t".into(),
            spy_with(set("etp_classified_at", text("2026-10-05t13:30:00Z"))),
        ),
        (
            "no instant".into(),
            spy_with(set("etp_classified_at", text("yesterday"))),
        ),
    ];
    let names = parse(SPY.as_bytes()).unwrap();
    let Value::Object(names) = names else {
        panic!("SPY is an object")
    };
    for name in names.keys() {
        let gone = spy_with(|m| {
            m.remove(name.as_str());
        });
        let number = spy_with(set_key(name.clone(), Value::Bool(true)));
        cases.push((format!("{} missing", name.as_str()), gone));
        cases.push((format!("{} not a string", name.as_str()), number));
    }
    assert_eq!(cases.len(), 26, "every member removed and mistyped once");
    for (case, object) in cases {
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let kind = ConfigKind::InstrumentSnapshot;
        let result = register(&mut journal, &mut store, &owner(), kind, &object, at(NOW));
        assert_eq!(refused(result), "instrument_snapshot_invalid", "{case}");
        assert!(
            store.is_empty() && journal.attempts.is_empty(),
            "{case}: nothing written"
        );
    }
}

fn set_key(name: Key, value: Value) -> impl FnOnce(&mut BTreeMap<Key, Value>) {
    move |m| {
        m.insert(name, value);
    }
}

#[test]
#[ignore = "pending E10-16"]
fn the_other_kinds_register_at_their_schema_version_and_a_bad_object_is_refused() {
    let kinds = [
        (ConfigKind::FeeConfig, "fee_config", 1),
        (ConfigKind::TradingCalendar, "trading_calendar", 1),
        (ConfigKind::RuleSet, "rule_set", 1),
        (ConfigKind::PolicySet, "policy_set", 2),
        (ConfigKind::ModelRegistry, "model_registry", 2),
    ];
    for (kind, code, version) in kinds {
        let object = format!(r#"{{"kind":"{code}","note":"a {code} object"}}"#);
        let (mut journal, mut store) = (Journal::default(), Store::new());
        register(
            &mut journal,
            &mut store,
            &owner(),
            kind,
            object.as_bytes(),
            at(NOW),
        )
        .unwrap();
        let canonical = to_canonical(&parse(object.as_bytes()).unwrap());
        assert_eq!(
            store.get(&Digest::of(&canonical)),
            Some(&canonical),
            "{code}"
        );
        let (draft, payload) = registered(&journal);
        assert_eq!(draft.schema_version(), version, "{code}");
        assert_eq!(payload, plain_payload(code, &canonical), "{code}");
    }
    let cases: [(ConfigKind, &[u8], &str); 4] = [
        (
            ConfigKind::PolicySet,
            br#"{"kind":"model_registry"}"#,
            "config_kind_mismatch",
        ),
        (
            ConfigKind::ModelRegistry,
            br#"{"note":"no kind"}"#,
            "config_kind_mismatch",
        ),
        (ConfigKind::FeeConfig, b"[1,2]", "config_object_invalid"),
        (ConfigKind::RuleSet, b"not json", "config_object_invalid"),
    ];
    for (kind, object, reason) in cases {
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let result = register(&mut journal, &mut store, &owner(), kind, object, at(NOW));
        assert_eq!(refused(result), reason, "{kind:?}");
        assert!(
            store.is_empty() && journal.attempts.is_empty(),
            "{kind:?}: nothing written"
        );
    }
}

#[test]
#[ignore = "pending E10-16"]
fn a_model_is_registered_with_the_hosts_content_and_hash_only() {
    let (mut journal, mut store) = (Journal::default(), Store::new());
    let (id, version) = ("quant.ma_crossover", "1.0.0");
    register_model(&mut journal, &mut store, &owner(), id, version, at(NOW)).unwrap();
    let content = mandate_modelhost::content(id, version).unwrap();
    assert_eq!(Digest::of(&content.canonical), content.hash);
    assert_eq!(store.get(&content.hash), Some(&content.canonical));
    let object = parse(&content.canonical).unwrap();
    let names: Vec<&str> = object
        .get("params")
        .and_then(Value::as_array)
        .unwrap()
        .iter()
        .map(|p| p.get("name").and_then(Value::as_str).unwrap())
        .collect();
    assert_eq!(names, ["fast_periods", "slow_periods"]);
    let text = format!(
        r#"{{"admits_instruments":false,"content_hash":"sha256:{}","kind":"model_version","model_id":"{id}","model_version":"{version}","params":["fast_periods","slow_periods"]}}"#,
        content.hash.to_hex()
    );
    let (draft, payload) = registered(&journal);
    assert_eq!(
        (draft.schema_version(), payload),
        (1, parse(text.as_bytes()).unwrap())
    );
    for (id, version) in [("quant.other", "1.0.0"), ("quant.ma_crossover", "9.9.9")] {
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let result = register_model(&mut journal, &mut store, &owner(), id, version, at(NOW));
        assert_eq!(refused(result), "model_unknown", "{id} {version}");
        assert!(
            store.is_empty() && journal.attempts.is_empty(),
            "{id} {version}"
        );
    }
}

#[test]
#[ignore = "pending E10-16"]
fn nothing_is_registered_outside_paper() {
    for environment in [Environment::Live, Environment::Backtest] {
        let owner = Owner {
            environment,
            ..owner()
        };
        let (mut journal, mut store) = (Journal::default(), Store::new());
        let kind = ConfigKind::InstrumentSnapshot;
        let snapshot = register(
            &mut journal,
            &mut store,
            &owner,
            kind,
            SPY.as_bytes(),
            at(NOW),
        );
        let model = register_model(
            &mut journal,
            &mut store,
            &owner,
            "quant.ma_crossover",
            "1.0.0",
            at(NOW),
        );
        assert_eq!(
            [refused(snapshot), refused(model)],
            ["paper_only"; 2],
            "{environment:?}"
        );
        assert!(
            store.is_empty() && journal.attempts.is_empty(),
            "{environment:?}"
        );
    }
}
