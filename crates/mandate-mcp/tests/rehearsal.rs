//! The `rh-sim` rehearsal build as a binary links it (DEC-922 items 1 and 2): plain `http` to a
//! loopback IP literal is accepted, and every host that is not loopback is refused, the pinned
//! broker's included, `https` or not; and the real OAuth login runs against the simulator binary
//! (DEC-922 item 3). `cargo xtask ci test-rh-sim` runs it; without the feature it compiles to
//! nothing, and `tests/production.rs` checks that build instead.

#![cfg(feature = "rh-sim")]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use mandate_mcp::{AuthServer, CallbackListener, McpError, PinnedEndpoint};
use mandate_mcp::{SystemMonotonic, TransportConfig};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[test]
fn the_rehearsal_build_accepts_plain_http_to_loopback() {
    for (host, endpoint) in [
        ("127.0.0.1", "http://127.0.0.1:1/mcp"),
        ("[::1]", "http://[::1]:1/mcp"),
        ("127.0.0.1", "https://127.0.0.1:1/mcp"),
    ] {
        let accepted = PinnedEndpoint::new(host, endpoint);
        assert!(accepted.is_ok(), "{endpoint}: {accepted:?}");
    }
}

#[test]
fn the_rehearsal_build_refuses_every_host_off_loopback() {
    for (host, endpoint) in [
        ("mcp.robinhood.com", "https://mcp.robinhood.com/mcp"),
        ("localhost", "https://localhost:1/mcp"),
        ("10.0.0.1", "https://10.0.0.1/mcp"),
    ] {
        let refused = PinnedEndpoint::new(host, endpoint);
        assert!(
            matches!(refused, Err(McpError::OffLoopback)),
            "{endpoint}: {refused:?}"
        );
    }
    let plain = PinnedEndpoint::new("mcp.robinhood.com", "http://mcp.robinhood.com/mcp");
    assert!(matches!(plain, Err(McpError::NotHttps)), "{plain:?}");
}

/// The variable `cargo xtask ci test-rh-sim` sets to the simulator binary it has just built.
const SIM_BINARY: &str = "MANDATE_RH_SIM_BIN";

/// The simulator process, killed when the test ends, however it ends.
struct Simulator {
    child: Child,
    endpoint: String,
}

impl Drop for Simulator {
    fn drop(&mut self) {
        drop(self.child.kill());
        drop(self.child.wait());
    }
}

/// Starts the binary over a one-account script and reads the endpoint it prints.
fn simulator(name: &str) -> Simulator {
    let binary = std::env::var(SIM_BINARY).unwrap_or_else(|_| {
        panic!(
            "{SIM_BINARY} names the simulator binary; run this through `cargo xtask ci test-rh-sim`"
        )
    });
    let script = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("rh-sim-login-{name}-{}.json", std::process::id()));
    let text = r#"{"accounts": [{"number": "5QR00001", "agentic_allowed": true,
        "cash": "750.25", "buying_power": "10000", "pattern_day_trader": false}]}"#;
    std::fs::write(&script, text).unwrap();
    let mut child = Command::new(binary)
        .arg("--script")
        .arg(&script)
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut line = String::new();
    BufReader::new(child.stdout.take().unwrap())
        .read_line(&mut line)
        .unwrap();
    let endpoint = line
        .trim()
        .strip_prefix("mandate-rh-sim: serving ")
        .unwrap()
        .to_owned();
    Simulator { child, endpoint }
}

/// The browser: one plain HTTP/1.1 request, and the whole answer.
async fn browse(url: &str, method: &str, body: &str) -> (u16, String, String) {
    let rest = url.strip_prefix("http://").unwrap();
    let (authority, target) = rest.split_at(rest.find('/').unwrap());
    let mut stream = TcpStream::connect(authority).await.unwrap();
    let request = format!(
        "{method} {target} HTTP/1.1\r\nhost: {authority}\r\nconnection: close\r\n\
         content-type: application/x-www-form-urlencoded\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut answer = Vec::new();
    let _read_until_closed = stream.read_to_end(&mut answer).await;
    let answer = String::from_utf8_lossy(&answer).into_owned();
    let (head, body) = answer.split_once("\r\n\r\n").unwrap_or((&answer, ""));
    let status = head.split(' ').nth(1).unwrap_or("0").parse().unwrap();
    let location = head
        .lines()
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("location"))
        .map(|(_, value)| value.trim().to_owned())
        .unwrap_or_default();
    (status, location, body.to_owned())
}

/// The owner at the simulator's authorization page: the form's request id, then `decision`.
async fn decide(authorize: &str, decision: &str) -> String {
    let (status, _, page) = browse(authorize, "GET", "").await;
    assert_eq!(status, 200, "{page}");
    let marker = "name=\"request\" value=\"";
    let at = page.find(marker).unwrap() + marker.len();
    let request = &page[at..at + page[at..].find('"').unwrap()];
    let approve = format!("{}/approve", authorize.split("/authorize").next().unwrap());
    let body = format!("request={request}&decision={decision}");
    let (status, location, _) = browse(&approve, "POST", &body).await;
    assert_eq!(status, 302);
    location
}

/// DEC-922 item 3: the real O1a discovery and registration and the O1b PKCE login, callback
/// listener and token exchange, against the simulator binary, with no step bypassed.
#[tokio::test]
async fn the_real_login_runs_against_the_simulator_binary() {
    let sim = simulator("approve");
    let config = TransportConfig::CONSERVATIVE;
    let endpoint = PinnedEndpoint::new("127.0.0.1", &sim.endpoint).unwrap();
    let server = AuthServer::discover(&endpoint, &["127.0.0.1"], &config)
        .await
        .unwrap();
    let (listener, redirect) = CallbackListener::bind().await.unwrap();
    let client = server.register(redirect, &config).await.unwrap();
    let (authorize, login) = server.begin(&client).unwrap();
    let callback = decide(authorize.as_str(), "approve").await;
    assert!(callback.starts_with(&redirect.uri()), "{callback}");
    let clock = SystemMonotonic::start();
    let (code, (status, _, _)) =
        tokio::join!(listener.accept(login, &clock), browse(&callback, "GET", ""));
    assert_eq!(status, 200);
    let token = server.exchange(code.unwrap(), &config).await;
    assert!(token.is_ok(), "{token:?}");
}

/// The owner's refusal at the simulator's page reaches the login as a denial.
#[tokio::test]
async fn a_refusal_at_the_simulators_page_denies_the_login() {
    let sim = simulator("deny");
    let config = TransportConfig::CONSERVATIVE;
    let endpoint = PinnedEndpoint::new("127.0.0.1", &sim.endpoint).unwrap();
    let server = AuthServer::discover(&endpoint, &["127.0.0.1"], &config)
        .await
        .unwrap();
    let (listener, redirect) = CallbackListener::bind().await.unwrap();
    let client = server.register(redirect, &config).await.unwrap();
    let (authorize, login) = server.begin(&client).unwrap();
    let callback = decide(authorize.as_str(), "deny").await;
    let clock = SystemMonotonic::start();
    let (code, _) = tokio::join!(listener.accept(login, &clock), browse(&callback, "GET", ""));
    assert!(
        matches!(code, Err(McpError::AuthorizationDenied)),
        "{code:?}"
    );
}
