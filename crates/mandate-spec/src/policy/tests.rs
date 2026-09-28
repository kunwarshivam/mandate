//! Each key's mandate value, each kind in both directions and at equality, the overlay's fold, and
//! the reading of `Absent` at a level and in a mandate: what the six `tests/policy.rs` tests and the 22
//! MC-P cases do not pin one by one, so a rule weakened anywhere fails by name.

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::{AutonomyDecision, Purpose};

use super::{
    AddingPurpose, LevelName, PolicyKey, PolicyLevel, PolicyOverlay, PolicyValue, check,
    internal_research_profile, platform_base, retail_profile, values_of,
};
use crate::validate::tests::{context, mandate};
use crate::validate::{Rejected, ValidatedMandate};
use crate::{DecGrammar, SchemaDec, SpecError};

type Checked = Result<(), String>;
/// A named patch of the base and the one key's value it must give.
type KeyRow<'a> = (&'a str, Vec<(&'a str, &'a str)>, PolicyKey, PolicyValue);

fn dec(text: &str) -> Result<PolicyValue, String> {
    SchemaDec::parse(text, DecGrammar::Decimal)
        .map(PolicyValue::Decimal)
        .map_err(|e| format!("{text}: {e}"))
}

fn names(items: &[&str]) -> PolicyValue {
    PolicyValue::Set(items.iter().map(|item| (*item).to_owned()).collect())
}

fn level(name: LevelName, values: Vec<(PolicyKey, PolicyValue)>) -> PolicyLevel {
    PolicyLevel {
        name,
        values: values.into_iter().collect(),
    }
}

fn overlay(levels: &[PolicyLevel]) -> Result<PolicyOverlay, String> {
    check(&mandate(&[])?, levels)
        .map(|result| result.overlay)
        .map_err(|e| e.to_string())
}

/// Every key of the base mandate, stated by hand from the document.
#[test]
fn the_base_states_every_key_it_has_and_nothing_else() -> Checked {
    let got = values_of(&mandate(&[])?).map_err(|e| e.to_string())?;
    let int = PolicyValue::Integer;
    let flag = PolicyValue::Flag;
    let expected = BTreeMap::from([
        (PolicyKey::AllocationUsd, dec("10000")?),
        (PolicyKey::MaxLossFromAllocation, dec("0.1")?),
        (PolicyKey::MaxPositionUsd, dec("1500")?),
        (PolicyKey::MaxPositionFraction, dec("0.5")?),
        (PolicyKey::MaxGrossExposureUsd, dec("3000")?),
        (PolicyKey::MaxOrderUsd, dec("1000")?),
        (PolicyKey::MaxOrdersPerDay, int(20)),
        (PolicyKey::MaxDailyLoss, dec("0.02")?),
        (PolicyKey::MaxDrawdown, dec("0.08")?),
        (PolicyKey::BreachConfirmS, int(60)),
        (PolicyKey::MaxOutputAgeS, int(1800)),
        (PolicyKey::ExitThreshold, dec("0.3")?),
        (PolicyKey::StopDistanceMax, dec("0.05")?),
        (PolicyKey::ExitsOnlyAtMax, dec("0.05")?),
        (PolicyKey::TwoApproverAboveUsd, PolicyValue::Absent),
        (PolicyKey::MaxInstruments, int(2)),
        (PolicyKey::ResearchWeight, PolicyValue::Absent),
        (PolicyKey::ResearchCostCapUsdPerDay, PolicyValue::Absent),
        (PolicyKey::MaxRevisionsPerLineage, PolicyValue::Absent),
        (PolicyKey::EntryThreshold, dec("0.3")?),
        (PolicyKey::RebalanceBand, dec("0.05")?),
        (PolicyKey::Hysteresis, dec("0.01")?),
        (PolicyKey::CadenceIntervalS, int(900)),
        (PolicyKey::ApprovalTimeoutS, int(600)),
        (PolicyKey::ReentryCooldownS, int(3600)),
        (PolicyKey::DailyBreachMinS, int(3600)),
        (PolicyKey::ScaleLiftAfterS, int(600)),
        (PolicyKey::ResearchIntervalS, PolicyValue::Absent),
        (PolicyKey::LeveragedEtpsAllowed, flag(false)),
        (PolicyKey::AutoAllowed, flag(false)),
        (PolicyKey::ResearchAgentAllowed, flag(false)),
        (PolicyKey::AdmissionAutoAllowed, flag(false)),
        (PolicyKey::ProtectionRequired, flag(true)),
        (PolicyKey::AssetClasses, names(&["us_equity"])),
        (PolicyKey::SignalModelTypes, names(&["quant"])),
        (PolicyKey::GoalTypes, names(&["continuous"])),
        (PolicyKey::Channels, names(&["email", "phone"])),
        (PolicyKey::Environments, names(&["paper"])),
    ]);
    if got != expected {
        return Err(format!("expected {expected:?}, got {got:?}"));
    }
    Ok(())
}

/// The keys that read more than one field: `auto` anywhere, the admitting model, the first rung that
/// is not `scale_sizes`, the oldest output age, every channel and model type, and the environment.
#[test]
fn the_derived_keys_read_what_section_four_three_names() -> Checked {
    let research =
        r#"{"interval_s": 3600, "cost_cap_usd_per_day": "5", "max_revisions_per_lineage": 3}"#;
    let rule_auto = r#"{"id": "small", "when": {"field": "order_usd", "op": "lt", "value": "10"}, "then": "auto"}"#;
    let second = r#"{"id": "quant.carry", "version": "1.0.0",
      "content_hash": "sha256:2222222222222222222222222222222222222222222222222222222222222222",
      "params": [], "weight": "1", "max_output_age_s": 3600, "admits_instruments": false}"#;
    let rows: Vec<KeyRow<'_>> = vec![
        (
            "auto as the default",
            vec![("/autonomy/default", r#""auto""#)],
            PolicyKey::AutoAllowed,
            PolicyValue::Flag(true),
        ),
        (
            "auto in a rule",
            vec![("/autonomy/rules/-", rule_auto)],
            PolicyKey::AutoAllowed,
            PolicyValue::Flag(true),
        ),
        (
            "auto only in admission is not auto_allowed",
            vec![("/autonomy/admission", r#""auto""#)],
            PolicyKey::AutoAllowed,
            PolicyValue::Flag(false),
        ),
        (
            "admission auto",
            vec![("/autonomy/admission", r#""auto""#)],
            PolicyKey::AdmissionAutoAllowed,
            PolicyValue::Flag(true),
        ),
        (
            "the admitting model's weight",
            vec![
                ("/behavior/signal_models/0/id", r#""llm.research""#),
                ("/behavior/signal_models/0/admits_instruments", "true"),
                ("/behavior/signal_models/0/weight", r#""0.4""#),
            ],
            PolicyKey::ResearchWeight,
            dec("0.4")?,
        ),
        (
            "an admitting model is a research agent",
            vec![("/behavior/signal_models/0/admits_instruments", "true")],
            PolicyKey::ResearchAgentAllowed,
            PolicyValue::Flag(true),
        ),
        (
            "the research envelope's interval",
            vec![("/behavior/research", research)],
            PolicyKey::ResearchIntervalS,
            PolicyValue::Integer(3600),
        ),
        (
            "the research envelope's cost cap",
            vec![("/behavior/research", research)],
            PolicyKey::ResearchCostCapUsdPerDay,
            dec("5")?,
        ),
        (
            "the research envelope's revisions",
            vec![("/behavior/research", research)],
            PolicyKey::MaxRevisionsPerLineage,
            PolicyValue::Integer(3),
        ),
        (
            "the first rung that is not scale_sizes, by action",
            vec![
                ("/risk/drawdown_ladder/1/action", r#""scale_sizes""#),
                ("/risk/drawdown_ladder/1/factor", r#""0.25""#),
            ],
            PolicyKey::ExitsOnlyAtMax,
            dec("0.08")?,
        ),
        (
            "the oldest output age over every model",
            vec![("/behavior/signal_models/-", second)],
            PolicyKey::MaxOutputAgeS,
            PolicyValue::Integer(3600),
        ),
        (
            "every model type",
            vec![("/behavior/signal_models/-", second)],
            PolicyKey::SignalModelTypes,
            names(&["quant"]),
        ),
        (
            "the environment as the schema spells it",
            vec![("/environment", r#""live""#)],
            PolicyKey::Environments,
            names(&["live"]),
        ),
        (
            "every channel as the schema spells it",
            vec![(
                "/notifications/channels",
                r#"["email", "phone", "slack", "sms", "telegram", "web_push"]"#,
            )],
            PolicyKey::Channels,
            names(&["email", "phone", "slack", "sms", "telegram", "web_push"]),
        ),
        (
            "a stated two-approver threshold",
            vec![("/autonomy/approval/two_approver_above_usd", r#""700""#)],
            PolicyKey::TwoApproverAboveUsd,
            dec("700")?,
        ),
        (
            "leveraged ETPs",
            vec![("/universe/leveraged_etps_enabled", "true")],
            PolicyKey::LeveragedEtpsAllowed,
            PolicyValue::Flag(true),
        ),
        (
            "protection off",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/stop_distance", "null"),
                ("/protection/take_profit_distance", "null"),
            ],
            PolicyKey::ProtectionRequired,
            PolicyValue::Flag(false),
        ),
        (
            "no stop is absent",
            vec![
                ("/protection/enabled", "false"),
                ("/protection/stop_distance", "null"),
                ("/protection/take_profit_distance", "null"),
            ],
            PolicyKey::StopDistanceMax,
            PolicyValue::Absent,
        ),
    ];
    for (name, patches, key, expected) in rows {
        let values = values_of(&mandate(&patches)?).map_err(|e| e.to_string())?;
        if values.get(&key) != Some(&expected) {
            return Err(format!(
                "{name}: expected {expected:?}, got {:?}",
                values.get(&key)
            ));
        }
    }
    Ok(())
}

/// One key of each kind, the child above, equal to, and below its parent; and a type that does not
/// fit the key refused.
#[test]
fn each_kind_breaks_in_its_own_direction_and_never_at_equality() -> Checked {
    let flag = PolicyValue::Flag;
    let rows: Vec<(PolicyKey, PolicyValue, PolicyValue, bool)> = vec![
        (PolicyKey::MaxDrawdown, dec("0.09")?, dec("0.08")?, true),
        (PolicyKey::MaxDrawdown, dec("0.08")?, dec("0.08")?, false),
        (PolicyKey::MaxDrawdown, dec("0.07")?, dec("0.08")?, false),
        (
            PolicyKey::MaxOrdersPerDay,
            PolicyValue::Integer(21),
            PolicyValue::Integer(20),
            true,
        ),
        (
            PolicyKey::MaxOrdersPerDay,
            PolicyValue::Integer(20),
            dec("20")?,
            false,
        ),
        (PolicyKey::EntryThreshold, dec("0.3")?, dec("0.4")?, true),
        (PolicyKey::EntryThreshold, dec("0.4")?, dec("0.4")?, false),
        (PolicyKey::EntryThreshold, dec("0.5")?, dec("0.4")?, false),
        (PolicyKey::AutoAllowed, flag(true), flag(false), true),
        (PolicyKey::AutoAllowed, flag(true), flag(true), false),
        (PolicyKey::AutoAllowed, flag(false), flag(false), false),
        (PolicyKey::AutoAllowed, flag(false), flag(true), false),
        (PolicyKey::ProtectionRequired, flag(false), flag(true), true),
        (PolicyKey::ProtectionRequired, flag(true), flag(true), false),
        (
            PolicyKey::ProtectionRequired,
            flag(false),
            flag(false),
            false,
        ),
        (
            PolicyKey::ProtectionRequired,
            flag(true),
            flag(false),
            false,
        ),
        (
            PolicyKey::AssetClasses,
            names(&["crypto", "us_equity"]),
            names(&["us_equity"]),
            true,
        ),
        (
            PolicyKey::AssetClasses,
            names(&["us_equity"]),
            names(&["crypto", "us_equity"]),
            false,
        ),
        (
            PolicyKey::AssetClasses,
            names(&["us_equity"]),
            names(&["us_equity"]),
            false,
        ),
        (
            PolicyKey::TwoApproverAboveUsd,
            PolicyValue::Absent,
            dec("5000")?,
            true,
        ),
        (
            PolicyKey::MaxDrawdown,
            PolicyValue::Absent,
            dec("0.08")?,
            false,
        ),
    ];
    for (key, child, parent, breaks) in rows {
        let got = super::violates(key, &child, &parent).map_err(|e| e.to_string())?;
        if got != breaks {
            return Err(format!(
                "{} {child:?} against {parent:?}: expected {breaks}",
                key.as_str()
            ));
        }
    }
    for (key, child, parent) in [
        (PolicyKey::MaxDrawdown, flag(true), dec("0.08")?),
        (PolicyKey::MaxDrawdown, dec("0.08")?, names(&["x"])),
        (PolicyKey::AutoAllowed, dec("1")?, flag(true)),
        (PolicyKey::AutoAllowed, flag(true), dec("1")?),
        (PolicyKey::ProtectionRequired, names(&["x"]), flag(true)),
        (PolicyKey::ProtectionRequired, flag(true), names(&["x"])),
        (PolicyKey::AssetClasses, flag(true), names(&["x"])),
        (PolicyKey::AssetClasses, names(&["x"]), flag(true)),
    ] {
        match super::violates(key, &child, &parent) {
            Err(SpecError::InvalidInput { .. }) => {}
            other => {
                return Err(format!(
                    "{} {child:?} against {parent:?}: expected invalid_input, got {other:?}",
                    key.as_str()
                ));
            }
        }
    }
    Ok(())
}

/// The overlay is the tightest stated value per key over the levels, and the mandate's value only
/// where no level states one.
#[test]
fn the_overlay_is_the_tightest_stated_value_and_governs_the_mandate() -> Checked {
    let flag = PolicyValue::Flag;
    let folded = overlay(&[
        level(
            LevelName::Platform,
            vec![
                (PolicyKey::MaxDrawdown, dec("0.1")?),
                (PolicyKey::EntryThreshold, dec("0.2")?),
                (PolicyKey::AutoAllowed, flag(true)),
                (PolicyKey::ProtectionRequired, flag(true)),
                (PolicyKey::AssetClasses, names(&["crypto", "us_equity"])),
                (PolicyKey::MaxInstruments, PolicyValue::Absent),
            ],
        ),
        level(
            LevelName::Organization,
            vec![
                (PolicyKey::MaxDrawdown, dec("0.08")?),
                (PolicyKey::EntryThreshold, dec("0.4")?),
                (PolicyKey::AutoAllowed, flag(false)),
                (PolicyKey::ProtectionRequired, flag(false)),
                (PolicyKey::AssetClasses, names(&["us_equity"])),
            ],
        ),
        level(
            LevelName::Workspace,
            vec![
                (PolicyKey::MaxDrawdown, dec("0.09")?),
                (PolicyKey::EntryThreshold, dec("0.3")?),
                (PolicyKey::AutoAllowed, flag(true)),
            ],
        ),
    ])?;
    let expected = BTreeMap::from([
        (PolicyKey::MaxDrawdown, dec("0.08")?),
        (PolicyKey::EntryThreshold, dec("0.4")?),
        (PolicyKey::AutoAllowed, flag(false)),
        (PolicyKey::ProtectionRequired, flag(true)),
        (PolicyKey::AssetClasses, names(&["us_equity"])),
    ]);
    if folded.tightest() != &expected {
        return Err(format!(
            "expected {expected:?}, got {:?}",
            folded.tightest()
        ));
    }
    if folded.auto_allowed() {
        return Err("an ancestor forbids auto".to_owned());
    }
    let effective = |key: PolicyKey, value: PolicyValue| {
        folded
            .effective(key, &value)
            .map_err(|e| format!("{}: {e}", key.as_str()))
    };
    let rows: Vec<(PolicyKey, PolicyValue, PolicyValue)> = vec![
        (PolicyKey::MaxDrawdown, dec("0.05")?, dec("0.05")?),
        (PolicyKey::MaxDrawdown, dec("0.2")?, dec("0.08")?),
        (PolicyKey::MaxDrawdown, PolicyValue::Absent, dec("0.08")?),
        (PolicyKey::EntryThreshold, dec("0.3")?, dec("0.4")?),
        (PolicyKey::EntryThreshold, dec("0.6")?, dec("0.6")?),
        (PolicyKey::AutoAllowed, flag(true), flag(false)),
        (PolicyKey::ProtectionRequired, flag(false), flag(true)),
        (
            PolicyKey::AssetClasses,
            names(&["crypto", "us_equity"]),
            names(&["us_equity"]),
        ),
        (PolicyKey::MaxOrderUsd, dec("1000")?, dec("1000")?),
        (
            PolicyKey::MaxOrderUsd,
            PolicyValue::Absent,
            PolicyValue::Absent,
        ),
    ];
    for (key, value, wanted) in rows {
        let got = effective(key, value.clone())?;
        if got != wanted {
            return Err(format!(
                "{} with {value:?}: expected {wanted:?}, got {got:?}",
                key.as_str()
            ));
        }
    }
    if effective(PolicyKey::MaxDrawdown, flag(true)).is_ok() {
        return Err("a flag is not a drawdown".to_owned());
    }
    Ok(())
}

#[test]
fn auto_is_allowed_unless_a_level_forbids_it() -> Checked {
    let flag = PolicyValue::Flag;
    for (levels, allowed) in [
        (vec![], true),
        (
            vec![level(
                LevelName::Platform,
                vec![(PolicyKey::AutoAllowed, flag(true))],
            )],
            true,
        ),
        (
            vec![level(
                LevelName::Platform,
                vec![(PolicyKey::AutoAllowed, flag(false))],
            )],
            false,
        ),
    ] {
        let folded = overlay(&levels)?;
        if folded.auto_allowed() != allowed {
            return Err(format!("{levels:?}: expected auto_allowed {allowed}"));
        }
        for adds in [AddingPurpose::Open, AddingPurpose::Increase] {
            let auto = if allowed {
                AutonomyDecision::Auto
            } else {
                AutonomyDecision::Ask
            };
            for (given, wanted) in [
                (AutonomyDecision::Auto, auto),
                (AutonomyDecision::Ask, AutonomyDecision::Ask),
                (AutonomyDecision::Deny, AutonomyDecision::Deny),
            ] {
                let narrowed = folded.narrow(adds, given);
                if narrowed != wanted {
                    return Err(format!(
                        "{levels:?}, {adds:?}: {given:?} narrowed to {narrowed:?}, not {wanted:?}"
                    ));
                }
            }
        }
    }
    Ok(())
}

/// A level's `Absent` states nothing, as a child and as an ancestor; a level is checked against the
/// levels above it only; each broken key is reported once per level, naming the nearest ancestor.
#[test]
fn the_chain_reads_every_level_against_the_levels_above_it() -> Checked {
    let m = mandate(&[])?;
    let chain = [
        level(
            LevelName::Platform,
            vec![
                (PolicyKey::MaxDrawdown, dec("0.1")?),
                (PolicyKey::MaxOrderUsd, PolicyValue::Absent),
            ],
        ),
        level(
            LevelName::Organization,
            vec![
                (PolicyKey::MaxDrawdown, dec("0.12")?),
                (PolicyKey::MaxOrderUsd, dec("500")?),
                (PolicyKey::MaxDailyLoss, PolicyValue::Absent),
            ],
        ),
        level(
            LevelName::Workspace,
            vec![
                (PolicyKey::MaxDrawdown, dec("0.07")?),
                (PolicyKey::MaxOrderUsd, PolicyValue::Absent),
            ],
        ),
    ];
    let result = check(&m, &chain).map_err(|e| e.to_string())?;
    let got: BTreeSet<(PolicyKey, LevelName, LevelName)> = result
        .violations
        .iter()
        .map(|v| (v.key, v.level, v.limit_level))
        .collect();
    let expected = BTreeSet::from([
        (
            PolicyKey::MaxDrawdown,
            LevelName::Organization,
            LevelName::Platform,
        ),
        (
            PolicyKey::MaxDrawdown,
            LevelName::Mandate,
            LevelName::Workspace,
        ),
        (
            PolicyKey::MaxOrderUsd,
            LevelName::Mandate,
            LevelName::Organization,
        ),
    ]);
    if got != expected || result.violations.len() != expected.len() {
        return Err(format!(
            "expected {expected:?}, got {:?}",
            result.violations
        ));
    }
    if result.overlay.tightest().get(&PolicyKey::MaxOrderUsd) != Some(&dec("500")?) {
        return Err("a level's Absent must not replace an ancestor's value".to_owned());
    }
    let approvals = [
        level(
            LevelName::Organization,
            vec![(PolicyKey::TwoApproverAboveUsd, dec("5000")?)],
        ),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::TwoApproverAboveUsd, PolicyValue::Absent)],
        ),
    ];
    let result = check(&m, &approvals).map_err(|e| e.to_string())?;
    let got: Vec<(PolicyKey, LevelName, PolicyValue, LevelName)> = result
        .violations
        .iter()
        .map(|v| (v.key, v.level, v.value.clone(), v.limit_level))
        .collect();
    let wanted = vec![(
        PolicyKey::TwoApproverAboveUsd,
        LevelName::Mandate,
        PolicyValue::Absent,
        LevelName::Organization,
    )];
    if got != wanted {
        return Err(format!(
            "the mandate's null two_approver_above_usd breaks the organization's, and the \
             workspace's Absent states nothing: expected {wanted:?}, got {got:?}"
        ));
    }
    Ok(())
}

/// The one constructor: a mandate that breaks a rule, or a policy, is not validated; one that breaks
/// neither is, and is the mandate it was given.
#[test]
fn a_validated_mandate_breaks_no_rule_and_no_policy() -> Checked {
    let ctx = context()?;
    let clean = mandate(&[])?;
    let validated = ValidatedMandate::new(clean.clone(), &ctx, &[]).map_err(|e| e.to_string())?;
    if validated.mandate() != &clean {
        return Err("the validated mandate is not the one given".to_owned());
    }
    let ceiling = [level(
        LevelName::Workspace,
        vec![(PolicyKey::MaxOrderUsd, dec("500")?)],
    )];
    match ValidatedMandate::new(clean, &ctx, &ceiling) {
        Err(Rejected::Rules { report, policy }) if report.is_valid() && policy.len() == 1 => {}
        other => return Err(format!("a policy breach must reject, got {other:?}")),
    }
    let broken = mandate(&[("/risk/max_order_usd", r#""2000""#)])?;
    match ValidatedMandate::new(broken, &ctx, &[]) {
        Err(Rejected::Rules { report, policy }) if !report.is_valid() && policy.is_empty() => {}
        other => return Err(format!("a V-rule breach must reject, got {other:?}")),
    }
    Ok(())
}

/// Broken by both ancestors, the violation names the nearer one, the workspace (MC-P03's reading).
#[test]
fn both_ancestors_broken_names_the_nearer() -> Checked {
    let chain = [
        level(
            LevelName::Organization,
            vec![(PolicyKey::MaxDrawdown, dec("0.06")?)],
        ),
        level(
            LevelName::Workspace,
            vec![(PolicyKey::MaxDrawdown, dec("0.05")?)],
        ),
    ];
    let result = check(&mandate(&[])?, &chain).map_err(|e| e.to_string())?;
    let got: Vec<(LevelName, LevelName)> = result
        .violations
        .iter()
        .map(|v| (v.level, v.limit_level))
        .collect();
    if got != vec![(LevelName::Mandate, LevelName::Workspace)] {
        return Err(format!(
            "expected the workspace alone, got {:?}",
            result.violations
        ));
    }
    Ok(())
}

/// Every value §4.3 states for the platform base and the two profiles, and nothing else.
#[test]
fn the_three_platform_levels_are_exactly_section_four_three() -> Checked {
    let flag = PolicyValue::Flag;
    let int = PolicyValue::Integer;
    let rows: Vec<(PolicyLevel, BTreeMap<PolicyKey, PolicyValue>)> = vec![
        (
            platform_base().map_err(|e| e.to_string())?,
            BTreeMap::from([
                (PolicyKey::MaxLossFromAllocation, dec("0.5")?),
                (PolicyKey::BreachConfirmS, int(300)),
                (PolicyKey::MaxInstruments, int(20)),
                (PolicyKey::StaggerWindowS, int(900)),
            ]),
        ),
        (
            retail_profile().map_err(|e| e.to_string())?,
            BTreeMap::from([
                (PolicyKey::AutoAllowed, flag(true)),
                (PolicyKey::SignalModelTypes, names(&["llm", "quant"])),
                (PolicyKey::LeveragedEtpsAllowed, flag(false)),
                (PolicyKey::ProtectionRequired, flag(true)),
                (PolicyKey::ApprovalTimeoutS, int(120)),
                (PolicyKey::MaxLossFromAllocation, dec("0.2")?),
                (PolicyKey::ResearchAgentAllowed, flag(false)),
                (PolicyKey::Environments, names(&["paper"])),
            ]),
        ),
        (
            internal_research_profile().map_err(|e| e.to_string())?,
            BTreeMap::from([
                (PolicyKey::ResearchAgentAllowed, flag(true)),
                (PolicyKey::AdmissionAutoAllowed, flag(false)),
                (PolicyKey::MaxRevisionsPerLineage, int(3)),
                (PolicyKey::Environments, names(&["paper"])),
            ]),
        ),
    ];
    for (got, values) in rows {
        if got.name != LevelName::Platform || got.values != values {
            return Err(format!("expected a platform level {values:?}, got {got:?}"));
        }
    }
    Ok(())
}

/// Only an action that adds risk can be narrowed: each of the four exit purposes §6.2 step 3 makes
/// built-in AUTO is refused by the type, and the two adding purposes map to themselves (AGENTS.md rules
/// 2 and 13, #263 round 1).
#[test]
fn no_exit_purpose_can_be_narrowed() -> Checked {
    for exit in [
        Purpose::RiskExit,
        Purpose::Protective,
        Purpose::DiscretionaryExit,
        Purpose::OwnerExit,
    ] {
        if AddingPurpose::try_from(exit) != Err(exit) {
            return Err(format!("{exit:?} must not become an AddingPurpose"));
        }
    }
    for (purpose, adds) in [
        (Purpose::Open, AddingPurpose::Open),
        (Purpose::Increase, AddingPurpose::Increase),
    ] {
        if AddingPurpose::try_from(purpose) != Ok(adds) {
            return Err(format!("{purpose:?} must be {adds:?}"));
        }
    }
    Ok(())
}

/// A chain holds only ancestors: a level named `Mandate` is refused, never compared as an ancestor
/// whose ceiling the overlay would then drop (#263 round 1, minor 4).
#[test]
fn a_mandate_level_in_the_chain_is_refused() -> Checked {
    let chain = [level(
        LevelName::Mandate,
        vec![(PolicyKey::MaxDrawdown, dec("0.01")?)],
    )];
    match check(&mandate(&[])?, &chain) {
        Err(SpecError::InvalidInput { .. }) => Ok(()),
        other => Err(format!("expected invalid_input, got {other:?}")),
    }
}

/// The platform base is always the outermost level of `ValidatedMandate::new`, so an empty chain still
/// checks it: no caller can skip policy by passing none (#263, reading 7).
#[test]
fn an_empty_chain_still_checks_the_platform_base() -> Checked {
    let ctx = context()?;
    let loose = mandate(&[("/capital/max_loss_from_allocation", r#""0.6""#)])?;
    match ValidatedMandate::new(loose, &ctx, &[]) {
        Err(Rejected::Rules { report, policy })
            if report.is_valid()
                && policy
                    .iter()
                    .map(|v| (v.key, v.limit_level))
                    .collect::<Vec<_>>()
                    == vec![(PolicyKey::MaxLossFromAllocation, LevelName::Platform)] =>
        {
            Ok(())
        }
        other => Err(format!(
            "0.6 is above the platform base's 0.5, got {other:?}"
        )),
    }
}

/// Passing the platform base again changes nothing: the fold is idempotent, so a caller that states
/// the base gets exactly what one that leaves it out gets (the coordinator's ruling on #263).
#[test]
fn stating_the_platform_base_again_changes_nothing() -> Checked {
    let ctx = context()?;
    let base = platform_base().map_err(|e| e.to_string())?;
    for patches in [
        vec![],
        vec![("/capital/max_loss_from_allocation", r#""0.6""#)],
        vec![("/risk/max_order_usd", r#""2000""#)],
    ] {
        let without = ValidatedMandate::new(mandate(&patches)?, &ctx, &[]);
        let with_base =
            ValidatedMandate::new(mandate(&patches)?, &ctx, std::slice::from_ref(&base));
        if without != with_base {
            return Err(format!(
                "{patches:?}: {without:?} differs from {with_base:?}"
            ));
        }
    }
    Ok(())
}
