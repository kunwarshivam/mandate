//! Reading and writing canonical payloads (journal spec §4): every decimal is canonical text, every
//! integer is a canonical `Int`, and a missing or mistyped field is a typed refusal, never a guess.

use std::collections::BTreeMap;

use mandate_canon::{Int, Key, Value};
use mandate_num::{Price, Qty};

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
