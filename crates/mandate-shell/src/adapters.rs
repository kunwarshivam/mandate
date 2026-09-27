//! The production adapters: one per [`Stage`](crate::Stage), each binding one crate's public API.
//!
//! **Every adapter is a stub in this tests PR** (DEC-77 stage 2, the coordinator's ruling on
//! E7-7): each answers [`Cause::Unimplemented`], or for the connector the `NotSent` refusal a
//! connector gives for a request it does not implement, so [`crate::run`] over [`production`]
//! refuses at its first probe and places no order. The implementation PR fills them in, one
//! upstream stream's merge at a time, and changes no test.
//!
//! What each will bind, per the task brief's step table:
//!
//! | Adapter | Binds |
//! |---|---|
//! | [`RiskExitPath`] | `mandate_risk::agent_flatten` (E6-3) |
//! | [`ExecutorProtection`] | `mandate_executor::handle` on a synthetic protective input (E7-4) |
//! | [`SpecMandate`] | `mandate_spec::validate`, then `ValidatedMandate::new` (stream F) |
//! | [`StoredBars`] | `mandate_marketdata::dataset::read_manifest` and `dataset::read` (E2-1, E2-2) |
//! | [`MovingAverage`] | `mandate_backtest::Strategy::MovingAverageCrossover` (E4-2) |
//! | [`BuilderPlan`] | `mandate-builder`'s sizing and classification (stream H) |
//! | [`RiskGate`] | `mandate_risk::evaluate` as a dry run (E6-3, E6-6 to E6-8) |
//! | [`StoreJournal`] | `mandate_journal::MemoryJournal` in CI, `mandate-journal-pg` for the manual run |
//! | [`ExecutorSink`] | the two `IntentHandoff` types, joined with the deployment's `AgentId` |
//! | [`CoreExecutor`] | `mandate_executor::handle` and `fold` (stream K) |
//! | [`AlpacaConnector`] | `mandate_alpaca::TradingClient` over a `TradingTransport` (stream K) |
//! | [`ExecutorReconciler`] | `mandate_executor::reconcile` on the connector's snapshot (E7-3) |

use std::path::PathBuf;

use mandate_accounting::InstrumentId;
use mandate_backtest::Signal;
use mandate_executor::{BrokerOutcome, BrokerRequest, ConnectorError};
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_num::Price;
use mandate_risk::Decision;
use mandate_runtime::{
    AgentId, Autonomy, FlattenPlan, FlattenRequest, IntentHandoff, MandateView, Proposal,
    SignalInputs,
};

use crate::error::Cause;
use crate::stages::{
    Admitted, Bars, Classifier, Connector, Executor, ExitPath, Gate, JournalWriter, MandateSource,
    Protection, Reconciled, Reconciler, SignalModel, Sink, Sizing, Stages,
};

/// `mandate_risk::agent_flatten`, probed once against a synthetic request before anything starts.
pub struct RiskExitPath;

impl ExitPath for RiskExitPath {
    fn probe(&self) -> Result<(), Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn plan(&self, request: &FlattenRequest) -> Result<FlattenPlan, Cause> {
        let _ = request;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The executor's protective sequence, probed through `mandate_executor::handle`, since
/// `mod protection` is private (task brief, Decisions needed 4).
pub struct ExecutorProtection;

impl Protection for ExecutorProtection {
    fn probe(&self) -> Result<(), Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The mandate document at `path`, validated with no violation.
pub struct SpecMandate {
    pub path: PathBuf,
}

impl MandateSource for SpecMandate {
    fn admitted(&self) -> Result<Admitted, Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The stored daily bars under `dir`.
pub struct StoredBars {
    pub dir: PathBuf,
}

impl Bars for StoredBars {
    fn closes(&self, instrument: &InstrumentId) -> Result<Vec<Price>, Cause> {
        let _ = instrument;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// E4-2's moving-average baseline with the windows the mandate's model names.
pub struct MovingAverage;

impl SignalModel for MovingAverage {
    fn signal(&self, closes: &[Price]) -> Result<Signal, Cause> {
        let _ = closes;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate-builder`'s `conviction_linear` sizing and §6 classification.
pub struct BuilderPlan;

impl Sizing for BuilderPlan {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let _ = (view, inputs);
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

impl Classifier for BuilderPlan {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Autonomy, Cause> {
        let _ = (view, proposal);
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_risk::evaluate`, as the runtime's advisory pass. The binding call stays inside
/// `mandate-executor` (`AGENTS.md` rules 1 and 12).
pub struct RiskGate;

impl Gate for RiskGate {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause> {
        let _ = proposal;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// The journal store: `mandate_journal::MemoryJournal` for CI and a scratch run,
/// `mandate-journal-pg` at the DSN for the manual paper run.
pub struct StoreJournal {
    pub dsn: Option<String>,
}

impl JournalWriter for StoreJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        let _ = stream;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        let _ = stream;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let _ = (stream, expected_head, writer_epoch, drafts);
        AppendOutcome::Unavailable
    }
}

/// The runtime's `IntentSink`, handing to the executor with this deployment's `AgentId`.
pub struct ExecutorSink {
    pub agent: AgentId,
}

impl Sink for ExecutorSink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause> {
        let _ = handoff;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_executor::handle` and `fold` over the account stream's state.
pub struct CoreExecutor;

impl Executor for CoreExecutor {
    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let _ = input;
        Err(Cause::Unimplemented { story: "E7-7" })
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        let _ = event;
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// `mandate_alpaca::TradingClient` over `transport`: the scripted one in CI, `AlpacaPaperHttp` for
/// the manual run. The shell names no host and builds no URL (TI-5).
pub struct AlpacaConnector<T> {
    pub transport: T,
}

impl<T> Connector for AlpacaConnector<T> {
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let _ = request;
        Err(ConnectorError::NotSent {
            code: "unimplemented",
        })
    }
}

/// `mandate_executor::reconcile` on the broker snapshot the connector reads at startup.
pub struct ExecutorReconciler;

impl Reconciler for ExecutorReconciler {
    fn reconcile(&mut self) -> Result<Reconciled, Cause> {
        Err(Cause::Unimplemented { story: "E7-7" })
    }
}

/// Where the production stages read from.
pub struct Sources<T> {
    pub mandate: PathBuf,
    pub dataset: PathBuf,
    /// `None` is a scratch in-memory journal: a run that places nothing keeps nothing, so a later
    /// start cannot re-hand an intent a planning run proposed (DEC-157 item 6).
    pub journal: Option<String>,
    pub agent: AgentId,
    pub transport: T,
}

/// The production stages. Every one is a stub in this tests PR.
pub fn production<T: 'static>(sources: Sources<T>) -> Stages {
    Stages {
        exit: Box::new(RiskExitPath),
        protection: Box::new(ExecutorProtection),
        mandate: Box::new(SpecMandate {
            path: sources.mandate,
        }),
        bars: Box::new(StoredBars {
            dir: sources.dataset,
        }),
        signal: Box::new(MovingAverage),
        reconciler: Box::new(ExecutorReconciler),
        sizing: Box::new(BuilderPlan),
        classifier: Box::new(BuilderPlan),
        gate: Box::new(RiskGate),
        journal: Box::new(StoreJournal {
            dsn: sources.journal,
        }),
        sink: Box::new(ExecutorSink {
            agent: sources.agent,
        }),
        executor: Box::new(CoreExecutor),
        connector: Box::new(AlpacaConnector {
            transport: sources.transport,
        }),
    }
}
