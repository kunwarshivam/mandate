//! The session routes (§3.11 items 5 and 6, §4.10, API-1 as amended, DEC-978 items 5 to 7).

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use common::{ManualClock, Reply, TestIssuer, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, PROBLEM_JSON, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::subject::digest;
use mandate_api_server::token::{Issuer, Keys};
use mandate_authn::{Algorithm, IssuerConfig};

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const ALICE: &str = "alice-subject";
const ALICE_ID: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Y";

/// A key set the test can withdraw, as an issuer outage would.
struct SwitchKeys(Mutex<Option<Arc<mandate_authn::Jwks>>>);

impl Keys for SwitchKeys {
    fn current(&self) -> Option<Arc<mandate_authn::Jwks>> {
        self.0.lock().expect("unpoisoned").clone()
    }
}

struct Fixture {
    addr: std::net::SocketAddr,
    keys: Arc<SwitchKeys>,
    issuer: TestIssuer,
    clock: Arc<ManualClock>,
    links: PathBuf,
    sessions: Arc<Sessions>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn dir() -> PathBuf {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-sessions-http-{}-{n}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    dir
}

fn fixture_with(keys: bool) -> Fixture {
    let issuer = TestIssuer::new("es256-1");
    let dir = dir();
    let key_file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&key_file, [0x33_u8; 32]).expect("the key");
    std::fs::set_permissions(&key_file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let links = dir.join("subject-links");
    std::fs::write(
        &links,
        format!("{} {ALICE_ID}\n", digest(&issuer.issuer, ALICE)),
    )
    .expect("the links");
    std::fs::set_permissions(&links, std::fs::Permissions::from_mode(0o644)).expect("mode");
    let config = IssuerConfig::new(&issuer.issuer, &["authenticated"], &[Algorithm::Es256])
        .expect("the issuer");
    let switch = Arc::new(SwitchKeys(Mutex::new(
        keys.then(|| Arc::new(issuer.jwks())),
    )));
    let clock = ManualClock::at(T0);
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::clone(&switch) as Arc<dyn Keys>)),
        links: Arc::new(common::LinksAt(links.clone())),
        clock: Arc::clone(&clock) as Arc<dyn mandate_api_server::clock::Clock>,
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
        folds: Arc::default(),
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        keys: switch,
        issuer,
        clock,
        links,
        sessions,
    }
}

fn fixture() -> Fixture {
    fixture_with(true)
}

const BROWSER: [(&str, &str); 3] = [
    ("Origin", APP),
    ("X-Mandate-Request", "1"),
    ("Content-Type", "application/json"),
];

fn browser_with(cookie: Option<&str>) -> Vec<(&'static str, String)> {
    let mut headers: Vec<(&'static str, String)> =
        BROWSER.iter().map(|(n, v)| (*n, (*v).to_owned())).collect();
    if let Some(cookie) = cookie {
        headers.push(("Cookie", format!("{COOKIE_NAME}={cookie}")));
    }
    headers
}

fn call(f: &Fixture, method: &str, path: &str, headers: &[(&str, String)], body: &str) -> Reply {
    let headers: Vec<(&str, &str)> = headers.iter().map(|(n, v)| (*n, v.as_str())).collect();
    send(f.addr, method, path, &headers, body.as_bytes())
}

fn open_body(token: &str) -> String {
    format!(r#"{{"access_token":"{token}"}}"#)
}

/// The cookie value a reply sets, and the rest of its `Set-Cookie`.
fn set_cookie(reply: &Reply) -> (String, String) {
    let header = reply.header("set-cookie").expect("a Set-Cookie");
    let (pair, attributes) = header.split_once(';').expect("attributes");
    let value = pair
        .strip_prefix(&format!("{COOKIE_NAME}="))
        .expect("the session cookie");
    (value.to_owned(), attributes.trim().to_owned())
}

fn unauthenticated(f: &Fixture) -> Vec<u8> {
    send(f.addr, "GET", "/nowhere", &[], b"").body
}

fn open(f: &Fixture) -> String {
    let token = f.issuer.token(ALICE, f.clock.secs() + 3_600);
    let reply = call(
        f,
        "POST",
        "/v1/sessions",
        &browser_with(None),
        &open_body(&token),
    );
    assert_eq!(
        reply.status,
        204,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    set_cookie(&reply).0
}

#[test]
fn a_verified_linked_token_opens_a_session_with_the_host_cookie() {
    let f = fixture();
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions",
        &browser_with(None),
        &open_body(&token),
    );
    assert_eq!(reply.status, 204);
    assert!(reply.body.is_empty());
    let (value, attributes) = set_cookie(&reply);
    assert_eq!(value.len(), 160);
    assert_eq!(
        attributes,
        "Path=/; Max-Age=43200; Secure; HttpOnly; SameSite=Strict"
    );
    assert_eq!(reply.header("access-control-allow-origin"), Some(APP));
    assert_eq!(
        reply.header("access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(f.sessions.len(), 1);
    assert_eq!(
        f.sessions.subject(&value),
        Ok(digest(&f.issuer.issuer, ALICE))
    );
}

#[test]
fn the_csrf_check_comes_first_and_answers_403() {
    let f = fixture();
    let token = f.issuer.token(ALICE, T0 + 3_600);
    for path in ["/v1/sessions", "/v1/sessions/refresh"] {
        for headers in [
            vec![("X-Mandate-Request", "1".to_owned())],
            vec![
                ("Origin", "https://evil.test".to_owned()),
                ("X-Mandate-Request", "1".to_owned()),
            ],
            vec![("Origin", APP.to_owned())],
            vec![
                ("Origin", APP.to_owned()),
                ("X-Mandate-Request", "0".to_owned()),
            ],
            vec![
                ("Origin", APP.to_owned()),
                ("Origin", APP.to_owned()),
                ("X-Mandate-Request", "1".to_owned()),
            ],
        ] {
            let reply = call(&f, "POST", path, &headers, &open_body(&token));
            assert_eq!(reply.status, 403, "{path} {headers:?}");
            assert_eq!(reply.header("content-type"), Some(PROBLEM_JSON));
            let body = String::from_utf8_lossy(&reply.body).into_owned();
            assert!(body.contains(r#""code":"forbidden""#), "{body}");
            assert!(body.contains(r#""effect":"none""#), "{body}");
        }
    }
    let reply = call(
        &f,
        "DELETE",
        "/v1/sessions/current",
        &[("Origin", APP.to_owned())],
        "",
    );
    assert_eq!(reply.status, 403);
    assert!(f.sessions.is_empty());
}

#[test]
fn every_other_refusal_to_open_is_the_one_401() {
    let f = fixture();
    let one = unauthenticated(&f);
    let other_issuer = TestIssuer::new("es256-1");
    for body in [
        String::new(),
        "not json".to_owned(),
        "{}".to_owned(),
        r#"{"access_token":null}"#.to_owned(),
        r#"{"access_token":7}"#.to_owned(),
        format!(
            r#"{{"access_token":"{}","extra":1}}"#,
            f.issuer.token(ALICE, T0 + 3_600)
        ),
        open_body("a.b.c"),
        open_body(&other_issuer.token(ALICE, T0 + 3_600)),
        open_body(&f.issuer.token(ALICE, T0 - 3_600)),
        open_body(&f.issuer.token("someone-unlinked", T0 + 3_600)),
        open_body(
            &f.issuer
                .token_from("https://evil.test/auth/v1", ALICE, T0 + 3_600),
        ),
        format!(r#"{{"token":"{}"}}"#, f.issuer.token(ALICE, T0 + 3_600)),
    ] {
        let reply = call(&f, "POST", "/v1/sessions", &browser_with(None), &body);
        assert_eq!(reply.status, 401, "{body}");
        assert_eq!(reply.body, one, "{body}");
        assert!(reply.header("set-cookie").is_none());
    }
    assert!(f.sessions.is_empty());
}

#[test]
fn with_no_key_set_or_an_exposed_link_file_nothing_opens() {
    let f = fixture_with(false);
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions",
        &browser_with(None),
        &open_body(&token),
    );
    assert_eq!(reply.status, 401);
    let f = fixture();
    std::fs::set_permissions(&f.links, std::fs::Permissions::from_mode(0o666)).expect("mode");
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions",
        &browser_with(None),
        &open_body(&token),
    );
    assert_eq!(reply.status, 401);
    assert!(f.sessions.is_empty());
}

#[test]
fn opening_with_a_cookie_ends_the_earlier_session() {
    let f = fixture();
    let first = open(&f);
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions",
        &browser_with(Some(&first)),
        &open_body(&token),
    );
    assert_eq!(reply.status, 204);
    let (second, _) = set_cookie(&reply);
    assert_ne!(second, first);
    assert_eq!(f.sessions.len(), 1);
    assert!(f.sessions.subject(&first).is_err());
}

#[test]
fn a_refresh_with_a_fresh_token_rotates_the_cookie() {
    let f = fixture();
    let cookie = open(&f);
    f.clock.set(T0 + 240);
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&token),
    );
    assert_eq!(reply.status, 204);
    let (rotated, attributes) = set_cookie(&reply);
    assert_ne!(rotated, cookie);
    assert_eq!(
        attributes,
        "Path=/; Max-Age=43200; Secure; HttpOnly; SameSite=Strict"
    );
    assert!(f.sessions.subject(&rotated).is_ok());
}

#[test]
fn a_null_refresh_is_an_outage_and_sets_no_cookie() {
    let f = fixture();
    let cookie = open(&f);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        r#"{"access_token":null}"#,
    );
    assert_eq!(reply.status, 204);
    assert!(reply.header("set-cookie").is_none());
    assert_eq!(f.sessions.len(), 1);
}

#[test]
fn a_refresh_whose_keys_cannot_be_had_is_an_outage_and_an_unlinked_subject_fails() {
    let f = fixture();
    let cookie = open(&f);
    *f.keys.0.lock().expect("unpoisoned") = None;
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&token),
    );
    assert_eq!(reply.status, 204, "an outage keeps the session");
    assert!(reply.header("set-cookie").is_none());
    assert_eq!(f.sessions.len(), 1);
    *f.keys.0.lock().expect("unpoisoned") = Some(Arc::new(f.issuer.jwks()));
    std::fs::write(&f.links, "").expect("the link is removed");
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&token),
    );
    assert_eq!(
        reply.status, 401,
        "a subject no longer linked fails the refresh"
    );
    assert!(f.sessions.is_empty());
}

#[test]
fn a_refresh_with_a_bad_token_or_another_subject_ends_the_session() {
    for subject in ["bad", "other"] {
        let f = fixture();
        let cookie = open(&f);
        let mut links = std::fs::read_to_string(&f.links).expect("links");
        links.push_str(&format!(
            "{} 01JB3K6F0C9R2V7N4M8Q1T5W40\n",
            digest(&f.issuer.issuer, "bob")
        ));
        std::fs::write(&f.links, links).expect("links");
        let token = if subject == "bad" {
            TestIssuer::new("es256-1").token(ALICE, T0 + 3_600)
        } else {
            f.issuer.token("bob", T0 + 3_600)
        };
        let reply = call(
            &f,
            "POST",
            "/v1/sessions/refresh",
            &browser_with(Some(&cookie)),
            &open_body(&token),
        );
        assert_eq!(reply.status, 401, "{subject}");
        assert_eq!(reply.body, unauthenticated(&f));
        assert!(f.sessions.is_empty(), "{subject}");
    }
}

#[test]
fn a_refresh_or_sign_out_without_a_live_session_is_the_one_401() {
    let f = fixture();
    let one = unauthenticated(&f);
    let token = f.issuer.token(ALICE, T0 + 3_600);
    for cookie in [None, Some("deadbeef"), Some(&"0".repeat(160)[..])] {
        let reply = call(
            &f,
            "POST",
            "/v1/sessions/refresh",
            &browser_with(cookie),
            &open_body(&token),
        );
        assert_eq!(reply.status, 401, "{cookie:?}");
        assert_eq!(reply.body, one);
        let reply = call(
            &f,
            "DELETE",
            "/v1/sessions/current",
            &browser_with(cookie),
            "",
        );
        assert_eq!(reply.status, 401, "{cookie:?}");
        assert_eq!(reply.body, one);
    }
}

#[test]
fn sign_out_ends_the_session_and_clears_the_cookie() {
    let f = fixture();
    let cookie = open(&f);
    let reply = call(
        &f,
        "DELETE",
        "/v1/sessions/current",
        &browser_with(Some(&cookie)),
        "",
    );
    assert_eq!(reply.status, 204);
    let (value, attributes) = set_cookie(&reply);
    assert_eq!(value, "");
    assert_eq!(
        attributes,
        "Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict"
    );
    assert!(f.sessions.is_empty());
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&token),
    );
    assert_eq!(reply.status, 401);
}

#[test]
fn a_session_cookie_among_others_is_found_and_two_of_them_are_refused() {
    let f = fixture();
    let cookie = open(&f);
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let mut headers = browser_with(None);
    headers.push((
        "Cookie",
        format!("theme=dark; {COOKIE_NAME}={cookie}; other=1"),
    ));
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &headers,
        &open_body(&token),
    );
    assert_eq!(reply.status, 204);
    let (rotated, _) = set_cookie(&reply);
    let mut headers = browser_with(None);
    headers.push((
        "Cookie",
        format!("{COOKIE_NAME}={rotated}; {COOKIE_NAME}={rotated}"),
    ));
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &headers,
        &open_body(&token),
    );
    assert_eq!(reply.status, 401);
}

#[test]
fn a_remapped_subject_link_fails_the_refresh() {
    let f = fixture();
    let cookie = open(&f);
    std::fs::write(
        &f.links,
        format!(
            "{} 01JB3K6F0C9R2V7N4M8Q1T5W40\n",
            digest(&f.issuer.issuer, ALICE)
        ),
    )
    .expect("the line now names another principal");
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&token),
    );
    assert_eq!(reply.status, 401);
    assert!(f.sessions.is_empty());
}

#[test]
fn an_unknown_key_or_an_unreadable_link_file_is_an_outage_not_a_failure() {
    let f = fixture();
    let cookie = open(&f);
    let rotated_key = TestIssuer::new("es256-2").token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&rotated_key),
    );
    assert_eq!(reply.status, 204, "a kid the last fetch did not bring");
    assert!(reply.header("set-cookie").is_none());
    std::fs::set_permissions(&f.links, std::fs::Permissions::from_mode(0o666)).expect("mode");
    let token = f.issuer.token(ALICE, T0 + 3_600);
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&token),
    );
    assert_eq!(reply.status, 204, "an exposed link file");
    std::fs::remove_file(&f.links).expect("removed");
    let reply = call(
        &f,
        "POST",
        "/v1/sessions/refresh",
        &browser_with(Some(&cookie)),
        &open_body(&token),
    );
    assert_eq!(reply.status, 204, "an unreadable link file");
    assert_eq!(f.sessions.len(), 1);
}

#[test]
fn signing_out_a_dead_cookie_still_clears_it() {
    let f = fixture();
    let cookie = open(&f);
    assert_eq!(
        call(
            &f,
            "DELETE",
            "/v1/sessions/current",
            &browser_with(Some(&cookie)),
            ""
        )
        .status,
        204
    );
    let reply = call(
        &f,
        "DELETE",
        "/v1/sessions/current",
        &browser_with(Some(&cookie)),
        "",
    );
    assert_eq!(reply.status, 401);
    assert_eq!(reply.body, unauthenticated(&f));
    let (value, attributes) = set_cookie(&reply);
    assert_eq!(value, "");
    assert_eq!(
        attributes,
        "Path=/; Max-Age=0; Secure; HttpOnly; SameSite=Strict"
    );
    let reply = call(
        &f,
        "DELETE",
        "/v1/sessions/current",
        &browser_with(None),
        "",
    );
    assert_eq!(reply.status, 401);
    assert!(
        reply.header("set-cookie").is_none(),
        "no cookie, nothing to clear"
    );
}
