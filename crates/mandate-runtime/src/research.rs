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

mod judge;

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;
use mandate_canon::{Digest, Value};
use mandate_research::drift::{DriftState, InputClass, InputObservation};
use mandate_research::{
    AdmissionFacts, ContentHash, LineageState, PolicyOverlay, SourceId as ResearchSource,
    ValidatedMandate, WorkingUniverse, next_proposal_at,
};
use mandate_research_run::{
    CallFailure, CallOutcome, CallPlan, ErrorKind, GatewayReply, InstrumentFact, ItemId, ItemKind,
    ModelPin, OutputBounds, RetrievalPlan, RetrievedItem, SourceId, Symbol,
};
use mandate_time::UtcNanos;

use crate::error::RuntimeError;
use crate::payload;
use crate::ports::Ports;
use crate::state::{RuntimeState, UnresolvedAppend};
use crate::step::{Batch, observed};
use crate::types::{
    Effect, EventId, FoldedEvent, Input, Mode, NotificationRef, Observation, RiskClock,
};

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
    /// The call record is drafted; judging waits until it is folded back as committed (HI-2).
    Recorded {
        invocation: EventId,
        committed: bool,
        reply: GatewayReply,
        observations: Vec<EventId>,
        items: Vec<ItemFacts>,
    },
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
            starts: 0,
            deadline_streak: 0,
            withdrawn: BTreeSet::new(),
            cancelled: BTreeMap::new(),
            observed: BTreeSet::new(),
            drift: DriftState::new(),
            doubted: None,
        }
    }

    /// Folds one journaled event: the instant of every research call, and whether the call record
    /// the in-flight run waits on has committed.
    ///
    /// # Errors
    /// [`RuntimeError::NonCanonicalPayload`] for a research call record it cannot read.
    pub fn fold(&mut self, event: &FoldedEvent) -> Result<(), RuntimeError> {
        if event.event_type != "ModelInvocationRecorded"
            || payload::str_of(&event.payload, "purpose") != Some("research")
        {
            return Ok(());
        }
        let called = payload::stamped_clock_of(&event.payload, "called_at")
            .ok_or_else(|| payload::non_canonical("called_at"))?;
        self.last_call = Some(self.last_call.map_or(called, |last| last.max(called)));
        if let Some(Run {
            phase:
                Phase::Recorded {
                    invocation,
                    committed,
                    ..
                },
            ..
        }) = self.run.as_mut()
            && *invocation == event.event_id
        {
            *committed = true;
        }
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
    match &input {
        ResearchInput::Poll(now) => {
            not_behind(state, *now)?;
            polled(state, research, *now, context, &mut batch, &mut requests)?;
        }
        ResearchInput::Retrieved { run_id, items } => {
            let mut step = Step {
                batch: &mut batch,
                requests: &mut requests,
            };
            retrieved(state, research, run_id, items, context, &mut step)?;
        }
        ResearchInput::Called {
            run_id,
            at,
            outcome,
        } => {
            not_behind(state, *at)?;
            called(research, run_id, *at, outcome, &mut batch)?;
        }
        ResearchInput::Cancelled { run_id, dispatched } => {
            acknowledged(research, run_id, *dispatched);
        }
    }
    let step = ResearchStep {
        effects: batch.effects.clone(),
        requests,
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

/// What one step writes: its batch and its requests for the worker.
struct Step<'s, 'b> {
    batch: &'s mut Batch<'b>,
    requests: &'s mut Vec<ResearchRequest>,
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
    let enabled = context.preconditions.hold()
        && state.effective_mode() == Mode::Normal
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
    let anchor = match (research.last_call, context.last_reservation) {
        (Some(call), Some(reservation)) => Some(call.max(reservation)),
        (call, reservation) => call.or(reservation),
    };
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
    batch: &mut Batch<'_>,
    requests: &mut Vec<ResearchRequest>,
) -> Result<(), RuntimeError> {
    let allowed = research_allowed(state, research, context);
    if let Some(run) = research.run.take() {
        match &run.phase {
            Phase::Recorded {
                committed: false, ..
            } => research.run = Some(run),
            Phase::Recorded {
                committed: true, ..
            } => {
                if let (Some(config), Some(_)) = (context.config, allowed.as_ref()) {
                    judge::judge(&run, config, context, batch)?;
                }
            }
            Phase::Retrieving | Phase::Calling { .. } => {
                if allowed.as_ref() == Some(&run.pin) && now <= run.bound {
                    research.run = Some(run);
                } else {
                    requests.push(ResearchRequest::Cancel {
                        run_id: run.run_id.clone(),
                    });
                    remember_cancelled(research, run);
                }
            }
        }
        return Ok(());
    }
    let (Some(pin), Some(config)) = (allowed, context.config) else {
        return Ok(());
    };
    if !due(research, now, context)? {
        return Ok(());
    }
    let ordinal = u32::MAX.saturating_sub(research.starts.saturating_mul(2));
    let run_id = batch.id_at(ordinal);
    let call_id = batch.id_at(ordinal.saturating_sub(1));
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
    requests.push(ResearchRequest::Start {
        run_id: run_id.0,
        cut_off: now,
    });
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
        Phase::Retrieving | Phase::Recorded { .. } => None,
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
    if run.run_id != run_id || !matches!(run.phase, Phase::Retrieving) {
        research.run = Some(run);
        return not_ours(research, run_id, "a retrieval");
    }
    let config = match context.config {
        Some(config) if research_allowed(state, research, context).as_ref() == Some(&run.pin) => {
            config
        }
        _ => {
            step.requests.push(ResearchRequest::Cancel {
                run_id: run.run_id.clone(),
            });
            remember_cancelled(research, run);
            return Ok(());
        }
    };
    if items.is_empty() {
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

/// The gateway's settled answer: always journaled as `ModelInvocationRecorded`, whatever the
/// outcome and whether or not its run was cancelled (§5.3, §6.6). Only an `ok` answer to the run
/// in flight goes on to judging, and only once its record has committed.
fn called(
    research: &mut ResearchState,
    run_id: &str,
    at: RiskClock,
    outcome: &CallOutcome,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let in_flight = research
        .run
        .take_if(|run| run.run_id == run_id && matches!(run.phase, Phase::Calling { .. }));
    let Some(run) = in_flight else {
        let Some(cancelled) = research.cancelled.remove(run_id).flatten() else {
            return Err(RuntimeError::NotInterpreted {
                what: "a call answer for a run that never called".to_owned(),
                story: "E19-3",
            });
        };
        let call = Call {
            run_id,
            call_id: &cancelled.call_id,
            pin: &cancelled.pin,
            at,
            observations: &cancelled.observations,
            used: false,
        };
        let id = batch.journal("ModelInvocationRecorded", None, invocation(&call, outcome)?)?;
        treat(research, &cancelled.pin, outcome, id, batch);
        return Ok(());
    };
    let Phase::Calling {
        observations,
        items,
    } = run.phase.clone()
    else {
        research.run = Some(run);
        return Ok(());
    };
    let call = Call {
        run_id,
        call_id: &run.call_id,
        pin: &run.pin,
        at,
        observations: &observations,
        used: true,
    };
    let id = batch.journal("ModelInvocationRecorded", None, invocation(&call, outcome)?)?;
    treat(research, &run.pin, outcome, id.clone(), batch);
    if let Ok(reply) = outcome {
        research.run = Some(Run {
            phase: Phase::Recorded {
                invocation: id,
                committed: false,
                reply: reply.clone(),
                observations,
                items,
            },
            ..run
        });
    }
    Ok(())
}

/// §7.3's treatment beyond "missing": an opaque operator alert, research disabled for a withdrawn
/// pin, and the three-in-a-row `deadline_exceeded` alert. Every notice carries only the call
/// record's id and a fixed key (`AGENTS.md` rule 6).
fn treat(
    research: &mut ResearchState,
    pin: &ModelPin,
    outcome: &CallOutcome,
    record: EventId,
    batch: &mut Batch<'_>,
) {
    let Err(CallFailure { kind, .. }) = outcome else {
        research.deadline_streak = 0;
        return;
    };
    let treatment = kind.treatment();
    if treatment.disable_research {
        research.withdrawn.insert(pin.clone());
    }
    if treatment.alert_operator {
        batch.notify(NotificationRef {
            subject_event: record.clone(),
            message_key: "research_call_refused",
        });
    }
    if *kind != ErrorKind::DeadlineExceeded {
        research.deadline_streak = 0;
        return;
    }
    research.deadline_streak = research.deadline_streak.saturating_add(1);
    if research.deadline_streak >= 3 {
        research.deadline_streak = 0;
        batch.notify(NotificationRef {
            subject_event: record,
            message_key: "research_deadline_streak",
        });
    }
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

/// One call's identity and context, for its record.
struct Call<'a> {
    run_id: &'a str,
    call_id: &'a str,
    pin: &'a ModelPin,
    at: RiskClock,
    observations: &'a [EventId],
    used: bool,
}

/// `ModelInvocationRecorded`'s provisional payload (DEC-996 item 4): inference spec §3.6's members
/// that a settled outcome carries, the run id in place of the envelope's `correlation_id`, the
/// retrieved context as the run's observation ids, and whether the answer was used.
fn invocation(call: &Call<'_>, outcome: &CallOutcome) -> Result<Value, RuntimeError> {
    let digest = |value: &Digest| payload::text(&format!("sha256:{value}"));
    let (kind, reply, completed) = match outcome {
        Ok(reply) => ("ok", Some(reply), reply.completed_at),
        Err(failure) => (failure.kind.as_str(), None, failure.completed_at),
    };
    let reported = match reply {
        Some(reply) => payload::object(vec![
            ("model_id", payload::text(&reply.reported.model_id)),
            (
                "model_version",
                payload::text(&reply.reported.model_version),
            ),
            ("content_hash", digest(&reply.reported.content_hash)),
        ])?,
        None => Value::Null,
    };
    let reference =
        |pick: fn(&GatewayReply) -> &Digest| reply.map_or(Value::Null, |r| digest(pick(r)));
    payload::object(vec![
        ("run_id", payload::text(call.run_id)),
        ("call_id", payload::text(call.call_id)),
        ("purpose", payload::text("research")),
        ("model_id", payload::text(&call.pin.model_id)),
        ("model_version", payload::text(&call.pin.model_version)),
        ("content_hash", digest(&call.pin.content_hash)),
        ("outcome", payload::text(kind)),
        ("reported_identity", reported),
        ("request_digest", reference(|r| &r.request_digest)),
        ("prompt_ref", reference(|r| &r.prompt_ref)),
        ("response_ref", reference(|r| &r.response_ref)),
        (
            "cost_usd",
            reply.map_or(Value::Null, |r| payload::text(&r.cost_usd.to_string())),
        ),
        ("called_at", payload::stamp(call.at, "called_at")?),
        ("completed_at", payload::text(&completed.to_string())),
        (
            "retrieved_context",
            Value::Array(
                call.observations
                    .iter()
                    .map(|id| payload::text(&id.0))
                    .collect(),
            ),
        ),
        ("used", Value::Bool(call.used)),
    ])
}
