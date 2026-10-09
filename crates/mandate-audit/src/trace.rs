//! The causal trace (workspace API §4.8.1 "Causal trace", J2, FR-7.2, backlog E12-1, DEC-761,
//! DEC-762, DEC-772): a breadth-first walk from one event back to its causes, through the
//! envelope's `causation_id` and the payload link members of §4.8.1's table only, every lookup
//! scoped to the caller's workspace (AU-1), bounded and visiting each event once (AU-3), with model
//! output only in each node's `quoted` list (AU-4, API-18).

use mandate_canon::Digest;
use mandate_identity::Tenant;

use crate::{AuditError, JournalEvent, MemoryRead};

/// The deepest a trace goes: a node is at most 16 hops from the start (DEC-762 item 2).
pub const MAX_DEPTH: u16 = 16;

/// The most events a trace holds, the start included (DEC-762 item 2).
pub const MAX_NODES: usize = 256;

/// The most hops a trace records; the walk stops at this many (DEC-762 item 2).
pub const MAX_HOPS: usize = 1024;

/// What became of one link target, in the order DEC-772 item 2 checks them (DEC-762 items 2 to 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HopStatus {
    /// The target is already in the trace; it is not followed again.
    AlreadyShown,
    /// The depth or event bound kept the target out of the trace.
    BeyondBound,
    /// The target is absent, in another workspace, or not on the stream the row names.
    NotRecorded,
    /// The target is in the workspace but of a type the row does not name: shown, not followed.
    UnexpectedType { event_type: String },
    /// The target was added to the trace at the next depth and is followed in its turn.
    Shown,
}

/// One link the walk met: `from` the event that holds it, `link` the member's name as §4.8.1's
/// table writes it (`causation_id`, `payload.intent_id`, `payload.evidence[]`, …), and `to` the
/// target's `event_id` when the walk resolved it in the workspace, `None` otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    pub from: String,
    pub link: String,
    pub to: Option<String>,
    pub status: HopStatus,
}

/// Who a quoted output is attributed to (`QuotedContent.author`, mandate spec §6.4, DEC-773).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Author {
    OwnerSelected,
    PlatformAuthored,
}

/// `QuotedContent` of `schemas/workspace-api/common.schema.json`, with no member added: `text` is
/// empty when the member holds an artifact ref, and `artifact` is that ref as `sha256:` text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuotedContent {
    pub author: Author,
    pub text: String,
    pub model_id: String,
    pub model_version: String,
    pub produced_at: String,
    pub event_id: String,
    pub artifact: Option<String>,
}

/// One model-authored member of a node's event: `path` its JSON Pointer in the event body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quoted {
    pub path: String,
    pub quoted: QuotedContent,
}

/// One event of the trace: the page item with its `stream_id`, its depth, and its quoted members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceNode {
    pub event: JournalEvent,
    pub depth: u16,
    pub quoted: Vec<Quoted>,
}

/// One stream the walk read, at its head in the walk's snapshot (`AsOf`); the API adds the
/// `sha256:` prefix to `hash`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Watermark {
    pub stream_id: String,
    pub seq: u64,
    pub hash: Digest,
    pub recorded_at: String,
}

/// The response of `GET /journal/events/{event_id}/trace`: nodes in the order the walk added them,
/// hops in the order it met them, and `as_of` in ascending `stream_id` bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Trace {
    pub start: String,
    pub nodes: Vec<TraceNode>,
    pub hops: Vec<Hop>,
    pub truncated: bool,
    pub as_of: Vec<Watermark>,
}

/// The causal trace over a store. A start event outside the tenant's workspace is the one
/// [`AuditError::NotFound`]; every link that cannot be shown is a hop, never an error.
pub trait TraceRead {
    fn trace(&self, tenant: &impl Tenant, event_id: &str) -> Result<Trace, AuditError>;
}

impl TraceRead for MemoryRead<'_> {
    fn trace(&self, _tenant: &impl Tenant, _event_id: &str) -> Result<Trace, AuditError> {
        Err(AuditError::Unimplemented { story: "E12-1" })
    }
}
