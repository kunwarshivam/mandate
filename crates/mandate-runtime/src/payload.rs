//! The payload side of the fold and the draft: how a value the journal carries becomes a core type,
//! and how a core type becomes a canonical value again (journal spec §4, §9).
//!
//! Every name here is written once and read back by the fold, so a draft this module writes is a
//! draft this module can rebuild state from. That round trip is what lets the runtime keep nothing
//! durable outside the journal (DEC-131 item 17).

use std::collections::BTreeMap;

use mandate_accounting::{InstrumentId, Side};
use mandate_approval::{AssertionId, ContentHash, StepUp, StepUpMethod};
use mandate_canon::{Digest, Int, Key, Value};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

use crate::error::RuntimeError;
use crate::types::{
    EventId, Initiator, Mode, ModelDirection, ModelOutput, ModelOutputIgnored, OwnerConfirmation,
    Purpose, RiskClock,
};

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

/// A count or a `seq` as the canonical integer (journal spec §4.4), refusing one beyond its bound.
pub(crate) fn count(n: u64, field: &'static str) -> Result<Value, RuntimeError> {
    Int::new(n)
        .map(Value::Int)
        .ok_or_else(|| non_canonical(field))
}

/// A whole-second risk clock as journal spec §4.7's canonical timestamp, the form §9.2 types
/// `OwnerCommandRefused`'s `effective_at` (DEC-261 item 7, DEC-308): the instant `UtcNanos` prints
/// and `UtcNanos::parse` reads back unchanged, on the whole second, never the integer seconds
/// [`seconds`] writes and the registered schema refuses (`schema` at `payload.effective_at`, as the
/// vectors' `refused_at_risk_clock_seconds` expects). The runtime's one writer of the member, `escalation::refused`, stamps
/// through here.
///
/// # Errors
/// [`RuntimeError::NonCanonicalPayload`] naming `field` when the second lies outside the range a §4.7
/// timestamp can hold.
pub(crate) fn stamp(at: RiskClock, field: &'static str) -> Result<Value, RuntimeError> {
    UtcNanos::from_parts(at.secs(), 0)
        .map(|instant| Value::Str(instant.to_string()))
        .map_err(|_| non_canonical(field))
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
    let model_id = str_of(payload, "model_id").ok_or_else(|| non_canonical("model_id"))?;
    let model_version =
        str_of(payload, "model_version").ok_or_else(|| non_canonical("model_version"))?;
    let content_hash = digest_of(payload, "content_hash")?;
    let instrument =
        str_of(payload, "instrument_id").ok_or_else(|| non_canonical("instrument_id"))?;
    let as_of = timestamp_of(payload, "as_of")?;
    let expires_at = timestamp_of(payload, "expires_at")?;
    let direction = str_of(payload, "direction")
        .map(model_direction_from)
        .ok_or_else(|| non_canonical("direction"))?;
    let conviction = str_of(payload, "conviction")
        .ok_or_else(|| non_canonical("conviction"))
        .and_then(|value| mandate_num::Conviction::parse(value).map_err(RuntimeError::Num))?;
    let confidence = str_of(payload, "confidence")
        .ok_or_else(|| non_canonical("confidence"))
        .and_then(|value| mandate_num::Unit::parse(value).map_err(RuntimeError::Num))?;
    let horizon_s = payload
        .get("horizon_s")
        .and_then(Value::as_int)
        .ok_or_else(|| non_canonical("horizon_s"))?;
    let thesis_ref = optional_digest_of(payload, "thesis_ref")?;
    let evidence = payload
        .get("evidence")
        .and_then(Value::as_array)
        .ok_or_else(|| non_canonical("evidence"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(|event_id| EventId(event_id.to_owned()))
                .ok_or_else(|| non_canonical("evidence"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let invalidation = optional_text_of(payload, "invalidation")?;
    let thesis_id = optional_text_of(payload, "thesis_id")?;
    let lineage_id = optional_text_of(payload, "lineage_id")?;
    let ignored = optional_text_of(payload, "ignored")?
        .map(|value| ignored_from(&value).ok_or_else(|| non_canonical("ignored")))
        .transpose()?;
    Ok(ModelOutput {
        model_id: model_id.to_owned(),
        model_version: model_version.to_owned(),
        content_hash,
        instrument_id: instrument_of(instrument)?,
        as_of,
        expires_at,
        direction,
        conviction,
        confidence,
        horizon_s,
        thesis_ref,
        evidence,
        invalidation,
        thesis_id,
        lineage_id,
        ignored,
    })
}

pub(crate) fn model_output(output: &ModelOutput) -> Result<Value, RuntimeError> {
    object(vec![
        ("model_id", text(&output.model_id)),
        ("model_version", text(&output.model_version)),
        (
            "content_hash",
            text(&format!("sha256:{}", output.content_hash)),
        ),
        ("instrument_id", text(output.instrument_id.as_str())),
        ("as_of", stamp(output.as_of, "as_of")?),
        ("expires_at", stamp(output.expires_at, "expires_at")?),
        ("direction", text(model_direction_name(&output.direction))),
        ("conviction", text(&output.conviction.to_string())),
        ("confidence", text(&output.confidence.to_string())),
        ("horizon_s", count(output.horizon_s, "horizon_s")?),
        (
            "thesis_ref",
            output
                .thesis_ref
                .map_or(Value::Null, |digest| text(&format!("sha256:{digest}"))),
        ),
        (
            "evidence",
            Value::Array(
                output
                    .evidence
                    .iter()
                    .map(|event_id| text(&event_id.0))
                    .collect(),
            ),
        ),
        (
            "invalidation",
            output.invalidation.as_deref().map_or(Value::Null, text),
        ),
        (
            "thesis_id",
            output.thesis_id.as_deref().map_or(Value::Null, text),
        ),
        (
            "lineage_id",
            output.lineage_id.as_deref().map_or(Value::Null, text),
        ),
        (
            "ignored",
            output.ignored.map(ignored_name).map_or(Value::Null, text),
        ),
    ])
}

fn digest_of(payload: &Value, field: &str) -> Result<Digest, RuntimeError> {
    str_of(payload, field)
        .and_then(|value| value.strip_prefix("sha256:"))
        .and_then(Digest::from_hex)
        .ok_or_else(|| non_canonical(field))
}

fn optional_digest_of(payload: &Value, field: &str) -> Result<Option<Digest>, RuntimeError> {
    match payload.get(field) {
        Some(Value::Null) => Ok(None),
        Some(_) => digest_of(payload, field).map(Some),
        None => Err(non_canonical(field)),
    }
}

fn optional_text_of(payload: &Value, field: &str) -> Result<Option<String>, RuntimeError> {
    match payload.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::Str(value)) => Ok(Some(value.clone())),
        Some(_) | None => Err(non_canonical(field)),
    }
}

fn timestamp_of(payload: &Value, field: &str) -> Result<RiskClock, RuntimeError> {
    str_of(payload, field)
        .and_then(|value| UtcNanos::parse(value).ok())
        .map(|value| RiskClock::from_secs(value.secs()))
        .ok_or_else(|| non_canonical(field))
}

fn model_direction_name(direction: &ModelDirection) -> &str {
    match direction {
        ModelDirection::Long => "long",
        ModelDirection::Other(direction) => direction,
    }
}

fn model_direction_from(direction: &str) -> ModelDirection {
    match direction {
        "long" => ModelDirection::Long,
        other => ModelDirection::Other(other.to_owned()),
    }
}

fn ignored_name(reason: ModelOutputIgnored) -> &'static str {
    match reason {
        ModelOutputIgnored::NotPinned => "not_pinned",
        ModelOutputIgnored::ModelWithdrawn => "model_withdrawn",
        ModelOutputIgnored::OutputLimits => "output_limits",
        ModelOutputIgnored::NotInUniverse => "not_in_universe",
        ModelOutputIgnored::DirectionNotAllowed => "direction_not_allowed",
        ModelOutputIgnored::HorizonMismatch => "horizon_mismatch",
        ModelOutputIgnored::RevisionWithoutPredecessor => "revision_without_predecessor",
    }
}

fn ignored_from(reason: &str) -> Option<ModelOutputIgnored> {
    match reason {
        "not_pinned" => Some(ModelOutputIgnored::NotPinned),
        "model_withdrawn" => Some(ModelOutputIgnored::ModelWithdrawn),
        "output_limits" => Some(ModelOutputIgnored::OutputLimits),
        "not_in_universe" => Some(ModelOutputIgnored::NotInUniverse),
        "direction_not_allowed" => Some(ModelOutputIgnored::DirectionNotAllowed),
        "horizon_mismatch" => Some(ModelOutputIgnored::HorizonMismatch),
        "revision_without_predecessor" => Some(ModelOutputIgnored::RevisionWithoutPredecessor),
        _ => None,
    }
}

/// The order an `IntentProposed` payload records. Shared by the fold and the retry, so an intent
/// handed again is byte-for-byte the one its draft recorded.
pub(crate) fn order_of(payload: &Value) -> Result<crate::types::IntentBody, RuntimeError> {
    let purpose = str_of(payload, "purpose")
        .and_then(purpose_from)
        .ok_or_else(|| non_canonical("purpose"))?;
    let instrument =
        str_of(payload, "instrument_id").ok_or_else(|| non_canonical("instrument_id"))?;
    let side = str_of(payload, "side")
        .and_then(side_from)
        .ok_or_else(|| non_canonical("side"))?;
    let qty = str_of(payload, "qty").ok_or_else(|| non_canonical("qty"))?;
    let limit = str_of(payload, "limit_price").ok_or_else(|| non_canonical("limit_price"))?;
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

/// Why an approval was cancelled without an owner command: the brief's `Superseded` reasons for a
/// version applied and for a mode at exits-only or stricter, whatever caused it.
pub(crate) const REASON_VERSION_APPLIED: &str = "version_applied";
pub(crate) const REASON_MODE_TIGHTENED: &str = "mode_tightened";

/// The one step-up method v0 has, as the control stream spells it (DEC-155 item 4).
const CLI_CONFIRM: &str = "cli_confirm";

/// The step-up evidence a control-stream event carries (DEC-257 item 5): `{assertion_id,
/// authenticated_at, method}` with the instant in whole risk-clock seconds. Anything else, a method
/// v0 does not know included, is no evidence, which every check that needs it refuses and no check
/// that does not needs (`AGENTS.md` rule 3).
pub(crate) fn step_up_of(payload: &Value) -> Option<StepUp> {
    let evidence = payload.get("step_up")?;
    let assertion = str_of(evidence, "assertion_id")?;
    let authenticated_at = clock_of(evidence, "authenticated_at")?;
    let method = match str_of(evidence, "method")? {
        CLI_CONFIRM => StepUpMethod::CliConfirm,
        _ => return None,
    };
    Some(StepUp {
        assertion: AssertionId(assertion.to_owned()),
        authenticated_at: mandate_approval::RiskClock(authenticated_at.secs()),
        method,
    })
}

/// The counterpart of [`step_up_of`], for the copy that records valid evidence.
pub(crate) fn step_up_value(evidence: &StepUp) -> Result<Value, RuntimeError> {
    let method = match evidence.method {
        StepUpMethod::CliConfirm => CLI_CONFIRM,
    };
    object(vec![
        ("assertion_id", text(&evidence.assertion.0)),
        (
            "authenticated_at",
            seconds(
                RiskClock::from_secs(evidence.authenticated_at.0),
                "authenticated_at",
            )?,
        ),
        ("method", text(method)),
    ])
}

/// The prefix a content hash is written with (journal spec §4, `ref`).
const SHA256: &str = "sha256:";

/// `sha256:` and the hex digest, as the request states its content hash and a response repeats it.
pub(crate) fn hash_text(hash: &ContentHash) -> String {
    format!("{SHA256}{}", hash.0)
}

/// A content hash read back from its text, or `None` for text that is not one.
pub(crate) fn hash_from(text: &str) -> Option<ContentHash> {
    text.strip_prefix(SHA256)
        .and_then(Digest::from_hex)
        .map(ContentHash)
}

/// The owner's confirmation an `OwnerExitRequested` recorded, or `None` when the owner confirmed
/// nothing. Its step-up is the assertion id, written as text by an `Input::Command` switch and as
/// the evidence object's `assertion_id` by a copy of an `OwnerCommandIssued` (DEC-257 item 5). A replay needs it so that a flatten handed again after a restart carries the same
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
    let step_up = str_of(payload, "step_up")
        .or_else(|| {
            payload
                .get("step_up")
                .and_then(|e| str_of(e, "assertion_id"))
        })
        .ok_or_else(|| non_canonical("step_up"))?;
    Ok(Some(OwnerConfirmation {
        bid: price_of(bid)?,
        bid_size: qty_of(bid_size)?,
        floor: price_of(floor)?,
        user: user.to_owned(),
        step_up: step_up.to_owned(),
    }))
}

/// The owner's confirmation an `OwnerExitRequested` lets the executor act on: the confirmed bid only
/// when the step-up was valid as the owner committed it. A `stale` or `absent` status records a
/// confirmed bid as given and unlocks nothing (journal spec §9.1, DEC-158 option (c)); a record
/// with no status predates the control stream and carries its confirmation as it always did.
pub(crate) fn honoured_confirmation(
    payload: &Value,
) -> Result<Option<OwnerConfirmation>, RuntimeError> {
    match str_of(payload, "step_up_status") {
        None | Some("valid") => owner_confirmation_of(payload),
        Some(_) => Ok(None),
    }
}
