//! `mandate agent` (M7 tests PR 4 of 4; the brief's "The CLI control surface"; mandate spec §6.1;
//! DEC-155 items 4 and 5, DEC-158 option (c), DEC-257 item 5).
//!
//! Every command commits exactly one control-stream event and nothing else. Pause needs no code.
//! Resume, Stop, and acknowledge need the code `code` prints and commit nothing without it. A kill
//! switch and an owner exit are never refused here: without the right code they are committed with
//! no step-up evidence, which the runtime still applies without the owner-exit privilege (rule 13).
//! Each code is bound to everything the owner confirms (the command, the agent, the instrument, the
//! confirmed bid, bid size and floor, the acknowledged event, the kill switch's scope) and to the
//! control stream's head, which this file checks by changing one of them at a time. No command
//! writes an agent stream.
//!
//! The #397 review's follow-ups (DEC-290) add: the closed member set of every payload the CLI
//! commits, so nothing the owner typed or anything else can creep into the journal; Stop's
//! `--release` choice and the record of the warning shown (DEC-136); `status`'s restrictions; and an
//! event id derived from the owner's choice and the head, so a re-run of a command that did commit
//! finds it. The tests of what is not implemented yet are pending E8-3 and fail on the command's
//! `ControlError::Unimplemented` (DEC-77, DEC-110).

mod common;

use std::collections::BTreeSet;

use common::{
    ACCOUNT, AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OTHER_AGENT, OWNER, at, body, member,
    member_text, owner,
};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_cli::agent::{
    Command, Confirmed, RELEASE_WARNING, Restriction, Scope, code, command, kill, kill_code, status,
};
use mandate_cli::approvals::approve;
use mandate_cli::control::{ControlError, Submitted};

fn answer<T>(what: &str, result: Result<T, ControlError>) -> T {
    result.unwrap_or_else(|e| panic!("{what} answers, not: {e}"))
}

fn control(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL).iter().map(body).collect()
}

/// The one event a command committed, checked as the owner's on the control stream; and since
/// the fixtures here write no agent stream, none was written by the command either.
fn committed_one(fx: &Fixture, before: usize) -> Value {
    let events = control(fx);
    assert_eq!(events.len(), before + 1, "one event per command");
    let event = events[before].clone();
    assert_eq!(member_text(&event, "actor.kind"), Some("user"));
    assert_eq!(member_text(&event, "actor.id"), Some(OWNER));
    for agent in [AGENT, OTHER_AGENT] {
        assert!(
            fx.rows(&common::agent_stream(agent)).is_empty(),
            "the CLI never writes an agent stream"
        );
    }
    event
}

fn confirmed(bid: &str, bid_size: &str, floor: &str) -> Confirmed {
    Confirmed {
        bid: bid.to_owned(),
        bid_size: bid_size.to_owned(),
        floor: floor.to_owned(),
    }
}

fn exit_of(instrument: &str, bid: Option<Confirmed>) -> Command {
    Command::Exit {
        instrument: instrument.to_owned(),
        bid,
    }
}

fn exit_aapl(bid: bool) -> Command {
    exit_of("AAPL", bid.then(|| confirmed("154.1", "300", "150")))
}

/// PX-4: pause needs no code, has none, and commits an `OwnerCommandIssued` with no step-up.
#[test]
fn pause_needs_no_code() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    assert_eq!(
        answer("code", code(&fx.journal, &owner(), AGENT, &Command::Pause)),
        None
    );
    answer(
        "pause",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &Command::Pause,
            None,
            at(ASKED_AT),
        ),
    );
    let event = committed_one(&fx, 0);
    assert_eq!(
        member_text(&event, "event_type"),
        Some("OwnerCommandIssued")
    );
    assert_eq!(member_text(&event, "payload.command"), Some("pause"));
    assert_eq!(member_text(&event, "payload.scope"), Some("agent"));
    assert_eq!(member_text(&event, "payload.subject"), Some(AGENT));
    assert_eq!(member(&event, "payload.step_up"), Some(&Value::Null));
    assert_eq!(
        member(&event, "payload.submitted_at").and_then(Value::as_int),
        u64::try_from(ASKED_AT).ok()
    );
}

/// Mandate spec §6.1: resume, Stop, and acknowledge need the code. A wrong or missing one is
/// refused locally and commits nothing; the right one commits the command with `cli_confirm`
/// evidence authenticated when the owner ran it.
#[test]
fn resume_stop_and_acknowledge_need_the_code() {
    for (cmd, event_type, name) in [
        (Command::Resume, "OwnerCommandIssued", Some("resume")),
        (
            Command::Stop { release: false },
            "OwnerCommandIssued",
            Some("stop"),
        ),
        (
            Command::Acknowledge {
                event: common::runtime_id(42),
            },
            "OwnerAcknowledged",
            None,
        ),
    ] {
        let mut fx = Fixture::new();
        let mut ids = FixedIds::default();
        let right = answer("code", code(&fx.journal, &owner(), AGENT, &cmd))
            .unwrap_or_else(|| panic!("{cmd:?} has a code"));
        for typed in [None, Some("00000000")] {
            assert_eq!(
                command(
                    &mut fx.journal,
                    &mut ids,
                    &owner(),
                    AGENT,
                    &cmd,
                    typed,
                    at(ASKED_AT)
                ),
                Err(ControlError::Refused {
                    reason: "step_up_missing"
                }),
                "{cmd:?} with {typed:?}"
            );
            assert!(control(&fx).is_empty());
        }
        answer(
            "command",
            command(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                &cmd,
                Some(&right),
                at(ASKED_AT),
            ),
        );
        let event = committed_one(&fx, 0);
        assert_eq!(member_text(&event, "event_type"), Some(event_type));
        if let Some(name) = name {
            assert_eq!(member_text(&event, "payload.command"), Some(name));
        }
        assert_eq!(
            member_text(&event, "payload.step_up.method"),
            Some("cli_confirm")
        );
        assert_eq!(
            member(&event, "payload.step_up.authenticated_at").and_then(Value::as_int),
            u64::try_from(ASKED_AT).ok()
        );
    }
}

/// A code is bound to what it confirms and to the control stream's head: the resume code is not
/// the Stop code, and once another command lands the old code no longer confirms.
#[test]
fn a_code_is_bound_to_its_command_and_the_head() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let resume = answer("code", code(&fx.journal, &owner(), AGENT, &Command::Resume));
    let stop = answer(
        "code",
        code(
            &fx.journal,
            &owner(),
            AGENT,
            &Command::Stop { release: false },
        ),
    );
    assert_ne!(resume, stop);
    answer(
        "pause",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &Command::Pause,
            None,
            at(ASKED_AT),
        ),
    );
    assert_ne!(
        answer("code", code(&fx.journal, &owner(), AGENT, &Command::Resume)),
        resume,
        "the head moved"
    );
    assert_eq!(
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &Command::Resume,
            resume.as_deref(),
            at(ASKED_AT + 1)
        ),
        Err(ControlError::Refused {
            reason: "step_up_missing"
        })
    );
    assert_eq!(control(&fx).len(), 1);
}

/// DEC-158 option (c), rule 13: a kill switch is never refused. With the scope's code it carries
/// step-up evidence; with a wrong code, or another scope's, it is still committed, with none.
#[test]
fn a_kill_switch_is_committed_with_or_without_the_code() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let agent = Scope::Agent(AGENT.to_owned());
    let workspace_code = answer(
        "kill code",
        kill_code(&fx.journal, &owner(), &Scope::Workspace),
    );
    let agent_code = answer("kill code", kill_code(&fx.journal, &owner(), &agent));
    assert_ne!(workspace_code, agent_code, "a code confirms one scope");

    answer(
        "kill",
        kill(
            &mut fx.journal,
            &mut ids,
            &owner(),
            &agent,
            Some(&workspace_code),
            at(ASKED_AT),
        ),
    );
    let event = committed_one(&fx, 0);
    assert_eq!(member_text(&event, "payload.command"), Some("kill_switch"));
    assert_eq!(member_text(&event, "payload.scope"), Some("agent"));
    assert_eq!(member_text(&event, "payload.subject"), Some(AGENT));
    assert_eq!(member(&event, "payload.step_up"), Some(&Value::Null));

    let fresh = answer("kill code", kill_code(&fx.journal, &owner(), &agent));
    assert_ne!(
        fresh, agent_code,
        "the head moved, so the agent scope's old code confirms nothing"
    );
    answer(
        "kill",
        kill(
            &mut fx.journal,
            &mut ids,
            &owner(),
            &agent,
            Some(&fresh),
            at(ASKED_AT + 1),
        ),
    );
    let event = committed_one(&fx, 1);
    assert_eq!(
        member_text(&event, "payload.step_up.method"),
        Some("cli_confirm")
    );

    answer(
        "kill",
        kill(
            &mut fx.journal,
            &mut ids,
            &owner(),
            &Scope::Workspace,
            None,
            at(ASKED_AT + 2),
        ),
    );
    let event = committed_one(&fx, 2);
    assert_eq!(member_text(&event, "payload.scope"), Some("workspace"));
    assert_eq!(
        member_text(&event, "payload.subject"),
        Some(common::WORKSPACE)
    );
}

/// Rule 13, the #281 obligation's CLI half: an owner exit is never refused here. Without the right
/// code it is committed with no step-up and the bid as confirmed, so the runtime routes it for the
/// regular session; with the code it carries both the confirmed bid and the evidence, the one shape
/// that unlocks the privilege.
#[test]
fn an_owner_exit_is_committed_with_or_without_the_code() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    answer(
        "exit",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &exit_aapl(true),
            Some("00000000"),
            at(ASKED_AT),
        ),
    );
    let event = committed_one(&fx, 0);
    assert_eq!(member_text(&event, "payload.command"), Some("owner_exit"));
    assert_eq!(member_text(&event, "payload.scope"), Some("instrument"));
    assert_eq!(member_text(&event, "payload.subject"), Some("AAPL"));
    assert_eq!(member_text(&event, "payload.agent"), Some(AGENT));
    assert_eq!(member_text(&event, "payload.bid"), Some("154.1"));
    assert_eq!(member_text(&event, "payload.bid_size"), Some("300"));
    assert_eq!(member_text(&event, "payload.floor"), Some("150"));
    assert_eq!(member(&event, "payload.step_up"), Some(&Value::Null));

    let right = answer("code", code(&fx.journal, &owner(), AGENT, &exit_aapl(true)))
        .unwrap_or_else(|| panic!("an owner exit has a code"));
    answer(
        "exit",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &exit_aapl(true),
            Some(&right),
            at(ASKED_AT + 1),
        ),
    );
    let event = committed_one(&fx, 1);
    assert_eq!(member_text(&event, "payload.bid"), Some("154.1"));
    assert_eq!(member_text(&event, "payload.bid_size"), Some("300"));
    assert_eq!(member_text(&event, "payload.floor"), Some("150"));
    assert_eq!(
        member_text(&event, "payload.step_up.method"),
        Some("cli_confirm")
    );
    assert_eq!(
        member(&event, "payload.step_up.authenticated_at").and_then(Value::as_int),
        u64::try_from(ASKED_AT + 1).ok()
    );
}

/// DEC-257 item 13, mandate spec §6.1, DEC-66: a code is bound to everything the owner confirms.
/// Changing only the agent, the instrument, the confirmed bid, bid size or floor, whether a bid was
/// confirmed at all, or the acknowledged event gives a different code; and a code typed for one
/// confirmed bid commits a different bid without step-up evidence, so the privilege is never
/// carried by another bid's code.
#[test]
fn a_code_is_bound_to_every_field_it_confirms() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let of = |agent: &str, cmd: &Command, fx: &Fixture| {
        answer("code", code(&fx.journal, &owner(), agent, cmd))
            .unwrap_or_else(|| panic!("{cmd:?} has a code"))
    };
    let ack = |n: u64| Command::Acknowledge {
        event: common::runtime_id(n),
    };
    let base = of(AGENT, &exit_aapl(true), &fx);
    let changed = [
        ("the agent", of(OTHER_AGENT, &exit_aapl(true), &fx)),
        (
            "the instrument",
            of(
                AGENT,
                &exit_of("MSFT", Some(confirmed("154.1", "300", "150"))),
                &fx,
            ),
        ),
        (
            "the bid",
            of(
                AGENT,
                &exit_of("AAPL", Some(confirmed("154.2", "300", "150"))),
                &fx,
            ),
        ),
        (
            "the bid size",
            of(
                AGENT,
                &exit_of("AAPL", Some(confirmed("154.1", "301", "150"))),
                &fx,
            ),
        ),
        (
            "the floor",
            of(
                AGENT,
                &exit_of("AAPL", Some(confirmed("154.1", "300", "149"))),
                &fx,
            ),
        ),
        (
            "whether a bid was confirmed",
            of(AGENT, &exit_aapl(false), &fx),
        ),
    ];
    for (what, other) in &changed {
        assert_ne!(&base, other, "changing only {what} changes the code");
    }
    assert_ne!(
        of(AGENT, &ack(42), &fx),
        of(AGENT, &ack(43), &fx),
        "the acknowledged event"
    );
    assert_ne!(
        of(AGENT, &Command::Resume, &fx),
        of(OTHER_AGENT, &Command::Resume, &fx),
        "the agent, for a resume"
    );

    let lower_bid = exit_of("AAPL", Some(confirmed("150", "300", "150")));
    answer(
        "exit",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &lower_bid,
            Some(&base),
            at(ASKED_AT),
        ),
    );
    let event = committed_one(&fx, 0);
    assert_eq!(member_text(&event, "payload.bid"), Some("150"));
    assert_eq!(
        member(&event, "payload.step_up"),
        Some(&Value::Null),
        "another bid's code carries no evidence for this one"
    );
    let other_events_code = of(AGENT, &ack(43), &fx);
    assert_eq!(
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &ack(42),
            Some(&other_events_code),
            at(ASKED_AT + 1),
        ),
        Err(ControlError::Refused {
            reason: "step_up_missing"
        }),
        "another event's acknowledgment code confirms nothing"
    );
    assert_eq!(control(&fx).len(), 1);
}

/// `status` reads the agent stream: the last mode change, whether it was the startup hold, and the
/// approvals still pending; with nothing on the account stream it shows no restriction, and it
/// commits nothing.
#[test]
fn status_reads_the_mode_the_hold_and_the_pending_count() {
    let mut fx = Fixture::new();
    let first = fx.ask(AGENT, "10", ASKED_AT);
    fx.ask(AGENT, "5", ASKED_AT);
    fx.ended(&first, "ApprovalCanceled");
    fx.mode(AGENT, "paused", "awaiting_reconciliation");
    let seen = answer("status", status(&fx.journal, &owner(), AGENT, ACCOUNT));
    assert_eq!(seen.mode, "paused");
    assert!(seen.startup_hold);
    assert_eq!(seen.pending_approvals, 1);
    fx.mode(AGENT, "normal", "restriction_changed");
    let seen = answer("status", status(&fx.journal, &owner(), AGENT, ACCOUNT));
    assert_eq!(seen.mode, "normal");
    assert!(!seen.startup_hold);
    assert!(seen.restrictions.is_empty());
    assert!(control(&fx).is_empty());
}

/// Mandate spec §6.1: step-up evidence is single-use, so each gesture draws a fresh assertion id.
/// A resume and a Stop committed one after the other carry two different ids, the ones the
/// injected `Ids` minted, and nothing else.
#[test]
fn each_gesture_draws_a_fresh_assertion() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    for (n, cmd) in [Command::Resume, Command::Stop { release: false }]
        .into_iter()
        .enumerate()
    {
        let right = answer("code", code(&fx.journal, &owner(), AGENT, &cmd))
            .unwrap_or_else(|| panic!("{cmd:?} has a code"));
        answer(
            "command",
            command(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                &cmd,
                Some(&right),
                at(ASKED_AT + i64::try_from(n).unwrap()),
            ),
        );
    }
    let events = control(&fx);
    assert_eq!(events.len(), 2);
    let assertions: Vec<Option<&str>> = events
        .iter()
        .map(|e| member_text(e, "payload.step_up.assertion_id"))
        .collect();
    assert_eq!(
        assertions,
        vec![Some("cli-assertion-1"), Some("cli-assertion-2")],
        "two gestures, two fresh assertions, both from the injected ids"
    );
    assert_eq!(ids.assertions, 2);
}

/// Journal spec §9's `OwnerCommandIssued`, as DEC-290 closes it until the catalogue does: these
/// members and no other.
const ISSUED: [&str; 12] = [
    "agent",
    "bid",
    "bid_size",
    "command",
    "floor",
    "release",
    "scope",
    "step_up",
    "subject",
    "submitted_at",
    "user",
    "warning_shown",
];

/// `OwnerAcknowledged`'s members (DEC-279 item 7).
const ACKNOWLEDGED: [&str; 6] = [
    "agent",
    "event",
    "risk_clock",
    "step_up",
    "submitted_at",
    "user",
];

/// Step-up evidence's members, when it is not `null` (DEC-155 item 4).
const STEP_UP: [&str; 3] = ["assertion_id", "authenticated_at", "method"];

/// The member names of the object at `path`, or a panic naming what is not an object.
fn members(event: &Value, path: &str) -> BTreeSet<String> {
    member(event, path)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("{path} is an object in {event:?}"))
        .keys()
        .map(|k| k.as_str().to_owned())
        .collect()
}

fn set(names: &[&str]) -> BTreeSet<String> {
    names.iter().map(|n| (*n).to_owned()).collect()
}

/// The payload's members are exactly `expected`, and its step-up evidence is `null` or exactly
/// the evidence's three members.
fn closed(event: &Value, expected: &[&str], what: &str) {
    assert_eq!(members(event, "payload"), set(expected), "{what}'s payload");
    if member(event, "payload.step_up") != Some(&Value::Null) {
        assert_eq!(
            members(event, "payload.step_up"),
            set(&STEP_UP),
            "{what}'s step-up"
        );
    }
}

/// The code `cmd` needs now, which every command here but pause has.
fn code_for(fx: &Fixture, agent: &str, cmd: &Command) -> String {
    answer("code", code(&fx.journal, &owner(), agent, cmd))
        .unwrap_or_else(|| panic!("{cmd:?} has a code"))
}

/// The #397 review, minor 1; DEC-290: every payload the CLI commits has exactly its members, so a
/// member nobody named (the code the owner typed, order content) fails here. Every command and
/// every kill-switch scope is committed once, with and without step-up evidence, and only a Stop
/// carries a release choice: `false` without `--release`, and no warning shown.
#[test]
fn every_owner_command_payload_has_exactly_its_members() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let stop = Command::Stop { release: false };
    let ack = Command::Acknowledge {
        event: common::runtime_id(42),
    };
    let mut second = ASKED_AT;
    let mut run = |fx: &mut Fixture, cmd: &Command, coded: bool| {
        let typed = coded.then(|| code_for(fx, AGENT, cmd));
        second += 1;
        answer(
            "command",
            command(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                cmd,
                typed.as_deref(),
                at(second),
            ),
        )
    };
    run(&mut fx, &Command::Pause, false);
    run(&mut fx, &Command::Resume, true);
    run(&mut fx, &stop, true);
    run(&mut fx, &exit_aapl(true), true);
    run(&mut fx, &exit_of("MSFT", None), false);
    run(&mut fx, &ack, true);
    let agent_code = answer(
        "kill code",
        kill_code(&fx.journal, &owner(), &Scope::Agent(AGENT.to_owned())),
    );
    for (scope, typed) in [
        (Scope::Agent(AGENT.to_owned()), Some(agent_code)),
        (Scope::Connection("conn-1".to_owned()), None),
        (Scope::Workspace, None),
    ] {
        answer(
            "kill",
            kill(
                &mut fx.journal,
                &mut ids,
                &owner(),
                &scope,
                typed.as_deref(),
                at(ASKED_AT + 100),
            ),
        );
    }
    let events = control(&fx);
    assert_eq!(events.len(), 9, "one event per command");
    let mut stepped_up = 0;
    for event in &events {
        let event_type = member_text(event, "event_type").unwrap_or_default();
        let what = member_text(event, "payload.command").unwrap_or(event_type);
        match event_type {
            "OwnerCommandIssued" => closed(event, &ISSUED, what),
            "OwnerAcknowledged" => closed(event, &ACKNOWLEDGED, what),
            other => panic!("the CLI committed a {other}"),
        }
        if member(event, "payload.step_up") != Some(&Value::Null) {
            stepped_up += 1;
        }
        if event_type == "OwnerCommandIssued" {
            let release = member(event, "payload.release");
            let expected = (what == "stop").then_some(Value::Bool(false));
            assert_eq!(release, Some(&expected.unwrap_or(Value::Null)), "{what}");
            assert_eq!(
                member(event, "payload.warning_shown"),
                Some(&Value::Null),
                "{what}"
            );
        }
    }
    assert_eq!(
        stepped_up, 5,
        "resume, Stop, the exit, the acknowledgment, the agent's kill"
    );
}

/// DEC-136, the #397 review, minor 2: `stop --release` needs the code like any Stop, records the
/// release choice, and records which warning was shown as the content reference of the exact text
/// the CLI shows, computed here from [`RELEASE_WARNING`]'s bytes. The warning says the positions
/// become unprotected.
#[test]
#[ignore = "pending E8-3"]
fn stop_with_release_records_the_choice_and_the_warning_shown() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let release = Command::Stop { release: true };
    let right = code_for(&fx, AGENT, &release);
    assert_eq!(
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &release,
            None,
            at(ASKED_AT)
        ),
        Err(ControlError::Refused {
            reason: "step_up_missing"
        })
    );
    assert!(control(&fx).is_empty());
    answer(
        "stop --release",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &release,
            Some(&right),
            at(ASKED_AT),
        ),
    );
    let event = committed_one(&fx, 0);
    closed(&event, &ISSUED, "stop --release");
    assert_eq!(member_text(&event, "payload.command"), Some("stop"));
    assert_eq!(member(&event, "payload.release"), Some(&Value::Bool(true)));
    let shown = format!("sha256:{}", Digest::of(RELEASE_WARNING.as_bytes()).to_hex());
    assert_eq!(
        member_text(&event, "payload.warning_shown"),
        Some(shown.as_str())
    );
    assert_eq!(
        member_text(&event, "payload.step_up.method"),
        Some("cli_confirm")
    );
    assert!(
        RELEASE_WARNING.to_lowercase().contains("unprotected"),
        "{RELEASE_WARNING}"
    );
}

/// DEC-279 item 1, DEC-136: a Stop's code is bound to the release choice, so the plain Stop's code
/// never confirms a release, nor the release's code a plain Stop; each is refused and commits
/// nothing.
#[test]
#[ignore = "pending E8-3"]
fn the_stop_code_is_bound_to_the_release_choice() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let plain = Command::Stop { release: false };
    let release = Command::Stop { release: true };
    let plain_code = code_for(&fx, AGENT, &plain);
    let release_code = code_for(&fx, AGENT, &release);
    assert_ne!(plain_code, release_code);
    for (cmd, typed) in [(&release, &plain_code), (&plain, &release_code)] {
        assert_eq!(
            command(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                cmd,
                Some(typed),
                at(ASKED_AT)
            ),
            Err(ControlError::Refused {
                reason: "step_up_missing"
            }),
            "{cmd:?}"
        );
    }
    assert!(control(&fx).is_empty());
}

/// The M7 brief's command table, the #397 review, minor 3; mandate spec §5.9: `status` shows the
/// restrictions the account stream holds for this agent and no other. Each agent restriction shows
/// the mode its latest `AgentModeApplied` names, and one lifted to `normal` is gone; each instrument
/// restriction shows while its latest change is active. Agent restrictions come first, each group
/// in name order.
#[test]
#[ignore = "pending E8-3"]
fn status_shows_the_restrictions_in_force() {
    let mut fx = Fixture::new();
    fx.applied(AGENT, "reconciliation", "exits_only");
    fx.applied(AGENT, "daily_loss", "paused");
    fx.applied(OTHER_AGENT, "drawdown_flatten", "stopped");
    fx.applied(AGENT, "reconciliation", "normal");
    fx.applied(AGENT, "daily_loss", "exits_only");
    fx.instrument(AGENT, "AAPL", "stale_mark", true);
    fx.instrument(AGENT, "MSFT", "removed_instrument", true);
    fx.instrument(AGENT, "MSFT", "removed_instrument", false);
    fx.instrument(OTHER_AGENT, "TSLA", "stale_mark", true);
    fx.instrument(AGENT, "AAPL", "removed_instrument", true);
    let seen = answer("status", status(&fx.journal, &owner(), AGENT, ACCOUNT));
    assert_eq!(
        seen.restrictions,
        vec![
            Restriction::Agent {
                restriction: "daily_loss".to_owned(),
                mode: "exits_only".to_owned(),
            },
            Restriction::Instrument {
                instrument: "AAPL".to_owned(),
                restriction: "removed_instrument".to_owned(),
            },
            Restriction::Instrument {
                instrument: "AAPL".to_owned(),
                restriction: "stale_mark".to_owned(),
            },
        ]
    );
    assert!(control(&fx).is_empty(), "status commits nothing");
}

/// ULID's shape, as journal spec §3 requires of an event id: 26 Crockford base-32 digits, the first
/// at most `7`.
fn ulid_shaped(id: &str) -> bool {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    id.len() == 26
        && id.bytes().all(|b| ALPHABET.contains(&b))
        && id.bytes().next().is_some_and(|b| b <= b'7')
}

/// The event id a pause of `agent` commits in a fresh journal at `second`, after `before` other
/// commands, read from the stored event.
fn pause_id(agent: &str, before: usize, second: i64) -> String {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    for n in 0..before {
        let typed = code_for(&fx, OTHER_AGENT, &Command::Resume);
        answer(
            "resume",
            command(
                &mut fx.journal,
                &mut ids,
                &owner(),
                OTHER_AGENT,
                &Command::Resume,
                Some(&typed),
                at(second - 10 + i64::try_from(n).unwrap_or_default()),
            ),
        );
    }
    let submitted = answer(
        "pause",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            agent,
            &Command::Pause,
            None,
            at(second),
        ),
    );
    let stored = committed_one(&fx, before);
    assert_eq!(
        member_text(&stored, "event_id"),
        Some(submitted.event_id.as_str())
    );
    submitted.event_id
}

/// DEC-290, the #397 review, minor 4: the event id is derived, not minted. The same choice at the
/// same head is the same id whenever it is run; another agent, another head, or the same exit with
/// and without the code is another id; and every id is ULID-shaped.
#[test]
fn the_event_id_is_derived_from_the_choice_and_the_head() {
    let first = pause_id(AGENT, 0, ASKED_AT);
    assert!(ulid_shaped(&first), "{first}");
    assert_eq!(first, pause_id(AGENT, 0, ASKED_AT + 7), "the second run at");
    assert_ne!(first, pause_id(OTHER_AGENT, 0, ASKED_AT), "the agent");
    let later = pause_id(AGENT, 1, ASKED_AT);
    assert!(ulid_shaped(&later), "{later}");
    assert_ne!(first, later, "the head");

    let exit_id = |coded: bool| {
        let mut fx = Fixture::new();
        let typed = coded.then(|| code_for(&fx, AGENT, &exit_aapl(true)));
        answer(
            "exit",
            command(
                &mut fx.journal,
                &mut FixedIds::default(),
                &owner(),
                AGENT,
                &exit_aapl(true),
                typed.as_deref(),
                at(ASKED_AT),
            ),
        )
        .event_id
    };
    assert_ne!(exit_id(true), exit_id(false), "whether it is stepped up");
}

/// DEC-290: only the control stream's last event can be the same command, so a pause repeated
/// after a resume is committed again, under its own id, and is never taken for the first.
#[test]
fn a_command_repeated_after_another_commits_anew() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let mut ran = Vec::new();
    for (n, cmd) in [Command::Pause, Command::Resume, Command::Pause]
        .iter()
        .enumerate()
    {
        let typed = (cmd == &Command::Resume).then(|| code_for(&fx, AGENT, cmd));
        ran.push(answer(
            "command",
            command(
                &mut fx.journal,
                &mut ids,
                &owner(),
                AGENT,
                cmd,
                typed.as_deref(),
                at(ASKED_AT + i64::try_from(n).unwrap_or_default()),
            ),
        ));
    }
    let events = control(&fx);
    assert_eq!(events.len(), 3);
    let stored: Vec<Option<&str>> = events.iter().map(|e| member_text(e, "event_id")).collect();
    let returned: Vec<Option<&str>> = ran.iter().map(|s| Some(s.event_id.as_str())).collect();
    assert_eq!(stored, returned);
    assert_ne!(ran[0].event_id, ran[2].event_id);
    assert_eq!(member_text(&events[2], "payload.command"), Some("pause"));
}

/// One of the commands a re-run is checked for. A kill switch is not one: it is always committed
/// (DEC-290 item 5), which `a_kill_switch_is_always_committed_never_reported_as_an_earlier_one`
/// pins.
enum Run {
    Pause,
    /// A resume, with the code printed before the first run.
    Resume(String),
    Exit,
}

fn run(
    fx: &mut Fixture,
    ids: &mut FixedIds,
    which: &Run,
    second: i64,
) -> Result<Submitted, ControlError> {
    let one = |fx: &mut Fixture, ids: &mut FixedIds, cmd: &Command, typed: Option<&str>| {
        command(
            &mut fx.journal,
            ids,
            &owner(),
            AGENT,
            cmd,
            typed,
            at(second),
        )
    };
    match which {
        Run::Pause => one(fx, ids, &Command::Pause, None),
        Run::Resume(typed) => one(fx, ids, &Command::Resume, Some(typed)),
        Run::Exit => one(fx, ids, &exit_aapl(true), None),
    }
}

/// DEC-290, the #397 review, minor 4: a command whose append committed but whose every answer was
/// lost gives up, naming the event it may have committed; a re-run, with a new process's ids and a
/// later clock, finds that event and commits nothing twice, spending no new step-up. A resume's
/// re-run types the code printed before the first run, at the head before its own event. The kill
/// switch is the exception, always committed (DEC-290 item 5).
#[test]
#[ignore = "pending E8-3"]
fn a_re_run_of_a_command_that_did_commit_commits_nothing_twice() {
    let resume = code_for(&Fixture::new(), AGENT, &Command::Resume);
    for (label, which) in [
        ("pause", Run::Pause),
        ("resume", Run::Resume(resume)),
        ("owner exit", Run::Exit),
    ] {
        let mut fx = Fixture::new();
        fx.journal.lost_for_good = true;
        let first = run(&mut fx, &mut FixedIds::default(), &which, ASKED_AT);
        let events = control(&fx);
        assert_eq!(events.len(), 1, "{label}: the first run did commit");
        let stored = member_text(&events[0], "event_id")
            .unwrap_or_default()
            .to_owned();
        match &first {
            Err(ControlError::Journal(why)) => {
                assert!(why.contains(&stored), "{label} names {stored}: {why}");
            }
            other => panic!("{label}: the first run gives up, not: {other:?}"),
        }
        fx.journal.recover();
        let mut ids = FixedIds::default();
        let again = run(&mut fx, &mut ids, &which, ASKED_AT + 5);
        assert_eq!(
            again,
            Ok(Submitted {
                event_id: stored.clone(),
                seq: 1
            }),
            "{label}"
        );
        assert_eq!(control(&fx).len(), 1, "{label}: committed once");
        assert_eq!(ids.assertions, 0, "{label}: the re-run spent no step-up");
    }
}

/// DEC-290: once the runtime has recorded the last command (an agent-stream event whose
/// `causation_id` is it), the same command again is a new one and is committed; until then it is
/// the same one, and a second pause reports the first.
#[test]
#[ignore = "pending E8-3"]
fn a_command_its_runtime_recorded_is_committed_anew() {
    let pause = |fx: &mut Fixture, second: i64| {
        answer(
            "pause",
            command(
                &mut fx.journal,
                &mut FixedIds::default(),
                &owner(),
                AGENT,
                &Command::Pause,
                None,
                at(second),
            ),
        )
    };
    let mut fx = Fixture::new();
    let first = pause(&mut fx, ASKED_AT);
    assert_eq!(pause(&mut fx, ASKED_AT + 1), first, "not yet recorded");
    assert_eq!(control(&fx).len(), 1);

    fx.copied(AGENT, "AgentModeChanged", &first.event_id);
    let second = pause(&mut fx, ASKED_AT + 2);
    assert_ne!(second.event_id, first.event_id);
    assert_eq!(second.seq, 2);
    let events = control(&fx);
    assert_eq!(events.len(), 2);
    assert_eq!(
        member_text(&events[1], "event_id"),
        Some(second.event_id.as_str())
    );
}

/// DEC-290 item 5, rule 13, rule 3 (the #409 review, minor 5): a kill switch of any scope is always
/// committed and never reported as an earlier one. The same kill switch twice in a row, for each
/// scope, commits twice under two ids; and a kill switch whose every answer was lost is committed
/// again by its re-run, never taken for the first. A pause repeated the same way is reported as the
/// first, so the difference is the kill switch's own.
#[test]
#[ignore = "pending E8-3"]
fn a_kill_switch_is_always_committed_never_reported_as_an_earlier_one() {
    let mut fx = Fixture::new();
    let pause = |fx: &mut Fixture, second: i64| {
        answer(
            "pause",
            command(
                &mut fx.journal,
                &mut FixedIds::default(),
                &owner(),
                AGENT,
                &Command::Pause,
                None,
                at(second),
            ),
        )
    };
    let first = pause(&mut fx, ASKED_AT);
    assert_eq!(pause(&mut fx, ASKED_AT + 1), first, "a pause is found");
    assert_eq!(control(&fx).len(), 1);

    for scope in [
        Scope::Agent(AGENT.to_owned()),
        Scope::Connection("conn-1".to_owned()),
        Scope::Workspace,
    ] {
        let mut fx = Fixture::new();
        let switch = |fx: &mut Fixture, second: i64| {
            kill(
                &mut fx.journal,
                &mut FixedIds::default(),
                &owner(),
                &scope,
                None,
                at(second),
            )
        };
        let first = answer("kill", switch(&mut fx, ASKED_AT));
        let second = answer("kill", switch(&mut fx, ASKED_AT + 1));
        assert_ne!(second.event_id, first.event_id, "{scope:?}");
        assert_eq!(second.seq, 2, "{scope:?}");
        assert_eq!(control(&fx).len(), 2, "{scope:?}: committed twice");

        let mut fx = Fixture::new();
        fx.journal.lost_for_good = true;
        let lost = switch(&mut fx, ASKED_AT);
        assert!(
            matches!(lost, Err(ControlError::Journal(_))),
            "{scope:?}: {lost:?}"
        );
        fx.journal.recover();
        let again = answer("kill", switch(&mut fx, ASKED_AT + 5));
        assert_eq!(again.seq, 2, "{scope:?}: the re-run is committed");
        assert_eq!(control(&fx).len(), 2, "{scope:?}");
    }
}

/// DEC-290 item 5: an acknowledgment has no agent-stream record the CLI can read, so the same
/// acknowledgment again right after it, with the code printed before the first, reports the first
/// and commits nothing, spending no new step-up.
#[test]
#[ignore = "pending E8-3"]
fn a_repeated_acknowledgment_reports_the_first() {
    let mut fx = Fixture::new();
    let ack = Command::Acknowledge {
        event: common::runtime_id(42),
    };
    let typed = code_for(&fx, AGENT, &ack);
    let acknowledge = |fx: &mut Fixture, ids: &mut FixedIds, second: i64| {
        command(
            &mut fx.journal,
            ids,
            &owner(),
            AGENT,
            &ack,
            Some(&typed),
            at(second),
        )
    };
    let first = answer(
        "acknowledge",
        acknowledge(&mut fx, &mut FixedIds::default(), ASKED_AT),
    );
    let mut ids = FixedIds::default();
    let again = answer("acknowledge", acknowledge(&mut fx, &mut ids, ASKED_AT + 3));
    assert_eq!(again, first);
    assert_eq!(control(&fx).len(), 1);
    assert_eq!(ids.assertions, 0, "no new step-up");
}

/// ULID's 26 Crockford base-32 digits of a 128-bit value, written out bit by bit: two zero bits
/// pad the 128 to 130, and each five give one digit, most significant first.
fn crockford(bytes: &[u8]) -> String {
    const ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let bits: String = std::iter::once("00".to_owned())
        .chain(bytes.iter().map(|b| format!("{b:08b}")))
        .collect();
    bits.as_bytes()
        .chunks(5)
        .map(|chunk| {
            let digit = chunk
                .iter()
                .fold(0, |n, bit| n * 2 + usize::from(*bit == b'1'));
            char::from(ALPHABET[digit])
        })
        .collect()
}

/// The event id DEC-290 item 4 defines, computed here from `mandate_canon` alone: the canonical
/// object naming the control head as decimal text, the event type, the owner's choice, whether it
/// is stepped up, and the stream, hashed, its first 128 bits written as a ULID.
fn oracle_id(event_type: &str, choice: Value, stepped_up: bool, head: u64) -> String {
    let bound = common::object(&[
        ("control_head", common::text(&head.to_string())),
        ("event_type", common::text(event_type)),
        ("key", choice),
        ("stepped_up", Value::Bool(stepped_up)),
        ("stream", common::text(CONTROL)),
    ]);
    crockford(&Digest::of(&to_canonical(&bound)).as_bytes()[..16])
}

/// The owner's choice in an `OwnerCommandIssued` for `agent`: every member but `submitted_at` and
/// `step_up` (DEC-290 item 4).
fn issued_choice(command: &str, agent: &str) -> Value {
    common::object(&[
        ("agent", common::text(agent)),
        ("bid", Value::Null),
        ("bid_size", Value::Null),
        ("command", common::text(command)),
        ("floor", Value::Null),
        ("release", Value::Null),
        ("scope", common::text("agent")),
        ("subject", common::text(agent)),
        ("user", common::text(OWNER)),
        ("warning_shown", Value::Null),
    ])
}

/// The #409 review, minor 2: the derived id has an oracle of its own. A pause at head 0, a resume
/// with its code at head 1, and a grant at head 2 each commit exactly the id this test computes
/// from the definition, so reshaping the hashed object or truncating the digest fails here.
#[test]
fn the_event_id_matches_its_definition() {
    let mut fx = Fixture::new();
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let mut ids = FixedIds::default();
    let pause = answer(
        "pause",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &Command::Pause,
            None,
            at(ASKED_AT),
        ),
    );
    assert_eq!(
        pause.event_id,
        oracle_id(
            "OwnerCommandIssued",
            issued_choice("pause", AGENT),
            false,
            0
        )
    );
    let typed = code_for(&fx, AGENT, &Command::Resume);
    let resume = answer(
        "resume",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &Command::Resume,
            Some(&typed),
            at(ASKED_AT + 1),
        ),
    );
    assert_eq!(
        resume.event_id,
        oracle_id(
            "OwnerCommandIssued",
            issued_choice("resume", AGENT),
            true,
            1
        )
    );
    let grant = answer(
        "approve",
        approve(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &asked.approval,
            &common::code_of(&asked.content),
            at(ASKED_AT + 2),
        ),
    );
    let choice = common::object(&[
        ("agent", common::text(AGENT)),
        ("approval", common::text(&asked.approval)),
        (
            "content_hash",
            common::text(&common::hash_of(&asked.content)),
        ),
        ("responder", common::text(OWNER)),
        ("role", common::text("approver")),
        ("verdict", common::text("approved")),
    ]);
    assert_eq!(
        grant.event_id,
        oracle_id("ApprovalResponseSubmitted", choice, true, 2)
    );
}
