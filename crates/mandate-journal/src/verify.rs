//! Verification (journal spec §11): ordered per-event checks over stored rows, and anchor checks.

use std::collections::BTreeMap;

use mandate_canon::{Digest, Value, parse, to_canonical};

use crate::schema::parse_digest_ref;
use crate::{Anchor, StoredEvent, StreamId, merkle_root};

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
    rows: &[StoredEvent],
    start: TrustedStart,
    artifacts: &dyn ArtifactSource,
) -> Result<Verified, EventFailure> {
    let mut expected_seq = start.from_seq;
    let mut prev_hash = start.prev_hash;
    for row in rows {
        let fail = |check| EventFailure {
            seq: row.seq,
            check,
        };
        let body = match parse(&row.body) {
            Ok(body) if to_canonical(&body) == row.body => body,
            _ => return Err(fail(EventCheck::NonCanonical)),
        };
        if !columns_match(row, &body) {
            return Err(fail(EventCheck::ColumnMismatch));
        }
        if row.seq != expected_seq {
            return Err(fail(EventCheck::SeqGap));
        }
        if Digest::of(&row.body) != row.hash {
            return Err(fail(EventCheck::RehashMismatch));
        }
        if row.prev_hash != prev_hash {
            return Err(fail(EventCheck::PrevHashMismatch));
        }
        for reference in body
            .get("artifact_refs")
            .and_then(Value::as_array)
            .unwrap_or_default()
        {
            let digest = reference.as_str().and_then(parse_digest_ref);
            match digest.and_then(|d| artifacts.artifact(&d).map(|bytes| (d, bytes))) {
                None => return Err(fail(EventCheck::ArtifactMissing)),
                Some((d, bytes)) if Digest::of(bytes) != d => {
                    return Err(fail(EventCheck::ArtifactMismatch));
                }
                Some(_) => {}
            }
        }
        prev_hash = row.hash;
        expected_seq = expected_seq.saturating_add(1);
    }
    Ok(Verified {
        next_seq: expected_seq,
        last_hash: prev_hash,
    })
}

/// The stored columns equal the body's fields (spec §11 check 2).
fn columns_match(row: &StoredEvent, body: &Value) -> bool {
    let text = |name: &str| body.get(name).and_then(Value::as_str);
    let int = |name: &str| body.get(name).and_then(Value::as_int);
    text("stream_id") == Some(row.stream_id.as_str())
        && int("seq") == Some(row.seq)
        && text("event_id") == Some(row.event_id.as_str())
        && text("event_type") == Some(row.event_type.as_str())
        && int("schema_version") == Some(row.schema_version)
        && text("environment") == Some(row.environment.as_str())
        && text("recorded_at") == Some(row.recorded_at.as_str())
        && text("prev_hash") == Some(row.prev_hash.to_hex().as_str())
}

/// Checks `rows` of `stream` against `anchor`: the event at the anchored `seq` exists with the
/// anchored hash, then the anchor's root matches its leaves (sorted by `stream_id`, no repeats).
pub fn verify_anchor(
    anchor: &Anchor,
    stream: &StreamId,
    rows: &[StoredEvent],
) -> Result<(), RangeCheck> {
    if let Some(leaf) = anchor
        .leaves
        .iter()
        .find(|l| l.stream_id == stream.as_str())
    {
        let present = rows
            .iter()
            .any(|r| r.seq == leaf.seq && r.hash == leaf.hash && r.stream_id == leaf.stream_id);
        if !present {
            return Err(RangeCheck::AnchorHeadMismatch);
        }
    }
    let sorted = anchor
        .leaves
        .windows(2)
        .all(|w| matches!(w, [a, b] if a.stream_id < b.stream_id));
    if !sorted || merkle_root(&anchor.leaves) != Some(anchor.root) {
        return Err(RangeCheck::AnchorRootMismatch);
    }
    Ok(())
}
