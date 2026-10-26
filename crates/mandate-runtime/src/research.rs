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
//! # The call record
//!
//! The worker calls through `mandate-gateway` (DEC-1120), and the core journals the gateway's own
//! [`CallRecord`] as journal spec §9.19's agent-stream `ModelInvocationRecorded`, whatever its
//! outcome. An `ok` record is judged in the step it arrives, and its verdicts go in the record,
//! which heads the judging batch (rule 151, INF-10). `mandate-journal` does not register that
//! record or the invalidation verdict (E19-5) yet, so [`Preconditions`] keeps every run from
//! starting until it does.

mod agentic;
mod judge;

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;
use mandate_canon::{Digest, Value};
use mandate_gateway::money::Usd6;
use mandate_gateway::{CallRecord, Outcome, Purpose, RequestError, Verdict};
use mandate_research::drift::{DriftState, InputClass, InputObservation};
use mandate_research::{
    AdmissionFacts, ContentHash, LineageState, PolicyOverlay, SourceId as ResearchSource,
    ValidatedMandate, WorkingUniverse, next_proposal_at,
};
use mandate_research_run::agentic::{
    Code, EndReason, LoopBounds, OutputMode, RunKind, Tool, ToolCall, ToolCallId, ToolOutcome,
    TurnPlan,
};
use mandate_research_run::{
    CallPlan, InstrumentFact, ItemId, ItemKind, ModelPin, OutputBounds, RetrievalPlan,
    RetrievedItem, SourceId, Symbol, treatment,
};
use mandate_time::UtcNanos;

use crate::error::RuntimeError;
use crate::payload;
use crate::ports::Ports;
use crate::state::{RuntimeState, UnresolvedAppend};
use crate::step::{Batch, observed};
use crate::types::{
    Effect, EventId, FoldedEvent, Input, Mode, NotificationRef, Observation, RiskClock,
    RuntimeAlert,
};
use judge::Judged;

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
    /// An agentic entry's loop (§6.9, DEC-2206 item 1); `None` for a `single_call` entry.
    pub agentic: Option<AgenticConfig>,
}

/// An agentic entry's pinned loop and the workspace's run ceiling (§6.9 to §6.11, DEC-2206 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgenticConfig {
    pub output: OutputMode,
    pub bounds: LoopBounds,
    /// The tools an `ideate` run may call.
    pub ideate_menu: BTreeSet<Tool>,
    /// The tools a `monitor` run may call: never one that adds risk (§6.9).
    pub monitor_menu: BTreeSet<Tool>,
    /// The workspace's `research_run_cost_cap_usd` (§6.11).
    pub cost_ceiling: Usd6,
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

/// Which closed agent-stream records register the research records, read through a catalogue's
/// `closes`: the candidates' verdicts are registered once the agent stream's
/// `ModelInvocationRecorded` is closed with its `verdicts` (journal spec §9.19, E15-8), and the
/// invalidation verdict once `ThesisInvalidated` is (§9.22, E19-5). The shell passes
/// [`JournalCatalog::JOURNAL`], `mandate-journal`'s own catalogue.
#[derive(Clone, Copy)]
pub struct JournalCatalog {
    closes: fn(&str, u64, mandate_journal::StreamType) -> bool,
}

impl JournalCatalog {
    /// `mandate-journal`'s own catalogue.
    pub const JOURNAL: Self = Self::over(mandate_journal::closes);

    /// The research records as registered by a catalogue whose `closes` answers as `closes` does.
    pub const fn over(closes: fn(&str, u64, mandate_journal::StreamType) -> bool) -> Self {
        Self { closes }
    }
}

impl RecordCatalog for JournalCatalog {
    fn registers(&self, record: ResearchRecord) -> bool {
        let (event_type, version) = match record {
            ResearchRecord::CandidateVerdicts => ("ModelInvocationRecorded", 1),
            ResearchRecord::InvalidationVerdict => ("ThesisInvalidated", 1),
        };
        (self.closes)(event_type, version, mandate_journal::StreamType::Agent)
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
        self.invalidation_verdict && self.candidate_verdicts
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
    /// The data universe's symbols, from the same fold as `facts.data_universe`: an agentic run's
    /// read scope (§6.9 step 1).
    pub data_symbols: &'a BTreeSet<Symbol>,
    /// The version in effect's `behavior.research.event_triggers` (mandate spec §8.4, DEC-2392);
    /// `None` when the version sets none, so no item brings a run forward.
    pub event_triggers: Option<&'a EventTriggers>,
}

/// `behavior.research.event_triggers` as the core reads it (mandate spec §8.4, agent harness spec
/// §5.3, DEC-2392): the item kinds that may bring a `monitor` run forward, the least time after the
/// last research call or event run start, and the most event runs a risk day may start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventTriggers {
    pub kinds: BTreeSet<ItemKind>,
    pub debounce_s: u32,
    pub max_runs_per_day: u32,
}

/// What the core keeps of one retrieved item: its identity, source, kind, instruments, and the
/// facts the drift detector folds. Never its text (DEC-266 item 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemFacts {
    pub item_id: ItemId,
    pub source: SourceId,
    pub kind: ItemKind,
    pub symbols: BTreeSet<Symbol>,
    pub instrument: Option<InstrumentFact>,
    pub knowledge_at: UtcNanos,
    pub content: Option<Digest>,
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
        item: &RetrievedItem,
        cut_off: RiskClock,
    ) -> Result<(Self, Vec<u8>), RuntimeError> {
        let unreadable =
            |error: mandate_research_run::ResearchRunError| RuntimeError::NotInterpreted {
                what: format!("a retrieved item ({})", error.code()),
                story: "E19-2",
            };
        let record = item.record().map_err(unreadable)?;
        let source = item.source().map_err(unreadable)?;
        let symbols: BTreeSet<Symbol> = match item {
            RetrievedItem::News(news) => news.symbols.clone(),
            other => other.symbol().cloned().into_iter().collect(),
        };
        let instrument_id = item
            .symbol()
            .map(|symbol| {
                InstrumentId::new(symbol.as_str())
                    .map_err(|_| payload::non_canonical("instrument_id"))
            })
            .transpose()?;
        let observation = Observation {
            source: source.as_str().to_owned(),
            instrument_id,
            as_of: cut_off,
            data_ref: Digest::of(&record),
        };
        let facts = ItemFacts {
            item_id: item.item_id().map_err(unreadable)?,
            source,
            kind: item.kind(),
            symbols,
            instrument: match item {
                RetrievedItem::Instrument(fact) => Some(fact.clone()),
                _ => None,
            },
            knowledge_at: item.knowledge_at(),
            content: item.content_hash(),
            length_bytes: item.length_bytes(),
        };
        Ok((
            Self {
                observation,
                item: facts,
            },
            record,
        ))
    }
}

/// What the research worker hands the core (spec §10.3).
#[allow(
    clippy::large_enum_variant,
    reason = "one input per research step, never stored in bulk; boxing the answer buys nothing"
)]
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
    /// The gateway's record of the run's one call, whatever its outcome, at the risk clock it
    /// arrived. The shell has stored the prompt and response artifacts it names.
    Called {
        run_id: String,
        at: RiskClock,
        record: Box<CallRecord>,
    },
    /// A call the gateway could not make at all, which has no record (inference spec §3.1).
    Refused {
        run_id: String,
        at: RiskClock,
        error: RequestError,
    },
    /// The worker's acknowledgment of a cancellation: `dispatched` says whether a call had left,
    /// whose late answer is still journaled (§5.3).
    Cancelled { run_id: String, dispatched: bool },
    /// An agentic run's dispatched reads and thinks, in reply order, each result's content
    /// artifact already stored (§6.9 step 5, DEC-2206 item 6).
    Dispatched {
        run_id: String,
        at: RiskClock,
        results: Vec<DispatchedResult>,
    },
    /// A news item or filing the data service stored, offered as an event trigger at `at` (agent
    /// harness spec §5.3, DEC-2392): its artifact is already stored, and the core alone decides
    /// whether it brings a `monitor` run forward.
    Event {
        at: RiskClock,
        item: RetrievedObservation,
    },
}

/// One dispatched read's or think's result, as the worker ran it (§6.9 step 5, §6.10).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchedResult {
    pub call: ToolCallId,
    pub outcome: ToolOutcome,
    pub code: Option<Code>,
    /// The result as the model is given it, stored as an artifact (`content_ref`).
    pub content: Digest,
    pub bytes: u32,
    pub truncated: bool,
    pub untrusted: bool,
    /// Every item the result holds, the run's earlier ones included, each with its observation.
    pub items: Vec<RetrievedObservation>,
}

/// An agentic turn's call (§6.9 step 3, DEC-2206 item 5): the call, the loop's plan for it, and the
/// transcript as artifact references the worker loads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRequest {
    pub call: CallPlan,
    /// The run's kind, fixed at its start (§6.9 step 1), which the turn's call names (DEC-2256).
    pub kind: RunKind,
    pub plan: TurnPlan,
    pub transcript: Vec<TurnRef>,
}

/// One earlier turn of the transcript: its reply's response artifact and each tool result's
/// content artifact, by the call's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnRef {
    pub response: Digest,
    pub results: Vec<Digest>,
}

/// A reply's read and think calls for the worker, in reply order (§6.9 step 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchRequest {
    pub run_id: String,
    pub turn: u32,
    pub calls: Vec<ToolCall>,
    /// The run's result bytes left, which the worker lowers read by read.
    pub result_bytes_left: u32,
    /// The run's frozen read scope (§6.9 step 1).
    pub cut_off: RiskClock,
    pub allowlist_version: u32,
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
    /// Send an agentic run's next turn.
    Turn(Box<TurnRequest>),
    /// Run a reply's reads and thinks.
    Dispatch(DispatchRequest),
}

/// One research step's answer: the effects to run in order, then the requests for the worker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResearchStep {
    pub effects: Vec<Effect>,
    pub requests: Vec<ResearchRequest>,
    /// The bytes of every artifact a draft of the step names that the core made itself, which the
    /// shell stores before it appends the batch (DEC-2206 item 4).
    pub artifacts: Vec<Vec<u8>>,
    /// The run every draft of the step belongs to, which the shell writes as each draft's
    /// `correlation_id` (journal spec §9.23 rule 171, DEC-1120 item 9, DEC-2206 item 8); `None` for a
    /// step that journals nothing of a run.
    pub correlation_id: Option<String>,
}

/// Where a run in flight is (§5.3's states between `Due` and `Idle`).
#[allow(
    clippy::large_enum_variant,
    reason = "at most one run is in flight per agent (HI-8), so one value of this enum exists"
)]
#[derive(Debug, Clone)]
enum Phase {
    Retrieving,
    Calling {
        observations: Vec<EventId>,
        items: Vec<ItemFacts>,
    },
    /// An agentic run, from its start to its end (§6.9).
    Agentic(Box<agentic::Agentic>),
}

#[derive(Debug, Clone)]
struct Run {
    run_id: String,
    call_id: String,
    pin: ModelPin,
    cut_off: RiskClock,
    /// The last risk-clock second the run may still be in flight: its start, plus the retrieval
    /// bound, plus the call deadline rounded up to the second (§7.1).
    bound: RiskClock,
    phase: Phase,
}

/// A cancelled run's call, which may still answer: the answer is journaled and never used (§5.3).
#[derive(Debug, Clone)]
struct Cancelled {
    call_id: String,
    pin: ModelPin,
    observations: Vec<EventId>,
}

/// The research loop's own state. It is process-local by design: a run in flight is lost on a
/// restart (§5.4), and what must survive one (the last call's instant) is folded back from the
/// journal by [`ResearchState::fold`].
#[derive(Debug, Clone)]
pub struct ResearchState {
    run: Option<Run>,
    last_call: Option<RiskClock>,
    /// The cut-off of the last run that ended with nothing to show the model, which the schedule
    /// counts from as it does a call, so a data plane that answers nothing is not re-read on
    /// every poll (HI-8). Process-local: a restart may read once more, never more often.
    last_attempt: Option<RiskClock>,
    starts: u32,
    deadline_streak: u32,
    withdrawn: BTreeSet<ModelPin>,
    /// Every cancelled run the worker has not yet acknowledged, with its call if one was planned.
    cancelled: BTreeMap<String, Option<Cancelled>>,
    observed: BTreeSet<ItemId>,
    drift: DriftState,
    /// The step whose drafts are not yet known committed, re-emitted whole if the same input is
    /// handed again (journal spec §5.1).
    doubted: Option<(ResearchInput, ResearchStep)>,
    /// The cut-off of every event-triggered run's `RunStarted`, from the journal: the debounce and
    /// the per-day cap count from them (agent harness spec §5.3, DEC-2392).
    event_starts: Vec<RiskClock>,
    /// The items, by `data_ref`, an event run's cause has held: one item starts one run (journal
    /// spec rule 271).
    triggered: BTreeSet<Digest>,
    /// The `ObservationRecorded` records folded since the last `RunStarted`, by `event_id`, with
    /// their `data_ref`: an event run's cause is among them, since it is journaled in the run's own
    /// start batch (journal spec rule 271), so every `RunStarted` clears them and the map holds no
    /// more than one run's observations.
    observations: BTreeMap<EventId, Digest>,
}

impl Default for ResearchState {
    fn default() -> Self {
        Self::new()
    }
}

impl ResearchState {
    pub fn new() -> Self {
        Self {
            run: None,
            last_call: None,
            last_attempt: None,
            starts: 0,
            deadline_streak: 0,
            withdrawn: BTreeSet::new(),
            cancelled: BTreeMap::new(),
            observed: BTreeSet::new(),
            drift: DriftState::new(),
            doubted: None,
            event_starts: Vec::new(),
            triggered: BTreeSet::new(),
            observations: BTreeMap::new(),
        }
    }

    /// Folds one journaled event: the instant of every research call, and whether the call record
    /// the in-flight run waits on has committed.
    ///
    /// # Errors
    /// [`RuntimeError::NonCanonicalPayload`] for a research call record it cannot read.
    pub fn fold(&mut self, event: &FoldedEvent) -> Result<(), RuntimeError> {
        match event.event_type.as_str() {
            "ObservationRecorded" => {
                let data = payload::str_of(&event.payload, "data_ref")
                    .and_then(|text| text.strip_prefix("sha256:"))
                    .and_then(Digest::from_hex)
                    .ok_or_else(|| payload::non_canonical("data_ref"))?;
                self.observations.insert(event.event_id.clone(), data);
                return Ok(());
            }
            "RunStarted" => return self.started(event),
            _ => {}
        }
        if event.event_type != "ModelInvocationRecorded"
            || payload::str_of(&event.payload, "purpose") != Some("research")
        {
            return Ok(());
        }
        let completed = payload::str_of(&event.payload, "completed_at")
            .and_then(|text| UtcNanos::parse(text).ok())
            .ok_or_else(|| payload::non_canonical("completed_at"))?;
        let completed = RiskClock::from_secs(completed.secs());
        self.last_call = Some(self.last_call.map_or(completed, |last| last.max(completed)));
        Ok(())
    }

    /// An event-triggered `RunStarted`: its cut-off counts toward the debounce and the day's cap,
    /// and the item its cause holds can start no other run (agent harness spec §5.3, rule 271).
    fn started(&mut self, event: &FoldedEvent) -> Result<(), RuntimeError> {
        let observations = std::mem::take(&mut self.observations);
        let trigger = event.payload.get("trigger");
        if trigger.and_then(|t| payload::str_of(t, "kind")) != Some("event") {
            return Ok(());
        }
        let cut_off = payload::str_of(&event.payload, "cut_off")
            .and_then(|text| UtcNanos::parse(text).ok())
            .ok_or_else(|| payload::non_canonical("cut_off"))?;
        self.event_starts.push(RiskClock::from_secs(cut_off.secs()));
        let data = trigger
            .and_then(|t| payload::str_of(t, "cause"))
            .and_then(|cause| observations.get(&EventId(cause.to_owned())))
            .ok_or_else(|| payload::non_canonical("trigger.cause"))?;
        self.triggered.insert(*data);
        Ok(())
    }

    /// The run in flight, if any.
    pub fn in_flight(&self) -> Option<&str> {
        self.run.as_ref().map(|run| run.run_id.as_str())
    }

    /// The instant of the last research call the journal holds.
    pub fn last_call(&self) -> Option<RiskClock> {
        self.last_call
    }

    /// E17-5's drift fold over every news and filing item observed, each once.
    pub fn drift(&self) -> &DriftState {
        &self.drift
    }
}

/// A research batch the journal refused, which stored nothing: the runtime forgets its doubt of
/// it, so trading inputs, the kill switch among them, are no longer held behind a batch that can
/// never commit (`AGENTS.md` rules 3 and 13), and the run in flight is dropped. Returns whether a
/// research batch was in doubt. A trading batch in doubt is never touched: only a research input's
/// batch is forgotten, and the caller stops sending research inputs (DEC-2204).
pub fn abandon_refused_research(state: &mut RuntimeState, research: &mut ResearchState) -> bool {
    let research_batch = state
        .unresolved_batch()
        .is_some_and(|doubted| matches!(doubted.input, Input::Research(_)));
    if research_batch {
        state.forget_refused();
        research.doubted = None;
        research.run = None;
    }
    research_batch
}

/// The research step. Like [`crate::handle`] it is the only producer of research effects, it
/// emits every draft before the request it authorises, and a batch whose append is unresolved
/// blocks every other input until the same input re-derives the same drafts (journal spec §5.1).
///
/// # Errors
/// [`RuntimeError`] when the runtime has not started, an append is unresolved, or a value cannot be
/// read or written.
pub fn handle_research(
    state: &mut RuntimeState,
    research: &mut ResearchState,
    input: ResearchInput,
    ports: &Ports<'_>,
    context: &ResearchContext<'_>,
) -> Result<ResearchStep, RuntimeError> {
    if !state.has_started() {
        return Err(RuntimeError::NotStarted);
    }
    if let Some(doubted) = state.unresolved_batch() {
        let same = Input::Research(input.clone()) == doubted.input;
        return match research.doubted.as_ref() {
            Some((held, step)) if same && *held == input => Ok(step.clone()),
            _ => Err(RuntimeError::AppendUnresolved {
                head: doubted.head.0,
            }),
        };
    }
    research.doubted = None;
    let epoch = state.epoch().ok_or(RuntimeError::NotStarted)?;
    let mut batch = Batch::new(ports.ids, epoch, state.agent_head());
    let mut requests = Vec::new();
    let mut step = Step {
        batch: &mut batch,
        requests: &mut requests,
        artifacts: Vec::new(),
        correlation_id: None,
    };
    match &input {
        ResearchInput::Poll(now) => {
            not_behind(state, *now)?;
            polled(state, research, *now, context, &mut step)?;
        }
        ResearchInput::Retrieved { run_id, items } => {
            retrieved(state, research, run_id, items, context, &mut step)?;
        }
        ResearchInput::Called { run_id, at, record } => {
            not_behind(state, *at)?;
            called(state, research, run_id, *at, record, context, &mut step)?;
        }
        ResearchInput::Refused { run_id, at, error } => {
            not_behind(state, *at)?;
            refused(research, run_id, *error, &mut step)?;
        }
        ResearchInput::Cancelled { run_id, dispatched } => {
            acknowledged(research, run_id, *dispatched);
        }
        ResearchInput::Dispatched {
            run_id,
            at,
            results,
        } => {
            not_behind(state, *at)?;
            dispatched(state, research, run_id, *at, results, context, &mut step)?;
        }
        ResearchInput::Event { at, item } => {
            not_behind(state, *at)?;
            evented(state, research, *at, item, context, &mut step)?;
        }
    }
    let Step {
        artifacts,
        correlation_id,
        ..
    } = step;
    let correlation_id = correlation_id.or_else(|| {
        input_run(&input)
            .filter(|_| !batch.drafts.is_empty())
            .map(str::to_owned)
    });
    let step = ResearchStep {
        effects: batch.effects.clone(),
        requests,
        artifacts,
        correlation_id,
    };
    if !batch.drafts.is_empty() {
        state.remember(UnresolvedAppend {
            head: batch.head(),
            input: Input::Research(input.clone()),
            drafts: batch.drafts.clone(),
        });
        research.doubted = Some((input, step.clone()));
    }
    Ok(step)
}

/// The run a worker input names; `None` for a poll.
fn input_run(input: &ResearchInput) -> Option<&str> {
    match input {
        ResearchInput::Poll(_) | ResearchInput::Event { .. } => None,
        ResearchInput::Retrieved { run_id, .. }
        | ResearchInput::Called { run_id, .. }
        | ResearchInput::Refused { run_id, .. }
        | ResearchInput::Cancelled { run_id, .. }
        | ResearchInput::Dispatched { run_id, .. } => Some(run_id),
    }
}

/// What one step writes: its batch and its requests for the worker.
struct Step<'s, 'b> {
    batch: &'s mut Batch<'b>,
    requests: &'s mut Vec<ResearchRequest>,
    /// The artifacts the core made for this step's drafts (DEC-2206 item 4).
    artifacts: Vec<Vec<u8>>,
    /// The run this step's drafts belong to (DEC-2206 item 8).
    correlation_id: Option<String>,
}

/// The risk clock never moves back (mandate spec §5.2).
fn not_behind(state: &RuntimeState, at: RiskClock) -> Result<(), RuntimeError> {
    match state.risk_clock() {
        Some(last) if at < last => Err(RuntimeError::RiskClockWentBackwards {
            last: last.secs(),
            found: at.secs(),
        }),
        _ => Ok(()),
    }
}

/// The pin research may run under: the version in effect's, when the preconditions, the mode, the
/// envelope, the policy, and the working universe all let research run at all, whatever the
/// schedule says. `None` otherwise.
fn research_allowed(
    state: &RuntimeState,
    research: &ResearchState,
    context: &ResearchContext<'_>,
) -> Option<ModelPin> {
    let config = context.config?;
    let envelope = context.mandate.envelope();
    let mode = state.effective_mode();
    let enabled = context.preconditions.hold()
        && if config.agentic.is_some() {
            agentic::runs_in(mode)
        } else {
            mode == Mode::Normal
        }
        && !research.withdrawn.contains(&config.pin)
        && envelope.research.is_some()
        && envelope.admits_instruments
        && !envelope.universe_pinned
        && context.overlay.research_agent_allowed
        && matches!(context.universe, WorkingUniverse::Known { .. });
    enabled.then(|| config.pin.clone())
}

/// `next_proposal_at` over the later of the last call and the last reservation, at the effective
/// interval: the owner's `behavior.research.interval_s`, raised to the policy's floor (§5.3, §5.4).
fn due(
    research: &ResearchState,
    now: RiskClock,
    context: &ResearchContext<'_>,
) -> Result<bool, RuntimeError> {
    let Some(envelope) = context.mandate.envelope().research.as_ref() else {
        return Ok(false);
    };
    let interval = context
        .overlay
        .effective_research_interval_s(envelope.interval_s);
    let anchor = [
        research.last_call,
        research.last_attempt,
        context.last_reservation,
    ]
    .into_iter()
    .flatten()
    .max();
    let anchor = anchor
        .map(|at| UtcNanos::from_parts(at.secs(), 0))
        .transpose()?;
    let next = next_proposal_at(anchor, interval).map_err(|_| RuntimeError::NotInterpreted {
        what: "a proposal interval past the representable range".to_owned(),
        story: "E19-3",
    })?;
    Ok(next.is_none_or(|next| now.secs() >= next.secs()))
}

/// One poll: judge a run whose call record has committed, cancel one that may no longer run, or
/// start one that is due. At most one of the three per poll, so a cancelled run is never replaced
/// in the same step.
fn polled(
    state: &RuntimeState,
    research: &mut ResearchState,
    now: RiskClock,
    context: &ResearchContext<'_>,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    let allowed = research_allowed(state, research, context);
    if let Some(run) = research.run.take() {
        if allowed.as_ref() == Some(&run.pin) && now <= run.bound {
            research.run = Some(run);
        } else {
            let reason = if now > run.bound {
                EndReason::Deadline
            } else {
                EndReason::Mode
            };
            cancel(research, run, reason, step)?;
        }
        return Ok(());
    }
    let (Some(pin), Some(config)) = (allowed, context.config) else {
        return Ok(());
    };
    if !due(research, now, context)? {
        return Ok(());
    }
    if let Some(loop_config) = &config.agentic {
        let Some((kind, menu)) = agentic::due_kind(state.effective_mode(), loop_config, context)
        else {
            return Ok(());
        };
        let ordinal = u32::MAX.saturating_sub(research.starts.saturating_mul(2));
        let run_id = step.batch.id_at(ordinal);
        research.starts = research.starts.saturating_add(1);
        let (loop_, universe) =
            agentic::start(kind, menu, loop_config, now, context, step.batch, None)?;
        step.artifacts.push(universe);
        step.correlation_id = Some(run_id.0.clone());
        research.run = Some(Run {
            run_id: run_id.0.clone(),
            call_id: String::new(),
            pin,
            cut_off: now,
            bound: loop_.deadline,
            phase: Phase::Agentic(Box::new(loop_)),
        });
        step.requests.push(ResearchRequest::Start {
            run_id: run_id.0,
            cut_off: now,
        });
        return Ok(());
    }
    let ordinal = u32::MAX.saturating_sub(research.starts.saturating_mul(2));
    let run_id = step.batch.id_at(ordinal);
    let call_id = step.batch.id_at(ordinal.saturating_sub(1));
    research.starts = research.starts.saturating_add(1);
    let bound = now
        .secs()
        .checked_add(i64::from(config.plan.retrieval_timeout_s))
        .and_then(|secs| secs.checked_add(i64::from(config.deadline_ms.div_ceil(1_000))))
        .ok_or_else(|| payload::non_canonical("bound"))?;
    research.run = Some(Run {
        run_id: run_id.0.clone(),
        call_id: call_id.0,
        pin,
        cut_off: now,
        bound: RiskClock::from_secs(bound),
        phase: Phase::Retrieving,
    });
    step.requests.push(ResearchRequest::Start {
        run_id: run_id.0,
        cut_off: now,
    });
    Ok(())
}

/// An item offered as an event trigger (agent harness spec §5.3, mandate spec §8.4, DEC-2392).
/// It brings a `monitor` run forward only when all of these hold, judged from folded state alone:
/// no run is in flight; research may run in this mode at all, with an `agentic` entry; the version
/// in effect lists the item's kind; its source is on the allowlist in effect; its knowledge time is
/// after the last research call; one of its symbols carries an active thesis (the smallest symbol
/// is the one journaled when several do); the day's research spend is below the cost cap, as
/// admission check 7 reads it; the risk clock is at
/// least `debounce_s` after the later of the last call, the last reservation, and the last event
/// run's start; fewer than `max_runs_per_day` event runs have started on the risk day; and no event
/// run has held the item before. Otherwise nothing is journaled. When it starts, the run's batch
/// journals the item as an `ObservationRecorded` (the matched symbol, the item's knowledge time,
/// its stored record) and then `RunStarted` naming it as its cause (journal spec rule 271), so the
/// trigger is journaled before the run acts. The run is `monitor`, so it can only invalidate.
fn evented(
    state: &RuntimeState,
    research: &mut ResearchState,
    now: RiskClock,
    item: &RetrievedObservation,
    context: &ResearchContext<'_>,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    if research.run.is_some() {
        return Ok(());
    }
    let (Some(pin), Some(config), Some(triggers)) = (
        research_allowed(state, research, context),
        context.config,
        context.event_triggers,
    ) else {
        return Ok(());
    };
    let Some(loop_config) = &config.agentic else {
        return Ok(());
    };
    let facts = &item.item;
    let allowlisted = ResearchSource::new(facts.source.as_str())
        .is_ok_and(|source| context.facts.allowlist.sources.contains(&source));
    let newer = research
        .last_call
        .is_none_or(|last| facts.knowledge_at.secs() > last.secs());
    let Some(thesis) = context
        .active_theses
        .iter()
        .filter(|thesis| facts.symbols.contains(&thesis.symbol))
        .min_by(|a, b| a.symbol.cmp(&b.symbol))
    else {
        return Ok(());
    };
    let under_cost_cap = context
        .mandate
        .envelope()
        .research
        .as_ref()
        .is_some_and(|research| {
            context.facts.research_spend_usd_today
                < context
                    .overlay
                    .effective_cost_cap(research.cost_cap_usd_per_day)
        });
    let data = item.observation.data_ref;
    if !triggers.kinds.contains(&facts.kind)
        || !allowlisted
        || !newer
        || !under_cost_cap
        || research.triggered.contains(&data)
        || !debounced(research, now, context, triggers)
        || !under_daily_cap(research, now, triggers)
    {
        return Ok(());
    }
    let ordinal = u32::MAX.saturating_sub(research.starts.saturating_mul(2));
    let run_id = step.batch.id_at(ordinal);
    research.starts = research.starts.saturating_add(1);
    let trigger = Observation {
        source: item.observation.source.clone(),
        instrument_id: Some(
            InstrumentId::new(thesis.symbol.as_str())
                .map_err(|_| payload::non_canonical("instrument_id"))?,
        ),
        as_of: RiskClock::from_secs(facts.knowledge_at.secs()),
        data_ref: data,
    };
    let cause = step
        .batch
        .journal("ObservationRecorded", None, observed(&trigger)?)?;
    let (loop_, universe) = agentic::start(
        RunKind::Monitor,
        &loop_config.monitor_menu,
        loop_config,
        now,
        context,
        step.batch,
        Some(&cause),
    )?;
    step.artifacts.push(universe);
    step.correlation_id = Some(run_id.0.clone());
    research.run = Some(Run {
        run_id: run_id.0.clone(),
        call_id: String::new(),
        pin,
        cut_off: now,
        bound: loop_.deadline,
        phase: Phase::Agentic(Box::new(loop_)),
    });
    step.requests.push(ResearchRequest::Start {
        run_id: run_id.0,
        cut_off: now,
    });
    Ok(())
}

/// Whether the risk clock is at least `debounce_s` after the later of the last research call, the
/// last meter reservation, and the last event run's start (agent harness spec §5.3).
fn debounced(
    research: &ResearchState,
    now: RiskClock,
    context: &ResearchContext<'_>,
    triggers: &EventTriggers,
) -> bool {
    [
        research.last_call,
        context.last_reservation,
        research.event_starts.last().copied(),
    ]
    .into_iter()
    .flatten()
    .max()
    .is_none_or(|anchor| now.secs() >= anchor.secs().saturating_add(i64::from(triggers.debounce_s)))
}

/// Whether fewer than `max_runs_per_day` event runs have started on `now`'s risk day, the
/// America/New_York date (mandate spec §5.4). A clock the calendar cannot place, now or in a
/// start the journal holds, starts none (`AGENTS.md` rule 3).
fn under_daily_cap(research: &ResearchState, now: RiskClock, triggers: &EventTriggers) -> bool {
    let day = |at: RiskClock| {
        UtcNanos::from_parts(at.secs(), 0)
            .ok()
            .and_then(|at| mandate_time::new_york_date_and_hour(at).ok())
            .map(|(date, _)| date)
    };
    let Some(today) = day(now) else {
        return false;
    };
    let mut started = 0_u32;
    for at in &research.event_starts {
        match day(*at) {
            Some(date) if date == today => started = started.saturating_add(1),
            Some(_) => {}
            None => return false,
        }
    }
    started < triggers.max_runs_per_day
}

/// Cancels `run`: the worker is told to stop, an agentic run ends with `reason` in this step (the
/// core never waits for the worker's acknowledgment, §6.9), and the run is remembered until the
/// worker acknowledges it.
fn cancel(
    research: &mut ResearchState,
    mut run: Run,
    reason: EndReason,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    step.requests.push(ResearchRequest::Cancel {
        run_id: run.run_id.clone(),
    });
    if let Phase::Agentic(loop_) = &mut run.phase {
        agentic::end(loop_, reason, step.batch)?;
        step.correlation_id = Some(run.run_id.clone());
    }
    remember_cancelled(research, run);
    Ok(())
}

/// One step of an agentic run: `f` gets the run and its loop apart, then the run is put back, or
/// forgotten once it ended. The step's requests and artifacts are the run's, and so is its
/// correlation id. A refused step leaves the run as it was.
fn drive(
    research: &mut ResearchState,
    mut run: Run,
    step: &mut Step<'_, '_>,
    f: impl FnOnce(
        &mut Run,
        &mut agentic::Agentic,
        &mut Batch<'_>,
    ) -> Result<agentic::Out, RuntimeError>,
) -> Result<(), RuntimeError> {
    let run_id = run.run_id.clone();
    let mut phase = std::mem::replace(&mut run.phase, Phase::Retrieving);
    let out = match &mut phase {
        Phase::Agentic(loop_) => {
            let before = (**loop_).clone();
            let out = f(&mut run, loop_, step.batch);
            if out.is_err() {
                **loop_ = before;
            }
            out
        }
        _ => Ok(agentic::Out::default()),
    };
    run.phase = phase;
    let out = match out {
        Ok(out) => out,
        Err(error) => {
            research.run = Some(run);
            return Err(error);
        }
    };
    step.requests.extend(out.requests);
    step.artifacts.extend(out.artifacts);
    step.correlation_id = Some(run_id);
    if out.ended {
        research.last_attempt = Some(run.cut_off);
    } else {
        research.run = Some(run);
    }
    Ok(())
}

/// A retrieval's items are all at the run's cut-off, each item once.
fn well_formed(run: &Run, items: &[RetrievedObservation]) -> Result<(), RuntimeError> {
    let mut ids = BTreeSet::new();
    for item in items {
        if item.observation.as_of != run.cut_off || !ids.insert(&item.item.item_id) {
            return Err(RuntimeError::NotInterpreted {
                what: "a retrieval off its run's cut-off, or with an item twice".to_owned(),
                story: "E19-2",
            });
        }
    }
    Ok(())
}

/// A cancelled run is remembered until the worker acknowledges it, so its late inputs are
/// answered with nothing, and with its call if one was planned, so a late answer is journaled.
fn remember_cancelled(research: &mut ResearchState, run: Run) {
    let call = match run.phase {
        Phase::Calling { observations, .. } => Some(Cancelled {
            call_id: run.call_id,
            pin: run.pin,
            observations,
        }),
        Phase::Agentic(loop_) if loop_.turning() => Some(Cancelled {
            call_id: run.call_id,
            pin: run.pin,
            observations: loop_.observations,
        }),
        Phase::Retrieving | Phase::Agentic(_) => None,
    };
    research.cancelled.insert(run.run_id, call);
}

/// A run's retrieval: every item journaled as `ObservationRecorded`, then the one call requested
/// naming exactly those observations (§6.2 steps 2 and 3, HI-2). A run that may no longer run is
/// cancelled instead, and its items are never shown, so never journaled.
fn retrieved(
    state: &RuntimeState,
    research: &mut ResearchState,
    run_id: &str,
    items: &[RetrievedObservation],
    context: &ResearchContext<'_>,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    let Some(run) = research.run.take() else {
        return not_ours(research, run_id, "a retrieval");
    };
    if run.run_id != run_id || !matches!(run.phase, Phase::Retrieving | Phase::Agentic(_)) {
        research.run = Some(run);
        return not_ours(research, run_id, "a retrieval");
    }
    let config = match context.config {
        Some(config) if research_allowed(state, research, context).as_ref() == Some(&run.pin) => {
            config
        }
        _ => return cancel(research, run, EndReason::Mode, step),
    };
    if matches!(run.phase, Phase::Agentic(_)) {
        if let Err(error) = well_formed(&run, items) {
            research.run = Some(run);
            return Err(error);
        }
        return drive(research, run, step, |run, loop_, batch| {
            agentic::seeded(run, loop_, items, config, state, batch)
        });
    }
    if items.is_empty() {
        research.last_attempt = Some(run.cut_off);
        return Ok(());
    }
    let mut ids = BTreeSet::new();
    for item in items {
        if item.observation.as_of != run.cut_off || !ids.insert(&item.item.item_id) {
            research.run = Some(run);
            return Err(RuntimeError::NotInterpreted {
                what: "a retrieval off its run's cut-off, or with an item twice".to_owned(),
                story: "E19-2",
            });
        }
    }
    let mut observations = Vec::new();
    for item in items {
        observations.push(step.batch.journal(
            "ObservationRecorded",
            None,
            observed(&item.observation)?,
        )?);
    }
    fold_drift(research, items);
    let deployment = state.deployment();
    step.requests.push(ResearchRequest::Call(CallPlan {
        run_id: run.run_id.clone(),
        call_id: run.call_id.clone(),
        workspace_id: deployment.workspace.0.clone(),
        agent_id: deployment.agent.0.clone(),
        model: run.pin.clone(),
        as_of: UtcNanos::from_parts(run.cut_off.secs(), 0)?,
        deadline_ms: config.deadline_ms,
        max_output_tokens: config.max_output_tokens,
        observations: observations.iter().map(|id| id.0.clone()).collect(),
    }));
    research.run = Some(Run {
        phase: Phase::Calling {
            observations,
            items: items.iter().map(|item| item.item.clone()).collect(),
        },
        ..run
    });
    Ok(())
}

/// A worker input for a run this process is not running: one it cancelled is answered with
/// nothing, and any other is refused, because the core does not know what it means (DEC-85).
fn not_ours(research: &ResearchState, run_id: &str, what: &str) -> Result<(), RuntimeError> {
    if research.cancelled.contains_key(run_id) {
        return Ok(());
    }
    Err(RuntimeError::NotInterpreted {
        what: format!("{what} for a run not in flight"),
        story: "E19-3",
    })
}

/// E17-5's drift fold over the run's news and filing items, each item id once (§6.3). An item the
/// detector refuses as out of order for its source folds nothing and stays unobserved, which is
/// the detector's own answer to a late-arriving older item (DEC-266).
fn fold_drift(research: &mut ResearchState, items: &[RetrievedObservation]) {
    let mut text: Vec<&ItemFacts> = items
        .iter()
        .map(|item| &item.item)
        .filter(|item| matches!(item.kind, ItemKind::News | ItemKind::Filing))
        .collect();
    text.sort_by(|a, b| (a.knowledge_at, &a.item_id).cmp(&(b.knowledge_at, &b.item_id)));
    for item in text {
        if research.observed.contains(&item.item_id) {
            continue;
        }
        let (Ok(source), Some(content)) = (ResearchSource::new(item.source.as_str()), item.content)
        else {
            continue;
        };
        let observation = InputObservation {
            source,
            class: if item.kind == ItemKind::News {
                InputClass::News
            } else {
                InputClass::Filing
            },
            observed_at: item.knowledge_at,
            content: ContentHash::new(content),
            length_bytes: item.length_bytes,
        };
        if research.drift.observe(&observation).is_ok() {
            research.observed.insert(item.item_id.clone());
        }
    }
}

/// The gateway's record of a call: always journaled as `ModelInvocationRecorded`, whatever the
/// outcome and whether or not its run was cancelled (§5.3, §6.6). An `ok` record of the run in
/// flight, on time, under the pin research still runs, is judged in the same step, and its
/// verdicts go in the record, which heads the batch (§6.2 steps 4 and 5, journal spec §9.19). A
/// record of another call, pin, purpose, agent, or retrieved context is refused and never
/// journaled as this run's (INF-2, caller side). A judgment the core cannot write is no judgment:
/// the record is still journaled, with no verdicts, so a paid call is never left unrecorded and the
/// schedule still counts from it (HI-8, DEC-1120 item 7). A record completed after the run's own
/// wall-clock bound is never judged, whatever deadline it carries (§7.1).
fn called(
    state: &RuntimeState,
    research: &mut ResearchState,
    run_id: &str,
    at: RiskClock,
    record: &CallRecord,
    context: &ResearchContext<'_>,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    let agent = state.deployment().agent.0.as_str();
    let turn = research
        .run
        .take_if(|run| run.run_id == run_id && matches!(run.phase, Phase::Agentic(_)));
    if let Some(run) = turn {
        return turned(state, research, run, at, record, context, step);
    }
    let batch = &mut *step.batch;
    let in_flight = research
        .run
        .take_if(|run| run.run_id == run_id && matches!(run.phase, Phase::Calling { .. }));
    let Some(run) = in_flight else {
        let owned = research
            .cancelled
            .get(run_id)
            .and_then(Option::as_ref)
            .is_some_and(|call| owns(record, &call.call_id, &call.pin, &call.observations, agent));
        let cancelled = if owned {
            research.cancelled.remove(run_id).flatten()
        } else {
            None
        };
        let Some(cancelled) = cancelled else {
            return Err(RuntimeError::NotInterpreted {
                what: "a call record for a run that never called, or of another call".to_owned(),
                story: "E19-3",
            });
        };
        let id = batch.journal("ModelInvocationRecorded", None, invocation(record, &[])?)?;
        note_call(research, record);
        treat(research, &cancelled.pin, record.outcome, id, batch);
        return Ok(());
    };
    let Phase::Calling {
        observations,
        items,
    } = &run.phase
    else {
        research.run = Some(run);
        return Ok(());
    };
    if !owns(record, &run.call_id, &run.pin, observations, agent) {
        research.run = Some(run);
        return Err(RuntimeError::NotInterpreted {
            what: "a call record of another call, pin, agent, or retrieved context".to_owned(),
            story: "E19-3",
        });
    }
    let judged = match context.config {
        Some(config)
            if record.outcome == Outcome::Ok
                && record.completed_at <= record.deadline
                && record.completed_at.secs() <= run.bound.secs()
                && research_allowed(state, research, context).as_ref() == Some(&run.pin) =>
        {
            let called = Called {
                observations,
                items,
            };
            judge::judge(&run, &called, record, config, context, batch).unwrap_or_default()
        }
        _ => Judged::default(),
    };
    let (body, invalidations, theses) = match invocation(record, &judged.verdicts) {
        Ok(body) => (body, judged.invalidations, judged.theses),
        Err(_) => (invocation(record, &[])?, Vec::new(), Vec::new()),
    };
    let id = batch.journal("ModelInvocationRecorded", None, body)?;
    for invalidation in invalidations {
        batch.journal("ThesisInvalidated", Some(id.clone()), invalidation)?;
    }
    for thesis in theses {
        let thesis_record = batch.journal(thesis.event_type, Some(id.clone()), thesis.body)?;
        if let Some(output) = thesis.output {
            batch.journal("ModelOutputRecorded", Some(thesis_record), output)?;
        }
    }
    note_call(research, record);
    treat(research, &run.pin, record.outcome, id, batch);
    Ok(())
}

/// An agentic turn's reply (§6.9 step 4): refused, and never journaled as the run's, unless it is
/// the record of the turn's own call; judged and recorded through the loop while research may still
/// run and the run's deadline has not passed; otherwise journaled with nothing used, and the run
/// ends (`mode` or `deadline`), as a cancelled run's late answer is (§5.3).
fn turned(
    state: &RuntimeState,
    research: &mut ResearchState,
    run: Run,
    at: RiskClock,
    record: &CallRecord,
    context: &ResearchContext<'_>,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    let agent = state.deployment().agent.0.as_str();
    let Phase::Agentic(loop_) = &run.phase else {
        research.run = Some(run);
        return Ok(());
    };
    if !loop_.turning() || !owns(record, &run.call_id, &run.pin, &loop_.observations, agent) {
        research.run = Some(run);
        return Err(RuntimeError::NotInterpreted {
            what: "a call record of another call, pin, agent, or retrieved context".to_owned(),
            story: "E19-13",
        });
    }
    let pin = run.pin.clone();
    let allowed = research_allowed(state, research, context).as_ref() == Some(&run.pin);
    let on_time = record.completed_at.secs() <= run.bound.secs();
    let config = context.config.filter(|_| allowed && on_time);
    let outcome = drive(research, run, step, |run, loop_, batch| match config {
        Some(config) => agentic::replied(run, loop_, record, at, config, context, state, batch),
        None => {
            let reason = if on_time {
                EndReason::Mode
            } else {
                EndReason::Deadline
            };
            agentic::abandoned(loop_, record, reason, batch)
        }
    });
    if outcome.is_ok() {
        note_call(research, record);
        if let Some(head) = step
            .batch
            .drafts
            .first()
            .map(|draft| draft.event_id.clone())
        {
            treat(research, &pin, record.outcome, head, step.batch);
        }
    }
    outcome
}

/// Whether `record` is the record of this call: its id, pin, purpose, agent, and exactly the
/// journaled observations it was planned with, in order.
fn owns(
    record: &CallRecord,
    call_id: &str,
    pin: &ModelPin,
    observations: &[EventId],
    agent: &str,
) -> bool {
    record.call_id == call_id
        && record.model == *pin
        && record.purpose == Purpose::Research
        && record.agent_id.as_deref() == Some(agent)
        && record
            .observations
            .iter()
            .map(String::as_str)
            .eq(observations.iter().map(|id| id.0.as_str()))
}

/// The schedule counts from the call's completion, the instant the record carries, so a restart
/// folds back the same anchor (HI-8).
fn note_call(research: &mut ResearchState, record: &CallRecord) {
    let completed = RiskClock::from_secs(record.completed_at.secs());
    research.last_call = Some(
        research
            .last_call
            .map_or(completed, |last| last.max(completed)),
    );
}

/// The journaled context of the call being judged: its run's observations and their items' facts.
struct Called<'a> {
    observations: &'a [EventId],
    items: &'a [ItemFacts],
}

/// A call the gateway could not make (another workspace's, a malformed call id, an agent and
/// purpose that disagree, or a pin the registry does not hold): the run ends with no output and no
/// record (HI-6, inference spec §3.1), and research waits for a new version of the pin, since the
/// same request would be refused again. A refusal for a cancelled run's call ends that run too.
fn refused(
    research: &mut ResearchState,
    run_id: &str,
    _error: RequestError,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    let turning = research
        .run
        .as_ref()
        .is_some_and(|run| run.run_id == run_id && matches!(run.phase, Phase::Agentic(_)));
    if turning {
        return refused_turn(research, run_id, step);
    }
    let in_flight = research
        .run
        .take_if(|run| run.run_id == run_id && matches!(run.phase, Phase::Calling { .. }));
    let Some(run) = in_flight else {
        if research.cancelled.get(run_id).is_some_and(Option::is_some) {
            research.cancelled.remove(run_id);
            return Ok(());
        }
        return Err(RuntimeError::NotInterpreted {
            what: "a refused call for a run that never called".to_owned(),
            story: "E19-3",
        });
    };
    research.withdrawn.insert(run.pin);
    research.last_attempt = Some(run.cut_off);
    Ok(())
}

/// The gateway's refusal of an agentic run's turn (DEC-2261): the run ends `error` with no record,
/// and its pin is withdrawn as a refused single call's is. A refusal while no turn is out is not
/// interpreted, and the run stays as it was.
fn refused_turn(
    research: &mut ResearchState,
    run_id: &str,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    let Some(run) = research.run.take_if(|run| run.run_id == run_id) else {
        return not_ours(research, run_id, "a refused turn");
    };
    let pin = run.pin.clone();
    drive(research, run, step, agentic::refused)?;
    research.withdrawn.insert(pin);
    Ok(())
}

/// §7.3's treatment beyond "missing": an opaque operator alert, research disabled for a withdrawn
/// pin, and the three-in-a-row `deadline_exceeded` alert. Every notice carries only the call
/// record's id and a fixed key (`AGENTS.md` rule 6).
fn treat(
    research: &mut ResearchState,
    pin: &ModelPin,
    outcome: Outcome,
    record: EventId,
    batch: &mut Batch<'_>,
) {
    let treatment = treatment(outcome);
    if treatment.disable_research {
        research.withdrawn.insert(pin.clone());
    }
    if treatment.alert_operator {
        batch.notify(NotificationRef {
            subject_event: record.clone(),
            kind: RuntimeAlert::ResearchCallRefused,
        });
    }
    if !treatment.deadline_streak {
        research.deadline_streak = 0;
        return;
    }
    research.deadline_streak = research.deadline_streak.saturating_add(1);
    if research.deadline_streak >= 3 {
        research.deadline_streak = 0;
        batch.notify(NotificationRef {
            subject_event: record,
            kind: RuntimeAlert::ResearchDeadlineStreak,
        });
    }
}

/// `ModelInvocationRecorded`'s agent-stream payload (journal spec §9.19), with the verdicts.
fn invocation(record: &CallRecord, verdicts: &[Verdict]) -> Result<Value, RuntimeError> {
    record
        .agent_payload(verdicts)
        .map_err(|_| payload::non_canonical("model_invocation"))
}

/// An agentic run's dispatched results (§6.9 step 5). Results for a run that may no longer run
/// end it (`mode`), and nothing they hold is journaled, so nothing the model never sees is shown.
fn dispatched(
    state: &RuntimeState,
    research: &mut ResearchState,
    run_id: &str,
    at: RiskClock,
    results: &[DispatchedResult],
    context: &ResearchContext<'_>,
    step: &mut Step<'_, '_>,
) -> Result<(), RuntimeError> {
    let Some(run) = research
        .run
        .take_if(|run| run.run_id == run_id && matches!(run.phase, Phase::Agentic(_)))
    else {
        return not_ours(research, run_id, "a dispatch's results");
    };
    let config = match context.config {
        Some(config)
            if research_allowed(state, research, context).as_ref() == Some(&run.pin)
                && at <= run.bound =>
        {
            config
        }
        _ => {
            let reason = if at > run.bound {
                EndReason::Deadline
            } else {
                EndReason::Mode
            };
            return cancel(research, run, reason, step);
        }
    };
    drive(research, run, step, |run, loop_, batch| {
        agentic::dispatched(run, loop_, results, at, config, state, batch)
    })
}

/// The worker's acknowledgment of a cancellation, its own or the core's. One that never
/// dispatched a call has nothing left to answer, so it is forgotten; one that did stays
/// remembered, so its late answer is journaled.
fn acknowledged(research: &mut ResearchState, run_id: &str, dispatched: bool) {
    if let Some(run) = research.run.take_if(|run| run.run_id == run_id) {
        remember_cancelled(research, run);
    }
    let called = research.cancelled.get(run_id).is_some_and(Option::is_some);
    if !(dispatched && called) {
        research.cancelled.remove(run_id);
    }
}
