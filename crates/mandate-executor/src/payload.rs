//! Reading and writing canonical payloads (journal spec §4): every decimal is canonical text, every
//! integer is a canonical `Int`, and a missing or mistyped field is a typed refusal, never a guess.

use mandate_canon::Value;

use crate::error::ExecutorError;

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
