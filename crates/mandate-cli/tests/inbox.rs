//! K1a (E8-3's `clap` follow-up, DEC-533): `mandate approvals list`, `show`, `approve` and `skip`.
//! The renderers run over the M7 tests' in-memory journal and the refusals need no database; the
//! binary's grant over Postgres is `grant.rs`, since J2 closed its records. What each command prints
//! is its output contract: `list` never shows the request's content, `show` shows it exactly, and a
//! refusal prints nothing and never names the DSN.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use clap::Parser;
use clap::error::ErrorKind;
use common::{AGENT, ASKED_AT, FixedIds, OTHER_AGENT, at, code_of, hash_of, owner};
use mandate_canon::to_canonical;
use mandate_cli::approvals::{Outcome, Revalidated, approve, list, message, show};
use mandate_cli::control::Submitted;
use mandate_cli::inbox::{
    ApprovalsCommand, ApproveArgs, ListArgs, ShowArgs, SkipArgs, assertion_id, granted_lines,
    list_lines, run_approve, run_list, run_show, run_skip, show_lines, skipped_line,
};
use mandate_cli::postgres::JournalArgs;
use mandate_cli::register::OwnerArgs;
use mandate_cli::{Cli, Command};
use mandate_time::UtcNanos;

const SENTINELS: [&str; 4] = [
    "k1a-user-sentinel",
    "k1a-password-sentinel",
    "k1a-host-sentinel.invalid",
    "k1a-db-sentinel",
];

fn words(line: &str) -> Vec<&str> {
    line.split_whitespace().collect()
}

fn parse(line: &str) -> Result<ApprovalsCommand, ErrorKind> {
    match Cli::try_parse_from(line.split_whitespace()) {
        Ok(Cli {
            command: Command::Approvals(command),
        }) => Ok(command),
        Ok(other) => panic!("not approvals: {other:?}"),
        Err(e) => Err(e.kind()),
    }
}

const TAIL: &str = "--workspace ws1 --user u1 --journal postgres://h/j --store s";

/// Each command takes D1b's owner and P0's journal; `list` needs an agent; `approve` needs the
/// code and waits 0 to 30 s, 30 by default; nothing takes an environment, and there is no batch
/// form (M7's control surface).
#[test]
fn the_approvals_commands_take_an_agent_and_one_approval_each() {
    let Ok(ApprovalsCommand::List(args)) = parse(&format!(
        "mandate approvals list --agent a --agent b {TAIL}"
    )) else {
        panic!("list")
    };
    assert_eq!(args.agents, ["a", "b"]);
    let missing = parse(&format!("mandate approvals list {TAIL}"));
    assert_eq!(missing.err(), Some(ErrorKind::MissingRequiredArgument));
    let approve = format!("mandate approvals approve A1 --agent a --code c0de {TAIL}");
    let Ok(ApprovalsCommand::Approve(args)) = parse(&approve) else {
        panic!("approve")
    };
    assert_eq!(
        (args.approval.as_str(), args.code.as_str(), args.wait_s),
        ("A1", "c0de", 30)
    );
    for (given, kind) in [
        (
            format!("mandate approvals approve A1 --agent a {TAIL}"),
            ErrorKind::MissingRequiredArgument,
        ),
        (format!("{approve} --wait-s 31"), ErrorKind::ValueValidation),
        (
            format!("{approve} --environment live"),
            ErrorKind::UnknownArgument,
        ),
        (format!("{approve} --all"), ErrorKind::UnknownArgument),
        (
            format!("mandate approvals skip A1 {TAIL}"),
            ErrorKind::MissingRequiredArgument,
        ),
        (
            "mandate approvals show A1 --agent a --workspace ws1 --journal j --store s".to_owned(),
            ErrorKind::MissingRequiredArgument,
        ),
    ] {
        assert_eq!(parse(&given).err(), Some(kind), "{given}");
    }
    let Ok(ApprovalsCommand::Approve(args)) = parse(&format!("{approve} --wait-s 0")) else {
        panic!("approve --wait-s 0")
    };
    assert_eq!(args.wait_s, 0);
}

/// `list` shows opaque ids, states and seconds, pending first, and none of the request's content.
#[test]
fn list_shows_ids_states_and_seconds_and_never_the_content() {
    let mut fx = common::Fixture::new();
    let first = fx.ask(AGENT, "2", ASKED_AT);
    let other = fx.ask(OTHER_AGENT, "3", ASKED_AT + 60);
    let done = fx.ask(AGENT, "4", ASKED_AT + 10);
    fx.ended(&done, "ApprovalTimedOut");
    let now = at(ASKED_AT + 100);
    let listed = list(&fx.journal, &owner(), &[AGENT, OTHER_AGENT], now).unwrap();
    let lines = list_lines(&listed).unwrap();
    let expected = [
        (
            first.approval.as_str(),
            AGENT,
            "pending",
            first.deadline,
            first.deadline - (ASKED_AT + 100),
        ),
        (
            other.approval.as_str(),
            OTHER_AGENT,
            "pending",
            other.deadline,
            other.deadline - (ASKED_AT + 100),
        ),
        (done.approval.as_str(), AGENT, "timed_out", done.deadline, 0),
    ];
    assert_eq!(lines.len(), expected.len(), "{lines:?}");
    for (line, (approval, agent, state, deadline, remaining)) in lines.iter().zip(expected) {
        let (deadline, remaining) = (deadline.to_string(), remaining.to_string());
        let want = [
            approval,
            agent,
            state,
            "deadline",
            &deadline,
            "remaining",
            &remaining,
        ];
        assert_eq!(words(line), want, "{line}");
        for content in ["AAPL", "155", "buy", "sha256:"] {
            assert!(!line.contains(content), "{line} shows {content}");
        }
    }
    assert_eq!(list_lines(&[]).unwrap(), ["no approvals"]);
}

/// `show` prints the request's content object as its exact canonical JSON, so what the owner
/// grants is exactly the order that would be sent, with the hash the grant repeats and the code
/// that confirms it.
#[test]
fn show_prints_the_exact_order_its_hash_and_its_code() {
    let mut fx = common::Fixture::new();
    let asked = fx.ask(AGENT, "2", ASKED_AT);
    let shown = show(&fx.journal, &owner(), AGENT, &asked.approval).unwrap();
    let lines = show_lines(&shown).unwrap();
    let canonical = String::from_utf8(to_canonical(&asked.content)).unwrap();
    let deadline = asked.deadline.to_string();
    let hash = hash_of(&asked.content);
    let code = code_of(&asked.content);
    assert_eq!(
        lines,
        [
            format!("approval {} deadline {deadline}", asked.approval),
            canonical,
            format!("content_hash {hash}"),
            format!("code {code}"),
        ]
    );
}

/// `approve` prints the event it committed and the hash it granted, then the outcome line; `skip`
/// prints the event alone.
#[test]
fn an_answer_prints_its_event_and_the_runtimes_outcome() {
    let mut fx = common::Fixture::new();
    let asked = fx.ask(AGENT, "2", ASKED_AT);
    let now = at(ASKED_AT + 5);
    let code = code_of(&asked.content);
    let submitted = approve(
        &mut fx.journal,
        &mut FixedIds { assertions: 0 },
        &owner(),
        AGENT,
        &asked.approval,
        &code,
        now,
    )
    .unwrap();
    let hash = hash_of(&asked.content);
    for outcome in [
        Outcome::NotRecorded,
        Outcome::Admitted {
            revalidation: Some(Revalidated::Act),
        },
    ] {
        let lines = granted_lines(&asked.approval, &hash, &submitted, &outcome, now).unwrap();
        let seq = submitted.seq.to_string();
        let first = [
            "granted",
            asked.approval.as_str(),
            "as",
            "event",
            submitted.event_id.as_str(),
            "at",
            "seq",
            &seq,
            "for",
            &hash,
        ];
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert_eq!(words(&lines[0]), first);
        assert_eq!(lines[1], message(&outcome, now).unwrap());
    }
    let skipped = Submitted {
        event_id: "01J0SK1PPED000000000000000".into(),
        seq: 7,
    };
    let line = skipped_line(&asked.approval, &skipped).unwrap();
    let want = [
        "skipped",
        asked.approval.as_str(),
        "as",
        "event",
        "01J0SK1PPED000000000000000",
        "at",
        "seq",
        "7",
    ];
    assert_eq!(words(&line), want);
}

/// The binary's assertion ids: non-empty, and distinct within a command, across instants, and
/// across owners, so no grant reuses an assertion the workspace has seen (mandate spec §6.1).
#[test]
fn assertion_ids_are_fresh_per_instant_owner_and_count() {
    let mut seen = BTreeSet::new();
    let other = mandate_cli::control::Owner {
        user: "user-other".into(),
        ..owner()
    };
    for nanos in [0_u32, 1, 999_999_999] {
        let at = UtcNanos::from_parts(1_790_000_000, nanos).unwrap();
        for who in [owner(), other.clone()] {
            for count in 1..=3 {
                let id = assertion_id(&who, at, count).unwrap();
                assert!(!id.is_empty() && id.is_ascii(), "{id:?}");
                assert!(seen.insert(id.clone()), "{id} repeats");
            }
        }
    }
    let at = UtcNanos::from_parts(1_790_000_000, 5).unwrap();
    let twice = [owner(), owner()].map(|o| assertion_id(&o, at, 1).unwrap());
    assert_eq!(
        twice[0], twice[1],
        "derived, so a replayed instant gives the same id"
    );
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-k1a-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

fn target(dsn: &str, store: &Path) -> JournalArgs {
    JournalArgs {
        journal: dsn.into(),
        store: store.to_owned(),
    }
}

/// Runs `command`, which must refuse with a message carrying `code` and no part of the DSN, and
/// print nothing.
fn refuses<T: std::fmt::Debug>(
    code: &str,
    command: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<T>,
) {
    let mut out = Vec::new();
    let why = command(&mut out).map(|t| format!("not refused: {t:?}"));
    let why = why.unwrap_or_else(|e| format!("{e:#}"));
    assert!(why.contains(code), "{code}: {why}");
    let named: Vec<&str> = SENTINELS.into_iter().filter(|s| why.contains(s)).collect();
    assert!(named.is_empty(), "{code} names {named:?}: {why}");
    assert!(
        out.is_empty(),
        "{code}: printed {}",
        String::from_utf8_lossy(&out)
    );
}

/// With no database: each command refuses a bad owner before the journal is opened or the store
/// created, and a journal whose host does not resolve is an error at once; no refusal prints or
/// names the DSN.
#[test]
fn every_command_refuses_without_a_database() {
    let [user, password, host, db] = SENTINELS;
    let dsn = format!("postgres://{user}:{password}@{host}:1/{db}");
    let store = scratch("refusals");
    let now = at(ASKED_AT);
    let approval = "01J0APPR0VA1000000000000A1".to_owned();
    for (workspace, who, code) in [
        ("ws:1", "u1", "owner_workspace_invalid"),
        ("ws1", "founder@example.com", "owner_user_invalid"),
        ("ws1", "u1", "unavailable"),
    ] {
        let owner = || OwnerArgs {
            workspace: workspace.into(),
            user: who.into(),
        };
        let journal = || target(&dsn, &store);
        let agent = AGENT.to_owned();
        let list = ListArgs {
            agents: vec![agent.clone()],
            owner: owner(),
            target: journal(),
        };
        refuses(code, |out| run_list(&list, now, out));
        let show = ShowArgs {
            approval: approval.clone(),
            agent: agent.clone(),
            owner: owner(),
            target: journal(),
        };
        refuses(code, |out| run_show(&show, out));
        let approve = ApproveArgs {
            approval: approval.clone(),
            agent: agent.clone(),
            code: "c0de".into(),
            wait_s: 0,
            owner: owner(),
            target: journal(),
        };
        refuses(code, |out| run_approve(&approve, now, out));
        let skip = SkipArgs {
            approval: approval.clone(),
            agent,
            owner: owner(),
            target: journal(),
        };
        refuses(code, |out| run_skip(&skip, now, out));
        if code != "unavailable" {
            assert!(!store.exists(), "{code}: the store is not created");
        }
    }
    std::fs::remove_dir_all(store).ok();
}
