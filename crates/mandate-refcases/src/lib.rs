#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! Reference-case harness (ADR-0001 ES-11). Each case in `fixtures/refcases/*.json` becomes one
//! named test. `status.toml` (founder-owned) marks cases `passing`, which must pass, or `pending`;
//! cases it does not list are pending. Pending cases run only with `--include-ignored`.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use mandate_canon::{Int, Key, Value};

pub mod journal;
pub mod mandate;
pub mod trading_domain;

pub type Json = serde_json::Value;

/// One reference case: a stable ID and the check to run.
pub struct Case {
    pub id: String,
    pub run: Box<dyn FnOnce() -> Result<(), String> + Send>,
}

impl Case {
    pub(crate) fn new(
        id: impl Into<String>,
        run: impl FnOnce() -> Result<(), String> + Send + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            run: Box::new(run),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaseStatus {
    Passing,
    Pending,
}

/// Parses `status.toml`: one table per suite, keyed by case ID within the suite, each value
/// `{ status = "passing" | "pending", story = "<story ID>" }`. Returns full case IDs
/// (`<suite>::<case>`).
pub fn parse_status(text: &str) -> Result<BTreeMap<String, CaseStatus>, String> {
    let table: toml::Table = text.parse().map_err(|e| format!("status.toml: {e}"))?;
    let mut out = BTreeMap::new();
    for (suite, cases) in &table {
        let cases = cases
            .as_table()
            .ok_or_else(|| format!("status.toml: `{suite}` must be a table"))?;
        for (case, entry) in cases {
            let id = format!("{suite}::{case}");
            let status = match entry.get("status").and_then(toml::Value::as_str) {
                Some("passing") => CaseStatus::Passing,
                Some("pending") => CaseStatus::Pending,
                _ => {
                    return Err(format!(
                        "status.toml: `{id}` needs status = \"passing\" or \"pending\""
                    ));
                }
            };
            if entry
                .get("story")
                .and_then(toml::Value::as_str)
                .is_none_or(str::is_empty)
            {
                return Err(format!("status.toml: `{id}` needs the story that owns it"));
            }
            out.insert(id, status);
        }
    }
    Ok(out)
}

pub fn read_fixture(dir: &Path, name: &str) -> Result<Arc<Json>, String> {
    let path = dir.join(name);
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_str(&text)
        .map(Arc::new)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Converts fixture JSON to a canonical value without going through the canonical parser.
pub fn to_canon(value: &Json) -> Result<Value, String> {
    Ok(match value {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => Value::Int(
            n.as_u64()
                .and_then(Int::new)
                .ok_or_else(|| format!("fixture number {n} is not a canonical integer"))?,
        ),
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(items) => Value::Array(items.iter().map(to_canon).collect::<Result<_, _>>()?),
        Json::Object(members) => Value::Object(
            members
                .iter()
                .map(|(k, v)| {
                    let key =
                        Key::new(k).map_err(|_| format!("fixture key `{k}` is not canonical"))?;
                    Ok((key, to_canon(v)?))
                })
                .collect::<Result<_, String>>()?,
        ),
    })
}

/// The member at a dotted path (`a.b.c`).
pub(crate) fn at<'a>(value: &'a Json, path: &str) -> Result<&'a Json, String> {
    path.split('.').try_fold(value, |v, key| {
        v.get(key).ok_or_else(|| format!("fixture has no `{path}`"))
    })
}

pub(crate) fn str_at<'a>(value: &'a Json, path: &str) -> Result<&'a str, String> {
    at(value, path)?
        .as_str()
        .ok_or_else(|| format!("fixture `{path}` is not a string"))
}

pub(crate) fn u64_at(value: &Json, path: &str) -> Result<u64, String> {
    at(value, path)?
        .as_u64()
        .ok_or_else(|| format!("fixture `{path}` is not an integer"))
}

pub(crate) fn list_at<'a>(value: &'a Json, path: &str) -> Result<&'a [Json], String> {
    at(value, path)?
        .as_array()
        .map(Vec::as_slice)
        .ok_or_else(|| format!("fixture `{path}` is not a list"))
}

pub(crate) fn ensure(ok: bool, message: impl FnOnce() -> String) -> Result<(), String> {
    if ok { Ok(()) } else { Err(message()) }
}

pub(crate) fn expect_eq<T: PartialEq + std::fmt::Debug>(
    what: &str,
    actual: T,
    expected: T,
) -> Result<(), String> {
    ensure(actual == expected, || {
        format!("{what}: expected {expected:?}, got {actual:?}")
    })
}
