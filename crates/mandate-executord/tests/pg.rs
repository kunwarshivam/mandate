//! The executor binary over a real Postgres journal (DEC-2007): its `connect-check` commits
//! `StreamOpened` and `ConnectionChecked` once, a second check appends only its record, and `run`
//! on the same account stream answers live, opens nothing again, and drains on SIGTERM. The suite's
//! PG class: it skips unless `MANDATE_PG_URL` is set, and fails without it where
//! `MANDATE_PG_REQUIRED` is (DEC-109, DEC-519). The journal is a local test schema and nothing here
//! carries a credential (`AGENTS.md` rules 7 and 8).

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
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use mandate_journal::StreamId;
use mandate_journal_pg::APP_ROLE;
use mandate_store::{PgStore, Store};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-executord");
const BOUND: Duration = Duration::from_secs(20);
const ACCOUNT: [&str; 4] = [
    "--workspace",
    "ws1",
    "--account",
    "01J8Z2ACCT0000000000000001",
];

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

fn check(dsn: &str) -> (Option<i32>, String) {
    let out = Command::new(BIN)
        .arg("connect-check")
        .args(ACCOUNT)
        .args([
            "--connection",
            "conn1",
            "--pii-ref",
            "pii_01J8Z2PQ000000000000000001",
        ])
        .env("MANDATE_JOURNAL_DSN", dsn)
        .output()
        .unwrap();
    (out.status.code(), String::from_utf8(out.stdout).unwrap())
}

/// Runs the binary on `dsn` until it is live, then sends SIGTERM and returns its exit code and
/// standard error.
fn run_until_drained(dsn: &str, socket: &PathBuf) -> (Option<i32>, String) {
    let mut child = Command::new(BIN)
        .args(["run", "--probe"])
        .arg(socket)
        .args(ACCOUNT)
        .env("MANDATE_JOURNAL_DSN", dsn)
        .env_remove("MANDATE_PROCESS")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + BOUND;
    while probe(socket, "livez") != Some(0) {
        if let Some(status) = child.try_wait().unwrap() {
            let mut stderr = String::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_string(&mut stderr)
                .ok();
            panic!("the executor exited before it was live: {status}: {stderr}");
        }
        assert!(Instant::now() < deadline, "never live");
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
            panic!("the executor did not drain");
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
fn the_connect_check_and_the_executor_on_postgres() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let dsn = dsn(&db);
    let (code, first) = check(&dsn);
    assert_eq!(code, Some(0), "{first}");
    let (code, second) = check(&dsn);
    assert_eq!(code, Some(0), "{second}");
    assert_ne!(first, second);
    let socket = std::env::temp_dir().join(format!("executord-pg-{}.sock", std::process::id()));
    assert_eq!(
        run_until_drained(&dsn, &socket),
        (Some(0), "drained\n".into())
    );
    let store = PgStore::connect(&dsn).unwrap();
    let account = StreamId::parse("acct:ws1:01J8Z2ACCT0000000000000001").unwrap();
    let types: Vec<String> = store
        .rows(&account)
        .unwrap()
        .into_iter()
        .map(|r| r.event_type)
        .collect();
    assert_eq!(
        types,
        ["StreamOpened", "ConnectionChecked", "ConnectionChecked"]
    );
    let id = first.strip_prefix("checked ").unwrap().trim_end();
    let rows = store.rows(&account).unwrap();
    assert_eq!(rows.get(1).map(|r| r.event_id.as_str()), Some(id));
}
