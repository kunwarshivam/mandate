//! What the stub answers over HTTP (DEC-2100 item 7): its own `Host` only, JSON token requests
//! only, and no CORS header on any answer.

mod common;

use common::{mint, now_s, send, serve};
use mandate_authn::{Algorithm, IssuerConfig, Jwks, TokenKind, UtcNanos, verify};
use mandate_stub_issuer::serve::MAX_BODY_BYTES;

#[test]
#[ignore = "pending E21-52"]
fn the_key_set_is_served_as_json() {
    let (issuer, addr) = serve("authenticated");
    let reply = send(addr, &addr.to_string(), "GET", "/jwks.json", &[], b"");
    assert_eq!(reply.status, 200);
    assert!(
        reply
            .header("content-type")
            .is_some_and(|media| media.starts_with("application/json")),
        "{:?}",
        reply.header("content-type")
    );
    assert_eq!(
        reply.json(),
        serde_json::from_str::<serde_json::Value>(&issuer.jwks).expect("JSON")
    );
    assert!(!reply.has_cors());
}

#[test]
#[ignore = "pending E21-52"]
fn a_json_token_request_gets_a_token_the_server_verifies() {
    let (issuer, addr) = serve("authenticated");
    let reply = mint(addr, r#"{"subject":"alice"}"#);
    assert_eq!(reply.status, 200);
    assert!(!reply.has_cors());
    let answer = reply.json();
    assert_eq!(answer["expires_in"], 300);
    let token = answer["access_token"].as_str().expect("a token");
    let config = IssuerConfig::new(&issuer.issuer, &["authenticated"], &[Algorithm::Es256])
        .expect("the issuer");
    let jwks = Jwks::parse(&issuer.jwks).expect("a key set");
    let now = UtcNanos::from_parts(now_s(), 0).expect("a time");
    let subject = verify(token, &config, &jwks, TokenKind::AccessToken, now).expect("it verifies");
    assert_eq!(subject.subject(), "alice");
}

#[test]
#[ignore = "pending E21-52"]
fn json_with_parameters_or_another_case_is_json() {
    let (_, addr) = serve("authenticated");
    for media in [
        "application/json; charset=utf-8",
        "Application/JSON",
        "application/json;charset=UTF-8",
    ] {
        let reply = send(
            addr,
            &addr.to_string(),
            "POST",
            "/token",
            &[("Content-Type", media)],
            br#"{"subject":"a"}"#,
        );
        assert_eq!(reply.status, 200, "{media}");
    }
}

#[test]
#[ignore = "pending E21-52"]
fn another_host_is_421_for_every_route() {
    let (_, addr) = serve("authenticated");
    let port = addr.port();
    for host in [
        format!("localhost:{port}"),
        format!("127.0.0.1:{port}."),
        "127.0.0.1".to_owned(),
        format!("[::1]:{port}"),
        format!("evil.test:{port}"),
        String::new(),
    ] {
        let get = send(addr, &host, "GET", "/jwks.json", &[], b"");
        let post = send(
            addr,
            &host,
            "POST",
            "/token",
            &[("Content-Type", "application/json")],
            br#"{"subject":"a"}"#,
        );
        assert_eq!((get.status, post.status), (421, 421), "{host:?}");
        assert!(!get.has_cors() && !post.has_cors());
    }
}

#[test]
#[ignore = "pending E21-52"]
fn a_token_request_that_is_not_json_is_415() {
    let (_, addr) = serve("authenticated");
    let body = br#"{"subject":"a"}"#;
    for media in [
        "text/plain",
        "application/x-www-form-urlencoded",
        "multipart/form-data; boundary=x",
        "application/jsonx",
    ] {
        let reply = send(
            addr,
            &addr.to_string(),
            "POST",
            "/token",
            &[("Content-Type", media)],
            body,
        );
        assert_eq!(reply.status, 415, "{media}");
    }
    assert_eq!(
        send(addr, &addr.to_string(), "POST", "/token", &[], body).status,
        415
    );
}

#[test]
#[ignore = "pending E21-52"]
fn a_bad_body_or_subject_is_400() {
    let (_, addr) = serve("authenticated");
    for body in [
        r#"{"subject":""}"#,
        r#"{"subject":"a\u0000"}"#,
        r#"{"subject":5}"#,
        r#"{"sub":"a"}"#,
        "{",
        "",
    ] {
        assert_eq!(mint(addr, body).status, 400, "{body}");
    }
    let long = format!(r#"{{"subject":"{}"}}"#, "a".repeat(1025));
    assert_eq!(mint(addr, &long).status, 400);
}

#[test]
#[ignore = "pending E21-52"]
fn a_body_of_the_limit_is_read_and_one_byte_more_is_413() {
    let (_, addr) = serve("authenticated");
    assert_eq!(MAX_BODY_BYTES, 4096);
    let at = |len: usize| {
        let head = r#"{"subject":"a""#;
        format!("{head}{}}}", " ".repeat(len - head.len() - 1))
    };
    assert_eq!(at(MAX_BODY_BYTES).len(), MAX_BODY_BYTES);
    assert_eq!(mint(addr, &at(MAX_BODY_BYTES)).status, 200);
    assert_eq!(mint(addr, &at(MAX_BODY_BYTES + 1)).status, 413);
}

#[test]
#[ignore = "pending E21-52"]
fn a_subject_of_the_limit_is_signed_over_http() {
    let (_, addr) = serve("authenticated");
    let body = format!(r#"{{"subject":"{}"}}"#, "a".repeat(1024));
    assert_eq!(mint(addr, &body).status, 200);
}

#[test]
#[ignore = "pending E21-52"]
fn no_host_two_hosts_or_http_1_0_is_refused() {
    let (_, addr) = serve("authenticated");
    let body = r#"{"subject":"a"}"#;
    let post = |hosts: &str, version: &str| {
        let request = format!(
            "POST /token {version}\r\n{hosts}Content-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        );
        common::raw(addr, request.as_bytes()).status
    };
    assert_eq!(post("", "HTTP/1.1"), 421, "no Host");
    let two = format!("Host: {addr}\r\nHost: evil.test\r\n");
    assert_eq!(post(&two, "HTTP/1.1"), 421, "two Host headers");
    let ours = format!("Host: {addr}\r\n");
    assert_eq!(post(&ours, "HTTP/1.1"), 200, "the control");
    assert!(post(&ours, "HTTP/1.0") >= 400, "HTTP/1.0 gets no token");
}

#[test]
#[ignore = "pending E21-52"]
fn another_host_is_421_before_any_other_answer() {
    let (_, addr) = serve("authenticated");
    let oversize = vec![b' '; MAX_BODY_BYTES + 1];
    let replies = [
        send(
            addr,
            "evil.test",
            "OPTIONS",
            "/token",
            &[("Origin", "https://evil.test")],
            b"",
        ),
        send(addr, "evil.test", "GET", "/nope", &[], b""),
        send(
            addr,
            "evil.test",
            "POST",
            "/token",
            &[("Content-Type", "text/plain")],
            b"{}",
        ),
        send(
            addr,
            "evil.test",
            "POST",
            "/token",
            &[("Content-Type", "application/json")],
            b"{",
        ),
        send(
            addr,
            "evil.test",
            "POST",
            "/token",
            &[("Content-Type", "application/json")],
            &oversize,
        ),
    ];
    for reply in &replies {
        assert_eq!(reply.status, 421);
        assert!(!reply.has_cors());
    }
}

#[test]
#[ignore = "pending E21-52"]
fn every_other_request_is_404_with_no_cors_header() {
    let (_, addr) = serve("authenticated");
    let host = addr.to_string();
    let preflight = [
        ("Origin", "https://evil.test"),
        ("Access-Control-Request-Method", "POST"),
        ("Access-Control-Request-Headers", "content-type"),
    ];
    let replies = [
        send(addr, &host, "OPTIONS", "/token", &preflight, b""),
        send(addr, &host, "GET", "/token", &[], b""),
        send(addr, &host, "POST", "/jwks.json", &[], b""),
        send(addr, &host, "GET", "/", &[], b""),
        send(
            addr,
            &host,
            "GET",
            "/.well-known/openid-configuration",
            &[],
            b"",
        ),
    ];
    for reply in &replies {
        assert_eq!(reply.status, 404);
        assert!(!reply.has_cors());
    }
}
