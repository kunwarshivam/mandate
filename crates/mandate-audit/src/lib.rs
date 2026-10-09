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
//! Read-only audit reads over the journal, scoped to one workspace (workspace API §4.8.1, backlog
//! E12-6, journal spec §7). Pure: no I/O of its own, no clock, no randomness, and it writes nothing
//! to any journal. Every read takes the caller's [`Tenant`], never a bare workspace id (identity
//! spec ID-8), and reaches only streams whose workspace segment is that tenant's workspace ULID
//! text (DEC-770 item 4). A stream or event of another workspace, a malformed id, and an absent one
//! all read as the same [`AuditError::NotFound`] (API-9, AU-1, DEC-760 item 4). Pages follow API-15
//! and DEC-760: `seq` order, each event's stored canonical body bytes with its `hash` and
//! `prev_hash`, the stream head read with the page, and the cursor the last `seq` served.

use mandate_canon::Digest;
use mandate_identity::Tenant;
use mandate_journal::MemoryJournal;

/// Why an audit read returned nothing. `NotFound` carries no detail, so a foreign id and an absent
/// one cannot be told apart by their error (API-9). The other variants are the 422 `invalid` of a
/// query member that is not an id, which a read checks before it resolves any id (DEC-760 item 5).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditError {
    /// The body of every stub in the tests PR (DEC-77, DEC-83).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The stream or event is absent, malformed, or in another workspace (API-9).
    #[error("not found")]
    NotFound,
    /// A page `limit` outside `1..=1000` (DEC-760 item 1). Refused, never clamped.
    #[error("page limit {limit} is outside 1..=1000")]
    LimitOutOfRange { limit: u64 },
    /// An `after_seq` above 2^53 − 1, the largest canonical integer (DEC-760 item 1).
    #[error("after_seq {after_seq} is outside 0..=9007199254740991")]
    AfterSeqOutOfRange { after_seq: u64 },
}

/// How many events or streams one page may hold: `1..=1000`, and 100 when the caller names none
/// (DEC-760 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLimit(u16);

impl PageLimit {
    pub fn new(limit: Option<u64>) -> Result<Self, AuditError> {
        let _ = limit;
        Err(AuditError::Unimplemented { story: "E12-6" })
    }
}

/// The stream types of journal spec §2, as the stream list names them. `Notice` (`ntf:{ws}`) is
/// named by the spec but unreachable until `mandate_journal::StreamId::parse` parses `ntf:` streams
/// (DEC-770 item 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StreamType {
    Account,
    Agent,
    Control,
    Scheduler,
    Notice,
}

/// A stream's head: its last `seq`, that event's hash (bare, §4.8.1 "Hash forms") and its
/// `recorded_at`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub seq: u64,
    pub hash: Digest,
    pub recorded_at: String,
}

/// One item of the stream list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEntry {
    pub stream_id: String,
    pub stream_type: StreamType,
    pub head: Head,
}

/// One stored event as an audit read serves it: the exact canonical body bytes that were hashed,
/// the stored `hash`, and the `prev_hash` that links it to the event before it. The other members
/// repeat the body's; the body is what a client checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEvent {
    pub stream_id: String,
    pub seq: u64,
    pub event_id: String,
    pub event_type: String,
    pub recorded_at: String,
    pub body: Vec<u8>,
    pub hash: Digest,
    pub prev_hash: Digest,
}

/// A page of one stream (API-15, DEC-760 item 2): the events after the request's `after_seq` in
/// `seq` order, the head read with them, the cursor for the next page (the last `seq` served, or
/// the request's own `after_seq` when the page is empty), and whether the page ends at the head.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub stream_id: String,
    pub head: Head,
    pub events: Vec<JournalEvent>,
    pub next_after_seq: u64,
    pub at_head: bool,
}

/// The journal reads the audit explorer needs, each scoped to the workspace of the caller's
/// [`Tenant`], which only `mandate_identity::authorize` constructs (identity spec ID-8). A read does
/// not check the tenant's permission again: the API layer authorizes `ReadRecords` before it calls
/// (DEC-770 item 4). An adapter implements them over its store: [`MemoryRead`] here, the Postgres
/// store in `mandate-journal-pg`.
pub trait JournalRead {
    /// The tenant's streams that hold at least one event, with their heads, in ascending
    /// `stream_id` bytes, those after `after` only, at most `limit` of them (DEC-760 item 6).
    fn streams(
        &self,
        tenant: &impl Tenant,
        after: Option<&str>,
        limit: PageLimit,
    ) -> Result<Vec<StreamEntry>, AuditError>;

    /// The events of `stream_id` after `after_seq`, at most `limit` of them, with the head read in
    /// the same snapshot.
    fn page(
        &self,
        tenant: &impl Tenant,
        stream_id: &str,
        after_seq: u64,
        limit: PageLimit,
    ) -> Result<Page, AuditError>;

    /// The event whose `event_id` is `event_id`, when its stream is in the tenant's workspace.
    fn event(&self, tenant: &impl Tenant, event_id: &str) -> Result<JournalEvent, AuditError>;
}

/// [`JournalRead`] over the in-memory journal. One borrow of the journal is one snapshot.
#[derive(Debug, Clone, Copy)]
pub struct MemoryRead<'a> {
    journal: &'a MemoryJournal,
}

impl<'a> MemoryRead<'a> {
    pub fn new(journal: &'a MemoryJournal) -> Self {
        Self { journal }
    }
}

impl JournalRead for MemoryRead<'_> {
    fn streams(
        &self,
        tenant: &impl Tenant,
        after: Option<&str>,
        limit: PageLimit,
    ) -> Result<Vec<StreamEntry>, AuditError> {
        let _ = (self.journal, tenant, after, limit.0);
        Err(AuditError::Unimplemented { story: "E12-6" })
    }

    fn page(
        &self,
        tenant: &impl Tenant,
        stream_id: &str,
        after_seq: u64,
        limit: PageLimit,
    ) -> Result<Page, AuditError> {
        let _ = (self.journal, tenant, stream_id, after_seq, limit.0);
        Err(AuditError::Unimplemented { story: "E12-6" })
    }

    fn event(&self, tenant: &impl Tenant, event_id: &str) -> Result<JournalEvent, AuditError> {
        let _ = (self.journal, tenant, event_id);
        Err(AuditError::Unimplemented { story: "E12-6" })
    }
}
