//! The `mandate-provisionerd` process: it refuses a start it cannot make, its `exec` probe answers
//! ready over the probe socket, and on SIGTERM it drains and exits cleanly (E21-33, DEC-2350;
//! DEC-994 item 5). It runs with `--fixture`, over an in-memory journal and a vault that keeps
//! nothing, so no network is used.

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-provisionerd");
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

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("provisionerd-{}-{name}", std::process::id()));
    let _fresh = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn probe(socket: &Path, word: &str) -> Option<i32> {
    Command::new(BIN)
        .args(["probe", word, "--probe"])
        .arg(socket)
        .status()
        .ok()
        .and_then(|s| s.code())
}

fn refused(args: &[&str]) -> (Option<i32>, String) {
    let mut child = Guard(
        Command::new(BIN)
            .args(args)
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let (status, stderr) = child.exited();
    (status.code(), stderr)
}

#[test]
fn without_a_probe_socket_a_workspace_a_namespace_or_a_vault_the_process_refuses() {
    assert_eq!(
        refused(&["run"]),
        (Some(2), "probe_socket_missing\n".to_owned())
    );
    let socket = scratch("refuse").join("p.sock");
    let socket = socket.to_str().unwrap();
    for args in [
        ["run", "--probe", socket, "--workspace", "ws_1"],
        ["run", "--probe", socket, "--namespace", "ws-1"],
    ] {
        assert_eq!(
            refused(&args),
            (Some(2), "workspace_or_namespace_missing\n".to_owned())
        );
    }
    let start = [
        "run",
        "--probe",
        socket,
        "--workspace",
        "ws_1",
        "--namespace",
        "ws-1",
    ];
    assert_eq!(
        refused(&start),
        (Some(2), "vault_address_or_dir_missing\n".to_owned())
    );
    assert_eq!(
        refused(&[&start[..], &["--vault-address", "https://127.0.0.1:1"]].concat()),
        (Some(2), "vault_address_or_dir_missing\n".to_owned())
    );
    assert_eq!(
        refused(&["serve", "--probe", socket]),
        (
            Some(2),
            "usage: mandate-provisionerd run|probe\n".to_owned()
        )
    );
}

/// Without `--fixture` the process needs the vault's CA in its vault directory, and refuses
/// rather than trusting any other root.
#[test]
fn a_vault_directory_without_the_ca_refuses_the_start() {
    let dir = scratch("noca");
    let socket = dir.join("p.sock");
    let (code, stderr) = refused(&[
        "run",
        "--probe",
        socket.to_str().unwrap(),
        "--workspace",
        "ws_1",
        "--namespace",
        "ws-1",
        "--vault-address",
        "https://127.0.0.1:1",
        "--vault-dir",
        dir.to_str().unwrap(),
    ]);
    assert_eq!(code, Some(2));
    assert_eq!(
        stderr,
        "role_provisioner_start_failed: vault_ca_unreadable\n"
    );
}

#[test]
fn a_probe_with_no_process_behind_it_fails_and_an_unknown_probe_is_refused() {
    let socket = scratch("noprobe").join("p.sock");
    assert_eq!(probe(&socket, "readyz"), Some(1));
    assert_eq!(probe(&socket, "startz"), Some(2));
}

/// The drain runs over the fixture, which a release build does not hold.
#[cfg(debug_assertions)]
#[test]
fn the_process_answers_its_probes_and_drains_on_sigterm() {
    let dir = scratch("drain");
    let socket = dir.join("p.sock");
    let mut child = Guard(
        Command::new(BIN)
            .args(["run", "--probe"])
            .arg(&socket)
            .args(["--workspace", "ws_1", "--namespace", "ws-1", "--fixture"])
            .stderr(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + BOUND;
    while probe(&socket, "readyz") != Some(0) {
        assert!(Instant::now() < deadline, "the process never became ready");
        thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(probe(&socket, "livez"), Some(0));
    let pid = child.0.id().to_string();
    assert!(
        Command::new("kill")
            .args(["-TERM", &pid])
            .status()
            .unwrap()
            .success()
    );
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(0), "{stderr}");
    assert!(
        stderr.ends_with("drained\n"),
        "it drains rather than dying: {stderr}"
    );
    assert_eq!(
        stderr
            .matches("role_provisioner_pass_halted: NotOpened")
            .count(),
        1,
        "an unopened stream is reported once, not every pass: {stderr}"
    );
}
