//! `mandate agent` (M7 tests PR 4 of 4; the brief's "The CLI control surface"; mandate spec §6.1;
//! DEC-155 items 4 and 5, DEC-158 option (c), DEC-257 item 5).
//!
//! Every command commits exactly one control-stream event and nothing else. Pause needs no code.
//! Resume, Stop, and acknowledge need the code `code` prints and commit nothing without it. A kill
//! switch and an owner exit are never refused here: without the right code they are committed with
//! no step-up evidence, which the runtime still applies without the owner-exit privilege (rule 13).
//! Each code is bound to what it confirms and to the control stream's head, which this file checks
//! by the code changing when the head moves and differing between scopes.
//!
//! Every test is pending until the CLI's implementation and fails on the command's
//! `ControlError::Unimplemented` (DEC-77, DEC-110).

mod common;

use common::{
    AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OWNER, at, body, member, member_text, owner,
};
use mandate_canon::Value;
use mandate_cli::agent::{Command, Confirmed, Scope, code, command, kill, kill_code, status};
use mandate_cli::control::ControlError;

fn answer<T>(what: &str, result: Result<T, ControlError>) -> T {
    result.unwrap_or_else(|e| panic!("{what} answers, not: {e}"))
}

fn control(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL).iter().map(body).collect()
}

/// The one event a command committed, checked as the owner's on the control stream.
fn committed_one(fx: &Fixture, before: usize) -> Value {
    let events = control(fx);
    assert_eq!(events.len(), before + 1, "one event per command");
    let event = events[before].clone();
    assert_eq!(member_text(&event, "actor.kind"), Some("user"));
    assert_eq!(member_text(&event, "actor.id"), Some(OWNER));
    event
}

fn exit_aapl(bid: bool) -> Command {
    Command::Exit {
        instrument: "AAPL".to_owned(),
        bid: bid.then(|| Confirmed {
            bid: "154.1".to_owned(),
            bid_size: "300".to_owned(),
            floor: "150".to_owned(),
        }),
    }
}

/// PX-4: pause needs no code, has none, and commits an `OwnerCommandIssued` with no step-up.
#[test]
#[ignore = "pending E8-3"]
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
#[ignore = "pending E8-3"]
fn resume_stop_and_acknowledge_need_the_code() {
    for (cmd, event_type, name) in [
        (Command::Resume, "OwnerCommandIssued", Some("resume")),
        (Command::Stop, "OwnerCommandIssued", Some("stop")),
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
#[ignore = "pending E8-3"]
fn a_code_is_bound_to_its_command_and_the_head() {
    let mut fx = Fixture::new();
    let mut ids = FixedIds::default();
    let resume = answer("code", code(&fx.journal, &owner(), AGENT, &Command::Resume));
    let stop = answer("code", code(&fx.journal, &owner(), AGENT, &Command::Stop));
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
#[ignore = "pending E8-3"]
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
/// regular session; with the code it carries the evidence that unlocks the privilege.
#[test]
#[ignore = "pending E8-3"]
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

    let right = answer(
        "code",
        code(&fx.journal, &owner(), AGENT, &exit_aapl(false)),
    )
    .unwrap_or_else(|| panic!("an owner exit has a code"));
    answer(
        "exit",
        command(
            &mut fx.journal,
            &mut ids,
            &owner(),
            AGENT,
            &exit_aapl(false),
            Some(&right),
            at(ASKED_AT + 1),
        ),
    );
    let event = committed_one(&fx, 1);
    assert_eq!(member(&event, "payload.bid"), Some(&Value::Null));
    assert_eq!(
        member_text(&event, "payload.step_up.method"),
        Some("cli_confirm")
    );
}

/// `status` reads the agent stream: the last mode change, whether it was the startup hold, and the
/// approvals still pending; it commits nothing.
#[test]
#[ignore = "pending E8-3"]
fn status_reads_the_mode_the_hold_and_the_pending_count() {
    let mut fx = Fixture::new();
    let first = fx.ask(AGENT, "10", ASKED_AT);
    fx.ask(AGENT, "5", ASKED_AT);
    fx.ended(&first, "ApprovalCanceled");
    fx.mode(AGENT, "paused", "awaiting_reconciliation");
    let seen = answer("status", status(&fx.journal, &owner(), AGENT));
    assert_eq!(seen.mode, "paused");
    assert!(seen.startup_hold);
    assert_eq!(seen.pending_approvals, 1);
    fx.mode(AGENT, "normal", "restriction_changed");
    let seen = answer("status", status(&fx.journal, &owner(), AGENT));
    assert_eq!(seen.mode, "normal");
    assert!(!seen.startup_hold);
    assert!(control(&fx).is_empty());
}
