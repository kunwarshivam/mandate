//! Route 2's challenge route (workspace API spec §4.10 route 2, identity spec §6.4 route 2,
//! DEC-2117, DEC-2119, DEC-653): browser-only and strict, a challenge that names nothing
//! workspace-specific and is never journaled, a device cookie that is random bytes and nothing
//! else, the address and device each request counts against, and the one 401 over every limit.

mod common;

use std::collections::BTreeSet;
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};

use axum::http::{HeaderMap, HeaderValue, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::{ManualClock, MemJournal, Reply, WORKSPACE, at, hosted, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, StartRefusal, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::reduction::{
    self, CHALLENGES_IN_ALL, DEVICE_COOKIE, INGRESS_PEERS, Reduction,
};
use mandate_api_server::session::{OsRandom, Random, Sessions, Unauthenticated};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, ClientKey, IssuerConfig, ReductionLimits};
use mandate_canon::{Value, decode_ulid, parse};
use ring::digest::{SHA256, digest};

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

/// A generator whose every draw fails.
struct Failing;

impl Random for Failing {
    fn fill(&self, _out: &mut [u8]) -> Result<(), Unauthenticated> {
        Err(Unauthenticated)
    }
}

/// A generator that draws distinct 32-byte values for as long as a test needs.
#[derive(Default)]
struct Wide(Mutex<u64>);

impl Random for Wide {
    fn fill(&self, out: &mut [u8]) -> Result<(), Unauthenticated> {
        let mut n = self.0.lock().expect("unpoisoned");
        *n += 1;
        out.iter_mut().for_each(|b| *b = 0xa5);
        let bytes = n.to_be_bytes();
        let tail = out.len() - bytes.len();
        out[tail..].copy_from_slice(&bytes);
        Ok(())
    }
}

fn ip(text: &str) -> IpAddr {
    text.parse().expect("an address")
}

fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
    let mut map = HeaderMap::new();
    for (name, value) in pairs {
        map.append(
            header::HeaderName::from_bytes(name.as_bytes()).expect("a name"),
            HeaderValue::from_str(value).expect("a value"),
        );
    }
    map
}

fn client(address: &str, device: &str) -> ClientKey {
    ClientKey {
        address: address.to_owned(),
        device: device.to_owned(),
    }
}

#[test]
#[ignore = "pending E9-1"]
fn the_limits_are_dec_2117s_numbers() {
    let n = |v: u32| core::num::NonZeroU32::new(v).expect("not zero");
    assert_eq!(
        reduction::limits(),
        ReductionLimits {
            challenges_per_address: n(32),
            challenges_per_device: n(4),
            failures_per_address: n(100),
            failures_per_device: n(10),
        }
    );
    assert_eq!(CHALLENGES_IN_ALL, 4_096);
}

#[test]
#[ignore = "pending E9-1"]
fn ingress_peers_name_addresses_and_refuse_anything_else() {
    assert_eq!(reduction::ingress_peers(None), Ok(Vec::new()));
    assert_eq!(reduction::ingress_peers(Some("")), Ok(Vec::new()));
    assert_eq!(
        reduction::ingress_peers(Some("127.0.0.1")),
        Ok(vec![ip("127.0.0.1")])
    );
    assert_eq!(
        reduction::ingress_peers(Some("127.0.0.1, ::1")),
        Ok(vec![ip("127.0.0.1"), ip("::1")])
    );
    for refused in [
        "10.0.0.0/8",
        "localhost",
        "127.0.0.1,",
        ",::1",
        "127.0.0.1;::1",
    ] {
        assert_eq!(
            reduction::ingress_peers(Some(refused)),
            Err(StartRefusal::Setting {
                name: INGRESS_PEERS
            }),
            "{refused}"
        );
    }
}

#[test]
#[ignore = "pending E9-1"]
fn an_ipv4_address_counts_as_itself_and_an_ipv6_as_its_64() {
    let none = HeaderMap::new();
    let at = |peer: &str| reduction::address(ip(peer), &none, &[]);
    assert_eq!(at("203.0.113.7"), "203.0.113.7");
    assert_eq!(
        at("::ffff:203.0.113.7"),
        "203.0.113.7",
        "an IPv4-mapped peer"
    );
    assert_eq!(at("2001:db8:1:2:3:4:5:6"), "2001:db8:1:2::/64");
    assert_eq!(at("2001:db8:1:2:ffff::1"), at("2001:db8:1:2::"));
    assert_ne!(at("2001:db8:1:3::1"), at("2001:db8:1:2::1"));
    assert_ne!(at("203.0.113.8"), at("203.0.113.7"));
}

#[test]
#[ignore = "pending E9-1"]
fn a_forwarded_address_counts_only_from_a_listed_ingress_peer() {
    let loopback = ip("127.0.0.1");
    let ingress = [loopback];
    let forwarded = headers(&[("x-forwarded-for", "198.51.100.1, 203.0.113.9")]);
    assert_eq!(
        reduction::address(loopback, &forwarded, &ingress),
        "203.0.113.9",
        "the entry the ingress appended"
    );
    assert_eq!(
        reduction::address(loopback, &forwarded, &[]),
        "127.0.0.1",
        "no ingress named: the peer"
    );
    assert_eq!(
        reduction::address(ip("10.0.0.5"), &forwarded, &ingress),
        "10.0.0.5",
        "a peer that is not the ingress forwards nothing"
    );
    assert_eq!(
        reduction::address(loopback, &HeaderMap::new(), &ingress),
        "127.0.0.1"
    );
    let junk = headers(&[("x-forwarded-for", "203.0.113.9, nonsense")]);
    assert_eq!(reduction::address(loopback, &junk, &ingress), "127.0.0.1");
    let lines = headers(&[
        ("x-forwarded-for", "198.51.100.1"),
        ("x-forwarded-for", "198.51.100.2, 2001:db8:9:9::1"),
    ]);
    assert_eq!(
        reduction::address(loopback, &lines, &ingress),
        "2001:db8:9:9::/64",
        "the lines are one list"
    );
    let through = headers(&[(
        "x-forwarded-for",
        "203.0.113.9, 127.0.0.1, ::ffff:127.0.0.1",
    )]);
    assert_eq!(
        reduction::address(loopback, &through, &ingress),
        "203.0.113.9",
        "walked from the right past every listed peer"
    );
    let mapped = headers(&[("x-forwarded-for", "::ffff:203.0.113.9")]);
    assert_eq!(
        reduction::address(loopback, &mapped, &ingress),
        "203.0.113.9",
        "a forwarded IPv4-mapped address counts as its IPv4 address, never as ::/64"
    );
    let only = headers(&[("x-forwarded-for", "127.0.0.1")]);
    assert_eq!(reduction::address(loopback, &only, &ingress), "127.0.0.1");
    assert_eq!(
        reduction::address(ip("::ffff:127.0.0.1"), &forwarded, &ingress),
        "203.0.113.9",
        "an IPv4-mapped peer matches its IPv4 address"
    );
    assert_eq!(
        reduction::address(loopback, &forwarded, &[ip("::ffff:127.0.0.1")]),
        "203.0.113.9",
        "and a mapped listing matches the IPv4 peer"
    );
}

#[test]
#[ignore = "pending E9-1"]
fn the_device_key_is_a_digest_of_one_well_formed_cookie() {
    let cookie = |value: &str| headers(&[("cookie", value)]);
    let key = reduction::device(&cookie(&format!("{DEVICE_COOKIE}={DEVICE}"))).expect("a key");
    let hex: String = digest(&SHA256, DEVICE.as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(key, hex, "the value's SHA-256, never the value");
    assert_eq!(
        reduction::device(&cookie(&format!(
            "__Host-mandate_session=abc; {DEVICE_COOKIE}={DEVICE}"
        ))),
        Some(hex)
    );
    let other = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8";
    assert_ne!(
        reduction::device(&cookie(&format!("{DEVICE_COOKIE}={other}"))),
        Some(key)
    );
    for refused in [
        String::new(),
        format!("mandate_device={DEVICE}"),
        format!("{DEVICE_COOKIE}={}", &DEVICE[..42]),
        format!("{DEVICE_COOKIE}={DEVICE}="),
        format!("{DEVICE_COOKIE}=q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJB"),
        format!("{DEVICE_COOKIE}={DEVICE}; {DEVICE_COOKIE}={other}"),
    ] {
        assert_eq!(reduction::device(&cookie(&refused)), None, "{refused}");
    }
    assert_eq!(reduction::device(&HeaderMap::new()), None);
}

#[test]
#[ignore = "pending E9-1"]
fn the_rp_id_is_the_app_origins_host() {
    assert_eq!(
        reduction::rp_id("https://app.owlhead.ai").as_deref(),
        Some("app.owlhead.ai")
    );
    assert_eq!(
        reduction::rp_id("https://app-staging.owlhead.ai:8443").as_deref(),
        Some("app-staging.owlhead.ai")
    );
    for refused in [
        "http://app.owlhead.ai",
        "https://localhost",
        "app.owlhead.ai",
        "https://",
    ] {
        assert_eq!(reduction::rp_id(refused), None, "{refused}");
    }
}

#[test]
#[ignore = "pending E9-1"]
fn a_challenge_id_is_the_ulid_of_its_millisecond_and_first_ten_bytes() {
    let bytes: [u8; 32] = core::array::from_fn(|i| u8::try_from(i * 7 % 256).expect("a byte"));
    let issued = mandate_time::UtcNanos::from_parts(T0, 123_456_789).expect("an instant");
    let id = reduction::challenge_id(issued, &bytes);
    let value = decode_ulid(&id).expect("a ULID");
    let millis = u128::try_from(T0).expect("positive") * 1_000 + 123;
    let random = bytes[..10]
        .iter()
        .fold(0_u128, |acc, b| (acc << 8) | u128::from(*b));
    assert_eq!(value, (millis << 80) | random);
}

#[test]
#[ignore = "pending E9-1"]
fn the_store_issues_within_the_gate_and_holds_at_most_4096_in_all() {
    let store = Reduction::new(Vec::new(), Box::new(Counting::default()));
    let now = at(T0);
    let first = store.issue(&client("a1", "d1"), now).expect("issued");
    assert_eq!(first.bytes, [0; 32]);
    assert_eq!(first.issued_at, now);
    assert_eq!(first.expires_at, at(T0 + 300));
    assert_eq!(
        first.challenge_id,
        reduction::challenge_id(now, &first.bytes)
    );
    for _ in 1..4 {
        store.issue(&client("a1", "d1"), now).expect("issued");
    }
    assert!(
        store.issue(&client("a1", "d1"), now).is_err(),
        "a fifth on one device"
    );
    for d in 2..=29 {
        store
            .issue(&client("a1", &format!("d{d}")), now)
            .expect("issued");
    }
    assert!(
        store.issue(&client("a1", "d99"), now).is_err(),
        "a 33rd on one address"
    );
    assert!(
        store.issue(&client("a1", "d1"), at(T0 + 300)).is_ok(),
        "expired challenges free their slots"
    );

    let wide = Reduction::new(Vec::new(), Box::new(Wide::default()));
    for n in 0..3_072 {
        wide.issue(&client(&format!("a{n}"), &format!("d{n}")), now)
            .expect("issued");
    }
    assert!(
        wide.issue(&client("a0", "e0"), now).is_err(),
        "past 3,072, an address that holds one gets no more"
    );
    assert!(wide.issue(&client("n0", "m0"), now).is_ok(), "the reserve");
    assert!(wide.issue(&client("n0", "m1"), now).is_err());
    for n in 1..1_024 {
        wide.issue(&client(&format!("n{n}"), &format!("m{n}")), now)
            .expect("issued from the reserve");
    }
    assert!(
        wide.issue(&client("fresh", "fresh"), now).is_err(),
        "the 4,097th outstanding in all"
    );
    assert!(wide.issue(&client("fresh", "fresh"), at(T0 + 300)).is_ok());

    let broken = Reduction::new(Vec::new(), Box::new(Failing));
    assert!(broken.issue(&client("a1", "d1"), now).is_err());
    assert!(broken.new_device().is_err());
    let device = Reduction::new(Vec::new(), Box::new(Counting::default()))
        .new_device()
        .expect("drawn");
    assert_eq!(device, URL_SAFE_NO_PAD.encode([0_u8; 32]));
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
    let nowhere = send(f.addr, "GET", "/nowhere", &[], b"");
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
        assert_eq!(reply.status, 400, "{:?}", String::from_utf8_lossy(body));
        assert_eq!(
            json(&reply).get("code"),
            Some(&Value::Str("invalid".to_owned()))
        );
    }
    let other = send(f.addr, "GET", PATH, &[("Origin", APP)], b"");
    the_one_401(&f, &other, "another method");
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
