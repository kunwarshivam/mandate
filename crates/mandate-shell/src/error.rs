//! Why the tracer stopped, with a stable reason `code()` per variant (ADR-0001 ES-09).
//!
//! No variant carries a credential, an account number, an order detail, or a mandate's content
//! (`AGENTS.md` rules 6 and 7, TI-8): the upstream errors carried here name paths, codes, and story
//! ids only.

use mandate_backtest::{BacktestError, Signal};
use mandate_builder::BuilderError;
use mandate_executor::{ConnectorError, ExecutorError};
use mandate_risk::GateError;
use mandate_runtime::{RuntimeError, SinkError};
use mandate_spec::SpecError;

use crate::stages::Stage;

/// Why one stage did not answer, in the stage's own crate's vocabulary. Every variant is a
/// non-answer: none of them is ever mapped to a permitting verdict (TI-3).
#[derive(Debug, thiserror::Error)]
pub enum Cause {
    /// The body of every production adapter in this tests PR (DEC-77, DEC-83).
    #[error("this stage is not implemented yet (pending {story})")]
    Unimplemented { story: &'static str },
    #[error(transparent)]
    Spec(#[from] SpecError),
    #[error(transparent)]
    Backtest(#[from] BacktestError),
    #[error(transparent)]
    Builder(#[from] BuilderError),
    #[error(transparent)]
    Gate(#[from] GateError),
    /// The gate answered, and its answer permits nothing. `reason_code` is the gate's own.
    #[error("the gate did not allow the proposal ({reason_code})")]
    GateAnswered { reason_code: String },
    #[error(transparent)]
    Executor(#[from] ExecutorError),
    #[error(transparent)]
    Connector(#[from] ConnectorError),
    #[error(transparent)]
    Sink(#[from] SinkError),
    /// An append answered neither `Committed` nor `AlreadyCommitted` (journal spec §5.1).
    #[error("the journal answered {outcome}")]
    Append { outcome: &'static str },
    /// The signal answered, and its answer opens nothing.
    #[error("the signal is {0:?}, which opens nothing")]
    NotLong(Signal),
    /// The mandate admits no proposal of this autonomy without a person, and the tracer has no
    /// escalation (TI-9).
    #[error("the proposal classified {autonomy}, and the tracer acts only on auto")]
    NotAuto { autonomy: &'static str },
    /// A data or planning stage answered with nothing usable.
    #[error("{what}")]
    Absent { what: &'static str },
    /// The flatten planner could not plan mid-run, so the runner halts rather than act on a plan
    /// nobody computed (task brief finding 2).
    #[error("the flatten planner could not plan: {0}")]
    Poisoned(String),
}

impl Cause {
    /// The upstream code, carried into a dry-run verdict and into the run's report.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unimplemented { .. } => "unimplemented",
            Self::Spec(e) => e.code(),
            Self::Backtest(e) => e.code(),
            Self::Builder(e) => e.code(),
            Self::Gate(e) => e.code(),
            Self::GateAnswered { .. } => "gate_answered",
            Self::Executor(e) => e.code(),
            Self::Connector(e) => connector_code(e),
            Self::Sink(e) => sink_code(e),
            Self::Append { .. } => "append_refused",
            Self::NotLong(_) => "not_long",
            Self::NotAuto { .. } => "not_auto",
            Self::Absent { .. } => "absent",
            Self::Poisoned(_) => "poisoned",
        }
    }
}

fn connector_code(error: &ConnectorError) -> &'static str {
    match error {
        ConnectorError::Unknown(unknown) => unknown.code(),
        ConnectorError::Unreadable { code } | ConnectorError::NotSent { code } => code,
    }
}

fn sink_code(error: &SinkError) -> &'static str {
    match error {
        SinkError::Unavailable => "sink_unavailable",
        SinkError::Refused { .. } => "sink_refused",
    }
}

/// Why a run stopped. Every variant means that no order was sent after the stop.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// A stage did not answer, or answered with nothing that permits an order.
    #[error("the tracer stopped at {stage}: {cause}")]
    Refused { stage: Stage, cause: Cause },
    /// The runtime refused an input: it does not know what the input means (`AGENTS.md` rule 3).
    #[error("the runtime refused its input: {0}")]
    Runtime(#[from] RuntimeError),
    /// The mandate's environment is not `paper`, or a draft would say so (TI-7).
    #[error("the tracer runs only against the paper environment")]
    NonPaperEnvironment,
    /// An environment variable tried to point the tracer at a host (TI-5, task brief control 3).
    #[error("{var} looks like a URL; the tracer reaches only the Alpaca paper host")]
    NonPaperHost { var: String },
    /// The mandate's working universe is not exactly one pinned instrument.
    #[error("the tracer trades exactly one pinned instrument")]
    UniverseNotPinned,
    /// The agent stream already carries an intent, so a new cycle could buy a second share (TI-12).
    #[error("the agent stream already carries an intent; pass --new-cycle to start another")]
    CycleAlreadyOpen,
    /// The startup reconciliation found the journal and the broker disagreeing. The agent stays
    /// paused and nothing in the tracer lifts it (trading-domain spec §11, TI-12).
    #[error("the startup reconciliation found a mismatch; the agent stays paused")]
    ReconciliationMismatch,
    /// An intent or a submission was about to go out without its journaled record (TI-1, rule 5).
    #[error("a {effect} was not preceded by its committed draft")]
    WriteAheadViolated { effect: &'static str },
    /// The executor asked for a second submission in one run. The tracer places one order.
    #[error("the tracer places one order per run, and the executor asked for another")]
    SecondSubmission,
    /// The builder proposed a quantity of zero, which no order can carry (PB-14).
    #[error("the order builder proposed a quantity of zero")]
    ProposalInvalid,
    /// A journal envelope could not be composed. Nothing is appended, so nothing acts.
    #[error("the journal envelope's {field} could not be composed")]
    Envelope { field: &'static str },
    /// The command line asked for something the tracer does not do.
    #[error("{0}")]
    Usage(String),
}

impl ShellError {
    /// The stable reason code the process exits with (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Refused { stage, .. } => stage.code(),
            Self::Runtime(e) => e.code(),
            Self::NonPaperEnvironment => "non_paper_environment",
            Self::NonPaperHost { .. } => "non_paper_host",
            Self::UniverseNotPinned => "universe_not_pinned",
            Self::CycleAlreadyOpen => "cycle_already_open",
            Self::ReconciliationMismatch => "reconciliation_mismatch",
            Self::WriteAheadViolated { .. } => "write_ahead_violated",
            Self::SecondSubmission => "one_order_only",
            Self::ProposalInvalid => "proposal_invalid",
            Self::Envelope { .. } => "envelope_not_composed",
            Self::Usage(_) => "usage",
        }
    }

    /// The stage that stopped the run, when a stage did.
    pub fn stage(&self) -> Option<Stage> {
        match self {
            Self::Refused { stage, .. } => Some(*stage),
            Self::Runtime(_)
            | Self::NonPaperEnvironment
            | Self::NonPaperHost { .. }
            | Self::UniverseNotPinned
            | Self::CycleAlreadyOpen
            | Self::ReconciliationMismatch
            | Self::WriteAheadViolated { .. }
            | Self::SecondSubmission
            | Self::ProposalInvalid
            | Self::Envelope { .. }
            | Self::Usage(_) => None,
        }
    }
}

/// A stage's non-answer as the run's refusal.
pub(crate) fn refused(stage: Stage) -> impl FnOnce(Cause) -> ShellError {
    move |cause| ShellError::Refused { stage, cause }
}
