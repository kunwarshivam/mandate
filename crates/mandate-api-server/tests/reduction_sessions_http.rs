//! Route 2's opening route over HTTP (workspace API spec §4.10 route 2 items 1, 4 and 5;
//! identity spec §6.4 route 2; E9-1 S2b-3c): a verified assertion over a challenge the challenge
//! route issued opens a reduction-only session for 15 minutes; every refusal is the one 401 with
//! the same headers and ends nothing; and a refresh answers a route-2 session `reduction_only`.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::net::IpAddr;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use common::control::{Rows, activation, hosted_stream};
use common::passkey::{ORIGIN, Signer, enrolled};
use common::{ManualClock, MemJournal, Reply, WORKSPACE, at, hosted, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::members::control_parser;
use mandate_api_server::reduction::{self, Reduction};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse};
use mandate_identity::{PrincipalId, Role};

const T0: i64 = common::control::T0;
const ALICE: PrincipalId = PrincipalId(7);
const CREDENTIAL: &str = "AAECAwQFBgcICQoLDA0ODw";
const DEVICE: &str = "q83vEjRWeJCrze8SNFZ4kKvN7xI0VniQq83vEjRWeJA";

struct Fixture {
    addr: std::net::SocketAddr,
    sessions: Arc<Sessions>,
    journal: Arc<MemJournal>,
    reduction: Arc<Reduction>,
    clock: Arc<ManualClock>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// The hosted workspace where ALICE is an active operator who enrolled `signer`, served at T0.
/// `signed_in` opens a full session for her first, the sign-in route 2's interim standing needs.
/// Her passkey's enrolment cool-off runs to 2030, so every opening here is by a cooling-off
/// passkey, which route 2 serves (identity spec §10.1).
fn fixture(signer: &Signer, signed_in: bool) -> Fixture {
    fixture_behind(signer, signed_in, Vec::new())
}

/// [`fixture`] behind the ingress peers `ingress` (`MANDATE_API_INGRESS_PEERS`).
fn fixture_behind(signer: &Signer, signed_in: bool, ingress: Vec<IpAddr>) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-reduction-sessions-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x7a_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let mut rows = Rows::opened();
    rows.hosted(
        "MemberActivated",
        T0 - 1_000,
        activation(ALICE, &BTreeSet::from([Role::Operator]), T0 - 1_000),
    );
    let cose = signer.cose_key();
    rows.hosted(
        "CredentialEnrolled",
        T0 - 900,
        enrolled(ALICE, CREDENTIAL, &cose, signer.alg()),
    );
    let journal = Arc::new(MemJournal::default());
    journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(hosted_stream(), rows.rows);
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    if signed_in {
        sessions
            .open(ALICE, "sha256:aaaa".to_owned(), None, at(T0 - 500))
            .expect("signed in");
    }
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let keys: BTreeMap<Digest, Vec<u8>> = BTreeMap::from([(Digest::of(&cose), cose)]);
    let reduction = Arc::new(Reduction::new(ingress, Box::new(OsRandom::new())));
    let clock = ManualClock::at(T0);
    let app = App {
        settings: Settings {
            app_origin: ORIGIN.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: Arc::clone(&clock) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(control_parser(hosted().workspace)),
        hosted: hosted(),
        artifacts: Some(Arc::new(keys)),
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::clone(&reduction),
        folds: Arc::default(),
        bars: None,
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        sessions,
        journal,
        reduction,
        clock,
    }
}

fn browser(extra: &[(&'static str, String)]) -> Vec<(&'static str, String)> {
    let mut all = vec![
        ("Origin", ORIGIN.to_owned()),
        ("X-Mandate-Request", "1".to_owned()),
        ("Cookie", format!("__Host-mandate_device={DEVICE}")),
    ];
    all.extend_from_slice(extra);
    all
}

fn post(f: &Fixture, path: &str, headers: &[(&'static str, String)], body: &[u8]) -> Reply {
    let headers: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (*k, v.as_str())).collect();
    send(f.addr, "POST", path, &headers, body)
}

fn json(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn text(value: &Value, key: &str) -> String {
    match value.get(key) {
        Some(Value::Str(s)) => s.clone(),
        other => panic!("{key}: {other:?}"),
    }
}

/// A challenge from the challenge route: its id and its bytes.
fn challenge(f: &Fixture) -> (String, Vec<u8>) {
    challenge_with(f, &browser(&[]))
}

/// A challenge asked for with `headers`.
fn challenge_with(f: &Fixture, headers: &[(&'static str, String)]) -> (String, Vec<u8>) {
    let reply = post(f, "/v1/reduction-sessions/challenges", headers, b"{}");
    assert_eq!(reply.status, 201);
    let answer = json(&reply);
    let bytes = URL_SAFE_NO_PAD
        .decode(text(&answer, "webauthn_challenge"))
        .expect("base64url");
    (text(&answer, "challenge_id"), bytes)
}

fn request(
    challenge_id: &str,
    passkey_id: &str,
    assertion: &(Vec<u8>, Vec<u8>, Vec<u8>),
) -> Vec<u8> {
    serde_json::to_vec(&serde_json::json!({
        "challenge_id": challenge_id,
        "passkey_id": passkey_id,
        "authenticator_data": URL_SAFE_NO_PAD.encode(&assertion.0),
        "client_data_json": URL_SAFE_NO_PAD.encode(&assertion.1),
        "signature": URL_SAFE_NO_PAD.encode(&assertion.2),
    }))
    .expect("JSON")
}

fn present(f: &Fixture, extra: &[(&'static str, String)], body: &[u8]) -> Reply {
    post(f, "/v1/reduction-sessions", &browser(extra), body)
}

/// The session cookie a reply set, and its attributes.
fn session_set(reply: &Reply) -> Option<String> {
    reply
        .all("set-cookie")
        .into_iter()
        .find(|cookie| cookie.starts_with(&format!("{COOKIE_NAME}=")))
        .map(str::to_owned)
}

/// Every `Set-Cookie` value of the reply that sets or clears the device cookie: the assertion
/// route never does (§4.10 route 2 item 3).
fn device_set(reply: &Reply) -> Vec<String> {
    reply
        .all("set-cookie")
        .into_iter()
        .filter(|cookie| cookie.trim_start().starts_with("__Host-mandate_device"))
        .map(str::to_owned)
        .collect()
}

fn cookie_value(set: &str) -> String {
    set.split(';')
        .next()
        .and_then(|pair| pair.split_once('='))
        .map(|(_, value)| value.to_owned())
        .expect("a value")
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

#[test]
fn a_verified_assertion_opens_a_reduction_only_session_for_fifteen_minutes() {
    for signer in [Signer::es256(), Signer::ed25519()] {
        let f = fixture(&signer, true);
        let (id, bytes) = challenge(&f);
        let body = request(&id, CREDENTIAL, &signer.assert(&bytes));
        let reply = present(&f, &[], &body);
        assert_eq!(
            reply.status,
            201,
            "{}, a cooling-off passkey: {:?}",
            signer.alg(),
            String::from_utf8_lossy(&reply.body)
        );
        assert_eq!(reply.header("cache-control"), Some("no-store"));
        assert_eq!(device_set(&reply), Vec::<String>::new(), "no device cookie");
        let set = session_set(&reply).expect("the session cookie");
        assert_eq!(
            reply.all("set-cookie").len(),
            1,
            "one Set-Cookie, the session's"
        );
        let (pair, attributes) = set.split_once("; ").expect("attributes after the value");
        let (name, value) = pair.split_once('=').expect("name=value");
        assert_eq!(name, COOKIE_NAME);
        assert!(!value.is_empty(), "{set}");
        assert_eq!(
            attributes.split("; ").collect::<Vec<_>>(),
            [
                "Path=/",
                "Max-Age=900",
                "Secure",
                "HttpOnly",
                "SameSite=Strict"
            ],
            "the whole attribute list, in order, and nothing else (no Domain, no Expires): {set}"
        );
        let answer = json(&reply);
        assert_eq!(text(&answer, "kind"), "reduction_only");
        assert_eq!(text(&answer, "expires_at"), at(T0 + 900).to_string());
        let workspaces = match answer.get("workspaces") {
            Some(Value::Array(items)) => items.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(workspaces.len(), 1, "the one hosted workspace");
        assert_eq!(text(&workspaces[0], "workspace_id"), WORKSPACE);
        assert_eq!(workspaces[0].get("label"), Some(&Value::Null));
        for ids in ["agent_ids", "connection_ids"] {
            assert!(
                matches!(workspaces[0].get(ids), Some(Value::Array(_))),
                "{ids}"
            );
        }
        let cookie = cookie_value(&set);
        assert!(f.sessions.opened_by_passkey(&cookie, at(T0)));
        let again = present(&f, &[], &body);
        assert_eq!(
            again.status,
            401,
            "{}: the challenge was used once",
            signer.alg()
        );
        assert_eq!(device_set(&again), Vec::<String>::new(), "no device cookie");
        let appended = f.journal.appends.lock().expect("unpoisoned").len();
        assert_eq!(
            appended, 0,
            "nothing is journaled until SessionOpened is registered"
        );
    }
}

#[test]
fn every_refusal_is_the_same_401_and_ends_nothing() {
    let signer = Signer::es256();
    let f = fixture(&signer, true);
    let full = f
        .sessions
        .open(ALICE, "sha256:aaaa".to_owned(), None, at(T0 - 10))
        .expect("a full session");
    let presented = [(
        "Cookie",
        format!(
            "__Host-mandate_device={DEVICE}; {COOKIE_NAME}={}",
            full.value()
        ),
    )];
    let no_marks = send(
        f.addr,
        "POST",
        "/v1/reduction-sessions",
        &[("Origin", ORIGIN)],
        b"{}",
    );
    assert_eq!(no_marks.status, 403, "the browser check comes first");
    let (used, used_bytes) = challenge(&f);
    let opened = present(
        &f,
        &[],
        &request(&used, CREDENTIAL, &signer.assert(&used_bytes)),
    );
    assert_eq!(opened.status, 201);
    let mut bodies: Vec<(&str, Vec<u8>)> = vec![
        ("not JSON", b"{".to_vec()),
        ("an empty object", b"{}".to_vec()),
        (
            "an unknown challenge",
            request(
                "01JB3K6F0C9R2V7N4M8Q1T5W3Z",
                CREDENTIAL,
                &signer.assert(&[1; 32]),
            ),
        ),
        (
            "a used challenge",
            request(&used, CREDENTIAL, &signer.assert(&used_bytes)),
        ),
    ];
    let (id, bytes) = challenge(&f);
    let mut forged = signer.assert(&bytes);
    forged.2[12] ^= 1;
    bodies.push(("a bad signature", request(&id, CREDENTIAL, &forged)));
    let (id, bytes) = challenge(&f);
    bodies.push((
        "an unknown passkey",
        request(&id, "EBESExQVFhcYGRobHB0eHw", &signer.assert(&bytes)),
    ));
    let (id, bytes) = challenge(&f);
    let mut extra: serde_json::Value =
        serde_json::from_slice(&request(&id, CREDENTIAL, &signer.assert(&bytes))).expect("JSON");
    extra["roles"] = serde_json::json!(["owner"]);
    bodies.push(("an extra member", serde_json::to_vec(&extra).expect("JSON")));
    let (id, bytes) = challenge(&f);
    let mut padded: serde_json::Value =
        serde_json::from_slice(&request(&id, CREDENTIAL, &signer.assert(&bytes))).expect("JSON");
    padded["signature"] =
        serde_json::json!(format!("{}==", padded["signature"].as_str().expect("text")));
    bodies.push((
        "padded base64url",
        serde_json::to_vec(&padded).expect("JSON"),
    ));
    let reference = present(&f, &presented, &bodies[0].1);
    for (name, body) in &bodies {
        let reply = present(&f, &presented, body);
        assert_eq!(reply.status, 401, "{name}");
        assert_eq!(reply.body, reference.body, "{name}: the one body");
        assert_eq!(shape(&reply), shape(&reference), "{name}: the same headers");
        assert_eq!(reply.header("cache-control"), Some("no-store"), "{name}");
        assert!(
            session_set(&reply).is_none(),
            "{name}: no cookie set or cleared"
        );
        assert_eq!(
            device_set(&reply),
            Vec::<String>::new(),
            "{name}: the device cookie neither set nor cleared"
        );
    }
    assert!(
        f.sessions
            .authenticate(full.value(), mandate_authn::Request::Other, at(T0))
            .is_ok(),
        "a refused call ends nothing"
    );
    let unseen = fixture(&signer, false);
    let (id, bytes) = challenge(&unseen);
    let reply = present(
        &unseen,
        &[],
        &request(&id, CREDENTIAL, &signer.assert(&bytes)),
    );
    assert_eq!(reply.status, 401, "no sign-in since the process started");
    assert_eq!(reply.body, reference.body);
}

#[test]
fn an_opening_replaces_the_session_it_presented_and_a_refresh_keeps_it_reduction_only() {
    let signer = Signer::es256();
    let f = fixture(&signer, true);
    let full = f
        .sessions
        .open(ALICE, "sha256:aaaa".to_owned(), None, at(T0 - 10))
        .expect("a full session");
    let presented = [(
        "Cookie",
        format!(
            "__Host-mandate_device={DEVICE}; {COOKIE_NAME}={}",
            full.value()
        ),
    )];
    let (id, bytes) = challenge(&f);
    let reply = present(
        &f,
        &presented,
        &request(&id, CREDENTIAL, &signer.assert(&bytes)),
    );
    assert_eq!(reply.status, 201);
    assert!(
        f.sessions
            .authenticate(full.value(), mandate_authn::Request::Pause, at(T0))
            .is_err(),
        "the session it presented ended"
    );
    let cookie = cookie_value(&session_set(&reply).expect("a cookie"));
    let refresh = |body: &[u8]| {
        let headers = [
            ("Origin", ORIGIN),
            ("X-Mandate-Request", "1"),
            ("Cookie", &format!("{COOKIE_NAME}={cookie}")),
        ];
        send(f.addr, "POST", "/v1/sessions/refresh", &headers, body)
    };
    for body in [
        &br#"{"access_token":null}"#[..],
        br#"{"access_token":"x.y.z"}"#,
    ] {
        let refused = refresh(body);
        assert_eq!(
            refused.status,
            403,
            "{:?}",
            String::from_utf8_lossy(&refused.body)
        );
        assert_eq!(text(&json(&refused), "code"), "reduction_only");
        assert!(session_set(&refused).is_none(), "nothing set or cleared");
        assert!(f.sessions.opened_by_passkey(&cookie, at(T0)), "kept");
    }
    assert_eq!(
        f.sessions.expires_at(&cookie, at(T0)),
        Ok(at(T0 + 900)),
        "never extended"
    );
}

/// A presentation that names no challenge, from the device `cookie` (none when `None`) and, when
/// `forwarded`, through the ingress for that address.
fn bad(f: &Fixture, cookie: Option<String>, forwarded: Option<String>) -> Reply {
    let mut headers = vec![
        ("Origin", ORIGIN.to_owned()),
        ("X-Mandate-Request", "1".to_owned()),
    ];
    if let Some(cookie) = cookie {
        headers.push(("Cookie", format!("__Host-mandate_device={cookie}")));
    }
    if let Some(forwarded) = forwarded {
        headers.push(("X-Forwarded-For", forwarded));
    }
    post(f, "/v1/reduction-sessions", &headers, b"{}")
}

/// A device cookie of its own for each `n`.
fn device_cookie(n: u32) -> String {
    let mut bytes = [0xa5_u8; 32];
    bytes[..4].copy_from_slice(&n.to_be_bytes());
    URL_SAFE_NO_PAD.encode(bytes)
}

#[test]
fn ten_bad_presentations_from_one_device_raise_the_alert_once() {
    let signer = Signer::es256();
    let f = fixture(&signer, true);
    for i in 1..=12_u64 {
        let (id, bytes) = challenge(&f);
        let mut forged = signer.assert(&bytes);
        forged.2[12] ^= 1;
        let reply = present(&f, &[], &request(&id, CREDENTIAL, &forged));
        assert_eq!(reply.status, 401, "a real challenge, a bad signature");
        assert_eq!(
            f.reduction.alerts_raised(),
            u64::from(i >= 10),
            "after {i}: the tenth failure of the device raises it, and only once"
        );
    }
    let (id, bytes) = challenge(&f);
    let reply = present(&f, &[], &request(&id, CREDENTIAL, &signer.assert(&bytes)));
    assert_eq!(
        reply.status, 201,
        "the device is past its failure limit, and a verified assertion is still never refused"
    );
    assert_eq!(
        f.reduction.alerts_raised(),
        1,
        "and an opening raises nothing"
    );
}

#[test]
fn an_opening_counts_no_failure() {
    let signer = Signer::es256();
    let f = fixture(&signer, true);
    let forged_presentation = |f: &Fixture| {
        let (id, bytes) = challenge(f);
        let mut forged = signer.assert(&bytes);
        forged.2[12] ^= 1;
        present(f, &[], &request(&id, CREDENTIAL, &forged)).status
    };
    for _ in 0..9 {
        assert_eq!(forged_presentation(&f), 401);
    }
    let (id, bytes) = challenge(&f);
    let opened = present(&f, &[], &request(&id, CREDENTIAL, &signer.assert(&bytes)));
    assert_eq!(opened.status, 201);
    assert_eq!(
        f.reduction.alerts_raised(),
        0,
        "nine failures and an opening: the opening is no tenth failure"
    );
    assert_eq!(forged_presentation(&f), 401);
    assert_eq!(
        f.reduction.alerts_raised(),
        1,
        "the next failure is the device's tenth, and raises the alert"
    );
}

#[test]
fn behind_a_listed_ingress_each_forwarded_address_counts_apart() {
    let signer = Signer::es256();
    let f = fixture_behind(
        &signer,
        true,
        vec!["127.0.0.1".parse().expect("an address")],
    );
    for i in 0..100_u32 {
        let forwarded = format!("198.51.{}.{}", 100 + i / 50, i % 50 + 1);
        assert_eq!(bad(&f, Some(device_cookie(i)), Some(forwarded)).status, 401);
    }
    assert_eq!(
        f.reduction.alerts_raised(),
        0,
        "a hundred failures from a hundred forwarded addresses reach no address's limit"
    );
    for i in 0..100_u32 {
        let reply = bad(
            &f,
            Some(device_cookie(1_000 + i)),
            Some("192.0.2.7".to_owned()),
        );
        assert_eq!(reply.status, 401);
        assert_eq!(
            f.reduction.alerts_raised(),
            u64::from(i == 99),
            "the one forwarded address reaches its hundred at its hundredth failure"
        );
    }
    let from_it = [
        ("Origin", ORIGIN.to_owned()),
        ("X-Mandate-Request", "1".to_owned()),
        (
            "Cookie",
            format!("__Host-mandate_device={}", device_cookie(5_000)),
        ),
        ("X-Forwarded-For", "192.0.2.7".to_owned()),
    ];
    let (id, bytes) = challenge_with(&f, &from_it);
    let body = request(&id, CREDENTIAL, &signer.assert(&bytes));
    let reply = post(&f, "/v1/reduction-sessions", &from_it, &body);
    assert_eq!(
        reply.status, 201,
        "the address is past its failure limit, and a verified assertion is still never refused"
    );
    assert_eq!(
        f.reduction.alerts_raised(),
        1,
        "and an opening raises nothing"
    );
}

/// A presentation without a device cookie, or with a malformed one, counts against one device
/// budget shared by every such client, for counting and the alert only ([DEC-2127]).
///
/// [DEC-2127]: ../../../docs/project/decisions/DEC-2127.md
#[test]
fn clients_without_a_device_cookie_share_one_device_budget() {
    let f = fixture_behind(
        &Signer::es256(),
        true,
        vec!["127.0.0.1".parse().expect("an address")],
    );
    for i in 1..=10_u64 {
        let forwarded = format!("203.0.113.{i}");
        assert_eq!(bad(&f, None, Some(forwarded)).status, 401);
        assert_eq!(
            f.reduction.alerts_raised(),
            u64::from(i == 10),
            "ten cookie-less failures from ten addresses share one device's ten"
        );
    }
}

#[test]
fn a_challenge_is_spent_by_its_first_presentation_whatever_follows() {
    let signer = Signer::es256();
    let f = fixture(&signer, true);
    let (id, bytes) = challenge(&f);
    let mut forged = signer.assert(&bytes);
    forged.2[12] ^= 1;
    let refused = present(&f, &[], &request(&id, CREDENTIAL, &forged));
    assert_eq!(refused.status, 401, "a bad signature");
    let retried = present(&f, &[], &request(&id, CREDENTIAL, &signer.assert(&bytes)));
    assert_eq!(
        retried.status, 401,
        "the refused presentation spent the challenge, so a correct assertion over it is refused"
    );
    assert_eq!(retried.body, refused.body, "the one 401");
    assert!(session_set(&retried).is_none());
    let (id, bytes) = challenge(&f);
    assert_eq!(
        present(&f, &[], &request(&id, CREDENTIAL, &signer.assert(&bytes))).status,
        201,
        "a fresh challenge serves"
    );
}

#[test]
fn an_expired_challenge_is_the_one_401() {
    let signer = Signer::es256();
    let f = fixture(&signer, true);
    let reference = present(&f, &[], b"{}");
    let (id, bytes) = challenge(&f);
    f.clock.set(T0 + 300);
    let reply = present(&f, &[], &request(&id, CREDENTIAL, &signer.assert(&bytes)));
    assert_eq!(
        reply.status, 401,
        "at its expiry, 300 s after it was issued"
    );
    assert_eq!(reply.body, reference.body, "the one 401");
    assert!(session_set(&reply).is_none());
    let (id, bytes) = challenge(&f);
    assert_eq!(
        present(&f, &[], &request(&id, CREDENTIAL, &signer.assert(&bytes))).status,
        201,
        "a challenge issued then still serves"
    );
}

#[test]
fn the_alert_names_only_its_code_and_runbook() {
    assert_eq!(
        reduction::alert_line(),
        "operator_alert reduction_failures_over_limit docs/runbooks/RB-20-reduction-failures.md",
        "no address, device, member, passkey, or challenge: the line takes no argument"
    );
    assert_eq!(
        reduction::FAILURES_RUNBOOK,
        "docs/runbooks/RB-20-reduction-failures.md"
    );
    let runbook = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(reduction::FAILURES_RUNBOOK);
    assert!(runbook.is_file(), "the runbook it names exists");
}
