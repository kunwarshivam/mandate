//! The fixture provider served without the issuer (DEC-1176 item 8): only its public `Host` is
//! answered, only `/alpaca/oauth/*` is served, and the browser's code redeems at the token
//! endpoint as it does beside the issuer.

#![cfg(debug_assertions)]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{Reply, send};
use mandate_stub_issuer::alpaca::{AUTHORIZE_PATH, Alpaca, Settings, TOKEN_PATH};
use mandate_stub_issuer::serve::serve_fixture;
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;

const HOST: &str = "oauth-fixture:8091";
const REDIRECT: &str = "http://127.0.0.1:8080/v1/oauth/alpaca/callback";
const SECRET: &str = "secret-canary-9f2a";
const VERIFIER: &str = "verifier-0123456789-abcdefghijklmnopqrstuvwxyz";
const FORM: (&str, &str) = ("Content-Type", "application/x-www-form-urlencoded");

/// How long a server that stopped by itself is given to say why before the test reads it as
/// serving.
const SETTLE: Duration = Duration::from_millis(300);

/// The fixture provider alone, answering [`HOST`], on a fresh loopback port. A server that stops
/// by itself fails the test with what it returned.
fn serve() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let addr = listener.local_addr().expect("its address");
    let settings = Settings {
        client_id: "client-1".to_owned(),
        client_secret: SECRET.as_bytes().to_vec(),
        redirect_uri: REDIRECT.to_owned(),
    };
    let alpaca = Arc::new(Alpaca::new(settings, Box::new(SystemRandom::new())));
    listener.set_nonblocking(true).expect("non-blocking");
    let (ended, why) = mpsc::channel();
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        let served = runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a listener");
            serve_fixture(listener, HOST.to_owned(), alpaca, std::future::pending()).await
        });
        let _ = ended.send(served);
    });
    if let Ok(served) = why.recv_timeout(SETTLE) {
        panic!("the fixture stopped serving: {served:?}");
    }
    addr
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(b).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn joined(params: &[(&str, &str)]) -> String {
    params
        .iter()
        .map(|(name, value)| format!("{name}={}", encode(value)))
        .collect::<Vec<_>>()
        .join("&")
}

fn authorize_path() -> String {
    let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, VERIFIER.as_bytes()));
    let query = joined(&[
        ("response_type", "code"),
        ("client_id", "client-1"),
        ("redirect_uri", REDIRECT),
        ("state", "st-1"),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("scope", "trading data"),
        ("env", "paper"),
    ]);
    format!("{AUTHORIZE_PATH}?{query}")
}

fn get(addr: SocketAddr, host: &str, path: &str) -> Reply {
    send(addr, host, "GET", path, &[], b"")
}

#[test]
#[ignore = "pending E21-33"]
fn the_browsers_code_redeems_for_a_token_at_the_public_host() {
    let addr = serve();
    let redirected = get(addr, HOST, &authorize_path());
    assert_eq!(redirected.status, 302);
    let location = redirected.header("location").expect("a Location");
    let code = location
        .strip_prefix(&format!("{REDIRECT}?"))
        .expect("the registered redirect URI")
        .split('&')
        .find_map(|pair| pair.strip_prefix("code="))
        .expect("a code")
        .to_owned();
    let body = joined(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("client_id", "client-1"),
        ("client_secret", SECRET),
        ("redirect_uri", REDIRECT),
        ("code_verifier", VERIFIER),
    ]);
    let token = send(addr, HOST, "POST", TOKEN_PATH, &[FORM], body.as_bytes());
    assert_eq!(
        token.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&token.body)
    );
    assert!(
        token.json()["access_token"].is_string(),
        "{:?}",
        token.json()
    );
    assert_eq!(token.header("cache-control"), Some("no-store"));
    assert!(!token.has_cors());
}

#[test]
#[ignore = "pending E21-33"]
fn any_host_but_its_public_host_or_two_hosts_are_misdirected() {
    let addr = serve();
    for host in [
        addr.to_string(),
        "oauth-fixture:8092".to_owned(),
        "oauth-fixture".to_owned(),
        "OAUTH-FIXTURE:8091".to_owned(),
        "127.0.0.1:8091".to_owned(),
        String::new(),
    ] {
        let reply = get(addr, &host, &authorize_path());
        assert_eq!(reply.status, 421, "{host:?}");
        assert!(reply.header("location").is_none(), "{host:?}");
    }
    let twice = send(addr, HOST, "GET", &authorize_path(), &[("Host", HOST)], b"");
    assert_eq!(twice.status, 421, "two Host headers");
}

#[test]
#[ignore = "pending E21-33"]
fn no_issuer_runs_so_its_routes_and_every_other_path_are_not_found() {
    let addr = serve();
    for (method, path, headers) in [
        ("GET", "/jwks.json", &[][..]),
        (
            "POST",
            "/token",
            &[("Content-Type", "application/json")][..],
        ),
        ("GET", "/", &[][..]),
        ("GET", "/alpaca/oauth/other", &[][..]),
        ("POST", AUTHORIZE_PATH, &[FORM][..]),
        ("GET", TOKEN_PATH, &[][..]),
    ] {
        let reply = send(
            addr,
            HOST,
            method,
            path,
            headers,
            br#"{"subject":"user_1"}"#,
        );
        assert_eq!(reply.status, 404, "{method} {path}");
        assert!(
            reply.body.is_empty() || !reply.body.starts_with(b"ey"),
            "{method} {path}"
        );
    }
}
