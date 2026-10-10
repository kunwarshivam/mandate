//! The plan of a requested verification run (workspace API §4.8.1 "Verification", J4, E12-3;
//! DEC-787 item 1, DEC-788 item 4, DEC-893). [`plan`] takes the refusals of DEC-788 item 4 that
//! follow the idempotent replay, in order: the `stream_id` (the one
//! [`VerificationRefusal::NotFound`], AU-1), the range against the head at start, the
//! 1,000,000-event bound on the range alone, then the trusted start. A refused request reads no
//! cold object, and none records anything.

use mandate_canon::Digest;
use mandate_identity::demand::{Permitted, ReadRecords};
use mandate_journal::{ColdRead, StartRequest, StoredEvent, TrustedStart, TrustedStartError};

/// The most events one run covers (§4.8.1): the range's, never the prefix before it.
pub const MAX_RUN_EVENTS: u64 = 1_000_000;

/// `POST /verifications`'s members once their shape is checked (§4.8.1 refusal step 2): `to_seq`
/// `None` is the head at start, and `trusted_start` names its record, never a `prev_hash`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerificationRequest<'r> {
    /// The stream to verify, resolved only among the workspace's streams (AU-1).
    pub stream_id: &'r str,
    /// The range's first `seq`; 0 is a `range` refusal (DEC-897 item 2).
    pub from_seq: u64,
    /// The range's last `seq`, or `None` for the stream's head at start.
    pub to_seq: Option<u64>,
    /// The record the range starts from (§9.14), never a `prev_hash`.
    pub trusted_start: StartRequest<'r>,
}

/// What a run reads at start, in the one snapshot of DEC-787 item 1: the requested stream's head
/// (`None` when it holds no event) and the workspace's own `ctl:{ws}` rows up to their head.
#[derive(Debug, Clone, Copy)]
pub struct ControlSnapshot<'s> {
    /// The requested stream's last `seq` at start, `None` when it holds no event.
    pub stream_head: Option<u64>,
    /// The workspace's own `ctl:{ws}` rows up to their head at start.
    pub control: &'s [StoredEvent],
}

/// The cold store's segment manifest objects (journal spec §6.2), by their `manifest_hash`.
pub trait ColdSource {
    /// The bytes of the manifest object whose hash is `manifest_hash`, or why there are none.
    fn manifest(&self, manifest_hash: &Digest) -> ColdRead;
}

/// Why no run starts (§4.8.1). None records anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationRefusal {
    /// The stream is absent, malformed, or another workspace's: the one 404 (AU-1, DEC-760).
    NotFound,
    /// 422 `invalid`, violation `range`: the range against the head at start, or the size bound.
    Range,
    /// 422 `invalid`, violation `trusted_start` (DEC-893 item 5).
    TrustedStart(TrustedStartError),
    /// The body of every stub in a tests PR (DEC-77).
    Unimplemented { story: &'static str },
}

impl VerificationRefusal {
    /// The code a refusal is answered with: `not_found` for the 404, and the 422's violation.
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotFound => "not_found",
            Self::Range => "range",
            Self::TrustedStart(_) => "trusted_start",
            Self::Unimplemented { .. } => "unimplemented",
        }
    }
}

/// The request's `trusted_start` as `VerificationRun` version 2 records it in `start` (DEC-788
/// item 1): a `manifest_hash` as a digest, so its `sha256:` ref and its bare hex are one value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordedStart {
    /// Seq 1 with 64 zeros.
    Genesis,
    /// The `SegmentExported` whose `manifest_hash` this is.
    Manifest { manifest_hash: Digest },
    /// The `AnchorComputed` whose `event_id` this is.
    Anchor { anchor_event_id: String },
}

/// A request that passed every refusal: the resolved `to_seq`, the trusted start the range is
/// walked from, and the start the record names. Only [`plan`] builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    to_seq: u64,
    start: TrustedStart,
    recorded_start: RecordedStart,
}

impl Plan {
    /// The range's last `seq`: the request's, or the head at start.
    pub fn to_seq(&self) -> u64 {
        self.to_seq
    }

    /// The `from_seq` and trusted `prev_hash` the range is walked from.
    pub fn start(&self) -> TrustedStart {
        self.start
    }

    /// The start `VerificationRun` version 2 records (DEC-788 item 1).
    pub fn recorded_start(&self) -> &RecordedStart {
        &self.recorded_start
    }
}

/// §4.8.1 refusal steps 4 to 7 over one snapshot, scoped to the workspace of the caller's
/// [`Permitted<'_, ReadRecords>`]. `cold` is asked only for a manifest start whose row the snapshot
/// vouches for, and so never for a request steps 4 to 6 refuse.
pub fn plan(
    tenant: &Permitted<'_, ReadRecords>,
    request: &VerificationRequest<'_>,
    snapshot: &ControlSnapshot<'_>,
    cold: &dyn ColdSource,
) -> Result<Plan, VerificationRefusal> {
    let _ = (tenant, request, snapshot, cold);
    Err(VerificationRefusal::Unimplemented { story: "E12-3" })
}

#[cfg(test)]
mod tests {
    use mandate_canon::Digest;
    use mandate_journal::{TrustedStart, TrustedStartError};

    use super::{Plan, RecordedStart, VerificationRefusal};

    #[test]
    fn a_plan_serves_its_own_members() {
        let start = TrustedStart {
            from_seq: 4,
            prev_hash: Digest::of(b"prev"),
        };
        let recorded_start = RecordedStart::Anchor {
            anchor_event_id: "01J8Z3C6A000000000000000R4".to_owned(),
        };
        let plan = Plan {
            to_seq: 9,
            start,
            recorded_start: recorded_start.clone(),
        };
        assert_eq!(
            (plan.to_seq(), plan.start(), plan.recorded_start()),
            (9, start, &recorded_start)
        );
    }

    #[test]
    fn each_refusal_has_its_own_code() {
        let codes = [
            VerificationRefusal::NotFound,
            VerificationRefusal::Range,
            VerificationRefusal::TrustedStart(TrustedStartError::Refused),
            VerificationRefusal::TrustedStart(TrustedStartError::ColdUnreadable),
            VerificationRefusal::Unimplemented { story: "E12-3" },
        ]
        .map(|refusal| refusal.code());
        let want = [
            "not_found",
            "range",
            "trusted_start",
            "trusted_start",
            "unimplemented",
        ];
        assert_eq!(codes, want);
    }
}
