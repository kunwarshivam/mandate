//! §9.14's "What a verifier reads" (journal spec v0.29, E12-3, DEC-783 item 8, DEC-767): the
//! trusted start of a range, resolved from the workspace's own control-stream records, never from
//! the request ([workspace API spec](../../../docs/specs/workspace-api.md) §4.8.1). The reference
//! is `reference/journal/control.py`'s `trusted_start`; the vectors are `cold_records.trusted_starts`.
//! It also reads an `AnchorComputed` row as the anchor a verifier checks ([`anchor_record`], §10,
//! §11). [`resolve_start_from_rows`] is the resolver over stored rows DEC-893 and DEC-894 define:
//! its start record is checked, and a manifest start is a [`ManifestStart`] the cold store must
//! confirm; the reference is `control.py`'s `start_from_rows`, the vectors
//! `cold_records.trusted_starts.row_cases`.

use mandate_canon::{Digest, Value, parse};

use crate::{Anchor, AnchorLeaf, ArtifactRef, StoredEvent, StreamId, StreamType, TrustedStart};

/// Workspace API §4.8.1's `trusted_start`: it names the record to read, never the `prev_hash`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartRequest<'a> {
    /// Seq 1 with 64 zeros.
    Genesis,
    /// The `SegmentExported` whose `manifest_hash` this is.
    Manifest { manifest_hash: Digest },
    /// The `AnchorComputed` whose `event_id` this is.
    Anchor { anchor_event_id: &'a str },
}

/// Why no trusted start was resolved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustedStartError {
    /// No usable start: the record is absent, another workspace's (the same refusal, DEC-767), of
    /// another stream, an anchor without a `token`, or does not fit `from_seq` (§4.8.1).
    Refused,
    /// The cold store could not answer for a manifest start: the segment's manifest object is
    /// absent or cannot be read (DEC-787 item 5, DEC-893 item 5). Kept apart from `Refused`
    /// because it is retryable and raises no alert; both are 422 `trusted_start` with nothing
    /// recorded.
    ColdUnreadable,
    /// Never returned now that E12-3 built the resolver; kept, as `ControlVerifyError` keeps its
    /// own, so a caller's match stays the same across the crate's stubs (DEC-77).
    Unimplemented { story: &'static str },
}

/// The trusted start of a range of `stream_id` entered at `from_seq`, from `request`, among the
/// `records` on the control stream of `stream_id`'s own workspace only: genesis is seq 1 with 64
/// zeros; a `SegmentExported` of `stream_id` with `first_seq` = `from_seq` gives its
/// `first_prev_hash`; an `AnchorComputed` with a `token` gives the `hash` of its leaf for
/// `stream_id` at `from_seq − 1`.
pub fn resolve_trusted_start(
    records: &[StoredEvent],
    stream_id: &str,
    from_seq: u64,
    request: StartRequest<'_>,
) -> Result<TrustedStart, TrustedStartError> {
    let refused = TrustedStartError::Refused;
    let workspace = workspace_of(stream_id).ok_or(refused)?;
    let own = records.iter().filter(|r| {
        StreamId::parse(&r.stream_id).is_some_and(|s| s.stream_type() == StreamType::Control)
            && workspace_of(&r.stream_id) == Some(workspace)
    });
    let found = match request {
        StartRequest::Genesis => (from_seq == 1).then_some(Digest::ZERO),
        StartRequest::Manifest { manifest_hash } => own
            .filter(|r| r.event_type == "SegmentExported")
            .filter_map(|r| parse(&r.body).ok())
            .filter_map(|b| b.get("payload").cloned())
            .find(|p| {
                digest_at(p, "manifest_hash") == Some(manifest_hash)
                    && p.get("stream_id").and_then(Value::as_str) == Some(stream_id)
                    && p.get("first_seq").and_then(Value::as_int) == Some(from_seq)
            })
            .and_then(|p| digest_at(&p, "first_prev_hash")),
        StartRequest::Anchor { anchor_event_id } => {
            let leaf_seq = from_seq.checked_sub(1).ok_or(refused)?;
            own.filter(|r| r.event_type == "AnchorComputed" && r.event_id == anchor_event_id)
                .filter_map(|r| parse(&r.body).ok())
                .filter_map(|b| b.get("payload").cloned())
                .filter(|p| p.get("token").is_some_and(|t| *t != Value::Null))
                .find_map(|p| {
                    let leaves = p.get("leaves").and_then(Value::as_array)?;
                    let leaf = leaves.iter().find(|l| {
                        l.get("stream_id").and_then(Value::as_str) == Some(stream_id)
                            && l.get("seq").and_then(Value::as_int) == Some(leaf_seq)
                    })?;
                    digest_at(leaf, "hash")
                })
        }
    };
    found
        .map(|prev_hash| TrustedStart {
            from_seq,
            prev_hash,
        })
        .ok_or(refused)
}

/// What [`resolve_start_from_rows`] found (DEC-893 item 4): a start ready to walk from, or a
/// manifest start the cold store has still to confirm.
#[derive(Debug, PartialEq, Eq)]
pub enum ResolvedStart {
    /// Genesis, or a checked stamped anchor: no cold read is needed.
    Ready(TrustedStart),
    /// A checked `SegmentExported` that only the hot store vouches for so far.
    Manifest(ManifestStart),
}

/// A manifest start the hot store vouches for, not yet a [`TrustedStart`]: only
/// [`ManifestStart::confirm`] makes it one, so a hot-only manifest start is unrepresentable
/// (DEC-893 item 4). Only the resolver builds one:
///
/// ```compile_fail,E0451
/// use mandate_journal::{ManifestStart, TrustedStart};
/// let forged = ManifestStart { start: TrustedStart::GENESIS, manifest_hash: mandate_canon::Digest::ZERO };
/// ```
#[derive(Debug, PartialEq, Eq)]
pub struct ManifestStart {
    start: TrustedStart,
    manifest_hash: Digest,
}

/// The cold store's answer for a segment's manifest object (§6.2): its bytes, no such object, or
/// no answer at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColdRead {
    Read(Vec<u8>),
    Absent,
    Unreadable,
}

impl ManifestStart {
    /// The start, once `Digest::of` the cold manifest's bytes is the record's `manifest_hash`
    /// (DEC-893 item 4): other bytes are `Refused` (DEC-894 item 1), and an absent or unreadable
    /// object is `ColdUnreadable` (DEC-787 item 5).
    pub fn confirm(self, cold: ColdRead) -> Result<TrustedStart, TrustedStartError> {
        let _ = (self.start, self.manifest_hash, cold);
        Err(TrustedStartError::Unimplemented { story: "E12-3" })
    }
}

/// §9.14's lookup over the stored `rows` of the control stream of `stream_id`'s own workspace
/// (DEC-893): exactly one row must match `request`, by its columns and its body's `payload`, as
/// [`resolve_trusted_start`] matches records, and more than one refuses (item 7). That row must
/// pass §11 checks 1, 2 and 4 in that order, and a `SegmentExported` rule 117 too (item 1); no
/// other check runs (item 2). Genesis and a checked stamped anchor are [`ResolvedStart::Ready`]; a
/// checked segment is a [`ManifestStart`].
pub fn resolve_start_from_rows(
    rows: &[StoredEvent],
    stream_id: &str,
    from_seq: u64,
    request: StartRequest<'_>,
) -> Result<ResolvedStart, TrustedStartError> {
    let _ = (rows, stream_id, from_seq, request);
    Err(TrustedStartError::Unimplemented { story: "E12-3" })
}

/// An `AnchorComputed` as a verifier reads it (§9.14): its leaves and root exactly as recorded,
/// neither sorted nor recomputed, so §11's anchor checks judge what the record holds, and its
/// timestamp token, `None` when the record's `token` is `null`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnchorRecord {
    pub anchor: Anchor,
    pub token: Option<ArtifactRef>,
}

/// Why a row gave no [`AnchorRecord`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorRecordError {
    /// The row is not an `AnchorComputed` at schema version 1.
    NotAnAnchor,
    /// Its body does not parse, or a member of its payload is missing or not of §9.14's type.
    Malformed,
    /// Never returned now that E12-3 built the reader; kept, as `TrustedStartError` keeps its
    /// own, so a caller's match stays the same across the crate's stubs (DEC-77).
    Unimplemented { story: &'static str },
}

/// The anchor an `AnchorComputed` row records (§9.14, §10).
pub fn anchor_record(row: &StoredEvent) -> Result<AnchorRecord, AnchorRecordError> {
    if row.event_type != "AnchorComputed" || row.schema_version != 1 {
        return Err(AnchorRecordError::NotAnAnchor);
    }
    let malformed = AnchorRecordError::Malformed;
    let body = parse(&row.body).map_err(|_| malformed)?;
    let payload = body.get("payload").ok_or(malformed)?;
    let leaves = payload
        .get("leaves")
        .and_then(Value::as_array)
        .ok_or(malformed)?
        .iter()
        .map(|leaf| {
            Some(AnchorLeaf {
                stream_id: leaf.get("stream_id")?.as_str()?.to_owned(),
                seq: leaf.get("seq")?.as_int()?,
                hash: digest_at(leaf, "hash")?,
            })
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(malformed)?;
    let root = digest_at(payload, "root").ok_or(malformed)?;
    let token = match payload.get("token").ok_or(malformed)? {
        Value::Null => None,
        token => Some(
            token
                .as_str()
                .and_then(ArtifactRef::parse)
                .ok_or(malformed)?,
        ),
    };
    Ok(AnchorRecord {
        anchor: Anchor { leaves, root },
        token,
    })
}

fn workspace_of(stream_id: &str) -> Option<&str> {
    stream_id.split(':').nth(1).filter(|w| !w.is_empty())
}

fn digest_at(value: &Value, name: &str) -> Option<Digest> {
    value
        .get(name)
        .and_then(Value::as_str)
        .and_then(Digest::from_hex)
}
