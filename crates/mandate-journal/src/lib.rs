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
//! The append-only, hash-chained journal ([journal spec](../../../docs/specs/journal.md), backlog
//! E5-1): draft validation (§3, §4, §9), the append protocol (§5.1), verification (§11), Merkle
//! anchoring (§10), export lines (§6.2), and content-addressed artifacts (§6.3, E5-2). Storage
//! here is in memory; the Postgres store (E5-3) implements the same protocol over the same `Draft`
//! and `seal`, and `mandate-artifacts-fs` stores artifacts on disk.

use std::collections::BTreeMap;
use std::fmt;

use mandate_canon::{Digest, Int, Key, ParseErrorKind, Value, parse, to_canonical};
use mandate_time::UtcNanos;

mod agent;
mod artifact;
mod catalogue;
mod control;
mod draft;
mod merkle;
mod schema;
mod verify;

pub use agent::{AgentStreamCheck, AgentStreamFailure, verify_agent_stream};

/// A batch's cross-draft checks: §9.1's rule 10 clause on the agent stream, then §9.5's rule 45
/// on the account stream (DEC-446 item 3). Each draft has already passed `Draft::parse`.
pub fn check_batch(drafts: &[Draft]) -> Result<(), (usize, Invalid)> {
    agent::check_batch(drafts)?;
    control::check_batch(drafts)
}
pub use artifact::{
    ArtifactError, ArtifactRef, ArtifactSource, ArtifactStore, check_artifact, get_artifact,
};
pub use draft::Draft;
pub use merkle::{Anchor, AnchorLeaf, merkle_root, tsa_imprint};
pub use verify::{
    EventCheck, EventFailure, RangeCheck, TrustedStart, Verified, verify_anchor, verify_events,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Environment {
    Paper,
    Live,
    Backtest,
}

impl Environment {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Paper => "paper",
            Self::Live => "live",
            Self::Backtest => "backtest",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "paper" => Some(Self::Paper),
            "live" => Some(Self::Live),
            "backtest" => Some(Self::Backtest),
            _ => None,
        }
    }
}

/// The four stream types of journal spec §2.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamType {
    Account,
    Agent,
    Control,
    Scheduler,
}

/// `acct:{workspace_id}:{account_ref}`, `agent:{workspace_id}:{agent_id}`, `ctl:{workspace_id}`,
/// or `clock:{workspace_id}`, each segment `[A-Za-z0-9_-]+` (journal spec §2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StreamId {
    text: String,
    stream_type: StreamType,
}

impl StreamId {
    pub fn parse(s: &str) -> Option<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        let stream_type = match parts.as_slice() {
            ["acct", _, _] => StreamType::Account,
            ["agent", _, _] => StreamType::Agent,
            ["ctl", _] => StreamType::Control,
            ["clock", _] => StreamType::Scheduler,
            _ => return None,
        };
        parts
            .iter()
            .skip(1)
            .all(|segment| schema::is_ident(segment))
            .then(|| Self {
                text: s.to_owned(),
                stream_type,
            })
    }

    pub fn as_str(&self) -> &str {
        &self.text
    }

    pub fn stream_type(&self) -> StreamType {
        self.stream_type
    }
}

impl fmt::Display for StreamId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.text)
    }
}

/// Why a draft or batch was rejected (`Invalid`, journal spec §5.1), and where.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{reason} at `{path}`")]
pub struct Invalid {
    pub reason: InvalidReason,
    /// Dotted path of the offending field; empty for the whole draft or batch.
    pub path: String,
}

impl Invalid {
    pub(crate) fn new(reason: InvalidReason, path: impl Into<String>) -> Self {
        Self {
            reason,
            path: path.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidReason {
    #[error("malformed JSON ({0})")]
    Json(ParseErrorKind),
    #[error("missing, unknown, or mistyped field")]
    Schema,
    #[error("value outside its canonical grammar")]
    NonCanonical,
    #[error("a config_refs key required for this event type is missing")]
    MissingConfigRef,
    #[error("the referenced configuration artifact is not stored")]
    MissingArtifact,
    #[error("the referenced configuration artifact has another kind")]
    ConfigRefKind,
    #[error("the referenced configuration artifact does not match the event payload")]
    ConfigRefMismatch,
    #[error("event type not in the catalogue")]
    UnknownEventType,
    #[error("no payload schema registered for this event type and schema version")]
    UnknownSchema,
    #[error("event type not allowed in this stream type")]
    WrongStream,
    #[error("stream_id differs from the append's stream or the StreamOpened subject")]
    StreamMismatch,
    #[error("environment differs from the stream's StreamOpened")]
    EnvironmentMismatch,
    #[error("seq 1 must be StreamOpened")]
    NotStreamOpened,
    #[error("StreamOpened is only allowed at seq 1")]
    StreamAlreadyOpened,
    #[error("the batch repeats an event_id")]
    DuplicateEventId,
    #[error("the batch has no drafts")]
    EmptyBatch,
    #[error("artifact_refs is not the sorted set of sha256 references in the payload")]
    ArtifactRefs,
    #[error("pii_refs is not a sorted set")]
    PiiRefs,
    #[error("risk_clock is earlier than the stream's last risk_clock")]
    RiskClockRegressed,
}

impl InvalidReason {
    /// Stable reason code (ADR-0001 ES-09); JSON errors use the parser's code, such as `float`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Json(kind) => kind.code(),
            Self::Schema => "schema",
            Self::NonCanonical => "non_canonical",
            Self::MissingConfigRef => "missing_config_ref",
            Self::MissingArtifact => "missing_artifact",
            Self::ConfigRefKind => "config_ref_kind",
            Self::ConfigRefMismatch => "config_ref_mismatch",
            Self::UnknownEventType => "unknown_event_type",
            Self::UnknownSchema => "unknown_schema",
            Self::WrongStream => "wrong_stream",
            Self::StreamMismatch => "stream_mismatch",
            Self::EnvironmentMismatch => "environment_mismatch",
            Self::NotStreamOpened => "not_stream_opened",
            Self::StreamAlreadyOpened => "stream_already_opened",
            Self::DuplicateEventId => "duplicate_event_id",
            Self::EmptyBatch => "empty_batch",
            Self::ArtifactRefs => "artifact_refs",
            Self::PiiRefs => "pii_refs",
            Self::RiskClockRegressed => "risk_clock_regressed",
        }
    }
}

/// One row of the hot store (journal spec §6.1): the columns, the hash, and the exact canonical
/// body bytes that were hashed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    pub stream_id: String,
    pub seq: u64,
    pub event_id: String,
    pub event_type: String,
    pub schema_version: u64,
    pub environment: String,
    pub recorded_at: String,
    pub prev_hash: Digest,
    pub hash: Digest,
    pub body: Vec<u8>,
}

/// Fields the journal assigns at append; the rest of the body is the writer's draft (spec §3).
const JOURNAL_FIELDS: [&str; 3] = ["prev_hash", "recorded_at", "seq"];

/// Adds the journal-assigned fields to `draft` and hashes the canonical body.
pub fn seal(
    draft: &Draft,
    seq: u64,
    prev_hash: Digest,
    recorded_at: UtcNanos,
) -> Result<StoredEvent, Invalid> {
    let seq_value = Int::new(seq)
        .filter(|_| seq >= 1)
        .ok_or_else(|| Invalid::new(InvalidReason::NonCanonical, "seq"))?;
    let key = |name: &str| Key::new(name).map_err(|_| Invalid::new(InvalidReason::Schema, name));
    let mut body = draft.fields().clone();
    body.insert(key("seq")?, Value::Int(seq_value));
    body.insert(key("prev_hash")?, Value::Str(prev_hash.to_hex()));
    body.insert(key("recorded_at")?, Value::Str(recorded_at.to_string()));
    let body = to_canonical(&Value::Object(body));
    Ok(StoredEvent {
        stream_id: draft.stream_id().as_str().to_owned(),
        seq,
        event_id: draft.event_id().to_owned(),
        event_type: draft.event_type().to_owned(),
        schema_version: draft.schema_version(),
        environment: draft.environment().as_str().to_owned(),
        recorded_at: recorded_at.to_string(),
        prev_hash,
        hash: Digest::of(&body),
        body,
    })
}

/// The canonical draft inside a stored body: the body without the journal-assigned fields.
fn stored_draft(row: &StoredEvent) -> Option<Vec<u8>> {
    let Value::Object(mut body) = parse(&row.body).ok()? else {
        return None;
    };
    for field in JOURNAL_FIELDS {
        body.remove(field)?;
    }
    Some(to_canonical(&Value::Object(body)))
}

/// The export line for one event (journal spec §6.2): the canonical form of
/// `{"body": <body>, "hash": "<hex>"}`, without the line feed. `"body"` sorts before `"hash"`, so
/// the body bytes are an exact slice of the line.
pub fn export_line(row: &StoredEvent) -> Vec<u8> {
    let mut line = Vec::with_capacity(row.body.len().saturating_add(84));
    line.extend_from_slice(b"{\"body\":");
    line.extend_from_slice(&row.body);
    line.extend_from_slice(b",\"hash\":\"");
    line.extend_from_slice(row.hash.to_hex().as_bytes());
    line.extend_from_slice(b"\"}");
    line
}

/// A segment file: one export line per event, each followed by a line feed.
pub fn export_segment(rows: &[StoredEvent]) -> Vec<u8> {
    rows.iter()
        .flat_map(|row| {
            let mut line = export_line(row);
            line.push(b'\n');
            line
        })
        .collect()
}

/// The result of an append (journal spec §5.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppendOutcome {
    Committed(Vec<StoredEvent>),
    /// Every draft was already stored with identical content; the stored events, in batch order.
    AlreadyCommitted(Vec<StoredEvent>),
    HeadMismatch {
        actual_seq: u64,
        actual_hash: Digest,
    },
    /// A stored event has the same `event_id` with different content, or the batch partly overlaps
    /// stored events; `stored_seq` is the first such stored event.
    IdempotencyConflict {
        stored_seq: u64,
    },
    Fenced {
        current_epoch: u64,
    },
    Invalid {
        draft: usize,
        error: Invalid,
    },
    /// Storage adapters only: nothing was written; retry with the same drafts.
    Unavailable,
    /// Storage adapters only: the writer must re-query by `event_id` before acting.
    Ambiguous,
}

impl AppendOutcome {
    /// The outcome's name as the spec and test vectors write it.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Committed(_) => "Committed",
            Self::AlreadyCommitted(_) => "AlreadyCommitted",
            Self::HeadMismatch { .. } => "HeadMismatch",
            Self::IdempotencyConflict { .. } => "IdempotencyConflict",
            Self::Fenced { .. } => "Fenced",
            Self::Invalid { .. } => "Invalid",
            Self::Unavailable => "Unavailable",
            Self::Ambiguous => "Ambiguous",
        }
    }
}

/// A stream's head: the last `seq` (0 when empty), its hash (64 zeros when empty), and the current
/// writer epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Head {
    pub seq: u64,
    pub hash: Digest,
    pub writer_epoch: u64,
}

#[derive(Debug, Default)]
struct StreamState {
    rows: Vec<StoredEvent>,
    epoch: u64,
    /// The latest `risk_clock` appended, which later risk inputs may not undercut (spec §2).
    risk_clock: Option<UtcNanos>,
}

enum ConfigArtifactFailure {
    Invalid(Invalid),
    Unavailable,
}

fn text_at<'a>(draft: &'a Draft, object: &str, member: &str) -> Option<&'a str> {
    draft
        .fields()
        .get(object)
        .and_then(Value::as_object)
        .and_then(|value| value.get(member))
        .and_then(Value::as_str)
}

fn referenced_config_object(
    artifacts: &dyn ArtifactSource,
    reference_text: Option<&str>,
    expected_kind: &str,
    reference_path: &str,
    kind_path: &str,
) -> Result<Value, ConfigArtifactFailure> {
    let reference = reference_text.and_then(ArtifactRef::parse).ok_or_else(|| {
        ConfigArtifactFailure::Invalid(Invalid::new(
            InvalidReason::ConfigRefMismatch,
            reference_path,
        ))
    })?;
    let bytes = match artifacts.read_artifact(&reference) {
        Ok(bytes) => bytes,
        Err(ArtifactError::Missing) => {
            return Err(ConfigArtifactFailure::Invalid(Invalid::new(
                InvalidReason::MissingArtifact,
                reference_path,
            )));
        }
        Err(ArtifactError::Corrupt | ArtifactError::Unavailable) => {
            return Err(ConfigArtifactFailure::Unavailable);
        }
    };
    if check_artifact(&reference, &bytes).is_err() {
        return Err(ConfigArtifactFailure::Unavailable);
    }
    let object = parse(&bytes).map_err(|_| {
        ConfigArtifactFailure::Invalid(Invalid::new(
            InvalidReason::ConfigRefMismatch,
            reference_path,
        ))
    })?;
    if to_canonical(&object) != bytes {
        return Err(ConfigArtifactFailure::Invalid(Invalid::new(
            InvalidReason::ConfigRefMismatch,
            reference_path,
        )));
    }
    if object.get("kind").and_then(Value::as_str) != Some(expected_kind) {
        return Err(ConfigArtifactFailure::Invalid(Invalid::new(
            InvalidReason::ConfigRefKind,
            kind_path,
        )));
    }
    Ok(object)
}

fn validate_config_artifacts(
    draft: &Draft,
    artifacts: &dyn ArtifactSource,
) -> Result<(), ConfigArtifactFailure> {
    if draft.schema_version() != 2 {
        return Ok(());
    }
    match draft.event_type() {
        "ModelOutputRecorded" => {
            let registry = referenced_config_object(
                artifacts,
                text_at(draft, "config_refs", catalogue::REG),
                catalogue::REG,
                "config_refs.model_registry",
                "config_refs.model_registry",
            )?;
            let payload = draft.fields().get("payload").unwrap_or(&Value::Null);
            let matches = registry
                .get("models")
                .and_then(Value::as_array)
                .map(|models| {
                    models
                        .iter()
                        .filter(|model| {
                            ["model_id", "model_version", "content_hash"]
                                .iter()
                                .all(|member| model.get(member) == payload.get(member))
                        })
                        .count()
                })
                .unwrap_or_default();
            if matches != 1 {
                return Err(ConfigArtifactFailure::Invalid(Invalid::new(
                    InvalidReason::ConfigRefMismatch,
                    "payload.model_version",
                )));
            }
        }
        "DecisionMade" => {
            referenced_config_object(
                artifacts,
                text_at(draft, "config_refs", catalogue::POL),
                catalogue::POL,
                "config_refs.policy_set",
                "config_refs.policy_set",
            )?;
            referenced_config_object(
                artifacts,
                text_at(draft, "config_refs", catalogue::REG),
                catalogue::REG,
                "config_refs.model_registry",
                "config_refs.model_registry",
            )?;
        }
        "ConfigSnapshotRegistered" => {
            let kind = text_at(draft, "payload", "kind").unwrap_or_default();
            if matches!(kind, catalogue::POL | catalogue::REG) {
                referenced_config_object(
                    artifacts,
                    text_at(draft, "payload", "content_hash"),
                    kind,
                    "payload.content_hash",
                    "payload.kind",
                )?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// An in-memory journal implementing the append protocol. Streams exist implicitly with head 0
/// and writer epoch 0.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    streams: BTreeMap<String, StreamState>,
    /// `event_id` → (`stream_id`, `seq`), global across streams (the `event_ids` table).
    event_ids: BTreeMap<String, (String, u64)>,
}

impl MemoryJournal {
    pub fn new() -> Self {
        Self::default()
    }

    /// Increments the stream's writer epoch, fencing out the previous writer, and returns it.
    pub fn take_ownership(&mut self, stream: &StreamId) -> u64 {
        let state = self.streams.entry(stream.as_str().to_owned()).or_default();
        state.epoch = state.epoch.saturating_add(1);
        state.epoch
    }

    pub fn head(&self, stream: &StreamId) -> Head {
        let state = self.streams.get(stream.as_str());
        let last = state.and_then(|s| s.rows.last());
        Head {
            seq: last.map_or(0, |r| r.seq),
            hash: last.map_or(Digest::ZERO, |r| r.hash),
            writer_epoch: state.map_or(0, |s| s.epoch),
        }
    }

    pub fn rows(&self, stream: &StreamId) -> &[StoredEvent] {
        self.streams
            .get(stream.as_str())
            .map_or(&[], |s| s.rows.as_slice())
    }

    pub fn event(&self, event_id: &str) -> Option<&StoredEvent> {
        let (stream, seq) = self.event_ids.get(event_id)?;
        self.streams
            .get(stream)?
            .rows
            .iter()
            .find(|r| r.seq == *seq)
    }

    /// Appends `drafts` (canonical-JSON draft bodies) to `stream` in one all-or-nothing step, in
    /// the order of journal spec §5.1: validation, idempotency, fencing and head check, then
    /// sequencing, chaining, and hashing. All events of a batch share `recorded_at`.
    pub fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
    ) -> AppendOutcome {
        let invalid = |draft: usize, reason: InvalidReason, path: &str| AppendOutcome::Invalid {
            draft,
            error: Invalid::new(reason, path),
        };
        if drafts.is_empty() {
            return invalid(0, InvalidReason::EmptyBatch, "");
        }
        let mut batch: Vec<Draft> = Vec::with_capacity(drafts.len());
        for (i, bytes) in drafts.iter().enumerate() {
            let draft = match Draft::parse(bytes) {
                Ok(d) => d,
                Err(error) => return AppendOutcome::Invalid { draft: i, error },
            };
            if draft.stream_id() != stream {
                return invalid(i, InvalidReason::StreamMismatch, "stream_id");
            }
            if batch.iter().any(|d| d.event_id() == draft.event_id()) {
                return invalid(i, InvalidReason::DuplicateEventId, "event_id");
            }
            batch.push(draft);
        }
        if let Err((draft, error)) = check_batch(&batch) {
            return AppendOutcome::Invalid { draft, error };
        }

        let stored: Vec<Option<&StoredEvent>> =
            batch.iter().map(|d| self.event(d.event_id())).collect();
        if let Some(first) = stored.iter().flatten().next() {
            for (draft, row) in batch.iter().zip(&stored) {
                if let Some(row) = row
                    && stored_draft(row).as_deref() != Some(draft.canonical_bytes())
                {
                    return AppendOutcome::IdempotencyConflict {
                        stored_seq: row.seq,
                    };
                }
            }
            if stored.iter().any(Option::is_none) {
                return AppendOutcome::IdempotencyConflict {
                    stored_seq: first.seq,
                };
            }
            return AppendOutcome::AlreadyCommitted(
                stored.into_iter().flatten().cloned().collect(),
            );
        }

        let head = self.head(stream);
        if writer_epoch != head.writer_epoch {
            return AppendOutcome::Fenced {
                current_epoch: head.writer_epoch,
            };
        }
        if expected_head != head.seq {
            return AppendOutcome::HeadMismatch {
                actual_seq: head.seq,
                actual_hash: head.hash,
            };
        }

        let mut environment = self
            .rows(stream)
            .first()
            .and_then(|r| Environment::parse(&r.environment));
        let mut risk_clock = self.streams.get(stream.as_str()).and_then(|s| s.risk_clock);
        let mut sealed = Vec::with_capacity(batch.len());
        let mut prev_hash = head.hash;
        let mut seq = head.seq;
        for (i, draft) in batch.iter().enumerate() {
            let Some(next) = seq.checked_add(1) else {
                return invalid(i, InvalidReason::NonCanonical, "seq");
            };
            seq = next;
            let opening = draft.event_type() == "StreamOpened";
            if seq == 1 && !opening {
                return invalid(i, InvalidReason::NotStreamOpened, "event_type");
            }
            if seq > 1 && opening {
                return invalid(i, InvalidReason::StreamAlreadyOpened, "event_type");
            }
            if *environment.get_or_insert(draft.environment()) != draft.environment() {
                return invalid(i, InvalidReason::EnvironmentMismatch, "environment");
            }
            if let Some(clock) = draft.risk_clock() {
                if risk_clock.is_some_and(|last| clock < last) {
                    return invalid(i, InvalidReason::RiskClockRegressed, "payload.risk_clock");
                }
                risk_clock = Some(clock);
            }
            let row = match seal(draft, seq, prev_hash, recorded_at) {
                Ok(row) => row,
                Err(error) => return AppendOutcome::Invalid { draft: i, error },
            };
            prev_hash = row.hash;
            sealed.push(row);
        }

        let state = self.streams.entry(stream.as_str().to_owned()).or_default();
        state.risk_clock = risk_clock;
        for row in &sealed {
            self.event_ids
                .insert(row.event_id.clone(), (row.stream_id.clone(), row.seq));
            state.rows.push(row.clone());
        }
        AppendOutcome::Committed(sealed)
    }

    /// Appends drafts whose versioned configuration references must be checked against the
    /// content-addressed artifact source before sequencing.
    pub fn append_with_config_artifacts(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        writer_epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[&[u8]],
        artifacts: &dyn ArtifactSource,
    ) -> AppendOutcome {
        for (index, bytes) in drafts.iter().enumerate() {
            let draft = match Draft::parse(bytes) {
                Ok(draft) => draft,
                Err(error) => {
                    return AppendOutcome::Invalid {
                        draft: index,
                        error,
                    };
                }
            };
            match validate_config_artifacts(&draft, artifacts) {
                Ok(()) => {}
                Err(ConfigArtifactFailure::Invalid(error)) => {
                    return AppendOutcome::Invalid {
                        draft: index,
                        error,
                    };
                }
                Err(ConfigArtifactFailure::Unavailable) => return AppendOutcome::Unavailable,
            }
        }
        self.append(stream, expected_head, writer_epoch, recorded_at, drafts)
    }
}

#[cfg(test)]
mod production_config_tests {
    use super::*;

    fn fixture() -> Result<Value, String> {
        parse(include_bytes!("../../../fixtures/refcases/journal.json")).map_err(|e| e.to_string())
    }

    fn list<'a>(value: &'a Value, member: &str) -> &'a [Value] {
        value
            .get(member)
            .and_then(Value::as_array)
            .unwrap_or_default()
    }

    fn text<'a>(value: &'a Value, member: &str) -> &'a str {
        value
            .get(member)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }

    fn artifacts(section: &Value) -> Result<BTreeMap<Digest, Vec<u8>>, String> {
        list(section, "artifacts")
            .iter()
            .map(|artifact| {
                let reference =
                    ArtifactRef::parse(text(artifact, "ref")).ok_or("an artifact ref")?;
                Ok((
                    reference.digest(),
                    text(artifact, "canonical").as_bytes().to_vec(),
                ))
            })
            .collect()
    }

    fn apply(draft: &mut Object, change: &Value) -> Result<(), String> {
        let path = text(change, "path");
        let mut members: Vec<&str> = path.split('.').collect();
        let last = members.pop().ok_or("an invalid path")?;
        let mut node = draft;
        for member in members {
            node = match node.get_mut(member) {
                Some(Value::Object(object)) => object,
                _ => return Err(format!("{path} has no object at {member}")),
            };
        }
        if change.get("delete") == Some(&Value::Bool(true)) {
            node.remove(last).map(|_| ()).ok_or_else(|| path.to_owned())
        } else {
            let key = Key::new(last).map_err(|e| e.to_string())?;
            let value = change.get("value").cloned().ok_or("a changed value")?;
            node.insert(key, value);
            Ok(())
        }
    }

    fn changed(section: &Value, case: &Value) -> Result<Value, String> {
        let base = text(case, "base");
        let mut draft = section
            .get("valid_drafts")
            .and_then(|drafts| drafts.get(base))
            .and_then(Value::as_object)
            .cloned()
            .ok_or_else(|| format!("no base {base}"))?;
        for change in list(case, "changes") {
            apply(&mut draft, change)?;
        }
        Ok(Value::Object(draft))
    }

    fn append(
        fixture: &Value,
        draft: &Value,
        artifacts: &dyn ArtifactSource,
    ) -> Result<AppendOutcome, String> {
        let stream_text = text(draft, "stream_id");
        let stream = StreamId::parse(stream_text).ok_or("a stream id")?;
        let stream_section = if stream_text.starts_with("agent:") {
            "agent_stream"
        } else {
            "control_stream"
        };
        let mut opening = fixture
            .get(stream_section)
            .and_then(|section| list(section, "chain").first())
            .and_then(|entry| entry.get("body"))
            .and_then(Value::as_object)
            .cloned()
            .ok_or("a stream opening")?;
        for member in JOURNAL_FIELDS {
            opening.remove(member);
        }
        let mut journal = MemoryJournal::new();
        let epoch = journal.take_ownership(&stream);
        let opening = to_canonical(&Value::Object(opening));
        let opened_at =
            UtcNanos::parse("2026-09-21T14:00:02.000000000Z").map_err(|e| e.to_string())?;
        if !matches!(
            journal.append(&stream, 0, epoch, opened_at, &[opening.as_slice()]),
            AppendOutcome::Committed(_)
        ) {
            return Err("stream opening refused".to_owned());
        }
        let bytes = to_canonical(draft);
        let at = UtcNanos::parse("2026-09-21T14:00:03.000000000Z").map_err(|e| e.to_string())?;
        Ok(journal.append_with_config_artifacts(
            &stream,
            1,
            epoch,
            at,
            &[bytes.as_slice()],
            artifacts,
        ))
    }

    #[test]
    fn every_production_configuration_vector_has_its_specified_outcome() -> Result<(), String> {
        let fixture = fixture()?;
        let section = fixture
            .get("production_config_refs")
            .ok_or("the production section")?;
        let stored = artifacts(section)?;
        let valid = section
            .get("valid_drafts")
            .and_then(Value::as_object)
            .ok_or("valid drafts")?;
        assert_eq!(valid.len(), 4);
        for (name, draft) in valid {
            assert!(
                matches!(
                    append(&fixture, draft, &stored)?,
                    AppendOutcome::Committed(_)
                ),
                "{name}"
            );
        }
        let invalid = list(section, "invalid_drafts");
        assert_eq!(invalid.len(), 8);
        for case in invalid {
            let expect = case.get("expect").ok_or("an expectation")?;
            let got = match append(&fixture, &changed(section, case)?, &stored)? {
                AppendOutcome::Invalid { error, .. } => {
                    Some((error.reason.code().to_owned(), error.path))
                }
                _ => None,
            };
            let wanted = Some((
                text(expect, "reason").to_owned(),
                text(expect, "path").to_owned(),
            ));
            assert_eq!(got, wanted, "{}", text(case, "name"));
        }
        Ok(())
    }

    #[test]
    fn every_new_reference_requires_its_stored_object() -> Result<(), String> {
        let fixture = fixture()?;
        let section = fixture
            .get("production_config_refs")
            .ok_or("the production section")?;
        let valid = section.get("valid_drafts").ok_or("valid drafts")?;
        for (name, object, member, path) in [
            (
                "model_output",
                "config_refs",
                "model_registry",
                "config_refs.model_registry",
            ),
            (
                "decision",
                "config_refs",
                "policy_set",
                "config_refs.policy_set",
            ),
            (
                "decision",
                "config_refs",
                "model_registry",
                "config_refs.model_registry",
            ),
            (
                "policy_registration",
                "payload",
                "content_hash",
                "payload.content_hash",
            ),
            (
                "model_registry_registration",
                "payload",
                "content_hash",
                "payload.content_hash",
            ),
        ] {
            let draft = valid.get(name).ok_or_else(|| name.to_owned())?;
            let reference = draft
                .get(object)
                .and_then(|value| value.get(member))
                .and_then(Value::as_str)
                .and_then(ArtifactRef::parse)
                .ok_or_else(|| path.to_owned())?;
            let mut stored = artifacts(section)?;
            stored
                .remove(&reference.digest())
                .ok_or("a stored object")?;
            let got = match append(&fixture, draft, &stored)? {
                AppendOutcome::Invalid { error, .. } => Some((error.reason.code(), error.path)),
                _ => None,
            };
            assert_eq!(got, Some(("missing_artifact", path.to_owned())), "{name}");
        }
        Ok(())
    }
}
