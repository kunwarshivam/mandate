//! The `not_in` reading of §9.2's single-comparison shapes (DEC-172 item 9), which no `tests/change.rs`
//! test and no MC-C case reaches: the MI-11 property generates `in` lists only, so a `not_in` rule
//! whose direction were ignored would read every change as increasing and pass all of them. Also the
//! refusal of a diverged mandate (DEC-172 item 12), which `tests/` cannot build.

use core::fmt::Debug;

use mandate_domain::AutonomyDecision::{self, Ask, Auto, Deny};

use super::{
    ChangeClass, Often, changed_paths, classify, classify_autonomy, how_often, pinning_switch,
};
use crate::condition::{Condition, ConditionField, ConditionValue, Operator};
use crate::document::{Approval, ApproverRef, Autonomy, OnTimeout, Rule, RuleId};
use crate::validate::tests::mandate;
use crate::{ParseError, SpecError};

type Checked = Result<(), String>;

fn not_in(field: ConditionField, members: &[&str]) -> Condition {
    Condition::Compare {
        field,
        op: Operator::NotIn,
        value: ConditionValue::List(members.iter().map(|m| (*m).to_owned()).collect()),
    }
}

fn autonomy(when: Condition, then: AutonomyDecision) -> Result<Autonomy, String> {
    Ok(Autonomy {
        rules: vec![Rule {
            id: RuleId::parse("only").map_err(|e| e.to_string())?,
            when,
            then,
        }],
        default: Ask,
        admission: Ask,
        approval: Approval {
            timeout_s: 600,
            on_timeout: OnTimeout::Skip,
            approvers: vec![ApproverRef::parse("role:approver").map_err(|e| e.to_string())?],
            two_approver_above_usd: None,
        },
    })
}

fn class(old: &[&str], new: &[&str], then: AutonomyDecision) -> Result<ChangeClass, String> {
    let field = ConditionField::Session;
    classify_autonomy(
        &autonomy(not_in(field, old), then)?,
        &autonomy(not_in(field, new), then)?,
    )
    .map_err(|e| e.to_string())
}

/// Excluding more matches less often and excluding less matches more often: the reverse of `in`. A
/// list that neither contains nor is contained by the old one has no direction.
#[test]
fn a_not_in_list_that_grows_matches_less_often() {
    let field = ConditionField::Purpose;
    let (one, two) = (["open"], ["increase", "open"]);
    assert_eq!(
        how_often(&not_in(field, &one), &not_in(field, &two)),
        Some(Often::Less)
    );
    assert_eq!(
        how_often(&not_in(field, &two), &not_in(field, &one)),
        Some(Often::More)
    );
    assert_eq!(
        how_often(&not_in(field, &one), &not_in(field, &["increase"])),
        None
    );
}

/// An `auto` rule excluding more is reducing and excluding less is increasing; an `ask` or `deny`
/// rule excluding less, with nothing stricter after it, is reducing, and excluding more is increasing.
#[test]
fn a_not_in_rule_is_reducing_only_in_the_direction_its_then_allows() -> Checked {
    let (one, two) = (["crypto"].as_slice(), ["crypto", "regular"].as_slice());
    assert_eq!(class(one, two, Auto)?, ChangeClass::RiskReducing);
    assert_eq!(class(two, one, Auto)?, ChangeClass::RiskIncreasing);
    for then in [Ask, Deny] {
        assert_eq!(
            class(two, one, then)?,
            ChangeClass::RiskReducing,
            "{then:?}"
        );
        assert_eq!(
            class(one, two, then)?,
            ChangeClass::RiskIncreasing,
            "{then:?}"
        );
    }
    Ok(())
}

/// [`ChangeClass`]'s [`Ord`] is its severity, which `join` takes the maximum of.
#[test]
fn the_class_order_is_the_severity_order() {
    let mut classes = [
        ChangeClass::Invalid,
        ChangeClass::RiskReducing,
        ChangeClass::Neutral,
        ChangeClass::RiskIncreasing,
    ];
    classes.sort();
    assert_eq!(
        classes,
        [
            ChangeClass::Neutral,
            ChangeClass::RiskReducing,
            ChangeClass::RiskIncreasing,
            ChangeClass::Invalid
        ]
    );
}

fn diverged<T: Debug>(what: &str, result: Result<T, SpecError>) -> Checked {
    match result {
        Err(SpecError::Parse(ParseError::Diverged)) => Ok(()),
        other => Err(format!("{what}: expected diverged, got {other:?}")),
    }
}

/// A mandate whose public fields no longer match the document it was parsed from is refused by all
/// three entry points, on either side of the change, rather than classified from fields the version
/// would not hash (DEC-172 item 12). The same pair undiverged classifies, so the refusal is the
/// divergence and not the fields.
#[test]
fn a_diverged_mandate_is_refused_by_every_entry_point() -> Checked {
    let parsed = mandate(&[])?;
    let mut changed = parsed.clone();
    changed.risk.max_orders_per_day = 1;
    for (old, new) in [(&parsed, &changed), (&changed, &parsed)] {
        diverged("classify", classify(old, new))?;
        diverged("changed_paths", changed_paths(old, new))?;
        diverged("pinning_switch", pinning_switch(old, new, &[]))?;
    }
    let same = classify(&parsed, &parsed).map_err(|e| e.to_string())?;
    assert_eq!(same.class, ChangeClass::Neutral);
    assert_eq!(
        changed_paths(&parsed, &parsed).map_err(|e| e.to_string())?,
        []
    );
    assert!(!pinning_switch(&parsed, &parsed, &[]).map_err(|e| e.to_string())?);
    Ok(())
}
