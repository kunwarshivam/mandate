//! The model half of a recorded research bundle's `research.json` (DEC-2210 items 1 and 2,
//! DEC-2266, DEC-2326 item 3): the pinned entry, its one endpoint, what the endpoint reports, the
//! model's recorded reply and an agentic run's scripted turns, read from the parsed file with every
//! member required and nothing defaulted. The cell's bundle loader and the model-gateway service's
//! `--recorded` mode read the same file through this, so the two cannot drift apart.

use mandate_canon::Value;
use mandate_gateway::money::Usage;
use mandate_gateway::registry::{Endpoint, Registry, Reported};

use crate::entry::ResearchEntry;

/// Why the model half did not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordedError {
    /// The member named is missing, empty, or out of its range.
    Malformed(&'static str),
    /// The gateway's registry refused the entry or its endpoint.
    Unregistered,
}

/// The model's one recorded reply: the output text the fake replays, its latency, and its usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedReply {
    pub output: String,
    pub latency_ms: u64,
    pub usage: Usage,
}

/// One scripted tool-call reply of an agentic run's turn (DEC-2266): each call's declared tool
/// name and canonical arguments, in reply order, its latency, and its usage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptedTurn {
    pub calls: Vec<(String, String)>,
    pub latency_ms: u64,
    pub usage: Usage,
}

/// The model a recording pins, and what it replays.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedModel {
    pub entry: ResearchEntry,
    pub endpoint: Endpoint,
    /// What the endpoint reports it serves, which the registry probes it with.
    pub probe: Reported,
    pub reply: RecordedReply,
    /// The scripted replies of an agentic run's turns before its final one, turn 1 first; empty
    /// for a recording with no `turns` (DEC-2266).
    pub turns: Vec<ScriptedTurn>,
}

#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
impl RecordedModel {
    /// The model half of `research`, a parsed `research.json`.
    ///
    /// # Errors
    /// [`RecordedError::Malformed`] naming the first member that is missing, empty, or out of
    /// range.
    pub fn read(research: &Value) -> Result<Self, RecordedError> {
        let _ = research;
        todo!()
    }

    /// The gateway's registry: the entry, offered, on its one endpoint.
    ///
    /// # Errors
    /// [`RecordedError::Unregistered`] when the registry refuses the entry or the endpoint.
    pub fn registry(&self) -> Result<Registry, RecordedError> {
        let _ = self;
        todo!()
    }
}

/// The gateway's registry for `entry` on `endpoint`, probed with `probe`: the entry, offered.
///
/// # Errors
/// [`RecordedError::Unregistered`] when the registry refuses the entry or the endpoint.
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn registry(
    entry: &ResearchEntry,
    endpoint: &Endpoint,
    probe: &Reported,
) -> Result<Registry, RecordedError> {
    let _ = (entry, endpoint, probe);
    todo!()
}
