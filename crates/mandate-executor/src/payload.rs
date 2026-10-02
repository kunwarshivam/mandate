//! Reading and writing canonical payloads (journal spec §4): every decimal is canonical text, every
//! integer is a canonical `Int`, and a missing or mistyped field is a typed refusal, never a guess.

use std::collections::BTreeMap;

use mandate_canon::{Int, Key, Value};
use mandate_num::{Price, Qty, Usd};

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

/// A risk-clock second as the non-negative integer the journal carries.
pub(crate) fn clock(at: RiskClock) -> Result<Value, ExecutorError> {
    int(u64::try_from(at.secs()).map_err(|_| refused("risk_clock"))?)
}

/// The risk clock as journal spec §9.2's `risk_clock` type (DEC-302): a whole-second §4.7
/// timestamp, the form the vectors' `snapshot_fees` draft carries and their
/// `snapshot_risk_clock_as_seconds` draft refuses as integer seconds. The account stream's writer
/// stamps [`clock`]'s integer form on every event today, which `append` refuses once the
/// snapshot's schema registers, so the fee step's own snapshot could never commit (DEC-261
/// item 7).
///
/// # Errors
/// [`ExecutorError::Unimplemented`] in this tests PR; the implementation PR replaces
/// [`crate::batch::Batch`]'s `clock` stamp with this form and deletes the pending pins (DEC-77).
#[allow(
    dead_code,
    reason = "the tests PR ships this stamp and its contract; `batch::journal` calls it in the implementation PR (DEC-77, DEC-83)"
)]
pub(crate) fn risk_clock_stamp(at: RiskClock) -> Result<Value, ExecutorError> {
    let _ = at;
    Err(ExecutorError::Unimplemented { story: "E7-10" })
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
    #[ignore = "pending E7-10"]
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
