//! The control-stream records journal spec v0.26 §9.13 closes at schema version 1 (DEC-780):
//! `RecordsAccessed`, `ExportCreated` and `VerificationRun`, with their `range` and
//! `checked_range`, the `stream_id` and `digest` types, and consistency rules 107 to 112, each
//! refusing `schema` at its member. §3's client actor (rules 81 to 83) is [`crate::workspace`]'s:
//! it admits a client's own `RecordsAccessed` and refuses a client on the other two before the
//! payload is read. Every rule only refuses a draft.

use mandate_canon::Value;

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamId, StreamType};

const READ: &str = "RecordsAccessed";
const EXPORT: &str = "ExportCreated";
const VERIFICATION: &str = "VerificationRun";

/// The control-stream event types §9.13 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 3] = [READ, EXPORT, VERIFICATION];

/// The `prev_hash` before seq 1 (§3).
const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The forms an export serves as a view of the canonical export, which name the view's digest
/// (rule 109).
const VIEW_FORMS: [&str; 2] = ["json", "csv"];

/// §11's codes a `checked_range`'s failure names: the checks reported at an event (checks 1 to
/// 6, the anchored head, the anchor's own, the agent stream's two, the hold's, the connection's
/// two, and the operator read's break-glass cause), then [`RANGE_CHECKS`].
const CHECKS: [&str; 19] = [
    "non_canonical",
    "column_mismatch",
    "seq_gap",
    "rehash_mismatch",
    "prev_hash_mismatch",
    "artifact_missing",
    "artifact_mismatch",
    "anchor_head_mismatch",
    "anchor_self_mismatch",
    "intent_action_mismatch",
    "mode_event_mismatch",
    "held_mismatch",
    "connection_lifecycle_mismatch",
    "connection_cause_mismatch",
    "break_glass_cause_mismatch",
    "anchor_root_mismatch",
    "tsa_token_invalid",
    "segment_manifest_mismatch",
    "segment_gap",
];

/// §11's checks reported for a range as a whole, whose failure names no `seq` (rule 111).
const RANGE_CHECKS: [&str; 4] = [
    "anchor_root_mismatch",
    "tsa_token_invalid",
    "segment_manifest_mismatch",
    "segment_gap",
];

/// The §9.13 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (READ, 1, StreamType::Control) => Some(&RECORDS_ACCESSED),
        (EXPORT, 1, StreamType::Control) => Some(&EXPORT_CREATED),
        (VERIFICATION, 1, StreamType::Control) => Some(&VERIFICATION_RUN),
        _ => None,
    }
}

/// Rules 107 to 112 on a payload its schema has normalized, in rule order: the first that fails
/// is reported. `stream`, `actor` and `causation_id` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    payload: &Value,
    stream: &StreamId,
    actor: Option<&Value>,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    if !CONTROL.contains(&event_type) {
        return Ok(());
    }
    let ranges = list(payload, "ranges");
    ranges_rule(ranges, stream)?;
    let kind = text(actor, "kind");
    match event_type {
        READ => {
            ensure(
                text(Some(payload), "accessor") == text(actor, "id"),
                "payload.accessor",
            )?;
            ensure(!matches!(kind, "agent" | "broker"), "actor.kind")?;
            let caused = causation_id.is_some_and(|c| *c != Value::Null);
            ensure(kind != "platform_operator" || caused, "causation_id")?;
            let resources: Vec<&str> = list(payload, "resources")
                .iter()
                .filter_map(Value::as_str)
                .collect();
            ensure(
                resources.windows(2).all(|w| matches!(w, [a, b] if a < b)),
                "payload.resources",
            )
        }
        EXPORT => {
            ensure(matches!(kind, "user" | "system"), "actor.kind")?;
            let view = payload.get("view").is_some_and(|v| *v != Value::Null);
            let form = text(Some(payload), "form");
            ensure(view == VIEW_FORMS.contains(&form), "payload.view")
        }
        _ => {
            let requested = text(Some(payload), "trigger") == "request";
            ensure(
                kind == "system" || (requested && kind == "user"),
                "actor.kind",
            )?;
            checked_ranges_rule(ranges)?;
            let passed = ranges.iter().all(|r| failure(r).is_none());
            let result = text(Some(payload), "result") == "pass";
            ensure(result == passed, "payload.result")
        }
    }
}

/// Rule 107: at least one range, each in this workspace, with `from_seq` at least 1, `to_seq` at
/// least `from_seq`, the genesis hash exactly before seq 1, and each after the first sorting after
/// the one before it by stream, or within one stream starting past its end.
fn ranges_rule(ranges: &[Value], stream: &StreamId) -> Result<(), Invalid> {
    ensure(!ranges.is_empty(), "payload.ranges")?;
    let workspace = stream.as_str().split(':').nth(1);
    let mut previous: Option<(&str, u64)> = None;
    for (i, range) in ranges.iter().enumerate() {
        let at = |member: &str| format!("payload.ranges[{i}].{member}");
        let named = text(Some(range), "stream_id");
        let (from, to) = (seq(range, "from_seq"), seq(range, "to_seq"));
        ensure(named.split(':').nth(1) == workspace, &at("stream_id"))?;
        ensure(from >= 1, &at("from_seq"))?;
        ensure(to >= from, &at("to_seq"))?;
        let genesis = text(Some(range), "prev_hash") == GENESIS;
        ensure(genesis == (from == 1), &at("prev_hash"))?;
        if let Some((before, end)) = previous {
            ensure(
                named > before || (named == before && from > end),
                &at("stream_id"),
            )?;
        }
        previous = Some((named, to));
    }
    Ok(())
}

/// Rule 111: a failure names its `seq` exactly when its check is reported at an event, and that
/// `seq` lies inside its range; a range with no failure names its head.
fn checked_ranges_rule(ranges: &[Value]) -> Result<(), Invalid> {
    for (i, range) in ranges.iter().enumerate() {
        let at = |member: &str| format!("payload.ranges[{i}].{member}");
        if let Some(failure) = failure(range) {
            let at_event = !RANGE_CHECKS.contains(&text(Some(failure), "check"));
            let reported = failure.get("seq").and_then(Value::as_int);
            ensure(reported.is_some() == at_event, &at("failure.seq"))?;
            let inside = reported
                .is_none_or(|s| (seq(range, "from_seq")..=seq(range, "to_seq")).contains(&s));
            ensure(inside, &at("failure.seq"))?;
        } else {
            ensure(
                range.get("to_hash").is_some_and(|h| *h != Value::Null),
                &at("to_hash"),
            )?;
        }
    }
    Ok(())
}

fn failure(range: &Value) -> Option<&Value> {
    range.get("failure").filter(|f| **f != Value::Null)
}

fn text<'a>(value: Option<&'a Value>, member: &str) -> &'a str {
    value
        .and_then(|v| v.get(member))
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn seq(range: &Value, member: &str) -> u64 {
    range
        .get(member)
        .and_then(Value::as_int)
        .unwrap_or_default()
}

fn list<'a>(value: &'a Value, member: &str) -> &'a [Value] {
    value
        .get(member)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

static RANGE: Ty = Ty::Record(&[
    ("stream_id", Ty::StreamName),
    ("from_seq", Ty::Int),
    ("to_seq", Ty::Int),
    ("prev_hash", Ty::Digest),
    ("to_hash", Ty::Digest),
]);

static CHECKED_RANGE: Ty = Ty::Record(&[
    ("stream_id", Ty::StreamName),
    ("from_seq", Ty::Int),
    ("to_seq", Ty::Int),
    ("prev_hash", Ty::Digest),
    ("to_hash", Ty::Nullable(&Ty::Digest)),
    (
        "failure",
        Ty::Nullable(&Ty::Record(&[
            ("check", Ty::OneOf(&CHECKS)),
            ("seq", Ty::Nullable(&Ty::Int)),
        ])),
    ),
]);

static RECORDS_ACCESSED: Ty = Ty::Record(&[
    ("accessor", Ty::Str),
    ("operation", Ty::Ident),
    ("ranges", Ty::List(&RANGE)),
    ("resources", Ty::List(&Ty::Ident)),
    ("result", Ty::Nullable(&Ty::DigestRef)),
]);

static EXPORT_CREATED: Ty = Ty::Record(&[
    ("form", Ty::OneOf(&["canonical", "json", "csv"])),
    ("ranges", Ty::List(&RANGE)),
    ("verifier_digest", Ty::Digest),
    ("view", Ty::Nullable(&Ty::Digest)),
]);

static VERIFICATION_RUN: Ty = Ty::Record(&[
    (
        "trigger",
        Ty::OneOf(&[
            "startup",
            "segment_export",
            "weekly",
            "request",
            "restore_drill",
        ]),
    ),
    ("ranges", Ty::List(&CHECKED_RANGE)),
    ("result", Ty::OneOf(&["pass", "fail"])),
]);
