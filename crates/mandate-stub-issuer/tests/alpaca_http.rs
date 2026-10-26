//! The fixture Alpaca OAuth provider over HTTP (DEC-2625 items 1, 3, 4 and 6): its two routes only
//! when started with a client, a redirect only on success, forms only at the token endpoint, no
//! cache and no CORS on any answer, and nothing secret in any answer but the one token.

#![cfg(debug_assertions)]

mod common;

use std::net::SocketAddr;
use std::sync::Arc;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{Reply, send, serve};
use mandate_stub_issuer::alpaca::{AUTHORIZE_PATH, Alpaca, Settings, TOKEN_PATH};
use mandate_stub_issuer::issuer::Issuer;
use mandate_stub_issuer::serve::{MAX_BODY_BYTES, serve_with};
use ring::digest::{SHA256, digest};
use ring::rand::SystemRandom;

const REDIRECT: &str = "http://127.0.0.1:8080/v1/oauth/alpaca/callback";
const SECRET: &str = "secret-canary-9f2a";
const VERIFIER: &str = "verifier-0123456789-abcdefghijklmnopqrstuvwxyz";
const FORM: (&str, &str) = ("Content-Type", "application/x-www-form-urlencoded");

/// A stub issuer with the Alpaca provider, served on a fresh loopback port.
fn serve_alpaca() -> SocketAddr {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let addr = listener.local_addr().expect("its address");
    let issuer = Arc::new(Issuer::new(addr, "authenticated").expect("a key"));
    let settings = Settings {
        client_id: "client-1".to_owned(),
        client_secret: SECRET.as_bytes().to_vec(),
        redirect_uri: REDIRECT.to_owned(),
    };
    let alpaca = Arc::new(Alpaca::new(settings, Box::new(SystemRandom::new())));
    listener.set_nonblocking(true).expect("non-blocking");
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("a runtime");
        runtime.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener).expect("a listener");
            serve_with(listener, issuer, Some(alpaca), std::future::pending())
                .await
                .expect("serving");
        });
    });
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

fn authorize_path(env: &str) -> String {
    let challenge = URL_SAFE_NO_PAD.encode(digest(&SHA256, VERIFIER.as_bytes()));
    let query = joined(&[
        ("response_type", "code"),
        ("client_id", "client-1"),
        ("redirect_uri", REDIRECT),
        ("state", "st-1"),
        ("code_challenge", &challenge),
        ("code_challenge_method", "S256"),
        ("scope", "trading data"),
        ("env", env),
    ]);
    format!("{AUTHORIZE_PATH}?{query}")
}

fn get(addr: SocketAddr, path: &str) -> Reply {
    send(addr, &addr.to_string(), "GET", path, &[], b"")
}

fn post(addr: SocketAddr, headers: &[(&str, &str)], body: &[u8]) -> Reply {
    send(addr, &addr.to_string(), "POST", TOKEN_PATH, headers, body)
}

fn no_secret(reply: &Reply) {
    let text = String::from_utf8_lossy(&reply.body);
    let headers = format!("{:?}", reply.headers);
    for secret in [SECRET, VERIFIER] {
        assert!(
            !text.contains(secret) && !headers.contains(secret),
            "{text} {headers}"
        );
    }
    assert!(!reply.has_cors());
}

#[test]
fn the_browser_is_redirected_with_a_code_the_exchange_redeems_for_a_token() {
    let addr = serve_alpaca();
    let redirected = get(addr, &authorize_path("paper"));
    assert_eq!(redirected.status, 302);
    no_secret(&redirected);
    let location = redirected.header("location").expect("a Location");
    let query = location
        .strip_prefix(&format!("{REDIRECT}?"))
        .expect("the registered redirect URI");
    let code = query
        .split('&')
        .find_map(|pair| pair.strip_prefix("code="))
        .expect("a code");
    assert!(query.split('&').any(|pair| pair == "state=st-1"), "{query}");
    let form = joined(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", "client-1"),
        ("client_secret", SECRET),
        ("redirect_uri", REDIRECT),
        ("code_verifier", VERIFIER),
    ]);
    let token = post(addr, &[FORM], form.as_bytes());
    assert_eq!(token.status, 200);
    assert_eq!(token.header("cache-control"), Some("no-store"));
    no_secret(&token);
    let answer = token.json();
    assert_eq!(answer["token_type"], "bearer");
    assert_eq!(answer["scope"], "trading data");
    let again = post(addr, &[FORM], form.as_bytes());
    assert_eq!(again.status, 400);
    assert_eq!(again.json(), serde_json::json!({"error": "invalid_grant"}));
}

#[test]
fn a_refused_authorization_redirects_nowhere() {
    let addr = serve_alpaca();
    let refused = get(addr, &authorize_path("live"));
    assert_eq!(refused.status, 400);
    assert_eq!(refused.header("location"), None);
    assert_eq!(refused.header("cache-control"), Some("no-store"));
    assert_eq!(
        refused.json(),
        serde_json::json!({"error": "invalid_request"})
    );
    no_secret(&refused);
}

#[test]
fn the_token_endpoint_takes_only_a_form_within_the_body_bound() {
    let addr = serve_alpaca();
    let json = post(addr, &[("Content-Type", "application/json")], b"{}");
    assert_eq!(json.status, 415);
    let none = post(addr, &[], b"code=x");
    assert_eq!(none.status, 415);
    let large = vec![b'a'; MAX_BODY_BYTES + 1];
    assert_eq!(post(addr, &[FORM], &large).status, 413);
    assert!(!json.has_cors());
}

#[test]
fn without_a_client_the_provider_is_not_served() {
    let (_, addr) = serve("authenticated");
    assert_eq!(get(addr, &authorize_path("paper")).status, 404);
    assert_eq!(post(addr, &[FORM], b"code=x").status, 404);
}

#[test]
fn only_the_two_routes_and_methods_are_served() {
    let addr = serve_alpaca();
    assert_eq!(
        send(
            addr,
            &addr.to_string(),
            "POST",
            AUTHORIZE_PATH,
            &[FORM],
            b""
        )
        .status,
        404
    );
    assert_eq!(get(addr, TOKEN_PATH).status, 404);
    assert_eq!(get(addr, "/alpaca/oauth/revoke").status, 404);
    let other_host = send(addr, "localhost", "GET", &authorize_path("paper"), &[], b"");
    assert_eq!(other_host.status, 421);
}

#[test]
fn two_authorizations_redirect_with_different_random_codes() {
    let addr = serve_alpaca();
    let codes: Vec<String> = (0..2)
        .map(|_| {
            let reply = get(addr, &authorize_path("paper"));
            let location = reply.header("location").expect("a Location").to_owned();
            let code = location
                .split(['?', '&'])
                .find_map(|pair| pair.strip_prefix("code="))
                .expect("a code")
                .to_owned();
            let bytes = URL_SAFE_NO_PAD.decode(&code).expect("base64url");
            assert_ne!(bytes, vec![0_u8; 32], "{code} is all zero");
            code
        })
        .collect();
    assert_ne!(codes[0], codes[1]);
}
