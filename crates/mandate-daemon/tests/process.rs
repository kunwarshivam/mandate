//! The `mandate-daemon` process: it refuses without a whole deployment or a journal, its `exec`
//! probe answers ready over the probe socket, and on SIGTERM it drains and exits cleanly (E21-30
//! slice 1, DEC-1012, DEC-1013; DEC-994 item 5). It runs over the in-memory journal, so no network
//! is used; `pg.rs` runs it over Postgres.

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-daemon");
const BOUND: Duration = Duration::from_secs(10);
const DEPLOYMENT: [&str; 8] = [
    "--workspace",
    "ws1",
    "--agent",
    "agent-a",
    "--connection",
    "conn-1",
    "--owner",
    "user-owner",
];

/// A child process that is killed when the test ends, passing or not.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

impl Guard {
    fn spawn(args: &[&str], socket: &PathBuf) -> Self {
        Self(
            Command::new(BIN)
                .args(args)
                .arg("--probe")
                .arg(socket)
                .env_remove("MANDATE_JOURNAL_DSN")
                .env_remove("MANDATE_PROCESS")
                .env_remove("MANDATE_WORKSPACE")
                .env_remove("MANDATE_SUBJECT")
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        )
    }

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
    std::env::temp_dir().join(format!("daemon-{}-{name}.sock", std::process::id()))
}

fn probe(socket: &PathBuf, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

/// Without `--fixture` the process runs only on the journal `MANDATE_JOURNAL_DSN` names, and with
/// none it refuses rather than running on a journal of its own (DEC-1013 item 1).
#[test]
fn without_a_whole_deployment_or_a_journal_the_process_refuses() {
    let path = socket("refuse");
    let (status, stderr) = Guard::spawn(&["run", "--workspace", "ws1"], &path).exited();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, "deployment_missing\n");
    let mut run = vec!["run"];
    run.extend(DEPLOYMENT);
    let (status, stderr) = Guard::spawn(&run, &path).exited();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, "runtime_start_failed: journal_dsn_missing\n");
    assert!(!path.exists(), "a refused process serves no probe");
}

/// A pod the deployment manager starts names its runtime by variables (DEC-1015). Until the
/// connection and owner are read from the agent's mandate document, such a launch refuses before
/// it is ready, and a pod for another process is refused outright.
#[test]
fn a_manager_launch_refuses_until_its_deployment_is_resolved() {
    let path = socket("manager");
    let launch = |process: &str| {
        let mut child = Command::new(BIN)
            .args(["run", "--probe"])
            .arg(&path)
            .env_remove("MANDATE_JOURNAL_DSN")
            .env("MANDATE_WORKSPACE", "ws1")
            .env("MANDATE_PROCESS", process)
            .env("MANDATE_SUBJECT", "agent-a")
            .env("MANDATE_GENERATION", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.wait().unwrap();
        let mut stderr = String::new();
        child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        (child.wait().unwrap().code(), stderr)
    };
    assert_eq!(
        launch("runtime"),
        (Some(2), "deployment_unresolved\n".to_owned())
    );
    assert_eq!(
        launch("executor"),
        (Some(2), "process_not_runtime\n".to_owned())
    );
    assert!(!path.exists(), "a refused process serves no probe");
}

#[test]
fn the_ready_runtime_answers_its_probes_and_drains_on_sigterm() {
    let path = socket("drain");
    let mut run = vec!["run", "--fixture"];
    run.extend(DEPLOYMENT);
    let mut child = Guard::spawn(&run, &path);
    let deadline = Instant::now() + BOUND;
    while probe(&path, "readyz") != Some(0) {
        assert!(Instant::now() < deadline, "never ready");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(probe(&path, "livez"), Some(0));
    let killed = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap();
    assert!(killed.success());
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(0));
    assert_eq!(stderr, "drained\n");
    assert_eq!(
        probe(&path, "readyz"),
        Some(1),
        "a stopped runtime is not ready"
    );
    std::fs::remove_file(&path).ok();
}
