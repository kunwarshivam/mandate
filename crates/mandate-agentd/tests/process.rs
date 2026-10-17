//! The `mandate-agentd` process: it refuses without a wired host, and on SIGTERM it drains and
//! exits cleanly (E21-2, DEC-968 items 4 and 6). It runs the start-up signature check first, so a
//! test build, which no release key signed, starts only as `ci` and is refused outside dev and CI
//! (E21-35, DEC-971, OPS-15).

#![allow(
    clippy::disallowed_methods,
    reason = "the tests bound their waits on a child process; no core code reads the clock"
)]

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-agentd");
const BOUND: Duration = Duration::from_secs(5);

/// A child process that is killed when the test ends, passing or not, so a failure never leaves
/// it running.
struct Guard(Child);

impl Drop for Guard {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

impl Guard {
    fn spawn(args: &[&str]) -> Self {
        Self::spawn_with(args, &[("MANDATE_DEPLOYMENT_TIER", "ci")])
    }

    /// Spawns with exactly the release check's variables `vars`, none inherited.
    fn spawn_with(args: &[&str], vars: &[(&str, &str)]) -> Self {
        Self(
            Command::new(BIN)
                .args(args)
                .env_remove("MANDATE_DEPLOYMENT_TIER")
                .env_remove("MANDATE_RELEASE_SIGNATURE")
                .env_remove("MANDATE_RELEASE_TEST_KEY")
                .envs(vars.iter().copied())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap_or_else(|e| panic!("spawn: {e}")),
        )
    }

    /// The exit status and stderr, once the process exits within the bound.
    fn exited(&mut self) -> (ExitStatus, String) {
        let deadline = Instant::now() + BOUND;
        loop {
            if let Some(status) = self.0.try_wait().unwrap_or_else(|e| panic!("wait: {e}")) {
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

#[test]
fn without_a_wired_host_the_process_refuses() {
    let mut child = Guard::spawn(&[]);
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(2));
    assert_eq!(stderr, "deployment_host_not_wired\n");
    let mut stdout = String::new();
    if let Some(mut pipe) = child.0.stdout.take() {
        pipe.read_to_string(&mut stdout).ok();
    }
    assert!(stdout.is_empty());
}

/// Outside dev and CI, which no setting means, the unsigned test build refuses before anything
/// else, with a generic code; a signature that is not cosign's is refused too.
#[test]
fn outside_dev_and_ci_an_unsigned_or_wrongly_signed_build_refuses() {
    let garbage = std::env::temp_dir().join(format!("agentd-{}.sig", std::process::id()));
    std::fs::write(&garbage, "not a signature").unwrap_or_else(|e| panic!("write: {e}"));
    let garbage = garbage.display().to_string();
    let cases: [(&[(&str, &str)], &str); 4] = [
        (&[], "release_unsigned\n"),
        (
            &[("MANDATE_DEPLOYMENT_TIER", "staging")],
            "release_unsigned\n",
        ),
        (
            &[("MANDATE_DEPLOYMENT_TIER", "production")],
            "release_unsigned\n",
        ),
        (
            &[
                ("MANDATE_DEPLOYMENT_TIER", "ci"),
                ("MANDATE_RELEASE_SIGNATURE", &garbage),
            ],
            "release_signature_malformed\n",
        ),
    ];
    for (vars, expected) in cases {
        let mut child = Guard::spawn_with(&["--fixture"], vars);
        let (status, stderr) = child.exited();
        assert_eq!(status.code(), Some(2), "{vars:?}");
        assert_eq!(stderr, expected, "{vars:?}");
    }
    std::fs::remove_file(&garbage).ok();
}

fn ready(addr: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect: {e}"));
    stream
        .write_all(b"GET /readyz HTTP/1.1\r\n\r\n")
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .unwrap_or_else(|e| panic!("read: {e}"));
    response
}

#[test]
fn sigterm_drains_the_ready_process_and_it_exits_drained() {
    let mut child = Guard::spawn(&["--fixture"]);
    let stdout = child.0.stdout.take().unwrap_or_else(|| unreachable!());
    let mut line = String::new();
    BufReader::new(stdout)
        .read_line(&mut line)
        .unwrap_or_else(|e| panic!("read: {e}"));
    let addr = line
        .trim()
        .strip_prefix("health ")
        .unwrap_or_else(|| panic!("announce line: {line:?}"))
        .to_owned();
    let deadline = Instant::now() + BOUND;
    while !ready(&addr).ends_with("\r\n\r\nready") {
        assert!(Instant::now() < deadline, "never ready");
        thread::sleep(Duration::from_millis(20));
    }
    let killed = Command::new("kill")
        .args(["-TERM", &child.0.id().to_string()])
        .status()
        .unwrap_or_else(|e| panic!("kill: {e}"));
    assert!(killed.success());
    let (status, stderr) = child.exited();
    assert_eq!(status.code(), Some(0));
    assert_eq!(stderr, "drained\n");
}
