//! Route 2's challenge route (workspace API spec §4.10 route 2, identity spec §6.4 route 2,
//! DEC-2117, DEC-2119, DEC-653): browser-only and strict, a challenge that names nothing
//! workspace-specific and is never journaled, a device cookie that is random bytes and nothing
//! else, and the one 401 over every limit, with no address claimed from a peer that is not the
//! ingress.

mod common;

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{ManualClock, MemJournal, Reply, WORKSPACE, at, hosted, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::reduction::{self, DEVICE_COOKIE, Reduction};
use mandate_api_server::session::{OsRandom, Random, Sessions, Unauthenticated};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Value, parse};

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const DEVICE: &str = "q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA";

/// A generator that hands out 0, 1, 2, ... in every byte of each draw, so no two draws repeat.
#[derive(Default)]
struct Counting(AtomicU8);

impl Random for Counting {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        out.iter_mut().for_each(|b| *b = n);
        Ok(())
    }
}

/// A generator that draws distinct 32-byte values for as long as a test needs, distinct in their
/// first eight bytes too.
#[derive(Default)]
struct Wide(Mutex<u64>);

impl Random for Wide {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let mut n = self.0.lock().expect("unpoisoned");
        *n += 1;
        out.iter_mut().for_each(|b| *b = 0xa5);
        out[..8].copy_from_slice(&n.to_be_bytes());
        Ok(())
    }
}

fn ip(text: &str) -> IpAddr {
    text.parse().expect("an address")
}

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture(reduction: Reduction) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-reduction-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x7a_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
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
        sessions: Arc::new(Sessions::new(&key, Box::new(OsRandom::new()))),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: ManualClock::at(T0) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(|_: &[mandate_journal::StoredEvent]| Ok(Vec::new())),
        hosted: hosted(),
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::new(reduction),
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
    }
}

const PATH: &str = "/v1/reduction-sessions/challenges";

fn ask(f: &Fixture, extra: &[(&str, &str)], body: &[u8]) -> Reply {
    let mut all = vec![("Origin", APP), ("X-Mandate-Request", "1")];
    all.extend_from_slice(extra);
    send(f.addr, "POST", PATH, &all, body)
}

fn json(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    match value.get(key) {
        Some(Value::Str(s)) => s,
        other => panic!("{key}: {other:?}"),
    }
}

/// The reply's headers by name, without the ones every reply varies in.
fn shape(reply: &Reply) -> BTreeSet<String> {
    reply
        .headers
        .iter()
        .map(|(name, _)| name.to_ascii_lowercase())
        .filter(|name| name != "date")
        .collect()
}

fn the_one_401(f: &Fixture, reply: &Reply, label: &str) {
    let nowhere = send(f.addr, "GET", "/nowhere", &[("Origin", APP)], b"");
    assert_eq!(reply.status, 401, "{label}");
    assert_eq!(reply.body, nowhere.body, "{label}: the one body");
    assert_eq!(shape(reply), shape(&nowhere), "{label}: the same headers");
    assert!(reply.all("set-cookie").is_empty(), "{label}: no cookie");
}

#[test]
#[ignore = "pending E9-1"]
fn a_challenge_names_nothing_but_itself_and_journals_nothing() {
    let f = fixture(Reduction::new(Vec::new(), Box::new(Counting::default())));
    let reply = ask(&f, &[], b"{}");
    assert_eq!(
        reply.status,
        201,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("content-type"), Some("application/json"));
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    let answer = json(&reply);
    let members: BTreeSet<&str> = match &answer {
        Value::Object(map) => map.keys().map(|k| k.as_str()).collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        members,
        BTreeSet::from([
            "api_version",
            "as_of",
            "build",
            "challenge_id",
            "expires_at",
            "issued_at",
            "rp_id",
            "served_at",
            "webauthn_challenge",
        ])
    );
    assert_eq!(answer.get("as_of"), Some(&Value::Array(Vec::new())));
    assert_eq!(text(&answer, "rp_id"), "app.owlhead.test");
    assert_eq!(text(&answer, "issued_at"), at(T0).to_string());
    assert_eq!(text(&answer, "served_at"), at(T0).to_string());
    assert_eq!(text(&answer, "expires_at"), at(T0 + 300).to_string());
    let bytes = URL_SAFE_NO_PAD
        .decode(text(&answer, "webauthn_challenge"))
        .expect("base64url");
    let drawn: [u8; 32] = bytes.as_slice().try_into().expect("32 bytes");
    assert_eq!(
        text(&answer, "challenge_id"),
        reduction::challenge_id(at(T0), &drawn)
    );
    let raw = String::from_utf8_lossy(&reply.body);
    assert!(!raw.contains(WORKSPACE), "no workspace is named");
    assert!(
        f.journal.appends.lock().expect("unpoisoned").is_empty(),
        "nothing is journaled"
    );
    let cookie = reply.header("set-cookie").expect("a device cookie");
    let (value, attributes) = cookie
        .strip_prefix(&format!("{DEVICE_COOKIE}="))
        .and_then(|rest| rest.split_once("; "))
        .expect("the device cookie");
    assert_eq!(
        attributes,
        "Path=/; Max-Age=2592000; Secure; HttpOnly; SameSite=Strict"
    );
    assert_eq!(
        URL_SAFE_NO_PAD.decode(value).expect("base64url").len(),
        32,
        "random bytes and nothing else"
    );
    assert_ne!(value, text(&answer, "webauthn_challenge"));
}

#[test]
#[ignore = "pending E9-1"]
fn a_device_cookie_presented_is_kept_and_a_malformed_one_replaced() {
    let f = fixture(Reduction::new(Vec::new(), Box::new(Counting::default())));
    let kept = ask(
        &f,
        &[("Cookie", &format!("{DEVICE_COOKIE}={DEVICE}"))],
        b"{}",
    );
    assert_eq!(kept.status, 201);
    assert!(
        kept.all("set-cookie").is_empty(),
        "the device keeps its key"
    );
    let replaced = ask(&f, &[("Cookie", &format!("{DEVICE_COOKIE}=short"))], b"{}");
    assert_eq!(replaced.status, 201);
    assert_eq!(replaced.all("set-cookie").len(), 1);
}

#[test]
#[ignore = "pending E9-1"]
fn the_challenge_route_is_browser_only_and_takes_an_empty_object() {
    let f = fixture(Reduction::default());
    let forbidden = |reply: Reply, label: &str| {
        assert_eq!(reply.status, 403, "{label}");
        assert_eq!(
            json(&reply).get("code"),
            Some(&Value::Str("forbidden".to_owned())),
            "{label}"
        );
    };
    forbidden(
        send(f.addr, "POST", PATH, &[("X-Mandate-Request", "1")], b"{}"),
        "no origin",
    );
    forbidden(
        send(
            f.addr,
            "POST",
            PATH,
            &[("Origin", "https://evil.test"), ("X-Mandate-Request", "1")],
            b"{}",
        ),
        "another origin",
    );
    forbidden(
        send(f.addr, "POST", PATH, &[("Origin", APP)], b"{}"),
        "no request mark",
    );
    for body in [&b"{\"workspace_id\":\"x\"}"[..], b"[]", b"", b"not json"] {
        let reply = ask(&f, &[], body);
        assert_eq!(reply.status, 422, "{:?}", String::from_utf8_lossy(body));
        assert_eq!(
            json(&reply).get("code"),
            Some(&Value::Str("invalid".to_owned()))
        );
    }
    let other = send(f.addr, "GET", PATH, &[("Origin", APP)], b"");
    let nowhere = send(f.addr, "GET", "/nowhere", &[("Origin", APP)], b"");
    assert_eq!(
        (other.status, &other.body),
        (401, &nowhere.body),
        "another method: the one 401"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn over_its_limits_the_challenge_route_answers_the_one_401() {
    let f = fixture(Reduction::new(Vec::new(), Box::new(Wide::default())));
    let device = [("Cookie", format!("{DEVICE_COOKIE}={DEVICE}"))];
    let device: Vec<(&str, &str)> = device.iter().map(|(n, v)| (*n, v.as_str())).collect();
    for _ in 0..4 {
        assert_eq!(ask(&f, &device, b"{}").status, 201);
    }
    the_one_401(&f, &ask(&f, &device, b"{}"), "a fifth on one device");
    for n in 4..32 {
        let claimed = format!("203.0.113.{n}");
        assert_eq!(
            ask(&f, &[("X-Forwarded-For", &claimed)], b"{}").status,
            201,
            "a fresh device each"
        );
    }
    the_one_401(
        &f,
        &ask(&f, &[("X-Forwarded-For", "198.51.100.7")], b"{}"),
        "a 33rd on one address: a forwarded address from no listed peer claims nothing",
    );
    let elsewhere = fixture(Reduction::new(
        vec![ip("10.0.0.1")],
        Box::new(Wide::default()),
    ));
    for n in 0..32 {
        let claimed = format!("203.0.113.{n}");
        assert_eq!(
            ask(&elsewhere, &[("X-Forwarded-For", &claimed)], b"{}").status,
            201
        );
    }
    the_one_401(
        &elsewhere,
        &ask(&elsewhere, &[("X-Forwarded-For", "198.51.100.7")], b"{}"),
        "an ingress listed elsewhere: the loopback peer is no ingress",
    );

    let behind = fixture(Reduction::new(
        vec![ip("127.0.0.1")],
        Box::new(Wide::default()),
    ));
    for n in 0..33 {
        let forwarded = format!("203.0.113.{n}");
        let reply = ask(&behind, &[("X-Forwarded-For", &forwarded)], b"{}");
        assert_eq!(
            reply.status, 201,
            "each client behind the ingress has its own budget"
        );
    }
}
