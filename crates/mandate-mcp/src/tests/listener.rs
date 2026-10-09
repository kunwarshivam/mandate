//! The callback listener and the login's lifetime (E7-24 O1b, DEC-859). Only loopback is dialed.

use std::net::{IpAddr, Ipv4Addr};
use std::time::Duration;

use reqwest::Url;
use secrecy::{ExposeSecret, SecretString};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use super::login::client;
use super::server::StepClock;
use crate::{
    AuthorizationCode, CallbackListener, LOGIN_LIFETIME, LoopbackRedirect, McpError, PendingLogin,
};

const STATE: &str = "Canary-State-0123456789";

pub(crate) fn secret(text: &str) -> SecretString {
    SecretString::from(text.to_owned())
}

fn login(port: u16) -> PendingLogin {
    let (state, verifier) = (secret(STATE), secret("canary-verifier"));
    let issuer = Url::parse("https://as.test/").unwrap();
    PendingLogin {
        state,
        verifier,
        client: client(port),
        issuer,
    }
}

/// The browser: one request, its write side shut, and the whole answer read.
pub(crate) async fn browse(port: u16, request: &str) -> String {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))
        .await
        .unwrap();
    stream.write_all(request.as_bytes()).await.unwrap();
    stream.shutdown().await.unwrap();
    let mut answer = Vec::new();
    let _read_until_closed_or_reset = stream.read_to_end(&mut answer).await;
    String::from_utf8_lossy(&answer).into_owned()
}

type Run = (Result<AuthorizationCode, McpError>, String, bool);

/// One login through a fresh listener at `at` on the login's clock: the result, the browser's
/// answer, and whether the port still took a connection afterwards.
pub(crate) async fn run(request: &str, at: Duration) -> Run {
    let (listener, redirect) = CallbackListener::bind().await.unwrap();
    let port = listener.socket.local_addr().unwrap().port();
    assert_eq!(redirect.uri(), format!("http://127.0.0.1:{port}/callback"));
    let clock = StepClock::default();
    clock.set(at);
    let request = request.replace('@', STATE);
    let (result, answer) =
        tokio::join!(listener.accept(login(port), &clock), browse(port, &request));
    let reopened = TcpStream::connect(("127.0.0.1", port)).await.is_ok();
    (result, answer, reopened)
}

pub(crate) const GOOD: &str = "GET /callback?code=canary-code&state=@ HTTP/1.1\r\n\r\n";

#[test]
fn a_login_lives_600_seconds() {
    assert_eq!(LOGIN_LIFETIME, Duration::from_secs(600));
}

#[tokio::test]
#[ignore = "pending E7-24"]
async fn the_listener_binds_127_0_0_1_only_on_the_port_its_redirect_names() {
    let (listener, redirect) = CallbackListener::bind().await.unwrap();
    let bound = listener.socket.local_addr().unwrap();
    assert_eq!(bound.ip(), IpAddr::V4(Ipv4Addr::LOCALHOST));
    assert_ne!(bound.port(), 0);
    assert_eq!(redirect, LoopbackRedirect::new(bound.port()).unwrap());
}

#[tokio::test]
#[ignore = "pending E7-24"]
async fn one_get_callback_yields_its_code_and_then_the_port_is_closed() {
    let (result, answer, reopened) = run(GOOD, Duration::ZERO).await;
    let code = result.unwrap();
    assert_eq!(code.code.expose_secret(), "canary-code");
    assert_eq!(code.verifier.expose_secret(), "canary-verifier");
    assert!(answer.starts_with("HTTP/1.1 200 "), "{answer}");
    assert!(!answer.to_lowercase().contains("canary"), "{answer}");
    assert!(!reopened, "a second connection was accepted");
}

#[tokio::test]
#[ignore = "pending E7-24"]
async fn a_bad_or_foreign_request_answers_400_and_spends_the_login() {
    let long = format!(
        "GET /callback?code=c&state=@ HTTP/1.1\r\nx: {}\r\n\r\n",
        "a".repeat(8192)
    );
    let cases = [
        (
            "GET /callback?code=c&state=x HTTP/1.1\r\n\r\n",
            "state_mismatch",
        ),
        (
            "POST /callback?code=c&state=@ HTTP/1.1\r\n\r\n",
            "malformed",
        ),
        ("GET /callback?code=c&state=@ HTTP/1.0\r\n\r\n", "malformed"),
        ("GET /callback?code=c&state=@\r\n\r\n", "malformed"),
        ("GET /callback?code=c&state=@ HTTP/1.1\r\n", "malformed"),
        ("", "malformed"),
    ];
    for (request, wanted) in cases {
        let (result, answer, reopened) = run(request, Duration::ZERO).await;
        let error = result.unwrap_err();
        let printed = format!("{error:?} {error} {answer}").to_lowercase();
        assert!(!printed.contains("canary"), "{printed}");
        assert_eq!(error.code(), wanted, "{request:?}");
        assert!(answer.starts_with("HTTP/1.1 400 "), "{answer}");
        assert!(!reopened, "{request:?}");
    }
    let (result, _, reopened) = run(&long, Duration::ZERO).await;
    assert_eq!(result.unwrap_err().code(), "malformed");
    assert!(!reopened);
}

#[tokio::test]
#[ignore = "pending E7-24"]
async fn a_callback_at_or_after_the_lifetime_is_refused() {
    let last = LOGIN_LIFETIME - Duration::from_nanos(1);
    let (result, answer, _) = run(GOOD, last).await;
    assert!(result.is_ok(), "{result:?}");
    assert!(answer.starts_with("HTTP/1.1 200 "), "{answer}");
    for at in [LOGIN_LIFETIME, LOGIN_LIFETIME + Duration::from_secs(1)] {
        let (result, answer, reopened) = run(GOOD, at).await;
        assert_eq!(result.unwrap_err().code(), "login_expired", "{at:?}");
        assert!(answer.starts_with("HTTP/1.1 400 "), "{answer}");
        assert!(!reopened);
    }
}
