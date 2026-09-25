//! Verification (journal spec §11): ordered per-event checks over stored rows, and anchor checks.

use std::collections::BTreeMap;

use mandate_canon::Digest;

use crate::{Anchor, StoredEvent, StreamId};

/// Per-event checks in the order they run; the first failure is reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventCheck {
    NonCanonical,
    ColumnMismatch,
    SeqGap,
    RehashMismatch,
    PrevHashMismatch,
    ArtifactMissing,
    ArtifactMismatch,
}

impl EventCheck {
    /// The check's code as the spec writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::NonCanonical => "non_canonical",
            Self::ColumnMismatch => "column_mismatch",
            Self::SeqGap => "seq_gap",
            Self::RehashMismatch => "rehash_mismatch",
            Self::PrevHashMismatch => "prev_hash_mismatch",
            Self::ArtifactMissing => "artifact_missing",
            Self::ArtifactMismatch => "artifact_mismatch",
        }
    }
}

/// The first failing event (by its `seq` column) and the check it failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EventFailure {
    pub seq: u64,
    pub check: EventCheck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeCheck {
    AnchorHeadMismatch,
    AnchorRootMismatch,
}

impl RangeCheck {
    pub fn code(self) -> &'static str {
        match self {
            Self::AnchorHeadMismatch => "anchor_head_mismatch",
            Self::AnchorRootMismatch => "anchor_root_mismatch",
        }
    }
}

/// Where verification starts, taken from a manifest or anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedStart {
    pub from_seq: u64,
    pub prev_hash: Digest,
}

impl TrustedStart {
    /// A full stream: seq 1 with 64 zeros.
    pub const GENESIS: Self = Self {
        from_seq: 1,
        prev_hash: Digest::ZERO,
    };
}

/// The state after a verified range: the next expected `seq` and the last hash.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Verified {
    pub next_seq: u64,
    pub last_hash: Digest,
}

/// Content-addressed artifact bytes by digest (journal spec §6.3).
pub trait ArtifactSource {
    fn artifact(&self, digest: &Digest) -> Option<&[u8]>;
}

impl ArtifactSource for BTreeMap<Digest, Vec<u8>> {
    fn artifact(&self, digest: &Digest) -> Option<&[u8]> {
        self.get(digest).map(Vec::as_slice)
    }
}

/// Walks `rows` in order from `start`, running the per-event checks of spec §11 in order.
pub fn verify_events(
    _rows: &[StoredEvent],
    _start: TrustedStart,
    _artifacts: &dyn ArtifactSource,
) -> Result<Verified, EventFailure> {
    Err(EventFailure {
        seq: 0,
        check: EventCheck::NonCanonical,
    })
}

/// Checks `rows` of `stream` against `anchor`: the event at the anchored `seq` exists with the
/// anchored hash, then the anchor's root matches its leaves (sorted by `stream_id`, no repeats).
pub fn verify_anchor(
    _anchor: &Anchor,
    _stream: &StreamId,
    _rows: &[StoredEvent],
) -> Result<(), RangeCheck> {
    Err(RangeCheck::AnchorRootMismatch)
}
