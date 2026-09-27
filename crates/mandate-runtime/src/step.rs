//! The live step: the only producer of effects.

use std::collections::BTreeSet;

use mandate_canon::Value;

use crate::error::RuntimeError;
use crate::payload;
use crate::ports::{IdGen, Ports};
use crate::state::{RuntimeState, UnresolvedAppend};
use crate::types::{
    ApprovalOutcome, ApprovalVerdict, Autonomy, Command, DryRunVerdict, Effect, EventDraft,
    EventId, FlattenRequest, Initiator, Input, IntentBody, IntentHandoff, KillScope, MandateView,
    Mode, ModelOutput, NotificationRef, Observation, OwnerConfirmation, Proposal, Purpose,
    RiskClock, Seq, TimerId, TimerRequest, WriterEpoch,
};

/// How long an ASKed action waits before the `skip` timeout fires (mandate spec §6.4).
///
/// The mandate's own `autonomy.approval.timeout_s` is an envelope field, and the frozen
/// [`MandateView`] does not carry it yet, so the core uses one conservative window until
/// `mandate-spec` (stream F) widens the view: 300 s sits above DEC-61's `approval_timeout_s ≥ 120`
/// placeholder and well inside the schema's 30 s to 86 400 s range, and a window that is too short
/// only skips an action (`on_timeout` is always `skip`), which never adds risk.
const APPROVAL_WINDOW_S: i64 = 300;

/// One step of the runtime (ADR-0001 ES-06).
///
/// The **only** producer of effects, which is what makes a replay safe: [`crate::fold`] emits
/// nothing, so recovery cannot re-send, and nothing but this function can reach the sink.
///
/// The returned list is ordered and the shell runs it in order, stopping at the first append that is
/// neither `Committed` nor `AlreadyCommitted` and discarding the rest; the next start re-derives
/// from the fold, which is why discarding is safe. Within one list every [`Effect::Intent`] follows
/// the [`Effect::Journal`] that records it — **except for [`Input::Started`]**, whose handoffs
/// re-send intents whose `IntentProposed` committed in an earlier run, so there the rule reads
/// "preceded by the draft that records it, or by a draft the fold saw committed before the call"
/// (DEC-131 item 7).
///
/// `Input::Started` is recovery: its first effect is the startup hold's `AgentModeChanged` when the
/// mode changed, and it never re-journals an intent, an observation, a decision, or an approval.
pub fn handle(
    state: &mut RuntimeState,
    input: Input,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, RuntimeError> {
    if let Input::Started(epoch) = &input {
        return started(state, *epoch, ports);
    }
    if !state.has_started() {
        return Err(RuntimeError::NotStarted);
    }
    if let Some(doubted) = state.unresolved_batch() {
        if doubted.input == input {
            return retried(state, &doubted, ports);
        }
        return Err(RuntimeError::AppendUnresolved {
            head: doubted.head.0,
        });
    }
    let epoch = state.epoch().ok_or(RuntimeError::NotStarted)?;
    let mut batch = Batch::new(ports.ids, epoch, state.agent_head());
    if let Input::Command(command) = &input {
        commanded(state, command, ports, &mut batch)?;
    } else {
        applied(state, &input, ports, &mut batch)?;
    }
    remember(state, &input, &batch);
    Ok(batch.effects)
}

/// The retry of a batch whose append was never answered (journal spec §5.1, DEC-131 item 6). It
/// re-emits the same drafts, so the append answers `AlreadyCommitted` rather than writing a second
/// event, **and re-emits the handoffs those drafts authorise**, so a batch that resolves on the retry
/// still reaches the sink. Emitting the drafts alone would have lost the handoff until some later
/// restart, and for a kill switch's flatten that is the one handoff `AGENTS.md` rule 13 says must
/// always get through. Every handoff here follows its own draft in the list, so write-before-acting
/// holds exactly as it did on the first attempt.
fn retried(
    state: &RuntimeState,
    doubted: &UnresolvedAppend,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, RuntimeError> {
    let mut effects = Vec::new();
    for draft in &doubted.drafts {
        effects.push(Effect::Journal(draft.clone()));
        match draft.event_type.as_str() {
            "IntentProposed" => effects.push(Effect::Intent(IntentHandoff {
                intent_id: draft.event_id.clone(),
                body: payload::order_of(&draft.payload)?,
            })),
            "KillSwitchActivated" => {
                if let Input::Command(Command::KillSwitch {
                    initiator,
                    confirmation,
                    ..
                }) = &doubted.input
                {
                    effects.push(Effect::Intent(IntentHandoff {
                        intent_id: draft.event_id.clone(),
                        body: IntentBody::Flatten(flattened(
                            state,
                            *initiator,
                            confirmation.clone(),
                            ports,
                        )),
                    }));
                }
            }
            _ => {}
        }
    }
    Ok(effects)
}

/// The plan stream G computes for this agent. Deterministic in the folded state and the request, so
/// a retry and a restart ask for and get the same plan (`AGENTS.md` rule 13, family F).
fn flattened(
    state: &RuntimeState,
    initiator: Initiator,
    confirmation: Option<OwnerConfirmation>,
    ports: &Ports<'_>,
) -> crate::types::FlattenPlan {
    ports.flatten.plan(&FlattenRequest {
        initiator,
        confirmation,
        working_orders: state.working_orders(),
    })
}

/// Everything but a command: the effective mode is settled first (tick step 3), then the input
/// itself is journaled or evaluated (steps 2, 4, and 5).
fn applied(
    state: &mut RuntimeState,
    input: &Input,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    state.settle_startup_hold();
    let effective = state.effective_mode();
    let origin = match input {
        Input::Journal(event) => Some(&event.event_id),
        _ => None,
    };
    mode_change(
        state,
        origin,
        payload::REASON_RESTRICTION,
        effective,
        state.lifecycle(),
        batch,
    )?;
    let version_applied = match input {
        Input::Journal(event) => matches!(event.event_type.as_str(), "MandateVersionApplied"),
        _ => false,
    };
    if effective >= Mode::ExitsOnly || version_applied {
        cancel_approvals(state, batch, "rebound")?;
    }
    match input {
        Input::Tick(at) => ticked(state, *at, ports, batch),
        Input::Observation(observation) => {
            batch.journal("ObservationRecorded", None, observed(observation)?)?;
            Ok(())
        }
        Input::ModelOutput(output) => {
            batch.journal("ModelOutputRecorded", None, modelled(output)?)?;
            Ok(())
        }
        Input::ApprovalResponse(outcome) => {
            batch.journal("ApprovalResponded", None, responded(state, outcome)?)?;
            Ok(())
        }
        Input::Journal(_) | Input::Started(_) | Input::Command(_) => Ok(()),
    }
}

/// Recovery (DEC-131 items 2 and 13): the startup hold is journaled first, then the outstanding
/// intents the folded mode still permits are handed again, then every deadline the fold carries is
/// re-armed. Nothing here re-journals an intent, an observation, a decision, or an approval.
fn started(
    state: &mut RuntimeState,
    epoch: WriterEpoch,
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, RuntimeError> {
    if state.has_started() {
        return Err(RuntimeError::AlreadyStarted);
    }
    state.start(epoch);
    state.take_startup_hold();
    let mut batch = Batch::new(ports.ids, epoch, state.agent_head());
    mode_change(
        state,
        None,
        payload::REASON_STARTUP_HOLD,
        state.effective_mode(),
        state.lifecycle(),
        &mut batch,
    )?;
    for live in state.pending_handoffs() {
        if state.permits_rehand(live.purpose)
            && let Some(body) = state.body_of(&live.intent_id)
        {
            batch.hand(IntentHandoff {
                intent_id: live.intent_id.clone(),
                body,
            });
        }
    }
    if let Some(switch) = state.switch()
        && state.permits_rehand(Purpose::Flatten)
    {
        batch.hand(IntentHandoff {
            intent_id: switch.event.clone(),
            body: IntentBody::Flatten(flattened(
                state,
                switch.initiator,
                switch.confirmation.clone(),
                ports,
            )),
        });
    }
    for (approval, pending) in state.pending_approvals() {
        batch.timer(TimerRequest::Arm {
            id: TimerId::ApprovalDeadline(approval.clone()),
            at: pending.deadline,
        });
    }
    Ok(batch.effects)
}

/// A command addressed to this deployment. A kill switch is honoured from folded state alone, in
/// every mode, with no model output, no gate call, and no approval (`AGENTS.md` rule 13).
fn commanded(
    state: &RuntimeState,
    command: &Command,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    match command {
        Command::Pause => {
            let lifecycle = state.lifecycle().max(Mode::Paused);
            lifecycle_change(state, payload::REASON_OWNER_PAUSE, lifecycle, batch)?;
            cancel_approvals(state, batch, payload::REASON_OWNER_PAUSE)
        }
        Command::Resume => {
            let lifecycle = match state.lifecycle() {
                Mode::Stopped => Mode::Stopped,
                _ => Mode::Normal,
            };
            lifecycle_change(state, payload::REASON_OWNER_RESUME, lifecycle, batch)
        }
        Command::Stop => {
            lifecycle_change(state, payload::REASON_OWNER_STOP, Mode::Stopped, batch)?;
            cancel_approvals(state, batch, payload::REASON_OWNER_STOP)
        }
        Command::KillSwitch {
            scope,
            initiator,
            confirmation,
        } => switched(
            state,
            scope,
            *initiator,
            confirmation.as_ref(),
            ports,
            batch,
        ),
        Command::OwnerExit { .. } => Err(RuntimeError::NotInterpreted {
            what: "an owner exit of one instrument".to_owned(),
            story: "E7-2",
        }),
    }
}

/// The runtime's half of a kill switch, in the order trading-domain spec §5.5 fixes: the final mode
/// first, then the owner's instruction, then the switch itself, then every pending approval
/// cancelled with its timer disarmed, then exactly one agent-scoped flatten handed to the sink. The
/// runtime computes no plan, cancels nothing, and sells nothing (`AGENTS.md` rule 13).
fn switched(
    state: &RuntimeState,
    scope: &KillScope,
    initiator: Initiator,
    confirmation: Option<&OwnerConfirmation>,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    if !state.deployment().in_scope(scope) {
        return Ok(());
    }
    mode_change(
        state,
        None,
        payload::REASON_KILL_SWITCH,
        state.effective_mode().max(initiator.final_mode()),
        state.lifecycle(),
        batch,
    )?;
    if matches!(initiator, Initiator::Owner) {
        batch.journal("OwnerExitRequested", None, owner_exit(scope, confirmation)?)?;
    }
    let switch = batch.journal("KillSwitchActivated", None, activated(scope, initiator)?)?;
    cancel_approvals(state, batch, payload::REASON_KILL_SWITCH)?;
    batch.hand(IntentHandoff {
        intent_id: switch,
        body: IntentBody::Flatten(flattened(state, initiator, confirmation.cloned(), ports)),
    });
    Ok(())
}

/// An owner `Pause`, `Resume`, or `Stop` changes the **deployment lifecycle**, which the agent-stream
/// catalogue records nowhere but on an `AgentModeChanged`, so this one is journaled even when the
/// effective mode is unchanged because a stricter restriction is already in force. Without the draft
/// the lifecycle would live only in memory, and a pause taken while the account stream or the startup
/// hold already paused the agent would be lost the moment that other restriction lifted — less strict
/// than the owner asked for. MI-6's "only when it changes" governs the mode *copies*, which are
/// journaled through [`mode_change`] (DEC-131 item 25).
fn lifecycle_change(
    state: &RuntimeState,
    reason: &'static str,
    lifecycle: Mode,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    if lifecycle == state.lifecycle() && state.mode_with(lifecycle) == state.journaled_mode() {
        return Ok(());
    }
    let body = payload::object(vec![
        (
            "from",
            payload::text(payload::mode_name(state.journaled_mode())),
        ),
        (
            "to",
            payload::text(payload::mode_name(state.mode_with(lifecycle))),
        ),
        ("reason", payload::text(reason)),
        ("lifecycle", payload::text(payload::mode_name(lifecycle))),
    ])?;
    batch.journal("AgentModeChanged", None, body)?;
    Ok(())
}

/// `AgentModeChanged` is journaled only when the effective mode changed (mandate spec §5.9, MI-6),
/// and a copy of an account-stream fact cites the event it came from (journal spec §2).
fn mode_change(
    state: &RuntimeState,
    origin: Option<&EventId>,
    reason: &'static str,
    effective: Mode,
    lifecycle: Mode,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    if effective != state.journaled_mode() {
        let body = payload::object(vec![
            (
                "from",
                payload::text(payload::mode_name(state.journaled_mode())),
            ),
            ("to", payload::text(payload::mode_name(effective))),
            ("reason", payload::text(reason)),
            ("lifecycle", payload::text(payload::mode_name(lifecycle))),
        ])?;
        batch.journal("AgentModeChanged", origin.cloned(), body)?;
    }
    Ok(())
}

/// An approval that outlives a tightening is an order after the stop, so the runtime cancels every
/// pending approval and disarms its deadline in the same list (mandate spec §5.9, DEC-131 items 11
/// and 23). The runtime is the agent stream's single writer, so the runtime is what journals this.
fn cancel_approvals(
    state: &RuntimeState,
    batch: &mut Batch<'_>,
    reason: &str,
) -> Result<(), RuntimeError> {
    for approval in state.pending_approvals().keys() {
        let body = payload::object(vec![
            ("approval", payload::text(&approval.0)),
            ("reason", payload::text(reason)),
        ])?;
        batch.journal("ApprovalCanceled", None, body)?;
        batch.resolved.insert(approval.clone());
        batch.timer(TimerRequest::Cancel {
            id: TimerId::ApprovalDeadline(approval.clone()),
        });
    }
    Ok(())
}

/// The evaluation cadence: what is due at this risk-clock second expires, and then one decision is
/// taken. A second the runtime has already evaluated is inert, so waking twice for one second
/// journals nothing (mandate spec §5.2, MI-13, DEC-131 items 5 and 8).
fn ticked(
    state: &mut RuntimeState,
    at: RiskClock,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let now = state.advance(at);
    if state.already_evaluated(now) {
        return Ok(());
    }
    state.mark_evaluated(now);
    expire(state, now, batch)?;
    decide(state, ports, now, batch)
}

/// `on_timeout` is always `skip`: a deadline reached journals `ApprovalTimedOut`, disarms its timer,
/// and acts on nothing (mandate spec §6.4).
fn expire(state: &RuntimeState, now: RiskClock, batch: &mut Batch<'_>) -> Result<(), RuntimeError> {
    for (approval, pending) in state.pending_approvals() {
        if batch.resolved.contains(approval) {
            continue;
        }
        if pending.deadline <= now {
            let body = payload::object(vec![
                ("approval", payload::text(&approval.0)),
                ("on_timeout", payload::text("skip")),
            ])?;
            batch.journal("ApprovalTimedOut", None, body)?;
            batch.timer(TimerRequest::Cancel {
                id: TimerId::ApprovalDeadline(approval.clone()),
            });
        }
    }
    Ok(())
}

/// One evaluation (tick step 5). The order builder proposes, the mode and the mandate's instrument
/// restrictions may narrow the proposal away, the gate's **dry run** may narrow it away again, and
/// only then is a decision journaled and either proposed, asked, or refused. A dry-run allow
/// authorises nothing: the binding gate runs on the account stream (`AGENTS.md` rules 1 and 12).
fn decide(
    state: &RuntimeState,
    ports: &Ports<'_>,
    now: RiskClock,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let inputs = state.signal_inputs(now);
    let Some(proposal) = ports.plan.plan(ports.view, &inputs) else {
        return Ok(());
    };
    if !state.permits(proposal.purpose) {
        return Ok(());
    }
    if proposal.purpose.adds_risk() && state.awaiting_risk_approval() {
        return Ok(());
    }
    if proposal.purpose.adds_risk()
        && ports
            .view
            .restricted_instruments
            .contains(&proposal.instrument)
    {
        return Ok(());
    }
    let verdict = ports.gate.check(&proposal);
    let autonomy = ports.plan.classify(ports.view, &proposal);
    let decision = batch.journal(
        "DecisionMade",
        None,
        decided(&proposal, &verdict, autonomy)?,
    )?;
    match &verdict {
        DryRunVerdict::Deny { .. } => {
            batch.notify(NotificationRef {
                subject_event: decision,
                message_key: "decision_skipped",
            });
            Ok(())
        }
        DryRunVerdict::Allow => allowed(&proposal, autonomy, now, ports.view, decision, batch),
    }
}

/// What the autonomy classification does with a proposal the dry run allowed (mandate spec §6):
/// AUTO proposes and hands off, ASK requests an approval and arms its deadline, DENY records the
/// refusal the `DecisionMade` already carries and stops there.
fn allowed(
    proposal: &Proposal,
    autonomy: Autonomy,
    now: RiskClock,
    view: &MandateView,
    decision: EventId,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    match autonomy {
        Autonomy::Auto => {
            let intent = batch.journal("IntentProposed", Some(decision), proposed(proposal)?)?;
            batch.hand(IntentHandoff {
                intent_id: intent,
                body: IntentBody::Order {
                    instrument: proposal.instrument.clone(),
                    side: proposal.side,
                    qty: proposal.qty,
                    limit: proposal.limit,
                    purpose: proposal.purpose,
                },
            });
            Ok(())
        }
        Autonomy::Ask => {
            let deadline = RiskClock::from_secs(now.secs().saturating_add(APPROVAL_WINDOW_S));
            let approval = batch.journal(
                "ApprovalRequested",
                Some(decision),
                asked(proposal, view, deadline)?,
            )?;
            batch.timer(TimerRequest::Arm {
                id: TimerId::ApprovalDeadline(approval),
                at: deadline,
            });
            Ok(())
        }
        Autonomy::Deny => Ok(()),
    }
}

/// Remembers a batch whose append has not been answered yet, so the only step the runtime takes
/// next is the same one, re-deriving the same ids and letting the append answer `AlreadyCommitted`
/// (journal spec §5.1, DEC-131 item 6).
fn remember(state: &mut RuntimeState, input: &Input, batch: &Batch<'_>) {
    if !batch.drafts.is_empty() {
        state.remember(UnresolvedAppend {
            head: batch.head,
            input: input.clone(),
            drafts: batch.drafts.clone(),
        });
    }
}

fn proposed(proposal: &Proposal) -> Result<Value, RuntimeError> {
    payload::object(vec![
        ("instrument", payload::text(proposal.instrument.as_str())),
        ("side", payload::text(payload::side_name(proposal.side))),
        ("qty", payload::text(&proposal.qty.to_string())),
        ("limit", payload::text(&proposal.limit.to_string())),
        (
            "purpose",
            payload::text(payload::purpose_name(proposal.purpose)),
        ),
    ])
}

/// What an approval binds: the quantity, the limit price, and the mandate version it was proposed
/// under, plus the deadline and the `skip` timeout (mandate spec §6.4).
fn asked(
    proposal: &Proposal,
    view: &MandateView,
    deadline: RiskClock,
) -> Result<Value, RuntimeError> {
    payload::object(vec![
        ("instrument", payload::text(proposal.instrument.as_str())),
        ("qty", payload::text(&proposal.qty.to_string())),
        ("limit", payload::text(&proposal.limit.to_string())),
        (
            "purpose",
            payload::text(payload::purpose_name(proposal.purpose)),
        ),
        ("mandate_version", payload::text(&view.version)),
        ("deadline", payload::seconds(deadline, "deadline")?),
        ("on_timeout", payload::text("skip")),
    ])
}

fn decided(
    proposal: &Proposal,
    verdict: &DryRunVerdict,
    autonomy: Autonomy,
) -> Result<Value, RuntimeError> {
    let (dry_run, reason_code) = match verdict {
        DryRunVerdict::Allow => ("allow", String::new()),
        DryRunVerdict::Deny { reason_code } => ("deny", reason_code.clone()),
    };
    let classification = match autonomy {
        Autonomy::Auto => "auto",
        Autonomy::Ask => "ask",
        Autonomy::Deny => "deny",
    };
    payload::object(vec![
        ("instrument", payload::text(proposal.instrument.as_str())),
        ("side", payload::text(payload::side_name(proposal.side))),
        ("qty", payload::text(&proposal.qty.to_string())),
        ("limit", payload::text(&proposal.limit.to_string())),
        (
            "purpose",
            payload::text(payload::purpose_name(proposal.purpose)),
        ),
        ("dry_run", payload::text(dry_run)),
        ("reason_code", payload::text(&reason_code)),
        ("autonomy", payload::text(classification)),
        ("combined_score", proposal.combined_score.clone()),
    ])
}

/// The owner's instruction, without which the executor cannot price an exit outside the regular
/// session (journal spec §9, mandate spec §5.10, trading-domain spec §5.5). It is recorded whether
/// or not the owner confirmed a bid, because `confirmed: false` is what makes equity sells wait.
fn owner_exit(
    scope: &KillScope,
    confirmation: Option<&OwnerConfirmation>,
) -> Result<Value, RuntimeError> {
    let (kind, subject) = match scope {
        KillScope::Agent(agent) => ("agent", agent.0.clone()),
        KillScope::Connection(connection) => ("connection", connection.0.clone()),
        KillScope::Workspace(workspace) => ("workspace", workspace.0.clone()),
    };
    let shown = confirmation.map(|confirmed| {
        (
            confirmed.bid.to_string(),
            confirmed.bid_size.to_string(),
            confirmed.floor.to_string(),
            confirmed.user.clone(),
            confirmed.step_up.clone(),
        )
    });
    let (bid, bid_size, floor, user, step_up) = shown.unwrap_or_default();
    payload::object(vec![
        ("scope", payload::text(kind)),
        ("subject", payload::text(&subject)),
        ("bid", payload::text(&bid)),
        ("bid_size", payload::text(&bid_size)),
        ("floor", payload::text(&floor)),
        ("user", payload::text(&user)),
        ("step_up", payload::text(&step_up)),
        ("confirmed", Value::Bool(confirmation.is_some())),
    ])
}

fn activated(scope: &KillScope, initiator: Initiator) -> Result<Value, RuntimeError> {
    let (kind, subject) = match scope {
        KillScope::Agent(agent) => ("agent", agent.0.clone()),
        KillScope::Connection(connection) => ("connection", connection.0.clone()),
        KillScope::Workspace(workspace) => ("workspace", workspace.0.clone()),
    };
    payload::object(vec![
        ("scope", payload::text(kind)),
        ("subject", payload::text(&subject)),
        (
            "initiator",
            payload::text(payload::initiator_name(initiator)),
        ),
    ])
}

fn observed(observation: &Observation) -> Result<Value, RuntimeError> {
    let instrument = match &observation.instrument {
        Some(instrument) => payload::text(instrument.as_str()),
        None => Value::Null,
    };
    payload::object(vec![
        ("source", payload::text(&observation.source)),
        ("instrument", instrument),
        ("at", payload::seconds_text(observation.at)),
        ("data", observation.data.clone()),
    ])
}

fn modelled(output: &ModelOutput) -> Result<Value, RuntimeError> {
    payload::object(vec![
        ("model", payload::text(&output.model)),
        ("version", payload::text(&output.version)),
        ("instrument", payload::text(output.instrument.as_str())),
        ("as_of", payload::seconds_text(output.as_of)),
        ("expires_at", payload::seconds_text(output.expires_at)),
        ("content", output.content.clone()),
    ])
}

/// An approver's response. A response for an approval the fold no longer knows — one a kill switch,
/// a tightening, a version change, or the `skip` timeout has already removed — is recorded as
/// refused and acts on nothing (DEC-131 item 11).
fn responded(state: &RuntimeState, outcome: &ApprovalOutcome) -> Result<Value, RuntimeError> {
    let known = state.pending_approvals().contains_key(&outcome.approval);
    let (verdict, responder, step_up) = match &outcome.verdict {
        ApprovalVerdict::Approved { responder, step_up } => {
            ("approved", responder.as_str(), step_up.as_str())
        }
        ApprovalVerdict::Denied { responder } => ("denied", responder.as_str(), ""),
    };
    let result = if known { "recorded" } else { "refused" };
    payload::object(vec![
        ("approval", payload::text(&outcome.approval.0)),
        ("verdict", payload::text(verdict)),
        ("responder", payload::text(responder)),
        ("step_up", payload::text(step_up)),
        ("result", payload::text(result)),
        ("at", payload::seconds_text(outcome.at)),
    ])
}

/// One step's effect list under construction. Ids are derived from the epoch, the head the batch is
/// built against, and the draft's ordinal in the batch, so a retry re-derives them (journal §5.1).
struct Batch<'a> {
    ids: &'a dyn IdGen,
    epoch: WriterEpoch,
    head: Seq,
    ordinal: u32,
    drafts: Vec<EventDraft>,
    effects: Vec<Effect>,
    /// The approvals this batch has already cancelled, so one approval cannot be both cancelled and
    /// timed out by a single step: two records of one removal, where the journal should carry one.
    resolved: BTreeSet<EventId>,
}

impl<'a> Batch<'a> {
    fn new(ids: &'a dyn IdGen, epoch: WriterEpoch, head: Seq) -> Self {
        Self {
            ids,
            epoch,
            head,
            ordinal: 0,
            drafts: Vec::new(),
            effects: Vec::new(),
            resolved: BTreeSet::new(),
        }
    }

    fn journal(
        &mut self,
        event_type: &str,
        causation_id: Option<EventId>,
        body: Value,
    ) -> Result<EventId, RuntimeError> {
        let event_id = self.ids.event_id(self.epoch, self.head, self.ordinal);
        self.ordinal = self.ordinal.saturating_add(1);
        let draft = EventDraft {
            event_id: event_id.clone(),
            event_type: event_type.to_owned(),
            causation_id,
            payload: body,
        };
        self.drafts.push(draft.clone());
        self.effects.push(Effect::Journal(draft));
        Ok(event_id)
    }

    fn hand(&mut self, handoff: IntentHandoff) {
        self.effects.push(Effect::Intent(handoff));
    }

    fn timer(&mut self, request: TimerRequest) {
        self.effects.push(Effect::Timer(request));
    }

    fn notify(&mut self, reference: NotificationRef) {
        self.effects.push(Effect::Notify(reference));
    }
}
