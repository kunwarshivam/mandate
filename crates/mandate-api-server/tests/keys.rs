//! Fetching the issuer's key set (identity spec §6.1; DEC-978 item 6: a failed fetch is an outage,
//! never a refusal of the tokens it would have checked).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};

use axum::http::StatusCode;
use axum::routing::get;
use common::{TestIssuer, at, serve};
use mandate_api_server::keys::{FetchError, FetchedKeys, MAX_BYTES};
use mandate_api_server::token::{Checked, Issuer, Keys, TokenCheck};
use mandate_authn::{Algorithm, IssuerConfig};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime")
}

/// A key-set host whose answer the test sets: 0 serves `body` with 200, any other status serves
/// that status.
fn host(body: String, status: Arc<AtomicU16>) -> std::net::SocketAddr {
    serve(axum::Router::new().route(
        "/jwks",
        get(move || {
            let body = body.clone();
            let status = Arc::clone(&status);
            async move {
                match status.load(Ordering::SeqCst) {
                    0 => (StatusCode::OK, body),
                    code => (StatusCode::from_u16(code).expect("a status"), String::new()),
                }
            }
        }),
    ))
}

#[test]
fn only_https_or_loopback_http_urls_are_accepted() {
    for url in [
        "https://issuer.test/auth/v1/.well-known/jwks.json",
        "http://127.0.0.1:8999/jwks",
        "http://[::1]:8999/jwks",
        "http://localhost:8999/jwks",
    ] {
        assert!(FetchedKeys::new(url).is_ok(), "{url}");
    }
    for url in [
        "http://issuer.test/jwks",
        "http://10.0.0.1/jwks",
        "ftp://issuer.test/jwks",
        "file:///etc/passwd",
        "not a url",
        "",
    ] {
        assert_eq!(FetchedKeys::new(url).err(), Some(FetchError::Url), "{url}");
    }
}

#[test]
fn a_fetched_key_set_checks_tokens_and_a_failed_fetch_keeps_it() {
    let issuer = TestIssuer::new("es256-1");
    let status = Arc::new(AtomicU16::new(0));
    let addr = host(issuer.jwks_json(), Arc::clone(&status));
    let keys = Arc::new(FetchedKeys::new(&format!("http://{addr}/jwks")).expect("a fetcher"));
    assert!(keys.current().is_none(), "nothing before the first fetch");
    let config =
        IssuerConfig::new(&issuer.issuer, &["authenticated"], &[Algorithm::Es256]).expect("cfg");
    let check = Issuer::new(config, Arc::clone(&keys) as Arc<dyn Keys>);
    let token = issuer.token("alice", 1_791_003_600);
    assert_eq!(check.check(&token, at(1_791_000_000)), Checked::Unreachable);
    let runtime = runtime();
    assert_eq!(runtime.block_on(keys.fetch()), Ok(()));
    assert_eq!(
        check.check(&token, at(1_791_000_000)),
        Checked::Subject {
            issuer: issuer.issuer.clone(),
            subject: "alice".to_owned()
        }
    );
    status.store(503, Ordering::SeqCst);
    assert_eq!(runtime.block_on(keys.fetch()), Err(FetchError::Unavailable));
    assert!(
        matches!(
            check.check(&token, at(1_791_000_000)),
            Checked::Subject { .. }
        ),
        "the last key set is kept"
    );
    assert_eq!(
        check.check(
            &TestIssuer::new("es256-1").token("alice", 1_791_003_600),
            at(1_791_000_000)
        ),
        Checked::Refused,
        "another key's signature"
    );
    assert_eq!(
        check.check(&token, at(1_791_003_600 + 600)),
        Checked::Refused,
        "an expired token"
    );
}

#[test]
fn a_body_too_large_or_not_a_key_set_is_refused_and_the_last_set_kept() {
    let runtime = runtime();
    for body in [
        "x".repeat(MAX_BYTES + 1),
        "{\"keys\":".to_owned(),
        "[]".to_owned(),
    ] {
        let status = Arc::new(AtomicU16::new(0));
        let addr = host(body.clone(), status);
        let keys = FetchedKeys::new(&format!("http://{addr}/jwks")).expect("a fetcher");
        assert_eq!(
            runtime.block_on(keys.fetch()),
            Err(FetchError::Body),
            "{}",
            body.len()
        );
        assert!(keys.current().is_none());
    }
    for code in [404, 500, 302] {
        let status = Arc::new(AtomicU16::new(code));
        let addr = host(String::new(), status);
        let keys = FetchedKeys::new(&format!("http://{addr}/jwks")).expect("a fetcher");
        assert_eq!(
            runtime.block_on(keys.fetch()),
            Err(FetchError::Unavailable),
            "{code}"
        );
    }
    let keys = FetchedKeys::new("http://127.0.0.1:9/jwks").expect("a fetcher");
    assert_eq!(runtime.block_on(keys.fetch()), Err(FetchError::Unavailable));
}
