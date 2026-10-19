//! The `mandate-executord` binary (DEC-2007): it refuses a launch that names no account or no
//! journal, its `connect-check` journals and prints the record's id, and `run` answers its probes
//! and drains on SIGTERM. Every run here is over the in-memory journal and the fixture broker, so
//! no network is used; `pg.rs` runs it over Postgres.

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-executord");
const BOUND: Duration = Duration::from_secs(10);
const ACCOUNT: [&str; 4] = [
    "--workspace",
    "ws1",
    "--account",
    "01J8Z2ACCT0000000000000001",
];

fn command(args: &[&str]) -> Command {
    let mut command = Command::new(BIN);
    command
        .args(args)
        .env_remove("MANDATE_JOURNAL_DSN")
        .env_remove("MANDATE_PROCESS")
        .env_remove("MANDATE_WORKSPACE")
        .env_remove("MANDATE_SUBJECT")
        .stdin(Stdio::null());
    command
}

/// Runs the binary to its exit, bounded: a process that does not exit within the bound fails the
/// test rather than hang it.
fn output(args: &[&str]) -> (Option<i32>, String, String) {
    let mut child = Guard(
        command(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let (status, stderr) = child.exited();
    let mut stdout = String::new();
    if let Some(mut pipe) = child.0.stdout.take() {
        pipe.read_to_string(&mut stdout).ok();
    }
    (status.code(), stdout, stderr)
}

/// A child process that is killed when the test ends, passing or not.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

impl Guard {
    fn exited(&mut self) -> (ExitStatus, String) {
        let deadline = Instant::now() + BOUND;
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                let mut stderr = String::new();
                if let Some(mut pipe) = self.0.stderr.take() {
                    pipe.read_to_string(&mut stderr).ok();
                }
                return (status, stderr);
            }
            assert!(Instant::now() < deadline, "the process did not exit");
            thread::sleep(Duration::from_millis(20));
        }
    }
}

fn socket(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("executord-{}-{name}.sock", std::process::id()))
}

fn probe(socket: &PathBuf, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

/// A launch with no probe socket, no account, another process's pod, or no journal is refused
/// before anything is served.
#[test]
fn a_launch_without_an_account_or_a_journal_is_refused() {
    let path = socket("refuse");
    let path_text = path.to_str().unwrap();
    assert_eq!(
        output(&["run"]),
        (Some(2), String::new(), "probe_socket_missing\n".to_owned())
    );
    assert_eq!(
        output(&["run", "--probe", path_text, "--workspace", "ws1"]),
        (Some(2), String::new(), "account_missing\n".to_owned())
    );
    let mut run = vec!["run", "--probe", path_text];
    run.extend(ACCOUNT);
    assert_eq!(
        output(&run),
        (
            Some(2),
            String::new(),
            "executor_start_failed: journal_dsn_missing\n".to_owned()
        )
    );
    let mut pod = Guard(
        command(&["run", "--probe", path_text])
            .env("MANDATE_PROCESS", "runtime")
            .env("MANDATE_WORKSPACE", "ws1")
            .env("MANDATE_SUBJECT", "agent-a")
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let (status, stderr) = pod.exited();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, "process_not_executor\n");
    assert_eq!(
        output(&["other"]),
        (
            Some(2),
            String::new(),
            "usage: mandate-executord run|connect-check|probe\n".to_owned()
        )
    );
    assert!(!path.exists(), "a refused process serves no probe");
}

/// The connect check prints the id of the passing record it journaled; without its arguments, or
/// without a journal, it refuses.
#[test]
fn the_connect_check_prints_its_records_id() {
    let mut check = vec!["connect-check", "--fixture"];
    check.extend(ACCOUNT);
    check.extend([
        "--connection",
        "conn1",
        "--pii-ref",
        "pii_01J8Z2PQ000000000000000001",
    ]);
    let (code, stdout, stderr) = output(&check);
    assert_eq!((code, stderr.as_str()), (Some(0), ""));
    let id = stdout.strip_prefix("checked ").unwrap().trim_end();
    assert_eq!(id.len(), 26, "a ULID: {stdout}");
    assert_eq!(
        output(&["connect-check", "--fixture", "--workspace", "ws1"]),
        (
            Some(2),
            String::new(),
            "connect_check_arguments_missing\n".to_owned()
        )
    );
    let unchecked: Vec<&str> = check
        .iter()
        .copied()
        .filter(|a| *a != "--fixture")
        .collect();
    assert_eq!(
        output(&unchecked),
        (
            Some(2),
            String::new(),
            "connect_check_failed: journal_dsn_missing\n".to_owned()
        )
    );
    let mut invalid = check.clone();
    if let Some(slot) = invalid.iter_mut().find(|a| a.starts_with("pii_")) {
        *slot = "not-a-reference";
    }
    assert_eq!(
        output(&invalid),
        (
            Some(2),
            String::new(),
            "connect_check_failed: pii_ref_invalid\n".to_owned()
        )
    );
}

/// The running process answers live, and not ready until a tick lets it start and reconcile (the
/// in-memory journal has no scheduler); on SIGTERM it drains and exits cleanly.
#[test]
fn the_process_answers_its_probes_and_drains_on_sigterm() {
    let path = socket("drain");
    let mut run = vec!["run", "--fixture", "--probe", path.to_str().unwrap()];
    run.extend(ACCOUNT);
    let mut child = Guard(
        command(&run)
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + BOUND;
    while probe(&path, "livez") != Some(0) {
        assert!(Instant::now() < deadline, "never live");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(probe(&path, "readyz"), Some(1), "no tick, so not ready");
    let killed = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(0));
    assert_eq!(stderr, "drained\n");
    assert_eq!(
        probe(&path, "livez"),
        Some(1),
        "a stopped process is not live"
    );
    assert_eq!(probe(&path, "unknown"), Some(2));
    std::fs::remove_file(&path).ok();
}
