//! E8-3's `clap` follow-up (DEC-279 item 10; DEC-2507, DEC-2512): `mandate agent status`,
//! `resume`, `stop`, `exit`, `acknowledge` and `request-ack`. Each command's lines are driven over the M7 tests' in-memory journal, and the
//! refusals need no database. Without `--code`, a resume or Stop shows its code and commits
//! nothing, and a release shows its warning first. A wrong code refuses both, with nothing
//! committed. An exit is never refused: it commits at once, carrying step-up evidence only with the
//! right code. `status` ends with the controls in PX-3's order, and no refusal prints anything or
//! names the DSN.

mod common;

use std::collections::BTreeMap;

use clap::Parser;
use common::{ACCOUNT, AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OWNER, at, body, member};
use common::{member_text, owner};
use mandate_canon::{Digest, Value};
use mandate_cli::agent::{
    Command as Owned, Confirmed, RELEASE_WARNING, Restriction, Status, Subject, code,
};
use mandate_cli::brake::AgentCommand;
use mandate_cli::control::ControlError;
use mandate_cli::owner::{
    Typed, answer, run_acknowledge, run_exit, run_request_ack, run_resume, run_status, run_stop,
    status_lines,
};
use mandate_cli::{Cli, Command};

const INSTRUMENT: &str = "SPY";
/// The account stream's `RiskLimitTriggered` an acknowledgment names.
const EVENT: &str = "01JBTG00001000000000000000";
/// An `OwnerAcknowledgmentRequested` an acknowledgment names.
const REQUEST: &str = "01JBRQ00001000000000000000";
const BID: [&str; 6] = [
    "--confirm-bid",
    "501.2",
    "--bid-size",
    "300",
    "--floor",
    "495",
];

/// `mandate agent <args>` as `user` of `workspace` in `environment`, on the journal `dsn`.
fn parsed_as(
    args: &[&str],
    owner: [&str; 3],
    dsn: &str,
    store: &str,
) -> clap::error::Result<AgentCommand> {
    let [workspace, user, environment] = owner;
    let tail = [
        "--workspace",
        workspace,
        "--user",
        user,
        "--environment",
        environment,
    ];
    let target = ["--journal", dsn, "--store", store];
    let argv = ["mandate", "agent"].into_iter().chain(args.iter().copied());
    match Cli::try_parse_from(argv.chain(tail).chain(target))?.command {
        Command::Agent(command) => Ok(command),
        other => panic!("{args:?} parsed as {other:?}"),
    }
}

fn parsed(args: &[&str]) -> clap::error::Result<AgentCommand> {
    let dsn = "postgres://journal@localhost/mandate";
    parsed_as(args, ["ws1", OWNER, "paper"], dsn, "/work/store")
}

/// The artifact store a command answers from: these agents' deployments are never read from one.
fn no_store() -> BTreeMap<Digest, Vec<u8>> {
    BTreeMap::new()
}

/// [`answer`] for [`AGENT`] at `ASKED_AT + n`, panicking with anything but lines.
fn answered(
    fx: &mut Fixture,
    ids: &mut FixedIds,
    command: &Owned,
    typed: Typed<'_>,
    n: usize,
) -> Vec<String> {
    let now = at(ASKED_AT + i64::try_from(n).unwrap());
    answer(
        &mut fx.journal,
        &no_store(),
        ids,
        &owner(),
        AGENT,
        command,
        typed,
        now,
    )
    .unwrap_or_else(|e| panic!("{command:?} answers, not: {e}"))
}

fn controls(fx: &Fixture) -> Vec<Value> {
    fx.rows(CONTROL).iter().map(body).collect()
}

fn code_now(fx: &Fixture, command: &Owned) -> String {
    code(&fx.journal, &owner(), AGENT, command)
        .unwrap_or_else(|e| panic!("{command:?}'s code: {e}"))
        .unwrap_or_else(|| panic!("{command:?} has a code"))
}

/// The line a committed command prints, naming the control stream's last event.
fn committed_line(fx: &Fixture, named: &str) -> String {
    let rows = fx.rows(CONTROL);
    let last = rows.last().expect("an event was committed");
    let id = member_text(&body(last), "event_id")
        .expect("an event id")
        .to_owned();
    format!("{named} committed as event {id} at seq {}", last.seq)
}

fn exit(floor: Option<&str>) -> Owned {
    let bid = floor.map(|floor| Confirmed {
        bid: "501.2".into(),
        bid_size: "300".into(),
        floor: floor.into(),
    });
    Owned::Exit {
        instrument: INSTRUMENT.to_owned(),
        bid,
    }
}

/// DEC-2507 item 1: `status` needs its account; `resume` and `stop` take an optional code, and only
/// `stop` takes `--release`; `exit` takes its three bid flags together or not at all, and a code or
/// `--show-code`, never both.
#[test]
fn the_binary_offers_status_resume_stop_and_exit() {
    let Ok(AgentCommand::Status(status)) = parsed(&["status", AGENT, "--account", ACCOUNT]) else {
        panic!("status")
    };
    assert_eq!(
        (status.agent.as_str(), status.account.as_str()),
        (AGENT, ACCOUNT)
    );
    assert!(
        parsed(&["status", AGENT]).is_err(),
        "status needs its account"
    );
    for (args, typed) in [
        (vec!["resume", AGENT], Typed::Show),
        (
            vec!["resume", AGENT, "--code", "c0de"],
            Typed::Commit(Some("c0de")),
        ),
    ] {
        let Ok(AgentCommand::Resume(resume)) = parsed(&args) else {
            panic!("{args:?}")
        };
        assert_eq!(resume.typed(), typed, "{args:?}");
    }
    assert!(
        parsed(&["resume", AGENT, "--release"]).is_err(),
        "only a Stop releases"
    );
    for (args, release, typed) in [
        (vec!["stop", AGENT, "--release"], true, Typed::Show),
        (
            vec!["stop", AGENT, "--code", "c0de"],
            false,
            Typed::Commit(Some("c0de")),
        ),
    ] {
        let Ok(AgentCommand::Stop(stop)) = parsed(&args) else {
            panic!("{args:?}")
        };
        assert_eq!(
            (stop.command(), stop.typed()),
            (Owned::Stop { release }, typed)
        );
    }
    let with_bid: Vec<&str> = ["exit", AGENT, INSTRUMENT].into_iter().chain(BID).collect();
    for (args, command, typed) in [
        (
            vec!["exit", AGENT, INSTRUMENT],
            exit(None),
            Typed::Commit(None),
        ),
        (with_bid, exit(Some("495")), Typed::Commit(None)),
        (
            vec!["exit", AGENT, INSTRUMENT, "--show-code"],
            exit(None),
            Typed::Show,
        ),
    ] {
        let Ok(AgentCommand::Exit(given)) = parsed(&args) else {
            panic!("{args:?}")
        };
        assert_eq!(
            (given.command(), given.typed()),
            (command, typed),
            "{args:?}"
        );
    }
    for partial in [&BID[..2], &BID[..4], &BID[2..]] {
        let args: Vec<&str> = ["exit", AGENT, INSTRUMENT]
            .into_iter()
            .chain(partial.iter().copied())
            .collect();
        assert!(
            parsed(&args).is_err(),
            "{args:?}: all three bid flags or none"
        );
    }
    let both = ["exit", AGENT, INSTRUMENT, "--show-code", "--code", "c0de"];
    assert!(parsed(&both).is_err(), "a code or --show-code, not both");
}

fn acknowledge(request: Option<&str>) -> Owned {
    Owned::Acknowledge {
        event: EVENT.to_owned(),
        request: request.map(str::to_owned),
        subject: None,
    }
}

fn request_ack() -> Owned {
    Owned::RequestAcknowledgment {
        event: EVENT.to_owned(),
        subject: None,
    }
}

/// DEC-2512: `acknowledge` and `request-ack` name the agent and the triggered event, take an
/// optional code as `resume` does, and only `acknowledge` names a `--request`.
#[test]
fn the_binary_offers_acknowledge_and_request_ack() {
    for (args, command, typed) in [
        (
            vec!["acknowledge", AGENT, EVENT],
            acknowledge(None),
            Typed::Show,
        ),
        (
            vec![
                "acknowledge",
                AGENT,
                EVENT,
                "--request",
                REQUEST,
                "--code",
                "c0de",
            ],
            acknowledge(Some(REQUEST)),
            Typed::Commit(Some("c0de")),
        ),
    ] {
        let Ok(AgentCommand::Acknowledge(given)) = parsed(&args) else {
            panic!("{args:?}")
        };
        assert_eq!(given.agent, AGENT);
        assert_eq!(
            (given.command(), given.typed()),
            (command, typed),
            "{args:?}"
        );
    }
    for (args, typed) in [
        (vec!["request-ack", AGENT, EVENT], Typed::Show),
        (
            vec!["request-ack", AGENT, EVENT, "--code", "c0de"],
            Typed::Commit(Some("c0de")),
        ),
    ] {
        let Ok(AgentCommand::RequestAck(given)) = parsed(&args) else {
            panic!("{args:?}")
        };
        assert_eq!(given.agent, AGENT);
        assert_eq!(
            (given.command(), given.typed()),
            (request_ack(), typed),
            "{args:?}"
        );
    }
    assert!(
        parsed(&["acknowledge", AGENT]).is_err(),
        "it names the event"
    );
    assert!(
        parsed(&["request-ack", AGENT, EVENT, "--request", REQUEST]).is_err(),
        "only an acknowledgment names a request"
    );
}

/// E7-4b: `--subject` names what the event is, on an acknowledgment and on a request, and takes
/// only §9.28's three subjects.
#[test]
fn acknowledge_and_request_ack_take_a_subject() {
    for (word, subject) in [
        ("limit", Subject::Limit),
        ("restriction", Subject::Restriction),
        ("reconciliation", Subject::Reconciliation),
    ] {
        let args = ["acknowledge", AGENT, EVENT, "--subject", word];
        let Ok(AgentCommand::Acknowledge(given)) = parsed(&args) else {
            panic!("{args:?}")
        };
        assert_eq!(
            given.command(),
            Owned::Acknowledge {
                event: EVENT.to_owned(),
                request: None,
                subject: Some(subject),
            }
        );
        let args = ["request-ack", AGENT, EVENT, "--subject", word];
        let Ok(AgentCommand::RequestAck(given)) = parsed(&args) else {
            panic!("{args:?}")
        };
        assert_eq!(
            given.command(),
            Owned::RequestAcknowledgment {
                event: EVENT.to_owned(),
                subject: Some(subject),
            }
        );
    }
    assert!(parsed(&["acknowledge", AGENT, EVENT, "--subject", "hold"]).is_err());
}

/// DEC-2512: without a code an acknowledgment and a request print the code `agent::code` gives and
/// commit nothing; with it each commits one event, which the line names.
#[test]
fn an_acknowledgment_or_request_shows_its_code_then_commits_with_it() {
    let (mut fx, mut ids) = (Fixture::new(), FixedIds::default());
    let commands = [
        (
            acknowledge(None),
            "OwnerAcknowledged",
            format!("acknowledge {AGENT} {EVENT}"),
        ),
        (
            request_ack(),
            "OwnerAcknowledgmentRequested",
            format!("request-ack {AGENT} {EVENT}"),
        ),
    ];
    for (n, (command, event_type, named)) in commands.into_iter().enumerate() {
        let right = code_now(&fx, &command);
        let shown = answered(&mut fx, &mut ids, &command, Typed::Show, n);
        assert_eq!(shown, [format!("code {right}")], "{command:?}");
        assert_eq!(controls(&fx).len(), n, "showing a code commits nothing");
        let lines = answered(&mut fx, &mut ids, &command, Typed::Commit(Some(&right)), n);
        assert_eq!(lines, [committed_line(&fx, &named)]);
        let events = controls(&fx);
        assert_eq!(events.len(), n + 1, "one event per command");
        assert_eq!(
            member_text(events.last().unwrap(), "event_type"),
            Some(event_type)
        );
    }
}

/// DEC-2507 item 1: without a code a resume and a Stop print the code `agent::code` gives and
/// commit nothing; a release prints the warning the payload records as shown, before its code.
#[test]
fn a_resume_or_stop_without_a_code_shows_it_and_commits_nothing() {
    let (mut fx, mut ids) = (Fixture::new(), FixedIds::default());
    for (command, warned) in [
        (Owned::Resume, false),
        (Owned::Stop { release: false }, false),
        (Owned::Stop { release: true }, true),
    ] {
        let right = code_now(&fx, &command);
        let mut expected = Vec::new();
        if warned {
            expected.push(format!("warning {RELEASE_WARNING}"));
        }
        expected.push(format!("code {right}"));
        let lines = answered(&mut fx, &mut ids, &command, Typed::Show, 0);
        assert_eq!(lines, expected, "{command:?}");
    }
    assert!(controls(&fx).is_empty(), "showing a code commits nothing");
    assert_eq!(ids.assertions, 0, "and draws no assertion");
}

/// The right code commits one `OwnerCommandIssued` with step-up evidence, and the line names it.
#[test]
fn a_resume_or_stop_with_its_code_commits_one_command_and_names_it() {
    let (mut fx, mut ids) = (Fixture::new(), FixedIds::default());
    let commands = [
        (Owned::Resume, "resume"),
        (Owned::Stop { release: true }, "stop"),
    ];
    for (n, (command, named)) in commands.into_iter().enumerate() {
        let right = code_now(&fx, &command);
        let lines = answered(&mut fx, &mut ids, &command, Typed::Commit(Some(&right)), n);
        assert_eq!(lines, [committed_line(&fx, &format!("{named} {AGENT}"))]);
        let events = controls(&fx);
        assert_eq!(events.len(), n + 1, "one event per command");
        let event = events.last().unwrap();
        assert_eq!(member_text(event, "payload.command"), Some(named));
        assert_ne!(member(event, "payload.step_up"), Some(&Value::Null));
    }
}

/// PX-4: a resume and a Stop need the code, so a wrong one is refused `step_up_missing`, prints
/// nothing and commits nothing.
#[test]
fn a_wrong_code_refuses_a_resume_or_stop_and_commits_nothing() {
    let (mut fx, mut ids) = (Fixture::new(), FixedIds::default());
    for command in [Owned::Resume, Owned::Stop { release: false }] {
        let typed = Typed::Commit(Some("not-the-code"));
        match answer(
            &mut fx.journal,
            &no_store(),
            &mut ids,
            &owner(),
            AGENT,
            &command,
            typed,
            at(ASKED_AT),
        ) {
            Err(ControlError::Refused {
                reason: "step_up_missing",
            }) => {}
            Err(e) => panic!("{command:?} is refused step_up_missing, not: {e}"),
            Ok(lines) => panic!("{command:?} is refused, not: {lines:?}"),
        }
    }
    assert!(controls(&fx).is_empty());
}

/// Rule 13, DEC-158 option (c): an exit commits without a code, with a wrong one and with the right
/// one, and only the right one carries step-up evidence; `--show-code` prints the code and commits
/// nothing.
#[test]
fn an_exit_is_never_refused_and_only_its_code_carries_step_up() {
    let (mut fx, mut ids) = (Fixture::new(), FixedIds::default());
    let shown = code_now(&fx, &exit(None));
    let lines = answered(&mut fx, &mut ids, &exit(None), Typed::Show, 0);
    assert_eq!(lines, [format!("code {shown}")]);
    assert!(controls(&fx).is_empty(), "--show-code commits nothing");
    let named = format!("owner_exit {AGENT} {INSTRUMENT}");
    for (n, command) in [exit(None), exit(Some("495")), exit(Some("494"))]
        .into_iter()
        .enumerate()
    {
        let typed = match n {
            0 => None,
            1 => Some("not-the-code".to_owned()),
            _ => Some(code_now(&fx, &command)),
        };
        let lines = answered(
            &mut fx,
            &mut ids,
            &command,
            Typed::Commit(typed.as_deref()),
            n + 1,
        );
        assert_eq!(lines, [committed_line(&fx, &named)], "{command:?}");
        let events = controls(&fx);
        assert_eq!(events.len(), n + 1, "every exit commits");
        let event = events.last().unwrap();
        assert_eq!(member_text(event, "payload.command"), Some("owner_exit"));
        let evidence = member(event, "payload.step_up") != Some(&Value::Null);
        assert_eq!(
            evidence,
            n == 2,
            "only the right code carries step-up: {typed:?}"
        );
    }
}

/// DEC-2507 item 1, trading spec §5.5: only what cannot be a confirmation is refused `bid_invalid`,
/// with nothing committed, whether the code is shown or the exit committed: text that is not a
/// fixed-point decimal, a value that is not positive, a price finer than nine places, or a floor
/// above the bid. A floor equal to the bid is a confirmation, and the exit without the bid flags
/// always commits.
#[test]
fn an_exit_with_an_unusable_bid_confirmation_is_refused_and_commits_nothing() {
    let (mut fx, mut ids) = (Fixture::new(), FixedIds::default());
    for (bid, bid_size, floor) in [
        ("501.2x", "300", "495"),
        ("", "300", "495"),
        ("5e2", "300", "495"),
        ("+501.2", "300", "495"),
        ("501,2", "300", "495"),
        (".", "300", "495"),
        ("0", "300", "0"),
        ("-501.20", "300", "-510"),
        ("501.2", "0", "495"),
        ("501.2", "-1", "495"),
        ("501.2", "300", "0"),
        ("501.2", "300", "501.21"),
        ("501.2000000001", "300", "495"),
    ] {
        let command = Owned::Exit {
            instrument: INSTRUMENT.to_owned(),
            bid: Some(Confirmed {
                bid: bid.into(),
                bid_size: bid_size.into(),
                floor: floor.into(),
            }),
        };
        for typed in [Typed::Show, Typed::Commit(None)] {
            match answer(
                &mut fx.journal,
                &no_store(),
                &mut ids,
                &owner(),
                AGENT,
                &command,
                typed,
                at(ASKED_AT),
            ) {
                Err(ControlError::Refused {
                    reason: "bid_invalid",
                }) => {}
                Err(e) => panic!("{bid} {bid_size} {floor} is refused bid_invalid, not: {e}"),
                Ok(lines) => panic!("{bid} {bid_size} {floor} is refused, not: {lines:?}"),
            }
        }
    }
    assert!(
        controls(&fx).is_empty(),
        "a refused confirmation commits nothing"
    );
    let lines = answered(
        &mut fx,
        &mut ids,
        &exit(Some("501.2")),
        Typed::Commit(None),
        1,
    );
    assert_eq!(
        lines,
        [committed_line(
            &fx,
            &format!("owner_exit {AGENT} {INSTRUMENT}")
        )]
    );
    let lines = answered(&mut fx, &mut ids, &exit(None), Typed::Commit(None), 2);
    assert_eq!(
        lines,
        [committed_line(
            &fx,
            &format!("owner_exit {AGENT} {INSTRUMENT}")
        )]
    );
}

/// DEC-2507 item 1: a well-formed confirmation is normalized before it is bound, so the bid an
/// owner types as displayed (`501.20`, `00.50`, `495.`) shows and commits the same code and payload
/// as its canonical text.
#[test]
fn an_exit_normalizes_its_bid_confirmation_before_binding_it() {
    let (mut fx, mut ids) = (Fixture::new(), FixedIds::default());
    let typed = Owned::Exit {
        instrument: INSTRUMENT.to_owned(),
        bid: Some(Confirmed {
            bid: "501.20".into(),
            bid_size: "00.50".into(),
            floor: "495.".into(),
        }),
    };
    let canonical_bid = Confirmed {
        bid: "501.2".into(),
        bid_size: "0.5".into(),
        floor: "495".into(),
    };
    let canonical = Owned::Exit {
        instrument: INSTRUMENT.to_owned(),
        bid: Some(canonical_bid),
    };
    let canonical = code_now(&fx, &canonical);
    let shown = answered(&mut fx, &mut ids, &typed, Typed::Show, 0);
    assert_eq!(
        shown,
        [format!("code {canonical}")],
        "the displayed text shows the canonical code"
    );
    let lines = answered(
        &mut fx,
        &mut ids,
        &typed,
        Typed::Commit(Some(&canonical)),
        1,
    );
    assert_eq!(
        lines,
        [committed_line(
            &fx,
            &format!("owner_exit {AGENT} {INSTRUMENT}")
        )]
    );
    let events = controls(&fx);
    let event = events.last().expect("the exit commits");
    for (member_path, value) in [
        ("payload.bid", "501.2"),
        ("payload.bid_size", "0.5"),
        ("payload.floor", "495"),
    ] {
        assert_eq!(
            member_text(event, member_path),
            Some(value),
            "{member_path}"
        );
    }
    assert_ne!(
        member(event, "payload.step_up"),
        Some(&Value::Null),
        "the canonical code steps up"
    );
}

/// The M7 brief's `status` row and PX-3: the mode, the startup hold, the pending count, each
/// restriction in [`Status`]'s order or `restrictions none`, then the controls, pause first.
#[test]
fn status_prints_its_facts_then_the_controls_in_px3_order() {
    let restrictions = vec![
        Restriction::Agent {
            restriction: "drawdown_rung_2".into(),
            mode: "reduce_only".into(),
        },
        Restriction::Instrument {
            instrument: INSTRUMENT.into(),
            restriction: "stale_mark".into(),
        },
    ];
    let held = Status {
        mode: "paused".into(),
        startup_hold: true,
        pending_approvals: 2,
        restrictions,
    };
    let clear = Status {
        mode: "normal".into(),
        startup_hold: false,
        pending_approvals: 0,
        restrictions: Vec::new(),
    };
    for (status, expected) in [
        (
            held,
            vec![
                "mode paused",
                "startup_hold yes",
                "pending_approvals 2",
                "restriction agent drawdown_rung_2 reduce_only",
                "restriction instrument SPY stale_mark",
                "controls pause, stop, kill",
            ],
        ),
        (
            clear,
            vec![
                "mode normal",
                "startup_hold no",
                "pending_approvals 0",
                "restrictions none",
                "controls pause, stop, kill",
            ],
        ),
    ] {
        let lines = status_lines(&status).unwrap_or_else(|e| panic!("status lines, not: {e}"));
        assert_eq!(lines, expected);
    }
}

const SENTINELS: [&str; 4] = [
    "m7-user-sentinel",
    "m7-password-sentinel",
    "m7-host-sentinel.invalid",
    "m7-db-sentinel",
];

/// DEC-2507 items 2 and 3, with no database: each command refuses a bad owner before the journal
/// is opened or the store created, and a journal whose host does not resolve is an error at once;
/// no refusal prints anything or names the DSN. An exit's refusal here is the owner's or the
/// journal's, never the exit's own.
#[test]
fn every_command_refuses_without_a_database() {
    let [user, password, host, db] = SENTINELS;
    let dsn = format!("postgres://{user}:{password}@{host}:1/{db}");
    let store =
        std::env::temp_dir().join(format!("mandate-cli-m7-{}-refusals", std::process::id()));
    std::fs::remove_dir_all(&store).ok();
    let store_text = store.to_string_lossy().into_owned();
    let now = at(ASKED_AT);
    for (owner, code) in [
        (["ws:1", "u1", "paper"], "owner_workspace_invalid"),
        (
            ["ws1", "founder@example.com", "paper"],
            "owner_user_invalid",
        ),
        (["ws1", "u1", "staging"], "environment_invalid"),
        (["ws1", "u1", "paper"], "unavailable"),
    ] {
        for args in [
            vec!["status", AGENT, "--account", ACCOUNT],
            vec!["resume", AGENT, "--code", "c0de"],
            vec!["stop", AGENT, "--release"],
            vec!["exit", AGENT, INSTRUMENT],
            vec!["acknowledge", AGENT, EVENT, "--code", "c0de"],
            vec!["acknowledge", AGENT, EVENT, "--request", REQUEST],
            vec!["request-ack", AGENT, EVENT, "--code", "c0de"],
        ] {
            let command = parsed_as(&args, owner, &dsn, &store_text)
                .unwrap_or_else(|e| panic!("{args:?}: {e}"));
            let mut out = Vec::new();
            let ran = match &command {
                AgentCommand::Status(a) => run_status(a, &mut out),
                AgentCommand::Resume(a) => run_resume(a, now, &mut out),
                AgentCommand::Stop(a) => run_stop(a, now, &mut out),
                AgentCommand::Exit(a) => run_exit(a, now, &mut out),
                AgentCommand::Acknowledge(a) => run_acknowledge(a, now, &mut out),
                AgentCommand::RequestAck(a) => run_request_ack(a, now, &mut out),
                other => panic!("{args:?} parsed as {other:?}"),
            };
            let why = ran.map_or_else(|e| format!("{e:#}"), |()| "not refused".to_owned());
            assert!(why.contains(code), "{args:?} {code}: {why}");
            let named: Vec<&str> = SENTINELS.into_iter().filter(|s| why.contains(s)).collect();
            assert!(named.is_empty(), "{args:?} names {named:?}: {why}");
            assert!(
                out.is_empty(),
                "{args:?} printed {}",
                String::from_utf8_lossy(&out)
            );
        }
        if code != "unavailable" {
            assert!(!store.exists(), "{code}: the store is not created");
        }
    }
    std::fs::remove_dir_all(store).ok();
}
