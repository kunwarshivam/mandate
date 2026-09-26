//! The folded state and the replay that builds it.

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::InstrumentId;

use crate::error::RuntimeError;
use crate::payload;
use crate::types::{
    Deployment, EventId, FoldedEvent, Handoff, IntentBody, LocalHold, Mode, ModelOutput,
    Outstanding, Purpose, RiskClock, Seq, SignalInputs, WriterEpoch,
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
    local_holds: BTreeSet<LocalHold>,
    switch: Option<crate::types::Initiator>,
    awaiting_reconciliation: bool,
    reconciled: bool,
    submission_unreconciled: bool,
    pending_approvals: BTreeMap<EventId, PendingApproval>,
    outstanding: BTreeMap<EventId, Outstanding>,
    bodies: BTreeMap<EventId, IntentBody>,
    outputs: BTreeMap<String, BTreeMap<InstrumentId, ModelOutput>>,
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
            local_holds: BTreeSet::new(),
            switch: None,
            awaiting_reconciliation: false,
            reconciled: false,
            submission_unreconciled: false,
            pending_approvals: BTreeMap::new(),
            outstanding: BTreeMap::new(),
            bodies: BTreeMap::new(),
            outputs: BTreeMap::new(),
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
        let mut mode = self.copied_mode;
        if self.awaiting_reconciliation {
            mode = mode.max(LocalHold::AwaitingReconciliation.mode());
        }
        for hold in &self.local_holds {
            mode = mode.max(hold.mode());
        }
        if let Some(initiator) = self.switch {
            mode = mode.max(initiator.final_mode());
        }
        mode
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

    /// The same maximum as [`Self::effective_mode`] without the startup hold, which governs new
    /// proposals alone (DEC-131 item 22).
    fn rehand_mode(&self) -> Mode {
        let mut mode = self.copied_mode;
        for hold in &self.local_holds {
            if !matches!(hold, LocalHold::AwaitingReconciliation) {
                mode = mode.max(hold.mode());
            }
        }
        if let Some(initiator) = self.switch {
            mode = mode.max(initiator.final_mode());
        }
        mode
    }

    /// Whether the broker's truth is known: a clean `ReconciliationRun` has been folded, no
    /// submission has happened since it, and nothing is still outstanding — an outstanding intent
    /// being exactly the case where the runtime cannot know what the broker did with it (trading
    /// spec §11, DEC-131 item 13). A submission after the last clean run is what "a reconciliation
    /// at or after the last observed submission" comes to on a single ordered stream.
    fn reconciliation_known(&self) -> bool {
        self.reconciled && !self.submission_unreconciled && self.outstanding.is_empty()
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
        matches!(kind, Some("agent" | "acct" | "ctl" | "clock"))
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
        self.awaiting_reconciliation = !self.reconciliation_known();
    }

    /// Lifts the startup hold once, when the reconciliation it waits for has arrived. Latched
    /// rather than recomputed, so a later proposal cannot re-impose a hold the account stream has
    /// already cleared.
    pub(crate) fn settle_startup_hold(&mut self) {
        if self.awaiting_reconciliation && self.reconciliation_known() {
            self.awaiting_reconciliation = false;
        }
    }

    pub(crate) fn hold(&mut self, hold: LocalHold) {
        self.local_holds.insert(hold);
    }

    pub(crate) fn release(&mut self, hold: LocalHold) {
        self.local_holds.remove(&hold);
    }

    pub(crate) fn switched(&mut self, initiator: crate::types::Initiator) {
        self.switch = Some(initiator);
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

    /// The client order ids a flatten plan is asked to cancel: every intent the fold still holds
    /// live, whether or not the executor has taken it, because an order the executor took and is
    /// working is exactly the one a kill switch must name (trading spec §5.5, DEC-131 item 22).
    pub(crate) fn working_orders(&self) -> Vec<String> {
        self.outstanding.keys().map(|id| id.0.clone()).collect()
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
            state.journaled_mode = mode_field(event)?;
            match payload::str_of(&event.payload, "reason") {
                Some(payload::REASON_OWNER_PAUSE) => {
                    state.local_holds.insert(LocalHold::OwnerPaused);
                }
                Some(payload::REASON_OWNER_RESUME) => {
                    state.local_holds.remove(&LocalHold::OwnerPaused);
                }
                Some(payload::REASON_OWNER_STOP) => {
                    state.local_holds.insert(LocalHold::Stopped);
                }
                _ => {}
            }
        }
        "KillSwitchActivated" => {
            let initiator = payload::str_of(&event.payload, "initiator")
                .and_then(payload::initiator_from)
                .ok_or_else(|| payload::non_canonical("initiator"))?;
            state.switched(initiator);
        }
        "IntentProposed" => {
            let purpose = payload::str_of(&event.payload, "purpose")
                .and_then(payload::purpose_from)
                .ok_or_else(|| payload::non_canonical("purpose"))?;
            state
                .bodies
                .insert(event.event_id.clone(), order_body(&event.payload, purpose)?);
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
            state
                .pending_approvals
                .insert(event.event_id.clone(), requested(&event.payload)?);
        }
        "ApprovalResponded" | "ApprovalTimedOut" | "ApprovalCanceled" => {
            if let Some(approval) = payload::str_of(&event.payload, "approval") {
                let approval = EventId(approval.to_owned());
                state.pending_approvals.remove(&approval);
            }
        }
        "ModelOutputRecorded" => {
            let output = payload::model_output_of(&event.payload)?;
            state
                .outputs
                .entry(output.model.clone())
                .or_default()
                .insert(output.instrument.clone(), output);
        }
        "AgentModeApplied" => state.copied_mode = mode_field(event)?,
        "ReconciliationRun" => {
            if matches!(payload::str_of(&event.payload, "result"), Some("clean")) {
                state.reconciled = true;
                state.submission_unreconciled = false;
            }
        }
        "OrderSubmitted" => state.submission_unreconciled = true,
        "IntentReceived" => {
            if let Some(intent) = payload::str_of(&event.payload, "intent_id") {
                let intent = EventId(intent.to_owned());
                if let Some(live) = state.outstanding.get_mut(&intent) {
                    live.handoff = Handoff::Taken;
                }
            }
        }
        "StreamOpened"
        | "GateDecided"
        | "OrderAbandoned"
        | "ObservationRecorded"
        | "ModelInvocationRecorded"
        | "DecisionMade"
        | "ApprovalDelivered"
        | "OwnerExitRequested"
        | "ThesisProposed"
        | "ThesisRevised"
        | "OrderStateChanged"
        | "BrokerExchangeRecorded"
        | "FillApplied"
        | "LateFillApplied"
        | "FeesCharged"
        | "MarkUpdated"
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
        | "OwnerAcknowledged"
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

fn mode_field(event: &FoldedEvent) -> Result<Mode, RuntimeError> {
    payload::str_of(&event.payload, "to")
        .and_then(payload::mode_from)
        .ok_or_else(|| payload::non_canonical("to"))
}

/// The order an `IntentProposed` records, rebuilt so that `Input::Started` can hand it again
/// without proposing it again (journal spec §5.2).
fn order_body(
    payload: &mandate_canon::Value,
    purpose: Purpose,
) -> Result<IntentBody, RuntimeError> {
    let instrument = payload::str_of(payload, "instrument")
        .ok_or_else(|| payload::non_canonical("instrument"))?;
    let side = payload::str_of(payload, "side")
        .and_then(payload::side_from)
        .ok_or_else(|| payload::non_canonical("side"))?;
    let qty = payload::str_of(payload, "qty").ok_or_else(|| payload::non_canonical("qty"))?;
    let limit = payload::str_of(payload, "limit").ok_or_else(|| payload::non_canonical("limit"))?;
    Ok(IntentBody::Order {
        instrument: payload::instrument_of(instrument)?,
        side,
        qty: payload::qty_of(qty)?,
        limit: payload::price_of(limit)?,
        purpose,
    })
}

/// What an `ApprovalRequested` binds: the quantity, the limit price, and the mandate version the
/// proposal was made under, plus the deadline the `skip` timeout fires on (mandate spec §6.4).
fn requested(payload: &mandate_canon::Value) -> Result<PendingApproval, RuntimeError> {
    let instrument = payload::str_of(payload, "instrument")
        .ok_or_else(|| payload::non_canonical("instrument"))?;
    let version = payload::str_of(payload, "mandate_version")
        .ok_or_else(|| payload::non_canonical("mandate_version"))?;
    let deadline =
        payload::clock_of(payload, "deadline").ok_or_else(|| payload::non_canonical("deadline"))?;
    let purpose = payload::str_of(payload, "purpose")
        .and_then(payload::purpose_from)
        .ok_or_else(|| payload::non_canonical("purpose"))?;
    Ok(PendingApproval {
        instrument: payload::instrument_of(instrument)?,
        mandate_version: version.to_owned(),
        deadline,
        adds_risk: purpose.adds_risk(),
    })
}
