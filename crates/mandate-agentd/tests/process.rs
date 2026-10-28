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
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_mandate-agentd");
/// How long each step a test is about may take: a later announce line, readiness, a reconciliation,
/// an exit once asked for.
const BOUND: Duration = Duration::from_secs(5);
/// How long, from spawn, the process may take to reach its first line or its exit: it runs the
/// start-up signature check before anything else, which reads and hashes its whole executable, a
/// debug build of about 90 MB. Warm that takes a tenth of a second; read cold on a CI runner where
/// many test binaries start at once, it took past [`BOUND`] (2026-10-28, #2882's `fast-test`).
const STARTUP: Duration = Duration::from_secs(30);

/// A child process that is killed when the test ends, passing or not, so a failure never leaves
/// it running, with the instant it was spawned, which [`STARTUP`] runs from.
struct Guard(Child, Instant);

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
        let spawned = Instant::now();
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
            spawned,
        )
    }

    /// The exit status and stderr of a process that refuses at start, once it exits within
    /// [`STARTUP`] of its spawn: the refusal follows the start-up check.
    fn refused(&mut self) -> (ExitStatus, String) {
        self.exit_by(self.1 + STARTUP)
    }

    /// The exit status and stderr of a process asked to exit, once it exits within [`BOUND`].
    fn exited(&mut self) -> (ExitStatus, String) {
        self.exit_by(Instant::now() + BOUND)
    }

    fn exit_by(&mut self, deadline: Instant) -> (ExitStatus, String) {
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

    /// The process's stdout, read line by line on its own thread: the first [`Lines::next`] waits
    /// until [`STARTUP`] from spawn, for the start-up check, and every later one [`BOUND`].
    fn announced(&mut self) -> Lines {
        let stdout = self.0.stdout.take().unwrap_or_else(|| unreachable!());
        let (send, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if send.send(line).is_err() {
                    return;
                }
            }
        });
        Lines {
            lines,
            first: self.1 + STARTUP,
            read: false,
        }
    }
}

/// A child's announce lines, from [`Guard::announced`].
struct Lines {
    lines: mpsc::Receiver<String>,
    first: Instant,
    read: bool,
}

impl Lines {
    /// The next line: the first by [`STARTUP`] from spawn, each later one within [`BOUND`].
    fn next(&mut self) -> String {
        let deadline = if self.read {
            Instant::now() + BOUND
        } else {
            self.first
        };
        self.read = true;
        self.lines
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap_or_else(|e| panic!("no announce line: {e}"))
    }
}

#[test]
fn without_a_wired_host_the_process_refuses() {
    let mut child = Guard::spawn(&[]);
    let (status, stderr) = child.refused();
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
        let (status, stderr) = child.refused();
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
    let line = child.announced().next();
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

fn scrape(addr: &str) -> String {
    let mut stream = TcpStream::connect(addr).unwrap_or_else(|e| panic!("connect: {e}"));
    stream
        .write_all(b"GET /metrics HTTP/1.1\r\n\r\n")
        .unwrap_or_else(|e| panic!("write: {e}"));
    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .unwrap_or_else(|e| panic!("read: {e}"));
    response
}

#[test]
fn the_process_serves_its_metrics_on_loopback() {
    let mut child = Guard::spawn(&["--fixture"]);
    let mut lines = child.announced();
    assert!(lines.next().starts_with("health 127.0.0.1:"));
    let metrics = lines.next();
    let addr = metrics
        .strip_prefix("metrics ")
        .unwrap_or_else(|| panic!("announce line: {metrics:?}"))
        .to_owned();
    assert!(addr.starts_with("127.0.0.1:"), "{addr}");
    let wanted = r#"mandate_process_reconciliations_total{process_type="agent_process",reason_code="completed"} 1"#;
    let deadline = Instant::now() + BOUND;
    let mut response = scrape(&addr);
    while !response.contains(wanted) {
        assert!(Instant::now() < deadline, "never reconciled: {response}");
        thread::sleep(Duration::from_millis(20));
        response = scrape(&addr);
    }
    assert!(response.starts_with("HTTP/1.1 200 OK\r\n"), "{response}");
    assert!(
        response.contains(
            r#"mandate_process_build_info{process_type="agent_process",release="0.0.0"} 1"#
        ),
        "{response}"
    );
}
