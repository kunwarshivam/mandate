//! `mandate approvals` (M7 tests PR 4 of 4; the brief's "The CLI control surface"; mandate spec
//! §6.4; DEC-155 item 5, DEC-257 items 5 and 9).
//!
//! The journal is `tests/common`'s own in-memory `Journal`, whose agent streams hold approval
//! events as the runtime writes them. Every expectation is computed here from the fixture: the content hash and the code from
//! the content object's canonical bytes, the deadlines from the fixture's own clock, and what the
//! CLI committed from the control stream's stored bytes. Each "commits nothing" case is paired with
//! the one that commits, so a command that does nothing passes none of them.
//!
//! The #397 review's follow-ups (DEC-290) add: the closed member set of every response the CLI
//! commits; a refusal's message that says whether the deadline has passed; a pause between append
//! attempts that grows and differs between commands; and a re-run of an answer that did commit
//! finding it, while a re-answer after the runtime refused one is committed. The tests of what is
//! not implemented yet are pending E8-3 and fail on the command's `ControlError::Unimplemented`
//! (DEC-77, DEC-110).

mod common;

use std::collections::BTreeSet;
use std::time::Duration;

use common::{
    AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OTHER_AGENT, OWNER, TIMEOUT_S, at, body, code_of,
    hash_of, member, member_text, owner,
};
use mandate_canon::Value;
use mandate_cli::approvals::{
    Ended, Outcome, Revalidated, State, approve, list, message, outcome, show, skip,
};
use mandate_cli::control::{ControlError, ControlJournal, Submitted, backoff};
use mandate_journal::AppendOutcome;

fn answer<T>(what: &str, result: Result<T, ControlError>) -> T {
    result.unwrap_or_else(|e| panic!("{what} answers, not: {e}"))
}

/// The control stream's events, as stored.
fn control(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL).iter().map(body).collect()
}

/// Live: journal spec §5.1, as the fixture journal runs it. A retry of the same event id with the
/// same bytes is `AlreadyCommitted` and stores nothing; with different bytes it is refused as
/// `IdempotencyConflict` naming the stored `seq`, and stores nothing either. The CLI never sends
/// such a retry (it mints its draft once), so this pins the fixture rather than the CLI.
#[test]
fn the_fixture_journal_refuses_a_changed_retry_of_a_stored_id() {
    let mut journal = common::Journal::default();
    let control = common::stream(CONTROL);
    let draft = |command: &str| {
        mandate_canon::to_canonical(&common::object(&[
            ("event_id", common::text("20000000000000000000000001")),
            ("event_type", common::text("OwnerCommandIssued")),
            (
                "payload",
                common::object(&[("command", common::text(command))]),
            ),
        ]))
    };
    let first = draft("pause");
    let epoch = journal.take_ownership(&control).unwrap();
    let committed = journal
        .append(&control, 0, epoch, at(ASKED_AT).at, &[&first])
        .unwrap();
    assert!(
        matches!(committed, AppendOutcome::Committed(_)),
        "{committed:?}"
    );
    let same = journal
        .append(&control, 0, epoch, at(ASKED_AT).at, &[&first])
        .unwrap();
    assert!(
        matches!(&same, AppendOutcome::AlreadyCommitted(rows) if rows.len() == 1 && rows[0].seq == 1),
        "{same:?}"
    );
    let changed = journal
        .append(&control, 1, epoch, at(ASKED_AT).at, &[&draft("resume")])
        .unwrap();
    assert_eq!(
        changed,
        AppendOutcome::IdempotencyConflict { stored_seq: 1 }
    );
    assert_eq!(
        journal.rows(&control).unwrap().len(),
        1,
        "nothing stored twice"
    );
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
    let counted = fx.ask(AGENT, "3", ASKED_AT);
    fx.responded(
        &counted,
        &common::runtime_id(904),
        "approved",
        "counted",
        None,
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
    assert_eq!(
        state(&counted.approval),
        Some(State::Pending),
        "a grant counted short of the quorum leaves the approval pending (mandate spec §6.4)"
    );
}

/// D6, P8: `show` renders the content object exactly as committed and the code for its hash,
/// computed here from the canonical bytes; an approval the stream does not hold is `not_pending`.
#[test]
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
    let waiting = answer("message", message(&nothing, at(ASKED_AT + 30))).to_lowercase();
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
    let sent = answer("message", message(&acted, at(ASKED_AT + 30))).to_lowercase();
    assert!(sent.contains("sent") && sent.contains("gate"), "{sent}");
}

/// A refusal is reported with its reason code and the request's deadline, and a re-validation skip
/// with its reason and "nothing was sent"; a response naming another submitted event is not this
/// one's outcome. What a refusal's message says is
/// `a_refusal_says_whether_the_deadline_has_passed`'s.
#[test]
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
            reason: "step_up_stale".to_owned(),
            deadline_s: asked.deadline,
        }
    );

    let skipped = Outcome::Admitted {
        revalidation: Some(Revalidated::Skip {
            reason: "drift".to_owned(),
        }),
    };
    let said = answer("message", message(&skipped, at(ASKED_AT + 30))).to_lowercase();
    assert!(
        said.contains("drift") && said.contains("nothing was sent"),
        "{said}"
    );
    let _ = TIMEOUT_S;
    let other = fx.ask(AGENT, "4", ASKED_AT);
    let second = answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &other.approval,
            &code_of(&other.content),
            at(ASKED_AT + 50),
        ),
    );
    fx.responded(&other, &second.event_id, "approved", "counted", None);
    assert_eq!(
        answer("outcome", outcome(&fx.journal, &owner(), AGENT, &second)),
        Outcome::Counted,
        "a grant counted short of the quorum is its own outcome, not a refusal"
    );
}

/// What one approval through a faulty journal left: what was submitted, every event on the control
/// stream, the ids each append attempt carried, and the pauses between attempts.
struct Through {
    submitted: Submitted,
    committed: Vec<Value>,
    attempts: Vec<Vec<String>>,
    waits: Vec<Duration>,
}

/// Approves the fixture's request through a journal set up to fail once.
fn approve_through(fault: fn(&mut common::Journal)) -> Through {
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
    Through {
        submitted,
        committed: control(&fx),
        attempts: fx.journal.attempts.clone(),
        waits: fx.journal.waits.clone(),
    }
}

/// DEC-155 item 2: the control-stream event id is the idempotency key, so every append attempt
/// carries the one id the command derived. The pauses between attempts are the backoff tests'.
fn one_id_for_every_attempt(through: &Through) {
    let Through {
        submitted,
        attempts,
        ..
    } = through;
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
fn a_fenced_append_is_retried_and_commits_once() {
    let through = approve_through(|j| j.fence_next = 1);
    one_id_for_every_attempt(&through);
    let Through {
        submitted,
        committed,
        ..
    } = through;
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
fn a_command_behind_the_head_retries_and_commits_once() {
    let through = approve_through(|j| j.behind_next = 1);
    one_id_for_every_attempt(&through);
    let Through {
        submitted,
        committed,
        ..
    } = through;
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
fn a_lost_answer_is_retried_under_the_same_id_and_commits_once() {
    let through = approve_through(|j| j.ambiguous_next = 1);
    one_id_for_every_attempt(&through);
    let Through {
        submitted,
        committed,
        ..
    } = through;
    assert_eq!(committed.len(), 1, "the retry found the stored event");
    assert_eq!(submitted.seq, 1);
    assert_eq!(
        member_text(&committed[0], "event_id"),
        Some(submitted.event_id.as_str())
    );
}

/// The #397 review, minor 4; DEC-290 item 6: the pause before each retry grows, so a writer that
/// keeps meeting another backs off instead of fencing it again at once; and the pauses stay
/// bounded, none above 4 s and all seven a command can take together within 30 s, so the owner is
/// answered. The append loop pauses exactly `backoff`'s answer before each retry, and never before
/// the first attempt.
#[test]
fn retries_back_off_between_attempts() {
    let through = approve_through(|j| j.fence_next = 4);
    one_id_for_every_attempt(&through);
    let id = through.submitted.event_id.clone();
    let schedule: Vec<Duration> = (1..=7)
        .map(|retry| answer("backoff", backoff(retry, &id)))
        .collect();
    for pair in schedule.windows(2).take(4) {
        assert!(pair[1] > pair[0], "each pause is longer: {schedule:?}");
    }
    assert!(
        schedule
            .iter()
            .all(|w| *w > Duration::ZERO && *w <= Duration::from_secs(4)),
        "{schedule:?}"
    );
    assert!(
        schedule.iter().sum::<Duration>() <= Duration::from_secs(30),
        "{schedule:?}"
    );
    assert_eq!(
        through.waits,
        schedule[..4].to_vec(),
        "four fences, four pauses, each the schedule's"
    );
}

/// DEC-290 item 6: two commands racing for the stream pause for different times, so they stop
/// meeting: the grant and the skip of one request, each fenced once in its own journal, are given
/// different first pauses, and each journal saw its own.
#[test]
fn two_racing_commands_back_off_differently() {
    let first_pause = |grant: bool| {
        let mut fx = Fixture::new();
        let asked = fx.ask(AGENT, "10", ASKED_AT);
        let mut ids = FixedIds::default();
        fx.journal.fence_next = 1;
        let now = at(ASKED_AT + 30);
        let result = if grant {
            approve(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                &asked.approval,
                &code_of(&asked.content),
                now,
            )
        } else {
            skip(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                &asked.approval,
                now,
            )
        };
        let submitted = answer("answer", result);
        let scheduled = answer("backoff", backoff(1, &submitted.event_id));
        assert_eq!(fx.journal.waits, vec![scheduled]);
        scheduled
    };
    assert_ne!(first_pause(true), first_pause(false));
}

/// DEC-290, the #397 review, minor 4: an answer whose append committed but whose every answer was
/// lost gives up after a bounded number of attempts, naming the event it may have committed; a
/// re-run, with a new process's ids and a later clock, finds that event, commits nothing twice, and
/// spends no new step-up.
#[test]
fn an_exhausted_answer_names_its_event_and_a_re_run_finds_it() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let code = code_of(&asked.content);
    fx.journal.lost_for_good = true;
    let first = approve(
        &mut fx.journal,
        &mut FixedIds::default(),
        &owner(),
        AGENT,
        &asked.approval,
        &code,
        at(ASKED_AT + 30),
    );
    let committed = control(&fx);
    assert_eq!(committed.len(), 1, "the first run did commit");
    let stored = member_text(&committed[0], "event_id")
        .unwrap_or_default()
        .to_owned();
    match &first {
        Err(ControlError::Journal(why)) => assert!(
            why.contains(&stored) && why.contains("if nothing else has been committed since"),
            "{why}"
        ),
        other => panic!("the first run gives up, not: {other:?}"),
    }
    assert!(fx.journal.attempts.len() >= 2, "it retried");
    fx.journal.recover();
    let mut ids = FixedIds::default();
    let again = approve(
        &mut fx.journal,
        &mut ids,
        &owner(),
        AGENT,
        &asked.approval,
        &code,
        at(ASKED_AT + 35),
    );
    assert_eq!(
        again,
        Ok(Submitted {
            event_id: stored,
            seq: 1
        })
    );
    assert_eq!(control(&fx).len(), 1, "committed once");
    assert_eq!(ids.assertions, 0, "the re-run spent no step-up");
}

/// The #409 review, minor 1; DEC-290 item 5: a fence, then another writer's event landing between
/// the command's read of the head and its append, then an append that commits but whose every
/// answer is lost. The report names the event and says a re-run finds it only if nothing else has
/// been committed since; nothing has, so the re-run finds it, though the other event moved the head
/// the command was decided at, and commits nothing twice.
#[test]
fn a_re_run_finds_its_event_after_another_writer_moved_the_head() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let code = code_of(&asked.content);
    fx.journal.fence_next = 1;
    fx.journal.behind_next = 1;
    fx.journal.lost_for_good = true;
    let first = approve(
        &mut fx.journal,
        &mut FixedIds::default(),
        &owner(),
        AGENT,
        &asked.approval,
        &code,
        at(ASKED_AT + 30),
    );
    let committed = control(&fx);
    assert_eq!(
        committed.len(),
        2,
        "the other writer's event, then this one"
    );
    let stored = member_text(&committed[1], "event_id")
        .unwrap_or_default()
        .to_owned();
    match &first {
        Err(ControlError::Journal(why)) => assert!(
            why.contains(&stored) && why.contains("if nothing else has been committed since"),
            "{why}"
        ),
        other => panic!("the first run gives up, not: {other:?}"),
    }
    fx.journal.recover();
    let mut ids = FixedIds::default();
    let again = approve(
        &mut fx.journal,
        &mut ids,
        &owner(),
        AGENT,
        &asked.approval,
        &code,
        at(ASKED_AT + 35),
    );
    assert_eq!(
        again,
        Ok(Submitted {
            event_id: stored,
            seq: 2
        })
    );
    assert_eq!(control(&fx).len(), 2, "committed once");
    assert_eq!(ids.assertions, 0, "the re-run spent no step-up");
}

/// DEC-290, the M7 brief's lifecycle: until the runtime records an answer, the same grant again is
/// that answer and commits nothing. Once the runtime has refused it (here for a stale step-up), the
/// request is still pending and the owner may answer again before the deadline; that answer is a
/// new event with fresh evidence, never taken for the refused one.
#[test]
fn a_re_answer_after_the_runtime_refused_is_committed_anew() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let mut ids = FixedIds::default();
    let code = code_of(&asked.content);
    let mut grant = |fx: &mut Fixture, second: i64| {
        answer(
            "approve",
            approve(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                &asked.approval,
                &code,
                at(second),
            ),
        )
    };
    let first = grant(&mut fx, ASKED_AT + 30);
    assert_eq!(grant(&mut fx, ASKED_AT + 31), first, "not yet recorded");
    assert_eq!(control(&fx).len(), 1);
    fx.responded(
        &asked,
        &first.event_id,
        "approved",
        "refused",
        Some("step_up_stale"),
    );
    let second = grant(&mut fx, ASKED_AT + 40);
    assert_ne!(second.event_id, first.event_id);
    assert_eq!(second.seq, 2);
    let committed = control(&fx);
    assert_eq!(committed.len(), 2);
    assert_eq!(
        member_text(&committed[1], "payload.step_up.assertion_id"),
        Some("cli-assertion-2"),
        "fresh evidence"
    );
}

/// The #397 review, minor 5: a refusal's message says whether the deadline has passed. Before it,
/// the owner is told they may answer again; at or after it, that the deadline has passed and the
/// action is skipped, and never offered another answer. Both name the reason and that nothing was
/// sent.
#[test]
fn a_refusal_says_whether_the_deadline_has_passed() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let submitted = answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut FixedIds::default(),
            &owner(),
            AGENT,
            &asked.approval,
            &code_of(&asked.content),
            at(ASKED_AT + 30),
        ),
    );
    fx.responded(
        &asked,
        &submitted.event_id,
        "approved",
        "refused",
        Some("step_up_stale"),
    );
    let refused = answer("outcome", outcome(&fx.journal, &owner(), AGENT, &submitted));
    let before = answer("message", message(&refused, at(asked.deadline - 1))).to_lowercase();
    let after = answer("message", message(&refused, at(asked.deadline))).to_lowercase();
    for said in [&before, &after] {
        assert!(
            said.contains("step_up_stale") && said.contains("nothing was sent"),
            "{said}"
        );
    }
    assert!(before.contains("answer again"), "{before}");
    assert!(!before.contains("has passed"), "{before}");
    assert!(
        after.contains("deadline has passed") && after.contains("skipped"),
        "{after}"
    );
    assert!(!after.contains("answer again"), "{after}");
}

/// `ApprovalResponseSubmitted`'s members (journal spec §9), as DEC-290 closes them until the
/// catalogue does.
const RESPONSE: [&str; 8] = [
    "agent",
    "approval",
    "content_hash",
    "responder",
    "role",
    "step_up",
    "submitted_at",
    "verdict",
];

fn members(event: &Value, path: &str) -> BTreeSet<String> {
    member(event, path)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{path} is an object in {event:?}"))
        .keys()
        .map(|k| k.as_str().to_owned())
        .collect()
}

/// The #397 review, minor 1; DEC-290: a grant and a skip each commit exactly the response's
/// members, and the grant's evidence exactly its three, so a member nobody named (the code the
/// owner typed, the content itself) fails here.
#[test]
fn every_response_payload_has_exactly_its_members() {
    let mut fx = Fixture::new();
    let granted = fx.ask(AGENT, "10", ASKED_AT);
    let skipped = fx.ask(AGENT, "4", ASKED_AT);
    let mut ids = FixedIds::default();
    answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &granted.approval,
            &code_of(&granted.content),
            at(ASKED_AT + 30),
        ),
    );
    answer(
        "skip",
        skip(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &skipped.approval,
            at(ASKED_AT + 31),
        ),
    );
    let committed = control(&fx);
    assert_eq!(committed.len(), 2);
    let expected: BTreeSet<String> = RESPONSE.iter().map(|m| (*m).to_owned()).collect();
    for event in &committed {
        assert_eq!(members(event, "payload"), expected, "{event:?}");
    }
    let evidence: BTreeSet<String> = ["assertion_id", "authenticated_at", "method"]
        .iter()
        .map(|m| (*m).to_owned())
        .collect();
    assert_eq!(members(&committed[0], "payload.step_up"), evidence);
    assert_eq!(member(&committed[1], "payload.step_up"), Some(&Value::Null));
}

/// Rule 3, FR-6.6, mandate spec §6.4: a counted grant (short of the quorum) and an admitted grant
/// not yet re-validated have both sent nothing. Their messages say so and never call the action
/// approved.
#[test]
fn the_outcomes_that_sent_nothing_say_so() {
    for outcome in [Outcome::Counted, Outcome::Admitted { revalidation: None }] {
        let said = answer("message", message(&outcome, at(ASKED_AT))).to_lowercase();
        assert!(said.contains("nothing was sent"), "{outcome:?}: {said}");
        assert!(!said.contains("approved"), "{outcome:?}: {said}");
    }
}
