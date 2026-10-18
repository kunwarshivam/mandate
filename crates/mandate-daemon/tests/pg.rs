//! The runtime process over a real Postgres journal (E21-30, DEC-1013): the binary, given only
//! `MANDATE_JOURNAL_DSN`, opens its agent stream once, answers ready, and drains on SIGTERM, and a
//! second run opens nothing again. The suite's PG class: it skips unless `MANDATE_PG_URL` is set,
//! and fails without it where `MANDATE_PG_REQUIRED` is (DEC-109, DEC-519). The journal is a local
//! test schema and nothing here carries a credential (`AGENTS.md` rules 7 and 8).

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

#[path = "../../mandate-journal/tests/common/mod.rs"]
#[allow(
    unused_imports,
    reason = "only the journal's support needs these; the suite runs in `mandate-journal-pg`"
)]
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

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use mandate_journal::StreamId;
use mandate_journal_pg::APP_ROLE;
use mandate_store::{PgStore, Store};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-daemon");
const BOUND: Duration = Duration::from_secs(20);

/// The DSN of the test's own schema, acting as the journal's application role.
fn dsn(db: &support::TestDb) -> String {
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

fn probe(socket: &PathBuf, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

/// Runs the binary on `dsn` until it is ready, then sends SIGTERM and returns its exit code and
/// standard error.
fn run_until_drained(dsn: &str, socket: &PathBuf) -> (Option<i32>, String) {
    let mut child: Child = Command::new(BIN)
        .args([
            "run",
            "--workspace",
            "ws1",
            "--agent",
            "agent-a",
            "--connection",
            "conn-1",
            "--owner",
            "user-owner",
            "--probe",
        ])
        .arg(socket)
        .env("MANDATE_JOURNAL_DSN", dsn)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + BOUND;
    while probe(socket, "readyz") != Some(0) {
        if let Some(status) = child.try_wait().unwrap() {
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .ok();
            panic!("the runtime exited before it was ready: {status}: {stderr}");
        }
        assert!(Instant::now() < deadline, "never ready");
        thread::sleep(Duration::from_millis(50));
    }
    let killed = Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let deadline = Instant::now() + BOUND;
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            child.kill().ok();
            panic!("the runtime did not drain");
        }
        thread::sleep(Duration::from_millis(20));
    };
    let mut stderr = String::new();
    child
        .stderr
        .take()
        .unwrap()
        .read_to_string(&mut stderr)
        .ok();
    std::fs::remove_file(socket).ok();
    (status.code(), stderr)
}

#[test]
fn the_runtime_on_postgres_opens_its_agent_stream_once_and_drains() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let dsn = dsn(&db);
    let socket = std::env::temp_dir().join(format!("daemon-pg-{}.sock", std::process::id()));
    assert_eq!(
        run_until_drained(&dsn, &socket),
        (Some(0), "drained\n".into())
    );
    assert_eq!(
        run_until_drained(&dsn, &socket),
        (Some(0), "drained\n".into())
    );
    let store = PgStore::connect(&dsn).unwrap();
    let agent = StreamId::parse("agent:ws1:agent-a").unwrap();
    let rows = store.rows(&agent).unwrap();
    let opened = rows
        .iter()
        .filter(|r| r.event_type == "StreamOpened")
        .count();
    assert_eq!(
        opened,
        1,
        "{:?}",
        rows.iter().map(|r| &r.event_type).collect::<Vec<_>>()
    );
    assert_eq!(rows.first().map(|r| r.seq), Some(1));
}
