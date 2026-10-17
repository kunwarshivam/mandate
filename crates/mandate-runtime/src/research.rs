//! The research loop's half in the core ([agent harness spec](../../../docs/specs/agent-harness.md)
//! §5.3, §5.4, §6.2, §6.5, §7; backlog E19-3, E19-4, and E17-2's harness half; DEC-996): when a run
//! starts, when it is cancelled, what is journaled before the model reads anything, and how a model's
//! answer is checked, filled with platform facts, and judged by `mandate_research`'s ordered checks.
//!
//! # One writer, two tasks
//!
//! The research worker (`mandate-research-run`) runs on its own task and reaches the core only as
//! [`ResearchInput`]s, through [`handle_research`]; the core answers with journal drafts and
//! [`ResearchRequest`]s the shell hands back to the worker. The core stays the agent stream's only
//! writer (HI-16), and nothing the trading loop decides waits on research (HI-7): [`crate::handle`]
//! never reads a [`ResearchState`].
//!
//! # What a run may not do
//!
//! A run starts only when spec §6.2's preconditions hold ([`Preconditions`]), the effective mode is
//! `normal` (HI-15), the research agent is configured, enabled, and pinned, and the interval since the
//! later of the last call and the last reservation has passed (§5.3, HI-8). A run is cancelled the
//! moment any of those stops holding, or its wall-clock bound passes (§7.1). Its one call is planned
//! only after every item it will show the model is drafted as `ObservationRecorded` (HI-2). Every
//! failure ends the run with no output (HI-6). A candidate reaches the order path only as an admitted
//! thesis's `ModelOutputRecorded`, which the existing order builder, the gate's dry run, and the
//! autonomy rules then judge like any model output (HI-1, HI-10).
//!
//! # Records that do not exist yet
//!
//! `ModelInvocationRecorded` on the agent stream is not closed (E15-8), and neither the candidates'
//! verdict record (E15-8) nor the invalidation verdict (E19-5) is in the journal spec. The drafts
//! here are DEC-996's provisional shapes, and [`Preconditions`] keeps every run from starting until
//! the journal registers both records, so no provisional draft reaches a production journal.

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_research::drift::DriftState;
use mandate_research::{
    AdmissionFacts, LineageState, PolicyOverlay, ValidatedMandate, WorkingUniverse,
};
use mandate_research_run::{
    CallOutcome, CallPlan, InstrumentFact, ItemId, ItemKind, ModelPin, OutputBounds, RetrievalPlan,
    RetrievedItem, SourceId, Symbol,
};
use mandate_time::UtcNanos;

use crate::error::RuntimeError;
use crate::ports::Ports;
use crate::state::RuntimeState;
use crate::types::{Effect, FoldedEvent, Observation, RiskClock};

/// The research entry's pinned configuration (§1.3, inference spec §4.1): the identity, the call's
/// deadline and token bound, the retrieval plan, the output bounds, and the owner's description.
/// The interval is not here: it is an envelope field, read from the confirmed mandate (§5.3, rule 11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchConfig {
    pub pin: ModelPin,
    pub deadline_ms: u32,
    pub max_output_tokens: u32,
    pub plan: RetrievalPlan,
    pub bounds: OutputBounds,
    /// The owner's `behavior.description`, already passed by the prompt guard (§5.1 step 5).
    pub description: String,
    /// Whether §8.6 item 5 allows revisions. `false` in v0.1, so every revision is refused
    /// `revisions_not_enabled` (§6.5).
    pub revisions_enabled: bool,
}

/// A record spec §6.2's preconditions need the journal to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ResearchRecord {
    /// The agent-stream invalidation verdict a removal can name as its cause (E19-5).
    InvalidationVerdict,
    /// The candidates' verdicts, as a member of the closed `ModelInvocationRecorded` or its own
    /// event (E15-8).
    CandidateVerdicts,
}

/// What the journal registers. The shell passes [`JournalCatalog`]; a test passes a double.
pub trait RecordCatalog {
    fn registers(&self, record: ResearchRecord) -> bool;
}

/// `mandate-journal`'s catalogue as of this change: it registers neither record, so no research
/// run starts in any deployment until E19-5 and E15-8 register them and change this answer.
pub struct JournalCatalog;

impl RecordCatalog for JournalCatalog {
    fn registers(&self, _record: ResearchRecord) -> bool {
        false
    }
}

/// Spec §6.2's first two preconditions, read from a catalogue. The third (no licensed text) is
/// held by the worker's retrieval, which drops every licensed item. The fields are private, so the
/// only way to a [`Preconditions`] that holds is a catalogue that registers both records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preconditions {
    invalidation_verdict: bool,
    candidate_verdicts: bool,
}

impl Preconditions {
    pub fn read(catalog: &dyn RecordCatalog) -> Self {
        Self {
            invalidation_verdict: catalog.registers(ResearchRecord::InvalidationVerdict),
            candidate_verdicts: catalog.registers(ResearchRecord::CandidateVerdicts),
        }
    }

    pub fn hold(&self) -> bool {
        false
    }
}

/// One of the agent's active theses, from the folded thesis records and the account stream's
/// working universe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveThesis {
    pub thesis_id: String,
    pub lineage_id: String,
    pub revision: u32,
    pub instrument: mandate_research::AssetId,
    pub symbol: Symbol,
    pub expires_at: RiskClock,
}

/// What a research step reads besides the runtime's own state. Every member is a fold of the
/// journal or a pinned configuration object, supplied by the shell; none is model output.
#[derive(Clone, Copy)]
pub struct ResearchContext<'a> {
    /// `None` when the version in effect has no research agent, or construction disabled research.
    pub config: Option<&'a ResearchConfig>,
    pub preconditions: &'a Preconditions,
    pub mandate: &'a ValidatedMandate,
    pub overlay: &'a PolicyOverlay,
    pub universe: &'a WorkingUniverse,
    pub facts: &'a AdmissionFacts,
    pub lineages: &'a LineageState,
    pub active_theses: &'a [ActiveThesis],
    /// The latest reservation the gateway's meter stream holds for this agent (inference spec §3.6).
    pub last_reservation: Option<RiskClock>,
}

/// What the core keeps of one retrieved item: its identity, source, kind, instrument, and the
/// facts the drift detector folds. Never its text (DEC-266 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFacts {
    pub item_id: ItemId,
    pub source: SourceId,
    pub kind: ItemKind,
    pub symbol: Option<Symbol>,
    pub instrument: Option<InstrumentFact>,
    pub knowledge_at: UtcNanos,
    pub content: Digest,
    pub length_bytes: u64,
}

/// One retrieved item as the core receives it: the observation to journal, whose artifact the
/// shell has stored, and the item's facts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RetrievedObservation {
    pub observation: Observation,
    pub item: ItemFacts,
}

impl RetrievedObservation {
    /// The observation and facts of `item` for a run cut off at `cut_off`, with the record bytes
    /// the shell stores as the artifact the observation's `data_ref` names.
    ///
    /// # Errors
    /// [`RuntimeError::NotInterpreted`] when the record cannot be built.
    pub fn capture(
        _item: &RetrievedItem,
        _cut_off: RiskClock,
    ) -> Result<(Self, Vec<u8>), RuntimeError> {
        Err(RuntimeError::Unimplemented { story: "E19-2" })
    }
}

/// What the research worker hands the core (spec §10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchInput {
    /// The scheduler's tick as the research loop sees it: start a due run, cancel one that may no
    /// longer run, or judge one whose call record has committed.
    Poll(RiskClock),
    /// A run's retrieval, every artifact already stored.
    Retrieved {
        run_id: String,
        items: Vec<RetrievedObservation>,
    },
    /// The gateway's settled answer, at the risk clock it arrived.
    Called {
        run_id: String,
        at: RiskClock,
        outcome: CallOutcome,
    },
    /// The worker's acknowledgment of a cancellation: `dispatched` says whether a call had left,
    /// whose late answer is still journaled (§5.3).
    Cancelled { run_id: String, dispatched: bool },
}

/// What the core asks the worker to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResearchRequest {
    /// Run the retrieval plan at `cut_off`.
    Start { run_id: String, cut_off: RiskClock },
    /// Send the one call. Every observation it names is drafted ahead of it in the same step.
    Call(CallPlan),
    /// Stop the run; a call already sent is never used.
    Cancel { run_id: String },
}

/// One research step's answer: the effects to run in order, then the requests for the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchStep {
    pub effects: Vec<Effect>,
    pub requests: Vec<ResearchRequest>,
}

/// The research loop's own state. It is process-local by design: a run in flight is lost on a
/// restart (§5.4), and what must survive one (the last call's instant) is folded back from the
/// journal by [`ResearchState::fold`].
#[derive(Debug, Clone)]
pub struct ResearchState {
    last_call: Option<RiskClock>,
    starts: u32,
    deadline_streak: u32,
    withdrawn: BTreeSet<ModelPin>,
    cancelled: BTreeSet<String>,
    observed: BTreeSet<ItemId>,
    drift: DriftState,
}

impl ResearchState {
    pub fn new() -> Self {
        Self {
            last_call: None,
            starts: 0,
            deadline_streak: 0,
            withdrawn: BTreeSet::new(),
            cancelled: BTreeSet::new(),
            observed: BTreeSet::new(),
            drift: DriftState::new(),
        }
    }

    /// Folds one journaled event: the instant of every research call, and whether the call record
    /// the in-flight run waits on has committed.
    ///
    /// # Errors
    /// [`RuntimeError::NonCanonicalPayload`] for a research call record it cannot read.
    pub fn fold(&mut self, _event: &FoldedEvent) -> Result<(), RuntimeError> {
        Err(RuntimeError::Unimplemented { story: "E19-3" })
    }

    /// The run in flight, if any.
    pub fn in_flight(&self) -> Option<&str> {
        None
    }

    /// The instant the schedule counts from: the later of the last call and the last reservation.
    pub fn last_call(&self) -> Option<RiskClock> {
        self.last_call
    }

    /// E17-5's drift fold over every news and filing item observed, each once.
    pub fn drift(&self) -> &DriftState {
        &self.drift
    }
}

/// The research step. Like [`crate::handle`] it is the only producer of research effects, it
/// emits every draft before the request it authorises, and a batch whose append is unresolved
/// blocks every other input until the same input re-derives the same drafts (journal spec §5.1).
///
/// # Errors
/// [`RuntimeError`] when the runtime has not started, an append is unresolved, or a value cannot be
/// read or written.
pub fn handle_research(
    _state: &mut RuntimeState,
    _research: &mut ResearchState,
    _input: ResearchInput,
    _ports: &Ports<'_>,
    _context: &ResearchContext<'_>,
) -> Result<ResearchStep, RuntimeError> {
    Err(RuntimeError::Unimplemented { story: "E19-3" })
}
