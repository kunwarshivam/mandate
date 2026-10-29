//! `mandate agent holds` (E7-4b, DEC-2830, DEC-2144): the holds an owner can acknowledge on one
//! account, read through the fold the workspace API's list and the executor share
//! (`mandate_journal::holds`), each with the `agent acknowledge` command that lifts it. A stream it
//! cannot read is an error, never an empty list.

mod common;

use clap::Parser;
use common::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OTHER_AGENT, at, body,
    member_text, object, owner, text,
};
use mandate_canon::Value;
use mandate_cli::agent::{Command, Subject, code, command, holds};
use mandate_cli::brake::AgentCommand;
use mandate_cli::control::{ControlError, Owner};
use mandate_cli::owner::{holds_lines, independence_line};
use mandate_cli::{Cli, Command as Top};
use mandate_journal::Environment;

fn trigger(fx: &mut Fixture, agent: &str, limit: &str) -> String {
    fx.append_in(
        "paper",
        ACCOUNT_STREAM,
        "RiskLimitTriggered",
        None,
        object(&[("agent_id", text(agent)), ("limit", text(limit))]),
    )
}

fn hold(fx: &mut Fixture, agent: &str, restriction: &str, to: &str) -> String {
    fx.append_in(
        "paper",
        ACCOUNT_STREAM,
        "AgentModeApplied",
        None,
        object(&[
            ("agent", text(agent)),
            ("originated", Value::Bool(true)),
            ("restriction", text(restriction)),
            ("to", text(to)),
        ]),
    )
}

fn listed(fx: &Fixture, agent: Option<&str>) -> Vec<String> {
    match holds(&fx.journal, &owner(), ACCOUNT, agent) {
        Ok(found) => holds_lines(&found),
        Err(e) => panic!("holds: {e}"),
    }
}

/// What the listing's first line says independence requires, `yes` or `no`, whatever note follows
/// it.
fn requires(lines: &[String]) -> Option<&str> {
    lines
        .first()?
        .strip_prefix("independent approval required: ")?
        .split_whitespace()
        .next()
}

/// The `agent acknowledge` command line printed for `event`, split into its words.
fn command_for(lines: &[String], event: &str) -> Vec<String> {
    let at = lines
        .iter()
        .position(|line| line.starts_with(event))
        .unwrap_or_else(|| panic!("{event} is listed: {lines:#?}"));
    lines
        .get(at + 1)
        .map(|line| line.split_whitespace().map(str::to_owned).collect())
        .unwrap_or_default()
}

#[test]
fn each_standing_hold_is_listed_with_its_subject_and_the_command_that_lifts_it() {
    let mut fx = Fixture::new();
    let limit = trigger(&mut fx, AGENT, "max_daily_loss");
    let incomplete = hold(&mut fx, "*", "reconciliation:incomplete", "exits_only");
    let lifted = hold(&mut fx, AGENT, "reconciliation:SPY", "paused");
    hold(&mut fx, AGENT, "reconciliation:SPY", "normal");
    let lines = listed(&fx, Some(AGENT));
    assert_eq!(requires(&lines), Some("no"), "{lines:#?}");
    assert!(
        !lines.iter().any(|l| l.contains(&lifted)),
        "a lifted hold is not listed: {lines:#?}"
    );
    let line = lines
        .iter()
        .find(|l| l.starts_with(&incomplete))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    for part in [
        "subject=reconciliation",
        "holder=*",
        "held=reconciliation:incomplete",
        "mode=exits_only",
        "request=-",
        "requested_by=-",
    ] {
        assert!(line.contains(part), "{part} in {line}");
    }
    assert!(line.contains("since="), "{line}");
    assert_eq!(
        command_for(&lines, &incomplete),
        [
            "mandate",
            "agent",
            "acknowledge",
            AGENT,
            &incomplete,
            "--subject",
            "reconciliation"
        ]
    );
    let limit_line = lines
        .iter()
        .find(|l| l.starts_with(&limit))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    for part in [
        "subject=limit",
        "held=max_daily_loss",
        "mode=-",
        &format!("holder={AGENT}"),
    ] {
        assert!(limit_line.contains(part), "{part} in {limit_line}");
    }
    assert_eq!(
        command_for(&lines, &limit),
        [
            "mandate",
            "agent",
            "acknowledge",
            AGENT,
            &limit,
            "--subject",
            "limit"
        ]
    );
}

/// With an agent, only the holds that block it: its own and the account's; without one, every
/// hold, an account-wide one posted to `<agent>`, any agent of the account.
#[test]
fn an_agent_sees_only_what_blocks_it_and_no_agent_sees_every_hold() {
    let mut fx = Fixture::new();
    let mine = trigger(&mut fx, AGENT, "max_daily_loss");
    let theirs = trigger(&mut fx, OTHER_AGENT, "max_daily_loss");
    let everyone = hold(&mut fx, "*", "reconciliation:cash", "paused");
    let for_me = listed(&fx, Some(AGENT));
    assert!(for_me.iter().any(|l| l.starts_with(&mine)));
    assert!(for_me.iter().any(|l| l.starts_with(&everyone)));
    assert!(!for_me.iter().any(|l| l.contains(&theirs)), "{for_me:#?}");
    let all = listed(&fx, None);
    for event in [&mine, &theirs, &everyone] {
        assert!(
            all.iter().any(|l| l.starts_with(event.as_str())),
            "{all:#?}"
        );
    }
    assert_eq!(
        command_for(&all, &theirs).get(3).map(String::as_str),
        Some(OTHER_AGENT)
    );
    assert_eq!(
        command_for(&all, &everyone).get(3).map(String::as_str),
        Some("AGENT_ID")
    );
}

/// The printed command, run as printed, commits the acknowledgment the executor judges: version 2
/// with the hold's event and subject.
#[test]
fn the_printed_command_commits_the_acknowledgment_of_that_hold() {
    let mut fx = Fixture::new();
    let incomplete = hold(&mut fx, "*", "reconciliation:incomplete", "exits_only");
    let words = command_for(&listed(&fx, Some(AGENT)), &incomplete);
    let mut args = vec!["mandate".to_owned()];
    args.extend(words.iter().skip(1).cloned());
    args.extend(
        [
            "--workspace",
            common::WORKSPACE,
            "--user",
            common::OWNER,
            "--environment",
            "paper",
            "--journal",
            "postgres://journal@localhost/mandate",
            "--store",
            "/work/store",
        ]
        .map(str::to_owned),
    );
    let Ok(Cli {
        command: Top::Agent(AgentCommand::Acknowledge(parsed)),
    }) = Cli::try_parse_from(&args)
    else {
        panic!("{args:?} parses as `agent acknowledge`")
    };
    let ack = parsed.command();
    assert_eq!(
        ack,
        Command::Acknowledge {
            event: incomplete.clone(),
            request: None,
            subject: Some(Subject::Reconciliation),
        }
    );
    let typed = code(&fx.journal, &owner(), &parsed.agent, &ack)
        .ok()
        .flatten();
    let done = command(
        &mut fx.journal,
        &mut FixedIds::default(),
        &owner(),
        &parsed.agent,
        &ack,
        typed.as_deref(),
        at(ASKED_AT),
    );
    assert!(done.is_ok(), "{done:?}");
    let rows = fx.rows(CONTROL);
    let event = body(rows.last().unwrap_or_else(|| panic!("committed")));
    assert_eq!(
        member_text(&event, "payload.event"),
        Some(incomplete.as_str())
    );
    assert_eq!(
        member_text(&event, "payload.subject"),
        Some("reconciliation")
    );
}

fn member_role(fx: &mut Fixture, required: Value) {
    fx.append_in(
        "paper",
        CONTROL,
        "MemberRoleChanged",
        None,
        object(&[("independent_approval_required", required)]),
    );
}

/// Where the policy requires an independent user, a hold with no request prints the request to
/// make, and one with another user's unspent request prints the acknowledgment that answers it; a
/// policy record that does not read requires independence, never not.
#[test]
fn independence_prints_the_request_or_the_answer_to_it() {
    let mut fx = Fixture::new();
    let incomplete = hold(&mut fx, "*", "reconciliation:incomplete", "exits_only");
    member_role(&mut fx, Value::Bool(true));
    let lines = listed(&fx, Some(AGENT));
    assert_eq!(requires(&lines), Some("yes"), "{lines:#?}");
    assert_eq!(
        command_for(&lines, &incomplete),
        [
            "mandate",
            "agent",
            "request-ack",
            AGENT,
            &incomplete,
            "--subject",
            "reconciliation"
        ]
    );
    let checker = Owner {
        workspace: common::WORKSPACE.to_owned(),
        user: "user-checker".to_owned(),
        environment: Environment::Paper,
    };
    let ask = Command::RequestAcknowledgment {
        event: incomplete.clone(),
        subject: Some(Subject::Reconciliation),
    };
    let typed = code(&fx.journal, &checker, AGENT, &ask).ok().flatten();
    let asked = command(
        &mut fx.journal,
        &mut FixedIds::default(),
        &checker,
        AGENT,
        &ask,
        typed.as_deref(),
        at(ASKED_AT),
    )
    .unwrap_or_else(|e| panic!("request: {e}"));
    let lines = listed(&fx, Some(AGENT));
    let line = lines
        .iter()
        .find(|l| l.starts_with(&incomplete))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(
        line.contains(&format!("request={}", asked.event_id)),
        "{line}"
    );
    assert_eq!(
        command_for(&lines, &incomplete),
        [
            "mandate",
            "agent",
            "acknowledge",
            AGENT,
            &incomplete,
            "--subject",
            "reconciliation",
            "--request",
            &asked.event_id
        ]
    );
    let mut unread = Fixture::new();
    hold(&mut unread, "*", "reconciliation:incomplete", "exits_only");
    member_role(&mut unread, Value::Bool(false));
    member_role(&mut unread, text("no"));
    let lines = listed(&unread, Some(AGENT));
    assert_eq!(requires(&lines), Some("yes"), "{lines:#?}");
}

/// A restriction is listed, but no command lifts it yet: the executor lifts no restriction until
/// E7-2y.
#[test]
fn a_restriction_is_listed_without_a_command() {
    let mut fx = Fixture::new();
    let restricted = fx.append_in(
        "paper",
        ACCOUNT_STREAM,
        "AccountRestrictionChanged",
        None,
        object(&[
            ("cause", text("broker_notice")),
            ("restriction", text("blocked")),
        ]),
    );
    let lines = listed(&fx, Some(AGENT));
    let line = lines
        .iter()
        .find(|l| l.starts_with(&restricted))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(line.contains("subject=restriction"), "{line}");
    assert!(line.contains("held=broker_notice"), "{line}");
    assert!(
        command_for(&lines, &restricted)
            .first()
            .is_some_and(|w| w != "mandate"),
        "{lines:#?}"
    );
}

#[test]
fn no_hold_says_so() {
    let mut fx = Fixture::new();
    fx.append_in(
        "paper",
        ACCOUNT_STREAM,
        "StreamOpened",
        None,
        object(&[("stream_type", text("account"))]),
    );
    let lines = listed(&fx, Some(AGENT));
    assert_eq!(requires(&lines), Some("no"), "{lines:#?}");
    assert_eq!(lines.get(1..), Some(&["no holds".to_owned()][..]));
}

/// A record the fold must read and cannot is an error, never an empty list.
#[test]
fn an_unreadable_record_is_an_error_not_an_empty_list() {
    let mut fx = Fixture::new();
    fx.append_in(
        "paper",
        ACCOUNT_STREAM,
        "AgentModeApplied",
        None,
        object(&[
            ("agent", text("*")),
            ("originated", text("yes")),
            ("restriction", text("reconciliation:cash")),
            ("to", text("paused")),
        ]),
    );
    assert!(matches!(
        holds(&fx.journal, &owner(), ACCOUNT, Some(AGENT)),
        Err(ControlError::Journal(_))
    ));
    let mut torn = Fixture::new();
    torn.journal
        .push_raw(ACCOUNT_STREAM, "RiskLimitTriggered", b"not json");
    assert!(matches!(
        holds(&torn.journal, &owner(), ACCOUNT, Some(AGENT)),
        Err(ControlError::Journal(_))
    ));
    let mut asked = Fixture::new();
    asked
        .journal
        .push_raw(CONTROL, "OwnerAcknowledgmentRequested", b"not json");
    assert!(matches!(
        holds(&asked.journal, &owner(), ACCOUNT, Some(AGENT)),
        Err(ControlError::Journal(_))
    ));
}

/// An owner of another environment is refused, as every owner command is, rather than told an
/// account of holds has none; an account with no stream is an error, not an empty list.
#[test]
fn another_environment_or_no_such_account_is_refused_never_an_empty_list() {
    let mut fx = Fixture::new();
    hold(&mut fx, "*", "reconciliation:incomplete", "exits_only");
    let live = Owner {
        environment: Environment::Live,
        ..owner()
    };
    assert_eq!(
        holds(&fx.journal, &live, ACCOUNT, Some(AGENT)).err(),
        Some(ControlError::Refused {
            reason: "workspace_environment"
        })
    );
    assert!(matches!(
        holds(
            &fx.journal,
            &owner(),
            "01J0N0SVCHACC0VNT000000000",
            Some(AGENT)
        ),
        Err(ControlError::Journal(_))
    ));
}

/// A request is listed with who made it, and the owner's own request is marked as another user's
/// to answer (rule 239), not printed as a command the owner would be refused.
#[test]
fn your_own_request_is_shown_as_another_user_s_to_answer() {
    let mut fx = Fixture::new();
    let incomplete = hold(&mut fx, "*", "reconciliation:incomplete", "exits_only");
    let ask = Command::RequestAcknowledgment {
        event: incomplete.clone(),
        subject: Some(Subject::Reconciliation),
    };
    let typed = code(&fx.journal, &owner(), AGENT, &ask).ok().flatten();
    let asked = command(
        &mut fx.journal,
        &mut FixedIds::default(),
        &owner(),
        AGENT,
        &ask,
        typed.as_deref(),
        at(ASKED_AT),
    )
    .unwrap_or_else(|e| panic!("request: {e}"));
    let lines = listed(&fx, Some(AGENT));
    let line = lines
        .iter()
        .find(|l| l.starts_with(&incomplete))
        .unwrap_or_else(|| panic!("{lines:#?}"));
    assert!(
        line.contains(&format!("requested_by={}", common::OWNER)),
        "{line}"
    );
    let next = command_for(&lines, &incomplete);
    assert_eq!(
        next.iter().take(4).map(String::as_str).collect::<Vec<_>>(),
        ["another", "user", "answers", "your"],
        "{next:?}"
    );
    assert_eq!(next.last(), Some(&asked.event_id));
}

#[test]
fn the_binary_offers_agent_holds() {
    let tail = [
        "--workspace",
        "ws1",
        "--user",
        "u",
        "--environment",
        "paper",
        "--journal",
        "postgres://journal@localhost/mandate",
        "--store",
        "/work/store",
    ];
    for (args, agent) in [
        (
            vec!["mandate", "agent", "holds", "--account", ACCOUNT],
            None,
        ),
        (
            vec!["mandate", "agent", "holds", AGENT, "--account", ACCOUNT],
            Some(AGENT),
        ),
    ] {
        let mut all = args.clone();
        all.extend(tail);
        let Ok(Cli {
            command: Top::Agent(AgentCommand::Holds(given)),
        }) = Cli::try_parse_from(&all)
        else {
            panic!("{all:?}")
        };
        assert_eq!(given.agent.as_deref(), agent);
        assert_eq!(given.account, ACCOUNT);
    }
    let mut no_account = vec!["mandate", "agent", "holds"];
    no_account.extend(tail);
    assert!(
        Cli::try_parse_from(&no_account).is_err(),
        "it names the account"
    );
}

/// Backlog E7-4b holds fold minors, item 1 (Must): the listing reads only the workspace's latest
/// membership record, while the executor's §9.28 check 4 takes the strictest overlay of every agent
/// deployed on the account, so until E9-6's effective-policy fold lands the line says it is
/// advisory. The wording is written out here, not taken from the code.
#[test]
#[ignore = "pending E7-4"]
fn the_independence_line_says_it_is_advisory_until_e9_6() {
    for (independent, expected) in [
        (
            true,
            "independent approval required: yes (advisory until E9-6: the executor applies the \
             strictest policy of every agent deployed on the account)",
        ),
        (
            false,
            "independent approval required: no (advisory until E9-6: the executor applies the \
             strictest policy of every agent deployed on the account)",
        ),
    ] {
        let line = independence_line(independent).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(line, expected);
    }
}

/// What a listing's stream holds before it is listed.
type Raise = fn(&mut Fixture);

/// Every listing opens with the advisory line, whichever holds stand: a limit, a reconciliation
/// hold, a restriction, or none, under a policy that requires independence and one that does not.
#[test]
#[ignore = "pending E7-4"]
fn every_listing_opens_with_the_advisory_independence_line() {
    let kinds: [(&str, Raise); 4] = [
        ("limit", |fx| {
            trigger(fx, AGENT, "max_daily_loss");
        }),
        ("reconciliation", |fx| {
            hold(fx, "*", "reconciliation:incomplete", "exits_only");
        }),
        ("restriction", |fx| {
            fx.append_in(
                "paper",
                ACCOUNT_STREAM,
                "AccountRestrictionChanged",
                None,
                object(&[
                    ("cause", text("broker_notice")),
                    ("restriction", text("blocked")),
                ]),
            );
        }),
        ("none", |fx| {
            fx.append_in(
                "paper",
                ACCOUNT_STREAM,
                "StreamOpened",
                None,
                object(&[("stream_type", text("account"))]),
            );
        }),
    ];
    for independent in [true, false] {
        let expected = independence_line(independent).unwrap_or_else(|e| panic!("{e}"));
        for (kind, raise) in kinds {
            let mut fx = Fixture::new();
            raise(&mut fx);
            member_role(&mut fx, Value::Bool(independent));
            let lines = listed(&fx, Some(AGENT));
            assert_eq!(
                lines.first(),
                Some(&expected),
                "{kind}, independence {independent}: {lines:#?}"
            );
            assert_eq!(
                lines
                    .iter()
                    .filter(|l| l.contains("advisory until E9-6"))
                    .count(),
                1,
                "{kind}: the note is printed once, on the first line"
            );
        }
    }
}
