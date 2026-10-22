//! The two routers (§3.11 items 3 and 4, §3.3 item 5, API-1).

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::{get, send, serve};
use mandate_api_server::http::{
    self, ALLOW_HEADERS, ALLOW_METHODS, App, EXPOSE_HEADERS, PROBLEM_JSON, Settings,
};
use mandate_api_server::lifecycle::{Drain, Lifecycle, Local};
use mandate_api_server::session::{OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};

const APP: &str = "https://app.owlhead.test";

fn api() -> std::net::SocketAddr {
    serve(http::api(app(), Drain::new()))
}

/// An app with no key set, no links, and a fixed clock: nothing can authenticate.
fn app() -> App {
    use std::os::unix::fs::PermissionsExt;
    static APPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-http-{}-{}",
        std::process::id(),
        APPS.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(mandate_api_server::config::SESSION_KEY_FILE);
    std::fs::write(&file, [9_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = mandate_api_server::config::read_session_key(&dir).expect("the key");
    let config = mandate_authn::IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[mandate_authn::Algorithm::Es256],
    )
    .expect("the issuer");
    App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::new(Sessions::new(&key, Box::new(OsRandom::new()))),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: common::ManualClock::at(1_791_000_000),
        journal: Arc::new(common::MemJournal::default()),
        members: Arc::new(mandate_api_server::members::control_parser(
            common::hosted().workspace,
        )),
        hosted: common::hosted(),
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
    }
}

struct Switch(AtomicBool);

impl Lifecycle for Switch {
    fn ready(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

#[test]
fn livez_answers_live() {
    let probes = serve(http::probes(Arc::new(Switch(AtomicBool::new(false)))));
    let reply = get(probes, "/livez");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, br#"{"status":"live"}"#);
    assert_eq!(reply.header("content-type"), Some("application/json"));
}

#[test]
fn readyz_follows_the_lifecycle() {
    let switch = Arc::new(Switch(AtomicBool::new(true)));
    let probes = serve(http::probes(Arc::clone(&switch) as Arc<dyn Lifecycle>));
    let reply = get(probes, "/readyz");
    assert_eq!(reply.status, 200);
    assert_eq!(reply.body, br#"{"status":"ready"}"#);
    switch.0.store(false, Ordering::SeqCst);
    let reply = get(probes, "/readyz");
    assert_eq!(reply.status, 503);
    assert_eq!(reply.body, br#"{"status":"not_ready"}"#);
    assert_eq!(reply.header("content-type"), Some("application/json"));
    assert_eq!(get(probes, "/livez").status, 200, "live while not ready");
}

#[test]
fn the_local_lifecycle_is_ready_until_the_drain_begins() {
    let drain = Drain::new();
    let local = Local::new(Arc::clone(&drain));
    assert!(local.ready());
    assert!(!drain.draining());
    drain.begin();
    assert!(drain.draining());
    assert!(!local.ready());
    drain.begin();
    assert!(drain.draining(), "a drain never ends");
    let probes = serve(http::probes(Arc::new(local)));
    assert_eq!(get(probes, "/readyz").status, 503);
}

#[test]
fn the_probes_listener_serves_nothing_else() {
    let probes = serve(http::probes(Arc::new(Switch(AtomicBool::new(true)))));
    for (method, path) in [
        ("GET", "/"),
        ("GET", "/v1/workspaces/ws_1/agents"),
        ("POST", "/livez"),
        ("DELETE", "/readyz"),
        ("GET", "/livez/x"),
        ("GET", "/metrics"),
        ("HEAD", "/livez"),
        ("HEAD", "/readyz"),
        ("OPTIONS", "/livez"),
    ] {
        let reply = send(probes, method, path, &[], b"");
        assert_eq!(reply.status, 404, "{method} {path}");
        assert!(reply.body.is_empty(), "{method} {path}");
    }
}

/// Every method and path a route of the API might take, none landed in this slice.
const CALLS: [(&str, &str); 12] = [
    ("GET", "/"),
    ("GET", "/livez"),
    ("GET", "/readyz"),
    ("GET", "/v1/workspaces/ws_1/agents"),
    ("GET", "/v1/workspaces/ws_1/agents/agt_1"),
    ("GET", "/v1/workspaces/ws_1/approvals?state=pending"),
    ("POST", "/v1/workspaces/ws_1/kill-switch"),
    ("POST", "/v1/workspaces/ws_1/agents/agt_1/pause"),
    ("PUT", "/v1/workspaces/ws_1/policies/workspace"),
    ("DELETE", "/v1/workspaces/ws_1/clients/c_1"),
    ("PATCH", "/v1/workspaces/ws_1/members/m_1"),
    ("GET", "/v1/me/workspaces"),
];

#[test]
fn every_method_and_path_answers_the_one_401() {
    let addr = api();
    let first = send(addr, "GET", "/nowhere", &[], b"");
    assert_eq!(first.status, 401);
    assert_eq!(first.header("content-type"), Some(PROBLEM_JSON));
    let body = std::str::from_utf8(&first.body).expect("JSON text");
    for member in [
        r#""type":"https://mandate.dev/problems/unauthenticated""#,
        r#""status":401"#,
        r#""code":"unauthenticated""#,
        r#""title":"Sign in required""#,
        r#""effect":"none""#,
        r#""event_id":null"#,
        r#""retryable":false"#,
        r#""violations":[]"#,
    ] {
        assert!(body.contains(member), "{member} in {body}");
    }
    for (method, path) in CALLS {
        let reply = send(addr, method, path, &[], b"{}");
        assert_eq!(reply.status, 401, "{method} {path}");
        assert_eq!(reply.body, first.body, "{method} {path}");
        assert_eq!(reply.header("content-type"), Some(PROBLEM_JSON));
        assert!(!reply.has_cors(), "{method} {path}: no origin, no CORS");
    }
}

#[test]
fn a_cookie_or_bearer_the_server_never_issued_is_the_same_401() {
    let addr = api();
    let plain = get(addr, "/v1/workspaces/ws_1/agents");
    for header in [
        ("Cookie", "__Host-mandate_session=forged"),
        ("Authorization", "Bearer eyJhbGciOiJub25lIn0.e30."),
        ("Authorization", "DPoP abc"),
    ] {
        let reply = send(addr, "GET", "/v1/workspaces/ws_1/agents", &[header], b"");
        assert_eq!(reply.status, 401, "{header:?}");
        assert_eq!(reply.body, plain.body, "{header:?}");
    }
}

fn preflight(origin: &str) -> common::Reply {
    send(
        api(),
        "OPTIONS",
        "/v1/workspaces/ws_1/kill-switch",
        &[
            ("Origin", origin),
            ("Access-Control-Request-Method", "POST"),
            (
                "Access-Control-Request-Headers",
                "content-type, idempotency-key, x-mandate-request",
            ),
        ],
        b"",
    )
}

#[test]
fn a_preflight_from_the_app_origin_is_allowed_and_reads_nothing() {
    let reply = preflight(APP);
    assert_eq!(reply.status, 204);
    assert!(reply.body.is_empty());
    assert_eq!(reply.header("access-control-allow-origin"), Some(APP));
    assert_eq!(
        reply.header("access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(
        reply.header("access-control-allow-methods"),
        Some(ALLOW_METHODS)
    );
    assert_eq!(
        reply.header("access-control-allow-headers"),
        Some(ALLOW_HEADERS)
    );
    assert_eq!(reply.header("access-control-max-age"), Some("600"));
    assert_eq!(reply.header("vary"), Some("Origin"));
}

#[test]
fn a_preflight_from_any_other_origin_gets_no_cors_header() {
    for origin in [
        "https://evil.test",
        "https://app.owlhead.test.evil.test",
        "http://app.owlhead.test",
        "https://APP.owlhead.test",
        "https://app.owlhead.test/",
        "null",
        "*",
    ] {
        let reply = preflight(origin);
        assert_eq!(reply.status, 204, "{origin}");
        assert!(reply.body.is_empty(), "{origin}");
        assert!(!reply.has_cors(), "{origin}: {:?}", reply.headers);
    }
}

#[test]
fn an_options_call_that_is_not_a_preflight_is_the_one_401() {
    let addr = api();
    let plain = get(addr, "/x");
    for headers in [
        vec![],
        vec![("Origin", APP)],
        vec![("Access-Control-Request-Method", "POST")],
    ] {
        let reply = send(addr, "OPTIONS", "/v1/workspaces/ws_1/agents", &headers, b"");
        assert_eq!(reply.status, 401, "{headers:?}");
        assert_eq!(reply.body, plain.body);
    }
}

#[test]
fn a_401_to_the_app_origin_carries_its_cors_headers() {
    let reply = send(
        api(),
        "POST",
        "/v1/workspaces/ws_1/kill-switch",
        &[("Origin", APP)],
        b"{}",
    );
    assert_eq!(reply.status, 401);
    assert_eq!(reply.header("access-control-allow-origin"), Some(APP));
    assert_eq!(
        reply.header("access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(
        reply.header("access-control-expose-headers"),
        Some(EXPOSE_HEADERS)
    );
    assert_eq!(reply.header("vary"), Some("Origin"));
    assert_eq!(reply.header("access-control-allow-methods"), None);
}

#[test]
fn a_401_to_another_origin_carries_none() {
    let reply = send(
        api(),
        "GET",
        "/v1/workspaces/ws_1/agents",
        &[("Origin", "https://evil.test")],
        b"",
    );
    assert_eq!(reply.status, 401);
    assert!(!reply.has_cors(), "{:?}", reply.headers);
    assert_eq!(reply.header("vary"), Some("Origin"));
}

#[test]
fn a_request_is_counted_in_flight_until_its_response_is_ready() {
    let drain = Drain::new();
    let (entered, wait_entered) = std::sync::mpsc::channel::<()>();
    let (release, released) = tokio::sync::oneshot::channel::<()>();
    let released = Arc::new(std::sync::Mutex::new(Some(released)));
    let slow = axum::Router::new().route(
        "/slow",
        axum::routing::get(move || {
            let entered = entered.clone();
            let released = released.lock().expect("unpoisoned").take();
            async move {
                let _ = entered.send(());
                if let Some(released) = released {
                    let _ = released.await;
                }
                "done"
            }
        }),
    );
    let addr = serve(http::counted(slow, Arc::clone(&drain)));
    assert_eq!(drain.in_flight(), 0);
    let client = std::thread::spawn(move || get(addr, "/slow"));
    wait_entered
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("the handler ran");
    assert_eq!(drain.in_flight(), 1, "counted while the handler runs");
    let _ = release.send(());
    let reply = client.join().expect("the client");
    assert_eq!(reply.body, b"done");
    assert_eq!(drain.in_flight(), 0, "uncounted once answered");
}

#[test]
fn a_drain_settles_when_nothing_is_in_flight_and_not_before() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let drain = Drain::new();
    assert!(runtime.block_on(drain.settled(std::time::Duration::ZERO)));
    let held = drain.enter();
    let second = drain.enter();
    assert_eq!(drain.in_flight(), 2);
    assert!(!runtime.block_on(drain.settled(std::time::Duration::from_millis(50))));
    drop(second);
    assert!(!runtime.block_on(drain.settled(std::time::Duration::from_millis(30))));
    let releaser = {
        let held = std::sync::Mutex::new(Some(held));
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            drop(held.lock().expect("unpoisoned").take());
        })
    };
    assert!(runtime.block_on(drain.settled(std::time::Duration::from_secs(10))));
    releaser.join().expect("the releaser");
    assert_eq!(drain.in_flight(), 0);
}

#[test]
fn two_origins_or_an_empty_one_get_no_cors_header() {
    let addr = api();
    for origins in [
        vec![("Origin", APP), ("Origin", "https://evil.test")],
        vec![("Origin", "https://evil.test"), ("Origin", APP)],
        vec![("Origin", APP), ("Origin", APP)],
        vec![("Origin", "")],
    ] {
        let reply = send(addr, "GET", "/v1/workspaces/ws_1/agents", &origins, b"");
        assert_eq!(reply.status, 401, "{origins:?}");
        assert!(!reply.has_cors(), "{origins:?}: {:?}", reply.headers);
        let mut preflight = origins.clone();
        preflight.push(("Access-Control-Request-Method", "POST"));
        let reply = send(
            addr,
            "OPTIONS",
            "/v1/workspaces/ws_1/kill-switch",
            &preflight,
            b"",
        );
        assert_eq!(reply.status, 204, "{origins:?}");
        assert!(!reply.has_cors(), "{origins:?}: {:?}", reply.headers);
    }
}
