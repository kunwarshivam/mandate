//! The HTTPS transport against a local TLS server (DEC-2200): what one attempt sends, what it hands
//! back, and which failures leave nothing sent (retryable) or a possibly billed attempt (never
//! retried).

#![allow(
    clippy::disallowed_methods,
    reason = "tests time the transport against the wall clock it runs on"
)]

mod common;

use std::time::Instant;

use common::server::{Reply, closed_port, pki, serve};
use common::{PATH, in_ms, request, send, sentinel, status_of, trusting};
use mandate_gateway::provider::{Clock, Delivery, HttpRequest, ProviderKey, Transport};
use mandate_gateway_http::{HttpsTransport, MAX_BODY_BYTES, SetupError, SystemClock, TlsRoots};

const KEY: &str = "test-key-not-a-secret-0001";

fn key() -> ProviderKey {
    ProviderKey::new(KEY.to_owned())
}

fn cap() -> usize {
    usize::try_from(MAX_BODY_BYTES).unwrap()
}

#[test]
fn one_post_reaches_the_path_with_the_body_and_the_key_as_a_bearer_header() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::ok(b"{\"answer\":1}")]);
    let mut transport = trusting(&pki);
    let delivery = send(
        &mut transport,
        &request(server.port, b"{\"q\":1}"),
        Some(&key()),
    );
    assert_eq!(
        delivery,
        Delivery::Response {
            status: 200,
            body: b"{\"answer\":1}".to_vec(),
            retry_after_ms: None,
        }
    );
    let seen = server.seen();
    assert_eq!(seen.len(), 1, "one attempt, one request");
    assert_eq!(seen[0].method, "POST");
    assert_eq!(seen[0].path, PATH);
    assert_eq!(seen[0].body, b"{\"q\":1}");
    assert_eq!(
        seen[0].header("authorization"),
        Some(format!("Bearer {KEY}").as_str())
    );
    assert_eq!(seen[0].header("content-type"), Some("application/json"));
    assert_eq!(
        seen[0].header("host"),
        Some(format!("localhost:{}", server.port).as_str())
    );
}

#[test]
fn a_call_with_no_key_sends_no_authorization() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::ok(b"{}")]);
    let delivery = send(&mut trusting(&pki), &request(server.port, b"{}"), None);
    assert_eq!(status_of(&delivery), Some(200));
    assert_eq!(server.seen()[0].header("authorization"), None);
}

#[test]
fn statuses_bodies_and_retry_after_seconds_come_back_as_sent() {
    let pki = pki();
    let server = serve(
        &pki,
        vec![
            Reply::with_header(429, "Retry-After", "7"),
            Reply::with_header(503, "Retry-After", "Wed, 21 Oct 2026 07:28:00 GMT"),
            Reply::status(500, b"upstream down"),
            Reply::with_header(429, "Retry-After", "0"),
        ],
    );
    let mut transport = trusting(&pki);
    let req = request(server.port, b"{}");
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::Response {
            status: 429,
            body: b"{}".to_vec(),
            retry_after_ms: Some(7_000),
        }
    );
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::Response {
            status: 503,
            body: b"{}".to_vec(),
            retry_after_ms: None,
        },
        "an HTTP date is not read; the gateway waits its default"
    );
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::Response {
            status: 500,
            body: b"upstream down".to_vec(),
            retry_after_ms: None,
        }
    );
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::Response {
            status: 429,
            body: b"{}".to_vec(),
            retry_after_ms: Some(0),
        }
    );
}

#[test]
fn a_redirect_is_a_status_and_is_not_followed() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::ok(b"{}"), Reply::ok(b"{}")]);
    let elsewhere = format!("https://localhost:{}/elsewhere", server.port);
    let redirecting = serve(&pki, vec![Reply::with_header(307, "Location", &elsewhere)]);
    let delivery = send(
        &mut trusting(&pki),
        &request(redirecting.port, b"{}"),
        Some(&key()),
    );
    assert_eq!(status_of(&delivery), Some(307));
    assert_eq!(redirecting.seen().len(), 1);
    sentinel(&pki, server.port);
    assert_eq!(
        server.seen().len(),
        1,
        "the redirect's target saw only the sentinel"
    );
}

#[test]
fn a_body_at_the_cap_is_read_and_one_byte_over_ends_the_attempt_as_unknown_cost() {
    let pki = pki();
    let at_cap = vec![b'a'; cap()];
    let over = vec![b'a'; cap() + 1];
    let server = serve(&pki, vec![Reply::ok(&at_cap), Reply::ok(&over)]);
    let mut transport = trusting(&pki);
    let req = request(server.port, b"{}");
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::Response {
            status: 200,
            body: at_cap,
            retry_after_ms: None,
        }
    );
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::TimedOut,
        "a body past the cap is unreadable: unknown cost, never retried"
    );
}

#[test]
fn a_declared_length_past_the_cap_ends_the_attempt_before_the_body_is_read() {
    let pki = pki();
    let server = serve(
        &pki,
        vec![Reply::CutShort {
            status: 200,
            declared: cap() + 1,
            body: vec![],
        }],
    );
    assert_eq!(
        send(
            &mut trusting(&pki),
            &request(server.port, b"{}"),
            Some(&key())
        ),
        Delivery::TimedOut
    );
}

#[test]
fn an_undeclared_body_is_capped_as_it_is_read() {
    let pki = pki();
    let at_cap = vec![b'b'; cap()];
    let server = serve(
        &pki,
        vec![
            Reply::Unsized(at_cap.clone()),
            Reply::Unsized(vec![b'b'; cap() + 1]),
        ],
    );
    let mut transport = trusting(&pki);
    let req = request(server.port, b"{}");
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::Response {
            status: 200,
            body: at_cap,
            retry_after_ms: None,
        }
    );
    assert_eq!(send(&mut transport, &req, Some(&key())), Delivery::TimedOut);
}

#[test]
fn a_reply_slower_than_the_deadline_is_timed_out_at_the_deadline() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::slow(3_000)]);
    let started = Instant::now();
    let delivery = trusting(&pki).send(
        &request(server.port, b"{}"),
        Some(&key()),
        in_ms(400),
        &mut SystemClock,
    );
    let took = started.elapsed().as_millis();
    assert_eq!(delivery, Delivery::TimedOut);
    assert!(took < 2_000, "gave up at the deadline, after {took} ms");
    assert_eq!(server.seen().len(), 1, "it was sent, so it may be billed");
}

#[test]
fn with_no_time_left_nothing_is_sent() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::ok(b"{}")]);
    let now = SystemClock.now();
    let delivery = trusting(&pki).send(
        &request(server.port, b"{}"),
        Some(&key()),
        now,
        &mut SystemClock,
    );
    assert_eq!(delivery, Delivery::TimedOut);
    sentinel(&pki, server.port);
    assert_eq!(server.handshakes(), 1, "only the sentinel was sent");
}

#[test]
fn a_refused_connection_is_connect_failed() {
    let pki = pki();
    let delivery = send(
        &mut trusting(&pki),
        &request(closed_port(), b"{}"),
        Some(&key()),
    );
    assert_eq!(delivery, Delivery::ConnectFailed);
}

#[test]
fn an_untrusted_server_is_connect_failed_and_is_sent_nothing() {
    let pki = pki();
    let stranger = common::server::pki();
    let server = serve(
        &pki,
        vec![Reply::ok(b"{}"), Reply::ok(b"{}"), Reply::ok(b"{}")],
    );
    let mut platform = HttpsTransport::new(TlsRoots::Platform).unwrap();
    assert_eq!(
        send(&mut platform, &request(server.port, b"{}"), Some(&key())),
        Delivery::ConnectFailed,
        "the platform's roots refuse the test authority"
    );
    assert_eq!(
        send(
            &mut trusting(&stranger),
            &request(server.port, b"{}"),
            Some(&key())
        ),
        Delivery::ConnectFailed,
        "another authority's roots refuse it too"
    );
    sentinel(&pki, server.port);
    assert_eq!(
        server.seen().len(),
        1,
        "no request reached the server it did not trust; only the sentinel did"
    );
    assert_eq!(
        server.handshakes(),
        1,
        "neither refused handshake completed"
    );
}

#[test]
fn a_connection_cut_mid_body_is_an_attempt_of_unknown_cost() {
    let pki = pki();
    let server = serve(
        &pki,
        vec![Reply::CutShort {
            status: 200,
            declared: 100,
            body: b"{\"model\":".to_vec(),
        }],
    );
    let delivery = send(
        &mut trusting(&pki),
        &request(server.port, b"{}"),
        Some(&key()),
    );
    assert_eq!(
        delivery,
        Delivery::TimedOut,
        "the request was sent and may be billed, so it must not be retried"
    );
}

#[test]
fn a_host_or_path_outside_the_rules_or_a_bad_key_sends_nothing() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::ok(b"{}")]);
    let mut transport = trusting(&pki);
    let port = server.port;
    for host in [
        format!("LOCALHOST:{port}"),
        format!("user@localhost:{port}"),
        format!("localhost:{port}/x"),
        format!("localhost:{port}#"),
        format!("localhost :{port}"),
        "localhost:0".to_owned(),
        "localhost:65536".to_owned(),
        format!("localhost:+{port}"),
        String::new(),
    ] {
        let req = HttpRequest {
            host: host.clone(),
            path: PATH.to_owned(),
            body: b"{}".to_vec(),
        };
        assert_eq!(
            send(&mut transport, &req, Some(&key())),
            Delivery::ConnectFailed,
            "{host:?}"
        );
    }
    for path in ["api/v1", "/a#b", "/a\\b", "/a b", "", "/é"] {
        let req = HttpRequest {
            host: format!("localhost:{port}"),
            path: path.to_owned(),
            body: b"{}".to_vec(),
        };
        assert_eq!(
            send(&mut transport, &req, Some(&key())),
            Delivery::ConnectFailed,
            "{path:?}"
        );
    }
    let bad_key = ProviderKey::new("line\nbreak".to_owned());
    assert_eq!(
        send(&mut transport, &request(port, b"{}"), Some(&bad_key)),
        Delivery::ConnectFailed
    );
    sentinel(&pki, server.port);
    assert_eq!(
        server.handshakes(),
        1,
        "none of them was sent; only the sentinel"
    );
}

#[test]
fn it_sends_from_inside_a_tokio_runtime() {
    let pki = pki();
    let server = serve(&pki, vec![Reply::ok(b"{}")]);
    let mut transport = trusting(&pki);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let delivery = runtime
        .block_on(async { send(&mut transport, &request(server.port, b"{}"), Some(&key())) });
    assert_eq!(status_of(&delivery), Some(200));
}

#[test]
fn setup_refuses_no_root_or_a_root_that_is_not_a_certificate() {
    assert_eq!(
        HttpsTransport::new(TlsRoots::Only(vec![])).unwrap_err(),
        SetupError::Root
    );
    assert_eq!(
        HttpsTransport::new(TlsRoots::Only(vec![b"not a certificate".to_vec()])).unwrap_err(),
        SetupError::Root
    );
    let pki = pki();
    assert_eq!(
        HttpsTransport::new(TlsRoots::Only(vec![pki.ca_der.clone(), b"junk".to_vec()]))
            .unwrap_err(),
        SetupError::Root,
        "every root must be a certificate"
    );
    assert_eq!(SetupError::Runtime.code(), "runtime");
    assert_eq!(SetupError::Root.code(), "root");
    assert_eq!(SetupError::Client.code(), "client");
}

#[test]
fn the_key_never_prints() {
    let pki = pki();
    let transport = trusting(&pki);
    let shown = format!("{transport:?}");
    assert!(!shown.contains(KEY), "{shown}");
    assert!(!format!("{:?}", key()).contains(KEY));
}

#[test]
fn an_idle_connection_is_never_reused_by_the_next_attempt() {
    let pki = pki();
    let server = serve(
        &pki,
        vec![
            Reply::KeepAlive {
                body: b"{\"first\":1}".to_vec(),
                linger_ms: 500,
            },
            Reply::ok(b"{\"second\":2}"),
        ],
    );
    let mut transport = trusting(&pki);
    let req = request(server.port, b"{}");
    assert_eq!(
        status_of(&send(&mut transport, &req, Some(&key()))),
        Some(200)
    );
    assert_eq!(
        send(&mut transport, &req, Some(&key())),
        Delivery::Response {
            status: 200,
            body: b"{\"second\":2}".to_vec(),
            retry_after_ms: None,
        },
        "the second attempt opened its own connection"
    );
    assert_eq!(server.handshakes(), 2);
}

const PROXY_CHILD: &str = "MANDATE_GATEWAY_HTTP_PROXY_CHILD";

/// Runs itself again in a child process whose environment names a proxy that refuses every
/// connection, since the workspace forbids the `unsafe` that setting it in-process would need.
#[test]
fn a_proxy_named_in_the_environment_is_not_used() {
    if std::env::var_os(PROXY_CHILD).is_some() {
        let pki = pki();
        let server = serve(&pki, vec![Reply::ok(b"{}")]);
        let delivery = send(
            &mut trusting(&pki),
            &request(server.port, b"{}"),
            Some(&key()),
        );
        assert_eq!(
            status_of(&delivery),
            Some(200),
            "sent directly, not to the proxy"
        );
        assert_eq!(server.seen().len(), 1);
        return;
    }
    let refusing = format!("http://127.0.0.1:{}", closed_port());
    let mut child = std::process::Command::new(std::env::current_exe().unwrap());
    child
        .args([
            "--exact",
            "a_proxy_named_in_the_environment_is_not_used",
            "--nocapture",
        ])
        .env(PROXY_CHILD, "1")
        .env_remove("NO_PROXY")
        .env_remove("no_proxy");
    for name in [
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
        "ALL_PROXY",
        "all_proxy",
    ] {
        child.env(name, &refusing);
    }
    let output = child.output().unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "{stdout}{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        stdout.contains("1 passed"),
        "the child ran the test: {stdout}"
    );
}
