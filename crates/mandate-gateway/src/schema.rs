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
    let Some(members) = schema.as_object() else {
        return false;
    };
    let kind = schema.get("type").and_then(Value::as_str);
    let only = |names: &[&str]| members.keys().all(|k| names.contains(&k.as_str()));
    match kind {
        Some("object") => {
            only(&["type", "properties"])
                && schema
                    .get("properties")
                    .and_then(Value::as_object)
                    .is_some_and(|p| p.values().all(well_formed))
        }
        Some("array") => {
            only(&["type", "items", "max_items"])
                && schema.get("items").is_some_and(well_formed)
                && schema.get("max_items").and_then(Value::as_int).is_some()
        }
        Some("string") => {
            let bounded = schema.get("max_length").and_then(Value::as_int).is_some();
            let listed = schema
                .get("enum")
                .and_then(Value::as_array)
                .is_some_and(|e| !e.is_empty() && e.iter().all(|v| v.as_str().is_some()));
            only(&["type", "max_length", "enum"]) && (bounded != listed)
        }
        Some("integer") => {
            only(&["type", "minimum", "maximum"])
                && matches!(
                    (schema.get("minimum").and_then(Value::as_int), schema.get("maximum").and_then(Value::as_int)),
                    (Some(lo), Some(hi)) if lo <= hi
                )
        }
        Some("decimal" | "boolean") => only(&["type"]),
        _ => false,
    }
}

/// Whether `value` fits `schema`, which must be [`well_formed`].
pub fn fits(schema: &Value, value: &Value) -> bool {
    match schema.get("type").and_then(Value::as_str) {
        Some("object") => {
            let (Some(properties), Some(members)) = (
                schema.get("properties").and_then(Value::as_object),
                value.as_object(),
            ) else {
                return false;
            };
            properties.len() == members.len()
                && properties
                    .iter()
                    .all(|(name, inner)| members.get(name).is_some_and(|v| fits(inner, v)))
        }
        Some("array") => {
            let (Some(items), Some(bound), Some(list)) = (
                schema.get("items"),
                schema.get("max_items").and_then(Value::as_int),
                value.as_array(),
            ) else {
                return false;
            };
            u64::try_from(list.len()).is_ok_and(|n| n <= bound)
                && list.iter().all(|v| fits(items, v))
        }
        Some("string") => {
            let Some(text) = value.as_str() else {
                return false;
            };
            match (
                schema.get("max_length").and_then(Value::as_int),
                schema.get("enum").and_then(Value::as_array),
            ) {
                (Some(bound), None) => {
                    u64::try_from(text.chars().count()).is_ok_and(|n| n <= bound)
                }
                (None, Some(options)) => options.iter().any(|o| o.as_str() == Some(text)),
                _ => false,
            }
        }
        Some("integer") => {
            match (
                value.as_int(),
                schema.get("minimum").and_then(Value::as_int),
                schema.get("maximum").and_then(Value::as_int),
            ) {
                (Some(n), Some(lo), Some(hi)) => lo <= n && n <= hi,
                _ => false,
            }
        }
        Some("decimal") => value
            .as_str()
            .is_some_and(|text| DecStr::parse(text).is_ok_and(|d| d.as_str() == text)),
        Some("boolean") => matches!(value, Value::Bool(_)),
        _ => false,
    }
}
