//! The live step: the only producer of effects.

use std::collections::BTreeSet;

use mandate_canon::Value;

use crate::error::RuntimeError;
use crate::escalation;
use crate::payload;
use crate::ports::{IdGen, Ports};
use crate::state::{RuntimeState, UnresolvedAppend};
use crate::types::{
    Autonomy, Classified, Command, DecisionClip, DryRunVerdict, Effect, EventDraft, EventId,
    ExitOrigin, FlattenRequest, Initiator, Input, IntentBody, IntentHandoff, KillScope,
    LiquidityRead, Mode, ModelOutput, NotificationRef, Observation, OwnerConfirmation, Proposal,
    Purpose, RiskClock, RuntimeAlert, Seq, TimeInForce, TimerId, TimerRequest, WriterEpoch,
};

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
    if matches!(input, Input::Research(_)) {
        return Err(RuntimeError::NotInterpreted {
            what: "a research input outside handle_research".to_owned(),
            story: "E19-3",
        });
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
    let mut shown = None;
    for draft in &doubted.drafts {
        effects.push(Effect::Journal(draft.clone()));
        let body = match draft.event_type.as_str() {
            "IntentProposed" => Some(payload::order_of(&draft.payload)?),
            "OwnerExitRequested" => {
                let confirmation = payload::honoured_confirmation(&draft.payload)?;
                if payload::str_of(&draft.payload, "scope") != Some("instrument") {
                    shown = confirmation;
                }
                None
            }
            "KillSwitchActivated" => {
                let initiator = payload::str_of(&draft.payload, "initiator")
                    .and_then(payload::initiator_from)
                    .ok_or_else(|| payload::non_canonical("initiator"))?;
                Some(IntentBody::Flatten(flattened(
                    state,
                    initiator,
                    shown.take(),
                    ports,
                )))
            }
            _ => None,
        };
        if let Some(body) = body {
            effects.push(Effect::Intent(IntentHandoff {
                intent_id: draft.event_id.clone(),
                body,
                execution: None,
            }));
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
        instrument: None,
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
    if version_applied {
        cancel_approvals(state, batch, payload::REASON_VERSION_APPLIED)?;
    }
    if effective >= Mode::ExitsOnly {
        cancel_approvals(state, batch, payload::REASON_MODE_TIGHTENED)?;
    }
    match input {
        Input::Journal(event) if escalation::is_owner_input(event) => {
            escalation::owner_input(state, event, ports, batch)
        }
        Input::Tick(at) => ticked(state, *at, ports, batch),
        Input::Observation(observation) => {
            batch.journal("ObservationRecorded", None, observed(observation)?)?;
            Ok(())
        }
        Input::ModelOutput(output) => {
            batch.journal("ModelOutputRecorded", None, modelled(output)?)?;
            Ok(())
        }
        Input::Journal(_) | Input::Started(_) | Input::Command(_) | Input::Research(_) => Ok(()),
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
                execution: None,
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
            execution: None,
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
            lifecycle_change(state, payload::REASON_OWNER_PAUSE, lifecycle, None, batch)?;
            cancel_approvals(state, batch, payload::REASON_OWNER_PAUSE)
        }
        Command::Resume => {
            let lifecycle = match state.lifecycle() {
                Mode::Stopped => Mode::Stopped,
                _ => Mode::Normal,
            };
            lifecycle_change(state, payload::REASON_OWNER_RESUME, lifecycle, None, batch)
        }
        Command::Stop => {
            lifecycle_change(
                state,
                payload::REASON_OWNER_STOP,
                Mode::Stopped,
                None,
                batch,
            )?;
            cancel_approvals(state, batch, payload::REASON_OWNER_STOP)
        }
        Command::KillSwitch {
            scope,
            initiator,
            confirmation,
        } => switched(
            state,
            &Switching {
                scope,
                initiator: *initiator,
                confirmation: confirmation.as_ref(),
                step_up: None,
                user: None,
                cause: None,
            },
            ports,
            batch,
        ),
        Command::OwnerExit { .. } => Err(RuntimeError::NotInterpreted {
            what: "an owner exit of one instrument".to_owned(),
            story: "E7-2",
        }),
    }
}

/// One kill switch as [`switched`] applies it. `step_up` is the status and evidence the owner's
/// `OwnerExitRequested` records when the switch came from the control stream (journal spec §9.1),
/// and `cause` the `OwnerCommandIssued` every copy names (rule 16); both are `None` for a switch
/// handed in as an [`Input::Command`].
pub(crate) struct Switching<'s> {
    pub(crate) scope: &'s KillScope,
    pub(crate) initiator: Initiator,
    pub(crate) confirmation: Option<&'s OwnerConfirmation>,
    pub(crate) step_up: Option<(&'static str, &'s Value)>,
    /// The owner who issued a control-stream switch, recorded whether or not a bid was confirmed
    /// (journal spec §9 rule 12).
    pub(crate) user: Option<&'s str>,
    pub(crate) cause: Option<EventId>,
}

/// The runtime's half of a kill switch, in the order trading-domain spec §5.5 fixes: the final mode
/// first, then the owner's instruction, then the switch itself, then every pending approval
/// cancelled with its timer disarmed, then exactly one agent-scoped flatten handed to the sink. The
/// runtime computes no plan, cancels nothing, and sells nothing (`AGENTS.md` rule 13).
pub(crate) fn switched(
    state: &RuntimeState,
    switching: &Switching<'_>,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let Switching {
        scope,
        initiator,
        confirmation,
        step_up,
        user,
        cause,
    } = switching;
    if !state.deployment().in_scope(scope) {
        return Ok(());
    }
    let mode_event = mode_change(
        state,
        cause.as_ref(),
        payload::REASON_KILL_SWITCH,
        state.effective_mode().max(initiator.final_mode()),
        state.lifecycle(),
        batch,
    )?;
    if matches!(initiator, Initiator::Owner) {
        batch.journal(
            "OwnerExitRequested",
            cause.clone(),
            owner_exit(scope, *confirmation, *step_up, *user)?,
        )?;
    }
    let switch = batch.journal(
        "KillSwitchActivated",
        cause.clone(),
        activated(scope, *initiator, mode_event.as_ref())?,
    )?;
    cancel_approvals(state, batch, payload::REASON_KILL_SWITCH)?;
    batch.hand(IntentHandoff {
        intent_id: switch,
        body: IntentBody::Flatten(flattened(state, *initiator, confirmation.cloned(), ports)),
        execution: None,
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
pub(crate) fn lifecycle_change(
    state: &RuntimeState,
    reason: &'static str,
    lifecycle: Mode,
    cause: Option<EventId>,
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
    batch.journal("AgentModeChanged", cause, body)?;
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
) -> Result<Option<EventId>, RuntimeError> {
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
        return batch
            .journal("AgentModeChanged", origin.cloned(), body)
            .map(Some);
    }
    Ok(None)
}

/// An approval that outlives a tightening is an order after the stop, so the runtime cancels every
/// pending approval and disarms its deadline in the same list (mandate spec §5.9, DEC-131 items 11
/// and 23). The runtime is the agent stream's single writer, so the runtime is what journals this.
pub(crate) fn cancel_approvals(
    state: &RuntimeState,
    batch: &mut Batch<'_>,
    reason: &str,
) -> Result<(), RuntimeError> {
    for approval in state.pending_approvals().keys() {
        if batch.resolved.contains(approval) {
            continue;
        }
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

/// `on_timeout` is always `skip`: a deadline reached journals `ApprovalTimedOut`, caused by the ask
/// it ends (journal spec §9.7 rule 135), disarms its timer, and acts on nothing (mandate spec §6.4).
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
            batch.journal("ApprovalTimedOut", Some(approval.clone()), body)?;
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
    let verdict = dry_run(state, ports, &proposal, now);
    let classified = overlaid(ports.plan.classify(ports.view, &proposal), &verdict);
    let (verdict, next) = next(state, &proposal, verdict, &classified, now, ports.view);
    let decision = batch.journal(
        "DecisionMade",
        None,
        decided(&proposal, &inputs, &verdict, &classified)?,
    )?;
    match &verdict {
        DryRunVerdict::Deny { .. } | DryRunVerdict::DenyOnFigures { .. } => {
            batch.notify(NotificationRef {
                subject_event: decision,
                kind: RuntimeAlert::DecisionSkipped,
            });
            Ok(())
        }
        DryRunVerdict::Allow | DryRunVerdict::AllowDeferring { .. } => {
            allowed(&proposal, next, ports, decision, batch)
        }
    }
}

/// The policy overlay's label (mandate spec §6.2 step 5c), which a decision the overlay changed
/// carries.
const POLICY_OVERLAY: &str = "policy_overlay";

/// `classified`, unless the dry run deferred a check: then an `auto` becomes an `ask` labelled
/// `policy_overlay`, so an opening whose checks the dry run could not all see is never proposed
/// without the owner (DEC-2378 item 11 condition 4, DEC-2388 item 12). An `ask` or a `deny` keeps
/// its own label. [`settled`] leaves a deferral only on an opening or an increase, so no exit is
/// ever asked (`AGENTS.md` rule 13).
pub(crate) fn overlaid(classified: Classified, verdict: &DryRunVerdict) -> Classified {
    match (verdict, classified.autonomy) {
        (DryRunVerdict::AllowDeferring { .. }, Autonomy::Auto) => Classified {
            autonomy: Autonomy::Ask,
            decided_by: Some(POLICY_OVERLAY.to_owned()),
        },
        _ => classified,
    }
}

/// The gate's `verdict` as the journal can record it for a proposal of `purpose` (journal spec
/// v0.85 rules 269 and 270): an exit defers nothing and carries no figures, so a deferring allow
/// of one is an allow and a denial on figures a denial; and an allow that left nothing to the
/// binding gate, having evaluated both liquidity checks on its figures, is an allow, written at
/// version 1 as any other.
fn settled(verdict: DryRunVerdict, purpose: Purpose) -> DryRunVerdict {
    match verdict {
        DryRunVerdict::AllowDeferring { .. } if !purpose.adds_risk() => DryRunVerdict::Allow,
        DryRunVerdict::DenyOnFigures { reason_code, .. } if !purpose.adds_risk() => {
            DryRunVerdict::Deny { reason_code }
        }
        DryRunVerdict::AllowDeferring {
            deferred,
            liquidity: Some(_),
        } if deferred.is_empty() => DryRunVerdict::Allow,
        verdict => verdict,
    }
}

/// The names `deferred_to_binding` carries for `verdict`, in byte order (rule 269): its deferrals,
/// and the liquidity floor and participation when no figures were evaluated (rule 270).
fn deferred_names(verdict: &DryRunVerdict) -> Vec<&'static str> {
    let mut names: Vec<&'static str> = match verdict {
        DryRunVerdict::AllowDeferring {
            deferred,
            liquidity,
        } => deferred
            .iter()
            .map(|deferral| deferral.name())
            .chain(
                liquidity
                    .is_none()
                    .then_some(["liquidity_floor", "participation"])
                    .into_iter()
                    .flatten(),
            )
            .collect(),
        DryRunVerdict::Allow | DryRunVerdict::Deny { .. } | DryRunVerdict::DenyOnFigures { .. } => {
            Vec::new()
        }
    };
    names.sort_by_key(|name| name.as_bytes());
    names.dedup();
    names
}

/// Version 3's `liquidity`: the four figures as decimals and the clock they were read at.
fn liquidity_value(read: &LiquidityRead) -> Result<Value, RuntimeError> {
    payload::object(vec![
        ("prior_close", payload::text(&read.prior_close.to_string())),
        (
            "median_dollar_volume_20d",
            payload::text(&read.median_dollar_volume_20d.to_string()),
        ),
        ("adv_20d", payload::text(&read.adv_20d.to_string())),
        (
            "trailing_5m_volume",
            payload::text(&read.trailing_5m_volume.to_string()),
        ),
        ("read_at", payload::text(&read.read_at.to_string())),
    ])
}

/// The dry run of one proposal at `now`. An opening or increase the connection's checks do not
/// cover is denied `account_restricted` before the gate is asked, as trading spec §7.3 row 5
/// denies one on a connection that is not usable, and one in an instrument where an opening this
/// runtime proposed still works is denied `working_order_limit` (trading spec §5.3 rule 6,
/// DEC-1150 item 5); every other proposal, and every exit whatever either says, is the gate's to
/// judge (`AGENTS.md` rule 13, DEC-969 item 4). An opening the gate allows with checks deferred but
/// for which the builder produced no plan is denied `plan_required`: no approval request can both
/// bind no plan and name the deferrals, so it is refused, journaled and notified rather than left
/// unasked (DEC-2810).
pub(crate) fn dry_run(
    state: &RuntimeState,
    ports: &Ports<'_>,
    proposal: &Proposal,
    now: RiskClock,
) -> DryRunVerdict {
    if proposal.purpose.adds_risk() && !state.connection_admits_openings(now) {
        return DryRunVerdict::Deny {
            reason_code: payload::ACCOUNT_RESTRICTED.to_owned(),
        };
    }
    if proposal.purpose.adds_risk() && state.opening_works_in(&proposal.instrument) {
        return DryRunVerdict::Deny {
            reason_code: payload::WORKING_ORDER_LIMIT.to_owned(),
        };
    }
    match settled(ports.gate.check(proposal), proposal.purpose) {
        DryRunVerdict::AllowDeferring { .. } if proposal.execution.is_none() => {
            DryRunVerdict::Deny {
                reason_code: payload::PLAN_REQUIRED.to_owned(),
            }
        }
        verdict => verdict,
    }
}

/// What an allowed decision does next (mandate spec §6): ask with the request built in full,
/// propose and hand off, or nothing, the refusal its `DecisionMade` already records.
pub(crate) enum Next {
    Ask(Box<escalation::Prepared>),
    Propose,
    Nothing,
}

/// The dry run's `verdict` and what follows it under `classified` (mandate spec §6):
/// - an allowed opening or increase classified `ask` is asked with its request built in full
///   before its decision is journaled, or, when the request cannot be built, the verdict becomes
///   the dry run's denial with the reason, which the decision records and notifies (DEC-2814);
/// - an `auto`, or an `ask` of a purpose that adds no risk, is proposed, because reducing risk never
///   needs approval and an exit is never asked (`AGENTS.md` rules 2 and 13, DEC-278 item 1);
/// - a `deny`, or any denied verdict, does nothing more.
fn next(
    state: &RuntimeState,
    proposal: &Proposal,
    verdict: DryRunVerdict,
    classified: &Classified,
    now: RiskClock,
    view: &crate::types::MandateView,
) -> (DryRunVerdict, Next) {
    if !matches!(
        verdict,
        DryRunVerdict::Allow | DryRunVerdict::AllowDeferring { .. }
    ) {
        return (verdict, Next::Nothing);
    }
    match classified.autonomy {
        Autonomy::Ask if proposal.purpose.adds_risk() => match escalation::prepared(
            state,
            proposal,
            classified.decided_by.as_deref(),
            &deferred_names(&verdict),
            now,
            view,
        ) {
            Ok(prepared) => (verdict, Next::Ask(Box::new(prepared))),
            Err(reason_code) => (
                DryRunVerdict::Deny {
                    reason_code: reason_code.to_owned(),
                },
                Next::Nothing,
            ),
        },
        Autonomy::Auto | Autonomy::Ask => (verdict, Next::Propose),
        Autonomy::Deny => (verdict, Next::Nothing),
    }
}

/// Carries out an allowed decision's [`Next`]: the ASK's request, its delivery, deadline and
/// notification; or the proposal's `IntentProposed` and hand-off; or nothing.
fn allowed(
    proposal: &Proposal,
    next: Next,
    ports: &Ports<'_>,
    decision: EventId,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    match next {
        Next::Ask(prepared) => {
            escalation::ask(*prepared, ports.view.approval.timeout_s, decision, batch)
        }
        Next::Propose => {
            let intent = batch.journal("IntentProposed", Some(decision), proposed(proposal)?)?;
            batch.hand(IntentHandoff {
                intent_id: intent,
                body: IntentBody::Order {
                    instrument: proposal.instrument.clone(),
                    side: proposal.side,
                    qty: proposal.qty,
                    limit: Some(proposal.limit),
                    purpose: proposal.purpose,
                },
                execution: proposal.execution,
            });
            Ok(())
        }
        Next::Nothing => Ok(()),
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

pub(crate) fn proposed(proposal: &Proposal) -> Result<Value, RuntimeError> {
    payload::object(vec![
        ("instrument_id", payload::text(proposal.instrument.as_str())),
        ("side", payload::text(payload::side_name(proposal.side))),
        ("type", payload::text("limit")),
        ("tif", payload::text(tif_name(tif_of(proposal)))),
        ("qty", payload::text(&proposal.qty.to_string())),
        ("limit_price", payload::text(&proposal.limit.to_string())),
        (
            "purpose",
            payload::text(payload::purpose_name(proposal.purpose)),
        ),
    ])
}

fn decided(
    proposal: &Proposal,
    inputs: &crate::types::SignalInputs,
    verdict: &DryRunVerdict,
    classified: &Classified,
) -> Result<Value, RuntimeError> {
    let (dry_run, reason_code) = match verdict {
        DryRunVerdict::Allow | DryRunVerdict::AllowDeferring { .. } => ("allow", Value::Null),
        DryRunVerdict::Deny { reason_code } | DryRunVerdict::DenyOnFigures { reason_code, .. } => {
            ("deny", payload::text(reason_code))
        }
    };
    let allowed = matches!(
        verdict,
        DryRunVerdict::Allow | DryRunVerdict::AllowDeferring { .. }
    );
    let classification = match classified.autonomy {
        Autonomy::Auto => "auto",
        Autonomy::Ask => "ask",
        Autonomy::Deny => "deny",
    };
    let classified_value = if allowed {
        payload::text(classification)
    } else {
        Value::Null
    };
    let decided_by = if allowed {
        classified
            .decided_by
            .as_deref()
            .map_or(Value::Null, payload::text)
    } else {
        Value::Null
    };
    let outputs_used = proposal
        .outputs_used
        .iter()
        .map(|model| {
            inputs
                .output_events
                .get(model)
                .and_then(|by_instrument| by_instrument.get(&proposal.instrument))
                .map(|event| payload::text(&event.0))
                .ok_or_else(|| payload::non_canonical("outputs_used"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let model_weights = proposal
        .model_weights
        .iter()
        .map(|(key, value)| {
            payload::object(vec![("key", payload::text(key)), ("value", value.clone())])
        })
        .collect::<Result<Vec<_>, _>>()?;
    let body = payload::object(vec![
        ("instrument_id", payload::text(proposal.instrument.as_str())),
        ("side", payload::text(payload::side_name(proposal.side))),
        ("type", payload::text("limit")),
        ("tif", payload::text(tif_name(tif_of(proposal)))),
        ("qty", payload::text(&proposal.qty.to_string())),
        ("limit_price", payload::text(&proposal.limit.to_string())),
        (
            "purpose",
            payload::text(payload::purpose_name(proposal.purpose)),
        ),
        (
            "exit_origin",
            proposal.exit_origin.map_or(Value::Null, |origin| {
                payload::text(exit_origin_name(origin))
            }),
        ),
        (
            "exit_conviction",
            proposal.exit_conviction.clone().unwrap_or(Value::Null),
        ),
        (
            "buy_conviction",
            proposal.buy_conviction.clone().unwrap_or(Value::Null),
        ),
        ("combined_score", proposal.combined_score.clone()),
        ("outputs_used", Value::Array(outputs_used)),
        ("model_weights", Value::Array(model_weights)),
        (
            "clips_applied",
            Value::Array(
                proposal
                    .clips_applied
                    .iter()
                    .map(|clip| payload::text(clip_name(*clip)))
                    .collect(),
            ),
        ),
        ("dry_run", payload::text(dry_run)),
        ("reason_code", reason_code),
        ("autonomy", classified_value),
        ("ask_suppressed", Value::Null),
        ("decided_by", decided_by),
        ("delegation_id", Value::Null),
        ("requested_by", payload::text("agent")),
        ("client_id", Value::Null),
    ])?;
    let read = match verdict {
        DryRunVerdict::AllowDeferring { liquidity, .. } => liquidity.as_ref(),
        DryRunVerdict::DenyOnFigures { liquidity, .. } => Some(liquidity),
        DryRunVerdict::Allow | DryRunVerdict::Deny { .. } => None,
    };
    let version_3 = match verdict {
        DryRunVerdict::AllowDeferring { .. } | DryRunVerdict::DenyOnFigures { .. } => true,
        DryRunVerdict::Allow | DryRunVerdict::Deny { .. } => false,
    };
    if !version_3 {
        return Ok(body);
    }
    let Value::Object(mut members) = body else {
        return Err(payload::non_canonical("payload"));
    };
    let deferred = Value::Array(
        deferred_names(verdict)
            .into_iter()
            .map(payload::text)
            .collect(),
    );
    let liquidity = read.map_or(Ok(Value::Null), liquidity_value)?;
    for (name, value) in [("deferred_to_binding", deferred), ("liquidity", liquidity)] {
        members.insert(
            mandate_canon::Key::new(name).map_err(|_| payload::non_canonical(name))?,
            value,
        );
    }
    Ok(Value::Object(members))
}

fn tif_of(proposal: &Proposal) -> TimeInForce {
    proposal.execution.map_or_else(
        || match proposal.asset_class {
            mandate_accounting::AssetClass::UsEquity => TimeInForce::Day,
            mandate_accounting::AssetClass::Crypto => TimeInForce::Gtc,
        },
        |execution| execution.tif,
    )
}

pub(crate) fn tif_name(tif: TimeInForce) -> &'static str {
    match tif {
        TimeInForce::Day => "day",
        TimeInForce::Gtc => "gtc",
        TimeInForce::Ioc => "ioc",
    }
}

fn exit_origin_name(origin: ExitOrigin) -> &'static str {
    match origin {
        ExitOrigin::Signal => "signal",
        ExitOrigin::GoalCompletion => "goal_completion",
        ExitOrigin::RemovedInstrument => "removed_instrument",
    }
}

fn clip_name(clip: DecisionClip) -> &'static str {
    match clip {
        DecisionClip::MaxOrderUsd => "max_order_usd",
        DecisionClip::PositionCap => "position_cap",
        DecisionClip::GrossExposureCap => "gross_exposure_cap",
        DecisionClip::TargetQty => "target_qty",
        DecisionClip::MaxSpendUsd => "max_spend_usd",
        DecisionClip::MaxAvgPrice => "max_avg_price",
    }
}

/// The owner's instruction, without which the executor cannot price an exit outside the regular
/// session (journal spec §9, mandate spec §5.10, trading-domain spec §5.5). It is recorded whether
/// or not the owner confirmed a bid, because `confirmed: false` is what makes equity sells wait.
fn owner_exit(
    scope: &KillScope,
    confirmation: Option<&OwnerConfirmation>,
    step_up: Option<(&'static str, &Value)>,
    issued_by: Option<&str>,
) -> Result<Value, RuntimeError> {
    let (kind, subject) = match scope {
        KillScope::Agent(agent) => ("agent", agent.0.clone()),
        KillScope::Connection(connection) => ("connection", connection.0.clone()),
        KillScope::Workspace(workspace) => ("workspace", workspace.0.clone()),
    };
    let shown = |member: fn(&OwnerConfirmation) -> String| {
        confirmation.map_or(Value::Null, |confirmed| payload::text(&member(confirmed)))
    };
    let user = issued_by
        .or(confirmation.map(|confirmed| confirmed.user.as_str()))
        .unwrap_or_default();
    let assertion = confirmation.map_or("", |confirmed| confirmed.step_up.as_str());
    let mut members = vec![
        ("scope", payload::text(kind)),
        ("subject", payload::text(&subject)),
        ("bid", shown(|c| c.bid.to_string())),
        ("bid_size", shown(|c| c.bid_size.to_string())),
        ("floor", shown(|c| c.floor.to_string())),
        ("user", payload::text(user)),
        (
            "step_up",
            step_up.map_or_else(|| payload::text(assertion), |(_, e)| e.clone()),
        ),
        ("confirmed", Value::Bool(confirmation.is_some())),
    ];
    if let Some((status, _)) = step_up {
        members.push(("step_up_status", payload::text(status)));
    }
    payload::object(members)
}

/// `mode_event` names the `AgentModeChanged` this switch wrote, so the fold can tell the executor's
/// copy of **this** switch's mode from any other copy on the account stream (journal spec §2). It is
/// `Null` when the switch changed no mode, in which case no copy can confirm it.
fn activated(
    scope: &KillScope,
    initiator: Initiator,
    mode_event: Option<&EventId>,
) -> Result<Value, RuntimeError> {
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
        (
            "mode_event",
            match mode_event {
                Some(event) => payload::text(&event.0),
                None => Value::Null,
            },
        ),
    ])
}

/// The `ObservationRecorded` payload of journal spec §9.1: the closed schema's four members, with the
/// cut-off as a §4.7 timestamp and the data as the stored artifact's digest (DEC-503 item 7). An
/// empty `source`, which §9.1's `text` refuses, is refused here rather than at append, where the batch
/// would go into doubt and hold every later input, the kill switch included, until restart.
pub(crate) fn observed(observation: &Observation) -> Result<Value, RuntimeError> {
    if observation.source.is_empty() {
        return Err(payload::non_canonical("source"));
    }
    payload::object(vec![
        ("source", payload::text(&observation.source)),
        (
            "instrument_id",
            observation
                .instrument_id
                .as_ref()
                .map_or(Value::Null, |instrument| payload::text(instrument.as_str())),
        ),
        ("as_of", payload::stamp(observation.as_of, "as_of")?),
        (
            "data_ref",
            payload::text(&format!("sha256:{}", observation.data_ref)),
        ),
    ])
}

fn modelled(output: &ModelOutput) -> Result<Value, RuntimeError> {
    payload::model_output(output)
}

/// One step's effect list under construction. Ids are derived from the epoch, the head the batch is
/// built against, and the draft's ordinal in the batch, so a retry re-derives them (journal §5.1).
pub(crate) struct Batch<'a> {
    ids: &'a dyn IdGen,
    epoch: WriterEpoch,
    head: Seq,
    ordinal: u32,
    pub(crate) drafts: Vec<EventDraft>,
    pub(crate) effects: Vec<Effect>,
    /// The approvals this batch has already cancelled, so one approval cannot be both cancelled and
    /// timed out by a single step: two records of one removal, where the journal should carry one.
    pub(crate) resolved: BTreeSet<EventId>,
}

impl<'a> Batch<'a> {
    pub(crate) fn new(ids: &'a dyn IdGen, epoch: WriterEpoch, head: Seq) -> Self {
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

    pub(crate) fn journal(
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

    /// An id derived at `ordinal` of this batch's head, for an id that is not a draft's own (a run's,
    /// a call's, a thesis's). The caller takes ordinals far above any draft's, so the two never
    /// meet.
    pub(crate) fn id_at(&self, ordinal: u32) -> EventId {
        self.ids.event_id(self.epoch, self.head, ordinal)
    }

    /// The head this batch is built against.
    pub(crate) fn head(&self) -> Seq {
        self.head
    }

    pub(crate) fn hand(&mut self, handoff: IntentHandoff) {
        self.effects.push(Effect::Intent(handoff));
    }

    pub(crate) fn timer(&mut self, request: TimerRequest) {
        self.effects.push(Effect::Timer(request));
    }

    pub(crate) fn notify(&mut self, reference: NotificationRef) {
        self.effects.push(Effect::Notify(reference));
    }

    /// An approval request's opaque notification, which follows the request's draft in the list
    /// (`AGENTS.md` rules 5 and 6).
    pub(crate) fn notify_approval(&mut self, notification: mandate_approval::Notification) {
        self.effects.push(Effect::NotifyApproval(notification));
    }
}

#[cfg(test)]
mod tests {
    use mandate_accounting::InstrumentId;
    use mandate_canon::{Digest, Value};
    use mandate_num::{Conviction, Unit};

    use super::{clip_name, exit_origin_name, modelled};
    use crate::payload::model_output_of;
    use crate::{
        DecisionClip, EventId, ExitOrigin, ModelDirection, ModelOutput, ModelOutputIgnored,
        RiskClock,
    };

    fn model_output_fixture() -> Result<ModelOutput, String> {
        Ok(ModelOutput {
            model_id: "quant.ma_crossover".to_owned(),
            model_version: "1.0.0".to_owned(),
            content_hash: Digest::from_hex(
                "5555555555555555555555555555555555555555555555555555555555555555",
            )
            .ok_or("content hash")?,
            instrument_id: InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
                .map_err(|error| error.to_string())?,
            as_of: RiskClock::from_secs(1_790_356_800),
            expires_at: RiskClock::from_secs(1_790_443_200),
            direction: ModelDirection::Long,
            conviction: Conviction::parse("1").map_err(|error| error.to_string())?,
            confidence: Unit::ONE,
            horizon_s: 86_400,
            thesis_ref: None,
            evidence: vec![EventId("01J8Z3M0A000000000000000G2".to_owned())],
            invalidation: None,
            thesis_id: None,
            lineage_id: None,
            ignored: Some(ModelOutputIgnored::NotPinned),
        })
    }

    #[test]
    fn model_output_uses_the_closed_journal_shape_and_replays_exactly() -> Result<(), String> {
        let output = model_output_fixture()?;
        let payload = modelled(&output).map_err(|error| error.to_string())?;
        let fields = payload
            .as_object()
            .ok_or("model output object")?
            .keys()
            .map(|key| key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            fields,
            [
                "as_of",
                "confidence",
                "content_hash",
                "conviction",
                "direction",
                "evidence",
                "expires_at",
                "horizon_s",
                "ignored",
                "instrument_id",
                "invalidation",
                "lineage_id",
                "model_id",
                "model_version",
                "thesis_id",
                "thesis_ref",
            ]
        );
        assert_eq!(
            payload.get("model_id").and_then(Value::as_str),
            Some("quant.ma_crossover")
        );
        assert_eq!(
            payload.get("direction").and_then(Value::as_str),
            Some("long")
        );
        assert_eq!(
            payload.get("horizon_s").and_then(Value::as_int),
            Some(86_400)
        );
        assert_eq!(
            payload.get("ignored").and_then(Value::as_str),
            Some("not_pinned")
        );
        assert_eq!(payload.get("content"), None);
        assert_eq!(payload.get("thesis_ref"), Some(&Value::Null));
        assert_eq!(
            model_output_of(&payload).map_err(|error| error.to_string())?,
            output
        );
        Ok(())
    }

    #[test]
    fn model_output_replays_digest_and_every_ignored_reason() -> Result<(), String> {
        let thesis =
            Digest::from_hex("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
                .ok_or("thesis digest")?;
        let cases = [
            (ModelOutputIgnored::NotPinned, "not_pinned"),
            (ModelOutputIgnored::ModelWithdrawn, "model_withdrawn"),
            (ModelOutputIgnored::OutputLimits, "output_limits"),
            (ModelOutputIgnored::NotInUniverse, "not_in_universe"),
            (
                ModelOutputIgnored::DirectionNotAllowed,
                "direction_not_allowed",
            ),
            (ModelOutputIgnored::HorizonMismatch, "horizon_mismatch"),
            (
                ModelOutputIgnored::RevisionWithoutPredecessor,
                "revision_without_predecessor",
            ),
        ];
        for (reason, canonical_name) in cases {
            let mut output = model_output_fixture()?;
            output.thesis_ref = Some(thesis);
            output.ignored = Some(reason);
            let payload = modelled(&output).map_err(|error| error.to_string())?;
            assert_eq!(
                payload.get("thesis_ref").and_then(Value::as_str),
                Some("sha256:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa")
            );
            assert_eq!(
                payload.get("ignored").and_then(Value::as_str),
                Some(canonical_name)
            );
            assert_eq!(
                model_output_of(&payload).map_err(|error| error.to_string())?,
                output
            );
        }
        Ok(())
    }

    #[test]
    fn decision_enum_names_are_exact_canonical_field_values() {
        assert_eq!(exit_origin_name(ExitOrigin::Signal), "signal");
        assert_eq!(
            exit_origin_name(ExitOrigin::GoalCompletion),
            "goal_completion"
        );
        assert_eq!(
            exit_origin_name(ExitOrigin::RemovedInstrument),
            "removed_instrument"
        );
        assert_eq!(clip_name(DecisionClip::MaxOrderUsd), "max_order_usd");
        assert_eq!(clip_name(DecisionClip::PositionCap), "position_cap");
        assert_eq!(
            clip_name(DecisionClip::GrossExposureCap),
            "gross_exposure_cap"
        );
        assert_eq!(clip_name(DecisionClip::TargetQty), "target_qty");
        assert_eq!(clip_name(DecisionClip::MaxSpendUsd), "max_spend_usd");
        assert_eq!(clip_name(DecisionClip::MaxAvgPrice), "max_avg_price");
    }
}
