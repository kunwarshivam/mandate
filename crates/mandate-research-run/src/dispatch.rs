//! Running a reply's reads and thinks ([agent harness spec](../../../docs/specs/agent-harness.md)
//! v0.2 §6.10 and §6.11; ADR-0005 slice 3; DEC-2203, DEC-2209): the worker's half of a dispatch.
//!
//! The core has already ordered the reply, screened every read against the run's frozen scope, and
//! applied the in-batch bounds ([`crate::agentic::RunState::on_reply`]). What it hands on is run
//! here, one call at a time in reply order, over the read-only ports: each read takes its scope from
//! the harness, every answer is filtered on knowledge time again (HI-13), a harness cap drops the
//! oldest items first and sets `truncated`, and a refused or failed call returns `{"error": code}`
//! and nothing else. A read queries the store and never causes a fetch (HI-28).
//!
//! Like the rest of the crate this is pure: no clock and no I/O of its own. `tool_timeout_s` is the
//! shell's to enforce on the wall clock; a reader past it answers [`crate::ReadError::Unavailable`].

use crate::agentic::{Code, LoopBounds, ToolCall, ToolCallId, ToolOutcome};
use crate::error::ResearchRunError;
use crate::plan::Scope;
use crate::ports::Readers;
use crate::retrieve::RetrievedItem;

/// What a dispatch reads within: the run's frozen scope, its pinned bounds, and the result bytes the
/// run has left (`max_run_result_bytes` less every earlier read's result).
#[derive(Debug, Clone, Copy)]
pub struct DispatchScope<'a> {
    pub scope: &'a Scope,
    pub bounds: &'a LoopBounds,
    pub result_bytes_left: u32,
}

/// One call's result, as the worker hands it back and `ToolResultRecorded` journals it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatched {
    pub call: ToolCallId,
    pub outcome: ToolOutcome,
    pub code: Option<Code>,
    /// The result as the model is given it: canonical JSON, stored as the record's `content_ref`.
    pub content: Vec<u8>,
    pub truncated: bool,
    /// The result holds third-party text, so an action written after it names it as provenance.
    pub untrusted: bool,
    /// Every item the result holds, in the result's order, each as it will be observed.
    pub items: Vec<RetrievedItem>,
}

/// A dispatch's results, in the order of the calls, and the run's result bytes left after them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dispatch {
    pub results: Vec<Dispatched>,
    pub result_bytes_left: u32,
}

/// Runs `calls`, the reads and thinks of turn `turn`'s reply the core dispatched, in their order.
///
/// # Errors
/// [`ResearchRunError::LoopOrder`] for an action, which the core mediates and never dispatches;
/// [`ResearchRunError`] when a result cannot be written canonically.
pub fn dispatch(
    turn: u32,
    calls: &[ToolCall],
    within: DispatchScope<'_>,
    readers: Readers<'_>,
) -> Result<Dispatch, ResearchRunError> {
    let _ = (turn, calls, within, readers);
    Err(ResearchRunError::Unimplemented { story: "E19-13" })
}
