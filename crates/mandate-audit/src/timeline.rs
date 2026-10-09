//! The per-agent timeline's core read (workspace API §4.8.1 "Timeline", J1, FR-7.3, E12-2, AU-5,
//! DEC-764, DEC-777): the agent's stream merged with the account streams of the connections it was
//! deployed on, one cursor per stream. The merge runs over the raw stream heads, filtered or not, and
//! a page is a prefix of one unbounded merge (DEC-777 items 1 to 3). The route, its query parsing,
//! and the resolution of `account_refs` from the agent's `AgentDeployed` records are the caller's.

use mandate_identity::demand::{Permitted, ReadRecords};
use mandate_time::UtcNanos;

use crate::{AuditError, JournalEvent, MemoryRead, PageLimit, Watermark};

/// The most events one timeline page consumes from one stream (DEC-764 item 4). Reaching it stops
/// the whole page, for every stream (DEC-777 item 3).
pub const MAX_CONSUMED_PER_STREAM: u64 = 10_000;

/// One stream's cursor, the `after=<stream_id>:<seq>` of the route: the last `seq` the timeline
/// consumed from that stream. A timeline stream no cursor names starts at 0.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct StreamCursor {
    pub stream_id: String,
    pub seq: u64,
}

/// One timeline request. `account_refs` are the `account_ref`s of the connections the agent's
/// `AgentDeployed` events name, which name the streams `acct:{ws}:{account_ref}` of the tenant's
/// own workspace. `types`, `from` (inclusive), and `to` (exclusive, on `recorded_at`) select what is
/// served; every event they exclude is still consumed (AU-5).
#[derive(Debug, Clone)]
pub struct TimelineQuery<'q> {
    pub agent_id: &'q str,
    pub account_refs: &'q [&'q str],
    pub after: &'q [StreamCursor],
    pub types: Option<&'q [&'q str]>,
    pub from: Option<UtcNanos>,
    pub to: Option<UtcNanos>,
    pub limit: PageLimit,
}

/// One timeline page, `{events, next, more, as_of}`: each event is the page item with its
/// `stream_id`, in merge order; `next` is the cursor of every timeline stream, in ascending
/// `stream_id`, one not reached yet at 0 (DEC-777 item 5); `more` is whether some stream holds an
/// event past its `next` cursor in the page's snapshot (DEC-777 item 4); `as_of` is one watermark per
/// stream read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Timeline {
    pub events: Vec<JournalEvent>,
    pub next: Vec<StreamCursor>,
    pub more: bool,
    pub as_of: Vec<Watermark>,
}

/// The per-agent timeline over a store, for the witness of a context authorized for `ReadRecords`
/// (DEC-655). An unknown agent, and a cursor naming a stream that is not one of the timeline's, are
/// the one [`AuditError::NotFound`] (DEC-760 item 4).
pub trait TimelineRead {
    fn timeline(
        &self,
        tenant: &Permitted<'_, ReadRecords>,
        query: &TimelineQuery<'_>,
    ) -> Result<Timeline, AuditError>;
}

impl TimelineRead for MemoryRead<'_> {
    fn timeline(
        &self,
        _tenant: &Permitted<'_, ReadRecords>,
        _query: &TimelineQuery<'_>,
    ) -> Result<Timeline, AuditError> {
        Err(AuditError::Unimplemented { story: "E12-2" })
    }
}
