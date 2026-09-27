//! The crate's one error enum, with a stable reason `code()` per variant (ADR-0001 ES-09).

use mandate_accounting::AccountingError;
use mandate_canon::ParseErrorKind;
use mandate_journal::InvalidReason;
use mandate_num::NumError;
use mandate_time::TimeError;

/// Every way a fold, a step, or a reconciliation can refuse.
///
/// Nothing here is recoverable by retrying the same input: a refusal means the executor does not
/// know what the input means, so it stops rather than guessing (`AGENTS.md` rule 3). A refusal is
/// never a broker rejection — a rejection is a folded fact, not an error.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ExecutorError {
    /// The body of every stub in the tests PR (DEC-77, DEC-83).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
    /// An event type, payload field, broker status, or command this crate does not interpret.
    /// Never a silent no-op ([DEC-85](../../../docs/project/04-decision-log.md#decisions)).
    #[error("{what} is not interpreted until {story}")]
    NotInterpreted { what: String, story: &'static str },
    /// A `seq` that is not exactly one above the stream's folded position (journal spec §2).
    #[error("stream {stream} expected seq {expected}, got {found}")]
    SequenceOutOfOrder {
        stream: String,
        expected: u64,
        found: u64,
    },
    /// An event on a stream this executor does not follow.
    #[error("stream {stream} is not one this executor follows")]
    ForeignStream { stream: String },
    /// A second `StreamOpened`: the first fixed the stream's `environment` for good, so another is
    /// refused whether it names the same environment or a different one (ADR-0001 ES-23).
    #[error("the stream was opened in {opened}; a second StreamOpened names {found}")]
    EnvironmentMismatch { opened: String, found: String },
    /// A copied cross-stream fact with no `causation_id` (journal spec §2).
    #[error("{event_type} was copied without a causation id")]
    CopyWithoutCausation { event_type: String },
    /// A risk input with no `risk_clock`, or one below the stream's last (journal spec §2).
    #[error("{event_type} carries no usable risk clock")]
    RiskClockMissing { event_type: String },
    /// A risk clock that moved backwards (mandate spec §5.2: monotone whole seconds).
    #[error("the risk clock went backwards from {last} to {found}")]
    RiskClockWentBackwards { last: i64, found: i64 },
    /// A live input arrived while an append's outcome was still unresolved (journal spec §5.1).
    #[error("an append at head {head} is unresolved, so no further input may be handled")]
    AppendUnresolved { head: u64 },
    /// A newer epoch owns the stream and this process is a ghost (journal spec §5.1).
    #[error("epoch {found} has been fenced by {owner}")]
    Fenced { owner: u64, found: u64 },
    /// A step was taken with a writer epoch other than the one the state was folded under.
    #[error("epoch {found} is not the folded epoch {folded}")]
    EpochMismatch { folded: u64, found: u64 },
    /// `Input::Started` arrived on a state that had already started.
    #[error("the executor has already started")]
    AlreadyStarted,
    /// Any input other than `Input::Started` before the executor has started.
    #[error("the executor has not started")]
    NotStarted,
    /// A payload value that is not canonical, or a field of the wrong type (journal spec §4).
    #[error("payload field {field} is not canonical")]
    NonCanonicalPayload { field: String },
    /// A `client_order_id` that does not match the derivation grammar (task brief interpretation 6).
    #[error("client order id {raw} is not one this platform could derive")]
    MalformedClientOrderId { raw: String },
    /// A broker status outside trading-domain spec §5.7's table. The spec's own last row pauses the
    /// agent and alerts, which the step does; the fold refuses, because a status it cannot map is a
    /// state it cannot derive.
    #[error("broker status {status} is outside the section 5.7 table")]
    UnmappedBrokerStatus { status: String },
    /// An order the fold does not carry.
    #[error("no order is known for {client_order_id}")]
    UnknownOrder { client_order_id: String },
    /// A reconciliation that was published against a head its snapshot did not cover
    /// (task brief interpretation 15).
    #[error("the snapshot was taken at head {snapshot}, but the stream is at {head}")]
    SnapshotOvertaken { snapshot: u64, head: u64 },
    #[error(transparent)]
    Json(#[from] JsonError),
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
    #[error(transparent)]
    Accounting(#[from] AccountingError),
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

impl ExecutorError {
    /// The stable reason code. Callers match on this, never on the message
    /// ([ADR-0001](../../../docs/adr/0001-engineering-setup.md) ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::NotInterpreted { .. } => "not_interpreted",
            Self::SequenceOutOfOrder { .. } => "sequence_out_of_order",
            Self::ForeignStream { .. } => "foreign_stream",
            Self::EnvironmentMismatch { .. } => "environment_mismatch",
            Self::CopyWithoutCausation { .. } => "copy_without_causation",
            Self::RiskClockMissing { .. } => "risk_clock_missing",
            Self::RiskClockWentBackwards { .. } => "risk_clock_went_backwards",
            Self::AppendUnresolved { .. } => "append_unresolved",
            Self::Fenced { .. } => "fenced",
            Self::EpochMismatch { .. } => "epoch_mismatch",
            Self::AlreadyStarted => "already_started",
            Self::NotStarted => "not_started",
            Self::NonCanonicalPayload { .. } => "non_canonical_payload",
            Self::MalformedClientOrderId { .. } => "malformed_client_order_id",
            Self::UnmappedBrokerStatus { .. } => "unmapped_broker_status",
            Self::UnknownOrder { .. } => "unknown_order",
            Self::SnapshotOvertaken { .. } => "snapshot_overtaken",
            Self::Json(_) => "payload_rejected",
            Self::Num(_) => "arithmetic",
            Self::Time(_) => "time",
            Self::Accounting(_) => "accounting",
        }
    }

    /// Every code this crate can answer with, in the order [`Self::code`] matches them. The set is
    /// closed, which is what `hand::every_error_code_is_stable_and_unique` checks (ES-09).
    pub const CODES: [&'static str; 22] = [
        "unimplemented",
        "not_interpreted",
        "sequence_out_of_order",
        "foreign_stream",
        "environment_mismatch",
        "copy_without_causation",
        "risk_clock_missing",
        "risk_clock_went_backwards",
        "append_unresolved",
        "fenced",
        "epoch_mismatch",
        "already_started",
        "not_started",
        "non_canonical_payload",
        "malformed_client_order_id",
        "unmapped_broker_status",
        "unknown_order",
        "snapshot_overtaken",
        "payload_rejected",
        "arithmetic",
        "time",
        "accounting",
    ];
}
