//! The crate's one error enum, with a stable reason `code()` per variant (ADR-0001 ES-09).

use mandate_approval::ApprovalError;
use mandate_canon::ParseErrorKind;
use mandate_journal::InvalidReason;
use mandate_num::NumError;
use mandate_time::TimeError;

/// Every way a fold or a step can refuse. Nothing here is recoverable by retrying the same input:
/// a refusal means the runtime does not know what the input means, so it stops rather than guessing
/// (`AGENTS.md` rule 3).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeError {
    /// The body of every stub in the tests PR (DEC-77, DEC-83).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// An event type, payload field, or command the core does not interpret. Never a silent no-op
    /// ([DEC-85](../../../docs/project/04-decision-log.md#decisions)).
    #[error("{what} is not interpreted until {story}")]
    NotInterpreted { what: String, story: &'static str },
    /// A `seq` that is not exactly one above the stream's folded position (journal spec §2).
    #[error("stream {stream} expected seq {expected}, got {found}")]
    SequenceOutOfOrder {
        stream: String,
        expected: u64,
        found: u64,
    },
    /// An event on a stream this runtime does not follow.
    #[error("stream {stream} is not one this runtime follows")]
    ForeignStream { stream: String },
    /// A copied cross-stream fact with no `causation_id` (journal spec §2).
    #[error("{event_type} was copied without a causation id")]
    CopyWithoutCausation { event_type: String },
    /// A risk clock that moved backwards (mandate spec §5.2: monotone whole seconds).
    #[error("the risk clock went backwards from {last} to {found}")]
    RiskClockWentBackwards { last: i64, found: i64 },
    /// A live input arrived while an append's outcome was still unresolved (journal spec §5.1).
    #[error("an append at head {head} is unresolved, so no further input may be handled")]
    AppendUnresolved { head: u64 },
    /// A step was taken with a writer epoch other than the one the state was folded under.
    #[error("epoch {found} is not the folded epoch {folded}")]
    EpochMismatch { folded: u64, found: u64 },
    /// `Input::Started` arrived on a state that had already started.
    #[error("the runtime has already started")]
    AlreadyStarted,
    /// Any input other than `Input::Started` before the runtime has started.
    #[error("the runtime has not started")]
    NotStarted,
    /// A payload value that is not canonical, or a field of the wrong type (journal spec §4).
    #[error("payload field {field} is not canonical")]
    NonCanonicalPayload { field: String },
    /// An approval response for a request the fold does not know.
    #[error("no approval request is pending for {approval}")]
    UnknownApproval { approval: String },
    /// A value an approval or an owner command holds that `mandate-approval` cannot represent, which
    /// the runtime reads as "do not act" (`AGENTS.md` rule 3).
    #[error(transparent)]
    Approval(#[from] ApprovalError),
    #[error(transparent)]
    Json(#[from] JsonError),
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
}

/// A parse or validation failure from the journal and canonical layers, kept as one variant so
/// this crate's `code()` set stays closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum JsonError {
    #[error("a payload value could not be parsed: {0:?}")]
    Parse(ParseErrorKind),
    #[error("a draft was rejected by the journal: {0:?}")]
    Invalid(InvalidReason),
}

impl RuntimeError {
    /// The stable reason code. Callers match on this, never on the message
    /// ([ADR-0001](../../../docs/adr/0001-engineering-setup.md) ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::NotInterpreted { .. } => "not_interpreted",
            Self::SequenceOutOfOrder { .. } => "sequence_out_of_order",
            Self::ForeignStream { .. } => "foreign_stream",
            Self::CopyWithoutCausation { .. } => "copy_without_causation",
            Self::RiskClockWentBackwards { .. } => "risk_clock_went_backwards",
            Self::AppendUnresolved { .. } => "append_unresolved",
            Self::EpochMismatch { .. } => "epoch_mismatch",
            Self::AlreadyStarted => "already_started",
            Self::NotStarted => "not_started",
            Self::NonCanonicalPayload { .. } => "non_canonical_payload",
            Self::UnknownApproval { .. } => "unknown_approval",
            Self::Approval(_) => "approval",
            Self::Json(_) => "payload_rejected",
            Self::Num(_) => "arithmetic",
            Self::Time(_) => "time",
        }
    }
}
