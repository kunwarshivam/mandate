//! `behavior.research.event_triggers` (mandate spec v0.11 §1, §8.4, §9.2; DEC-2392; E19-12): the
//! parse reads it as the schema states, absent and `null` both being off; V-009 keeps its sources
//! sorted; and a version change classifies it by direction, so a change that lets more items start
//! model calls needs step-up and one that lets fewer is reducing (rules 2 and 11).

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{arr, base, edit, i, obj, s, with};
use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_spec::Mandate;
use mandate_spec::ParseError;
use mandate_spec::change::{ChangeClass, classify};
use mandate_spec::document::ProvenanceMap;
use mandate_spec::document::{EventTrigger, EventTriggers};
use mandate_spec::validate::{ValidationContext, validate};
use mandate_time::Date;

use ChangeClass::{Neutral, RiskIncreasing, RiskReducing};

/// The research envelope with `triggers` as its `event_triggers`, or without the member at all.
fn research(triggers: Option<Value>) -> Value {
    let mut members = vec![
        ("interval_s", i(3600)),
        ("cost_cap_usd_per_day", s("5")),
        ("max_revisions_per_lineage", i(3)),
    ];
    if let Some(triggers) = triggers {
        members.push(("event_triggers", triggers));
    }
    obj(members)
}

fn triggers(sources: &[&str], debounce_s: u64, max_runs_per_day: u64) -> Value {
    obj(vec![
        (
            "sources",
            arr(sources.iter().map(|source| s(source)).collect()),
        ),
        ("debounce_s", i(debounce_s)),
        ("max_runs_per_day", i(max_runs_per_day)),
    ])
}

fn researched(triggers: Option<Value>) -> Value {
    with("/behavior/research", Some(research(triggers)))
}

fn parsed(document: &Value) -> Result<Option<EventTriggers>, ParseError> {
    Mandate::parse(document).map(|mandate| {
        mandate
            .behavior
            .research
            .and_then(|research| research.event_triggers)
    })
}

fn refused_at(document: &Value) -> String {
    match Mandate::parse(document) {
        Err(
            ParseError::UnknownMember { path }
            | ParseError::MissingMember { path }
            | ParseError::WrongType { path }
            | ParseError::NotInEnum { path }
            | ParseError::OutOfBounds { path }
            | ParseError::NotUnique { path },
        ) => path.as_str().to_owned(),
        other => panic!("refused with a path, not {other:?}"),
    }
}

#[test]
fn absent_and_null_are_off_and_a_set_value_reads_as_written() {
    assert_eq!(parsed(&researched(None)), Ok(None), "absent is off");
    assert_eq!(
        parsed(&researched(Some(Value::Null))),
        Ok(None),
        "null is off"
    );
    assert_eq!(
        parsed(&researched(Some(triggers(&["filings", "news"], 900, 6)))),
        Ok(Some(EventTriggers {
            sources: BTreeSet::from([EventTrigger::Filings, EventTrigger::News]),
            debounce_s: 900,
            max_runs_per_day: 6,
        }))
    );
    assert_eq!(
        parsed(&researched(Some(triggers(&["news"], 300, 1)))),
        Ok(Some(EventTriggers {
            sources: BTreeSet::from([EventTrigger::News]),
            debounce_s: 300,
            max_runs_per_day: 1,
        })),
        "the lower bounds are inside"
    );
    assert!(
        parsed(&researched(Some(triggers(&["filings"], 86_400, 24)))).is_ok(),
        "the upper bounds are inside"
    );
}

#[test]
fn every_bound_and_member_the_schema_states_is_refused_at_its_pointer() {
    let at = "/behavior/research/event_triggers";
    for (document, pointer) in [
        (
            researched(Some(triggers(&["news"], 299, 6))),
            format!("{at}/debounce_s"),
        ),
        (
            researched(Some(triggers(&["news"], 86_401, 6))),
            format!("{at}/debounce_s"),
        ),
        (
            researched(Some(triggers(&["news"], 900, 0))),
            format!("{at}/max_runs_per_day"),
        ),
        (
            researched(Some(triggers(&["news"], 900, 25))),
            format!("{at}/max_runs_per_day"),
        ),
        (
            researched(Some(triggers(&[], 900, 6))),
            format!("{at}/sources"),
        ),
        (
            researched(Some(triggers(&["price"], 900, 6))),
            format!("{at}/sources/0"),
        ),
        (
            researched(Some(triggers(&["news", "news"], 900, 6))),
            format!("{at}/sources/1"),
        ),
        (
            researched(Some(edit(
                &triggers(&["news"], 900, 6),
                &[("/extra", Some(i(1)))],
            ))),
            format!("{at}/extra"),
        ),
        (
            researched(Some(edit(
                &triggers(&["news"], 900, 6),
                &[("/debounce_s", None)],
            ))),
            format!("{at}/debounce_s"),
        ),
        (researched(Some(s("news"))), at.to_owned()),
    ] {
        assert_eq!(refused_at(&document), pointer);
    }
}

/// V-009: set-like arrays are sorted, so equal mandates hash equally.
#[test]
fn unsorted_sources_break_v009() {
    use mandate_spec::Violation;
    let violations = |document: &Value| {
        let mandate = Mandate::parse(document).expect("the document parses");
        validate(&mandate, &context())
            .expect("the document is evaluable")
            .violations
            .contains(&Violation::V009)
    };
    assert!(!violations(&researched(Some(triggers(
        &["filings", "news"],
        900,
        6
    )))));
    assert!(violations(&researched(Some(triggers(
        &["news", "filings"],
        900,
        6
    )))));
}

fn context() -> ValidationContext {
    ValidationContext {
        account_equity_usd: Usd::parse("25000").expect("dollars"),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-24").expect("a date"),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
        stop_limit_asset_classes: mandate_spec::context::stop_limit_asset_classes("alpaca"),
    }
}

fn class(old: &Value, new: &Value) -> (ChangeClass, bool) {
    let c = classify(
        &Mandate::parse(old).expect("the old version parses"),
        &Mandate::parse(new).expect("the new version parses"),
    )
    .expect("two parsed mandates classify");
    (c.class, c.step_up_required)
}

/// §9.2's row: increasing when the field is set, a source is added, `debounce_s` falls, or
/// `max_runs_per_day` rises; reducing for the reverse of each; absent and `null` alike.
#[test]
fn a_change_classifies_by_whether_it_lets_more_items_start_runs() {
    let on = || researched(Some(triggers(&["news"], 900, 6)));
    let off = researched(None);
    let null = researched(Some(Value::Null));
    for (old, new, expected) in [
        (off.clone(), on(), RiskIncreasing),
        (null.clone(), on(), RiskIncreasing),
        (on(), off.clone(), RiskReducing),
        (on(), null.clone(), RiskReducing),
        (off.clone(), null.clone(), Neutral),
        (
            on(),
            researched(Some(triggers(&["filings", "news"], 900, 6))),
            RiskIncreasing,
        ),
        (
            researched(Some(triggers(&["filings", "news"], 900, 6))),
            on(),
            RiskReducing,
        ),
        (
            on(),
            researched(Some(triggers(&["news"], 600, 6))),
            RiskIncreasing,
        ),
        (
            on(),
            researched(Some(triggers(&["news"], 1800, 6))),
            RiskReducing,
        ),
        (
            on(),
            researched(Some(triggers(&["news"], 900, 7))),
            RiskIncreasing,
        ),
        (
            on(),
            researched(Some(triggers(&["news"], 900, 2))),
            RiskReducing,
        ),
        (
            on(),
            researched(Some(triggers(&["news"], 1800, 7))),
            RiskIncreasing,
        ),
        (
            on(),
            researched(Some(triggers(&["filings"], 900, 6))),
            RiskIncreasing,
        ),
    ] {
        let (got, step_up) = class(&old, &new);
        assert_eq!(got, expected, "{old:?} -> {new:?}");
        assert_eq!(step_up, expected == RiskIncreasing);
    }
    assert_eq!(
        class(&on(), &base()).0,
        RiskReducing,
        "clearing the research agent as a whole stays reducing"
    );
}
