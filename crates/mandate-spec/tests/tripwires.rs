//! E6-13 tripwires (mandate spec MI-31, V-044, §6.7, §9.2; DEC-350 to DEC-352).
//!
//! Every behavior test is pending on the tests-PR stubs. The live vocabulary and refusal tests pin
//! the public boundary and prove the stubs fail closed.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{arr, base, edit, i, obj, s, with};
use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_spec::change::{ChangeClass, classify, classify_tripwires};
use mandate_spec::document::{
    ConnectionId, Pointer, Provenance, ProvenanceMap, Source, Tripwire, TripwireAction, TripwireId,
    TripwireMetric,
};
use mandate_spec::validate::{PreviousVersion, ValidationContext, validate};
use mandate_spec::{DecGrammar, Mandate, ParseError, SchemaDec, Violation};
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

use ChangeClass::{Neutral, RiskIncreasing, RiskReducing};
use TripwireAction::{EndDelegations, ExitsOnly};
use TripwireMetric::{ConsecutiveLosingExits, NewInstruments, RealizedLossUsd};

fn dec(text: &str) -> SchemaDec {
    SchemaDec::parse(text, DecGrammar::PositiveDecimal).expect("a positive decimal")
}

fn tripwire_value(id: &str, metric: &str, threshold: &str, action: &str) -> Value {
    obj(vec![
        ("id", s(id)),
        ("metric", s(metric)),
        ("threshold", s(threshold)),
        ("action", s(action)),
    ])
}

fn tripwired(items: Vec<Value>) -> Value {
    with("/autonomy/tripwires", Some(arr(items)))
}

fn typed(id: &str, metric: TripwireMetric, threshold: u32, action: TripwireAction) -> Tripwire {
    Tripwire {
        id: TripwireId::parse(id).expect("a tripwire id"),
        metric,
        threshold: dec(&threshold.to_string()),
        action,
    }
}

fn context(previous: Option<&Value>, confirmed: bool) -> ValidationContext {
    let provenance = ProvenanceMap::new(BTreeMap::from([(
        Pointer::new("/autonomy/tripwires"),
        Provenance {
            source: Source::PlatformProposed,
            confirmed,
        },
    )]));
    ValidationContext {
        account_equity_usd: Usd::parse("25000").expect("a dollar amount"),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-24").expect("a date"),
        registry: None,
        provenance,
        workspace_users: 2,
        approver_users: 2,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: previous.map(|document| PreviousVersion {
            environment: Environment::Paper,
            connection_id: ConnectionId::parse("conn_alpaca_paper_01").expect("a connection id"),
            mandate: Some(Mandate::parse(document).expect("the previous mandate parses")),
            mandate_version: None,
        }),
        current_mandate_version: None,
    }
}

#[test]
fn tripwire_public_vocabulary_is_closed_and_ordered() {
    assert_eq!(
        TripwireMetric::ALL.map(TripwireMetric::as_str),
        [
            "consecutive_losing_exits",
            "realized_loss_usd",
            "new_instruments"
        ]
    );
    assert_eq!(
        TripwireAction::ALL.map(TripwireAction::as_str),
        ["end_delegations", "exits_only"]
    );
    assert!(
        EndDelegations < ExitsOnly,
        "exits_only is the stricter action"
    );
    assert_eq!(Violation::V044.code(), "V-044");
    assert_eq!(
        TripwireId::parse("losing_streak")
            .expect("a tripwire id")
            .as_str(),
        "losing_streak"
    );
    assert!(TripwireId::parse("LosingStreak").is_err());
    assert!(TripwireId::parse(&format!("t{}", "x".repeat(32))).is_err());
}

#[test]
fn tripwire_parser_and_classifier_are_live() {
    let stated = tripwired(vec![tripwire_value(
        "losing_streak",
        "consecutive_losing_exits",
        "2",
        "exits_only",
    )]);
    assert_eq!(
        Mandate::parse(&stated)
            .expect("the tripwire parses")
            .autonomy
            .tripwires,
        vec![typed("losing_streak", ConsecutiveLosingExits, 2, ExitsOnly)]
    );
    assert_eq!(
        classify_tripwires(
            &[],
            &[typed("losing_streak", ConsecutiveLosingExits, 2, ExitsOnly)]
        ),
        Ok(RiskReducing)
    );
    assert_eq!(classify_tripwires(&[], &[]), Ok(Neutral));
}

/// The four document members parse to the public types, absence is empty, the list accepts twenty,
/// and a twenty-first item is refused.
#[test]
fn tripwires_parse_member_by_member_and_enforce_the_schema_list_bound() {
    let values = vec![
        tripwire_value("day_loss", "realized_loss_usd", "100.25", "end_delegations"),
        tripwire_value(
            "losing_streak",
            "consecutive_losing_exits",
            "2",
            "exits_only",
        ),
        tripwire_value("new_names", "new_instruments", "3", "end_delegations"),
    ];
    let parsed = Mandate::parse(&tripwired(values)).expect("tripwires parse");
    assert_eq!(
        parsed.autonomy.tripwires,
        vec![
            Tripwire {
                id: TripwireId::parse("day_loss").expect("an id"),
                metric: RealizedLossUsd,
                threshold: dec("100.25"),
                action: EndDelegations,
            },
            typed("losing_streak", ConsecutiveLosingExits, 2, ExitsOnly),
            typed("new_names", NewInstruments, 3, EndDelegations),
        ]
    );
    assert!(
        Mandate::parse(&base())
            .expect("the base parses")
            .autonomy
            .tripwires
            .is_empty()
    );
    let twenty: Vec<Value> = (0..20)
        .map(|n| {
            tripwire_value(
                &format!("t{n:02}"),
                "new_instruments",
                "1",
                "end_delegations",
            )
        })
        .collect();
    assert_eq!(
        Mandate::parse(&tripwired(twenty.clone()))
            .expect("twenty tripwires parse")
            .autonomy
            .tripwires
            .len(),
        20
    );
    let mut twenty_one = twenty;
    twenty_one.push(tripwire_value(
        "t20",
        "new_instruments",
        "1",
        "end_delegations",
    ));
    assert!(Mandate::parse(&tripwired(twenty_one)).is_err());
}

/// The schema's metric and action lists are closed, `paused` is impossible, every member is
/// required, ids use the rule-id grammar, and thresholds are positive decimal strings.
#[test]
fn tripwire_schema_rejects_every_member_shape_outside_its_closed_vocabulary() {
    let valid = tripwire_value(
        "losing_streak",
        "consecutive_losing_exits",
        "2",
        "exits_only",
    );
    for (invalid, expected) in [
        (
            edit(&valid, &[("/metric", Some(s("unrealized_loss_usd")))]),
            ParseError::NotInEnum {
                path: Pointer::new("/autonomy/tripwires/0/metric"),
            },
        ),
        (
            edit(&valid, &[("/action", Some(s("paused")))]),
            ParseError::NotInEnum {
                path: Pointer::new("/autonomy/tripwires/0/action"),
            },
        ),
        (
            edit(&valid, &[("/threshold", None)]),
            ParseError::MissingMember {
                path: Pointer::new("/autonomy/tripwires/0/threshold"),
            },
        ),
        (
            edit(&valid, &[("/threshold", Some(s("0")))]),
            ParseError::OffGrammar {
                path: Pointer::new("/autonomy/tripwires/0/threshold"),
                grammar: DecGrammar::PositiveDecimal,
            },
        ),
        (
            edit(&valid, &[("/threshold", Some(i(2)))]),
            ParseError::DecimalAsNumber {
                path: Pointer::new("/autonomy/tripwires/0/threshold"),
            },
        ),
        (
            edit(&valid, &[("/id", Some(s("Bad-ID")))]),
            ParseError::OffPattern {
                path: Pointer::new("/autonomy/tripwires/0/id"),
            },
        ),
        (
            edit(&valid, &[("/extra", Some(s("unknown")))]),
            ParseError::UnknownMember {
                path: Pointer::new("/autonomy/tripwires/0/extra"),
            },
        ),
    ] {
        assert_eq!(Mandate::parse(&tripwired(vec![invalid])), Err(expected));
    }
}

/// V-044 requires ids to be strictly sorted and unique.
#[test]
fn v044_requires_sorted_unique_ids() {
    let item = |id| tripwire_value(id, "new_instruments", "2", "end_delegations");
    let valid = Mandate::parse(&tripwired(vec![item("a"), item("b")])).expect("sorted parses");
    assert!(
        !validate(&valid, &context(None, true))
            .expect("validation runs")
            .violations
            .contains(&Violation::V044)
    );
    for items in [vec![item("b"), item("a")], vec![item("a"), item("a")]] {
        let mandate = Mandate::parse(&tripwired(items)).expect("the schema admits id order");
        assert!(
            validate(&mandate, &context(None, true))
                .expect("validation runs")
                .violations
                .contains(&Violation::V044)
        );
    }
}

/// Count thresholds are whole numbers in the inclusive range 1 through 1,000.
#[test]
fn v044_checks_every_count_threshold_edge() {
    for (threshold, valid) in [
        ("1", true),
        ("1000", true),
        ("0.5", false),
        ("1.5", false),
        ("1001", false),
    ] {
        for metric in ["consecutive_losing_exits", "new_instruments"] {
            let mandate = Mandate::parse(&tripwired(vec![tripwire_value(
                "count",
                metric,
                threshold,
                "end_delegations",
            )]))
            .expect("the schema admits a positive decimal");
            assert_eq!(
                !validate(&mandate, &context(None, true))
                    .expect("validation runs")
                    .violations
                    .contains(&Violation::V044),
                valid,
                "{metric} at {threshold}"
            );
        }
    }
}

/// Realized-loss thresholds are in whole cents and no larger than the allocation, inclusively.
#[test]
fn v044_checks_the_realized_loss_cent_and_allocation_edges() {
    for (threshold, valid) in [
        ("0.01", true),
        ("10000", true),
        ("0.001", false),
        ("10000.01", false),
    ] {
        let mandate = Mandate::parse(&tripwired(vec![tripwire_value(
            "day_loss",
            "realized_loss_usd",
            threshold,
            "end_delegations",
        )]))
        .expect("the schema admits a positive decimal");
        assert_eq!(
            !validate(&mandate, &context(None, true))
                .expect("validation runs")
                .violations
                .contains(&Violation::V044),
            valid,
            "{threshold}"
        );
    }
}

/// Tripwires may be platform-proposed only when confirmed; an owner-entered value remains valid.
#[test]
fn v020_requires_a_proposed_tripwire_to_be_confirmed() {
    let document = tripwired(vec![tripwire_value(
        "day_loss",
        "realized_loss_usd",
        "100",
        "end_delegations",
    )]);
    let mandate = Mandate::parse(&document).expect("the tripwire parses");
    assert!(
        !validate(&mandate, &context(None, true))
            .expect("validation runs")
            .violations
            .contains(&Violation::V020)
    );
    assert!(
        validate(&mandate, &context(None, false))
            .expect("validation runs")
            .violations
            .contains(&Violation::V020)
    );
}

/// Matching is by id: additions, lower thresholds, and stricter actions reduce; removals, higher
/// thresholds, softer actions, and metric changes increase and require step-up through `classify`.
#[test]
fn the_tripwire_change_row_covers_every_shape_and_matches_by_id() {
    let a = typed("a", ConsecutiveLosingExits, 3, EndDelegations);
    let b = typed("b", NewInstruments, 4, ExitsOnly);
    let cases = [
        (vec![], vec![a.clone()], RiskReducing),
        (
            vec![a.clone()],
            vec![typed("a", ConsecutiveLosingExits, 2, EndDelegations)],
            RiskReducing,
        ),
        (
            vec![a.clone()],
            vec![typed("a", ConsecutiveLosingExits, 3, ExitsOnly)],
            RiskReducing,
        ),
        (vec![a.clone(), b.clone()], vec![b.clone()], RiskIncreasing),
        (
            vec![a.clone()],
            vec![typed("a", ConsecutiveLosingExits, 4, EndDelegations)],
            RiskIncreasing,
        ),
        (
            vec![b.clone()],
            vec![typed("b", NewInstruments, 4, EndDelegations)],
            RiskIncreasing,
        ),
        (
            vec![a],
            vec![typed("a", NewInstruments, 3, EndDelegations)],
            RiskIncreasing,
        ),
    ];
    for (old, new, expected) in cases {
        assert_eq!(
            classify_tripwires(&old, &new).expect("tripwires classify"),
            expected
        );
    }
}

/// The tripwire path is its own row, so a reducing tripwire beside an increasing change joins to
/// increasing, and a reducing tripwire alone has exactly one changed path and no step-up.
#[test]
fn the_tripwire_row_is_wired_into_the_whole_mandate_join() {
    let added = tripwired(vec![tripwire_value(
        "losing_streak",
        "consecutive_losing_exits",
        "2",
        "exits_only",
    )]);
    let old = Mandate::parse(&base()).expect("the base parses");
    let new = Mandate::parse(&added).expect("the tripwire parses");
    let reducing = classify(&old, &new).expect("the change classifies");
    assert_eq!(reducing.class, RiskReducing);
    assert_eq!(
        reducing.changed_paths,
        vec![Pointer::new("/autonomy/tripwires")]
    );
    assert!(!reducing.step_up_required);
    let mixed = edit(&added, &[("/capital/allocation_usd", Some(s("11000")))]);
    let increasing = classify(
        &old,
        &Mandate::parse(&mixed).expect("the mixed version parses"),
    )
    .expect("the mixed change classifies");
    assert_eq!(increasing.class, RiskIncreasing);
    assert!(increasing.step_up_required);
}

/// Removing a tripwire is increasing and V-042 therefore refuses a carried delegation; adding one
/// is reducing and carries the same delegation.
#[test]
fn v042_reads_the_tripwire_change_row() {
    let delegation = obj(vec![
        ("id", s("d1")),
        ("lifts", s("rule:large_orders")),
        (
            "when",
            obj(vec![
                ("field", s("purpose")),
                ("op", s("in")),
                ("value", arr(vec![s("increase"), s("open")])),
            ]),
        ),
        ("max_order_usd", s("1000")),
        ("max_orders", i(3)),
        ("max_total_usd", s("3000")),
        ("starts_at", s("2026-09-24T00:00:00.000000000Z")),
        ("expires_at", s("2026-10-14T00:00:00.000000000Z")),
        ("source_approval_id", Value::Null),
    ]);
    let delegated = with("/autonomy/delegations", Some(arr(vec![delegation])));
    let prior = edit(
        &delegated,
        &[(
            "/autonomy/tripwires",
            Some(arr(vec![tripwire_value(
                "losing_streak",
                "consecutive_losing_exits",
                "2",
                "exits_only",
            )])),
        )],
    );
    let removed = delegated;
    let removed_mandate = Mandate::parse(&removed).expect("the new version parses");
    let report = validate(&removed_mandate, &context(Some(&prior), true))
        .expect("the version can be validated");
    assert!(
        report.violations.contains(&Violation::V042),
        "removing a tripwire is an increasing path, so d1 cannot carry"
    );
}

fn oracle_class(old: &[Tripwire], new: &[Tripwire]) -> ChangeClass {
    if old == new {
        return Neutral;
    }
    let increasing = old.iter().any(|before| {
        new.iter()
            .find(|after| after.id == before.id)
            .is_none_or(|after| {
                after.metric != before.metric
                    || after.threshold > before.threshold
                    || after.action < before.action
            })
    });
    if increasing {
        RiskIncreasing
    } else {
        RiskReducing
    }
}

fn drawn_tripwires() -> impl Strategy<Value = (Vec<Tripwire>, Vec<Tripwire>)> {
    prop::collection::btree_map(
        0u8..8,
        (
            prop::sample::select(&TripwireMetric::ALL),
            1u32..=1000,
            prop::sample::select(&TripwireAction::ALL),
            prop::option::of((
                prop::sample::select(&TripwireMetric::ALL),
                1u32..=1000,
                prop::sample::select(&TripwireAction::ALL),
            )),
        ),
        0..8,
    )
    .prop_map(|drawn| {
        let mut old = Vec::new();
        let mut new = Vec::new();
        for (id, (metric, threshold, action, changed)) in drawn {
            old.push(typed(&format!("t{id}"), metric, threshold, action));
            if let Some((new_metric, new_threshold, new_action)) = changed {
                new.push(typed(
                    &format!("t{id}"),
                    new_metric,
                    new_threshold,
                    new_action,
                ));
            }
        }
        (old, new)
    })
}

/// MI-31's version clause: an independent id-indexed oracle classifies random valid lists, including
/// removals and metric changes. It shares no production comparison helper.
#[test]
fn property_tripwire_classification_matches_an_independent_oracle() {
    let mut runner = TestRunner::default();
    runner
        .run(&drawn_tripwires(), |(old, new)| {
            prop_assert_eq!(classify_tripwires(&old, &new), Ok(oracle_class(&old, &new)));
            Ok(())
        })
        .expect("the tripwire classifier matches the independent oracle");
}
