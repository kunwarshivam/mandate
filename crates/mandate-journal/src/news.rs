//! The news stream journal spec v0.97 §9.37 adds (DEC-2278): `news:{workspace_id}`, written by the
//! workspace's news reader alone, with its `StreamOpened` and `NewsItemStored`, one record per news
//! item the reader stored, each at schema version 1. Rule 283 judges one record and rule 286 its
//! writer and its opening's subject; rules 284 and 285 are stream rules, [`stream_rule`]'s, which
//! [`crate::check_stream`] runs with the stream's earlier records. Every rule only refuses a draft.
//!
//! The stream is never a risk input: no gate, limit, or fold of the account or the mandate reads
//! it, and no record on another stream copies from it, so it maps to no `JournaledFact`.

use std::collections::BTreeSet;

use mandate_canon::Value;
use mandate_time::UtcNanos;

use crate::schema::Ty;
use crate::{Draft, Invalid, InvalidReason, StreamId, StreamType};

const OPENED: &str = "StreamOpened";
const STORED: &str = "NewsItemStored";

/// The research core's own item ids begin with these, so a news item's id never does (rule 283).
const RESERVED_PREFIXES: [&str; 3] = ["bar:", "quote:", "instrument:"];

/// The most tickers one record names (rule 283).
const MAX_SYMBOLS: usize = 32;

/// The longest item id, in characters of `[A-Za-z0-9._:-]` (rule 283).
const MAX_ITEM_ID: usize = 128;

/// Whether §9.37 governs records on `stream`: every record of the news stream.
pub(crate) fn governs(stream: &StreamId) -> bool {
    stream.stream_type() == StreamType::News
}

/// The §9.37 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (OPENED, 1, StreamType::News) => Some(&STREAM_OPENED),
        (STORED, 1, StreamType::News) => Some(&NEWS_ITEM_STORED),
        _ => None,
    }
}

/// Rule 283 on a `NewsItemStored` payload its schema has normalized, its clauses in the spec's
/// order (tickers, then the item id, then the known time), then rule 286's writer on every record
/// of the stream: the first that fails is reported. `actor` is the envelope's.
pub(crate) fn rules(
    event_type: &str,
    stream: StreamType,
    payload: &Value,
    actor: Option<&Value>,
) -> Result<(), Invalid> {
    if stream != StreamType::News {
        return Ok(());
    }
    if event_type == STORED {
        let symbols = texts(payload, "symbols");
        ensure(
            (1..=MAX_SYMBOLS).contains(&symbols.len())
                && symbols.windows(2).all(|w| matches!(w, [a, b] if a < b)),
            "payload.symbols",
        )?;
        ensure(item_id(text(payload, "item_id")), "payload.item_id")?;
        ensure(
            instant(payload, "knowledge_at") >= instant(payload, "published_at"),
            "payload.knowledge_at",
        )?;
    }
    let writer = actor
        .and_then(|a| a.get("kind"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    ensure(writer == "system", "actor.kind")
}

/// Rule 286 (`stream_mismatch`): the news stream's `StreamOpened` names its own stream.
pub(crate) fn subject(event_type: &str, stream: &StreamId, payload: &Value) -> Result<(), Invalid> {
    if event_type != OPENED || stream.stream_type() != StreamType::News {
        return Ok(());
    }
    if format!("news:{}", text(payload, "workspace_id")) == stream.as_str() {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::StreamMismatch, "stream_id"))
    }
}

/// Rules 284 and 285 on `draft`, in that order, given `before`, the same stream's earlier records:
/// no earlier `NewsItemStored` names its `item_id`, and its `knowledge_at` is not before the
/// previous one's.
pub(crate) fn stream_rule(before: &[&Draft], draft: &Draft) -> Result<(), Invalid> {
    if draft.event_type() != STORED || draft.stream_id().stream_type() != StreamType::News {
        return Ok(());
    }
    let mine = payload_of(draft);
    let stored: Vec<Value> = before
        .iter()
        .filter(|d| d.event_type() == STORED)
        .map(|d| payload_of(d))
        .collect();
    let ids: BTreeSet<&str> = stored.iter().map(|p| text(p, "item_id")).collect();
    ensure(!ids.contains(text(&mine, "item_id")), "payload.item_id")?;
    let last = stored.last().map(|p| instant(p, "knowledge_at"));
    ensure(
        last.is_none_or(|last| instant(&mine, "knowledge_at") >= last),
        "payload.knowledge_at",
    )
}

/// The item id's grammar, `[A-Za-z0-9._:-]{1,128}`, and none of the research core's prefixes.
fn item_id(id: &str) -> bool {
    (1..=MAX_ITEM_ID).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b':' | b'-'))
        && !RESERVED_PREFIXES
            .iter()
            .any(|prefix| id.starts_with(prefix))
}

fn payload_of(draft: &Draft) -> Value {
    draft
        .fields()
        .get("payload")
        .cloned()
        .unwrap_or(Value::Null)
}

fn text<'a>(value: &'a Value, member: &str) -> &'a str {
    value
        .get(member)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn texts<'a>(value: &'a Value, member: &str) -> Vec<&'a str> {
    value
        .get(member)
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect()
}

/// A timestamp member the schema has already checked, as an instant; `None`, before every instant,
/// when it is absent, which a well-typed payload never is.
fn instant(value: &Value, member: &str) -> Option<UtcNanos> {
    UtcNanos::parse(text(value, member)).ok()
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

static STREAM_OPENED: Ty = Ty::Record(&[
    ("stream_type", Ty::OneOf(&["news"])),
    ("workspace_id", Ty::Ident),
]);

static NEWS_ITEM_STORED: Ty = Ty::Record(&[
    ("source", Ty::Str),
    ("allowlist_version", Ty::Int),
    ("item_id", Ty::Str),
    ("symbols", Ty::List(&Ty::Str)),
    ("published_at", Ty::Timestamp),
    ("knowledge_at", Ty::Timestamp),
    ("item_ref", Ty::DigestRef),
    ("licensed", Ty::Bool),
]);
