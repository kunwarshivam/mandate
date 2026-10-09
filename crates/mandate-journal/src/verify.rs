//! Verification (journal spec §11): ordered per-event checks over stored rows, and anchor checks.

use mandate_canon::{Digest, Value, parse, to_canonical};

use crate::{
    Anchor, ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, StreamId, merkle_root,
};

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

/// Walks `rows` in order from `start`, running the per-event checks of spec §11 in order. The
/// artifact check re-hashes what `artifacts` returns itself; an artifact the source cannot read
/// (`Unavailable`) fails as `artifact_missing`, so an unreachable store never passes verification.
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
        let artifact_refs = body
            .get("artifact_refs")
            .and_then(Value::as_array)
            .into_iter()
            .flatten();
        let config_refs = body
            .get("config_refs")
            .and_then(Value::as_object)
            .into_iter()
            .flat_map(|refs| refs.values());
        for reference in artifact_refs.chain(config_refs) {
            let read = reference
                .as_str()
                .and_then(ArtifactRef::parse)
                .map(|r| (r, artifacts.read_artifact(&r)));
            match read {
                None | Some((_, Err(ArtifactError::Missing | ArtifactError::Unavailable))) => {
                    return Err(fail(EventCheck::ArtifactMissing));
                }
                Some((_, Err(ArtifactError::Corrupt))) => {
                    return Err(fail(EventCheck::ArtifactMismatch));
                }
                Some((r, Ok(bytes))) if Digest::of(&bytes) != r.digest() => {
                    return Err(fail(EventCheck::ArtifactMismatch));
                }
                Some((_, Ok(_))) => {}
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

/// The stored rows `seq` 1 to `from_seq − 1` of a range's stream, verified by §11's checks 1 to 6
/// from genesis and bound to the range's trusted start (DEC-892). Every anchor fold takes one, so
/// none can read history a hot-store rewrite forged; [`VerifiedPrefix::bind`] is its only
/// constructor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerifiedPrefix<'a> {
    rows: &'a [StoredEvent],
}

/// Why rows were not bound as a range's prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixError {
    /// The first row that fails §11's checks 1 to 6 from genesis.
    Unverified(EventFailure),
    /// The rows do not end just before the trusted start: not empty for `from_seq` 1, or else a
    /// last `seq` that is not `from_seq − 1` or a last `hash` that is not `start.prev_hash`.
    Unbound,
    /// `bind` is a DEC-77 stub until `story` lands.
    Unimplemented { story: &'static str },
}

impl<'a> VerifiedPrefix<'a> {
    /// `rows` as the prefix of a range entered at `start`: they pass checks 1 to 6 from genesis
    /// with no gap, and are empty when `start.from_seq` is 1, else end at `seq` `from_seq − 1`
    /// with `hash` `start.prev_hash`. A refusal gives the caller no anchor, so its range fails
    /// closed (DEC-892 item 3).
    pub fn bind(
        rows: &'a [StoredEvent],
        start: TrustedStart,
        artifacts: &dyn ArtifactSource,
    ) -> Result<Self, PrefixError> {
        let _ = (rows, start, artifacts);
        Err(PrefixError::Unimplemented { story: "E12-3" })
    }

    pub fn rows(&self) -> &'a [StoredEvent] {
        self.rows
    }
}

/// One range walked position by position (§11's per-event checks; §9.13 rules 111 and 132;
/// workspace API §4.8.1 "Coverage", AU-8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RangeWalk {
    /// The events of the range that passed every per-event check, counted from `from_seq`.
    pub checked: u64,
    /// The `hash` of the event at `to_seq` when every position passed, else the first failure,
    /// reported at the position walked, whatever `seq` the row there holds.
    pub outcome: Result<Digest, EventFailure>,
}

/// Why a range could not be walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RangeWalkError {
    /// `from_seq` is 0 or `to_seq` is below it, so the bounds name no range (rule 107).
    NotARange,
    /// The walk is not built yet (DEC-77).
    Unimplemented { story: &'static str },
}

/// Walks `rows`, the stored events read for the range `start.from_seq` to `to_seq`, running §11's
/// per-event checks at each position from `from_seq` in turn: the row at position `p` must be the
/// event `p`, and the first failure is reported at `p`, never at the `seq` the row holds. A row the
/// walk expects and does not find fails `seq_gap` at the expected position, and a row left over
/// after `to_seq` fails `seq_gap` at `to_seq`, the last position rule 111 lets a failure name.
pub fn walk_range(
    rows: &[StoredEvent],
    start: TrustedStart,
    to_seq: u64,
    artifacts: &dyn ArtifactSource,
) -> Result<RangeWalk, RangeWalkError> {
    let _ = (rows, start, to_seq, artifacts);
    Err(RangeWalkError::Unimplemented { story: "E12-3" })
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
