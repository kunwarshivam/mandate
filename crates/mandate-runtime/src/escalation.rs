//! The runtime's half of escalation v0 (M7, backlog E8-1 to E8-3; mandate spec §6.1, §6.4; journal
//! spec §2, §9; DEC-155, DEC-156, DEC-158 option (c), DEC-257, DEC-278).
//!
//! **Asking.** An ASKed `open` or `increase` commits its request, the canonical content object and
//! its hash, its `cli_inbox` delivery, the deadline timer, and one opaque notification, in that
//! order and in one batch, so a notification never names an uncommitted request and every request
//! is grantable from creation (DEC-257 item 9, `AGENTS.md` rules 5 and 6).
//!
//! **Answering.** The owner's answer arrives only as a control-stream `ApprovalResponseSubmitted`,
//! tailed and handed in as `Input::Journal`. `mandate_approval::admit` judges it against the request
//! the fold holds after this step's own cancellations, and the copy, `ApprovalResponded`, records
//! the result whatever it is, and for a grant check 7 judged the quorum it applied (journal spec
//! §9, DEC-488). An admitted grant is re-validated in the same step by
//! `mandate_approval::revalidate`, and only its `act` journals and hands an intent, which is the
//! bound order and nothing else (EI-4). Nothing here can act on a timeout, a refusal, or a skip
//! (`AGENTS.md` rule 3).
//!
//! **Commanding.** An `OwnerCommandIssued` is judged by `mandate_approval`'s step-up rules and
//! copied with its `causation_id`. Pause needs nothing; resume and Stop need evidence fresh when
//! the runtime processes them; a kill switch and an owner exit are judged as the owner committed
//! them and are **never refused**: evidence that does not count withdraws only the out-of-session
//! privilege, and the stop, the flatten, or the exit itself still reaches the executor (`AGENTS.md`
//! rules 2 and 13, the #281 obligation).

use std::num::NonZeroU8;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_approval::{
    Admission, AdmissionContext, ApprovalRef, AskablePurpose, BoundAction, Classification,
    CommandAuthority, ContentHash, Current, DryRun, EvidenceAuthor, EvidenceRef, ExecutionTerms,
    GenericText, KillSwitchAuthority, ModeNow, Notification, OpaqueUser, OwnerCommandKind,
    PolicyOverlay, ProtectionTerms, Refusal, Request, RequestContent, Response, Revalidation,
    SkipReason, StepUp, StepUpRefusal, Verdict, admit, content_hash, content_object, quorum,
    revalidate,
};
use mandate_canon::{Digest, Value};
use mandate_num::{Price, Qty, Signed};

use crate::error::RuntimeError;
use crate::payload;
use crate::ports::Ports;
use crate::state::RuntimeState;
use crate::step::{
    Batch, Switching, cancel_approvals, dry_run, lifecycle_change, overlaid, proposed, switched,
    tif_name,
};
use crate::types::{
    AgentId, Autonomy, ConnectionId, DryRunVerdict, EventId, FlattenLeg, FlattenRequest,
    FoldedEvent, Initiator, IntentBody, IntentHandoff, KillScope, Mode, OrderExecution,
    OwnerConfirmation, Proposal, ProtectionPrices, Purpose, RiskClock, TimeInForce, TimerId,
    TimerRequest, WorkspaceId,
};

/// The control stream's two event types the runtime acts on (journal spec §9, DEC-155 item 2).
pub(crate) const RESPONSE_SUBMITTED: &str = "ApprovalResponseSubmitted";
pub(crate) const COMMAND_ISSUED: &str = "OwnerCommandIssued";

/// Whether a tailed event is the owner's input from this workspace's control stream.
pub(crate) fn is_owner_input(event: &FoldedEvent) -> bool {
    event.stream.starts_with("ctl:")
        && matches!(
            event.event_type.as_str(),
            RESPONSE_SUBMITTED | COMMAND_ISSUED
        )
}

/// The owner's input, copied at most once: a control-stream event this runtime has already copied
/// is inert, however often it is tailed (DEC-155 item 2, EI-3).
pub(crate) fn owner_input(
    state: &RuntimeState,
    event: &FoldedEvent,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    if state.has_copied(&event.event_id) {
        return Ok(());
    }
    if event.event_type == RESPONSE_SUBMITTED {
        answered(state, event, ports, batch)
    } else {
        commanded(state, event, ports, batch)
    }
}

/// The approval crate's asset class for the runtime's own (DEC-165 item 2: the same two classes).
pub(crate) fn approval_class(class: AssetClass) -> mandate_approval::AssetClass {
    match class {
        AssetClass::UsEquity => mandate_approval::AssetClass::UsEquity,
        AssetClass::Crypto => mandate_approval::AssetClass::Crypto,
    }
}

fn runtime_class(class: mandate_approval::AssetClass) -> AssetClass {
    match class {
        mandate_approval::AssetClass::UsEquity => AssetClass::UsEquity,
        mandate_approval::AssetClass::Crypto => AssetClass::Crypto,
    }
}

fn class_name(class: mandate_approval::AssetClass) -> &'static str {
    match class {
        mandate_approval::AssetClass::UsEquity => "us_equity",
        mandate_approval::AssetClass::Crypto => "crypto",
    }
}

fn askable(purpose: Purpose) -> Option<AskablePurpose> {
    match purpose {
        Purpose::Open => Some(AskablePurpose::Open),
        Purpose::Increase => Some(AskablePurpose::Increase),
        Purpose::RiskExit
        | Purpose::OwnerExit
        | Purpose::DiscretionaryExit
        | Purpose::Protective
        | Purpose::Flatten => None,
    }
}

fn purpose_of(purpose: AskablePurpose) -> Purpose {
    match purpose {
        AskablePurpose::Open => Purpose::Open,
        AskablePurpose::Increase => Purpose::Increase,
    }
}

/// The plan a request binds (DEC-942 item 1): the builder's own, or `None` when it produced none.
/// A plan naming another asset class than the order is ambiguous, so the order is not asked
/// (`Err(())`), which never adds risk (`AGENTS.md` rule 3).
fn bound_plan(proposal: &Proposal) -> Result<Option<ExecutionTerms>, ()> {
    match proposal.execution {
        None => Ok(None),
        Some(plan) if plan.asset_class == proposal.asset_class => Ok(Some(ExecutionTerms {
            asset_class: approval_class(plan.asset_class),
            tif: match plan.tif {
                TimeInForce::Day => mandate_approval::TimeInForce::Day,
                TimeInForce::Gtc => mandate_approval::TimeInForce::Gtc,
                TimeInForce::Ioc => mandate_approval::TimeInForce::Ioc,
            },
            protection_required: plan.protection_required,
            protection: plan.protection.map(|p| ProtectionTerms {
                stop: p.stop,
                take_profit: p.take_profit,
            }),
        })),
        Some(_) => Err(()),
    }
}

/// The plan a request bound, as the executor's sink reads it: the same members, unchanged.
fn granted_plan(terms: &ExecutionTerms) -> OrderExecution {
    OrderExecution {
        asset_class: runtime_class(terms.asset_class),
        tif: match terms.tif {
            mandate_approval::TimeInForce::Day => TimeInForce::Day,
            mandate_approval::TimeInForce::Gtc => TimeInForce::Gtc,
            mandate_approval::TimeInForce::Ioc => TimeInForce::Ioc,
        },
        protection_required: terms.protection_required,
        protection: terms.protection.map(|p| ProtectionPrices {
            stop: p.stop,
            take_profit: p.take_profit,
        }),
    }
}

/// The plan a journaled request's content states, recovered only when it re-derives byte for
/// byte: its members parse, its asset class is the order's, and the canonical object written back
/// from them equals the journaled one. Anything else refuses (DEC-942 item 3, `AGENTS.md` rule 3).
pub(crate) fn recovered_plan(
    stated: &Value,
    order_class: mandate_approval::AssetClass,
) -> Result<ExecutionTerms, RuntimeError> {
    let refused = || payload::non_canonical("content.execution");
    let text = |field: &str| payload::str_of(stated, field).ok_or_else(refused);
    let asset_class = match text("asset_class")? {
        "us_equity" => mandate_approval::AssetClass::UsEquity,
        "crypto" => mandate_approval::AssetClass::Crypto,
        _ => return Err(refused()),
    };
    let tif = match text("tif")? {
        "day" => mandate_approval::TimeInForce::Day,
        "gtc" => mandate_approval::TimeInForce::Gtc,
        "ioc" => mandate_approval::TimeInForce::Ioc,
        _ => return Err(refused()),
    };
    let Some(Value::Bool(protection_required)) = stated.get("protection_required") else {
        return Err(refused());
    };
    let protection = match stated.get("protection") {
        Some(Value::Null) => None,
        Some(protection) => Some(ProtectionTerms {
            stop: payload::price_of(payload::str_of(protection, "stop").ok_or_else(refused)?)?,
            take_profit: match protection.get("take_profit") {
                Some(Value::Null) => None,
                Some(Value::Str(price)) => Some(payload::price_of(price)?),
                Some(_) | None => return Err(refused()),
            },
        }),
        None => return Err(refused()),
    };
    let terms = ExecutionTerms {
        asset_class,
        tif,
        protection_required: *protection_required,
        protection,
    };
    if asset_class != order_class || mandate_approval::execution_object(&terms)? != *stated {
        return Err(refused());
    }
    Ok(terms)
}

fn clock(at: RiskClock) -> mandate_approval::RiskClock {
    mandate_approval::RiskClock(at.secs())
}

/// The order a request binds, or `None` when one of its parts cannot be bound: an exit, which is
/// never asked (`AGENTS.md` rule 2); a trigger the classifier cannot name, which check 10 could not
/// compare (DEC-156 item 4); or a combined score that is not a decimal. Each of those is not asked,
/// which never adds risk (DEC-278 item 2).
fn bound_action(
    state: &RuntimeState,
    proposal: &Proposal,
    decided_by: Option<&str>,
    version: &str,
) -> Option<BoundAction> {
    let purpose = askable(proposal.purpose)?;
    let decided_by = decided_by.filter(|label| !label.is_empty())?;
    let combined_score = Signed::parse(proposal.combined_score.as_str()?).ok()?;
    Some(BoundAction {
        instrument: proposal.instrument.as_str().to_owned(),
        asset_class: approval_class(proposal.asset_class),
        qty: proposal.qty,
        limit: proposal.limit,
        purpose,
        mandate_version: version.to_owned(),
        decided_by: decided_by.to_owned(),
        combined_score,
        reference_mark: state.mark(&proposal.instrument),
        approvers_required: NonZeroU8::MIN,
        independent_required: false,
    })
}

/// An ASK (tick step 5, mandate spec §6.4): the request with its content object and hash, its
/// `cli_inbox` delivery, the deadline timer, and the opaque notification, in that order. The
/// deadline is the risk clock plus the version's `timeout_s`, and `on_timeout` is `skip`.
///
/// The approver requirement is the bound one, one approver and no independence, because the
/// runtime folds no `PolicyChanged` yet; check 7 reads the overlay as `NONE` until it does
/// (DEC-257 item 12, DEC-278 item 3).
///
/// `deferred` names the checks the decision's dry run left to the binding gate, in byte order. The
/// request's content names the same (version 3, journal spec rules 291 and 292; DEC-2515), so the
/// approver is told which checks have not run. A deferral with no builder plan never reaches here:
/// the dry run denies it `plan_required` (DEC-2810).
#[allow(
    clippy::too_many_arguments,
    reason = "the ask's inputs are the decision's own: the proposal, its label, its deferrals, the clock, the ports, the decision and the batch"
)]
pub(crate) fn ask(
    state: &RuntimeState,
    proposal: &Proposal,
    decided_by: Option<&str>,
    deferred: &[&'static str],
    now: RiskClock,
    ports: &Ports<'_>,
    decision: EventId,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let view = ports.view;
    let Some(bound) = bound_action(state, proposal, decided_by, &view.version) else {
        return Ok(());
    };
    let Ok(execution) = bound_plan(proposal) else {
        return Ok(());
    };
    let deadline = RiskClock::from_secs(now.secs().saturating_add(view.approval.timeout_s));
    let evidence = state
        .evidence_for(&proposal.instrument, now)
        .into_iter()
        .map(|event| EvidenceRef {
            event_id: event.0,
            artifact: None,
            author: EvidenceAuthor::OwnerSelected,
        })
        .collect();
    let content = RequestContent {
        bound,
        evidence,
        risk_impact: Vec::new(),
        deadline: clock(deadline),
        execution,
        deferred_to_binding: deferred.iter().map(|name| (*name).to_owned()).collect(),
    };
    let (Ok(object), Ok(hash)) = (content_object(&content), content_hash(&content)) else {
        return Ok(());
    };
    let request = batch.journal(
        "ApprovalRequested",
        Some(decision),
        requested_payload(&content, object, &hash, view.approval.timeout_s)?,
    )?;
    let delivered = payload::object(vec![
        ("approval", payload::text(&request.0)),
        ("channel", payload::text("cli_inbox")),
        ("status", payload::text("delivered")),
        ("message_id", Value::Null),
    ])?;
    batch.journal("ApprovalDelivered", Some(request.clone()), delivered)?;
    batch.timer(TimerRequest::Arm {
        id: TimerId::ApprovalDeadline(request.clone()),
        at: deadline,
    });
    batch.notify_approval(Notification {
        subject: ApprovalRef::of_requested_event(&request.0)?,
        text: GenericText::ApprovalNeeded,
    });
    Ok(())
}

/// The request's payload: the bound fields the fold rebuilds the request from, kept at the top level
/// where earlier readers find `qty`, `limit`, `mandate_version`, and `deadline`, plus the content
/// object itself and its hash (journal spec §9, the brief's "Journal events").
fn requested_payload(
    content: &RequestContent,
    object: Value,
    hash: &ContentHash,
    timeout_s: i64,
) -> Result<Value, RuntimeError> {
    let bound = &content.bound;
    let reference_mark = match bound.reference_mark {
        None => Value::Null,
        Some(mark) => payload::object(vec![
            ("price", payload::text(&mark.price.to_string())),
            ("seq", payload::count(mark.seq, "seq")?),
        ])?,
    };
    payload::object(vec![
        ("instrument", payload::text(&bound.instrument)),
        ("asset_class", payload::text(class_name(bound.asset_class))),
        ("side", payload::text("buy")),
        ("qty", payload::text(&bound.qty.to_string())),
        ("limit", payload::text(&bound.limit.to_string())),
        (
            "purpose",
            payload::text(payload::purpose_name(purpose_of(bound.purpose))),
        ),
        ("mandate_version", payload::text(&bound.mandate_version)),
        ("decided_by", payload::text(&bound.decided_by)),
        (
            "combined_score",
            payload::text(&bound.combined_score.to_string()),
        ),
        ("reference_mark", reference_mark),
        (
            "approvers_required",
            payload::count(
                u64::from(bound.approvers_required.get()),
                "approvers_required",
            )?,
        ),
        (
            "independent_required",
            Value::Bool(bound.independent_required),
        ),
        (
            "deadline",
            payload::seconds(RiskClock::from_secs(content.deadline.0), "deadline")?,
        ),
        (
            "timeout_s",
            payload::seconds(RiskClock::from_secs(timeout_s), "timeout_s")?,
        ),
        ("on_timeout", payload::text("skip")),
        ("content", object),
        ("content_hash", payload::text(&payload::hash_text(hash))),
    ])
}

/// Whether a control-stream event is addressed to this deployment's agent. One for a sibling is
/// that sibling's runtime's to copy, so it is inert here.
fn addressed_here(state: &RuntimeState, event: &FoldedEvent) -> bool {
    payload::str_of(&event.payload, "agent") == Some(state.deployment().agent.0.as_str())
}

fn submitted_at(event: &FoldedEvent) -> Result<RiskClock, RuntimeError> {
    payload::clock_of(&event.payload, "submitted_at")
        .ok_or_else(|| payload::non_canonical("submitted_at"))
}

/// The later of a control event's `submitted_at` and the folded clock: the effective time of a
/// response, and the time resume and Stop are judged at (DEC-156 items 1 and 8, DEC-257 item 10).
fn processed_at(state: &RuntimeState, submitted: RiskClock) -> RiskClock {
    state
        .risk_clock()
        .map_or(submitted, |folded| folded.max(submitted))
}

fn refusal_code(refusal: Refusal) -> &'static str {
    match refusal {
        Refusal::NotPending => "not_pending",
        Refusal::Late => "late",
        Refusal::NotAnApprover => "not_an_approver",
        Refusal::NotDelivered => "not_delivered",
        Refusal::ContentMismatch => "content_mismatch",
        Refusal::StepUpMissing => "step_up_missing",
        Refusal::StepUpStale => "step_up_stale",
        Refusal::StepUpReused => "step_up_reused",
        Refusal::StepUpMethod => "step_up_method",
        Refusal::DuplicateApprover => "duplicate_approver",
        Refusal::NotIndependent => "not_independent",
    }
}

/// The response as admission reads it. A hash that is not a content hash cannot match any request,
/// so it is carried as the digest of its own text, which differs from every content object's
/// (check 5 refuses it as `content_mismatch` in its place among the checks).
fn response_of(
    event: &FoldedEvent,
    approval: ApprovalRef,
    verdict: Verdict,
    submitted: RiskClock,
) -> Result<Response, RuntimeError> {
    let responder = payload::str_of(&event.payload, "responder")
        .ok_or_else(|| payload::non_canonical("responder"))?;
    let stated = payload::str_of(&event.payload, "content_hash")
        .ok_or_else(|| payload::non_canonical("content_hash"))?;
    let content_hash =
        payload::hash_from(stated).unwrap_or_else(|| ContentHash(Digest::of(stated.as_bytes())));
    Ok(Response {
        source: event.event_id.0.clone(),
        approval,
        actor_kind: event.actor,
        responder: OpaqueUser(responder.to_owned()),
        verdict,
        content_hash,
        submitted_at: clock(submitted),
    })
}

/// The quorum check 7 applied, recorded on exactly the grants it judged: `admitted`, `counted`, or
/// refused `duplicate_approver` or `not_independent` (journal spec §9, DEC-488 item 2). It is
/// [`quorum`] of the request the fold holds and the overlay admission read, the call check 7
/// makes (DEC-488 item 3). A skip, which runs checks 1 to 5 only, and a grant refused at checks 1
/// to 6 applied no quorum, so their records carry no member rather than a null one. `granted` says
/// whether the answer is a grant: an admitted skip reads `Admitted` too, and check 7 never judged it.
///
/// # Errors
/// [`quorum`]'s, which admission already returned for the same request and overlay.
fn quorum_applied(
    pending: Option<&Request>,
    granted: bool,
    admission: Admission,
    policy: &PolicyOverlay,
) -> Result<Option<Value>, RuntimeError> {
    let judged = granted
        && matches!(
            admission,
            Admission::Admitted
                | Admission::Counted
                | Admission::Refused(Refusal::DuplicateApprover | Refusal::NotIndependent)
        );
    let (true, Some(request)) = (judged, pending) else {
        return Ok(None);
    };
    let applied = quorum(&request.content.bound, policy)?;
    Ok(Some(payload::object(vec![
        ("independent", Value::Bool(applied.independent_required)),
        (
            "required",
            payload::count(u64::from(applied.approvers_required.get()), "required")?,
        ),
    ])?))
}

/// An `ApprovalResponseSubmitted`: judged, copied once as `ApprovalResponded` with the result
/// whatever it is and the quorum check 7 applied where it did, and, for an admitted grant,
/// re-validated in the same step (checks 1 to 12).
fn answered(
    state: &RuntimeState,
    event: &FoldedEvent,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    if !addressed_here(state, event) {
        return Ok(());
    }
    let approval = payload::str_of(&event.payload, "approval")
        .ok_or_else(|| payload::non_canonical("approval"))?;
    let evidence = payload::step_up_of(&event.payload);
    let (verdict, verdict_name) = match payload::str_of(&event.payload, "verdict") {
        Some("approved") => (Verdict::Approve(evidence.clone()), "approved"),
        Some("skipped") => (Verdict::Skip, "skipped"),
        _ => return Err(payload::non_canonical("verdict")),
    };
    let submitted = submitted_at(event)?;
    let effective = processed_at(state, submitted);
    let approval_id = EventId(approval.to_owned());
    let pending = state
        .pending_approvals()
        .get(&approval_id)
        .filter(|_| !batch.resolved.contains(&approval_id))
        .map(|pending| &pending.request);
    let (admission, applied) = match ApprovalRef::of_requested_event(approval) {
        Err(_) => (Admission::Refused(Refusal::NotPending), None),
        Ok(reference) => {
            let response = response_of(event, reference, verdict, submitted)?;
            let settings = &ports.view.approval;
            let context = AdmissionContext {
                folded_clock: clock(effective),
                approvers: settings
                    .approvers
                    .iter()
                    .map(|user| OpaqueUser(user.clone()))
                    .collect(),
                author: OpaqueUser(settings.author.clone()),
                environment: environment(settings.environment),
                used_assertions: state.used_assertions(&event.event_id),
                policy: PolicyOverlay::NONE,
                live_cli_confirm: settings
                    .live_cli_confirm
                    .clone()
                    .filter(|_| settings.environment == mandate_journal::Environment::Live),
            };
            let admission = admit(pending, &response, &context)?;
            let applied = quorum_applied(
                pending,
                verdict_name == "approved",
                admission,
                &context.policy,
            )?;
            (admission, applied)
        }
    };
    let (result, reason) = match admission {
        Admission::Admitted => ("admitted", Value::Null),
        Admission::Counted => ("counted", Value::Null),
        Admission::Refused(refusal) => ("refused", payload::text(refusal_code(refusal))),
    };
    let responder = payload::str_of(&event.payload, "responder").unwrap_or_default();
    let mut members = vec![
        ("approval", payload::text(approval)),
        ("verdict", payload::text(verdict_name)),
        ("responder", payload::text(responder)),
        ("role", payload::text("approver")),
        ("result", payload::text(result)),
        ("reason", reason),
        ("effective_at", payload::seconds(effective, "effective_at")?),
        (
            "step_up",
            match &evidence {
                Some(evidence) => payload::step_up_value(evidence)?,
                None => Value::Null,
            },
        ),
    ];
    members.push(("quorum", applied.unwrap_or(Value::Null)));
    members.push(("separation_of_duties", Value::Null));
    members.push(("delegation", Value::Null));
    let body = payload::object(members)?;
    let copy = batch.journal("ApprovalResponded", Some(event.event_id.clone()), body)?;
    let (Admission::Admitted, Some(request)) = (admission, pending) else {
        return Ok(());
    };
    batch.resolved.insert(approval_id.clone());
    batch.timer(TimerRequest::Cancel {
        id: TimerId::ApprovalDeadline(approval_id.clone()),
    });
    if verdict_name == "approved" {
        revalidated(state, request, &approval_id, copy, effective, ports, batch)?;
    }
    Ok(())
}

/// `cli_confirm` is admitted on `paper` (DEC-155 item 4), so every other environment reads as `live`,
/// which refuses it outside DEC-923's scope: a backtest has no owner to step up.
fn environment(environment: mandate_journal::Environment) -> mandate_approval::Environment {
    match environment {
        mandate_journal::Environment::Paper => mandate_approval::Environment::Paper,
        mandate_journal::Environment::Live | mandate_journal::Environment::Backtest => {
            mandate_approval::Environment::Live
        }
    }
}

fn mode_now(mode: Mode) -> ModeNow {
    match mode {
        Mode::Normal => ModeNow::Normal,
        Mode::ExitsOnly => ModeNow::ExitsOnly,
        Mode::Paused => ModeNow::Paused,
        Mode::Stopped => ModeNow::Stopped,
    }
}

/// The re-classification's `ask` label, or `null` when it is not an `ask` (journal spec §9.7,
/// DEC-533 item 4): an `auto` passes check 10 and a `deny` skips, whatever label decided them.
fn asked_by(classification: &Classification) -> Value {
    match classification {
        Classification::Ask { decided_by } if !decided_by.is_empty() => payload::text(decided_by),
        Classification::Ask { .. } | Classification::Auto | Classification::Deny => Value::Null,
    }
}

fn skip_code(reason: &SkipReason) -> &str {
    match reason {
        SkipReason::VersionChanged => "version_changed",
        SkipReason::Mode => "mode",
        SkipReason::InstrumentRestricted => "instrument_restricted",
        SkipReason::ReclassifiedDeny => "reclassified_deny",
        SkipReason::ReclassifiedOtherTrigger => "reclassified_other_trigger",
        SkipReason::Gate { reason } => reason,
        SkipReason::Drift => "drift",
    }
}

/// The bound order as the ports read a proposal, so the classifier and the dry run judge exactly
/// what the owner approved (checks 10 and 11), with the plan its request bound under the content
/// hash and never a new or re-sized one (DEC-942 item 3, `AGENTS.md` rule 4).
fn bound_proposal(
    bound: &BoundAction,
    execution: Option<OrderExecution>,
) -> Result<Proposal, RuntimeError> {
    Ok(Proposal {
        instrument: payload::instrument_of(&bound.instrument)?,
        asset_class: runtime_class(bound.asset_class),
        side: Side::Buy,
        qty: bound.qty,
        limit: bound.limit,
        purpose: purpose_of(bound.purpose),
        exit_origin: None,
        exit_conviction: None,
        buy_conviction: None,
        combined_score: payload::text(&bound.combined_score.to_string()),
        outputs_used: Default::default(),
        model_weights: Default::default(),
        clips_applied: Vec::new(),
        execution,
    })
}

fn price_or_null(price: Option<Price>) -> Value {
    price.map_or(Value::Null, |p| payload::text(&p.to_string()))
}

/// Checks 8 to 12 on the state this step applies, and on `act` the bound order, journaled before it
/// is handed with the re-validation as its cause (journal spec §9.1, EI-4, `AGENTS.md` rule 5).
fn revalidated(
    state: &RuntimeState,
    request: &Request,
    approval: &EventId,
    responded: EventId,
    now: RiskClock,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let bound = &request.content.bound;
    let execution = request.content.execution.as_ref().map(granted_plan);
    let proposal = bound_proposal(bound, execution)?;
    let verdict = dry_run(state, ports, &proposal, now);
    let classified = overlaid(ports.plan.classify(ports.view, &proposal), &verdict);
    let decided_now = classified.decided_by.clone().unwrap_or_default();
    let classification = match classified.autonomy {
        Autonomy::Auto => Classification::Auto,
        Autonomy::Ask => Classification::Ask {
            decided_by: decided_now,
        },
        Autonomy::Deny => Classification::Deny,
    };
    let asked_label = asked_by(&classification);
    let (dry_run, dry_run_name, dry_run_reason) = match verdict {
        DryRunVerdict::Allow | DryRunVerdict::AllowDeferring { .. } => {
            (DryRun::Allow, "allow", Value::Null)
        }
        DryRunVerdict::Deny { reason_code } | DryRunVerdict::DenyOnFigures { reason_code, .. } => (
            DryRun::Deny {
                reason: reason_code.clone(),
            },
            "deny",
            payload::text(&reason_code),
        ),
    };
    let mode = state.effective_mode();
    let mark_now = state.mark(&proposal.instrument).map(|mark| mark.price);
    let current = Current {
        mandate_version: ports.view.version.clone(),
        mode: mode_now(mode),
        instrument_restricted: ports
            .view
            .restricted_instruments
            .contains(&proposal.instrument),
        in_working_universe: ports.view.working_universe.contains(&proposal.instrument),
        classification,
        dry_run,
        mark_now,
    };
    let outcome = revalidate(request, &current)?;
    let (result, reason) = match &outcome {
        Revalidation::Act(_) => ("act", Value::Null),
        Revalidation::Skip(why) => ("skip", payload::text(skip_code(why))),
    };
    let band = mandate_approval::band_bp(bound.asset_class)?;
    let body = payload::object(vec![
        ("approval", payload::text(&approval.0)),
        ("result", payload::text(result)),
        ("reason", reason),
        (
            "mandate_version_bound",
            payload::text(&bound.mandate_version),
        ),
        (
            "mandate_version_now",
            payload::text(&current.mandate_version),
        ),
        ("mode", payload::text(payload::mode_name(mode))),
        (
            "instrument_restricted",
            Value::Bool(current.instrument_restricted),
        ),
        ("decided_by_bound", payload::text(&bound.decided_by)),
        ("decided_by_now", asked_label),
        ("dry_run", payload::text(dry_run_name)),
        ("dry_run_reason", dry_run_reason),
        (
            "m_req",
            price_or_null(bound.reference_mark.map(|mark| mark.price)),
        ),
        ("m_now", price_or_null(mark_now)),
        ("band_bp", payload::count(u64::from(band), "band_bp")?),
    ])?;
    let revalidation = batch.journal("ApprovalRevalidated", Some(responded), body)?;
    let Revalidation::Act(granted) = outcome else {
        return Ok(());
    };
    let order = granted.order();
    let act = bound_proposal(order, execution)?;
    let intent = batch.journal("IntentProposed", Some(revalidation), proposed(&act)?)?;
    batch.hand(IntentHandoff {
        intent_id: intent,
        body: IntentBody::Order {
            instrument: act.instrument,
            side: act.side,
            qty: act.qty,
            limit: Some(act.limit),
            purpose: act.purpose,
        },
        execution: act.execution,
    });
    Ok(())
}

/// The bid, bid size, and floor an owner command confirmed, or `None` when it confirmed no bid.
/// Partly given is not given (DEC-66): a bid without its size or floor cannot price the ladder.
fn confirmed_bid(event: &FoldedEvent) -> Result<Option<(Price, Qty, Price)>, RuntimeError> {
    let (Some(bid), Some(size), Some(floor)) = (
        payload::str_of(&event.payload, "bid"),
        payload::str_of(&event.payload, "bid_size"),
        payload::str_of(&event.payload, "floor"),
    ) else {
        return Ok(None);
    };
    Ok(Some((
        payload::price_of(bid)?,
        payload::qty_of(size)?,
        payload::price_of(floor)?,
    )))
}

/// The confirmation the owner-exit privilege unlocks: a confirmed bid under valid step-up, and
/// nothing otherwise (DEC-158 option (c), DEC-66).
fn privileged(
    bid: Option<(Price, Qty, Price)>,
    evidence: Option<&StepUp>,
    user: &str,
) -> Option<OwnerConfirmation> {
    let (bid, bid_size, floor) = bid?;
    Some(OwnerConfirmation {
        bid,
        bid_size,
        floor,
        user: user.to_owned(),
        step_up: evidence?.assertion.0.clone(),
    })
}

/// `step_up_status` for evidence judged as the owner committed it (DEC-257 item 8): `valid` when it
/// counts, `stale` when only its age fails, and `absent` otherwise, since evidence that does not
/// count is missing evidence (mandate spec §6.1).
fn step_up_status(refusal: Option<StepUpRefusal>) -> &'static str {
    match refusal {
        None => "valid",
        Some(StepUpRefusal::Stale) => "stale",
        Some(StepUpRefusal::Missing | StepUpRefusal::Reused | StepUpRefusal::Method) => "absent",
    }
}

/// How the owner's command was judged, for the copy that records it: the status, the evidence when
/// it is valid, and the confirmation the privilege unlocks.
pub(crate) struct Judged {
    pub(crate) status: &'static str,
    pub(crate) step_up: Value,
    pub(crate) confirmation: Option<OwnerConfirmation>,
}

fn judged(
    refusal: Option<StepUpRefusal>,
    evidence: Option<&StepUp>,
    bid: Option<(Price, Qty, Price)>,
    user: &str,
) -> Result<Judged, RuntimeError> {
    let valid = refusal.is_none();
    Ok(Judged {
        status: step_up_status(refusal),
        step_up: match evidence.filter(|_| valid) {
            Some(evidence) => payload::step_up_stamped(evidence)?,
            None => Value::Null,
        },
        confirmation: privileged(bid, evidence.filter(|_| valid), user),
    })
}

/// An `OwnerCommandIssued` (mandate spec §6.1, PX-4, DEC-156 item 8, DEC-158 option (c)).
fn commanded(
    state: &RuntimeState,
    event: &FoldedEvent,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let command = payload::str_of(&event.payload, "command")
        .ok_or_else(|| payload::non_canonical("command"))?;
    let submitted = submitted_at(event)?;
    let evidence = payload::step_up_of(&event.payload);
    let user =
        payload::str_of(&event.payload, "user").ok_or_else(|| payload::non_canonical("user"))?;
    let environment = environment(ports.view.approval.environment);
    let used = state.used_assertions(&event.event_id);
    let cause = Some(event.event_id.clone());
    if command == "kill_switch" {
        let scope = kill_scope(event)?;
        let authority =
            mandate_approval::kill_switch(evidence.as_ref(), clock(submitted), environment, &used)?;
        let refusal = match authority {
            KillSwitchAuthority::OwnerExitPrivileges => None,
            KillSwitchAuthority::AutomatedFlatten(why) => Some(why),
        };
        let judged = judged(refusal, evidence.as_ref(), confirmed_bid(event)?, user)?;
        return switched(
            state,
            &Switching {
                scope: &scope,
                initiator: Initiator::Owner,
                confirmation: judged.confirmation.as_ref(),
                step_up: Some((judged.status, &judged.step_up)),
                user: Some(user),
                cause,
            },
            ports,
            batch,
        );
    }
    if !addressed_here(state, event) {
        return Ok(());
    }
    let kind = match command {
        "pause" => OwnerCommandKind::Pause,
        "resume" => OwnerCommandKind::Resume,
        "stop" => OwnerCommandKind::Stop,
        "owner_exit" => OwnerCommandKind::OwnerExit,
        _ => return Err(payload::non_canonical("command")),
    };
    let judged_at = processed_at(state, submitted);
    let authority = mandate_approval::owner_command(
        kind,
        evidence.as_ref(),
        clock(submitted),
        clock(judged_at),
        environment,
        &used,
    )?;
    let refusal = match authority {
        CommandAuthority::Apply => None,
        CommandAuthority::Refused(why) => Some(why),
    };
    match (kind, refusal) {
        (OwnerCommandKind::Pause, _) => {
            let lifecycle = state.lifecycle().max(Mode::Paused);
            lifecycle_change(state, payload::REASON_OWNER_PAUSE, lifecycle, cause, batch)?;
            cancel_approvals(state, batch, payload::REASON_OWNER_PAUSE)
        }
        (OwnerCommandKind::Resume, None) => {
            let lifecycle = match state.lifecycle() {
                Mode::Stopped => Mode::Stopped,
                _ => Mode::Normal,
            };
            lifecycle_change(state, payload::REASON_OWNER_RESUME, lifecycle, cause, batch)
        }
        (OwnerCommandKind::Stop, None) => {
            lifecycle_change(
                state,
                payload::REASON_OWNER_STOP,
                Mode::Stopped,
                cause,
                batch,
            )?;
            cancel_approvals(state, batch, payload::REASON_OWNER_STOP)
        }
        (OwnerCommandKind::OwnerExit, _) => {
            let judged = judged(refusal, evidence.as_ref(), confirmed_bid(event)?, user)?;
            owner_exit(state, event, &judged, ports, batch)
        }
        (OwnerCommandKind::Resume, Some(why)) => refused("resume", why, judged_at, cause, batch),
        (OwnerCommandKind::Stop, Some(why)) => refused("stop", why, judged_at, cause, batch),
        (OwnerCommandKind::Acknowledge, _) => Ok(()),
    }
}

/// A resume or Stop whose step-up does not count, recorded as its one copy, `OwnerCommandRefused`,
/// in place of the `AgentModeChanged` it would have written (journal spec §9 and rule 16, mandate
/// spec §6.1; DEC-280 item 7, the #395 review's major 1): the command, the reason, and the
/// effective time it was judged at, with the `OwnerCommandIssued` as `causation_id`.
///
/// # Errors
/// [`RuntimeError`] when the draft cannot be written.
fn refused(
    command: &'static str,
    refusal: StepUpRefusal,
    judged_at: RiskClock,
    cause: Option<EventId>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    let reason = match refusal {
        StepUpRefusal::Missing => "step_up_missing",
        StepUpRefusal::Stale => "step_up_stale",
        StepUpRefusal::Reused => "step_up_reused",
        StepUpRefusal::Method => "step_up_method",
    };
    let body = payload::object(vec![
        ("command", payload::text(command)),
        ("effective_at", payload::stamp(judged_at, "effective_at")?),
        ("reason", payload::text(reason)),
    ])?;
    batch.journal("OwnerCommandRefused", cause, body)?;
    Ok(())
}

/// The scope a control-stream kill switch names. Whether it reaches this deployment is
/// [`switched`]'s to decide, as for every kill switch (trading-domain spec §5.5).
fn kill_scope(event: &FoldedEvent) -> Result<KillScope, RuntimeError> {
    let subject = payload::str_of(&event.payload, "subject")
        .ok_or_else(|| payload::non_canonical("subject"))?
        .to_owned();
    let scope = match payload::str_of(&event.payload, "scope") {
        Some("agent") => KillScope::Agent(AgentId(subject)),
        Some("connection") => KillScope::Connection(ConnectionId(subject)),
        Some("workspace") => KillScope::Workspace(WorkspaceId(subject)),
        _ => return Err(payload::non_canonical("scope")),
    };
    Ok(scope)
}

/// The owner's exit of one instrument (DEC-257 item 7, DEC-2387): the copy records the bid as given
/// and the step-up as judged at commit. Then each sell of the agent-scoped plan of that instrument
/// is journaled as an `IntentProposed` at the market, caused by the copy, and handed right after
/// its own draft, whatever the step-up: the executor reads the floor or the confirmation from the
/// copy (DEC-2040), so only those depend on it. The journal is the only channel to the cell's
/// executor, which is why the sells are proposed and not handed as a flatten (journal spec §9.1).
fn owner_exit(
    state: &RuntimeState,
    event: &FoldedEvent,
    judged: &Judged,
    ports: &Ports<'_>,
    batch: &mut Batch<'_>,
) -> Result<(), RuntimeError> {
    if payload::str_of(&event.payload, "scope") != Some("instrument") {
        return Err(payload::non_canonical("scope"));
    }
    let subject = payload::str_of(&event.payload, "subject")
        .ok_or_else(|| payload::non_canonical("subject"))?;
    let instrument = payload::instrument_of(subject)?;
    let user =
        payload::str_of(&event.payload, "user").ok_or_else(|| payload::non_canonical("user"))?;
    let given = |field: &'static str| {
        payload::str_of(&event.payload, field).map_or(Value::Null, payload::text)
    };
    let body = payload::object(vec![
        ("scope", payload::text("instrument")),
        ("subject", payload::text(instrument.as_str())),
        ("confirmed", Value::Bool(confirmed_bid(event)?.is_some())),
        ("bid", given("bid")),
        ("bid_size", given("bid_size")),
        ("floor", given("floor")),
        ("user", payload::text(user)),
        ("step_up_status", payload::text(judged.status)),
        ("step_up", judged.step_up.clone()),
    ])?;
    let copy = batch.journal("OwnerExitRequested", Some(event.event_id.clone()), body)?;
    let plan = exit_plan(state, instrument, judged.confirmation.clone(), ports);
    for sell in plan.sells {
        let tif = owner_sell_tif(sell.asset_class);
        let leg = batch.journal(
            "IntentProposed",
            Some(copy.clone()),
            owner_sell(&sell, tif)?,
        )?;
        batch.hand(IntentHandoff {
            intent_id: leg,
            body: IntentBody::Order {
                instrument: sell.instrument,
                side: Side::Sell,
                qty: sell.qty,
                limit: None,
                purpose: Purpose::OwnerExit,
            },
            execution: Some(OrderExecution {
                asset_class: sell.asset_class,
                tif,
                protection_required: false,
                protection: None,
            }),
        });
    }
    Ok(())
}

/// An owner's market sell's time in force: the day for an equity, and good till cancelled for a
/// crypto pair, which trades around the clock and takes no `day` (trading spec §5.1).
fn owner_sell_tif(class: AssetClass) -> TimeInForce {
    match class {
        AssetClass::UsEquity => TimeInForce::Day,
        AssetClass::Crypto => TimeInForce::Gtc,
    }
}

/// One owner's sell as journal spec §9.1's `IntentProposed` records it: at the market, so no
/// limit, at `tif`, and the purpose `owner_exit`.
fn owner_sell(sell: &FlattenLeg, tif: TimeInForce) -> Result<Value, RuntimeError> {
    payload::object(vec![
        ("instrument_id", payload::text(sell.instrument.as_str())),
        ("side", payload::text(payload::side_name(Side::Sell))),
        ("type", payload::text("market")),
        ("tif", payload::text(tif_name(tif))),
        ("qty", payload::text(&sell.qty.to_string())),
        ("limit_price", Value::Null),
        (
            "purpose",
            payload::text(payload::purpose_name(Purpose::OwnerExit)),
        ),
    ])
}

/// The planner's flatten of one instrument for an owner exit: only this agent's orders in it, and
/// exactly its sub-ledger quantity (family F). Shared by the live step, the retry, and the restart,
/// so all three ask for the same plan.
pub(crate) fn exit_plan(
    state: &RuntimeState,
    instrument: InstrumentId,
    confirmation: Option<OwnerConfirmation>,
    ports: &Ports<'_>,
) -> crate::types::FlattenPlan {
    let working_orders = state.working_orders_in(&instrument);
    ports.flatten.plan(&FlattenRequest {
        initiator: Initiator::Owner,
        instrument: Some(instrument),
        confirmation,
        working_orders,
    })
}

#[cfg(test)]
mod tests;
