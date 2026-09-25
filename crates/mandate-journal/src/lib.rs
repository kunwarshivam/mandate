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
//! anchoring (§10), and export lines (§6.2). Storage here is in memory; the Postgres store (E5-3)
//! implements the same protocol over the same `Draft` and `seal`.
//!
//! API stubs for the E5-1 tests PR (DEC-77); the implementation PR replaces the bodies.

use std::collections::BTreeMap;
use std::fmt;

use mandate_canon::{Digest, ParseErrorKind};
use mandate_time::UtcNanos;

mod draft;
mod merkle;
mod verify;

pub use draft::Draft;
pub use merkle::{Anchor, AnchorLeaf, merkle_root, tsa_imprint};
pub use verify::{
    ArtifactSource, EventCheck, EventFailure, RangeCheck, TrustedStart, Verified, verify_anchor,
    verify_events,
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
    pub fn parse(_s: &str) -> Option<Self> {
        None
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
}

impl InvalidReason {
    /// Stable reason code (ADR-0001 ES-09); JSON errors use the parser's code, such as `float`.
    pub fn code(self) -> &'static str {
        match self {
            Self::Json(kind) => kind.code(),
            Self::Schema => "schema",
            Self::NonCanonical => "non_canonical",
            Self::MissingConfigRef => "missing_config_ref",
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

/// Adds the journal-assigned fields to `draft` and hashes the canonical body.
pub fn seal(
    _draft: &Draft,
    _seq: u64,
    _prev_hash: Digest,
    _recorded_at: UtcNanos,
) -> Result<StoredEvent, Invalid> {
    Err(Invalid::new(InvalidReason::Schema, ""))
}

/// The export line for one event (journal spec §6.2): the canonical form of
/// `{"body": <body>, "hash": "<hex>"}`, without the line feed. `"body"` sorts before `"hash"`, so
/// the body bytes are an exact slice of the line.
pub fn export_line(_row: &StoredEvent) -> Vec<u8> {
    Vec::new()
}

/// A segment file: one export line per event, each followed by a line feed.
pub fn export_segment(_rows: &[StoredEvent]) -> Vec<u8> {
    Vec::new()
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
        _stream: &StreamId,
        _expected_head: u64,
        _writer_epoch: u64,
        _recorded_at: UtcNanos,
        _drafts: &[&[u8]],
    ) -> AppendOutcome {
        AppendOutcome::Unavailable
    }
}
