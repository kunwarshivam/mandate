//! §9.14's "What a verifier reads" (journal spec v0.29, E12-3, DEC-783 item 8, DEC-767): the
//! trusted start of a range, resolved from the workspace's own control-stream records, never from
//! the request ([workspace API spec](../../../docs/specs/workspace-api.md) §4.8.1). The reference
//! is `reference/journal/control.py`'s `trusted_start`; the vectors are `cold_records.trusted_starts`.

use mandate_canon::{Digest, Value, parse};

use crate::{StoredEvent, TrustedStart};

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
    /// The resolver is not built yet (DEC-77).
    Unimplemented { story: &'static str },
}

/// The trusted start of a range of `stream_id` entered at `from_seq`, from `request`, among the
/// `records` of `stream_id`'s own workspace only: genesis is seq 1 with 64 zeros; a
/// `SegmentExported` of `stream_id` with `first_seq` = `from_seq` gives its `first_prev_hash`; an
/// `AnchorComputed` with a `token` gives the `hash` of its leaf for `stream_id` at `from_seq − 1`.
pub fn resolve_trusted_start(
    records: &[StoredEvent],
    stream_id: &str,
    from_seq: u64,
    request: StartRequest<'_>,
) -> Result<TrustedStart, TrustedStartError> {
    let refused = TrustedStartError::Refused;
    let workspace = workspace_of(stream_id).ok_or(refused)?;
    let own = records
        .iter()
        .filter(|r| workspace_of(&r.stream_id) == Some(workspace));
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

fn workspace_of(stream_id: &str) -> Option<&str> {
    stream_id.split(':').nth(1).filter(|w| !w.is_empty())
}

fn digest_at(value: &Value, name: &str) -> Option<Digest> {
    value
        .get(name)
        .and_then(Value::as_str)
        .and_then(Digest::from_hex)
}
