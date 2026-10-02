//! The payload schemas journal spec §9.2 closes (DEC-261, E7-10): the control stream's records
//! that `ValidationContext::from_journal` reads, and `OwnerCommandRefused` on the agent and account
//! streams. Every rule only refuses a draft; none changes what a writer may do.
//!
//! **`AccountSnapshotRecorded` is not governed here yet** (DEC-261 item 7). The executor's fee
//! step still journals it without the three cash members §9.2 requires as `null`, and refusing that
//! snapshot at `append` must never stop the fee step pausing every agent and alerting the owner
//! (`AGENTS.md` rules 3 and 13). It is registered in the change that follows stream K's writer fix,
//! and until then it stays refused as `unknown_schema`, as it always was.
//!
//! The checks are stubs until E7-10's implementation: each governed draft is refused as
//! `unimplemented`.

use mandate_canon::Value;

use crate::{Invalid, InvalidReason, StreamId, StreamType};

/// The event types §9.2 closes on the control stream.
const CONTROL: [&str; 9] = [
    "StreamOpened",
    "ConnectionEstablished",
    "ConnectionRevoked",
    "DisclosureAccepted",
    "ConfigSnapshotRegistered",
    "MandateVersionCreated",
    "MandateConfirmed",
    "AgentDeployed",
    "AgentStopped",
];

/// The event type §9.2 closes on both the agent and the account stream.
const REFUSAL: &str = "OwnerCommandRefused";

/// Whether §9.2 governs `event_type` on `stream`. `AccountSnapshotRecorded` is held back (module
/// doc), and every other event keeps its own registration.
pub(crate) fn governs(stream: &StreamId, event_type: &str) -> bool {
    match stream.stream_type() {
        StreamType::Control => CONTROL.contains(&event_type),
        StreamType::Agent | StreamType::Account => event_type == REFUSAL,
        StreamType::Scheduler => false,
    }
}

/// The payload normalized against its §9.2 schema, then consistency rules 17 to 23 in number order
/// (reported only on a well-typed payload). `config_refs` is the envelope's, which rule 22 reads.
pub(crate) fn payload(
    _event_type: &str,
    _schema_version: u64,
    _payload: &Value,
    _config_refs: Option<&Value>,
) -> Result<Value, Invalid> {
    Err(Invalid::new(InvalidReason::Unimplemented, "payload"))
}

/// Subject rules 25 and 26 (`stream_mismatch`), then copy rule 27, on a payload that passed
/// [`payload`]: reported after `artifact_refs` and `pii_refs` (§9.1's order, which §9.2 keeps).
pub(crate) fn subject_and_copy(
    _event_type: &str,
    _stream: &StreamId,
    _payload: &Value,
    _causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    Err(Invalid::new(InvalidReason::Unimplemented, "payload"))
}
