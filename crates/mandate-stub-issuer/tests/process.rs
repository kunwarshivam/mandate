//! The binary: its refusals write their code alone and exit 2 (DEC-2100 items 3, 5 and 10), and a
//! started stub announces its issuer, serves, and stops on SIGTERM or SIGINT, writing nothing else.

mod common;

use std::io::{BufRead, BufReader, Read};
use std::process::{Command, Output, Stdio};
use std::time::Duration;

const BIN: &str = env!("CARGO_BIN_EXE_mandate-stub-issuer");

fn stub(tier: Option<&str>, bind: &str) -> Command {
    let mut command = Command::new(BIN);
    command
        .args(["--bind", bind])
        .env_remove("MANDATE_DEPLOYMENT_TIER");
    if let Some(tier) = tier {
        command.env("MANDATE_DEPLOYMENT_TIER", tier);
    }
    command
}

fn refused(output: &Output, code: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr, format!("{code}\n"), "stderr was {stderr:?}");
    assert!(output.stdout.is_empty(), "nothing on stdout");
    assert_eq!(output.status.code(), Some(2));
}

#[test]
#[ignore = "pending E21-52"]
fn outside_dev_and_ci_it_writes_its_refusal_alone() {
    for tier in [None, Some("staging"), Some("production"), Some("Dev")] {
        refused(
            &stub(tier, "127.0.0.1:8899").output().expect("ran"),
            "stub_issuer_tier",
        );
    }
}

#[test]
#[ignore = "pending E21-52"]
fn a_bind_that_is_not_a_loopback_ip_literal_is_refused() {
    for bind in ["0.0.0.0:8899", "localhost:8899", "127.0.0.1:0"] {
        refused(
            &stub(Some("dev"), bind).output().expect("ran"),
            "stub_issuer_bind",
        );
    }
}

#[test]
#[ignore = "pending E21-52"]
fn a_port_in_use_is_refused() {
    let taken = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let bind = taken.local_addr().expect("its address").to_string();
    refused(
        &stub(Some("ci"), &bind).output().expect("ran"),
        "stub_issuer_bind",
    );
}

/// A started stub, killed if a test fails before it stops.
struct Running(std::process::Child);

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Starts a stub on a free loopback port with `extra` arguments, waits for its announce line, and
/// checks that the line names the issuer, its key set's URL and `audience`, in any order.
fn started(
    extra: &[&str],
    audience: &str,
) -> (
    Running,
    BufReader<std::process::ChildStdout>,
    std::net::SocketAddr,
) {
    let free = std::net::TcpListener::bind("127.0.0.1:0").expect("a port");
    let addr = free.local_addr().expect("its address");
    drop(free);
    let mut child = Running(
        stub(Some("dev"), &addr.to_string())
            .args(extra)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("started"),
    );
    let mut stdout = BufReader::new(child.0.stdout.take().expect("stdout"));
    let mut line = String::new();
    stdout.read_line(&mut line).expect("a line");
    if line.is_empty() {
        let mut stderr = String::new();
        let _ = child
            .0
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr);
        panic!("the stub wrote no line; stderr: {stderr}");
    }
    assert!(
        line.ends_with('\n') && line.matches('\n').count() == 1,
        "{line:?}"
    );
    let mut values: Vec<&str> = line
        .split_whitespace()
        .map(|word| word.rsplit_once('=').map_or(word, |(_, value)| value))
        .collect();
    values.sort_unstable();
    let issuer = format!("http://{addr}");
    let jwks = format!("http://{addr}/jwks.json");
    let mut expected = vec![issuer.as_str(), jwks.as_str(), audience];
    expected.sort_unstable();
    assert_eq!(values, expected, "the announce line {line:?}");
    (child, stdout, addr)
}

#[test]
#[ignore = "pending E21-52"]
fn it_announces_its_issuer_serves_and_stops_on_sigterm_or_sigint_writing_nothing_else() {
    for signal in ["-TERM", "-INT"] {
        let (mut child, mut stdout, addr) = started(&[], "authenticated");
        let reply = common::send(addr, &addr.to_string(), "GET", "/jwks.json", &[], b"");
        assert_eq!(reply.status, 200);
        assert_eq!(common::mint(addr, r#"{"subject":"alice"}"#).status, 200);
        let pid = child.0.id().to_string();
        assert!(
            Command::new("kill")
                .args([signal, &pid])
                .status()
                .expect("kill")
                .success()
        );
        let stopped = (0..250).find_map(|_| {
            std::thread::sleep(Duration::from_millis(20));
            child.0.try_wait().expect("a status")
        });
        assert!(
            stopped.is_some(),
            "the stub stops within 5 seconds of {signal}"
        );
        let mut rest = String::new();
        stdout
            .read_to_string(&mut rest)
            .expect("the rest of stdout");
        let mut stderr = String::new();
        child
            .0
            .stderr
            .take()
            .expect("stderr")
            .read_to_string(&mut stderr)
            .expect("stderr");
        assert_eq!(
            (rest.as_str(), stderr.as_str()),
            ("", ""),
            "nothing after the announce line ({signal})"
        );
    }
}

#[test]
#[ignore = "pending E21-52"]
fn the_audience_given_is_announced_and_signed() {
    let (_child, _stdout, addr) = started(&["--audience", "mandate-demo"], "mandate-demo");
    let reply = common::mint(addr, r#"{"subject":"alice"}"#);
    let token = reply.json()["access_token"]
        .as_str()
        .expect("a token")
        .to_owned();
    let claims = token.split('.').nth(1).expect("claims");
    let claims: serde_json::Value = serde_json::from_slice(
        &base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, claims)
            .expect("base64url"),
    )
    .expect("JSON");
    assert_eq!(claims["aud"], "mandate-demo");
}
