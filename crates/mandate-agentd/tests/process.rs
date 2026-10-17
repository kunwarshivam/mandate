//! The `mandate-agentd` process: it refuses without a wired host, and on SIGTERM it drains and
//! exits cleanly (E21-2, DEC-968 items 4 and 6).

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
        Self(
            Command::new(BIN)
                .args(args)
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
    let stdout = child.0.stdout.take().unwrap_or_else(|| unreachable!());
    let (send, lines) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if send.send(line).is_err() {
                return;
            }
        }
    });
    let next = || {
        lines
            .recv_timeout(BOUND)
            .unwrap_or_else(|e| panic!("no announce line: {e}"))
    };
    assert!(next().starts_with("health 127.0.0.1:"));
    let metrics = next();
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
