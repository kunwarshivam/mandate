//! §9.14's "What a verifier reads" (journal spec v0.29, E12-3, DEC-783 item 8, DEC-767): the
//! trusted start of a range, resolved from the workspace's own control-stream rows, never from
//! the request ([workspace API spec](../../../docs/specs/workspace-api.md) §4.8.1).
//! [`resolve_start_from_rows`] is the resolver over stored rows DEC-893 and DEC-894 define: its
//! start record is checked, and a manifest start is a [`ManifestStart`] the cold store must
//! confirm; the reference is `reference/journal/control.py`'s `start_from_rows`, the vectors
//! `cold_records.trusted_starts.row_cases`. It also reads an `AnchorComputed` row as the anchor a
//! verifier checks ([`anchor_record`], §10, §11).

use mandate_canon::{Digest, Value, parse, to_canonical};

use crate::records::manifest_hash;
use crate::verify::{columns_match, root_holds};
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
        match cold {
            ColdRead::Read(bytes) if Digest::of(&bytes) == self.manifest_hash => Ok(self.start),
            ColdRead::Read(_) => Err(TrustedStartError::Refused),
            ColdRead::Absent | ColdRead::Unreadable => Err(TrustedStartError::ColdUnreadable),
        }
    }
}

/// §9.14's lookup over the stored `rows` of the control stream of `stream_id`'s own workspace
/// (DEC-893): exactly one row must match `request`, by its columns and its body's `payload`, and
/// more than one refuses (item 7): a segment of `stream_id` whose `first_seq` is `from_seq`, or a
/// stamped anchor with a leaf for `stream_id` at `from_seq − 1`. That row must
/// pass §11 checks 2 and 4, a `SegmentExported` rule 117 too (item 1), and an `AnchorComputed`
/// §11's root check (DEC-895 item 2); every candidate row, of the record type the request needs,
/// must pass check 1 first, or the request is refused (DEC-895 item 1). No other check runs
/// (item 2). Genesis and a checked stamped anchor are [`ResolvedStart::Ready`]; a
/// checked segment is a [`ManifestStart`].
pub fn resolve_start_from_rows(
    rows: &[StoredEvent],
    stream_id: &str,
    from_seq: u64,
    request: StartRequest<'_>,
) -> Result<ResolvedStart, TrustedStartError> {
    let refused = TrustedStartError::Refused;
    let workspace = workspace_of(stream_id).ok_or(refused)?;
    let wanted = match request {
        StartRequest::Genesis => {
            let genesis = TrustedStart {
                from_seq,
                prev_hash: Digest::ZERO,
            };
            return (from_seq == 1)
                .then_some(ResolvedStart::Ready(genesis))
                .ok_or(refused);
        }
        StartRequest::Manifest { manifest_hash } => Wanted::Segment(manifest_hash),
        StartRequest::Anchor { anchor_event_id } => Wanted::Anchor(anchor_event_id),
    };
    let mut found = None;
    for row in rows
        .iter()
        .filter(|r| on_own_control(r, workspace) && r.event_type == wanted.event_type())
    {
        let body = strict_body(row).ok_or(refused)?;
        let payload = body.get("payload");
        let hit = payload.and_then(|p| wanted.start_in(row, p, stream_id, from_seq));
        if let Some(prev_hash) = hit
            && found.replace((row, body, prev_hash)).is_some()
        {
            return Err(refused);
        }
    }
    let (row, body, prev_hash) = found.ok_or(refused)?;
    let holds = columns_match(row, &body)
        && Digest::of(&row.body) == row.hash
        && body.get("payload").is_some_and(|p| wanted.rule_holds(p));
    let start = TrustedStart {
        from_seq,
        prev_hash,
    };
    match wanted {
        Wanted::Segment(manifest_hash) if holds => Ok(ResolvedStart::Manifest(ManifestStart {
            start,
            manifest_hash,
        })),
        Wanted::Anchor(_) if holds => Ok(ResolvedStart::Ready(start)),
        _ => Err(refused),
    }
}

/// The record a manifest or anchor [`StartRequest`] needs: the `SegmentExported` with this
/// `manifest_hash`, or the `AnchorComputed` with this `event_id`.
#[derive(Clone, Copy)]
enum Wanted<'a> {
    Segment(Digest),
    Anchor(&'a str),
}

impl Wanted<'_> {
    fn event_type(self) -> &'static str {
        match self {
            Wanted::Segment(_) => "SegmentExported",
            Wanted::Anchor(_) => "AnchorComputed",
        }
    }

    /// §9.14's lookup on one candidate row, by its columns and its body's `payload`: a
    /// segment of `stream_id` from `from_seq` gives its `first_prev_hash`; a stamped anchor with
    /// this `event_id` column gives the `hash` of its leaf for `stream_id` at `from_seq − 1`.
    fn start_in(
        self,
        row: &StoredEvent,
        payload: &Value,
        stream_id: &str,
        from_seq: u64,
    ) -> Option<Digest> {
        match self {
            Wanted::Segment(manifest_hash) => (digest_at(payload, "manifest_hash")
                == Some(manifest_hash)
                && payload.get("stream_id").and_then(Value::as_str) == Some(stream_id)
                && payload.get("first_seq").and_then(Value::as_int) == Some(from_seq))
            .then(|| digest_at(payload, "first_prev_hash"))
            .flatten(),
            Wanted::Anchor(anchor_event_id) => {
                let leaf_seq = from_seq.checked_sub(1)?;
                if row.event_id != anchor_event_id {
                    return None;
                }
                payload.get("token").filter(|t| **t != Value::Null)?;
                let leaf = payload
                    .get("leaves")
                    .and_then(Value::as_array)?
                    .iter()
                    .find(|l| {
                        l.get("stream_id").and_then(Value::as_str) == Some(stream_id)
                            && l.get("seq").and_then(Value::as_int) == Some(leaf_seq)
                    })?;
                digest_at(leaf, "hash")
            }
        }
    }

    /// The record's own rule after §11 checks 2 and 4: rule 117 for a segment (its
    /// `manifest_hash`, already the request's, is its six fields' hash, DEC-893 item 1), and §11's
    /// root check for an anchor (leaves in strictly rising `stream_id` order, §10's root over them,
    /// DEC-895 item 2).
    fn rule_holds(self, payload: &Value) -> bool {
        match self {
            Wanted::Segment(hash) => manifest_hash(payload) == Some(hash),
            Wanted::Anchor(_) => anchor_in(payload).is_some_and(|a| root_holds(&a)),
        }
    }
}

/// §11 check 1: the body parses, duplicate keys rejected, and re-canonicalizes to its own bytes.
fn strict_body(row: &StoredEvent) -> Option<Value> {
    parse(&row.body)
        .ok()
        .filter(|body| to_canonical(body) == row.body)
}

/// The row is on the control stream of `workspace`, by its columns.
fn on_own_control(row: &StoredEvent, workspace: &str) -> bool {
    StreamId::parse(&row.stream_id).is_some_and(|s| s.stream_type() == StreamType::Control)
        && workspace_of(&row.stream_id) == Some(workspace)
}

/// An `AnchorComputed` payload's leaves and root exactly as recorded (§9.14).
fn anchor_in(payload: &Value) -> Option<Anchor> {
    let leaves = payload
        .get("leaves")
        .and_then(Value::as_array)?
        .iter()
        .map(|leaf| {
            Some(AnchorLeaf {
                stream_id: leaf.get("stream_id")?.as_str()?.to_owned(),
                seq: leaf.get("seq")?.as_int()?,
                hash: digest_at(leaf, "hash")?,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    let root = digest_at(payload, "root")?;
    Some(Anchor { leaves, root })
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
    let anchor = anchor_in(payload).ok_or(malformed)?;
    let token = match payload.get("token").ok_or(malformed)? {
        Value::Null => None,
        token => Some(
            token
                .as_str()
                .and_then(ArtifactRef::parse)
                .ok_or(malformed)?,
        ),
    };
    Ok(AnchorRecord { anchor, token })
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
