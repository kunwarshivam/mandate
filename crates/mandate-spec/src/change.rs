//! Versioning and change classification
//! ([mandate spec §9](../../../docs/specs/mandate.md#9-versioning-and-change-classification-dec-43)).
//!
//! [`classify`] reads two documents and nothing else: no context, no policy, no state. The verdict is
//! the join over the changed paths — increasing if any path is, else reducing if any is, else neutral —
//! and an unlisted path is increasing, which is the fail-safe §9.2 ends on.
//!
//! [`pinning_switch`] is the one exception, and it is named rather than buried: DEC-121 classifies
//! turning bring-your-own-strategy on as **one** risk-reducing change instead of a field-by-field
//! verdict, so it looks at the whole change at once.
//!
//! The paths come from the canonical values the version hashes, so a reported path is exactly one
//! that changes the hash; each path's verdict then reads the typed field where §9.2's row has one, and
//! the canonical text where it does not (DEC-172 item 10).

use core::cmp::Ordering;
use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_domain::AutonomyDecision;

use crate::condition::{Condition, ConditionValue, Operator};
use crate::document::{
    Approval, Autonomy, Goal, LadderRung, Mandate, Pointer, Rule, ScaleAction, SignalModel, pointer,
};
use crate::{SchemaDec, SpecError};

/// What a version does to risk (§9.2).
///
/// [`Ord`] is the severity order `Neutral < RiskReducing < RiskIncreasing < Invalid`, so a verdict
/// over several paths is a maximum and "which class wins" is one thing in one place, as it is for
/// [`AutonomyDecision`] and [`AgentMode`](mandate_domain::AgentMode).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ChangeClass {
    Neutral,
    RiskReducing,
    RiskIncreasing,
    /// `environment` or `connection_id` changed, which §9.2 calls invalid rather than classifying.
    /// V-031 reports the same thing at validation; neither surface depends on the other
    /// (DEC-128 item 12).
    Invalid,
}

impl ChangeClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RiskIncreasing => "risk_increasing",
            Self::RiskReducing => "risk_reducing",
            Self::Neutral => "neutral",
            Self::Invalid => "invalid",
        }
    }
}

/// A classified change.
///
/// `step_up_required` is exactly `class == RiskIncreasing` (§9.2's last paragraph). Independent
/// approval is a policy question and belongs to
/// [`PolicyOverlay`](crate::policy::PolicyOverlay), not here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Classification {
    pub class: ChangeClass,
    pub changed_paths: Vec<Pointer>,
    pub step_up_required: bool,
}

/// Classifies `new` against `old`.
///
/// A changed [`REFUSED_PATHS`] member is [`ChangeClass::Invalid`] before anything else is looked at,
/// the pinning switch included, so that verdict never rests on what [`PIN_SWITCH_PATHS`] lists.
///
/// A mandate whose public fields no longer match the document it was parsed from has no canonical
/// form, and is refused with [`ParseError::Diverged`](crate::ParseError::Diverged) rather than
/// classified from fields the version would not hash.
pub fn classify(old: &Mandate, new: &Mandate) -> Result<Classification, SpecError> {
    let (before, after) = (old.canonical()?, new.canonical()?);
    let changed_paths = diff(&before, &after);
    let class = if changed_paths
        .iter()
        .any(|p| REFUSED_PATHS.contains(&p.as_str()))
    {
        ChangeClass::Invalid
    } else if is_pinning_switch(old, new, &changed_paths) {
        ChangeClass::RiskReducing
    } else {
        let mut classes = Vec::with_capacity(changed_paths.len());
        for path in &changed_paths {
            classes.push(row(old, new, &before, &after, path.as_str())?);
        }
        join(classes)
    };
    Ok(Classification {
        class,
        step_up_required: class == ChangeClass::RiskIncreasing,
        changed_paths,
    })
}

/// The paths that differ between two documents.
///
/// Objects are walked; **arrays are compared whole**, so a ladder change reports
/// `/risk/drawdown_ladder` and never `/risk/drawdown_ladder/2/at`. MC-C01 depends on it, and an
/// element-by-element walk is planted bug 18.
pub fn changed_paths(old: &Mandate, new: &Mandate) -> Result<Vec<Pointer>, SpecError> {
    Ok(diff(&old.canonical()?, &new.canonical()?))
}

/// The paths §9.2 refuses to classify: a version may not move a mandate to another environment or
/// connection (V-031, DEC-128 item 12).
pub const REFUSED_PATHS: [&str; 2] = ["/environment", "/connection_id"];

/// The five paths DEC-121's pinning switch may touch and no others.
pub const PIN_SWITCH_PATHS: [&str; 5] = [
    "/universe/pinned",
    "/universe/pinned_instruments",
    "/universe/max_instruments",
    "/behavior/research",
    "/behavior/signal_models",
];

/// True when the change is DEC-121's pinning switch: not pinned before and pinned after, from a
/// version that **had** an admitting model, clearing `behavior.research` and every
/// `admits_instruments`, not raising `max_instruments`, and changing nothing outside
/// [`PIN_SWITCH_PATHS`].
///
/// Pinning a version that had no admitting model is increasing instead, and §9.2 says why: such a
/// version has an empty working universe and can open nothing, so pinning hands the agent instruments
/// it could not trade before. DEC-121's reason for calling pinning reducing — that it removes the
/// platform's discretion — does not apply when there was no discretion to remove.
///
/// `paths` must be [`changed_paths`] of the same two documents. Any other list is `false`, the answer
/// that needs step-up, so a caller cannot make a change reducing by leaving a path out (DEC-172
/// item 2).
pub fn pinning_switch(old: &Mandate, new: &Mandate, paths: &[Pointer]) -> Result<bool, SpecError> {
    Ok(paths == changed_paths(old, new)?.as_slice() && is_pinning_switch(old, new, paths))
}

/// Whether an autonomy change is reducing (§9.2's autonomy row).
///
/// Reducing only if **every** change is one of the six shapes §9.2 lists; anything else — reordering
/// rules, changing a field, an operator, a compound condition, the approvers, or the timeout — is
/// increasing. MI-11 is the property that matters here, and its oracle does not look at this function
/// at all: it evaluates generated actions under both rule sets and asserts the new decision is never
/// less strict.
///
/// Two identical rule sets are [`ChangeClass::Neutral`]: nothing changed, so nothing was made
/// stricter either (DEC-172 item 3).
pub fn classify_autonomy(old: &Autonomy, new: &Autonomy) -> Result<ChangeClass, SpecError> {
    if old == new {
        return Ok(ChangeClass::Neutral);
    }
    Ok(if autonomy_reduces(old, new) {
        ChangeClass::RiskReducing
    } else {
        ChangeClass::RiskIncreasing
    })
}

/// The changed paths of two canonical values, in pointer order.
fn diff(old: &Value, new: &Value) -> Vec<Pointer> {
    let mut out = Vec::new();
    walk(Some(old), Some(new), "", &mut out);
    out
}

/// Walks both objects' keys in byte order, which is pointer order too: a key's bytes are
/// `[a-z0-9_]`, all above `/`, so every path under a key sorts before the next key's. Anything that is
/// not an object on both sides — an array, a scalar, or an object meeting `null` — is compared whole
/// and reported where it sits. An absent member and a `null` one are different values and different
/// hashes, so a change between them is a change.
fn walk(old: Option<&Value>, new: Option<&Value>, at: &str, out: &mut Vec<Pointer>) {
    match (old, new) {
        (Some(Value::Object(before)), Some(Value::Object(after))) => {
            let keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
            for key in keys {
                walk(before.get(key), after.get(key), &format!("{at}/{key}"), out);
            }
        }
        _ => {
            if old != new {
                out.push(Pointer::new(at));
            }
        }
    }
}

/// The most severe class, [`ChangeClass`]'s [`Ord`]: increasing if any class is, else reducing if
/// any is, else neutral, and neutral for none.
fn join(classes: impl IntoIterator<Item = ChangeClass>) -> ChangeClass {
    classes.into_iter().fold(ChangeClass::Neutral, Ord::max)
}

/// §9.2's row for one changed path. `before` and `after` are the canonical values of `old` and
/// `new`, read only where the row compares text rather than a typed field.
///
/// [`classify`] has refused [`REFUSED_PATHS`] before any row is read, so they have no arm here.
fn row(
    old: &Mandate,
    new: &Mandate,
    before: &Value,
    after: &Value,
    path: &str,
) -> Result<ChangeClass, SpecError> {
    let (o, n) = (old, new);
    Ok(match path {
        "/name" => ChangeClass::Neutral,
        "/notifications/channels" => {
            if o.notifications
                .channels
                .is_subset(&n.notifications.channels)
            {
                ChangeClass::Neutral
            } else {
                ChangeClass::RiskIncreasing
            }
        }
        quiet
            if quiet == "/notifications/quiet_hours"
                || quiet.starts_with("/notifications/quiet_hours/") =>
        {
            ChangeClass::Neutral
        }
        "/capital/allocation_usd" => maximum(
            Some(&o.capital.allocation_usd),
            Some(&n.capital.allocation_usd),
        ),
        "/capital/max_loss_from_allocation" => maximum(
            Some(&o.capital.max_loss_from_allocation),
            Some(&n.capital.max_loss_from_allocation),
        ),
        "/risk/max_position_usd" => maximum(
            Some(&o.risk.max_position_usd),
            Some(&n.risk.max_position_usd),
        ),
        "/risk/max_position_fraction" => maximum(
            Some(&o.risk.max_position_fraction),
            Some(&n.risk.max_position_fraction),
        ),
        "/risk/max_gross_exposure_usd" => maximum(
            Some(&o.risk.max_gross_exposure_usd),
            Some(&n.risk.max_gross_exposure_usd),
        ),
        "/risk/max_order_usd" => maximum(Some(&o.risk.max_order_usd), Some(&n.risk.max_order_usd)),
        "/risk/max_orders_per_day" => maximum(
            Some(&o.risk.max_orders_per_day),
            Some(&n.risk.max_orders_per_day),
        ),
        "/risk/max_daily_loss" => {
            maximum(Some(&o.risk.max_daily_loss), Some(&n.risk.max_daily_loss))
        }
        "/risk/max_drawdown" => maximum(Some(&o.risk.max_drawdown), Some(&n.risk.max_drawdown)),
        "/risk/breach_confirm_s" => maximum(
            Some(&o.risk.breach_confirm_s),
            Some(&n.risk.breach_confirm_s),
        ),
        "/goal/target_qty"
        | "/goal/max_spend_usd"
        | "/goal/max_avg_price"
        | "/goal/profit_level" => maximum(goal_maximum(&o.goal, path), goal_maximum(&n.goal, path)),
        "/protection/stop_distance" => maximum(
            o.protection.stop_distance.as_ref(),
            n.protection.stop_distance.as_ref(),
        ),
        "/behavior/sizing/exit_threshold" => maximum(
            Some(&o.behavior.sizing.exit_threshold),
            Some(&n.behavior.sizing.exit_threshold),
        ),
        "/universe/max_instruments" => maximum(
            Some(&o.universe.max_instruments),
            Some(&n.universe.max_instruments),
        ),
        "/behavior/research/cost_cap_usd_per_day" => maximum(
            o.behavior
                .research
                .as_ref()
                .map(|r| &r.cost_cap_usd_per_day),
            n.behavior
                .research
                .as_ref()
                .map(|r| &r.cost_cap_usd_per_day),
        ),
        "/behavior/research/max_revisions_per_lineage" => maximum(
            o.behavior
                .research
                .as_ref()
                .map(|r| &r.max_revisions_per_lineage),
            n.behavior
                .research
                .as_ref()
                .map(|r| &r.max_revisions_per_lineage),
        ),
        "/behavior/sizing/entry_threshold" => minimum(
            Some(&o.behavior.sizing.entry_threshold),
            Some(&n.behavior.sizing.entry_threshold),
        ),
        "/behavior/sizing/rebalance_band" => minimum(
            Some(&o.behavior.sizing.rebalance_band),
            Some(&n.behavior.sizing.rebalance_band),
        ),
        "/risk/hysteresis" => minimum(Some(&o.risk.hysteresis), Some(&n.risk.hysteresis)),
        "/risk/reentry_cooldown_s" => minimum(
            Some(&o.risk.reentry_cooldown_s),
            Some(&n.risk.reentry_cooldown_s),
        ),
        "/risk/daily_breach_min_s" => minimum(
            Some(&o.risk.daily_breach_min_s),
            Some(&n.risk.daily_breach_min_s),
        ),
        "/risk/scale_lift_after_s" => minimum(
            Some(&o.risk.scale_lift_after_s),
            Some(&n.risk.scale_lift_after_s),
        ),
        "/behavior/research/interval_s" => minimum(
            o.behavior.research.as_ref().map(|r| &r.interval_s),
            n.behavior.research.as_ref().map(|r| &r.interval_s),
        ),
        "/risk/drawdown_ladder" => ladder(&o.risk.drawdown_ladder, &n.risk.drawdown_ladder),
        "/risk/scale_action" => scale_action(o.risk.scale_action, n.risk.scale_action),
        "/universe/pinned_instruments" => {
            if o.universe.pinned == n.universe.pinned {
                membership(
                    &o.universe.pinned_instruments.iter().collect(),
                    &n.universe.pinned_instruments.iter().collect(),
                )
            } else {
                ChangeClass::Neutral
            }
        }
        "/universe/pinned" => {
            if n.universe.pinned && has_admitting_model(o) {
                ChangeClass::RiskReducing
            } else {
                ChangeClass::RiskIncreasing
            }
        }
        "/universe/asset_classes" => {
            membership(&o.universe.asset_classes, &n.universe.asset_classes)
        }
        "/behavior/research" => {
            if n.behavior.research.is_some() {
                ChangeClass::RiskIncreasing
            } else {
                ChangeClass::RiskReducing
            }
        }
        "/protection/crypto_stop_limit_offset" => looser_if(
            n.protection
                .crypto_stop_limit_offset
                .cmp(&o.protection.crypto_stop_limit_offset),
        ),
        "/goal/end_date" => maximum(text_at(before, path), text_at(after, path)),
        "/protection/enabled" => looser_if(o.protection.enabled.cmp(&n.protection.enabled)),
        "/universe/leveraged_etps_enabled" => looser_if(
            n.universe
                .leveraged_etps_enabled
                .cmp(&o.universe.leveraged_etps_enabled),
        ),
        autonomy if autonomy.starts_with("/autonomy/") => {
            classify_autonomy(&o.autonomy, &n.autonomy)?
        }
        _ => ChangeClass::RiskIncreasing,
    })
}

/// A move along an order in which `Greater` is looser: increasing, reducing, or nothing.
fn looser_if(ordering: Ordering) -> ChangeClass {
    match ordering {
        Ordering::Greater => ChangeClass::RiskIncreasing,
        Ordering::Less => ChangeClass::RiskReducing,
        Ordering::Equal => ChangeClass::Neutral,
    }
}

/// §9.2's maximum row: larger is increasing, smaller is reducing, and a `null` or absent maximum is
/// unbounded, above every value (DEC-172 item 5).
fn maximum<T: Ord + ?Sized>(old: Option<&T>, new: Option<&T>) -> ChangeClass {
    looser_if(match (old, new) {
        (Some(old), Some(new)) => new.cmp(old),
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Less,
        (Some(_), None) => Ordering::Greater,
    })
}

/// §9.2's minimum row: smaller is increasing, larger is reducing, and a `null` or absent minimum is
/// no minimum, below every value — which is `Option`'s own order (DEC-172 item 5).
fn minimum<T: Ord>(old: Option<&T>, new: Option<&T>) -> ChangeClass {
    looser_if(old.cmp(&new))
}

/// Increasing if `new` holds anything `old` does not; else reducing if `old` holds anything `new`
/// does not; else neutral. Pinned instruments compare by whole entry, so an entry whose id stayed
/// while its symbol or class changed is an added one (DEC-172 item 1).
fn membership<T: Ord>(old: &BTreeSet<T>, new: &BTreeSet<T>) -> ChangeClass {
    if !new.is_subset(old) {
        ChangeClass::RiskIncreasing
    } else if !old.is_subset(new) {
        ChangeClass::RiskReducing
    } else {
        ChangeClass::Neutral
    }
}

/// The ladder row: new actions, in number or kind, are increasing; otherwise the join over every
/// rung's `at` and `factor` read as maximums, so any one rising is increasing even when another
/// tightened, and a `factor` that became `null` has risen (DEC-172 item 5).
fn ladder(old: &[LadderRung], new: &[LadderRung]) -> ChangeClass {
    let actions = |rungs: &[LadderRung]| rungs.iter().map(|r| r.action).collect::<Vec<_>>();
    if actions(old) != actions(new) {
        return ChangeClass::RiskIncreasing;
    }
    join(old.iter().zip(new).flat_map(|(o, n)| {
        [
            maximum(Some(&o.at), Some(&n.at)),
            maximum(o.factor.as_ref(), n.factor.as_ref()),
        ]
    }))
}

/// `limit_buys` to `trim_to_target` is reducing; the reverse is increasing.
fn scale_action(old: ScaleAction, new: ScaleAction) -> ChangeClass {
    match (old, new) {
        (ScaleAction::LimitBuys, ScaleAction::TrimToTarget) => ChangeClass::RiskReducing,
        (ScaleAction::TrimToTarget, ScaleAction::LimitBuys) => ChangeClass::RiskIncreasing,
        (ScaleAction::LimitBuys, ScaleAction::LimitBuys)
        | (ScaleAction::TrimToTarget, ScaleAction::TrimToTarget) => ChangeClass::Neutral,
    }
}

/// The goal maximum a path names, or `None` where the goal's type has no such member, which reads
/// as unbounded like a `null` one.
fn goal_maximum<'a>(goal: &'a Goal, path: &str) -> Option<&'a SchemaDec> {
    match (goal, path) {
        (Goal::Accumulate { target_qty, .. }, "/goal/target_qty") => Some(target_qty),
        (Goal::Accumulate { max_spend_usd, .. }, "/goal/max_spend_usd") => Some(max_spend_usd),
        (Goal::Accumulate { max_avg_price, .. }, "/goal/max_avg_price") => max_avg_price.as_ref(),
        (Goal::ProfitStop { profit_level, .. }, "/goal/profit_level") => Some(profit_level),
        _ => None,
    }
}

/// The text at a path of a canonical value, `None` for `null` or absent. `end_date` compares this
/// way: the schema's `YYYY-MM-DD` pattern makes text order date order, and a date the calendar
/// rejects (MC-V22) still has its text here where the typed goal has `None` (DEC-172 item 5).
fn text_at<'a>(document: &'a Value, path: &str) -> Option<&'a str> {
    pointer(document, path).and_then(Value::as_str)
}

fn has_admitting_model(mandate: &Mandate) -> bool {
    mandate
        .behavior
        .signal_models
        .iter()
        .any(|m| m.admits_instruments)
}

/// DEC-121's switch over the change's own paths, which [`classify`] computed and
/// [`pinning_switch`] checks a caller's list against.
///
/// When the switch fails, [`row`]'s `/universe/pinned` arm is reducing on the same two conditions
/// it starts from (pinned after, an admitting model before), which is looser than the switch. That
/// arm never decides a verdict between two valid versions: V-037 leaves the pinned version no
/// admitting model, so the model that admitted before has changed, and `/behavior/signal_models`,
/// an unlisted row, is increasing; V-036 makes `/behavior/research` move with it. Only a version
/// validation rejects reaches that arm's reducing verdict alone.
fn is_pinning_switch(old: &Mandate, new: &Mandate, paths: &[Pointer]) -> bool {
    let cleared: Vec<SignalModel> = old
        .behavior
        .signal_models
        .iter()
        .map(|m| SignalModel {
            admits_instruments: false,
            ..m.clone()
        })
        .collect();
    !old.universe.pinned
        && new.universe.pinned
        && has_admitting_model(old)
        && new.behavior.research.is_none()
        && new.behavior.signal_models == cleared
        && maximum(
            Some(&old.universe.max_instruments),
            Some(&new.universe.max_instruments),
        ) != ChangeClass::RiskIncreasing
        && paths.iter().all(|p| PIN_SWITCH_PATHS.contains(&p.as_str()))
}

/// True when every difference between two unequal autonomy blocks is one of §9.2's six reducing
/// shapes.
///
/// The blocks are destructured whole, here and in [`kept_rule_reduces`], so a field added to
/// [`Autonomy`], [`Approval`], or [`Rule`] does not compile until it is compared: an uncompared
/// field would let a change to it alone read as reducing.
fn autonomy_reduces(old: &Autonomy, new: &Autonomy) -> bool {
    let Autonomy {
        rules: _,
        default,
        admission,
        approval:
            Approval {
                timeout_s,
                on_timeout,
                approvers,
                two_approver_above_usd,
            },
        review_by,
    } = old;
    review_by == &new.review_by
        && approvers == &new.approval.approvers
        && timeout_s == &new.approval.timeout_s
        && on_timeout == &new.approval.on_timeout
        && maximum(
            two_approver_above_usd.as_ref(),
            new.approval.two_approver_above_usd.as_ref(),
        ) != ChangeClass::RiskIncreasing
        && new.default >= *default
        && new.admission >= *admission
        && rules_reduce(old, new)
}

/// The rule list's half of the autonomy row. Rules are identified by id: an id only the old list
/// holds is a removal, one only the new list holds an addition, and the ids both hold must keep
/// their order.
///
/// A new list that repeats an id is increasing, because no shape can be read off a repeated rule
/// (DEC-172 item 6). An id repeated only in the old list and kept appears twice among the old list's
/// kept ids and once among the new list's, so the order check refuses it.
fn rules_reduce(old: &Autonomy, new: &Autonomy) -> bool {
    let old_ids: Vec<_> = old.rules.iter().map(|r| &r.id).collect();
    let new_ids: Vec<_> = new.rules.iter().map(|r| &r.id).collect();
    if new_ids.iter().collect::<BTreeSet<_>>().len() != new_ids.len() {
        return false;
    }
    let kept_in_old: Vec<_> = old_ids.iter().filter(|id| new_ids.contains(id)).collect();
    let kept_in_new: Vec<_> = new_ids.iter().filter(|id| old_ids.contains(id)).collect();
    if kept_in_old != kept_in_new {
        return false;
    }
    let removals_reduce = with_later(&old.rules)
        .filter(|(rule, _)| !new_ids.contains(&&rule.id))
        .all(|(rule, later)| loosest(later, old.default) >= rule.then);
    removals_reduce
        && with_later(&new.rules).all(|(rule, later)| {
            let strictest_after = strictest(later, new.default);
            match old.rules.iter().find(|r| r.id == rule.id) {
                None => rule.then >= strictest_after,
                Some(before) => kept_rule_reduces(before, rule, strictest_after),
            }
        })
}

/// A kept rule, one of §9.2's shapes or unchanged: with its condition unchanged it may only make
/// its `then` stricter; with its condition changed it must keep its `then`, and one value of a
/// single comparison must move so an `auto` rule matches less often, or an `ask` or `deny` rule more
/// often with nothing stricter after it (DEC-172 item 9). The caller matched the two by id.
fn kept_rule_reduces(old: &Rule, new: &Rule, strictest_after: AutonomyDecision) -> bool {
    let Rule { id: _, when, then } = old;
    if when == &new.when {
        return new.then >= *then;
    }
    if then != &new.then {
        return false;
    }
    match (new.then, how_often(when, &new.when)) {
        (AutonomyDecision::Auto, Some(Often::Less)) => true,
        (AutonomyDecision::Ask | AutonomyDecision::Deny, Some(Often::More)) => {
            strictest_after <= new.then
        }
        _ => false,
    }
}

/// Which way a single comparison's one changed value moves how often it matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Often {
    More,
    Less,
}

/// `None` unless both conditions are one comparison of the same field with the same operator and
/// the value moved in a direction the operator orders: a decimal for `lt`, `lte`, `gt`, and `gte`,
/// and strict set containment for `in` and `not_in`. `eq`, `ne`, a compound condition, and a set
/// that neither contains nor is contained by the old one have no direction (DEC-172 item 9).
fn how_often(old: &Condition, new: &Condition) -> Option<Often> {
    let (
        Condition::Compare {
            field: old_field,
            op,
            value: old_value,
        },
        Condition::Compare {
            field: new_field,
            op: new_op,
            value: new_value,
        },
    ) = (old, new)
    else {
        return None;
    };
    if (old_field, op) != (new_field, new_op) {
        return None;
    }
    match (op, old_value, new_value) {
        (Operator::Lt | Operator::Lte, ConditionValue::Decimal(a), ConditionValue::Decimal(b)) => {
            by_order(b.cmp(a))
        }
        (Operator::Gt | Operator::Gte, ConditionValue::Decimal(a), ConditionValue::Decimal(b)) => {
            by_order(a.cmp(b))
        }
        (Operator::In, ConditionValue::List(a), ConditionValue::List(b)) => by_containment(a, b),
        (Operator::NotIn, ConditionValue::List(a), ConditionValue::List(b)) => by_containment(b, a),
        _ => None,
    }
}

/// `Greater` widens the comparison.
fn by_order(ordering: Ordering) -> Option<Often> {
    match ordering {
        Ordering::Greater => Some(Often::More),
        Ordering::Less => Some(Often::Less),
        Ordering::Equal => None,
    }
}

/// `More` when `wider` strictly contains `narrower` as sets, `Less` when the reverse, else `None`.
fn by_containment(narrower: &[String], wider: &[String]) -> Option<Often> {
    let a: BTreeSet<&String> = narrower.iter().collect();
    let b: BTreeSet<&String> = wider.iter().collect();
    if a == b {
        None
    } else if a.is_subset(&b) {
        Some(Often::More)
    } else if b.is_subset(&a) {
        Some(Often::Less)
    } else {
        None
    }
}

/// Each rule with the rules after it.
fn with_later(rules: &[Rule]) -> impl Iterator<Item = (&Rule, &[Rule])> {
    let mut rest = rules;
    core::iter::from_fn(move || {
        let (rule, later) = rest.split_first()?;
        rest = later;
        Some((rule, later))
    })
}

/// The strictest decision among later rules and the default.
fn strictest(later: &[Rule], default: AutonomyDecision) -> AutonomyDecision {
    later
        .iter()
        .map(|r| r.then)
        .fold(default, AutonomyDecision::stricter)
}

/// The loosest decision among later rules and the default.
fn loosest(later: &[Rule], default: AutonomyDecision) -> AutonomyDecision {
    later.iter().map(|r| r.then).fold(default, Ord::min)
}

#[cfg(test)]
mod tests;
