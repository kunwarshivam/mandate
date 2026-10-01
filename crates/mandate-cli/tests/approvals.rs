//! `mandate approvals` (M7 tests PR 4 of 4; the brief's "The CLI control surface"; mandate spec
//! §6.4; DEC-155 item 5, DEC-257 items 5 and 9).
//!
//! The journal is `tests/common`'s own in-memory `Journal`, whose agent streams hold approval
//! events as the runtime writes them. Every expectation is computed here from the fixture: the content hash and the code from
//! the content object's canonical bytes, the deadlines from the fixture's own clock, and what the
//! CLI committed from the control stream's stored bytes. Each "commits nothing" case is paired with
//! the one that commits, so a command that does nothing passes none of them.
//!
//! Every test but the fixture check is pending until the CLI's implementation and fails on the
//! command's `ControlError::Unimplemented` (DEC-77, DEC-110).

mod common;

use common::{
    AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OTHER_AGENT, OWNER, TIMEOUT_S, at, body, code_of,
    hash_of, member, member_text, owner,
};
use mandate_canon::Value;
use mandate_cli::approvals::{
    Ended, Outcome, Revalidated, State, approve, list, message, outcome, show, skip,
};
use mandate_cli::control::{ControlError, Submitted};

fn answer<T>(what: &str, result: Result<T, ControlError>) -> T {
    result.unwrap_or_else(|e| panic!("{what} answers, not: {e}"))
}

/// The control stream's events, as stored.
fn control(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL).iter().map(body).collect()
}

/// Live: the fixture's journal holds what the runtime would have written, and its oracle computes
/// the hash the request states.
#[test]
fn the_fixture_journal_holds_a_delivered_request() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let rows = fx.rows(&common::agent_stream(AGENT));
    assert_eq!(rows.len(), 2);
    let request = body(&rows[0]);
    assert_eq!(
        member_text(&request, "event_type"),
        Some("ApprovalRequested")
    );
    assert_eq!(
        member_text(&request, "payload.content_hash"),
        Some(hash_of(&asked.content).as_str())
    );
    assert_eq!(code_of(&asked.content).len(), 8);
    assert!(control(&fx).is_empty());
}

/// D5: pending approvals by deadline, then resolved ones with how they ended; each with its
/// deadline and the time left, from the fixture's clock.
#[test]
#[ignore = "pending E8-3"]
fn list_orders_pending_by_deadline_then_the_resolved() {
    let mut fx = Fixture::new();
    let later = fx.ask(AGENT, "10", ASKED_AT + 60);
    let sooner = fx.ask(OTHER_AGENT, "5", ASKED_AT);
    let skipped = fx.ask(AGENT, "3", ASKED_AT - 1_000);
    fx.responded(
        &skipped,
        &common::runtime_id(900),
        "skipped",
        "admitted",
        None,
    );
    let timed_out = fx.ask(OTHER_AGENT, "2", ASKED_AT - 2_000);
    fx.ended(&timed_out, "ApprovalTimedOut");
    let now = ASKED_AT + 100;

    let listed = answer(
        "list",
        list(&fx.journal, &owner(), &[AGENT, OTHER_AGENT], at(now)),
    );
    let seen: Vec<(&str, State, i64, i64)> = listed
        .iter()
        .map(|l| (l.approval.as_str(), l.state, l.deadline_s, l.remaining_s))
        .collect();
    assert_eq!(
        seen[..2],
        [
            (
                sooner.approval.as_str(),
                State::Pending,
                sooner.deadline,
                sooner.deadline - now
            ),
            (
                later.approval.as_str(),
                State::Pending,
                later.deadline,
                later.deadline - now
            ),
        ]
    );
    let mut resolved: Vec<(&str, State)> = seen[2..].iter().map(|s| (s.0, s.1)).collect();
    resolved.sort();
    let mut want = vec![
        (skipped.approval.as_str(), State::Skipped(Ended::ByOwner)),
        (timed_out.approval.as_str(), State::Skipped(Ended::TimedOut)),
    ];
    want.sort();
    assert_eq!(resolved, want);
    assert!(seen[2..].iter().all(|s| s.3 == 0), "{seen:?}");
}

/// A grant that acted and one re-validation skipped are told apart, and a refused or counted
/// response leaves the approval pending.
#[test]
#[ignore = "pending E8-3"]
fn list_reads_how_a_grant_ended() {
    let mut fx = Fixture::new();
    let acted = fx.ask(AGENT, "10", ASKED_AT);
    fx.responded(
        &acted,
        &common::runtime_id(901),
        "approved",
        "admitted",
        None,
    );
    fx.revalidated(&acted, "act", None);
    let drifted = fx.ask(AGENT, "4", ASKED_AT);
    fx.responded(
        &drifted,
        &common::runtime_id(902),
        "approved",
        "admitted",
        None,
    );
    fx.revalidated(&drifted, "skip", Some("drift"));
    let refused = fx.ask(AGENT, "2", ASKED_AT);
    fx.responded(
        &refused,
        &common::runtime_id(903),
        "approved",
        "refused",
        Some("step_up_stale"),
    );

    let listed = answer(
        "list",
        list(&fx.journal, &owner(), &[AGENT], at(ASKED_AT + 1)),
    );
    let state = |id: &str| listed.iter().find(|l| l.approval == id).map(|l| l.state);
    assert_eq!(state(&acted.approval), Some(State::Acted));
    assert_eq!(
        state(&drifted.approval),
        Some(State::Skipped(Ended::OnRevalidation))
    );
    assert_eq!(state(&refused.approval), Some(State::Pending));
}

/// D6, P8: `show` renders the content object exactly as committed and the code for its hash,
/// computed here from the canonical bytes; an approval the stream does not hold is `not_pending`.
#[test]
#[ignore = "pending E8-3"]
fn show_renders_the_committed_content_and_its_code() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let shown = answer("show", show(&fx.journal, &owner(), AGENT, &asked.approval));
    assert_eq!(shown.content, asked.content);
    assert_eq!(shown.content_hash, hash_of(&asked.content));
    assert_eq!(shown.code, code_of(&asked.content));
    assert_eq!(shown.deadline_s, asked.deadline);
    assert_eq!(
        show(&fx.journal, &owner(), AGENT, &common::runtime_id(777)),
        Err(ControlError::Refused {
            reason: "not_pending"
        })
    );
    assert!(control(&fx).is_empty(), "show commits nothing");
}

/// EI-14, PX-7: `approve` with the right code commits exactly one `ApprovalResponseSubmitted` by
/// the owner as a `user`, repeating the content hash, with `cli_confirm` evidence authenticated
/// when the owner ran it and a fresh assertion; nothing reaches an agent stream. Another
/// request's code is refused first and commits nothing.
#[test]
#[ignore = "pending E8-3"]
fn approve_commits_one_stepped_up_response() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let other = fx.ask(AGENT, "4", ASKED_AT);
    let before = fx.rows(&common::agent_stream(AGENT)).len();
    let mut ids = FixedIds::default();
    let now = ASKED_AT + 30;
    let shown = answer("show", show(&fx.journal, &owner(), AGENT, &asked.approval));
    let other_code = code_of(&other.content);
    assert_ne!(shown.code, other_code, "two requests, two codes");
    assert_eq!(
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            &other_code,
            at(now),
        ),
        Err(ControlError::Refused {
            reason: "content_mismatch"
        }),
        "another request's code confirms nothing here"
    );
    assert!(control(&fx).is_empty(), "a refusal commits nothing");
    let submitted = answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            &code_of(&asked.content),
            at(now),
        ),
    );
    let committed = control(&fx);
    assert_eq!(committed.len(), 1, "one event per command");
    let event = &committed[0];
    assert_eq!(
        submitted,
        Submitted {
            event_id: member_text(event, "event_id")
                .unwrap_or_default()
                .to_owned(),
            seq: 1,
        }
    );
    assert_eq!(
        member_text(event, "event_type"),
        Some("ApprovalResponseSubmitted")
    );
    assert_eq!(member_text(event, "actor.kind"), Some("user"));
    assert_eq!(member_text(event, "actor.id"), Some(OWNER));
    assert_eq!(member_text(event, "payload.agent"), Some(AGENT));
    assert_eq!(
        member_text(event, "payload.approval"),
        Some(asked.approval.as_str())
    );
    assert_eq!(member_text(event, "payload.verdict"), Some("approved"));
    assert_eq!(
        member_text(event, "payload.content_hash"),
        Some(hash_of(&asked.content).as_str())
    );
    assert_eq!(member_text(event, "payload.responder"), Some(OWNER));
    assert_eq!(
        member(event, "payload.submitted_at").and_then(Value::as_int),
        u64::try_from(now).ok()
    );
    assert_eq!(
        member(event, "payload.step_up.authenticated_at").and_then(Value::as_int),
        u64::try_from(now).ok()
    );
    assert_eq!(
        member_text(event, "payload.step_up.method"),
        Some("cli_confirm")
    );
    assert_eq!(
        member_text(event, "payload.step_up.assertion_id"),
        Some("cli-assertion-1")
    );
    assert_eq!(
        fx.rows(&common::agent_stream(AGENT)).len(),
        before,
        "the CLI never writes an agent stream"
    );
}

/// The local refusals, each committing nothing: a wrong code, a deadline already passed, and an
/// approval the runtime already ended. The same approval with the right code in time commits.
#[test]
#[ignore = "pending E8-3"]
fn approve_refuses_locally_and_commits_nothing() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let ended = fx.ask(OTHER_AGENT, "4", ASKED_AT);
    fx.ended(&ended, "ApprovalCanceled");
    let mut ids = FixedIds::default();
    let right = answer("show", show(&fx.journal, &owner(), AGENT, &asked.approval)).code;
    assert_eq!(right, code_of(&asked.content));
    let cases = [
        (
            AGENT,
            &asked,
            "00000000".to_owned(),
            ASKED_AT + 30,
            "content_mismatch",
        ),
        (AGENT, &asked, right.clone(), asked.deadline, "late"),
        (
            OTHER_AGENT,
            &ended,
            code_of(&ended.content),
            ASKED_AT + 30,
            "not_pending",
        ),
    ];
    for (agent, target, code, now, reason) in cases {
        assert_eq!(
            approve(
                &mut fx.journal,
                &mut ids,
                &owner(),
                agent,
                &target.approval,
                &code,
                at(now)
            ),
            Err(ControlError::Refused { reason }),
            "{reason}"
        );
        assert!(control(&fx).is_empty(), "{reason}: nothing committed");
    }
    answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            &right,
            at(asked.deadline - 1),
        ),
    );
    assert_eq!(control(&fx).len(), 1);
}

/// PX-7, PX-10: `skip` takes the approval alone, needs no code, and commits a response with no
/// step-up; a skip of an approval already ended commits nothing.
#[test]
#[ignore = "pending E8-3"]
fn skip_commits_a_response_without_step_up() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let ended = fx.ask(AGENT, "4", ASKED_AT);
    fx.ended(&ended, "ApprovalTimedOut");
    let mut ids = FixedIds::default();
    answer("show", show(&fx.journal, &owner(), AGENT, &asked.approval));
    assert_eq!(
        skip(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &ended.approval,
            at(ASKED_AT + 10)
        ),
        Err(ControlError::Refused {
            reason: "not_pending"
        })
    );
    assert!(control(&fx).is_empty());
    answer(
        "skip",
        skip(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            at(ASKED_AT + 10),
        ),
    );
    let committed = control(&fx);
    assert_eq!(committed.len(), 1);
    assert_eq!(
        member_text(&committed[0], "payload.verdict"),
        Some("skipped")
    );
    assert_eq!(member(&committed[0], "payload.step_up"), Some(&Value::Null));
    assert_eq!(
        member_text(&committed[0], "payload.content_hash"),
        Some(hash_of(&asked.content).as_str())
    );
}

/// Rule 3: until the runtime records an `ApprovalResponded` naming the submitted event, the
/// outcome is "not recorded" and the message never says approved or sent; once it records an
/// admitted grant and an `act`, the message says it was sent and the gate still decides.
#[test]
#[ignore = "pending E8-3"]
fn the_outcome_is_only_what_the_runtime_recorded() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let mut ids = FixedIds::default();
    let submitted = answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            &code_of(&asked.content),
            at(ASKED_AT + 30),
        ),
    );
    let nothing = answer("outcome", outcome(&fx.journal, &owner(), AGENT, &submitted));
    assert_eq!(nothing, Outcome::NotRecorded);
    let waiting = answer("message", message(&nothing)).to_lowercase();
    assert!(
        !waiting.contains("approved") && !waiting.contains("sent"),
        "{waiting}"
    );
    assert!(
        waiting.contains("skipped"),
        "the default is stated: {waiting}"
    );

    fx.responded(&asked, &submitted.event_id, "approved", "admitted", None);
    fx.revalidated(&asked, "act", None);
    let acted = answer("outcome", outcome(&fx.journal, &owner(), AGENT, &submitted));
    assert_eq!(
        acted,
        Outcome::Admitted {
            revalidation: Some(Revalidated::Act)
        }
    );
    let sent = answer("message", message(&acted)).to_lowercase();
    assert!(sent.contains("sent") && sent.contains("gate"), "{sent}");
}

/// A refusal and a re-validation skip are reported with their reason codes and "nothing was
/// sent"; a response naming another submitted event is not this one's outcome.
#[test]
#[ignore = "pending E8-3"]
fn a_refusal_or_a_skip_reports_its_reason() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let mut ids = FixedIds::default();
    let submitted = answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            &code_of(&asked.content),
            at(ASKED_AT + 30),
        ),
    );
    fx.responded(
        &asked,
        &common::runtime_id(950),
        "approved",
        "admitted",
        None,
    );
    assert_eq!(
        answer("outcome", outcome(&fx.journal, &owner(), AGENT, &submitted)),
        Outcome::NotRecorded,
        "another event's record is not this one's"
    );
    fx.responded(
        &asked,
        &submitted.event_id,
        "approved",
        "refused",
        Some("step_up_stale"),
    );
    let refused = answer("outcome", outcome(&fx.journal, &owner(), AGENT, &submitted));
    assert_eq!(
        refused,
        Outcome::Refused {
            reason: "step_up_stale".to_owned()
        }
    );
    let said = answer("message", message(&refused)).to_lowercase();
    assert!(
        said.contains("step_up_stale") && said.contains("nothing was sent"),
        "{said}"
    );

    let skipped = Outcome::Admitted {
        revalidation: Some(Revalidated::Skip {
            reason: "drift".to_owned(),
        }),
    };
    let said = answer("message", message(&skipped)).to_lowercase();
    assert!(
        said.contains("drift") && said.contains("nothing was sent"),
        "{said}"
    );
    let _ = TIMEOUT_S;
}

/// Approves the fixture's request through a journal set up to fail once, and returns what was
/// submitted, every event on the control stream, and the ids each append attempt carried.
fn approve_through(
    fault: fn(&mut common::Journal),
) -> (Submitted, Vec<Value>, Vec<Vec<String>>, FixedIds) {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let mut ids = FixedIds::default();
    fault(&mut fx.journal);
    let submitted = answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            &code_of(&asked.content),
            at(ASKED_AT + 30),
        ),
    );
    let attempts = fx.journal.attempts.clone();
    (submitted, control(&fx), attempts, ids)
}

/// DEC-155 item 2: the control-stream event id is the idempotency key, so it is minted once per
/// command and every append attempt carries that same id.
fn one_id_for_every_attempt(submitted: &Submitted, attempts: &[Vec<String>], ids: &FixedIds) {
    assert_eq!(ids.events, 1, "one event id minted per command");
    assert!(
        attempts.len() >= 2,
        "the fault forced a retry: {attempts:?}"
    );
    for attempt in attempts {
        assert_eq!(
            attempt,
            &vec![submitted.event_id.clone()],
            "a retry carries the same event id"
        );
    }
}

/// DEC-155 item 5, DEC-257 item 16: the CLI takes a new writer epoch per invocation, and when
/// another writer fences it, retries with a fresh read rather than interleaving, so exactly one
/// event is committed, under the one event id.
#[test]
#[ignore = "pending E8-3"]
fn a_fenced_append_is_retried_and_commits_once() {
    let (submitted, committed, attempts, ids) = approve_through(|j| j.fence_next = 1);
    one_id_for_every_attempt(&submitted, &attempts, &ids);
    assert_eq!(committed.len(), 1, "one event despite the fence");
    assert_eq!(submitted.seq, 1);
    assert_eq!(
        member_text(&committed[0], "event_id"),
        Some(submitted.event_id.as_str())
    );
}

/// DEC-257 item 16: an append behind the head (another event landed after the CLI read it) is
/// retried at the new head, so the command commits once, after the other event.
#[test]
#[ignore = "pending E8-3"]
fn a_command_behind_the_head_retries_and_commits_once() {
    let (submitted, committed, attempts, ids) = approve_through(|j| j.behind_next = 1);
    one_id_for_every_attempt(&submitted, &attempts, &ids);
    assert_eq!(
        committed.len(),
        2,
        "the other writer's event, then this one"
    );
    assert_ne!(
        member_text(&committed[0], "event_id"),
        Some(submitted.event_id.as_str())
    );
    assert_eq!(submitted.seq, 2);
    assert_eq!(
        member_text(&committed[1], "event_id"),
        Some(submitted.event_id.as_str())
    );
}

/// Journal spec §5.1, DEC-155 item 2: an append whose answer is lost (`Ambiguous`) did commit, and
/// a retry of the same draft is `AlreadyCommitted`, so the command reports the stored event and
/// commits nothing twice. A fresh id per attempt would commit the answer twice.
#[test]
#[ignore = "pending E8-3"]
fn a_lost_answer_is_retried_under_the_same_id_and_commits_once() {
    let (submitted, committed, attempts, ids) = approve_through(|j| j.ambiguous_next = 1);
    one_id_for_every_attempt(&submitted, &attempts, &ids);
    assert_eq!(committed.len(), 1, "the retry found the stored event");
    assert_eq!(submitted.seq, 1);
    assert_eq!(
        member_text(&committed[0], "event_id"),
        Some(submitted.event_id.as_str())
    );
}

/// Rule 3, FR-6.6, mandate spec §6.4: a counted grant (short of the quorum) and an admitted grant
/// not yet re-validated have both sent nothing. Their messages say so and never call the action
/// approved.
#[test]
#[ignore = "pending E8-3"]
fn the_outcomes_that_sent_nothing_say_so() {
    for outcome in [Outcome::Counted, Outcome::Admitted { revalidation: None }] {
        let said = answer("message", message(&outcome)).to_lowercase();
        assert!(said.contains("nothing was sent"), "{outcome:?}: {said}");
        assert!(!said.contains("approved"), "{outcome:?}: {said}");
    }
}
