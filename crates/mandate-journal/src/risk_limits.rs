//! The account stream's risk-limit records journal spec v0.58 §9.27 closes at schema version 1
//! (DEC-2301; E6-15): `RiskDayStarted`, `RiskLimitTriggered`, `RiskLimitLifted`,
//! `HighWaterMarkReset`, `PositionReleased`, `InstrumentRestrictionChanged`, and `GoalCompleted`,
//! with consistency rules 230 to 238, each refusing at its member in the order §9.27 states. Every
//! member is the risk-state fold's event or its snapshot after the input, so every rule only
//! refuses a draft the fold would never write; none compares one record with another.

use mandate_canon::Value;
use mandate_num::Usd;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamType};

const DAY: &str = "RiskDayStarted";
const TRIGGERED: &str = "RiskLimitTriggered";
const LIFTED: &str = "RiskLimitLifted";
const RESET: &str = "HighWaterMarkReset";
const RELEASED: &str = "PositionReleased";
const RESTRICTION: &str = "InstrumentRestrictionChanged";
const GOAL: &str = "GoalCompleted";

/// The account-stream event types §9.27 adds to [`crate::control::governs`].
pub(crate) const ACCOUNT: [&str; 7] = [DAY, TRIGGERED, LIFTED, RESET, RELEASED, RESTRICTION, GOAL];

const TRIPWIRE: &str = "tripwire:";
const RUNG: &str = "drawdown_ladder[";
const COUNTED: [&str; 2] = ["consecutive_losing_exits", "new_instruments"];
/// §9.3 rule 31's removing reasons, which set `removed_instrument`.
const REMOVING: [&str; 6] = [
    "thesis_expired",
    "thesis_invalidated",
    "lineage_retired",
    "eligibility_lost",
    "operator_halt",
    "version_applied",
];
/// §9.3 rule 31's admitting reasons, which clear it.
const ADMITTING: [&str; 2] = ["thesis_admitted", "version_applied"];

/// The §9.27 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    if schema_version != 1 || stream != StreamType::Account {
        return None;
    }
    match event_type {
        DAY => Some(&RISK_DAY_STARTED),
        TRIGGERED => Some(&RISK_LIMIT_TRIGGERED),
        LIFTED => Some(&RISK_LIMIT_LIFTED),
        RESET => Some(&HIGH_WATER_MARK_RESET),
        RELEASED => Some(&POSITION_RELEASED),
        RESTRICTION => Some(&INSTRUMENT_RESTRICTION_CHANGED),
        GOAL => Some(&GOAL_COMPLETED),
        _ => None,
    }
}

/// Rules 230 to 238 on a payload its schema has normalized, in rule order: the first that fails is
/// reported. `causation_id` is the envelope's, which rule 235 reads.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    let text = |member: &str| {
        payload
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let null = |member: &str| matches!(payload.get(member), None | Some(Value::Null));
    match event_type {
        TRIGGERED => triggered(payload),
        LIFTED => {
            ensure(
                !text("inherited_loss").starts_with('-'),
                "payload.inherited_loss",
            )?;
            let limit = text("limit");
            let action_holds = null("action")
                || (text("action") == "scale_sizes" && limit.starts_with(RUNG) && null("reason"));
            ensure(action_holds, "payload.action")?;
            let reason = text("reason");
            let reason_holds = if limit.starts_with(TRIPWIRE) {
                reason == "owner_acknowledged"
            } else {
                reason != "version_loosened" || limit == "lifetime_floor"
            };
            ensure(reason_holds, "payload.reason")?;
            if reason == "owner_acknowledged" {
                ensure(caused(causation_id), "causation_id")?;
            }
            Ok(())
        }
        RESET => ensure(caused(causation_id), "causation_id"),
        RELEASED => {
            let owner = text("cause") == "owner_release";
            let qty = text("qty");
            let qty_holds = !qty.starts_with('-') && (!owner || qty != "0");
            ensure(qty_holds, "payload.qty")?;
            for member in ["warning_ref", "user_id", "step_up"] {
                ensure(null(member) != owner, &format!("payload.{member}"))?;
            }
            Ok(())
        }
        RESTRICTION => {
            let reason = text("reason");
            let active = payload.get("active") == Some(&Value::Bool(true));
            let (reason_holds, active_holds) = if text("restriction") == "stale_mark" {
                (
                    reason == "no_sane_mark" || reason == "sane_mark",
                    active == (reason == "no_sane_mark"),
                )
            } else {
                let setting = REMOVING.contains(&reason);
                let clearing = ADMITTING.contains(&reason);
                (setting || clearing, if active { setting } else { clearing })
            };
            ensure(reason_holds, "payload.reason")?;
            ensure(active_holds, "payload.active")
        }
        GOAL => {
            ensure(null("then") != null("on_complete"), "payload.then")?;
            ensure(null("reason") || !null("then"), "payload.reason")
        }
        _ => Ok(()),
    }
}

/// Rules 230 to 233 on a `RiskLimitTriggered`.
fn triggered(payload: &Value) -> Result<(), Invalid> {
    let text = |member: &str| {
        payload
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    };
    let null = |member: &str| matches!(payload.get(member), None | Some(Value::Null));
    let limit = text("limit");
    let tripwire = limit.starts_with(TRIPWIRE);
    let action = text("action");
    ensure(
        (text("reason") == "tripwire_condition") == tripwire,
        "payload.reason",
    )?;
    let action_holds = if tripwire {
        action == "end_delegations" || action == "exits_only"
    } else {
        action != "end_delegations" && (action != "scale_sizes" || limit.starts_with(RUNG))
    };
    ensure(action_holds, "payload.action")?;
    ensure(
        !text("inherited_loss").starts_with('-'),
        "payload.inherited_loss",
    )?;
    ensure(null("breach_time_s") == tripwire, "payload.breach_time_s")?;
    for member in ["metric", "threshold", "value"] {
        ensure(null(member) != tripwire, &format!("payload.{member}"))?;
    }
    if !tripwire {
        return Ok(());
    }
    let threshold_text = text("threshold");
    let value_text = text("value");
    let amount = |member: &str, text: &str| {
        Usd::parse(text)
            .map_err(|_| Invalid::new(InvalidReason::Schema, format!("payload.{member}")))
    };
    let threshold = amount("threshold", threshold_text)?;
    let counted = COUNTED.contains(&text("metric"));
    let threshold_holds = if counted {
        threshold_text
            .parse::<u64>()
            .is_ok_and(|n| (1..=1000).contains(&n))
    } else {
        let cents = threshold_text
            .split_once('.')
            .is_none_or(|(_, fraction)| fraction.len() <= 2);
        cents && !threshold.is_negative() && threshold_text != "0"
    };
    ensure(threshold_holds, "payload.threshold")?;
    let value = amount("value", value_text)?;
    let value_whole = !counted || value_text.parse::<u64>().is_ok();
    ensure(value_whole && value >= threshold, "payload.value")
}

/// Rule 235: an acknowledgment's lift or reset names the copy that caused it.
fn caused(causation_id: Option<&Value>) -> bool {
    causation_id.is_some_and(|cause| *cause != Value::Null)
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// §9.27's `limit_key`: a named limit, one of at most five rungs, or a tripwire by its mandate
/// schema `id`, `^[a-z][a-z0-9_]{0,31}$`.
fn limit_key(text: &str) -> bool {
    if matches!(text, "max_daily_loss" | "lifetime_floor" | "profit_stop") {
        return true;
    }
    if let Some(index) = text.strip_prefix(RUNG).and_then(|t| t.strip_suffix(']')) {
        return matches!(index, "0" | "1" | "2" | "3" | "4");
    }
    let Some(id) = text.strip_prefix(TRIPWIRE) else {
        return false;
    };
    let mut chars = id.chars();
    chars.next().is_some_and(|c| c.is_ascii_lowercase())
        && id.len() <= 32
        && chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

const STEP_UP: &[(&str, Ty)] = &[
    ("assertion_id", Ty::Str),
    ("authenticated_at", Ty::Timestamp),
    ("method", Ty::Str),
];

const ACTIONS: &[&str] = &[
    "scale_sizes",
    "exits_only",
    "flatten_and_pause",
    "end_delegations",
];
const ON_COMPLETE: &[&str] = &["hold_protected", "disarm_ladder", "release"];

static RISK_DAY_STARTED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("day", Ty::Date),
    ("day_start_equity", Ty::Decimal),
    ("risk_clock", Ty::RiskClock),
]);

static RISK_LIMIT_TRIGGERED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("agent_equity", Ty::Decimal),
    ("high_water_mark", Ty::Decimal),
    ("drawdown", Ty::Decimal),
    ("day_start_equity", Ty::Decimal),
    ("capital_base", Ty::Decimal),
    ("inherited_loss", Ty::Decimal),
    ("net_contributed", Ty::Decimal),
    ("limit", Ty::Text(limit_key)),
    ("action", Ty::OneOf(ACTIONS)),
    (
        "reason",
        Ty::Nullable(&Ty::OneOf(&[
            "hard_trigger",
            "hard_breach_pending",
            "resolved_at_rollover",
            "new_day_breach",
            "after_reset",
            "tripwire_condition",
        ])),
    ),
    ("breach_time_s", Ty::Nullable(&Ty::Int)),
    (
        "metric",
        Ty::Nullable(&Ty::OneOf(&[
            "consecutive_losing_exits",
            "realized_loss_usd",
            "new_instruments",
        ])),
    ),
    ("threshold", Ty::Nullable(&Ty::Decimal)),
    ("value", Ty::Nullable(&Ty::Decimal)),
    ("risk_clock", Ty::RiskClock),
]);

static RISK_LIMIT_LIFTED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("agent_equity", Ty::Decimal),
    ("high_water_mark", Ty::Decimal),
    ("drawdown", Ty::Decimal),
    ("day_start_equity", Ty::Decimal),
    ("capital_base", Ty::Decimal),
    ("inherited_loss", Ty::Decimal),
    ("net_contributed", Ty::Decimal),
    ("limit", Ty::Text(limit_key)),
    ("action", Ty::Nullable(&Ty::OneOf(ACTIONS))),
    (
        "reason",
        Ty::Nullable(&Ty::OneOf(&[
            "owner_acknowledged",
            "version_loosened",
            "hard_breach_cleared",
        ])),
    ),
    ("risk_clock", Ty::RiskClock),
]);

static HIGH_WATER_MARK_RESET: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("from", Ty::Decimal),
    ("to", Ty::Decimal),
    ("user_id", Ty::Ident),
    ("step_up", Ty::Record(STEP_UP)),
    ("risk_clock", Ty::RiskClock),
]);

static POSITION_RELEASED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("instrument", Ty::AssetId),
    ("qty", Ty::Decimal),
    ("cause", Ty::OneOf(&["owner_release", "goal_release"])),
    ("canceled_order_ids", Ty::List(&Ty::Ident)),
    ("warning_ref", Ty::Nullable(&Ty::DigestRef)),
    ("user_id", Ty::Nullable(&Ty::Ident)),
    ("step_up", Ty::Nullable(&Ty::Record(STEP_UP))),
    ("risk_clock", Ty::RiskClock),
]);

static INSTRUMENT_RESTRICTION_CHANGED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    ("instrument", Ty::AssetId),
    (
        "restriction",
        Ty::OneOf(&["stale_mark", "removed_instrument"]),
    ),
    (
        "reason",
        Ty::OneOf(&[
            "no_sane_mark",
            "sane_mark",
            "thesis_admitted",
            "thesis_expired",
            "thesis_invalidated",
            "lineage_retired",
            "eligibility_lost",
            "operator_halt",
            "version_applied",
        ]),
    ),
    ("active", Ty::Bool),
    ("risk_clock", Ty::RiskClock),
]);

static GOAL_COMPLETED: Ty = Ty::Record(&[
    ("agent_id", Ty::Ident),
    (
        "reason",
        Ty::Nullable(&Ty::OneOf(&[
            "profit_stop_reached",
            "target_qty",
            "max_spend",
            "end_date",
        ])),
    ),
    (
        "then",
        Ty::Nullable(&Ty::OneOf(&[
            "hold_protected",
            "disarm_ladder",
            "release",
            "discretionary_exit_all_then_retire",
        ])),
    ),
    ("on_complete", Ty::Nullable(&Ty::OneOf(ON_COMPLETE))),
    ("risk_clock", Ty::RiskClock),
]);
