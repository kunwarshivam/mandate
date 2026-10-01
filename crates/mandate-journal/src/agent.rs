//! The agent stream's closed payload schemas (journal spec §9.1, DEC-177, DEC-252) and §11's two
//! agent-stream per-range checks (E7-9, DEC-168). Every rule only refuses a draft or fails a
//! range; none changes what a writer may do. The checks are stubs until E7-9's implementation:
//! each §9.1 draft is refused as `unimplemented`, as it was refused as `unknown_schema` before, and
//! every agent-stream range fails.

use mandate_canon::Value;

use crate::{Invalid, InvalidReason, StoredEvent, StreamId, StreamType, TrustedStart};

/// The event types §9.1 closes on the agent stream.
const CLOSED: [&str; 8] = [
    "StreamOpened",
    "ObservationRecorded",
    "ModelOutputRecorded",
    "DecisionMade",
    "IntentProposed",
    "AgentModeChanged",
    "KillSwitchActivated",
    "OwnerExitRequested",
];

/// Whether §9.1 governs `event_type` on `stream`; every other agent-stream event keeps its own
/// registration, which today is none.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    stream.stream_type() == StreamType::Agent && CLOSED.contains(&event_type)
}

/// The payload normalized against its §9.1 schema, then consistency rules 1 to 13 in number order
/// (reported only on a well-typed payload). `causation_id` is the envelope's, which rule 10 reads.
pub(crate) fn payload(
    event_type: &str,
    schema_version: u64,
    payload: &Value,
    causation_id: Option<&Value>,
) -> Result<Value, Invalid> {
    let _ = (event_type, schema_version, payload, causation_id);
    Err(Invalid::new(InvalidReason::Unimplemented, "payload"))
}

/// §11's per-range checks that span agent-stream events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStreamCheck {
    /// An `IntentProposed` differs in an action member from the `DecisionMade` it names.
    IntentActionMismatch,
    /// A `KillSwitchActivated`'s `mode_event` names no earlier `AgentModeChanged` with reason
    /// `kill_switch`.
    ModeEventMismatch,
    /// The stub's answer until E7-9's implementation lands.
    Unimplemented,
}

impl AgentStreamCheck {
    /// The check's code as the spec writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::IntentActionMismatch => "intent_action_mismatch",
            Self::ModeEventMismatch => "mode_event_mismatch",
            Self::Unimplemented => "unimplemented",
        }
    }
}

/// The first event (by its `seq` column) that fails an agent-stream range check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentStreamFailure {
    pub seq: u64,
    pub check: AgentStreamCheck,
}

/// Runs §11's `intent_action_mismatch` and `mode_event_mismatch` over `rows` of one agent stream,
/// in order, after [`crate::verify_events`] passed them. A reference to an event before the range
/// is not checked, except that on a full chain (`from_seq` 1) a `mode_event` naming no earlier
/// event fails. Rows of any other stream type pass, since neither check applies there.
pub fn verify_agent_stream(
    rows: &[StoredEvent],
    start: TrustedStart,
) -> Result<(), AgentStreamFailure> {
    let _ = (rows, start);
    Err(AgentStreamFailure {
        seq: 0,
        check: AgentStreamCheck::Unimplemented,
    })
}
