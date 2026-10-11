//! The control-stream records journal spec v0.26 §9.13 closes at schema version 1 (DEC-780):
//! `RecordsAccessed`, `ExportCreated` and `VerificationRun`, with their `range` and
//! `checked_range`, the `stream_id` and `digest` types, and consistency rules 107 to 112, each
//! refusing `schema` at its member. §3's client actor (rules 81 to 83) is [`crate::workspace`]'s:
//! it admits a client's own `RecordsAccessed` and refuses a client on the other two before the
//! payload is read. Every rule only refuses a draft.
//!
//! It also holds §9.14's `AnchorComputed` and `SegmentExported` (DEC-783, journal spec v0.27),
//! closed there at schema version 1, with consistency rules 113 to 118.

use mandate_canon::{Digest, Key, Object, Value, to_canonical};

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StreamId, StreamType};

const READ: &str = "RecordsAccessed";
const EXPORT: &str = "ExportCreated";
const VERIFICATION: &str = "VerificationRun";
const ANCHOR: &str = "AnchorComputed";
const SEGMENT: &str = "SegmentExported";

/// The control-stream event types §9.13 and §9.14 add to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 5] = [READ, EXPORT, VERIFICATION, ANCHOR, SEGMENT];

/// DEC-263's six manifest fields, each as its manifest names it and the record member it is read
/// from (rule 117).
const MANIFEST: [(&str, &str); 6] = [
    ("stream", "stream_id"),
    ("first_seq", "first_seq"),
    ("last_seq", "last_seq"),
    ("first_prev_hash", "first_prev_hash"),
    ("last_hash", "last_hash"),
    ("file_sha256", "file_sha256"),
];

/// The `prev_hash` before seq 1 (§3).
const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The forms an export serves as a view of the canonical export, which name the view's digest
/// (rule 109).
const VIEW_FORMS: [&str; 2] = ["json", "csv"];

/// §11's codes a `checked_range`'s failure names: the checks reported at an event (checks 1 to
/// 6, the anchored head, the anchor's own, the agent stream's two, the hold's, the connection's
/// two, and the operator read's break-glass cause), then version 1's four of [`RANGE_CHECKS`].
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

/// §11's checks reported for a range as a whole, whose failure names no `seq` (rule 111):
/// version 1's four, then `segment_rows_mismatch`, which only version 2's list names (DEC-894).
const RANGE_CHECKS: [&str; 5] = [
    "anchor_root_mismatch",
    "tsa_token_invalid",
    "segment_manifest_mismatch",
    "segment_gap",
    "segment_rows_mismatch",
];

/// Version 2's codes: [`CHECKS`], then `segment_rows_mismatch` (DEC-894). Version 1's closed list
/// is not edited (§8).
const CHECKS_V2: [&str; 20] = [
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
    "segment_rows_mismatch",
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
        (VERIFICATION, 2, StreamType::Control) => Some(&VERIFICATION_RUN_V2),
        (ANCHOR, 1, StreamType::Control) => Some(&ANCHOR_COMPUTED),
        (SEGMENT, 1, StreamType::Control) => Some(&SEGMENT_EXPORTED),
        _ => None,
    }
}

/// Rules 107 to 118 on a payload its schema has normalized, in rule order: the first that fails
/// is reported. `stream`, `actor` and `causation_id` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    stream: &StreamId,
    actor: Option<&Value>,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    match event_type {
        ANCHOR => return anchor_rules(payload, stream, actor),
        SEGMENT => return segment_rules(payload, stream, actor),
        _ if !CONTROL.contains(&event_type) => return Ok(()),
        _ => {}
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
            if schema_version == 1 {
                let passed = ranges.iter().all(|r| failure(r).is_none());
                let result = text(Some(payload), "result") == "pass";
                return ensure(result == passed, "payload.result");
            }
            version_2_rule(ranges)?;
            let outcome = if ranges.iter().any(|r| failure(r).is_some()) {
                "fail"
            } else if ranges.iter().any(|r| incomplete(r).is_some()) {
                "incomplete"
            } else {
                "pass"
            };
            ensure(text(Some(payload), "result") == outcome, "payload.result")
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

/// Rule 132: each range's start names its own record and enters where its kind can, `checked`
/// is bounded by the range and is all of it unless the range failed, a failed range records no
/// incomplete check, and an anchor start's range never passes.
fn version_2_rule(ranges: &[Value]) -> Result<(), Invalid> {
    for (i, range) in ranges.iter().enumerate() {
        let at = |member: &str| format!("payload.ranges[{i}].{member}");
        let start = range.get("start");
        let kind = text(start, "kind");
        let named = |member: &str| {
            start
                .and_then(|s| s.get(member))
                .is_some_and(|v| *v != Value::Null)
        };
        ensure(
            named("manifest_hash") == (kind == "manifest"),
            &at("start.manifest_hash"),
        )?;
        ensure(
            named("anchor_event_id") == (kind == "anchor"),
            &at("start.anchor_event_id"),
        )?;
        let from = seq(range, "from_seq");
        let enters = match kind {
            "genesis" => from == 1,
            "anchor" => from >= 2,
            _ => true,
        };
        ensure(enters, &at("start.kind"))?;
        let length = seq(range, "to_seq").saturating_sub(from).saturating_add(1);
        let checked = seq(range, "checked");
        ensure(checked <= length, &at("checked"))?;
        let failed = failure(range).is_some();
        ensure(failed || checked == length, &at("checked"))?;
        let unfinished = incomplete(range).is_some();
        ensure(!(failed && unfinished), &at("incomplete"))?;
        ensure(kind != "anchor" || failed || unfinished, &at("incomplete"))?;
    }
    Ok(())
}

/// Rules 113 to 115: non-empty leaves, each in this workspace with `seq` at least 1 and sorting
/// after the one before it by `stream_id` bytes, one of them the envelope's own stream; the root
/// recomputed over them; and a `system` writer.
fn anchor_rules(payload: &Value, stream: &StreamId, actor: Option<&Value>) -> Result<(), Invalid> {
    let leaves = list(payload, "leaves");
    ensure(!leaves.is_empty(), "payload.leaves")?;
    let workspace = stream.as_str().split(':').nth(1);
    let mut previous: Option<&str> = None;
    for (i, leaf) in leaves.iter().enumerate() {
        let at = |member: &str| format!("payload.leaves[{i}].{member}");
        let named = text(Some(leaf), "stream_id");
        ensure(named.split(':').nth(1) == workspace, &at("stream_id"))?;
        ensure(seq(leaf, "seq") >= 1, &at("seq"))?;
        if let Some(before) = previous {
            ensure(named > before, &at("stream_id"))?;
        }
        previous = Some(named);
    }
    let own = leaves
        .iter()
        .any(|leaf| text(Some(leaf), "stream_id") == stream.as_str());
    ensure(own, "payload.leaves")?;
    let root = anchor_leaves(leaves)
        .as_deref()
        .and_then(crate::merkle::merkle_root)
        .map(|digest| digest.to_hex());
    ensure(
        root.as_deref() == Some(text(Some(payload), "root")),
        "payload.root",
    )?;
    ensure(text(actor, "kind") == "system", "actor.kind")
}

/// The normalized `leaves` as §10's leaves, or `None` when one does not convert.
fn anchor_leaves(leaves: &[Value]) -> Option<Vec<crate::merkle::AnchorLeaf>> {
    leaves
        .iter()
        .map(|leaf| {
            Some(crate::merkle::AnchorLeaf {
                stream_id: leaf.get("stream_id")?.as_str()?.to_owned(),
                seq: leaf.get("seq")?.as_int()?,
                hash: Digest::from_hex(leaf.get("hash")?.as_str()?)?,
            })
        })
        .collect()
}

/// Rules 116 to 118: the segment's stream in this workspace, `first_seq` at least 1, `last_seq`
/// at least `first_seq`, the genesis hash exactly before seq 1; the manifest hash over DEC-263's
/// six fields; and a `system` writer.
fn segment_rules(payload: &Value, stream: &StreamId, actor: Option<&Value>) -> Result<(), Invalid> {
    let workspace = stream.as_str().split(':').nth(1);
    let named = text(Some(payload), "stream_id");
    ensure(named.split(':').nth(1) == workspace, "payload.stream_id")?;
    let (first, last) = (seq(payload, "first_seq"), seq(payload, "last_seq"));
    ensure(first >= 1, "payload.first_seq")?;
    ensure(last >= first, "payload.last_seq")?;
    let genesis = text(Some(payload), "first_prev_hash") == GENESIS;
    ensure(genesis == (first == 1), "payload.first_prev_hash")?;
    let manifest = manifest_hash(payload).map(|digest| digest.to_hex());
    ensure(
        manifest.as_deref() == Some(text(Some(payload), "manifest_hash")),
        "payload.manifest_hash",
    )?;
    ensure(text(actor, "kind") == "system", "actor.kind")
}

/// DEC-263 item 3: the SHA-256 of the canonical JSON of the six manifest fields this record
/// carries, or `None` when one is missing.
pub(crate) fn manifest_hash(payload: &Value) -> Option<Digest> {
    let mut manifest = Object::new();
    for (field, member) in MANIFEST {
        manifest.insert(Key::new(field).ok()?, payload.get(member)?.clone());
    }
    Some(Digest::of(&to_canonical(&Value::Object(manifest))))
}

fn failure(range: &Value) -> Option<&Value> {
    range.get("failure").filter(|f| **f != Value::Null)
}

fn incomplete(range: &Value) -> Option<&Value> {
    range.get("incomplete").filter(|f| **f != Value::Null)
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

static CHECKED_RANGE_V2: Ty = Ty::Record(&[
    ("stream_id", Ty::StreamName),
    ("from_seq", Ty::Int),
    ("to_seq", Ty::Int),
    ("prev_hash", Ty::Digest),
    ("to_hash", Ty::Nullable(&Ty::Digest)),
    (
        "start",
        Ty::Record(&[
            ("kind", Ty::OneOf(&["genesis", "manifest", "anchor"])),
            ("manifest_hash", Ty::Nullable(&Ty::Digest)),
            ("anchor_event_id", Ty::Nullable(&Ty::Ulid)),
        ]),
    ),
    ("checked", Ty::Int),
    (
        "failure",
        Ty::Nullable(&Ty::Record(&[
            ("check", Ty::OneOf(&CHECKS_V2)),
            ("seq", Ty::Nullable(&Ty::Int)),
        ])),
    ),
    (
        "incomplete",
        Ty::Nullable(&Ty::Record(&[
            ("check", Ty::OneOf(&["tsa_token_invalid"])),
            (
                "cause",
                Ty::OneOf(&["token_unverifiable", "anchor_unstamped"]),
            ),
        ])),
    ),
]);

static VERIFICATION_RUN_V2: Ty = Ty::Record(&[
    ("trigger", Ty::OneOf(&TRIGGERS)),
    ("ranges", Ty::List(&CHECKED_RANGE_V2)),
    ("result", Ty::OneOf(&["pass", "incomplete", "fail"])),
]);

const TRIGGERS: [&str; 5] = [
    "startup",
    "segment_export",
    "weekly",
    "request",
    "restore_drill",
];

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
    ("trigger", Ty::OneOf(&TRIGGERS)),
    ("ranges", Ty::List(&CHECKED_RANGE)),
    ("result", Ty::OneOf(&["pass", "fail"])),
]);

static LEAF: Ty = Ty::Record(&[
    ("hash", Ty::Digest),
    ("seq", Ty::Int),
    ("stream_id", Ty::StreamName),
]);

static ANCHOR_COMPUTED: Ty = Ty::Record(&[
    ("leaves", Ty::List(&LEAF)),
    ("root", Ty::Digest),
    ("token", Ty::Nullable(&Ty::DigestRef)),
]);

static SEGMENT_EXPORTED: Ty = Ty::Record(&[
    ("stream_id", Ty::StreamName),
    ("first_seq", Ty::Int),
    ("last_seq", Ty::Int),
    ("first_prev_hash", Ty::Digest),
    ("last_hash", Ty::Digest),
    ("file_sha256", Ty::Digest),
    ("manifest_hash", Ty::Digest),
]);
