//! The payload side of the fold and the draft: how a value the journal carries becomes a core type,
//! and how a core type becomes a canonical value again (journal spec §4, §9).
//!
//! Every name here is written once and read back by the fold, so a draft this module writes is a
//! draft this module can rebuild state from. That round trip is what lets the runtime keep nothing
//! durable outside the journal (DEC-131 item 17).

use std::collections::BTreeMap;

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::{Int, Key, Value};
use mandate_num::{Price, Qty};

use crate::error::RuntimeError;
use crate::types::{Initiator, Mode, ModelOutput, OwnerConfirmation, Purpose, RiskClock};

/// A canonical object, refusing a key the canonical form does not admit (journal spec §4.1).
pub(crate) fn object(pairs: Vec<(&'static str, Value)>) -> Result<Value, RuntimeError> {
    let mut members: BTreeMap<Key, Value> = BTreeMap::new();
    for (name, value) in pairs {
        let key = Key::new(name).map_err(|_| non_canonical(name))?;
        members.insert(key, value);
    }
    Ok(Value::Object(members))
}

/// A refusal naming the payload field that could not be read or written (ADR-0001 ES-09).
pub(crate) fn non_canonical(field: &str) -> RuntimeError {
    RuntimeError::NonCanonicalPayload {
        field: field.to_owned(),
    }
}

pub(crate) fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

/// A whole-second risk clock as the canonical integer journal spec §2 requires of a risk input.
pub(crate) fn seconds(at: RiskClock, field: &'static str) -> Result<Value, RuntimeError> {
    let secs = u64::try_from(at.secs()).map_err(|_| non_canonical(field))?;
    Int::new(secs)
        .map(Value::Int)
        .ok_or_else(|| non_canonical(field))
}

/// A risk-clock second that may be negative, which the canonical integer form cannot hold (journal
/// spec §4.4 admits no sign), written as canonical decimal text instead.
pub(crate) fn seconds_text(at: RiskClock) -> Value {
    Value::Str(at.secs().to_string())
}

pub(crate) fn str_of<'a>(payload: &'a Value, field: &str) -> Option<&'a str> {
    payload.get(field).and_then(Value::as_str)
}

/// A risk-clock second read from a payload. `None` for a missing field, a non-integer, or an
/// integer outside `i64`, so an unreadable clock never silently becomes zero.
pub(crate) fn clock_of(payload: &Value, field: &str) -> Option<RiskClock> {
    let secs = payload.get(field).and_then(Value::as_int)?;
    i64::try_from(secs).ok().map(RiskClock::from_secs)
}

/// The counterpart of [`seconds_text`].
pub(crate) fn clock_text_of(payload: &Value, field: &str) -> Option<RiskClock> {
    str_of(payload, field)
        .and_then(|text| text.parse::<i64>().ok())
        .map(RiskClock::from_secs)
}

pub(crate) fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::Normal => "normal",
        Mode::ExitsOnly => "exits_only",
        Mode::Paused => "paused",
        Mode::Stopped => "stopped",
    }
}

pub(crate) fn mode_from(name: &str) -> Option<Mode> {
    match name {
        "normal" => Some(Mode::Normal),
        "exits_only" => Some(Mode::ExitsOnly),
        "paused" => Some(Mode::Paused),
        "stopped" => Some(Mode::Stopped),
        _ => None,
    }
}

pub(crate) fn purpose_name(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Open => "open",
        Purpose::Increase => "increase",
        Purpose::RiskExit => "risk_exit",
        Purpose::OwnerExit => "owner_exit",
        Purpose::DiscretionaryExit => "discretionary_exit",
        Purpose::Protective => "protective",
        Purpose::Flatten => "flatten",
    }
}

/// Every purpose, so that [`purpose_from`] is the inverse of [`purpose_name`] by construction rather
/// than by a second list that could disagree with the first.
const PURPOSES: [Purpose; 7] = [
    Purpose::Open,
    Purpose::Increase,
    Purpose::RiskExit,
    Purpose::OwnerExit,
    Purpose::DiscretionaryExit,
    Purpose::Protective,
    Purpose::Flatten,
];

pub(crate) fn purpose_from(name: &str) -> Option<Purpose> {
    PURPOSES
        .into_iter()
        .find(|purpose| purpose_name(*purpose) == name)
}

pub(crate) fn side_name(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

pub(crate) fn side_from(name: &str) -> Option<Side> {
    match name {
        "buy" => Some(Side::Buy),
        "sell" => Some(Side::Sell),
        _ => None,
    }
}

pub(crate) fn initiator_name(initiator: Initiator) -> &'static str {
    match initiator {
        Initiator::Owner => "owner",
        Initiator::RiskLimit => "risk_limit",
        Initiator::PlatformOperator => "platform_operator",
    }
}

/// `Broker` is absent on purpose: a broker-driven restriction is account state that arrives as a
/// copied `AgentModeApplied`, never a kill switch (trading-domain spec §7.3, DEC-131 item 21).
pub(crate) fn initiator_from(name: &str) -> Option<Initiator> {
    match name {
        "owner" => Some(Initiator::Owner),
        "risk_limit" => Some(Initiator::RiskLimit),
        "platform_operator" => Some(Initiator::PlatformOperator),
        _ => None,
    }
}

/// The model output a `ModelOutputRecorded` payload carries, rebuilt field by field so that a
/// replay reaches the same signal inputs the live run decided on (mandate spec §8.1).
pub(crate) fn model_output_of(payload: &Value) -> Result<ModelOutput, RuntimeError> {
    let model = str_of(payload, "model").ok_or_else(|| non_canonical("model"))?;
    let version = str_of(payload, "version").ok_or_else(|| non_canonical("version"))?;
    let instrument = str_of(payload, "instrument").ok_or_else(|| non_canonical("instrument"))?;
    let as_of = clock_text_of(payload, "as_of").ok_or_else(|| non_canonical("as_of"))?;
    let expires_at =
        clock_text_of(payload, "expires_at").ok_or_else(|| non_canonical("expires_at"))?;
    let content = payload
        .get("content")
        .cloned()
        .ok_or_else(|| non_canonical("content"))?;
    Ok(ModelOutput {
        model: model.to_owned(),
        version: version.to_owned(),
        instrument: instrument_of(instrument)?,
        as_of,
        expires_at,
        content,
    })
}

/// The order an `IntentProposed` payload records. Shared by the fold and the retry, so an intent
/// handed again is byte-for-byte the one its draft recorded.
pub(crate) fn order_of(payload: &Value) -> Result<crate::types::IntentBody, RuntimeError> {
    let purpose = str_of(payload, "purpose")
        .and_then(purpose_from)
        .ok_or_else(|| non_canonical("purpose"))?;
    let instrument = str_of(payload, "instrument").ok_or_else(|| non_canonical("instrument"))?;
    let side = str_of(payload, "side")
        .and_then(side_from)
        .ok_or_else(|| non_canonical("side"))?;
    let qty = str_of(payload, "qty").ok_or_else(|| non_canonical("qty"))?;
    let limit = str_of(payload, "limit").ok_or_else(|| non_canonical("limit"))?;
    Ok(crate::types::IntentBody::Order {
        instrument: instrument_of(instrument)?,
        side,
        qty: qty_of(qty)?,
        limit: price_of(limit)?,
        purpose,
    })
}

pub(crate) fn instrument_of(name: &str) -> Result<InstrumentId, RuntimeError> {
    InstrumentId::new(name).map_err(|_| non_canonical("instrument"))
}

pub(crate) fn qty_of(text: &str) -> Result<Qty, RuntimeError> {
    Qty::parse(text).map_err(RuntimeError::Num)
}

pub(crate) fn price_of(text: &str) -> Result<Price, RuntimeError> {
    Price::parse(text).map_err(RuntimeError::Num)
}

/// Why an `AgentModeChanged` was written. The fold reads it to restore the deployment's lifecycle
/// state, which no account stream can know (Decisions needed 4): a replay of the agent stream is
/// what tells a restarted process that its owner had paused or stopped it.
pub(crate) const REASON_RESTRICTION: &str = "restriction_changed";
pub(crate) const REASON_STARTUP_HOLD: &str = "awaiting_reconciliation";
pub(crate) const REASON_OWNER_PAUSE: &str = "owner_pause";
pub(crate) const REASON_OWNER_RESUME: &str = "owner_resume";
pub(crate) const REASON_OWNER_STOP: &str = "owner_stop";
pub(crate) const REASON_KILL_SWITCH: &str = "kill_switch";

/// The owner's confirmation an `OwnerExitRequested` recorded, or `None` when the owner confirmed
/// nothing. A replay needs it so that a flatten handed again after a restart carries the same
/// confirmed bid, bid size, and floor price the executor priced the first one from (trading-domain
/// spec §5.5, §5.6).
pub(crate) fn owner_confirmation_of(
    payload: &Value,
) -> Result<Option<OwnerConfirmation>, RuntimeError> {
    if !matches!(payload.get("confirmed"), Some(Value::Bool(true))) {
        return Ok(None);
    }
    let bid = str_of(payload, "bid").ok_or_else(|| non_canonical("bid"))?;
    let bid_size = str_of(payload, "bid_size").ok_or_else(|| non_canonical("bid_size"))?;
    let floor = str_of(payload, "floor").ok_or_else(|| non_canonical("floor"))?;
    let user = str_of(payload, "user").ok_or_else(|| non_canonical("user"))?;
    let step_up = str_of(payload, "step_up").ok_or_else(|| non_canonical("step_up"))?;
    Ok(Some(OwnerConfirmation {
        bid: price_of(bid)?,
        bid_size: qty_of(bid_size)?,
        floor: price_of(floor)?,
        user: user.to_owned(),
        step_up: step_up.to_owned(),
    }))
}
