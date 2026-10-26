//! The tracer's stages: the [`Stage`] enum that is the fail-closed suite's contract, and one trait
//! per stage, which is the seam a production adapter and a test double both implement.
//!
//! A trait answers in its own crate's vocabulary — a `mandate_risk::Decision`, a
//! `mandate_executor::Effect`, an `AppendOutcome` — so the shell's mappings in [`crate::map`] are
//! tested over the values the real crates return, not over values the shell invented.

use std::fmt;
use std::sync::Arc;

use mandate_accounting::InstrumentId;
use mandate_canon::Digest;
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{BrokerOutcome, BrokerRequest, ConnectorError};
use mandate_journal::{AppendOutcome, ArtifactSource, Environment, StoredEvent};
use mandate_risk::Decision;
use mandate_runtime::{
    Classified, FlattenPlan, FlattenRequest, IntentHandoff, MandateView, Proposal, RiskClock,
    SignalInputs,
};

use crate::approvers::ApproverSource;
use crate::error::Cause;

/// One stage of the path, in the order the tracer reaches it (task brief, "The `Stage` enum").
///
/// Steps 10, 11, 12, 14 and 16 are internal to `mandate_executor::handle` and deliberately have no
/// variant: the binding gate, the idempotency key and the journal-then-broker ordering are the
/// executor's own, and a trait to stub them one at a time would put the executor's decisions behind
/// something a caller can substitute (`AGENTS.md` rule 1, review finding 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Stage {
    /// Step 17's first half, run before anything else: `mandate_risk::agent_flatten` can plan.
    FlattenProbe,
    /// Step 17's second half: the executor's protective sequence (E7-4) can answer.
    ProtectionProbe,
    /// Step 1: the mandate validates with no violation.
    Validate,
    /// The market data a run decides on: an observation whose stored bytes are the run's, read
    /// back and re-hashed from the run's own store before the runtime is handed anything (E15-13,
    /// the brief's slice H3), and the trusted daily closes and liquidity facts the paper adapter
    /// reads. Unlike the rest, a refusal here can arrive after the startup's own records, because
    /// the streams are opened first; nothing about the cycle is ever decided.
    MarketData,
    /// Step 16, run at every start: the broker's truth is reconciled before anything opens.
    Reconcile,
    /// Step 5: the order builder's sizing.
    Size,
    /// Step 5: the autonomy classification.
    Classify,
    /// Step 6: the gate's advisory pass.
    GateDryRun,
    /// Step 8: the append of the agent and account streams.
    Journal,
    /// Step 9: the handoff from the runtime to the executor.
    Sink,
    /// Steps 10 to 12 and 14, inside `mandate_executor::handle`.
    Executor,
    /// Step 13: the paper connector.
    Connector,
}

impl Stage {
    /// Every stage, in path order. [`Stage::position`] is an exhaustive match, so a new variant does
    /// not compile until it has a position, and `every_stage_is_listed_once_in_path_order` fails
    /// until it is listed here.
    pub const ALL: [Self; 12] = [
        Self::FlattenProbe,
        Self::ProtectionProbe,
        Self::Validate,
        Self::MarketData,
        Self::Reconcile,
        Self::Size,
        Self::Classify,
        Self::GateDryRun,
        Self::Journal,
        Self::Sink,
        Self::Executor,
        Self::Connector,
    ];

    /// The stage's place on the path, which is its index in [`Stage::ALL`].
    pub fn position(self) -> usize {
        match self {
            Self::FlattenProbe => 0,
            Self::ProtectionProbe => 1,
            Self::Validate => 2,
            Self::MarketData => 3,
            Self::Reconcile => 4,
            Self::Size => 5,
            Self::Classify => 6,
            Self::GateDryRun => 7,
            Self::Journal => 8,
            Self::Sink => 9,
            Self::Executor => 10,
            Self::Connector => 11,
        }
    }

    /// The stable reason code the run reports when this stage does not answer (ADR-0001 ES-09). The
    /// process exits with it and prints it on one line of stderr. The connector's is
    /// `broker_request_not_sent`, a request that never left the process; one the broker answered
    /// and the connector could not read is `broker_answer_uninterpretable` instead, and one that
    /// left with no answer is `broker_outcome_unknown`, which [`crate::ShellError::code`] tells
    /// apart by the cause (#248 review, #288 review minor 3).
    pub fn code(self) -> &'static str {
        match self {
            Self::FlattenProbe => "exit_path_unavailable",
            Self::ProtectionProbe => "protection_unavailable",
            Self::Validate => "mandate_not_validated",
            Self::MarketData => "market_data_untrusted",
            Self::Reconcile => "reconciliation_unavailable",
            Self::Size => "no_proposal",
            Self::Classify => "autonomy_not_auto",
            Self::GateDryRun => "gate_refused",
            Self::Journal => "append_not_committed",
            Self::Sink => "intent_not_handed",
            Self::Executor => "executor_refused",
            Self::Connector => "broker_request_not_sent",
        }
    }

    /// Whether the stage can **reduce** risk. Such a stage that cannot answer stops the tracer
    /// before it opens anything, never with an empty plan (`AGENTS.md` rule 13, TI-4).
    pub fn reduces_risk(self) -> bool {
        match self {
            Self::FlattenProbe | Self::ProtectionProbe => true,
            Self::Validate
            | Self::MarketData
            | Self::Reconcile
            | Self::Size
            | Self::Classify
            | Self::GateDryRun
            | Self::Journal
            | Self::Sink
            | Self::Executor
            | Self::Connector => false,
        }
    }
}

impl fmt::Display for Stage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

/// What the mandate stage admits: the runtime's view of a mandate that validated with no
/// violation, its environment, and the registered policy set and model registry that govern the
/// run, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Admitted {
    pub view: MandateView,
    /// What the run resolves `view`'s approvers from again at each control-stream read
    /// (DEC-2384 item 6).
    pub approvers: ApproverSource,
    pub environment: Environment,
    /// The registered policy set and model registry that govern the run, or `None` when nothing
    /// does yet (DEC-484).
    pub governed: Option<GovernedRefs>,
}

/// The content hashes of the effective `policy_set` and `model_registry` registrations, which a
/// governed run's version-2 `ModelOutputRecorded` and `DecisionMade` reference (journal spec
/// v0.16, DEC-484 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GovernedRefs {
    pub policy_set: Digest,
    pub model_registry: Digest,
}

/// Step 17's flatten half, and the planner behind the runtime's infallible `FlattenPlanner`.
pub trait ExitPath {
    /// Whether an agent-scoped flatten can be planned at all. Asked once, before anything else.
    fn probe(&self) -> Result<(), Cause>;
    /// The agent-scoped plan for one request, over the journal as it stands at the call: the
    /// caller — the run's bridge, which owns the stages — hands the journal's read capability,
    /// the account stream's id, and the step's risk clock with each call, so the plan can never
    /// be taken over a fold the run has already written past or a session the clock has already
    /// left (DEC-449). A scratch run's memory journal is readable through the same hand. The
    /// clock is `None` before any has advanced, and a plan without one is refused — the session
    /// cannot be derived, and no default invents it.
    fn plan(
        &self,
        request: &FlattenRequest,
        journal: &dyn JournalWriter,
        stream: &str,
        clock: Option<RiskClock>,
    ) -> Result<FlattenPlan, Cause>;
}

/// Step 17's protection half: whether the executor's protective sequence can answer (E7-4).
pub trait Protection {
    fn probe(&self) -> Result<(), Cause>;
}

/// Step 1: the mandate, validated with no violation.
pub trait MandateSource {
    fn admitted(&self) -> Result<Admitted, Cause>;
}

/// Step 5's sizing. `Ok(None)` is the builder's own "hold", which is an answer, not a failure.
pub trait Sizing {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause>;
}

/// Step 5's autonomy classification.
pub trait Classifier {
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Result<Classified, Cause>;
}

/// Step 6: `mandate_risk::evaluate` on the proposal, as a dry run.
pub trait Gate {
    fn evaluate(&self, proposal: &Proposal) -> Result<Decision, Cause>;
}

/// Step 8: the append protocol of journal spec §5.1, one stream at a time.
pub trait JournalWriter {
    /// Takes the stream's writer epoch, fencing out any earlier writer.
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause>;
    /// Every committed event of the stream, in `seq` order.
    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause>;
    /// Appends canonical draft bytes at `expected_head` under `writer_epoch`.
    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome;
}

/// Step 9: the runtime's handoff, converted into the executor's. The two `IntentHandoff` types are
/// distinct: the executor's carries the `AgentId` the shell supplies (task brief step 9).
pub trait Sink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause>;
}

/// Steps 10 to 12 and 14: one `mandate_executor::handle` call, and the fold of what it journaled.
pub trait Executor {
    /// Starts one process-local fold from an empty state while retaining its trusted context.
    fn reset(&mut self) -> Result<(), Cause>;
    /// Hands the executor the connector's capability profile, which its protection reads, after
    /// the fold at every start (DEC-838 item 5). Required, so every executor states what it does
    /// with the profile.
    fn use_profile(&mut self, profile: CapabilityProfile) -> Result<(), Cause>;
    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause>;
    /// The watch's protection fact (LT-10, DEC-912 item 3): whether the filled quantity the
    /// executor holds in `instrument` is covered right now — nothing filled, or the profile's
    /// strongest protection resting over the whole of it (`mandate_executor::is_protected`,
    /// the executor's public protection probe). The watch asks this at every wake beside its
    /// broker read; an answer the executor cannot give is a stop after which no further read
    /// happens, never a guess at covered (`AGENTS.md` rule 3).
    fn protected(&mut self, instrument: &InstrumentId) -> Result<bool, Cause>;
    /// Folds one committed account-stream event back, which is how a single writer learns that its
    /// own append committed (journal spec §5.1).
    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause>;
    /// The orders the folded stream owns whose fills it may not have booked
    /// (`mandate_executor::owed_orders`, DEC-955 item 2), and the activity cursor its last
    /// reconciliation recorded, from which their fills are read. An executor without the
    /// own-fill reconciliation answers that it has none, so the run stops rather than skip it.
    fn owed(
        &self,
    ) -> Result<
        (
            Vec<mandate_executor::ClientOrderId>,
            mandate_executor::ActivityCursor,
        ),
        Cause,
    > {
        Err(Cause::Absent {
            what: "the own-fill reconciliation",
        })
    }
    /// The fill records the broker's word calls for (`mandate_executor::book_own_fills`,
    /// DEC-955 items 2 to 4), drafted at `epoch` and `at`; journal records only.
    fn book_own(
        &mut self,
        epoch: mandate_executor::WriterEpoch,
        at: mandate_executor::RiskClock,
        orders: &[mandate_executor::BrokerOrder],
        fills: &[mandate_executor::BrokerFill],
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let _ = (epoch, at, orders, fills);
        Err(Cause::Absent {
            what: "the own-fill reconciliation",
        })
    }
}

/// Step 13: one broker round trip. An `Err` is never a rejection (`BrokerConnector`'s contract).
pub trait Connector {
    /// The broker's capability profile, from its published contract alone
    /// (`BrokerConnector::profile`, DEC-531 item 1). The default is Alpaca's, which every
    /// connector the shell holds is until B3 (DEC-838 item 5).
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        mandate_alpaca::alpaca_profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError>;
}

/// Step 16: the startup reconciliation.
pub trait Reconciler {
    /// Reads one typed broker snapshot through the run's connector.
    fn snapshot(
        &mut self,
        connector: &mut dyn Connector,
        requests: &[BrokerRequest],
    ) -> Result<mandate_executor::BrokerSnapshot, Cause>;

    /// Reconciles that gathered snapshot after its account report has committed and folded.
    fn reconcile(
        &mut self,
        snapshot: &mandate_executor::BrokerSnapshot,
    ) -> Result<mandate_executor::Reconciliation, Cause>;
}

/// Every stage the tracer reaches, one implementation each.
pub struct Stages {
    pub exit: Box<dyn ExitPath>,
    pub protection: Box<dyn Protection>,
    pub mandate: Box<dyn MandateSource>,
    pub reconciler: Box<dyn Reconciler>,
    pub sizing: Box<dyn Sizing>,
    pub classifier: Box<dyn Classifier>,
    pub gate: Box<dyn Gate>,
    pub journal: Box<dyn JournalWriter>,
    pub sink: Box<dyn Sink>,
    pub executor: Box<dyn Executor>,
    pub connector: Box<dyn Connector>,
    /// The content-addressed store the run's artifacts were put in, the one the journal appends
    /// against. An observation's data is checked here before the runtime is handed the
    /// observation, so a missing or altered artifact never puts a runtime batch in doubt (E15-13,
    /// the brief's slice H3; `AGENTS.md` rule 13).
    pub artifacts: Option<Arc<dyn ArtifactSource + Send + Sync>>,
}

#[cfg(test)]
mod doubles;
#[cfg(test)]
mod fail_closed;
#[cfg(test)]
mod grant_install;
#[cfg(test)]
mod lt10_install;
#[cfg(test)]
mod observed;
#[cfg(test)]
mod own_fills_install;
#[cfg(test)]
mod tail_install;
