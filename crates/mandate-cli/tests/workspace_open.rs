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
use std::process::{Command as Process, Output};

use clap::Parser;
use clap::error::ErrorKind;
use mandate_canon::{Value, parse};
use mandate_cli::control::ControlJournal;
use mandate_cli::control::{Now, Submitted};
use mandate_cli::postgres::{JournalArgs, PgControlJournal, read_stream};
use mandate_cli::workspace::{OpenArgs, WorkspaceCommand, open};
use mandate_cli::{Cli, Command};
use mandate_journal::{AppendOutcome, StoredEvent, StreamId};
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
fn dsn_of(db: &TestDb) -> String {
    let url = std::env::var(URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The password and the host in `MANDATE_PG_URL`, which no output may show (`AGENTS.md` rule 7).
fn dsn_parts() -> Vec<String> {
    let url = std::env::var(URL_VAR).unwrap_or_default();
    let Some((credentials, location)) = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split_once('@'))
    else {
        return Vec::new();
    };
    let password = credentials.split_once(':').map(|(_, p)| p.to_owned());
    let host = location.split(['/', ':', '?']).next().map(str::to_owned);
    [password, host]
        .into_iter()
        .flatten()
        .filter(|p| !p.is_empty())
        .collect()
}

/// Runs the binary; neither of its outputs shows a part of the DSN.
fn mandate(argv: &[&str]) -> Output {
    let run = Process::new(env!("CARGO_BIN_EXE_mandate"))
        .args(argv)
        .output()
        .unwrap();
    for part in dsn_parts() {
        for (name, out) in [("stdout", &run.stdout), ("stderr", &run.stderr)] {
            let shown = String::from_utf8_lossy(out);
            assert!(
                !shown.contains(&part),
                "{argv:?}: {name} shows a DSN part: {shown}"
            );
        }
    }
    run
}

fn open_with(dsn: &str, workspace: &str, store: &Path) -> Output {
    let store = store.to_str().unwrap();
    mandate(&[
        "workspace",
        "open",
        "--workspace",
        workspace,
        "--journal",
        dsn,
        "--store",
        store,
    ])
}

fn rows(dsn: &str, stream: &str) -> Vec<StoredEvent> {
    read_stream(&dsn.into(), &StreamId::parse(stream).unwrap()).unwrap()
}

/// Refused with `code`, nothing on stdout.
fn refused(run: &Output, code: &str) {
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(!run.status.success() && run.stdout.is_empty(), "{stderr}");
    assert!(stderr.contains(code), "{stderr}");
}

/// `ctl:ws9`'s opener in paper as another writer drafts it: the journal vectors' event id, not the
/// one the CLI derives.
const FOREIGN: &str = r#"{"actor":{"build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","id":"control_services","kind":"system","version":"0.1.0"},"artifact_refs":[],"causation_id":null,"clock_source":"local","config_refs":{},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"01J8Y0A0A000000000000000S1","event_time":"2026-09-20T13:00:00.000000000Z","event_type":"StreamOpened","payload":{"stream_type":"control","workspace_id":"ws9"},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws9"}"#;
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415","symbol":"SPY"}"#;

/// The binary opens `ctl:ws1` with exactly the vectors' opener in paper, printing one line that
/// names the event and its `seq`, under an id derived from what it commits, so a fresh database
/// gets the same one. A stream that holds any event, its own opener or another writer's, is refused
/// `workspace_already_open`, with nothing printed, appended, or created. A second workspace opens
/// its own stream, and D1b's `config register` appends to the opened one.
#[test]
#[ignore = "pending E10-16"]
fn the_binary_opens_the_control_stream_once() {
    refusals_without_a_database("binary");
    let Some(db) = TestDb::new() else {
        return;
    };
    let (dsn, store, elsewhere) = (dsn_of(&db), scratch("binary"), scratch("binary-elsewhere"));
    let first = open_with(&dsn, "ws1", &store);
    let stderr = String::from_utf8_lossy(&first.stderr);
    assert!(first.status.success() && stderr.is_empty(), "{stderr}");
    let opened = rows(&dsn, "ctl:ws1");
    let [row] = opened.as_slice() else {
        panic!("one event: {opened:?}")
    };
    let stdout = String::from_utf8(first.stdout).unwrap();
    let lines: Vec<&str> = stdout.lines().collect();
    let [line] = lines.as_slice() else {
        panic!("one line: {stdout:?}")
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    let seq = row.seq.to_string();
    assert!(
        words.contains(&row.event_id.as_str()) && words.contains(&seq.as_str()),
        "{line}"
    );
    assert_eq!(seq, "1");
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
    refused(
        &open_with(&dsn, "ws1", &elsewhere),
        "workspace_already_open",
    );
    assert_eq!(rows(&dsn, "ctl:ws1").len(), 1);
    let target = JournalArgs {
        journal: dsn.as_str().into(),
        store: store.clone(),
    };
    let mut writer = PgControlJournal::open(&target).unwrap();
    let ws9 = StreamId::parse("ctl:ws9").unwrap();
    let epoch = writer.take_ownership(&ws9).unwrap();
    let seeded = writer.append(&ws9, 0, epoch, now().at, &[FOREIGN.as_bytes()]);
    assert!(
        matches!(seeded, Ok(AppendOutcome::Committed(_))),
        "{seeded:?}"
    );
    refused(
        &open_with(&dsn, "ws9", &elsewhere),
        "workspace_already_open",
    );
    assert_eq!(
        rows(&dsn, "ctl:ws9").len(),
        1,
        "another writer's opener stays alone"
    );
    assert!(!elsewhere.exists(), "a refusal creates no store");
    let other = open_with(&dsn, "ws2", &store);
    let stderr = String::from_utf8_lossy(&other.stderr);
    assert!(other.status.success() && stderr.is_empty(), "{stderr}");
    if let Some(fresh) = TestDb::new() {
        let again = dsn_of(&fresh);
        assert!(open_with(&again, "ws1", &elsewhere).status.success());
        let derived = rows(&again, "ctl:ws1").pop().map(|r| r.event_id);
        assert_eq!(
            derived.as_ref(),
            Some(&row.event_id),
            "the id is derived, not drawn"
        );
    }
    let spy = store.join("spy.json");
    std::fs::write(&spy, SPY).unwrap();
    let registered = mandate(&[
        "config",
        "register",
        "--kind",
        "instrument_snapshot",
        spy.to_str().unwrap(),
        "--workspace",
        "ws1",
        "--user",
        "u1",
        "--journal",
        &dsn,
        "--store",
        store.to_str().unwrap(),
    ]);
    let stderr = String::from_utf8_lossy(&registered.stderr);
    assert!(registered.status.success(), "{stderr}");
    let stream = rows(&dsn, "ctl:ws1");
    let shape: Vec<(u64, &str)> = stream
        .iter()
        .map(|r| (r.seq, r.event_type.as_str()))
        .collect();
    assert_eq!(
        shape,
        [(1, "StreamOpened"), (2, "ConfigSnapshotRegistered")]
    );
    for made in [store, elsewhere] {
        std::fs::remove_dir_all(made).ok();
    }
}
