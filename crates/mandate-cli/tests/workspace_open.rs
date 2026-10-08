//! D1c (E10-16, DEC-527 item 7): `mandate workspace open` commits the control stream's
//! `StreamOpened` once, as the vectors' `control_services` opener, in paper, and is refused on a
//! stream that holds any event. The refusals need no database, and the Postgres binary test starts
//! with them, so the pending gate sees the stub without one (DEC-510 item 3).

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use std::path::{Path, PathBuf};
use std::process::Command as Process;

use clap::Parser;
use clap::error::ErrorKind;
use mandate_canon::{Value, parse};
use mandate_cli::control::{Now, Submitted};
use mandate_cli::postgres::{JournalArgs, read_stream};
use mandate_cli::workspace::{OpenArgs, WorkspaceCommand, open};
use mandate_cli::{Cli, Command};
use mandate_journal::StreamId;
use mandate_journal_pg::APP_ROLE;
use mandate_time::UtcNanos;
use support::{TestDb, URL_VAR};

const SENTINELS: [&str; 4] = [
    "d1c-user-sentinel",
    "d1c-password-sentinel",
    "d1c-host-sentinel.invalid",
    "d1c-db-sentinel",
];

fn now() -> Now {
    Now {
        at: UtcNanos::from_parts(1_790_000_000, 0).unwrap(),
        secs: 1_790_000_000,
    }
}

/// A fresh, absent directory for one test.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("mandate-cli-d1c-{}-{name}", std::process::id()));
    std::fs::remove_dir_all(&root).ok();
    root
}

fn open_args(workspace: &str, journal: &str, store: &Path) -> OpenArgs {
    OpenArgs {
        workspace: workspace.to_owned(),
        target: JournalArgs {
            journal: journal.into(),
            store: store.to_owned(),
        },
    }
}

/// Runs `command`, which must refuse with a message carrying `code` and no part of the DSN, and
/// print nothing.
fn refuses(code: &str, command: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<Submitted>) {
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

/// The command takes a workspace and the journal, and no owner or environment.
#[test]
fn workspace_open_takes_a_workspace_and_no_owner() {
    let line = "mandate workspace open --workspace ws1 --journal postgres://h/j --store s";
    let full: Vec<&str> = line.split_whitespace().collect();
    let parsed = Cli::try_parse_from(&full).map(|cli| cli.command);
    let Ok(Command::Workspace(WorkspaceCommand::Open(args))) = parsed else {
        panic!("{parsed:?}")
    };
    assert_eq!(args.workspace, "ws1");
    let given: Vec<&str> = [&full[..3], &full[5..]].concat();
    let kind = Cli::try_parse_from(given).err().map(|e| e.kind());
    assert_eq!(
        kind,
        Some(ErrorKind::MissingRequiredArgument),
        "no --workspace"
    );
    for extra in [["--user", "u1"], ["--environment", "live"]] {
        let given: Vec<&str> = full.iter().copied().chain(extra).collect();
        let kind = Cli::try_parse_from(given).err().map(|e| e.kind());
        assert_eq!(kind, Some(ErrorKind::UnknownArgument), "{extra:?}");
    }
}

/// With no database: a bad workspace is refused before the journal is opened or the store created,
/// and a journal whose host does not resolve is an error at once. `tag` keeps each caller's files
/// apart.
fn refusals_without_a_database(tag: &str) {
    let [user, password, host, db] = SENTINELS;
    let dsn = format!("postgres://{user}:{password}@{host}:1/{db}");
    let store = scratch(&format!("{tag}-store"));
    for workspace in ["", "ws:1", "ws 1", "wé", "ws\n"] {
        let args = open_args(workspace, &dsn, &store);
        refuses("owner_workspace_invalid", |out| open(&args, now(), out));
        assert!(!store.exists(), "{workspace:?}: the store is not created");
    }
    let args = open_args("ws1", &dsn, &store);
    refuses("unavailable", |out| open(&args, now(), out));
    std::fs::remove_dir_all(store).ok();
}

#[test]
#[ignore = "pending E10-16"]
fn a_bad_workspace_or_an_unreachable_journal_is_refused_without_a_database() {
    refusals_without_a_database("refusals");
}

/// A DSN acting as the application role in `db`'s schema.
fn dsn(db: &TestDb) -> String {
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The binary opens `ctl:ws1` with exactly the vectors' opener in paper, printing one line that
/// names the event and its `seq`; a second run is refused `workspace_already_open`, prints
/// nothing, and appends nothing.
#[test]
#[ignore = "pending E10-16"]
fn the_binary_opens_the_control_stream_once() {
    refusals_without_a_database("binary");
    let Some(db) = TestDb::new() else {
        return;
    };
    let (dsn, store) = (dsn(&db), scratch("binary"));
    let mandate = |workspace: &str| {
        let argv = [
            "workspace",
            "open",
            "--workspace",
            workspace,
            "--journal",
            &dsn,
        ];
        Process::new(env!("CARGO_BIN_EXE_mandate"))
            .args(argv)
            .args(["--store", store.to_str().unwrap()])
            .output()
            .unwrap()
    };
    let first = mandate("ws1");
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(first.status.success() && stderr.is_empty(), "{stderr}");
    let ctl = StreamId::parse("ctl:ws1").unwrap();
    let rows = read_stream(&dsn.as_str().into(), &ctl).unwrap();
    let [row] = rows.as_slice() else {
        panic!("one event: {rows:?}")
    };
    let stdout = String::from_utf8(first.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    let [line] = lines.as_slice() else {
        panic!("one line: {stdout:?}")
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    assert!(
        words.contains(&row.event_id.as_str()) && words.contains(&"1"),
        "{line}"
    );
    assert!(!line.contains("postgres://"), "{line}");
    let body = parse(&row.body).unwrap();
    let text = |path: &[&str]| {
        let found = path.iter().try_fold(&body, |v, name| v.get(name));
        found.and_then(Value::as_str).map(str::to_owned)
    };
    let expected = [
        (&["event_type"][..], "StreamOpened"),
        (&["environment"], "paper"),
        (&["stream_id"], "ctl:ws1"),
        (&["actor", "kind"], "system"),
        (&["actor", "id"], "control_services"),
        (&["payload", "stream_type"], "control"),
        (&["payload", "workspace_id"], "ws1"),
    ];
    for (path, value) in expected {
        assert_eq!(text(path).as_deref(), Some(value), "{path:?}");
    }
    let payload = body.get("payload").and_then(Value::as_object).unwrap();
    assert_eq!(payload.len(), 2, "the payload is exactly the two members");
    let again = mandate("ws1");
    let stderr = String::from_utf8_lossy(&again.stderr);
    assert!(
        !again.status.success() && again.stdout.is_empty(),
        "{stderr}"
    );
    assert!(stderr.contains("workspace_already_open"), "{stderr}");
    assert_eq!(read_stream(&dsn.as_str().into(), &ctl).unwrap().len(), 1);
    let other = mandate("ws2");
    assert!(
        other.status.success(),
        "another workspace opens its own stream"
    );
    std::fs::remove_dir_all(store).ok();
}
