//! The folded state and the replay that builds it.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU8;

use mandate_accounting::InstrumentId;
use mandate_approval::{
    ApprovalRef, AskablePurpose, AssertionId, BoundAction, OpaqueUser, ReferenceMark, Request,
    RequestContent,
};
use mandate_canon::Value;
use mandate_num::Signed;

use crate::error::RuntimeError;
use crate::payload;
use crate::types::{
    Deployment, EventId, FoldedEvent, Handoff, Initiator, IntentBody, LocalHold, Mode, ModelOutput,
    Outstanding, OwnerConfirmation, Purpose, RiskClock, Seq, SignalInputs, WriterEpoch,
};

/// The `fold_version` of ADR-0001 ES-21. Bumped whenever fold output changes, with the golden
/// journal regenerated in the same change.
pub const FOLD_VERSION: u32 = 1;

/// What the runtime knows, derived from journaled events and nothing else.
///
/// Every field is private and every collection is ordered (ES-21). The folded position of each
/// followed stream lives here and is re-derived by replay, so nothing durable exists outside the
/// journal and a restart cannot mistake an old event for a new one (DEC-131 item 17).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeState {
    deployment: Deployment,
    heads: BTreeMap<String, Seq>,
    epoch: Option<WriterEpoch>,
    started: bool,
    unresolved: Option<UnresolvedAppend>,
    risk_clock: Option<RiskClock>,
    last_evaluated: Option<RiskClock>,
    copied_mode: Mode,
    journaled_mode: Mode,
    lifecycle: Mode,
    switch: Option<Switch>,
    shown: Option<OwnerConfirmation>,
    awaiting_reconciliation: bool,
    reconciled: bool,
    submission_unreconciled: bool,
    pending_approvals: BTreeMap<EventId, PendingApproval>,
    outstanding: BTreeMap<EventId, Outstanding>,
    bodies: BTreeMap<EventId, IntentBody>,
    acknowledged: BTreeMap<EventId, Seq>,
    outputs: BTreeMap<String, BTreeMap<InstrumentId, ModelOutput>>,
    /// The `ModelOutputRecorded` each held output came from, which a request cites as evidence.
    output_events: BTreeMap<String, BTreeMap<InstrumentId, EventId>>,
    /// The last folded `MarkUpdated` of each instrument and its account-stream `seq`: the reference
    /// a request records and the mark re-validation compares with it (DEC-156 item 3).
    marks: BTreeMap<InstrumentId, ReferenceMark>,
    /// The control-stream events this runtime has already copied: the `causation_id` of every
    /// `ApprovalResponded`, `AgentModeChanged`, `KillSwitchActivated`, `OwnerExitRequested`, and
    /// `OwnerCommandRefused` (DEC-291). The
    /// control stream's `event_id` is the idempotency key (DEC-155 item 2), so one re-tailed, or
    /// re-tailed after a restart, is copied and acted on once (EI-3).
    copied: BTreeSet<EventId>,
    /// Every step-up assertion a control-stream event has carried, with the first event that
    /// carried it. Evidence is usable once per workspace (EI-11), so any other event that repeats
    /// an assertion is reusing it, and a re-tail of the first is not.
    assertions: BTreeMap<AssertionId, EventId>,
    /// The owner's exits of one instrument, by the `OwnerExitRequested` that records each, so that a
    /// restart can hand one the executor has not taken again (`AGENTS.md` rule 13, DEC-257 item 7).
    owner_exits: BTreeMap<EventId, OwnerExit>,
}

/// An owner's exit of one instrument as the fold holds it: what the planner is asked for again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OwnerExit {
    pub(crate) instrument: InstrumentId,
    pub(crate) confirmation: Option<OwnerConfirmation>,
    pub(crate) handoff: Handoff,
}

/// The kill switch the fold remembers. The `event` is the `KillSwitchActivated` that identifies the
/// flatten's handoff, which is what lets `Input::Started` hand an unfinished flatten again: the sink
/// is at-least-once by design, so handing one twice costs nothing and handing none at all would mean
/// a switch the owner pulled never reached the executor (`AGENTS.md` rule 13, DEC-131 item 22).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Switch {
    pub(crate) event: EventId,
    pub(crate) initiator: Initiator,
    pub(crate) confirmation: Option<OwnerConfirmation>,
    /// The `AgentModeChanged` this switch wrote, which the executor copies back as
    /// `AgentModeApplied` carrying this id as its `causation_id` (journal spec §2). Identity, not
    /// arrival order, is what ties that copy to **this** switch: a flag set by whichever copy
    /// happened to fold first answers differently on a replay, where the agent stream folds ahead of
    /// the account stream (journal spec §8). `None` when the switch changed no mode, in which case
    /// there is no copy to confirm it and nothing can lift it.
    pub(crate) mode_event: Option<EventId>,
    /// The account-stream `seq` of the copy that confirmed this switch, or `None` while unconfirmed.
    pub(crate) confirmed_at: Option<Seq>,
}

/// A batch whose append has not been answered. The input and the drafts are kept so that the only
/// permitted next step is the same input, which re-emits the same drafts and lets the append answer
/// `AlreadyCommitted` rather than appending a second event (journal spec §5.1, DEC-131 item 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedAppend {
    pub head: Seq,
    pub input: crate::types::Input,
    pub drafts: Vec<crate::types::EventDraft>,
}

/// An approval the runtime is waiting on, with what it binds (mandate spec §6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingApproval {
    pub instrument: InstrumentId,
    pub mandate_version: String,
    pub deadline: RiskClock,
    pub adds_risk: bool,
    /// What admission and re-validation read: the bound action, the deadline, the content hash the
    /// request stated, whether `cli_inbox` delivered it, and the approvers whose grants were
    /// `counted` so far. The evidence and risk figures stay in the committed content object, whose
    /// hash is what a response must repeat (EI-14), so the fold does not hold them again.
    pub request: Request,
}

impl RuntimeState {
    /// An empty state for one deployment, before any event is folded. Reads nothing: there is no
    /// constructor that touches a clock, a file, or a random number. The deployment's ids are what
    /// let the fold reject another workspace's stream and the kill switch tell its own scope from a
    /// sibling's, so they are given rather than inferred from the first event folded.
    pub fn new(deployment: Deployment) -> Self {
        Self {
            deployment,
            heads: BTreeMap::new(),
            epoch: None,
            started: false,
            unresolved: None,
            risk_clock: None,
            last_evaluated: None,
            copied_mode: Mode::Normal,
            journaled_mode: Mode::Normal,
            lifecycle: Mode::Normal,
            switch: None,
            shown: None,
            awaiting_reconciliation: false,
            reconciled: false,
            submission_unreconciled: false,
            pending_approvals: BTreeMap::new(),
            outstanding: BTreeMap::new(),
            bodies: BTreeMap::new(),
            acknowledged: BTreeMap::new(),
            outputs: BTreeMap::new(),
            output_events: BTreeMap::new(),
            marks: BTreeMap::new(),
            copied: BTreeSet::new(),
            assertions: BTreeMap::new(),
            owner_exits: BTreeMap::new(),
        }
    }

    /// Which deployment this runtime is.
    pub fn deployment(&self) -> &Deployment {
        &self.deployment
    }

    /// The effective mode: the strictest of the copied account-stream mode and every local hold
    /// (mandate spec §5.9, MI-6). A maximum over [`Mode`]'s ordering, so the runtime is never less
    /// strict than the account stream and never lifts what the account stream set.
    pub fn effective_mode(&self) -> Mode {
        self.mode_with(self.lifecycle)
    }

    /// Whether an intent of this purpose may be **proposed** in the effective mode.
    pub fn permits(&self, purpose: Purpose) -> bool {
        match self.effective_mode() {
            Mode::Normal => true,
            Mode::ExitsOnly => !purpose.adds_risk(),
            Mode::Paused | Mode::Stopped => false,
        }
    }

    /// The intents a restart would hand again: those the account stream has not yet taken.
    pub fn pending_handoffs(&self) -> Vec<&Outstanding> {
        self.outstanding
            .values()
            .filter(|live| matches!(live.handoff, Handoff::Pending))
            .collect()
    }

    /// Whether a restart may **re-hand** an intent whose `IntentProposed` already committed.
    ///
    /// The startup hold is excluded from this gate, which [`Self::permits`] applies: the hold means
    /// "the broker's truth is not yet confirmed", which is a reason not to *decide*, while dropping
    /// an exit already journaled would remove protection rather than add it, and trading-domain spec
    /// §5.5 lets only `paused`, `stopped`, an `Unknown` order, or the broker hold an exit. The copied
    /// account mode and the lifecycle state still govern, so `stopped` re-hands a flatten alone and
    /// `exits_only` re-hands no opening (DEC-131 item 22).
    pub fn permits_rehand(&self, purpose: Purpose) -> bool {
        match self.rehand_mode() {
            Mode::Normal => true,
            Mode::ExitsOnly => !purpose.adds_risk(),
            Mode::Paused => matches!(purpose, Purpose::Protective | Purpose::Flatten),
            Mode::Stopped => purpose == Purpose::Flatten,
        }
    }

    /// The folded `seq` of one stream, or `None` for a stream with nothing folded yet.
    pub fn head(&self, stream: &str) -> Option<Seq> {
        self.heads.get(stream).copied()
    }

    /// The latest risk-clock second the fold has seen.
    pub fn risk_clock(&self) -> Option<RiskClock> {
        self.risk_clock
    }

    /// The intents whose `IntentProposed` committed and which have no terminal outcome yet.
    pub fn outstanding(&self) -> &BTreeMap<EventId, Outstanding> {
        &self.outstanding
    }

    /// The approvals the runtime is waiting on.
    pub fn pending_approvals(&self) -> &BTreeMap<EventId, PendingApproval> {
        &self.pending_approvals
    }

    /// The effective mode a given deployment lifecycle would produce: the strictest of the copied
    /// account-stream mode, that lifecycle, the startup hold, and the kill switch's final mode. A
    /// maximum over [`Mode`]'s ordering, so the runtime is never less strict than the account stream
    /// and never lifts what the account stream set.
    pub(crate) fn mode_with(&self, lifecycle: Mode) -> Mode {
        let mut mode = self.copied_mode.max(lifecycle);
        if self.awaiting_reconciliation {
            mode = mode.max(LocalHold::AwaitingReconciliation.mode());
        }
        if let Some(switch) = &self.switch {
            mode = mode.max(switch.initiator.final_mode());
        }
        mode
    }

    /// The deployment's own lifecycle state, as a mode rather than as a set of
    /// [`LocalHold`]s. It is a mode because `AgentModeChanged` is the only agent-stream event that
    /// can carry it and MI-6 writes that event only when the effective mode changed, so a mode is the
    /// one form of this state a replay can restore (DEC-131 item 25).
    pub(crate) fn lifecycle(&self) -> Mode {
        self.lifecycle
    }

    /// The same maximum as [`Self::effective_mode`] without the startup hold, which governs new
    /// proposals alone (DEC-131 item 22).
    fn rehand_mode(&self) -> Mode {
        let mut mode = self.copied_mode.max(self.lifecycle);
        if let Some(switch) = &self.switch {
            mode = mode.max(switch.initiator.final_mode());
        }
        mode
    }

    /// Whether the broker's truth is known: a clean `ReconciliationRun` has been folded and no
    /// submission has happened since it, which is what "a reconciliation at or after the last
    /// observed submission" comes to on a single ordered stream (trading spec §11, DEC-131 item 13).
    ///
    /// This is what **lifts** the startup hold. Taking it is stricter — it also wants nothing
    /// outstanding (see [`Self::take_startup_hold`]) — and the two must not be one predicate: an
    /// intent stays outstanding until a terminal outcome the runtime folds as inert, so a single
    /// predicate would hold an agent `paused` for ever after its first order, which no reading of
    /// trading spec §11 supports and which would stop even a discretionary exit.
    fn reconciliation_known(&self) -> bool {
        self.reconciled && !self.submission_unreconciled
    }

    /// The stream this runtime writes, which is the only one it appends to (journal spec §2).
    pub(crate) fn agent_stream(&self) -> String {
        format!(
            "agent:{}:{}",
            self.deployment.workspace.0, self.deployment.agent.0
        )
    }

    /// Whether the fold follows a stream at all: this workspace's agent, account, control, and
    /// scheduler streams, and nothing of another workspace's.
    fn follows(&self, stream: &str) -> bool {
        let mut segments = stream.split(':');
        let kind = segments.next();
        let workspace = segments.next();
        if matches!(kind, Some("agent")) {
            return stream == self.agent_stream();
        }
        matches!(kind, Some("acct" | "ctl" | "clock"))
            && workspace == Some(self.deployment.workspace.0.as_str())
    }

    pub(crate) fn has_started(&self) -> bool {
        self.started
    }

    pub(crate) fn start(&mut self, epoch: WriterEpoch) {
        self.started = true;
        self.epoch = Some(epoch);
    }

    pub(crate) fn epoch(&self) -> Option<WriterEpoch> {
        self.epoch
    }

    /// The head the next batch is built against: the runtime's own stream position (journal §5.1).
    pub(crate) fn agent_head(&self) -> Seq {
        self.head(&self.agent_stream()).unwrap_or(Seq(0))
    }

    pub(crate) fn journaled_mode(&self) -> Mode {
        self.journaled_mode
    }

    /// Takes the startup hold unless the fold already proves the broker's truth is known
    /// (DEC-131 item 13). Called by `Input::Started` and by nothing else, so the hold is a decision
    /// a process takes once, at the point where its replay ends.
    pub(crate) fn take_startup_hold(&mut self) {
        self.awaiting_reconciliation =
            !(self.reconciliation_known() && self.outstanding.is_empty());
    }

    /// Lifts the startup hold once, when the reconciliation it waits for has arrived. Latched
    /// rather than recomputed, so a later proposal cannot re-impose a hold the account stream has
    /// already cleared.
    pub(crate) fn settle_startup_hold(&mut self) {
        if self.awaiting_reconciliation && self.reconciliation_known() {
            self.awaiting_reconciliation = false;
        }
    }

    /// The risk-clock second a tick is handled at. Monotone by construction: a tick that arrives
    /// out of order is read at the second the state already holds, which never adds risk and never
    /// moves a deadline backwards (mandate spec §5.2, `AGENTS.md` rule 3).
    pub(crate) fn advance(&mut self, at: RiskClock) -> RiskClock {
        let now = self.risk_clock.map_or(at, |last| last.max(at));
        self.risk_clock = Some(now);
        now
    }

    pub(crate) fn already_evaluated(&self, now: RiskClock) -> bool {
        self.last_evaluated == Some(now)
    }

    pub(crate) fn mark_evaluated(&mut self, now: RiskClock) {
        self.last_evaluated = Some(now);
    }

    /// The signal inputs a decision reads, from the outputs the fold holds (mandate spec §8.1).
    pub(crate) fn signal_inputs(&self, now: RiskClock) -> SignalInputs {
        SignalInputs {
            outputs: self.outputs.clone(),
            now,
        }
    }

    /// The intent event ids a flatten plan is asked about: every intent the fold still holds
    /// live, whether or not the executor has taken it, because an order the executor took and is
    /// working is exactly the one a kill switch must name (trading spec §5.5, DEC-131 item 22).
    /// The plan's cancels name client order ids, a mapping the exit adapter derives from the
    /// journal (DEC-449 item 3).
    pub(crate) fn working_orders(&self) -> Vec<String> {
        self.outstanding.keys().map(|id| id.0.clone()).collect()
    }

    /// Whether an approval that would **add** risk is waiting. Only a risk-adding proposal defers to
    /// one: holding a discretionary or protective exit behind an opening ASK would be a hold
    /// trading-domain spec §5.5 gives only `paused`, `stopped`, an `Unknown` order, or the broker.
    pub(crate) fn awaiting_risk_approval(&self) -> bool {
        self.pending_approvals
            .values()
            .any(|pending| pending.adds_risk)
    }

    /// The kill switch the fold holds, if one has been journaled.
    pub(crate) fn switch(&self) -> Option<&Switch> {
        self.switch.as_ref()
    }

    /// The last folded mark of one instrument.
    pub(crate) fn mark(&self, instrument: &InstrumentId) -> Option<ReferenceMark> {
        self.marks.get(instrument).copied()
    }

    /// The `ModelOutputRecorded` events behind every output for `instrument` still unexpired at
    /// `now`: the evidence a request cites (mandate spec §6.4).
    pub(crate) fn evidence_for(&self, instrument: &InstrumentId, now: RiskClock) -> Vec<EventId> {
        self.outputs
            .iter()
            .filter_map(|(model, by_instrument)| {
                by_instrument
                    .get(instrument)
                    .filter(|output| output.expires_at >= now)
                    .and_then(|_| self.output_events.get(model)?.get(instrument).cloned())
            })
            .collect()
    }

    /// Whether this runtime has already copied a control-stream event (DEC-155 item 2).
    pub(crate) fn has_copied(&self, control: &EventId) -> bool {
        self.copied.contains(control)
    }

    /// The assertions some other control-stream event than `event` carried first: what check 6
    /// reads as already used (EI-11).
    pub(crate) fn used_assertions(&self, event: &EventId) -> BTreeSet<AssertionId> {
        self.assertions
            .iter()
            .filter(|(_, first)| *first != event)
            .map(|(assertion, _)| assertion.clone())
            .collect()
    }

    /// The owner exits the executor has not been seen to take, which a restart hands again.
    pub(crate) fn untaken_owner_exits(&self) -> impl Iterator<Item = (&EventId, &OwnerExit)> {
        self.owner_exits
            .iter()
            .filter(|(_, exit)| matches!(exit.handoff, Handoff::Pending))
    }

    /// The intent event ids of the intents still live in one instrument: all an owner's exit of
    /// that instrument may cancel (mandate spec §6.1, trading-domain spec §5.5). The plan's
    /// cancels name client order ids, a mapping the exit adapter derives from the journal
    /// (DEC-449 item 3).
    pub(crate) fn working_orders_in(&self, instrument: &InstrumentId) -> Vec<String> {
        self.outstanding
            .keys()
            .filter(|id| {
                matches!(
                    self.bodies.get(*id),
                    Some(IntentBody::Order { instrument: held, .. }) if held == instrument
                )
            })
            .map(|id| id.0.clone())
            .collect()
    }

    pub(crate) fn body_of(&self, intent: &EventId) -> Option<IntentBody> {
        self.bodies.get(intent).cloned()
    }

    pub(crate) fn unresolved_batch(&self) -> Option<UnresolvedAppend> {
        self.unresolved.clone()
    }

    /// Remembers a batch whose append has not been answered, so the only step the runtime will take
    /// next is the same one, re-deriving the same ids (journal spec §5.1).
    pub(crate) fn remember(&mut self, batch: UnresolvedAppend) {
        self.unresolved = Some(batch);
    }

    /// Clears a remembered batch once its last draft has been folded back, which is how a single
    /// writer learns that its own append committed (journal spec §5.1).
    fn resolve(&mut self, appended: &EventId) {
        let done = self.unresolved.as_ref().is_some_and(|batch| {
            batch
                .drafts
                .last()
                .is_some_and(|last| last.event_id == *appended)
        });
        if done {
            self.unresolved = None;
        }
    }
}

/// Replays one journaled event into the state.
///
/// Total over the catalogue and **effect-free**: a replay can never re-send anything, which is half
/// of crash safety (the other half is that only [`crate::handle`] produces effects). An event type,
/// payload field, or stream the core does not interpret is [`RuntimeError::NotInterpreted`], never a
/// silent no-op (DEC-85).
pub fn fold(state: &mut RuntimeState, event: &FoldedEvent) -> Result<(), RuntimeError> {
    if !state.follows(&event.stream) {
        return Err(RuntimeError::ForeignStream {
            stream: event.stream.clone(),
        });
    }
    let expected = state
        .heads
        .get(&event.stream)
        .map_or(1, |head| head.0.saturating_add(1));
    if event.seq.0 != expected {
        return Err(RuntimeError::SequenceOutOfOrder {
            stream: event.stream.clone(),
            expected,
            found: event.seq.0,
        });
    }
    interpret(state, event)?;
    state.heads.insert(event.stream.clone(), event.seq);
    state.resolve(&event.event_id);
    Ok(())
}

/// What one event type means to the runtime's state. The arms that change nothing are named rather
/// than absent, because DEC-85 makes the difference between "interpreted and inert" and "unknown"
/// the difference between a fold and a silent loss.
fn interpret(state: &mut RuntimeState, event: &FoldedEvent) -> Result<(), RuntimeError> {
    if let Some(at) = payload::clock_of(&event.payload, "risk_clock") {
        state.risk_clock = Some(state.risk_clock.map_or(at, |last| last.max(at)));
    }
    match event.event_type.as_str() {
        "AgentModeChanged" => {
            copy_of(state, event);
            state.journaled_mode = mode_field(event)?;
            state.lifecycle = payload::str_of(&event.payload, "lifecycle")
                .and_then(payload::mode_from)
                .ok_or_else(|| payload::non_canonical("lifecycle"))?;
        }
        "KillSwitchActivated" => {
            copy_of(state, event);
            let initiator = payload::str_of(&event.payload, "initiator")
                .and_then(payload::initiator_from)
                .ok_or_else(|| payload::non_canonical("initiator"))?;
            state.switch = Some(Switch {
                event: event.event_id.clone(),
                initiator,
                confirmation: state.shown.take(),
                mode_event: payload::str_of(&event.payload, "mode_event")
                    .map(|id| EventId(id.to_owned())),
                confirmed_at: None,
            });
        }
        "OwnerExitRequested" => {
            copy_of(state, event);
            let confirmation = payload::honoured_confirmation(&event.payload)?;
            if payload::str_of(&event.payload, "scope") == Some("instrument") {
                let subject = payload::str_of(&event.payload, "subject")
                    .ok_or_else(|| payload::non_canonical("subject"))?;
                state.owner_exits.insert(
                    event.event_id.clone(),
                    OwnerExit {
                        instrument: payload::instrument_of(subject)?,
                        confirmation,
                        handoff: Handoff::Pending,
                    },
                );
            } else {
                state.shown = confirmation;
            }
        }
        "IntentProposed" => {
            let purpose = payload::str_of(&event.payload, "purpose")
                .and_then(payload::purpose_from)
                .ok_or_else(|| payload::non_canonical("purpose"))?;
            state
                .bodies
                .insert(event.event_id.clone(), payload::order_of(&event.payload)?);
            state.outstanding.insert(
                event.event_id.clone(),
                Outstanding {
                    intent_id: event.event_id.clone(),
                    purpose,
                    handoff: Handoff::Pending,
                },
            );
        }
        "ApprovalRequested" => {
            state.pending_approvals.insert(
                event.event_id.clone(),
                requested(&event.event_id, &event.payload)?,
            );
        }
        "ApprovalDelivered" => {
            if payload::str_of(&event.payload, "status") == Some("delivered")
                && let Some(pending) = approval_of(&event.payload)
                    .and_then(|approval| state.pending_approvals.get_mut(&approval))
            {
                pending.request.delivered = true;
            }
        }
        "ApprovalResponded" => {
            copy_of(state, event);
            responded(state, &event.payload);
        }
        "OwnerCommandRefused" => copy_of(state, event),
        "ApprovalRevalidated" | "ApprovalTimedOut" | "ApprovalCanceled" => {
            if let Some(approval) = approval_of(&event.payload) {
                state.pending_approvals.remove(&approval);
            }
        }
        "ApprovalResponseSubmitted" | "OwnerCommandIssued" => {
            if let Some(evidence) = payload::step_up_of(&event.payload) {
                state
                    .assertions
                    .entry(evidence.assertion)
                    .or_insert_with(|| event.event_id.clone());
            }
        }
        "ModelOutputRecorded" => {
            let output = payload::model_output_of(&event.payload)?;
            state
                .output_events
                .entry(output.model.clone())
                .or_default()
                .insert(output.instrument.clone(), event.event_id.clone());
            state
                .outputs
                .entry(output.model.clone())
                .or_default()
                .insert(output.instrument.clone(), output);
        }
        "MarkUpdated" => {
            let instrument = payload::str_of(&event.payload, "instrument")
                .ok_or_else(|| payload::non_canonical("instrument"))?;
            let price = payload::str_of(&event.payload, "price")
                .ok_or_else(|| payload::non_canonical("price"))?;
            state.marks.insert(
                payload::instrument_of(instrument)?,
                ReferenceMark {
                    price: payload::price_of(price)?,
                    seq: event.seq.0,
                },
            );
        }
        "AgentModeApplied" => {
            if addressed_here(state, &event.payload) {
                let to = mode_field(event)?;
                state.copied_mode = to;
                state.switch = retired(state.switch.take(), to, event, &state.acknowledged);
            }
        }
        "ReconciliationRun" => {
            if matches!(payload::str_of(&event.payload, "result"), Some("clean")) {
                state.reconciled = true;
                state.submission_unreconciled = false;
            }
        }
        "OrderSubmitted" => state.submission_unreconciled = true,
        "OwnerAcknowledged" => {
            state.acknowledged.insert(event.event_id.clone(), event.seq);
        }
        "IntentReceived" => {
            if let Some(intent) = payload::str_of(&event.payload, "intent_id") {
                let intent = EventId(intent.to_owned());
                if let Some(live) = state.outstanding.get_mut(&intent) {
                    live.handoff = Handoff::Taken;
                }
                if let Some(exit) = state.owner_exits.get_mut(&intent) {
                    exit.handoff = Handoff::Taken;
                }
            }
        }
        "StreamOpened"
        | "GateDecided"
        | "OrderAbandoned"
        | "ObservationRecorded"
        | "ModelInvocationRecorded"
        | "DecisionMade"
        | "ThesisProposed"
        | "ThesisRevised"
        | "OrderStateChanged"
        | "BrokerExchangeRecorded"
        | "FillApplied"
        | "LateFillApplied"
        | "FeesCharged"
        | "SettlementPosted"
        | "DividendPaid"
        | "CashInLieuPosted"
        | "CorporateActionPrepared"
        | "CorporateActionApplied"
        | "ProtectionChanged"
        | "BrokerPositionObserved"
        | "CompensatingEvent"
        | "AccountSnapshotRecorded"
        | "AccountStateObserved"
        | "RejectObserved"
        | "AccountRestrictionChanged"
        | "ExternalActivityIngested"
        | "RelatedAccountsCoordination"
        | "ConductBreachDetected"
        | "TradingDayStarted"
        | "MandateVersionApplied"
        | "RiskDayStarted"
        | "RiskLimitTriggered"
        | "RiskLimitLifted"
        | "HighWaterMarkReset"
        | "PositionReleased"
        | "InstrumentRestrictionChanged"
        | "GoalCompleted"
        | "UniverseChanged"
        | "ClockAdvanced"
        | "ClockOffsetRecorded"
        | "ClockToleranceExceeded"
        | "OwnerAlertSent"
        | "PlatformOperatorAction" => {}
        unknown => {
            return Err(RuntimeError::NotInterpreted {
                what: unknown.to_owned(),
                story: "the story that writes it",
            });
        }
    }
    Ok(())
}

/// What an account-stream mode change does to a kill switch the runtime still holds.
///
/// A `RiskLimit` flatten's restriction lifts through the account stream by the owner's
/// acknowledgment and nowhere else (brief item 21, mandate spec §5.4, §5.7, §5.8). Every step of that
/// is keyed on **event identity**, never on what folded first, because a rule that reads arrival
/// order answers differently on a replay: journal spec §8 replays the agent stream ahead of the
/// account stream, so a flag set by "some copy at or above my mode" lets one switch's echo confirm a
/// later switch and one switch's acknowledgment lift it.
///
/// - **Confirmed** only by the copy whose `causation_id` is this switch's own `AgentModeChanged`
///   (journal spec §2 makes the executor write exactly that). The switch's own `paused` returning as
///   an account-stream fact is therefore a confirmation and never a lift; lifting on it would retire
///   the switch the instant it was pulled and leave a restart with no flatten to hand.
/// - **Lifted** only by a later copy to a looser mode whose `causation_id` names an
///   `OwnerAcknowledged` this fold has seen on the account stream. A `normal` written before the
///   breach cites no such acknowledgment, so it cannot lift, however the streams interleave — and the
///   streams have no global order at all (journal spec §2).
/// - Never by anything, once the initiator's final mode is `stopped` (DEC-131 item 12).
///
/// The ruling also asked for the lifting copy's `seq` to exceed the confirming one. That comparison
/// is omitted because it can never be false where it would be read: a stream folds gaplessly in `seq`
/// order, so any copy folded after the confirmation already has the greater `seq`. An unreachable
/// branch is one the mutation gate cannot cover, and `confirmed_at` records the confirming `seq` for
/// a reader either way.
fn retired(
    switch: Option<Switch>,
    to: Mode,
    event: &FoldedEvent,
    acknowledged: &BTreeMap<EventId, Seq>,
) -> Option<Switch> {
    let mut switch = switch?;
    let final_mode = switch.initiator.final_mode();
    if final_mode == Mode::Stopped {
        return Some(switch);
    }
    if let Some(mode_event) = &switch.mode_event
        && event.causation_id.as_ref() == Some(mode_event)
    {
        switch.confirmed_at = Some(event.seq);
        return Some(switch);
    }
    if switch.confirmed_at.is_none() {
        return Some(switch);
    }
    if to >= final_mode {
        return Some(switch);
    }
    let lifts = event
        .causation_id
        .as_ref()
        .is_some_and(|cause| acknowledged.contains_key(cause));
    if lifts { None } else { Some(switch) }
}

/// Whether an agent-scoped fact on the shared account stream is this deployment's. Mandate spec
/// §5.10 gives `AgentModeApplied` an agent field, and the account stream carries every agent on the
/// connection, so a sibling's `normal` must not lift this agent's restriction (`AGENTS.md` rules 1
/// and 3). A payload with no agent field is this agent's by construction: the runtime follows only
/// its own account stream, and the frozen fixtures carry none.
fn addressed_here(state: &RuntimeState, payload: &mandate_canon::Value) -> bool {
    payload::str_of(payload, "agent").is_none_or(|agent| agent == state.deployment.agent.0)
}

fn mode_field(event: &FoldedEvent) -> Result<Mode, RuntimeError> {
    payload::str_of(&event.payload, "to")
        .and_then(payload::mode_from)
        .ok_or_else(|| payload::non_canonical("to"))
}

/// Records that an agent-stream event copies the control-stream event its `causation_id` names, so
/// the runtime never copies that event again (DEC-155 item 2, EI-3). Only the five copy types call
/// it, and a `causation_id` that names anything else is never looked up as a control event.
fn copy_of(state: &mut RuntimeState, event: &FoldedEvent) {
    if let Some(cause) = &event.causation_id {
        state.copied.insert(cause.clone());
    }
}

fn approval_of(payload: &Value) -> Option<EventId> {
    payload::str_of(payload, "approval").map(|approval| EventId(approval.to_owned()))
}

/// What an `ApprovalResponded` does to its approval: a `refused` answer leaves it pending (EI-6), a
/// `counted` grant joins the grant set check 7 counts, and anything else ends it, a legacy record
/// with no `result` included (journal spec §9).
fn responded(state: &mut RuntimeState, payload: &Value) {
    let Some(approval) = approval_of(payload) else {
        return;
    };
    match payload::str_of(payload, "result") {
        Some("refused") => {}
        Some("counted") => {
            if let (Some(pending), Some(responder)) = (
                state.pending_approvals.get_mut(&approval),
                payload::str_of(payload, "responder"),
            ) {
                pending
                    .request
                    .grants
                    .insert(OpaqueUser(responder.to_owned()));
            }
        }
        _ => {
            state.pending_approvals.remove(&approval);
        }
    }
}

/// What an `ApprovalRequested` binds: the action, the trigger, the reference mark, the approver
/// requirement, the deadline, and the content hash the request stated (mandate spec §6.4). An exit
/// is never asked (`AGENTS.md` rule 2), so a purpose that is not `open` or `increase` is refused.
fn requested(id: &EventId, payload: &Value) -> Result<PendingApproval, RuntimeError> {
    let text = |field: &'static str| {
        payload::str_of(payload, field).ok_or_else(|| payload::non_canonical(field))
    };
    let instrument = payload::instrument_of(text("instrument")?)?;
    let purpose = match text("purpose")? {
        "open" => AskablePurpose::Open,
        "increase" => AskablePurpose::Increase,
        _ => return Err(payload::non_canonical("purpose")),
    };
    let asset_class = match text("asset_class")? {
        "us_equity" => mandate_approval::AssetClass::UsEquity,
        "crypto" => mandate_approval::AssetClass::Crypto,
        _ => return Err(payload::non_canonical("asset_class")),
    };
    let deadline =
        payload::clock_of(payload, "deadline").ok_or_else(|| payload::non_canonical("deadline"))?;
    let reference_mark = match payload.get("reference_mark") {
        Some(Value::Null) => None,
        Some(mark) => Some(ReferenceMark {
            price: payload::price_of(
                payload::str_of(mark, "price").ok_or_else(|| payload::non_canonical("price"))?,
            )?,
            seq: mark
                .get("seq")
                .and_then(Value::as_int)
                .ok_or_else(|| payload::non_canonical("seq"))?,
        }),
        None => return Err(payload::non_canonical("reference_mark")),
    };
    let approvers_required = payload
        .get("approvers_required")
        .and_then(Value::as_int)
        .and_then(|n| u8::try_from(n).ok())
        .and_then(NonZeroU8::new)
        .ok_or_else(|| payload::non_canonical("approvers_required"))?;
    let independent_required = match payload.get("independent_required") {
        Some(Value::Bool(required)) => *required,
        _ => return Err(payload::non_canonical("independent_required")),
    };
    let content_hash = payload::hash_from(text("content_hash")?)
        .ok_or_else(|| payload::non_canonical("content_hash"))?;
    let version = text("mandate_version")?;
    let bound = BoundAction {
        instrument: instrument.as_str().to_owned(),
        asset_class,
        qty: payload::qty_of(text("qty")?)?,
        limit: payload::price_of(text("limit")?)?,
        purpose,
        mandate_version: version.to_owned(),
        decided_by: text("decided_by")?.to_owned(),
        combined_score: Signed::parse(text("combined_score")?)?,
        reference_mark,
        approvers_required,
        independent_required,
    };
    Ok(PendingApproval {
        instrument,
        mandate_version: version.to_owned(),
        deadline,
        adds_risk: true,
        request: Request {
            id: ApprovalRef::of_requested_event(&id.0)?,
            content: RequestContent {
                bound,
                evidence: Vec::new(),
                risk_impact: Vec::new(),
                deadline: mandate_approval::RiskClock(deadline.secs()),
            },
            content_hash,
            delivered: false,
            grants: BTreeSet::new(),
        },
    })
}
