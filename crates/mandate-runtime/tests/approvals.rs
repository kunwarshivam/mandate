//! The runtime's approval path (M7 tests PR 3 of 4; backlog E8-1 to E8-3; mandate spec §6.1 and
//! §6.4; journal spec §2 and §9; DEC-155, DEC-156, DEC-158 option (c), DEC-173, DEC-257).
//!
//! The owner's answer reaches the runtime only as a control-stream `ApprovalResponseSubmitted`, and
//! a command only as an `OwnerCommandIssued`: each is folded as the tailer folds it and then handed
//! in as its own `Input::Journal`. Expectations are read from the drafts' payloads and from the
//! fixture's own constants, never from the runtime's state or from `mandate-approval`, and every
//! "nothing happened" assertion is paired with the case where the same step does act, so a runtime
//! that does nothing passes none of them.
//!
//! The grant path is live. The tests of `ApprovalResponded.quorum`, the count and independence
//! check 7 applied (journal spec §9, DEC-488), read the member off the journaled record; before
//! the runtime wrote it they failed on its absence rather than at a stub, which no stub can report
//! from a path the live tests exercise (DEC-489).

mod common;

use common::escalation::{
    ASKED_AT, Answer, Asked, BOUND_LIMIT, Command, DEADLINE, RecordingFlatten, asking_shell,
    evidence, hash_of, mark, mode_applied, next_account_seq, next_control_seq, quorum_of,
    rebound_shell, tail, two_approvers, version_applied,
};
use common::{
    AUTHOR, AllowGate, DenyGate, FixedPlan, OWNER, Ran, Shell, TestIds, clock, int, object, ports,
    ports_with_flatten, universe,
};
use mandate_accounting::Side;
use mandate_approval::{GenericText, Notification};
use mandate_canon::Value;
use mandate_journal::Environment;
use mandate_runtime::{
    ActorKind, Autonomy, Effect, EventDraft, EventId, Initiator, Input, IntentBody, KillScope,
    Purpose, TimerId, TimerRequest,
};
use mandate_time::UtcNanos;

fn member<'a>(draft: &'a EventDraft, key: &str) -> Option<&'a str> {
    draft.payload.get(key).and_then(Value::as_str)
}

fn drafts_of<'a>(ran: &'a Ran, event_type: &str) -> Vec<&'a EventDraft> {
    ran.drafts
        .iter()
        .filter(|d| d.event_type == event_type)
        .collect()
}

fn only<'a>(ran: &'a Ran, event_type: &str) -> &'a EventDraft {
    let found = drafts_of(ran, event_type);
    assert_eq!(
        found.len(),
        1,
        "exactly one {event_type} in {:?}",
        ran.draft_types()
    );
    found[0]
}

/// The `ApprovalResponded` a step wrote: its result and reason, and that it names the approval and
/// copies the control-stream event it answers (journal spec §2, rule 16's pattern).
fn responded(ran: &Ran, asked: &Asked, source: &EventId) -> (String, Option<String>) {
    let draft = only(ran, "ApprovalResponded");
    assert_eq!(member(draft, "approval"), Some(asked.approval.0.as_str()));
    assert_eq!(
        draft.causation_id.as_ref(),
        Some(source),
        "an ApprovalResponded copies the ApprovalResponseSubmitted it answers"
    );
    (
        member(draft, "result").unwrap_or_default().to_owned(),
        member(draft, "reason").map(str::to_owned),
    )
}

fn refused_with(ran: &Ran, asked: &Asked, source: &EventId, reason: &str) {
    assert_eq!(
        responded(ran, asked, source),
        ("refused".to_owned(), Some(reason.to_owned())),
        "{:?}",
        ran.draft_types()
    );
    assert!(
        drafts_of(ran, "IntentProposed").is_empty() && ran.handed.is_empty(),
        "a refused response acts on nothing: {:?}",
        ran.draft_types()
    );
}

/// The bound order the fixture asks for, as the content object's `action` states it.
fn action_of(asked: &Asked) -> Value {
    asked
        .request
        .payload
        .get("content")
        .and_then(|content| content.get("action"))
        .cloned()
        .unwrap_or_else(|| {
            panic!(
                "the request carries its content object: {:?}",
                asked.request
            )
        })
}

/// That `ran` is the grant's act: admitted, re-validated `act`, and then exactly the bound order,
/// journaled before it is handed, with the causation chain intent → re-validation.
fn acted(ran: &Ran, asked: &Asked, source: &EventId) {
    assert_eq!(
        ran.draft_types(),
        vec!["ApprovalResponded", "ApprovalRevalidated", "IntentProposed"],
        "admission, re-validation and the intent commit in one batch (brief, lifecycle)"
    );
    assert_eq!(responded(ran, asked, source), ("admitted".to_owned(), None));
    let revalidated = only(ran, "ApprovalRevalidated");
    assert_eq!(
        member(revalidated, "approval"),
        Some(asked.approval.0.as_str())
    );
    assert_eq!(member(revalidated, "result"), Some("act"));
    let intent = only(ran, "IntentProposed");
    assert_eq!(
        intent.causation_id.as_ref(),
        Some(&revalidated.event_id),
        "after a grant the intent's causation is the ApprovalRevalidated (journal spec §9.1)"
    );
    let action = action_of(asked);
    for (intent_key, action_key) in [
        ("instrument_id", "instrument"),
        ("side", "side"),
        ("qty", "qty"),
        ("limit_price", "limit"),
        ("purpose", "purpose"),
    ] {
        assert_eq!(
            intent.payload.get(intent_key),
            action.get(action_key),
            "the intent repeats the bound {action_key} (EI-4)"
        );
    }
    assert_eq!(member(intent, "type"), Some("limit"));
    assert_eq!(member(intent, "tif"), Some("day"));
    assert_eq!(ran.handed.len(), 1, "one handoff: {:?}", ran.handed);
    assert_eq!(ran.handed[0].intent_id, intent.event_id);
    assert_eq!(
        ran.handed[0].body,
        IntentBody::Order {
            instrument: common::instrument("AAPL"),
            side: Side::Buy,
            qty: common::qty("10"),
            limit: common::price(BOUND_LIMIT),
            purpose: Purpose::Open,
        },
        "the handoff is the bound order, never re-priced or re-sized (EI-4)"
    );
    let drafted = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_id == intent.event_id));
    let handed = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Intent(_)));
    assert!(drafted < handed, "journal before acting (rule 5)");
}

fn skipped_on_revalidation(ran: &Ran, asked: &Asked, source: &EventId, reason: &str) {
    assert_eq!(
        ran.draft_types(),
        vec!["ApprovalResponded", "ApprovalRevalidated"],
        "re-validation only skips"
    );
    assert_eq!(responded(ran, asked, source), ("admitted".to_owned(), None));
    let revalidated = only(ran, "ApprovalRevalidated");
    assert_eq!(member(revalidated, "result"), Some("skip"));
    assert_eq!(member(revalidated, "reason"), Some(reason));
    assert!(ran.handed.is_empty(), "{:?}", ran.handed);
}

/// E8-1, E8-3, EI-4, EI-10, EI-14, EI-16: the request commits its content object and the hash of
/// it, is delivered to `cli_inbox` and notified in its own batch, and a timely, stepped-up grant
/// from a listed approver acts with exactly the bound order; the same grant from the author, who is
/// not listed, is refused first and acts on nothing.
#[test]
fn a_timely_grant_acts_with_the_bound_order() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let by_author = Answer {
        responder: AUTHOR.to_owned(),
        ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20)
    }
    .event();
    refused_with(
        &tail(&mut shell, &by_author, &ports),
        &asked,
        &by_author.event_id,
        "not_an_approver",
    );
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &grant, &ports);
    acted(&ran, &asked, &grant.event_id);
    assert!(
        shell.state.pending_approvals().is_empty(),
        "an act is terminal (EI-6)"
    );

    let content = asked
        .request
        .payload
        .get("content")
        .expect("the request carries its content object");
    assert_eq!(
        Some(asked.content_hash.as_str()),
        Some(hash_of(content).as_str()),
        "the content hash is the hash of the committed content object (EI-14)"
    );
    assert_eq!(
        asked.batch.draft_types(),
        vec!["DecisionMade", "ApprovalRequested", "ApprovalDelivered"],
        "cli_inbox is delivered in the request's own batch (DEC-156 item 6)"
    );
    let delivered = only(&asked.batch, "ApprovalDelivered");
    assert_eq!(
        member(delivered, "approval"),
        Some(asked.approval.0.as_str())
    );
    assert_eq!(member(delivered, "channel"), Some("cli_inbox"));
    assert_eq!(member(delivered, "status"), Some("delivered"));
    assert_eq!(
        asked.batch.approval_notifications,
        vec![Notification {
            subject: mandate_approval::ApprovalRef::of_requested_event(&asked.approval.0)
                .expect("a ULID-shaped request id"),
            text: GenericText::ApprovalNeeded,
        }],
        "one opaque notification per request (EI-9)"
    );
    let requested_at = asked
        .batch
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_type == "ApprovalRequested"));
    let notified_at = asked
        .batch
        .effects
        .iter()
        .position(|e| matches!(e, Effect::NotifyApproval(_)));
    assert!(
        requested_at < notified_at,
        "a notification never points at an uncommitted request (rule 5)"
    );
}

/// PX-7: an admitted skip from a listed approver ends the approval, with no step-up, and nothing
/// is sent; its deadline is disarmed. A grant after it is not pending.
#[test]
fn a_skip_ends_the_approval_and_sends_nothing() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let skip = Answer::skip(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &skip, &ports);
    assert_eq!(ran.draft_types(), vec!["ApprovalResponded"]);
    assert_eq!(
        responded(&ran, &asked, &skip.event_id),
        ("admitted".to_owned(), None)
    );
    assert_eq!(
        member(only(&ran, "ApprovalResponded"), "verdict"),
        Some("skipped")
    );
    assert!(ran.handed.is_empty());
    assert!(ran.timers.contains(&TimerRequest::Cancel {
        id: TimerId::ApprovalDeadline(asked.approval.clone()),
    }));
    assert!(shell.state.pending_approvals().is_empty());

    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 40).event();
    let ran = tail(&mut shell, &grant, &ports);
    refused_with(&ran, &asked, &grant.event_id, "not_pending");
}

/// PB-1, EI-2: the deadline skips and acts on nothing, and a grant the owner submitted before the
/// deadline but that the runtime reads after the timeout is not pending; read before it, the same
/// grant acts.
#[test]
fn the_timeout_never_acts_and_a_grant_read_after_it_is_not_pending() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let ran = shell.run(Input::Tick(clock(DEADLINE)), &ports);
    assert!(drafts_of(&ran, "ApprovalTimedOut").len() == 1 && ran.handed.is_empty());

    let grant = Answer::grant(next_control_seq(&shell), &asked, DEADLINE - 10).event();
    let ran = tail(&mut shell, &grant, &ports);
    refused_with(&ran, &asked, &grant.event_id, "not_pending");

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let grant = Answer::grant(next_control_seq(&shell), &asked, DEADLINE - 10).event();
    acted(&tail(&mut shell, &grant, &ports), &asked, &grant.event_id);
}

/// PB-2, PB-3, EI-15: the effective time is the later of `submitted_at` and the folded clock, so a
/// response submitted in time and read once the clock has reached the deadline is late, and so is
/// one submitted exactly at it. A refusal is not terminal: the deadline still times it out.
#[test]
fn lateness_is_judged_at_the_later_of_submission_and_the_folded_clock() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let at_deadline = Answer::grant(next_control_seq(&shell), &asked, DEADLINE).event();
    let ran = tail(&mut shell, &at_deadline, &ports);
    refused_with(&ran, &asked, &at_deadline.event_id, "late");

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let clocked = mark(next_account_seq(&shell), BOUND_LIMIT, DEADLINE);
    shell.fold_one(&clocked).expect("a mark folds");
    let early = Answer::grant(next_control_seq(&shell), &asked, DEADLINE - 100).event();
    let ran = tail(&mut shell, &early, &ports);
    refused_with(&ran, &asked, &early.event_id, "late");
    assert!(
        shell
            .state
            .pending_approvals()
            .contains_key(&asked.approval),
        "a refusal is not terminal (EI-6)"
    );
    let ran = shell.run(Input::Tick(clock(DEADLINE)), &ports);
    assert_eq!(drafts_of(&ran, "ApprovalTimedOut").len(), 1);

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let timely = Answer::grant(next_control_seq(&shell), &asked, DEADLINE - 1).event();
    acted(&tail(&mut shell, &timely, &ports), &asked, &timely.event_id);
}

/// PB-4, EI-3, DEC-173 item 10: a control-stream response tailed twice is copied once and acts
/// once, and so is one re-tailed after a restart; the restart re-hands the one intent it recorded
/// and proposes no second. A second, distinct grant for the same approval is copied and refused as
/// `not_pending`, since the first one ended it.
#[test]
fn a_re_tailed_response_is_copied_once_and_acts_once() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let first = tail(&mut shell, &grant, &ports);
    acted(&first, &asked, &grant.event_id);
    let intent = only(&first, "IntentProposed").event_id.clone();

    let again = shell.run(Input::Journal(grant.clone()), &ports);
    assert!(
        again.is_empty(),
        "the ctl event_id is the idempotency key: {:?}",
        again.draft_types()
    );

    let (mut restarted, recovered) = shell.restart(&ports);
    assert!(
        recovered
            .drafts
            .iter()
            .all(|d| d.event_type != "IntentProposed")
            && recovered.handed.iter().all(|h| h.intent_id == intent),
        "a restart re-hands the recorded intent and proposes nothing new: {:?}",
        recovered.draft_types()
    );
    let replayed = restarted.run(Input::Journal(grant), &ports);
    assert!(
        replayed
            .draft_types()
            .iter()
            .all(|t| !t.starts_with("Approval") && *t != "IntentProposed")
            && replayed.handed.is_empty(),
        "a re-tailed response after a restart is not copied or acted on again: {:?}",
        replayed.draft_types()
    );
    let second = Answer::grant(next_control_seq(&restarted), &asked, ASKED_AT + 40).event();
    refused_with(
        &tail(&mut restarted, &second, &ports),
        &asked,
        &second.event_id,
        "not_pending",
    );
    let intents = restarted
        .agent_journal
        .iter()
        .filter(|e| e.event_type == "IntentProposed")
        .count();
    assert_eq!(intents, 1, "one approval, at most one intent (EI-3)");
}

/// EI-14, PB-15: a response that does not repeat the request's content hash is refused and leaves
/// the approval pending, so the right one still acts.
#[test]
fn a_wrong_content_hash_is_refused_and_the_right_one_acts() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let wrong = Answer {
        content_hash: format!("sha256:{}", "a".repeat(64)),
        ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30)
    }
    .event();
    let ran = tail(&mut shell, &wrong, &ports);
    refused_with(&ran, &asked, &wrong.event_id, "content_mismatch");
    let right = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 40).event();
    acted(&tail(&mut shell, &right, &ports), &asked, &right.event_id);
}

/// PB-10, EI-10: only a `user` actor listed in `approvers` is admitted. An agent, a system, a
/// broker, or a platform operator naming the owner as responder is refused, and so is the author,
/// who is not listed.
#[test]
fn only_a_listed_user_is_admitted() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    for actor in [
        ActorKind::Agent,
        ActorKind::System,
        ActorKind::Broker,
        ActorKind::PlatformOperator,
    ] {
        let forged = Answer {
            actor,
            ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 10)
        }
        .event();
        let ran = tail(&mut shell, &forged, &ports);
        refused_with(&ran, &asked, &forged.event_id, "not_an_approver");
    }
    let author = Answer {
        responder: AUTHOR.to_owned(),
        ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20)
    }
    .event();
    let ran = tail(&mut shell, &author, &ports);
    refused_with(&ran, &asked, &author.event_id, "not_an_approver");

    let owner = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    acted(&tail(&mut shell, &owner, &ports), &asked, &owner.event_id);
}

/// PB-11, PB-12, EI-11: a grant's step-up is judged at its effective time. Missing evidence,
/// evidence older than 300 s, and an assertion an earlier control-stream event already carried
/// are each refused; evidence exactly 300 s old acts.
#[test]
fn a_grant_needs_fresh_unused_step_up() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let at = ASKED_AT + 200;

    let missing = Answer {
        step_up: None,
        ..Answer::grant(next_control_seq(&shell), &asked, at)
    }
    .event();
    let ran = tail(&mut shell, &missing, &ports);
    refused_with(&ran, &asked, &missing.event_id, "step_up_missing");

    let stale = Answer {
        step_up: evidence("assertion-stale", at - 301),
        ..Answer::grant(next_control_seq(&shell), &asked, at)
    }
    .event();
    let ran = tail(&mut shell, &stale, &ports);
    refused_with(&ran, &asked, &stale.event_id, "step_up_stale");

    let reused = Answer {
        step_up: evidence("assertion-stale", at),
        ..Answer::grant(next_control_seq(&shell), &asked, at)
    }
    .event();
    let ran = tail(&mut shell, &reused, &ports);
    refused_with(&ran, &asked, &reused.event_id, "step_up_reused");

    let edge = Answer {
        step_up: evidence("assertion-edge", at - 300),
        ..Answer::grant(next_control_seq(&shell), &asked, at)
    }
    .event();
    acted(&tail(&mut shell, &edge, &ports), &asked, &edge.event_id);
}

/// PB-21, EI-7, DEC-131 item 25(j): a grant handed in the step that applies a copied exits-only
/// restriction is judged against the pending set after that step's own cancellations, so it is not
/// pending and never acts. Without the restriction the same grant acts.
#[test]
fn a_grant_in_the_step_that_cancels_its_approval_is_not_pending() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let tightened = mode_applied(next_account_seq(&shell), "exits_only", ASKED_AT + 20);
    shell.fold_one(&tightened).expect("the copy folds");
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &grant, &ports);
    assert_eq!(
        ran.draft_types(),
        vec!["AgentModeChanged", "ApprovalCanceled", "ApprovalResponded"],
        "the step applies the mode, cancels, and only then judges the response"
    );
    refused_with(&ran, &asked, &grant.event_id, "not_pending");

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    acted(&tail(&mut shell, &grant, &ports), &asked, &grant.event_id);
}

/// PB-5, EI-7: any `MandateVersionApplied` cancels the approval, so a grant bound to the old
/// version is not pending and never re-binds to the new one.
#[test]
fn an_approval_under_a_changed_version_is_not_pending() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let applied = version_applied(next_account_seq(&shell), "v2", ASKED_AT + 10);
    shell.fold_one(&applied).expect("folds");
    let ran = shell.run(Input::Journal(applied), &ports);
    assert_eq!(drafts_of(&ran, "ApprovalCanceled").len(), 1);

    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
    let ran = tail(&mut shell, &grant, &ports);
    refused_with(&ran, &asked, &grant.event_id, "not_pending");

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
    acted(&tail(&mut shell, &grant, &ports), &asked, &grant.event_id);
}

/// PB-13, EI-8: a kill switch is applied whatever the approval state and cancels the approval, so
/// a grant read after it is not pending and the switch's flatten is the only handoff.
#[test]
fn a_grant_read_after_a_kill_switch_is_not_pending() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let switched = shell.run(
        Input::Command(mandate_runtime::Command::KillSwitch {
            scope: KillScope::Agent(common::deployment().agent),
            initiator: Initiator::RiskLimit,
            confirmation: None,
        }),
        &ports,
    );
    assert_eq!(switched.handed.len(), 1, "the flatten is handed at once");
    assert_eq!(drafts_of(&switched, "ApprovalCanceled").len(), 1);

    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &grant, &ports);
    refused_with(&ran, &asked, &grant.event_id, "not_pending");

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    acted(&tail(&mut shell, &grant, &ports), &asked, &grant.event_id);
}

/// Checks 10 and 11, PB-6, EI-5: a grant whose order is now classified `deny`, or `ask` by another
/// trigger, or that the dry run denies, is admitted and then skipped with that reason; nothing is
/// sent and the approval is terminal.
#[test]
fn re_validation_skips_a_reclassified_or_gate_denied_order() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let asking = ports(&ids, &gate, &plan, &view);
    let denying_plan = FixedPlan::opening(Autonomy::Deny);
    let other_trigger = FixedPlan {
        decided_by: "rule:another",
        ..FixedPlan::opening(Autonomy::Ask)
    };
    let closing = DenyGate("close_window");
    for (now, reason) in [
        (
            ports(&ids, &gate, &denying_plan, &view),
            "reclassified_deny",
        ),
        (
            ports(&ids, &gate, &other_trigger, &view),
            "reclassified_other_trigger",
        ),
        (ports(&ids, &closing, &plan, &view), "close_window"),
    ] {
        let (mut shell, asked) = asking_shell(&asking, Some(BOUND_LIMIT));
        let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
        let ran = tail(&mut shell, &grant, &now);
        skipped_on_revalidation(&ran, &asked, &grant.event_id, reason);
        assert!(shell.state.pending_approvals().is_empty(), "{reason}");
    }
}

/// Check 12, PB-7, DEC-156 item 3: an equity's band is 100 bp of the mark at the request, either
/// way. A mark exactly at the band acts; one unit beyond it, up or down, skips as `drift`; and a
/// request with no mark skips as `drift`.
#[test]
fn drift_beyond_the_band_either_way_or_no_mark_skips() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    for (now, acts) in [
        ("156.55", true),
        ("153.45", true),
        ("156.550000001", false),
        ("153.449999999", false),
    ] {
        let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
        let moved = mark(next_account_seq(&shell), now, ASKED_AT + 20);
        shell.fold_one(&moved).expect("a mark folds");
        let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
        let ran = tail(&mut shell, &grant, &ports);
        if acts {
            acted(&ran, &asked, &grant.event_id);
        } else {
            skipped_on_revalidation(&ran, &asked, &grant.event_id, "drift");
        }
    }
    let (mut shell, asked) = asking_shell(&ports, None);
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &grant, &ports);
    skipped_on_revalidation(&ran, &asked, &grant.event_id, "drift");
}

/// The quorum check 7 applied, or `None` when the record carries none: `quorum` absent, or `null`
/// as journal spec v0.17 §9.7 writes it (DEC-533 item 3, whose presence `answer_records.rs` pins).
fn quorum_recorded(ran: &Ran) -> Option<&Value> {
    only(ran, "ApprovalResponded")
        .payload
        .get("quorum")
        .filter(|recorded| !matches!(recorded, Value::Null))
}

/// Journal spec §9, mandate spec §6.4 check 7, DEC-488: an admitted grant's `ApprovalResponded`
/// records exactly the approver count and independence check 7 applied, `{required, independent}`
/// as the request bound them with no overlay folded; a skip (checks 1 to 5 only) and a grant
/// refused at any check before 7 (`not_an_approver`, `content_mismatch`, `step_up_missing`, `late`,
/// and `not_pending` after the act) record no quorum, because check 7 applied nothing to them.
#[test]
fn an_admitted_grant_records_the_quorum_check_7_applied_and_no_other_response_does() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let expected = quorum_of(&asked.request);
    assert_eq!(
        expected,
        object(&[("independent", Value::Bool(false)), ("required", int(1))]),
        "the fixture binds one approver without independence (DEC-278 item 3)"
    );
    let wrong_hash = format!("sha256:{}", "b".repeat(64));
    let before_check_7 = [
        (
            Answer {
                responder: AUTHOR.to_owned(),
                ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20)
            },
            "not_an_approver",
        ),
        (
            Answer {
                content_hash: wrong_hash,
                ..Answer::grant(next_control_seq(&shell) + 1, &asked, ASKED_AT + 21)
            },
            "content_mismatch",
        ),
        (
            Answer {
                step_up: None,
                ..Answer::grant(next_control_seq(&shell) + 2, &asked, ASKED_AT + 22)
            },
            "step_up_missing",
        ),
        (
            Answer::grant(next_control_seq(&shell) + 3, &asked, DEADLINE),
            "late",
        ),
    ];
    for (answer, reason) in before_check_7 {
        let event = answer.event();
        let ran = tail(&mut shell, &event, &ports);
        refused_with(&ran, &asked, &event.event_id, reason);
        assert_eq!(
            quorum_recorded(&ran),
            None,
            "a grant refused `{reason}` never reached check 7, so its record applies no quorum"
        );
    }
    let grant = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &grant, &ports);
    acted(&ran, &asked, &grant.event_id);
    assert_eq!(
        quorum_recorded(&ran),
        Some(&expected),
        "an admitted grant records the count and independence check 7 applied: {:?}",
        only(&ran, "ApprovalResponded").payload
    );
    let after_the_act = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 40).event();
    let ran = tail(&mut shell, &after_the_act, &ports);
    refused_with(&ran, &asked, &after_the_act.event_id, "not_pending");
    assert_eq!(
        quorum_recorded(&ran),
        None,
        "a grant to an ended approval is refused at check 1 and applies no quorum"
    );

    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let skip = Answer::skip(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &skip, &ports);
    assert_eq!(
        responded(&ran, &asked, &skip.event_id),
        ("admitted".to_owned(), None)
    );
    assert_eq!(
        quorum_recorded(&ran),
        None,
        "a skip runs checks 1 to 5 only, so its record applies no quorum (PX-7)"
    );
}

/// Journal spec §8, DEC-488 item 3: the quorum is read from the request the fold holds, so the
/// record a restarted process writes for the same grant is the record the live process writes,
/// member for member, `quorum` included.
#[test]
fn the_quorum_is_rebuilt_from_the_journal_on_restart() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut live, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let (mut restarted, _) = live.restart(&ports);
    let grant = Answer::grant(next_control_seq(&live), &asked, ASKED_AT + 30).event();
    let from_live = tail(&mut live, &grant, &ports);
    let from_restart = tail(&mut restarted, &grant, &ports);
    acted(&from_live, &asked, &grant.event_id);
    acted(&from_restart, &asked, &grant.event_id);
    assert_eq!(
        only(&from_restart, "ApprovalResponded").payload,
        only(&from_live, "ApprovalResponded").payload,
        "the record is a function of the journal alone"
    );
    assert_eq!(
        quorum_recorded(&from_restart),
        Some(&quorum_of(&asked.request)),
        "the restarted process applies the quorum the journaled request binds"
    );
}

/// Mandate spec §6.4 check 7, DEC-488 item 2: under a request bound to two approvers, the first
/// grant is `counted` and records the requirement it was counted against, `{required: 2}`; the same
/// approver's second grant is refused `duplicate_approver` at check 7 and records that same
/// requirement; the second approver's grant is admitted with it and acts. Every record of the three
/// states the quorum, because check 7 judged each.
#[test]
fn a_grant_counted_short_of_two_approvers_records_the_requirement_it_was_counted_against() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        two_approvers(),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = rebound_shell(&ports, 2, false);
    let expected = quorum_of(&asked.request);
    assert_eq!(
        expected,
        object(&[("independent", Value::Bool(false)), ("required", int(2))]),
        "the rebound request binds two approvers"
    );
    let first = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20).event();
    let ran = tail(&mut shell, &first, &ports);
    assert_eq!(
        ran.draft_types(),
        vec!["ApprovalResponded"],
        "a counted grant ends nothing and acts on nothing"
    );
    assert_eq!(
        responded(&ran, &asked, &first.event_id),
        ("counted".to_owned(), None)
    );
    assert!(ran.handed.is_empty(), "{:?}", ran.handed);
    assert_eq!(
        quorum_recorded(&ran),
        Some(&expected),
        "a counted grant records the quorum it fell short of"
    );
    let again = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 25).event();
    let ran = tail(&mut shell, &again, &ports);
    refused_with(&ran, &asked, &again.event_id, "duplicate_approver");
    assert_eq!(
        quorum_recorded(&ran),
        Some(&expected),
        "a grant refused at check 7 records the quorum that refused it"
    );
    let second = Answer {
        responder: AUTHOR.to_owned(),
        ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30)
    }
    .event();
    let ran = tail(&mut shell, &second, &ports);
    acted(&ran, &asked, &second.event_id);
    assert_eq!(
        quorum_recorded(&ran),
        Some(&expected),
        "the grant that reaches two approvers records the quorum it reached"
    );
}

/// Mandate spec §6.4 check 7, DEC-488 item 2: under a request bound to independent approval, the
/// author's grant is refused `not_independent` at check 7 and records `{independent: true}`, the
/// requirement that excluded it; a listed approver who is not the author is admitted with the same
/// record and acts. Independence is the bound value, not the count of listed approvers.
#[test]
fn an_author_s_grant_under_independence_is_refused_with_the_quorum_that_excluded_it() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        two_approvers(),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = rebound_shell(&ports, 1, true);
    let expected = quorum_of(&asked.request);
    assert_eq!(
        expected,
        object(&[("independent", Value::Bool(true)), ("required", int(1))]),
        "the rebound request binds one independent approver"
    );
    let by_author = Answer {
        responder: AUTHOR.to_owned(),
        ..Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 20)
    }
    .event();
    let ran = tail(&mut shell, &by_author, &ports);
    refused_with(&ran, &asked, &by_author.event_id, "not_independent");
    assert_eq!(
        quorum_recorded(&ran),
        Some(&expected),
        "the author's grant records the independence that excluded it"
    );
    let by_owner = Answer::grant(next_control_seq(&shell), &asked, ASKED_AT + 30).event();
    let ran = tail(&mut shell, &by_owner, &ports);
    acted(&ran, &asked, &by_owner.event_id);
    assert_eq!(
        quorum_recorded(&ran),
        Some(&expected),
        "an independent approver's admitted grant records the same requirement"
    );
}

/// The owner's exit of one instrument and the copy the runtime makes of it.
fn owner_exit_copy<'a>(ran: &'a Ran, source: &EventId) -> &'a EventDraft {
    let copy = only(ran, "OwnerExitRequested");
    assert_eq!(
        copy.causation_id.as_ref(),
        Some(source),
        "a copy names its OwnerCommandIssued (journal spec §9.1 rule 16)"
    );
    assert_eq!(member(copy, "scope"), Some("instrument"));
    assert_eq!(member(copy, "subject"), Some("AAPL"));
    assert_eq!(member(copy, "user"), Some(OWNER));
    copy
}

/// The handoff an owner exit makes: one flatten of `AAPL` alone, journaled before it is handed and
/// identified by the `OwnerExitRequested` that records it.
fn exit_handed(ran: &Ran, copy: &EventDraft, flatten: &RecordingFlatten) -> FlattenRequestSeen {
    assert_eq!(
        ran.handed.len(),
        1,
        "the exit reaches the executor: {:?}",
        ran.handed
    );
    assert_eq!(ran.handed[0].intent_id, copy.event_id);
    let IntentBody::Flatten(plan) = &ran.handed[0].body else {
        panic!("an owner exit is handed as a flatten: {:?}", ran.handed[0]);
    };
    assert_eq!(plan.purpose, Purpose::OwnerExit);
    let drafted = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Journal(d) if d.event_id == copy.event_id));
    let handed = ran
        .effects
        .iter()
        .position(|e| matches!(e, Effect::Intent(_)));
    assert!(drafted < handed, "journal before acting (rule 5)");
    let asked = flatten.asked.borrow();
    let request = asked.last().expect("the planner was asked");
    assert_eq!(request.initiator, Initiator::Owner);
    assert_eq!(request.instrument, Some(common::instrument("AAPL")));
    FlattenRequestSeen {
        confirmed: request.confirmation.is_some(),
        plan_confirmed: plan.confirmation.is_some(),
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FlattenRequestSeen {
    confirmed: bool,
    plan_confirmed: bool,
}

/// The #281 obligation, rule 13, mandate spec §6.1 ("Owner controls and step-up", DEC-173 item 5):
/// an owner exit whose step-up was already stale when the owner committed it, even with a confirmed
/// bid, is refused **as an owner exit**: the copy records `step_up_status: stale` and the bid as
/// given, the plan carries no confirmation, so equities wait for the regular session, and the exit
/// itself still reaches the executor. With valid step-up the same exit carries the confirmation.
#[test]
fn a_refused_owner_exit_step_up_still_routes_the_exit() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let flatten = RecordingFlatten::new();
    let ports = ports_with_flatten(&ids, &gate, &plan, &flatten, &view);
    let (mut shell, _) = started(&ports);
    let committed = ASKED_AT + 30;
    let stale = Command {
        bid: Some(("154.1", "300", "150")),
        step_up: evidence("assertion-exit", committed - 301),
        ..Command::exit_aapl(next_control_seq(&shell), committed)
    }
    .event();
    let ran = tail(&mut shell, &stale, &ports);
    let copy = owner_exit_copy(&ran, &stale.event_id);
    assert_eq!(member(copy, "step_up_status"), Some("stale"));
    assert_eq!(copy.payload.get("step_up"), Some(&Value::Null));
    assert_eq!(
        copy.payload.get("confirmed"),
        Some(&Value::Bool(true)),
        "a confirmed bid is recorded as given and unlocks nothing (journal spec §9.1)"
    );
    assert_eq!(
        exit_handed(&ran, copy, &flatten),
        FlattenRequestSeen {
            confirmed: false,
            plan_confirmed: false
        },
        "the refusal withdraws only the out-of-session privilege; the exit is never dropped"
    );

    let (mut shell, _) = started(&ports);
    let valid = Command {
        bid: Some(("154.1", "300", "150")),
        step_up: evidence("assertion-exit-2", committed - 10),
        ..Command::exit_aapl(next_control_seq(&shell), committed)
    }
    .event();
    let ran = tail(&mut shell, &valid, &ports);
    let copy = owner_exit_copy(&ran, &valid.event_id);
    assert_eq!(member(copy, "step_up_status"), Some("valid"));
    assert_ne!(copy.payload.get("step_up"), Some(&Value::Null));
    assert_eq!(
        exit_handed(&ran, copy, &flatten),
        FlattenRequestSeen {
            confirmed: true,
            plan_confirmed: true
        }
    );
}

/// The #281 obligation's other half: an owner exit with no step-up at all is refused the same way
/// and is still routed, and so is one the runtime reads long after the owner committed it with
/// evidence that was fresh then (judged at commit, DEC-156 item 8).
#[test]
fn an_owner_exit_is_judged_at_commit_and_never_dropped() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let flatten = RecordingFlatten::new();
    let ports = ports_with_flatten(&ids, &gate, &plan, &flatten, &view);

    let (mut shell, _) = started(&ports);
    let bare = Command {
        step_up: None,
        ..Command::exit_aapl(next_control_seq(&shell), ASKED_AT + 30)
    }
    .event();
    let ran = tail(&mut shell, &bare, &ports);
    let copy = owner_exit_copy(&ran, &bare.event_id);
    assert_eq!(member(copy, "step_up_status"), Some("absent"));
    assert_eq!(
        exit_handed(&ran, copy, &flatten),
        FlattenRequestSeen {
            confirmed: false,
            plan_confirmed: false
        }
    );

    let (mut shell, _) = started(&ports);
    shell.run(Input::Tick(clock(ASKED_AT + 3_600)), &ports);
    let late_read = Command {
        bid: Some(("154.1", "300", "150")),
        ..Command::exit_aapl(next_control_seq(&shell), ASKED_AT + 30)
    }
    .event();
    let ran = tail(&mut shell, &late_read, &ports);
    let copy = owner_exit_copy(&ran, &late_read.event_id);
    assert_eq!(member(copy, "step_up_status"), Some("valid"));
    assert_eq!(
        exit_handed(&ran, copy, &flatten),
        FlattenRequestSeen {
            confirmed: true,
            plan_confirmed: true
        }
    );
}

/// PX-4, EI-8: an owner's pause through the control stream needs no step-up, applies at once, and
/// cancels the pending approval; its copy names the command.
#[test]
fn an_owner_pause_needs_no_step_up_and_cancels_the_approval() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let pause = Command {
        step_up: None,
        ..Command::to_agent(next_control_seq(&shell), "pause", ASKED_AT + 10)
    }
    .event();
    let ran = tail(&mut shell, &pause, &ports);
    let changed = only(&ran, "AgentModeChanged");
    assert_eq!(member(changed, "reason"), Some("owner_pause"));
    assert_eq!(member(changed, "to"), Some("paused"));
    assert_eq!(changed.causation_id.as_ref(), Some(&pause.event_id));
    let canceled = only(&ran, "ApprovalCanceled");
    assert_eq!(
        member(canceled, "approval"),
        Some(asked.approval.0.as_str())
    );
}

/// Mandate spec §6.1: a resume needs step-up valid when the runtime processes it. Stale evidence is
/// refused and leaves the pause in force, and its one copy is the `OwnerCommandRefused` (journal
/// spec §9, rule 16; DEC-280 item 7); fresh evidence resumes, and the copy names its command.
#[test]
fn a_resume_needs_step_up_fresh_when_processed() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, _) = started(&ports);
    let pause = Command {
        step_up: None,
        ..Command::to_agent(next_control_seq(&shell), "pause", ASKED_AT + 10)
    }
    .event();
    tail(&mut shell, &pause, &ports);
    shell.run(Input::Tick(clock(ASKED_AT + 400)), &ports);

    let stale = Command::to_agent(next_control_seq(&shell), "resume", ASKED_AT + 20).event();
    let ran = tail(&mut shell, &stale, &ports);
    refused_only(
        &ran,
        &stale.event_id,
        "resume",
        "step_up_stale",
        ASKED_AT + 400,
    );
    assert_eq!(shell.state.effective_mode(), mandate_runtime::Mode::Paused);

    let fresh = Command::to_agent(next_control_seq(&shell), "resume", ASKED_AT + 390).event();
    let ran = tail(&mut shell, &fresh, &ports);
    let changed = only(&ran, "AgentModeChanged");
    assert_eq!(member(changed, "reason"), Some("owner_resume"));
    assert_eq!(changed.causation_id.as_ref(), Some(&fresh.event_id));
    assert!(drafts_of(&ran, "OwnerCommandRefused").is_empty());
    assert_eq!(shell.state.effective_mode(), mandate_runtime::Mode::Normal);
}

/// `OwnerCommandRefused`'s members, which journal spec §9.2 closes (DEC-261): the command, the
/// reason, and the effective time it was judged at.
const REFUSED_MEMBERS: [&str; 3] = ["command", "effective_at", "reason"];

/// Journal spec §9 and rule 16, mandate spec §6.1, DEC-280 item 7: a refused resume or Stop leaves
/// exactly one event, its `OwnerCommandRefused`, and never an `AgentModeChanged`. It names the
/// command, the reason, and the effective time it was judged at (the later of `submitted_at` and
/// the folded clock), copies the `OwnerCommandIssued` as `causation_id`, and has no other member.
///
/// The judged second is read in the form the writer carries, whatever that form is (DEC-308): the
/// integer seconds DEC-291's writer shipped, or the §4.7 timestamp of the same instant §9.2 types
/// (DEC-261 item 7) that the implementation PR stamps. The timestamp is compared as an instant, not
/// a truncated second: one off the whole second, such as `<judged second>.999999999Z`, is not the
/// judged second. The form itself is pinned by the pending
/// pins in `escalation::tests`, not here, so the implementation PR changes no `tests/` file but
/// the `#[ignore]` deletions (DEC-77 item 2, DEC-309).
fn refused_only(ran: &Ran, source: &EventId, command: &str, reason: &str, judged_at: i64) {
    assert_eq!(
        ran.draft_types(),
        vec!["OwnerCommandRefused"],
        "a refused {command} leaves exactly its refusal"
    );
    let draft = &ran.drafts[0];
    assert_eq!(draft.causation_id.as_ref(), Some(source));
    assert_eq!(member(draft, "command"), Some(command));
    assert_eq!(member(draft, "reason"), Some(reason));
    let judged = match draft.payload.get("effective_at") {
        Some(Value::Int(secs)) => i64::try_from(secs.get()).ok(),
        Some(Value::Str(stamp)) => UtcNanos::parse(stamp)
            .ok()
            .filter(|instant| instant.nanos() == 0)
            .map(UtcNanos::secs),
        other => panic!("effective_at is {other:?}, not the judged second {judged_at}"),
    };
    assert_eq!(
        judged,
        Some(judged_at),
        "judged at {judged_at}, got {:?}",
        draft.payload.get("effective_at")
    );
    let members: Vec<&str> = draft
        .payload
        .as_object()
        .map(|o| o.keys().map(|k| k.as_str()).collect())
        .unwrap_or_default();
    assert_eq!(members, REFUSED_MEMBERS.to_vec());
}

/// The #395 review, major 1, and its backlog row: a Stop whose step-up does not count leaves
/// exactly its `OwnerCommandRefused` and never an `AgentModeChanged`, for each reason a Stop can be
/// refused that this fixture can write: no evidence (`step_up_missing`), evidence older than its
/// window when processed (`step_up_stale`), and an assertion an earlier command already used
/// (`step_up_reused`). The agent keeps running after each. A Stop with fresh, unused evidence then
/// stops it, with an `AgentModeChanged` and no refusal.
#[test]
fn a_refused_stop_leaves_exactly_its_owner_command_refused() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, _) = started(&ports);
    let reconciled = shell.state.effective_mode();

    let missing = Command {
        step_up: None,
        ..Command::to_agent(next_control_seq(&shell), "stop", ASKED_AT + 10)
    }
    .event();
    let ran = tail(&mut shell, &missing, &ports);
    refused_only(
        &ran,
        &missing.event_id,
        "stop",
        "step_up_missing",
        ASKED_AT + 10,
    );
    assert_eq!(shell.state.effective_mode(), reconciled);

    let stale = Command {
        step_up: evidence("assertion-stale", ASKED_AT - 600),
        ..Command::to_agent(next_control_seq(&shell), "stop", ASKED_AT + 20)
    }
    .event();
    let ran = tail(&mut shell, &stale, &ports);
    refused_only(
        &ran,
        &stale.event_id,
        "stop",
        "step_up_stale",
        ASKED_AT + 20,
    );
    assert_eq!(shell.state.effective_mode(), reconciled);

    let pause = Command {
        step_up: evidence("assertion-shared", ASKED_AT + 30),
        ..Command::to_agent(next_control_seq(&shell), "pause", ASKED_AT + 30)
    }
    .event();
    tail(&mut shell, &pause, &ports);
    let paused = shell.state.effective_mode();
    let reused = Command {
        step_up: evidence("assertion-shared", ASKED_AT + 30),
        ..Command::to_agent(next_control_seq(&shell), "stop", ASKED_AT + 31)
    }
    .event();
    let ran = tail(&mut shell, &reused, &ports);
    refused_only(
        &ran,
        &reused.event_id,
        "stop",
        "step_up_reused",
        ASKED_AT + 31,
    );
    assert_eq!(shell.state.effective_mode(), paused);

    let fresh = Command::to_agent(next_control_seq(&shell), "stop", ASKED_AT + 40).event();
    let ran = tail(&mut shell, &fresh, &ports);
    let changed = only(&ran, "AgentModeChanged");
    assert_eq!(member(changed, "reason"), Some("owner_stop"));
    assert_eq!(changed.causation_id.as_ref(), Some(&fresh.event_id));
    assert!(drafts_of(&ran, "OwnerCommandRefused").is_empty());
    assert_eq!(shell.state.effective_mode(), mandate_runtime::Mode::Stopped);
}

/// Mandate spec §6.1, DEC-155 item 4: `cli_confirm` counts on `paper` alone. On a backtest view a
/// Stop whose evidence is fresh and unused is refused for its method, and leaves exactly its
/// `OwnerCommandRefused` with `step_up_method` (the #411 review, minor 1).
#[test]
fn a_stop_with_cli_confirm_off_paper_is_refused_for_its_method() {
    let (ids, gate, plan) = (TestIds, AllowGate, FixedPlan::silent());
    let mut view = universe(&["AAPL"]);
    view.approval.environment = Environment::Backtest;
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, _) = started(&ports);
    let reconciled = shell.state.effective_mode();
    let stop = Command::to_agent(next_control_seq(&shell), "stop", ASKED_AT + 10).event();
    let ran = tail(&mut shell, &stop, &ports);
    refused_only(
        &ran,
        &stop.event_id,
        "stop",
        "step_up_method",
        ASKED_AT + 10,
    );
    assert_eq!(shell.state.effective_mode(), reconciled);
}

/// Journal spec §2, EI-3: a refused command is copied at most once. After a restart that folds
/// the journal, the same `OwnerCommandIssued` re-tailed writes nothing, neither a second refusal nor
/// a mode change.
#[test]
fn a_refused_command_is_recorded_once_across_a_restart() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, _) = started(&ports);
    let stop = Command {
        step_up: None,
        ..Command::to_agent(next_control_seq(&shell), "stop", ASKED_AT + 10)
    }
    .event();
    let ran = tail(&mut shell, &stop, &ports);
    refused_only(
        &ran,
        &stop.event_id,
        "stop",
        "step_up_missing",
        ASKED_AT + 10,
    );

    let (mut restarted, _) = shell.restart(&ports);
    let again = restarted.run(Input::Journal(stop.clone()), &ports);
    assert!(
        drafts_of(&again, "OwnerCommandRefused").is_empty()
            && drafts_of(&again, "AgentModeChanged").is_empty(),
        "a re-tailed refused command is not copied again: {:?}",
        again.draft_types()
    );
}

/// PB-14, PB-14b, DEC-158 option (c): an owner's kill switch is never refused. With missing or
/// stale evidence it still stops the agent and flattens, without the owner-exit privilege; with
/// evidence fresh when the owner committed it, a runtime that reads it an hour later applies it with
/// the confirmed bid.
#[test]
fn an_owner_kill_switch_is_never_refused() {
    let (ids, gate, plan, view) = (TestIds, AllowGate, FixedPlan::silent(), universe(&["AAPL"]));
    let flatten = RecordingFlatten::new();
    let ports = ports_with_flatten(&ids, &gate, &plan, &flatten, &view);
    let committed = ASKED_AT + 30;
    for (step_up, confirmed) in [
        (None, false),
        (evidence("assertion-kill-stale", committed - 301), false),
        (evidence("assertion-kill-fresh", committed), true),
    ] {
        let (mut shell, _) = started(&ports);
        shell.run(Input::Tick(clock(committed + 3_600)), &ports);
        let kill = Command {
            bid: Some(("154.1", "300", "150")),
            step_up,
            ..Command::to_agent(next_control_seq(&shell), "kill_switch", committed)
        }
        .event();
        let ran = tail(&mut shell, &kill, &ports);
        let switch = only(&ran, "KillSwitchActivated");
        assert_eq!(switch.causation_id.as_ref(), Some(&kill.event_id));
        assert_eq!(member(switch, "initiator"), Some("owner"));
        assert_eq!(
            shell.state.effective_mode(),
            mandate_runtime::Mode::Stopped,
            "an owner's switch stops the agent"
        );
        assert_eq!(ran.handed.len(), 1, "and flattens: {:?}", ran.handed);
        let asked = flatten.asked.borrow();
        let request = asked.last().expect("the planner was asked");
        assert_eq!(request.instrument, None, "a kill switch flattens the agent");
        assert_eq!(
            request.confirmation.is_some(),
            confirmed,
            "only valid step-up unlocks the out-of-session privilege"
        );
    }
}

/// A started shell with nothing pending, for the owner-command cases.
fn started(ports: &mandate_runtime::Ports<'_>) -> (Shell, Ran) {
    let mut shell = Shell::new(1);
    shell
        .fold_one(&common::escalation::reconciliation(1, ASKED_AT))
        .expect("folds");
    shell.restart(ports)
}

/// Live, EI-6, DEC-131 item 23: an approval still pending when a restart takes the startup hold is
/// cancelled by the first step after it, and that step's expiry skips it even when the step is a
/// tick at or past its deadline, so it ends in exactly one terminal event, never a cancellation and
/// a timeout both.
#[test]
fn a_restart_past_the_deadline_ends_the_approval_once() {
    let (ids, gate, plan, view) = (
        TestIds,
        AllowGate,
        FixedPlan::opening(Autonomy::Ask),
        universe(&["AAPL"]),
    );
    let ports = ports(&ids, &gate, &plan, &view);
    let (mut shell, asked) = asking_shell(&ports, Some(BOUND_LIMIT));
    let submitted = common::event(
        common::ACCOUNT_STREAM,
        next_account_seq(&shell),
        "OrderSubmitted",
        common::object(&[("client_order_id", common::text("elsewhere"))]),
    );
    shell
        .fold_one(&submitted)
        .expect("an unreconciled submission folds");
    let (mut restarted, _) = shell.restart(&ports);
    assert_eq!(
        restarted.state.effective_mode(),
        mandate_runtime::Mode::Paused,
        "the fixture takes the startup hold"
    );
    let ran = restarted.run(Input::Tick(clock(DEADLINE + 10)), &ports);
    let ended: Vec<&EventDraft> = ran
        .drafts
        .iter()
        .filter(|d| {
            matches!(
                d.event_type.as_str(),
                "ApprovalCanceled" | "ApprovalTimedOut"
            ) && member(d, "approval") == Some(asked.approval.0.as_str())
        })
        .collect();
    assert_eq!(
        ended
            .iter()
            .map(|d| d.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["ApprovalCanceled"],
        "one terminal event: {:?}",
        ran.draft_types()
    );
}
