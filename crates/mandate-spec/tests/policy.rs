//! The policy hierarchy (`kind: policy`, MC-P01 to MC-P22; §4.3).
//!
//! The cases carry the violations. These carry the two things §4.3 is easiest to get backwards: that
//! the reported ancestor is the **nearest** one, and that an unset mandate value constrains nothing —
//! except for the two keys where absence is itself the violation.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{base, s, with, with_all};
use mandate_spec::Mandate;
use mandate_spec::policy::{
    KeyKind, LevelName, PolicyKey, PolicyLevel, PolicyValue, check, internal_research_profile,
    platform_base, retail_profile,
};

fn level(name: LevelName, values: Vec<(PolicyKey, PolicyValue)>) -> PolicyLevel {
    PolicyLevel {
        name,
        values: values.into_iter().collect(),
    }
}

fn dec(text: &str) -> PolicyValue {
    PolicyValue::Decimal(
        mandate_spec::SchemaDec::parse(text, mandate_spec::DecGrammar::PositiveDecimal)
            .or_else(|_| {
                mandate_spec::SchemaDec::parse(text, mandate_spec::DecGrammar::OpenFraction)
            })
            .expect("a policy decimal"),
    )
}

/// MC-P01: the organization allows 0.1 and the workspace 0.08, and the mandate asks for 0.09. Both are ancestors; the workspace is the one that binds, and the one an author has to argue with.
#[test]
#[ignore = "pending E10-1"]
fn a_violation_names_the_nearest_broken_ancestor() {
    let document = with_all(&[
        ("/risk/max_drawdown", Some(s("0.09"))),
        ("/risk/drawdown_ladder/2/at", Some(s("0.09"))),
    ]);
    let mandate = Mandate::parse(&document).expect("parses");
    let chain = [
        platform_base().expect("the platform base"),
        level(
            LevelName::Organization,
            vec![(PolicyKey::MaxDrawdown, dec("0.1"))],
        ),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::MaxDrawdown, dec("0.08"))],
        ),
    ];
    let result = check(&mandate, &chain).expect("evaluable");
    let violation = result
        .violations
        .first()
        .expect("0.09 is above the workspace's 0.08");
    assert_eq!(violation.key, PolicyKey::MaxDrawdown);
    assert_eq!(violation.level, LevelName::Mandate);
    assert_eq!(
        violation.limit_level,
        LevelName::Workspace,
        "the nearest broken ancestor, not the outermost"
    );
}

/// A stricter mandate value can only stay conformant.
#[test]
#[ignore = "pending E10-1"]
fn tightening_a_key_never_creates_a_violation() {
    let mandate = Mandate::parse(&base()).expect("the base parses");
    let chain = [platform_base().expect("the platform base")];
    let baseline = check(&mandate, &chain).expect("evaluable");
    assert!(
        baseline.violations.is_empty(),
        "the base conforms to the platform base"
    );
    for (path, tighter) in [
        ("/risk/max_drawdown", "0.04"),
        ("/capital/max_loss_from_allocation", "0.05"),
        ("/risk/max_daily_loss", "0.01"),
    ] {
        let document = with(path, Some(s(tighter)));
        if let Ok(tightened) = Mandate::parse(&document) {
            let result = check(&tightened, &chain).expect("evaluable");
            assert!(
                result.violations.is_empty(),
                "tightening `{path}` to {tighter} must not create a violation"
            );
        }
    }
}

/// A maximum binds from above, a minimum from below.
///
/// A permission may be true in a child only if every ancestor is true.
///
/// A requirement, once true, is true in every child.
///
/// A set is a subset.
#[test]
#[ignore = "pending E10-1"]
fn each_key_kind_compares_in_its_own_direction() {
    let mandate = Mandate::parse(&base()).expect("the base parses");
    let too_loose_max = [
        platform_base().expect("base"),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::MaxOrderUsd, dec("500"))],
        ),
    ];
    assert!(
        !check(&mandate, &too_loose_max)
            .expect("evaluable")
            .violations
            .is_empty(),
        "max_order_usd 1000 is above a 500 ceiling"
    );
    let too_low_min = [
        platform_base().expect("base"),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::EntryThreshold, dec("0.5"))],
        ),
    ];
    assert!(
        !check(&mandate, &too_low_min)
            .expect("evaluable")
            .violations
            .is_empty(),
        "entry_threshold 0.3 is below a 0.5 floor"
    );
    let forbidden = [
        platform_base().expect("base"),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::AutoAllowed, PolicyValue::Flag(false))],
        ),
    ];
    let with_auto = Mandate::parse(&with("/autonomy/default", Some(s("auto")))).expect("parses");
    assert!(
        !check(&with_auto, &forbidden)
            .expect("evaluable")
            .violations
            .is_empty()
    );
    let required = [
        platform_base().expect("base"),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::ProtectionRequired, PolicyValue::Flag(true))],
        ),
    ];
    assert!(
        check(&mandate, &required)
            .expect("evaluable")
            .violations
            .is_empty(),
        "the base has protection on"
    );
    let classes = [
        platform_base().expect("base"),
        level(
            LevelName::Workspace,
            vec![(
                PolicyKey::AssetClasses,
                PolicyValue::Set(BTreeSet::from(["crypto".to_owned()])),
            )],
        ),
    ];
    assert!(
        !check(&mandate, &classes)
            .expect("evaluable")
            .violations
            .is_empty(),
        "us_equity is not in a crypto-only set"
    );
}

/// The asymmetry of §4.3: `two_approver_above_usd` and `stop_distance_max` are the two keys where an unset mandate value is *itself* the violation, because "no limit" is looser than any limit.
///
/// Every other key: a level that states nothing constrains nothing.
#[test]
#[ignore = "pending E10-1"]
fn absence_constrains_nothing_except_where_no_limit_is_looser_than_any_limit() {
    for key in [PolicyKey::TwoApproverAboveUsd, PolicyKey::StopDistanceMax] {
        assert!(
            key.absence_violates(),
            "`{}` must treat absence as a violation",
            key.as_str()
        );
    }
    let mandate = Mandate::parse(&base()).expect("the base parses");
    let chain = [
        platform_base().expect("base"),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::TwoApproverAboveUsd, dec("5000"))],
        ),
    ];
    let result = check(&mandate, &chain).expect("evaluable");
    assert!(
        result
            .violations
            .iter()
            .any(|v| v.key == PolicyKey::TwoApproverAboveUsd),
        "the base leaves two_approver_above_usd null, which §4.3 makes the violation"
    );
    let silent = [
        platform_base().expect("base"),
        level(LevelName::Workspace, vec![]),
    ];
    assert!(
        check(&mandate, &silent)
            .expect("evaluable")
            .violations
            .is_empty()
    );
}

/// §4.3's runtime overlay: when an ancestor turns `auto_allowed` off, every `auto` evaluates as `ask` at the next evaluation. That is a tightening the order path applies on its own.
#[test]
#[ignore = "pending E10-1"]
fn the_overlay_narrows_auto_to_ask_without_a_new_version() {
    let mandate = Mandate::parse(&base()).expect("the base parses");
    let forbidden = [
        platform_base().expect("base"),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::AutoAllowed, PolicyValue::Flag(false))],
        ),
    ];
    let result = check(&mandate, &forbidden).expect("evaluable");
    assert!(!result.overlay.auto_allowed());
    assert_eq!(
        result
            .overlay
            .narrow(mandate_domain::AutonomyDecision::Auto),
        mandate_domain::AutonomyDecision::Ask
    );
    assert_eq!(
        result
            .overlay
            .narrow(mandate_domain::AutonomyDecision::Deny),
        mandate_domain::AutonomyDecision::Deny,
        "and it never loosens a deny"
    );
}

#[test]
#[ignore = "pending E10-1"]
fn the_platform_base_and_the_two_profiles_carry_the_values_the_spec_states() {
    let base_level = platform_base().expect("the platform base");
    assert_eq!(base_level.name, LevelName::Platform);
    assert_eq!(
        base_level.values.get(&PolicyKey::BreachConfirmS),
        Some(&PolicyValue::Integer(300))
    );
    assert_eq!(
        base_level.values.get(&PolicyKey::MaxInstruments),
        Some(&PolicyValue::Integer(20)),
        "DEC-117's ceiling"
    );
    assert_eq!(
        base_level.values.get(&PolicyKey::StaggerWindowS),
        Some(&PolicyValue::Integer(900)),
        "DEC-123's minimum"
    );
    let retail = retail_profile().expect("the retail profile");
    assert_eq!(
        retail.values.get(&PolicyKey::ResearchAgentAllowed),
        Some(&PolicyValue::Flag(false)),
        "DEC-98: off until the DEC-99 evaluation passes"
    );
    assert_eq!(
        retail.values.get(&PolicyKey::Environments),
        Some(&PolicyValue::Set(BTreeSet::from(["paper".to_owned()]))),
        "no live trading for any user until counsel signs off"
    );
    let internal = internal_research_profile().expect("the internal profile");
    assert_eq!(
        internal.values.get(&PolicyKey::AdmissionAutoAllowed),
        Some(&PolicyValue::Flag(false)),
        "DEC-103: every admission is ask on the thin slice"
    );
    assert_eq!(
        internal.values.get(&PolicyKey::MaxRevisionsPerLineage),
        Some(&PolicyValue::Integer(3))
    );
}

/// Live: the kinds are a total map over the key enum, which this PR implements.
#[test]
fn every_key_has_exactly_one_kind() {
    let keys = [
        PolicyKey::AllocationUsd,
        PolicyKey::EntryThreshold,
        PolicyKey::AutoAllowed,
        PolicyKey::ProtectionRequired,
        PolicyKey::AssetClasses,
    ];
    let kinds: Vec<KeyKind> = keys.iter().map(|k| k.kind()).collect();
    assert_eq!(
        kinds,
        vec![
            KeyKind::Maximum,
            KeyKind::Minimum,
            KeyKind::Permission,
            KeyKind::Requirement,
            KeyKind::Set
        ]
    );
    let names: BTreeSet<&str> = keys.iter().map(|k| k.as_str()).collect();
    assert_eq!(names.len(), keys.len(), "one name per key");
    let mut counts: BTreeMap<KeyKind, usize> = BTreeMap::new();
    for key in keys {
        *counts.entry(key.kind()).or_default() += 1;
    }
    assert_eq!(counts.len(), 5, "all five kinds of §4.3 are reachable");
}
