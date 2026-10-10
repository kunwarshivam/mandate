//! Why the tracer stopped, with a stable reason `code()` per variant (ADR-0001 ES-09).
//!
//! No variant carries a credential, an account number, an order detail, or a mandate's content
//! (`AGENTS.md` rules 6 and 7, TI-8): the upstream errors carried here name paths, codes, and story
//! ids only.

use mandate_backtest::{BacktestError, Signal};
use mandate_builder::BuilderError;
use mandate_executor::{ConnectorError, ExecutorError};
use mandate_marketdata::dataset::DatasetError;
use mandate_marketdata::inspect::InspectError;
use mandate_marketdata::model::ModelError;
use mandate_num::NumError;
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
    #[error(transparent)]
    Inspect(#[from] InspectError),
    #[error(transparent)]
    Dataset(#[from] DatasetError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Num(#[from] NumError),
    /// The stored data answered, and the answer is not one to decide on.
    #[error("the stored data cannot be trusted: {what}")]
    Untrusted { what: &'static str },
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
            Self::Inspect(e) => e.code(),
            Self::Dataset(e) => e.code(),
            Self::Model(e) => e.code(),
            Self::Num(e) => e.code(),
            Self::Untrusted { .. } => "untrusted",
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
    /// E1b's watch reached the end of the regular session without the broker confirming the
    /// entry's cancel (DEC-858 item 5, DEC-877 item 1). The entry's `pending_cancel` stays
    /// journaled for the next start's reconciliation; nothing more is placed or cancelled.
    #[error(
        "the entry's cancel was unconfirmed at the session's end; the next start reconciles it"
    )]
    CancelUnconfirmed,
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
            Self::Refused {
                stage: Stage::Connector,
                cause: Cause::Connector(ConnectorError::Unreadable { .. }),
            } => "broker_answer_uninterpretable",
            Self::Refused {
                stage: Stage::Connector,
                cause: Cause::Connector(ConnectorError::Unknown(_)),
            } => "broker_outcome_unknown",
            Self::Refused { stage, .. } => stage.code(),
            Self::Runtime(e) => e.code(),
            Self::NonPaperEnvironment => "non_paper_environment",
            Self::NonPaperHost { .. } => "non_paper_host",
            Self::UniverseNotPinned => "universe_not_pinned",
            Self::CycleAlreadyOpen => "cycle_already_open",
            Self::ReconciliationMismatch => "reconciliation_mismatch",
            Self::WriteAheadViolated { .. } => "write_ahead_violated",
            Self::SecondSubmission => "one_order_only",
            Self::CancelUnconfirmed => "cancel_unconfirmed",
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
            | Self::CancelUnconfirmed
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use mandate_executor::{BrokerUnknown, ConnectorError};
    use mandate_marketdata::dataset::DatasetError;
    use mandate_marketdata::inspect::InspectError;
    use mandate_marketdata::model::ModelError;
    use mandate_num::NumError;
    use mandate_runtime::SinkError;

    use super::{Cause, ShellError};
    use crate::stages::Stage;

    #[test]
    fn a_cause_carries_its_upstream_code() {
        let cases = [
            (Cause::Unimplemented { story: "E7-7" }, "unimplemented"),
            (
                Cause::Connector(ConnectorError::Unknown(BrokerUnknown::Timeout)),
                "timeout",
            ),
            (
                Cause::Connector(ConnectorError::Unreadable { code: "wire" }),
                "wire",
            ),
            (
                Cause::Connector(ConnectorError::NotSent {
                    code: "refused_path",
                }),
                "refused_path",
            ),
            (Cause::Sink(SinkError::Unavailable), "sink_unavailable"),
            (
                Cause::Sink(SinkError::Refused {
                    reason: "closed".to_owned(),
                }),
                "sink_refused",
            ),
            (Cause::Append { outcome: "Fenced" }, "append_refused"),
            (Cause::Absent { what: "gap" }, "absent"),
            (Cause::Untrusted { what: "gap" }, "untrusted"),
            (
                Cause::Inspect(InspectError::Overflow { column: "close" }),
                "overflow",
            ),
            (
                Cause::Dataset(DatasetError::Manifest {
                    path: PathBuf::from("manifest.json"),
                    reason: "absent".to_owned(),
                }),
                "manifest",
            ),
            (
                Cause::Model(ModelError::Timeframe("2Day".to_owned())),
                "timeframe",
            ),
            (Cause::Num(NumError::NotCanonical), "not_canonical"),
        ];
        for (cause, code) in cases {
            assert_eq!(cause.code(), code, "{cause:?}");
        }
    }

    #[test]
    fn a_refusal_reports_its_stages_code_and_names_its_stage() {
        for stage in Stage::ALL {
            let error = ShellError::Refused {
                stage,
                cause: Cause::Unimplemented { story: "E7-7" },
            };
            assert_eq!(error.code(), stage.code());
            assert_eq!(error.stage(), Some(stage));
            assert_eq!(
                error.to_string(),
                format!(
                    "the tracer stopped at {stage:?}: this stage is not implemented yet (pending E7-7)"
                )
            );
        }
        assert_eq!(ShellError::CycleAlreadyOpen.stage(), None);
        assert_eq!(ShellError::SecondSubmission.code(), "one_order_only");
    }

    /// A request that left the process and got no answer is not one that never left it (#288
    /// review, minor 3): the connector stage reports each of its three non-answers by its own code.
    #[test]
    fn the_connector_stage_tells_unsent_unread_and_unknown_apart() {
        let refused = |cause| ShellError::Refused {
            stage: Stage::Connector,
            cause,
        };
        let codes = [
            refused(Cause::Connector(ConnectorError::NotSent {
                code: "refused_path",
            }))
            .code(),
            refused(Cause::Connector(ConnectorError::Unreadable {
                code: "wire",
            }))
            .code(),
            refused(Cause::Connector(ConnectorError::Unknown(
                BrokerUnknown::Timeout,
            )))
            .code(),
        ];
        assert_eq!(
            codes,
            [
                "broker_request_not_sent",
                "broker_answer_uninterpretable",
                "broker_outcome_unknown"
            ]
        );
    }
}
