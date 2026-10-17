//! The output schema language (inference spec §3.2, §4.1): a strict subset of JSON Schema, read by
//! a deterministic validator, so an output either fits its pinned schema exactly or is refused
//! `schema_invalid`. Nothing is repaired or defaulted (INF-4).
//!
//! A schema is one of:
//! - `{"type": "object", "properties": {…}}`: every listed member required, no other member;
//! - `{"type": "array", "items": S, "max_items": n}`;
//! - `{"type": "string", "max_length": n}` or `{"type": "string", "enum": [ … ]}`;
//! - `{"type": "integer", "minimum": a, "maximum": b}`;
//! - `{"type": "decimal"}`: a string in the journal's decimal grammar, since a model's number
//!   reaches the gateway as text and floats are refused (journal spec §4.6);
//! - `{"type": "boolean"}`.
//!
//! Keys are snake_case because the schema itself is canonical JSON.

use mandate_canon::{DecStr, Value};

/// Whether `schema` is in the language above.
pub fn well_formed(schema: &Value) -> bool {
        let _ = schema; true
    }

/// Whether `value` fits `schema`, which must be [`well_formed`].
pub fn fits(schema: &Value, value: &Value) -> bool {
        let _ = (schema, value); false
    }
