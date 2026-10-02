//! Reading and writing canonical payloads (journal spec §4): every decimal is canonical text, every
//! integer is a canonical `Int`, and a missing or mistyped field is a typed refusal, never a guess.

use std::collections::BTreeMap;

use mandate_canon::{Int, Key, Value};
use mandate_num::{Price, Qty, Usd};
use mandate_time::UtcNanos;

use crate::error::ExecutorError;
use crate::types::RiskClock;

/// Builds an object from `(key, value)` pairs. Every key this crate writes is a literal of the
/// journal's key grammar, so a refusal here is a defect in this crate, reported rather than hidden.
pub(crate) fn object(pairs: Vec<(&str, Value)>) -> Result<Value, ExecutorError> {
    let mut fields = BTreeMap::new();
    for (name, value) in pairs {
        let key = Key::new(name).map_err(|_| refused(name))?;
        fields.insert(key, value);
    }
    Ok(Value::Object(fields))
}

pub(crate) fn text(value: impl Into<String>) -> Value {
    Value::Str(value.into())
}

pub(crate) fn int(value: u64) -> Result<Value, ExecutorError> {
    Int::new(value)
        .map(Value::Int)
        .ok_or_else(|| refused("int"))
}

/// A risk-clock second as integer seconds, the form the account stream wrote before DEC-306 and
/// the golden journal still carries. The fold reads it ([`clock_of`]); nothing in the crate writes
/// it any more, so only the in-crate tests' hand-built payloads use it.
#[cfg(test)]
pub(crate) fn clock(at: RiskClock) -> Result<Value, ExecutorError> {
    int(u64::try_from(at.secs()).map_err(|_| refused("risk_clock"))?)
}

/// The risk clock as journal spec §9.2's `risk_clock` type (DEC-302): a whole-second §4.7
/// timestamp, the form the vectors' `snapshot_fees` draft carries and their
/// `snapshot_risk_clock_as_seconds` draft refuses as integer seconds. [`crate::batch::Batch`]
/// stamps it on every account-stream event (DEC-306 item 2).
///
/// # Errors
/// [`ExecutorError::NonCanonicalPayload`] for a second outside §4.7's range, which a risk clock
/// read from the journal never is.
pub(crate) fn risk_clock_stamp(at: RiskClock) -> Result<Value, ExecutorError> {
    let instant = UtcNanos::from_parts(at.secs(), 0).map_err(|_| refused("risk_clock"))?;
    Ok(text(instant.to_string()))
}

/// The risk-clock second a payload's `field` carries, in either form the account stream has
/// written: [`risk_clock_stamp`]'s whole-second timestamp, or the integer seconds written before
/// it (the golden journal's). A timestamp off the second, a non-canonical one, or anything else is
/// `None`, so an unreadable clock is refused rather than rounded (DEC-390).
pub(crate) fn clock_of(payload: &Value, field: &str) -> Option<RiskClock> {
    let secs = match payload.get(field)? {
        Value::Str(stamp) => UtcNanos::parse(stamp)
            .ok()
            .filter(|instant| instant.nanos() == 0)
            .map(UtcNanos::secs),
        other => other.as_int().and_then(|secs| i64::try_from(secs).ok()),
    };
    secs.map(RiskClock::from_secs)
}

fn refused(field: &str) -> ExecutorError {
    ExecutorError::NonCanonicalPayload {
        field: field.to_owned(),
    }
}

pub(crate) fn optional_text<'a>(payload: &'a Value, field: &str) -> Option<&'a str> {
    payload.get(field).and_then(Value::as_str)
}

pub(crate) fn required_text<'a>(payload: &'a Value, field: &str) -> Result<&'a str, ExecutorError> {
    optional_text(payload, field).ok_or_else(|| refused(field))
}

pub(crate) fn optional_int(payload: &Value, field: &str) -> Option<u64> {
    payload.get(field).and_then(Value::as_int)
}

pub(crate) fn flag(payload: &Value, field: &str) -> bool {
    payload.get(field) == Some(&Value::Bool(true))
}

pub(crate) fn qty(payload: &Value, field: &str) -> Result<Qty, ExecutorError> {
    Ok(Qty::parse(required_text(payload, field)?)?)
}

pub(crate) fn optional_qty(payload: &Value, field: &str) -> Result<Option<Qty>, ExecutorError> {
    optional_text(payload, field)
        .map(Qty::parse)
        .transpose()
        .map_err(ExecutorError::from)
}

pub(crate) fn optional_price(payload: &Value, field: &str) -> Result<Option<Price>, ExecutorError> {
    optional_text(payload, field)
        .map(Price::parse)
        .transpose()
        .map_err(ExecutorError::from)
}

pub(crate) fn usd(payload: &Value, field: &str) -> Result<Usd, ExecutorError> {
    Ok(Usd::parse(required_text(payload, field)?)?)
}

pub(crate) fn optional_usd(payload: &Value, field: &str) -> Result<Option<Usd>, ExecutorError> {
    optional_text(payload, field)
        .map(Usd::parse)
        .transpose()
        .map_err(ExecutorError::from)
}

#[cfg(test)]
mod stamp_tests {
    use serde_json::Value as Json;

    use super::risk_clock_stamp;
    use crate::error::ExecutorError;
    use crate::types::RiskClock;

    /// The journal vectors (`fixtures/refcases/journal.json`, generated from
    /// `docs/specs/reference-cases/journal.yaml`): the fee-step snapshot's whole-second
    /// `risk_clock`, and the integer seconds the vectors refuse as that member's form.
    fn vector_risk_clock() -> Result<(String, i64), ExecutorError> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/refcases/journal.json"
        );
        let text = std::fs::read_to_string(path).map_err(|_| non_canonical())?;
        let fixture: Json = serde_json::from_str(&text).map_err(|_| non_canonical())?;
        let stamp = fixture
            .pointer("/control_stream/drafts/snapshot_fees/payload/risk_clock")
            .and_then(Json::as_str)
            .ok_or_else(non_canonical)?
            .to_owned();
        let seconds = fixture
            .pointer("/control_stream/invalid_drafts")
            .and_then(Json::as_array)
            .ok_or_else(non_canonical)?
            .iter()
            .find(|draft| {
                draft.get("name").and_then(Json::as_str) == Some("snapshot_risk_clock_as_seconds")
            })
            .ok_or_else(non_canonical)?
            .pointer("/changes/0/value")
            .and_then(Json::as_i64)
            .ok_or_else(non_canonical)?;
        Ok((stamp, seconds))
    }

    fn non_canonical() -> ExecutorError {
        ExecutorError::NonCanonicalPayload {
            field: "journal fixture".to_owned(),
        }
    }

    /// The executor's `risk_clock` stamp is the vectors' whole-second timestamp, and the seconds
    /// they refuse are that same instant: one pin, both halves read from the fixture, so a vector
    /// change re-reads rather than passing silently.
    #[test]
    fn risk_clock_stamps_the_whole_second_the_vectors_refuse_as_seconds()
    -> Result<(), ExecutorError> {
        let (stamp, seconds) = vector_risk_clock()?;
        let stamped = risk_clock_stamp(RiskClock::from_secs(seconds))?;
        assert_eq!(
            stamped,
            mandate_canon::Value::Str(stamp.clone()),
            "the account stream's risk_clock is the whole-second timestamp {stamp}, never the \
             integer seconds the vectors refuse (journal spec §9.2, DEC-302)"
        );
        Ok(())
    }
}
