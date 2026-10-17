//! The `mandate-scheduler` process: its `exec` probe answers ready over the probe socket, and on
//! SIGTERM it drains and exits cleanly (E21-29, DEC-1010; DEC-994 item 5). It runs over the
//! in-memory journal and asks no time server, so no network is used.

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::io::Read;
use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-scheduler");
const BOUND: Duration = Duration::from_secs(10);

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
    std::env::temp_dir().join(format!("sched-{}-{name}.sock", std::process::id()))
}

fn probe(socket: &PathBuf, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

#[test]
fn without_a_probe_socket_or_a_workspace_the_process_refuses() {
    let status = Command::new(BIN).args(["run"]).output().unwrap();
    assert_eq!(status.status.code(), Some(2));
    let path = socket("refuse");
    let status = Command::new(BIN)
        .args(["run", "--probe"])
        .arg(&path)
        .output()
        .unwrap();
    assert_eq!(status.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&status.stderr),
        "workspace_missing\n"
    );
}

#[test]
fn the_ready_scheduler_answers_its_probes_and_drains_on_sigterm() {
    let path = socket("drain");
    let mut child = Guard(
        Command::new(BIN)
            .args(["run", "--workspace", "ws_1", "--fixture", "--probe"])
            .arg(&path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
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
        "a stopped scheduler is not ready"
    );
    std::fs::remove_file(&path).ok();
}
