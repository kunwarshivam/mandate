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
//! Read-only audit reads over the journal, scoped to one workspace (workspace API §4.8, backlog
//! E12-6, journal spec §7). Pure: no I/O of its own, no clock, no randomness, and it writes nothing
//! to any journal. A stream or event of another workspace, a malformed id, and an absent one all
//! read as the same [`AuditError::NotFound`] (API-9). Pages follow API-15 and DEC-770: `seq`
//! order, each event's canonical body bytes with its `hash` and `prev_hash`, and the cursor is the
//! last `seq` served.

use mandate_canon::Digest;
use mandate_journal::MemoryJournal;

/// Why an audit read returned nothing. `NotFound` carries no detail, so a foreign id and an absent
/// one cannot be told apart by their error (API-9).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditError {
    /// The body of every stub in the tests PR (DEC-77, DEC-83).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// The stream, event or workspace is absent, malformed, or in another workspace (API-9).
    #[error("not found")]
    NotFound,
    /// A page `limit` outside `1..=500` (DEC-770 item 1). Refused, never clamped.
    #[error("page limit {limit} is outside 1..=500")]
    LimitOutOfRange { limit: u64 },
}

/// The caller's workspace, taken from its authenticated principal: one `[A-Za-z0-9_-]+` segment
/// (journal spec §2).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct WorkspaceId(String);

impl WorkspaceId {
    /// A malformed id names no workspace, so it reads as [`AuditError::NotFound`].
    pub fn parse(text: &str) -> Result<Self, AuditError> {
        let _ = text;
        Err(AuditError::Unimplemented { story: "E12-6" })
    }
}

/// How many events one page may hold: `1..=500`, and 100 when the caller names none (DEC-770
/// item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageLimit(u16);

impl PageLimit {
    pub fn new(limit: Option<u64>) -> Result<Self, AuditError> {
        let _ = limit;
        Err(AuditError::Unimplemented { story: "E12-6" })
    }
}

/// A stream of the workspace and its head: the last `seq` and that event's hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamHead {
    pub stream_id: String,
    pub seq: u64,
    pub hash: Digest,
}

/// One stored event as an audit read serves it: the exact canonical body bytes that were hashed,
/// the stored `hash`, and the `prev_hash` that links it to the event before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalEvent {
    pub stream_id: String,
    pub seq: u64,
    pub event_id: String,
    pub body: Vec<u8>,
    pub hash: Digest,
    pub prev_hash: Digest,
}

/// A page of one stream (API-15): the events after the request's `after_seq`, in `seq` order, and
/// the cursor to pass as the next `after_seq`: the last `seq` served, or the request's own
/// `after_seq` when the page is empty (DEC-770 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub events: Vec<JournalEvent>,
    pub cursor: u64,
}

/// The journal reads the audit explorer needs, each scoped to the caller's workspace. An adapter
/// implements them over its store: [`MemoryRead`] here, the Postgres store in
/// `mandate-journal-pg`.
pub trait JournalRead {
    /// The workspace's streams that hold at least one event, with their heads, sorted by
    /// `stream_id` bytes (DEC-770 item 3).
    fn streams(&self, workspace: &WorkspaceId) -> Result<Vec<StreamHead>, AuditError>;

    /// The events of `stream_id` after `after_seq`, at most `limit` of them.
    fn page(
        &self,
        workspace: &WorkspaceId,
        stream_id: &str,
        after_seq: u64,
        limit: PageLimit,
    ) -> Result<Page, AuditError>;

    /// The event whose `event_id` is `event_id`, when its stream is in `workspace`.
    fn event(&self, workspace: &WorkspaceId, event_id: &str) -> Result<JournalEvent, AuditError>;
}

/// [`JournalRead`] over the in-memory journal.
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
    fn streams(&self, workspace: &WorkspaceId) -> Result<Vec<StreamHead>, AuditError> {
        let _ = (self.journal, workspace);
        Err(AuditError::Unimplemented { story: "E12-6" })
    }

    fn page(
        &self,
        workspace: &WorkspaceId,
        stream_id: &str,
        after_seq: u64,
        limit: PageLimit,
    ) -> Result<Page, AuditError> {
        let _ = (self.journal, workspace, stream_id, after_seq, limit.0);
        Err(AuditError::Unimplemented { story: "E12-6" })
    }

    fn event(&self, workspace: &WorkspaceId, event_id: &str) -> Result<JournalEvent, AuditError> {
        let _ = (self.journal, workspace, event_id);
        Err(AuditError::Unimplemented { story: "E12-6" })
    }
}
