//! R0 blocker R1b (DEC-952): paper and live never share a stream. `workspace open --environment
//! live` opens a live workspace whose control stream is live from its first event, an id whose
//! stream holds any event is refused either way, and every owner command states the workspace's
//! environment with `--environment` (`paper` by default): a command whose environment is not its
//! control stream's is refused `workspace_environment` before anything is appended, through each
//! of the three ways the CLI commits (a choice, an answer, a caused record). Oracles: the rows the
//! journal holds and the parsed command lines; `workspace_open.rs` opens both over Postgres.

mod common;

use std::path::PathBuf;

use clap::Parser;
use common::{
    ACCOUNT, ACCOUNT_STREAM, AGENT, ASKED_AT, CONTROL, FixedIds, Fixture, OWNER, WORKSPACE, at,
    object, owner, stream, text,
};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Value;
use mandate_cli::approvals::skip;
use mandate_cli::config::{ConfigKind, register};
use mandate_cli::connection::{Recorded, Request, record};
use mandate_cli::control::{ControlError, ControlJournal, Owner};
use mandate_cli::inbox::ApprovalsCommand;
use mandate_cli::register::ConfigCommand;
use mandate_cli::workspace::WorkspaceCommand;
use mandate_cli::{Cli, Command};
use mandate_journal::Environment;

const REFUSED: Result<(), ControlError> = Err(ControlError::Refused {
    reason: "workspace_environment",
});

fn live() -> Owner {
    Owner {
        environment: Environment::Live,
        ..owner()
    }
}

/// A fixture whose control stream is opened in `environment`.
fn opened(environment: &str) -> Fixture {
    let mut fx = Fixture::new();
    let payload = object(&[
        ("stream_type", text("control")),
        ("workspace_id", text(WORKSPACE)),
    ]);
    fx.append_in(environment, CONTROL, "StreamOpened", None, payload);
    fx
}

fn control_rows(fx: &Fixture) -> Vec<(String, String)> {
    fx.journal
        .rows(&stream(CONTROL))
        .unwrap()
        .into_iter()
        .map(|row| (row.event_type, row.environment))
        .collect()
}

/// A fresh, absent directory for one test.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-r1b-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

/// A registration (a choice) commits in the workspace's own environment and is refused in the
/// other, with nothing appended.
#[test]
fn a_registration_commits_only_in_its_workspaces_environment() {
    let fee = br#"{"schedule":"zero"}"#;
    for (workspace, by, admitted) in [
        ("live", live(), true),
        ("paper", owner(), true),
        ("live", owner(), false),
        ("paper", live(), false),
    ] {
        let mut fx = opened(workspace);
        let dir = scratch(&format!("register-{workspace}-{admitted}"));
        let mut store = FsArtifactStore::open(&dir).unwrap();
        let outcome = register(
            &mut fx.journal,
            &mut store,
            &by,
            ConfigKind::FeeConfig,
            fee,
            at(ASKED_AT),
        );
        let rows = control_rows(&fx);
        if admitted {
            assert!(outcome.is_ok(), "{workspace} {by:?}: {outcome:?}");
            assert_eq!(
                rows,
                [
                    ("StreamOpened".to_owned(), workspace.to_owned()),
                    ("ConfigSnapshotRegistered".to_owned(), workspace.to_owned()),
                ]
            );
        } else {
            assert_eq!(outcome.map(drop), REFUSED, "{workspace} {by:?}");
            assert_eq!(rows.len(), 1, "{workspace} {by:?}");
        }
    }
}

/// An answer (a skip of a pending approval) on a workspace of the other environment is refused
/// before it is appended.
#[test]
fn an_answer_is_refused_on_a_workspace_of_the_other_environment() {
    let mut fx = opened("live");
    let asked = fx.ask(AGENT, "10", ASKED_AT);
    let mut ids = FixedIds::default();
    let skipped = skip(
        &mut fx.journal,
        &mut ids,
        &owner(),
        AGENT,
        &asked.approval,
        at(ASKED_AT + 10),
    );
    assert_eq!(skipped.map(drop), REFUSED);
    assert_eq!(control_rows(&fx).len(), 1);
}

fn request(checked: &str) -> Request {
    Request {
        connection_id: "conn_rh_live_01".to_owned(),
        broker: "robinhood".to_owned(),
        scopes: vec!["get_accounts".to_owned()],
        account_ref: ACCOUNT.to_owned(),
        checked: checked.to_owned(),
        margin_attestation: Some("cash_account".to_owned()),
    }
}

/// A fixture with its control stream opened in `environment` and a passing live connect check
/// on the account stream; the check's event id.
fn checked(environment: &str) -> (Fixture, String) {
    let mut fx = opened(environment);
    let result = |check: &str| {
        object(&[
            ("check", text(check)),
            ("reason", Value::Null),
            ("result", text("passed")),
        ])
    };
    let payload = object(&[
        ("account_pii_ref", text("pii_r1b")),
        ("connection_id", text("conn_rh_live_01")),
        ("occasion", text("connect")),
        (
            "results",
            Value::Array(
                ["account", "contract", "environment", "scope"]
                    .into_iter()
                    .map(result)
                    .collect(),
            ),
        ),
    ]);
    let id = fx.append_in("live", ACCOUNT_STREAM, "ConnectionChecked", None, payload);
    (fx, id)
}

/// A caused record (`connection record`) commits on a live workspace and is refused on a paper
/// one, after the code is shown and before anything is appended.
#[test]
fn a_live_connection_is_recorded_only_on_a_live_workspace() {
    for (workspace, admitted) in [("live", true), ("paper", false)] {
        let (mut fx, check) = checked(workspace);
        let mut ids = FixedIds::default();
        let mut out = Vec::new();
        let shown = record(
            &mut fx.journal,
            &mut ids,
            &live(),
            &request(&check),
            None,
            at(ASKED_AT),
            &mut out,
        );
        let Ok(Recorded::Shown { code }) = shown else {
            panic!("{workspace}: {shown:?}")
        };
        let outcome = record(
            &mut fx.journal,
            &mut ids,
            &live(),
            &request(&check),
            Some(&code),
            at(ASKED_AT),
            &mut Vec::new(),
        );
        let rows = control_rows(&fx);
        if admitted {
            assert!(matches!(outcome, Ok(Recorded::Committed(_))), "{outcome:?}");
            assert_eq!(
                rows.last(),
                Some(&("ConnectionEstablished".to_owned(), "live".to_owned()))
            );
        } else {
            assert_eq!(outcome.map(drop), REFUSED);
            assert_eq!(rows.len(), 1);
        }
    }
}

/// Every owner command takes `--environment`, `paper` by default; `workspace open` too.
#[test]
fn every_owner_command_states_its_environment() {
    let parse_line = |line: &str| {
        let words: Vec<&str> = line.split_whitespace().collect();
        Cli::try_parse_from(&words).map_err(|e| e.to_string())
    };
    let register = |extra: &str| {
        let line = format!(
            "mandate config register --kind fee_config fee.json --workspace {WORKSPACE} --user \
             {OWNER} --journal postgres://h/j --store s {extra}"
        );
        let Command::Config(ConfigCommand::Register(args)) = parse_line(&line).unwrap().command
        else {
            panic!("config register")
        };
        args.owner.owner()
    };
    assert_eq!(register(""), Ok(owner()));
    assert_eq!(register("--environment paper"), Ok(owner()));
    assert_eq!(register("--environment live"), Ok(live()));
    assert_eq!(
        register("--environment backtest"),
        Err(ControlError::Refused {
            reason: "environment_invalid"
        })
    );
    let line = format!(
        "mandate approvals list --agent {AGENT} --workspace {WORKSPACE} --user {OWNER} \
         --journal postgres://h/j --store s --environment live"
    );
    let Command::Approvals(ApprovalsCommand::List(args)) = parse_line(&line).unwrap().command
    else {
        panic!("approvals list")
    };
    assert_eq!(args.owner.owner(), Ok(live()));
    for (extra, environment) in [("", "paper"), ("--environment live", "live")] {
        let line = format!(
            "mandate workspace open --workspace {WORKSPACE} --journal postgres://h/j --store s \
             {extra}"
        );
        let Command::Workspace(WorkspaceCommand::Open(args)) = parse_line(&line).unwrap().command
        else {
            panic!("workspace open")
        };
        assert_eq!(args.environment, environment);
    }
}
