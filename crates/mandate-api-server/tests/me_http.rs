//! One's own session over the workspace API (§4.10; identity spec §4.2, §4.5; DEC-2113): the
//! caller's principal, its session's kind, and when that session ends untouched; nothing
//! secret-shaped, and nothing at all without a live full session.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{ManualClock, MemJournal, Reply, at, hosted, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::session::{Answer, COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Value, parse};
use mandate_identity::PrincipalId;
use mandate_journal::StoredEvent;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const IDLE_S: i64 = 3_600;
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const SUBJECT: &str = "sha256:5ub1ec7";

struct Fixture {
    addr: std::net::SocketAddr,
    sessions: Arc<Sessions>,
    clock: Arc<ManualClock>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture() -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("mandate-api-server-me-{}-{n}", std::process::id()));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x5d_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let clock = ManualClock::at(T0);
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: Arc::clone(&clock) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::new(MemJournal::default()) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(|_: &[StoredEvent]| Ok(Vec::new())),
        hosted: hosted(),
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        sessions,
        clock,
    }
}

fn principal(text: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(text).expect("a ULID")
}

fn open(f: &Fixture, who: &str, secs: i64) -> String {
    f.sessions
        .open(principal(who), SUBJECT.to_owned(), None, at(secs))
        .expect("opened")
        .value()
        .to_owned()
}

fn me(f: &Fixture, method: &str, cookie: Option<&str>) -> Reply {
    let cookie = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let headers: Vec<(&str, &str)> = cookie
        .as_deref()
        .map(|c| vec![("Cookie", c)])
        .unwrap_or_default();
    send(f.addr, method, "/v1/me/session", &headers, b"")
}

fn body(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    match value.get(key) {
        Some(Value::Str(s)) => s,
        other => panic!("{key}: {other:?}"),
    }
}

fn members(value: &Value) -> BTreeSet<String> {
    match value {
        Value::Object(map) => map.keys().map(|k| k.as_str().to_owned()).collect(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn own_session_names_the_caller_and_nothing_secret_shaped() {
    let f = fixture();
    let alice = open(&f, ALICE, T0 - 10);
    let bob = open(&f, BOB, T0 - 20);
    f.clock.set(T0 + 5);
    let reply = me(&f, "GET", Some(&alice));
    assert_eq!(
        reply.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("content-type"), Some("application/json"));
    let answer = body(&reply);
    assert_eq!(
        members(&answer),
        [
            "api_version",
            "as_of",
            "build",
            "expires_at",
            "kind",
            "principal_id",
            "served_at"
        ]
        .into_iter()
        .map(str::to_owned)
        .collect(),
        "no session reference, cookie, token, digest, subject or issuer"
    );
    assert_eq!(text(&answer, "principal_id"), ALICE);
    assert_eq!(text(&answer, "kind"), "full");
    assert_eq!(
        text(&answer, "expires_at"),
        at(T0 + 5 + IDLE_S).to_string(),
        "the request itself is the session's last activity"
    );
    assert_eq!(text(&answer, "served_at"), at(T0 + 5).to_string());
    assert_eq!(answer.get("as_of"), Some(&Value::Array(Vec::new())));
    let raw = String::from_utf8_lossy(&reply.body);
    assert!(!raw.contains(&alice[..32]), "no part of the cookie");
    assert!(!raw.contains(SUBJECT), "no subject link");
    let theirs = body(&me(&f, "GET", Some(&bob)));
    assert_eq!(
        text(&theirs, "principal_id"),
        BOB,
        "each caller is told only itself"
    );
}

#[test]
fn without_a_live_full_session_nothing_is_served() {
    let f = fixture();
    let nowhere = send(f.addr, "GET", "/nowhere", &[], b"");
    assert_eq!(nowhere.status, 401);
    let forged = "0".repeat(160);
    let ended = open(&f, ALICE, T0);
    f.sessions.end(&ended).expect("ended");
    let idle = open(&f, ALICE, T0 - IDLE_S);
    let live = open(&f, ALICE, T0);
    for (label, method, cookie) in [
        ("no cookie", "GET", None),
        ("a forged cookie", "GET", Some(forged.as_str())),
        ("an ended session", "GET", Some(ended.as_str())),
        ("an idle session", "GET", Some(idle.as_str())),
        ("another method", "POST", Some(live.as_str())),
        ("another method", "DELETE", Some(live.as_str())),
    ] {
        let reply = me(&f, method, cookie);
        assert_eq!(reply.status, 401, "{label}");
        assert_eq!(reply.body, nowhere.body, "{label}: the one 401");
    }
    let narrowed = open(&f, BOB, T0);
    assert_eq!(
        f.sessions.refresh(&narrowed, Answer::Unreachable, at(T0)),
        Ok(None)
    );
    let reply = me(&f, "GET", Some(&narrowed));
    assert_eq!(
        reply.status, 403,
        "an outage reaches pause and the kill switch only"
    );
    assert_eq!(text(&body(&reply), "code"), "reduction_only");
    assert_eq!(
        me(&f, "GET", Some(&live)).status,
        200,
        "the live one still is"
    );
}
