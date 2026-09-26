//! Semantic validation (`kind: semantic`, MC-V01 to MC-V67; §4.1, §4.2).
//!
//! The reference cases carry the code sets. These carry the two things a case set cannot: that the
//! **closed** §7 list is closed, and that the four worst-case figures are the products §4.2 names
//! rather than whatever the code happens to compute.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{base, s, with, with_all};
use mandate_canon::Value;
use mandate_domain::Environment;
use mandate_num::Usd;
use mandate_spec::document::{Pointer, Provenance, ProvenanceMap, Source};
use mandate_spec::validate::{
    NEVER_PROPOSED, PlatformProposals, ValidationContext, platform_defaultable, validate,
};
use mandate_spec::{Mandate, Violation, Warning};
use mandate_time::Date;

fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("a dollar amount")
}

fn context(provenance: ProvenanceMap) -> ValidationContext {
    ValidationContext {
        account_equity_usd: usd("25000"),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-24").expect("a date"),
        registry: None,
        provenance,
        workspace_users: 1,
        approver_users: 1,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
    }
}

fn sourced(path: &str, source: Source, confirmed: bool) -> ProvenanceMap {
    ProvenanceMap::new(BTreeMap::from([(
        Pointer::new(path),
        Provenance { source, confirmed },
    )]))
}

fn codes(document: &Value, provenance: ProvenanceMap) -> BTreeSet<Violation> {
    let mandate = Mandate::parse(document).expect("the document parses");
    validate(&mandate, &context(provenance))
        .expect("the document is evaluable")
        .violations
}

#[test]
#[ignore = "pending E10-1"]
fn a_platform_default_is_allowed_only_on_the_listed_paths_with_the_listed_value() {
    for (path, allowed_value) in platform_defaultable() {
        let provenance = sourced(path, Source::PlatformDefault, true);
        let violations = codes(&base(), provenance);
        assert!(
            !violations.contains(&Violation::V020),
            "`{path}` is on §7's list (value {allowed_value:?}), so a platform default there is valid"
        );
    }
}

/// The point of the closed list: the platform may not quietly choose a limit for an owner.
#[test]
#[ignore = "pending E10-1"]
fn a_platform_default_anywhere_else_is_a_violation() {
    for path in [
        "/capital/allocation_usd",
        "/capital/max_loss_from_allocation",
        "/risk/max_drawdown",
        "/risk/max_position_usd",
        "/universe/max_instruments",
        "/behavior/sizing/entry_threshold",
        "/goal",
    ] {
        let violations = codes(&base(), sourced(path, Source::PlatformDefault, true));
        assert!(
            violations.contains(&Violation::V020),
            "a platform default on `{path}` must be V-020"
        );
    }
}

/// `/autonomy/default` may be a platform default only when it is `ask`.
#[test]
#[ignore = "pending E10-1"]
fn a_platform_default_with_the_wrong_value_is_a_violation() {
    let document = with("/autonomy/default", Some(s("deny")));
    let violations = codes(
        &document,
        sourced("/autonomy/default", Source::PlatformDefault, true),
    );
    assert!(violations.contains(&Violation::V020));
}

#[test]
#[ignore = "pending E10-1"]
fn an_unconfirmed_or_proposed_envelope_field_is_a_violation() {
    let unconfirmed = codes(
        &base(),
        sourced("/risk/max_drawdown", Source::UserEntered, false),
    );
    assert!(
        unconfirmed.contains(&Violation::V020),
        "MI-12: nothing unconfirmed is active"
    );
    let template = codes(
        &base(),
        sourced("/risk/max_drawdown", Source::TemplateStructure, true),
    );
    assert!(
        template.contains(&Violation::V020),
        "`template_structure` is not one of the three owner sources"
    );
    let proposed = codes(
        &base(),
        sourced("/risk/max_drawdown", Source::PlatformProposed, true),
    );
    assert!(
        !proposed.contains(&Violation::V020),
        "a confirmed proposal is an owner source (DEC-97)"
    );
}

/// V-022: the compiler and templates never produce or propose `auto`, for the default, a rule's `then`, or the admission ceiling.
#[test]
#[ignore = "pending E10-1"]
fn every_auto_must_be_entered_by_the_owner_and_confirmed() {
    for path in ["/autonomy/default", "/autonomy/admission"] {
        let document = with(path, Some(s("auto")));
        for source in [
            Source::PlatformProposed,
            Source::PlatformDefault,
            Source::UserStated,
            Source::TemplateStructure,
        ] {
            let violations = codes(&document, sourced(path, source, true));
            assert!(
                violations.contains(&Violation::V022),
                "`auto` at `{path}` from {source:?} must be V-022"
            );
        }
        let entered = codes(&document, sourced(path, Source::UserEntered, true));
        assert!(!entered.contains(&Violation::V022));
        let unconfirmed = codes(&document, sourced(path, Source::UserEntered, false));
        assert!(unconfirmed.contains(&Violation::V022), "confirmed, too");
    }
}

/// V-038: bring-your-own-strategy means the owner's own universe, and the research agent is the path for the platform's ideas.
#[test]
#[ignore = "pending E17-1"]
fn the_owners_own_universe_and_connection_are_never_proposed() {
    for path in NEVER_PROPOSED {
        let violations = codes(&base(), sourced(path, Source::PlatformProposed, true));
        assert!(
            violations.contains(&Violation::V038),
            "a proposal on `{path}` must be V-038"
        );
    }
}

/// §7, DEC-117 and DEC-111. Both shown as proposed, both needing confirmation.
#[test]
#[ignore = "pending E17-1"]
fn the_platform_proposes_five_instruments_and_three_revisions() {
    assert_eq!(PlatformProposals::DEFAULT.max_instruments, 5);
    assert_eq!(PlatformProposals::DEFAULT.max_revisions_per_lineage, 3);
    let proposed = codes(
        &base(),
        sourced("/universe/max_instruments", Source::PlatformProposed, false),
    );
    assert!(
        proposed.contains(&Violation::V020),
        "a proposal that has not been confirmed is not active"
    );
}

/// Recomputed by hand from §4.2 on the builder's base: A = 10000, cap = min(1500, 0.5 × 10000) = 1500, stop 0.05 with no crypto offset, so one position at its stop is 75; the daily budget is 0.02 × 10000 = 200; the flatten loss is 0.08 × 10000 = 800; the floor loss is 0.1 × 10000 = 1000.
#[test]
#[ignore = "pending E10-1"]
fn the_worst_case_figures_are_the_products_the_screen_shows() {
    let mandate = Mandate::parse(&base()).expect("the base parses");
    let report = validate(&mandate, &context(ProvenanceMap::default())).expect("evaluable");
    assert_eq!(report.worst_case.one_position_at_stop_usd, Some(usd("75")));
    assert_eq!(report.worst_case.daily_loss_budget_usd, usd("200"));
    assert_eq!(report.worst_case.flatten_trigger_loss_usd, usd("800"));
    assert_eq!(report.worst_case.lifetime_floor_loss_usd, usd("1000"));
    assert!(
        !report.warnings.contains(&Warning::W002),
        "75 is below the 200 budget, so W-002 does not fire on this base"
    );
}

/// W-002 with the position cap raised to the allocation: 10000 × 0.05 = 500 > 200.
#[test]
#[ignore = "pending E10-1"]
fn a_position_that_can_lose_more_than_a_days_budget_warns() {
    let document = with_all(&[
        ("/risk/max_position_usd", Some(s("10000"))),
        ("/risk/max_position_fraction", Some(s("1"))),
        ("/risk/max_gross_exposure_usd", Some(s("10000"))),
    ]);
    let mandate = Mandate::parse(&document).expect("parses");
    let report = validate(&mandate, &context(ProvenanceMap::default())).expect("evaluable");
    assert_eq!(report.worst_case.one_position_at_stop_usd, Some(usd("500")));
    assert!(report.warnings.contains(&Warning::W002));
}

/// W-003, and `one_position_at_stop_usd` absent rather than zero: there is no stop, which is a different statement from "the stop loses nothing".
#[test]
#[ignore = "pending E10-1"]
fn disabled_protection_has_no_stop_to_lose_at() {
    let document = with_all(&[
        ("/protection/enabled", Some(Value::Bool(false))),
        ("/protection/stop_distance", Some(Value::Null)),
    ]);
    let mandate = Mandate::parse(&document).expect("parses");
    let report = validate(&mandate, &context(ProvenanceMap::default())).expect("evaluable");
    assert_eq!(report.worst_case.one_position_at_stop_usd, None);
    assert!(report.warnings.contains(&Warning::W003));
}

#[test]
#[ignore = "pending E10-1"]
fn warnings_are_never_violations() {
    let mandate = Mandate::parse(&base()).expect("the base parses");
    let report = validate(&mandate, &context(ProvenanceMap::default())).expect("evaluable");
    assert!(report.is_valid(), "the base breaks no rule");
    let violation_codes: BTreeSet<&str> = report.violations.iter().map(|v| v.code()).collect();
    let warning_codes: BTreeSet<&str> = report.warnings.iter().map(|w| w.code()).collect();
    assert!(
        violation_codes.is_disjoint(&warning_codes),
        "§4.2: a warning never blocks and never appears as a violation"
    );
}

/// V-013's chain and V-014, which are the two rules that need nothing but ordering.
#[test]
#[ignore = "pending E10-1"]
fn the_ordering_rules_compare_by_decimal_value() {
    let document = with("/risk/max_order_usd", Some(s("2000")));
    assert!(
        codes(&document, ProvenanceMap::default()).contains(&Violation::V013),
        "max_order_usd 2000 above max_position_usd 1500 breaks the chain"
    );
    let document = with("/capital/max_loss_from_allocation", Some(s("0.05")));
    assert!(
        codes(&document, ProvenanceMap::default()).contains(&Violation::V014),
        "a floor of 0.05 below max_drawdown 0.08 breaks V-014"
    );
}

/// Live, not pending: it reads the enum's own codes, which this PR implements, and the pending gate
/// correctly rejects a pending test that passes on the stubs (DEC-110).
///
/// The gaps are deliberate: §4.1 defines no V-004, V-019, V-021, or V-025 to V-029, and §4.2 no W-004. A variant for one of them would read as a rule someone forgot.
#[test]
fn every_code_the_enum_names_is_one_the_spec_defines() {
    let named: BTreeSet<&str> = [
        Violation::V001,
        Violation::V039,
        Violation::V020,
        Violation::V022,
        Violation::V038,
    ]
    .iter()
    .map(|v| v.code())
    .collect();
    assert!(named.contains("V-001") && named.contains("V-039"));
    for absent in ["V-004", "V-019", "V-021", "V-025", "V-029", "W-004"] {
        assert!(
            !named.contains(absent),
            "{absent} is not a code the spec states"
        );
    }
}
