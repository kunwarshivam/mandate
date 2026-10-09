//! §9.14's "What a verifier reads" (journal spec v0.29, E12-3, DEC-783 item 8, DEC-767): the
//! trusted start of a range, resolved from the workspace's own control-stream records, never from
//! the request ([workspace API spec](../../../docs/specs/workspace-api.md) §4.8.1). The reference
//! is `reference/journal/control.py`'s `trusted_start`; the vectors are `cold_records.trusted_starts`.

use mandate_canon::Digest;

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
    let _ = (records, stream_id, from_seq, request);
    Err(TrustedStartError::Unimplemented { story: "E12-3" })
}
