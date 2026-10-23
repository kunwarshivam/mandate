//! One's own passkeys over HTTP (workspace API spec §3.6, DEC-2108 items 4 and 5, DEC-2115,
//! DEC-2118 item 1, DEC-2124): the registration challenge and the list. Expected ids, user
//! handles, and watermarks are computed here from the specs, and the member's passkeys by a fold
//! of the test's own rows, kept apart from `mandate-passkey`.

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::control::{Rows, hosted_stream};
use common::{MemJournal, Reply, WORKSPACE, b64url, hosted, send, serve};
use mandate_api::problem::{Effect, Problem, ProblemCode, Violation};
use mandate_api::wire::decode;
use mandate_api_server::challenges::Challenges;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::passkeys::{
    CHALLENGE_OPERATION, FRESH_S, TOMBSTONE_S, TOMBSTONES_PER_PRINCIPAL,
};
use mandate_api_server::session::{Answer, COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Value, encode_ulid, parse, to_canonical};
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;
use serde_json::{Value as Json, json};

const APP: &str = "https://app.owlhead.test";
const RP_ID: &str = "app.owlhead.test";
const T0: i64 = 1_791_000_000;
const KEY: &str = "k1Z-9_aaaaaaaaaa";
const KEY_2: &str = "k2Z-9_bbbbbbbbbb";
const SUBJECT: &str = "sha256:aaaa";
/// An operator and approver.
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
/// A viewer: the `own` cell holds for every role.
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
/// No membership.
const DAVE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W43";
const PASSKEY_A: &str = "AAECAwQFBgcICQoLDA0ODw";
const PASSKEY_B: &str = "EBESExQVFhcYGRobHB0eHw";
const PASSKEY_C: &str = "QEFCQ0RFRkdISUpLTE1OTw";
const KEY_REF: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
const COOL_OFF_ENDS: &str = "2026-10-04T04:00:00.000000000Z";

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    sessions: Arc<Sessions>,
    clock: Arc<NanoClock>,
}

/// The server's clock, settable to the nanosecond, so a window's end is tested at its exact
/// instant and one nanosecond past it.
struct NanoClock(Mutex<UtcNanos>);

impl NanoClock {
    fn set(&self, secs: i64) {
        self.set_at(at(secs));
    }

    fn set_at(&self, now: UtcNanos) {
        *self.0.lock().expect("unpoisoned") = now;
    }

    fn now_at(&self) -> UtcNanos {
        *self.0.lock().expect("unpoisoned")
    }
}

impl mandate_api_server::clock::Clock for NanoClock {
    fn now(&self) -> Option<UtcNanos> {
        Some(self.now_at())
    }
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// A server for the hosted workspace, its control stream `rows`, where ALICE and BOB are members
/// and DAVE is none, with `challenges` as its store.
fn fixture(rows: &Rows, challenges: Challenges) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-passkeys-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x47_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(hosted_stream(), rows.rows.clone());
    let records = vec![
        activated(1, id(ALICE), &[Role::Operator, Role::Approver]),
        activated(2, id(BOB), &[Role::Viewer]),
    ];
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let clock = Arc::new(NanoClock(Mutex::new(at(T0))));
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: Arc::clone(&clock) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| Ok(records.clone())),
        hosted: hosted(),
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::new(challenges),
        connect: None,
        reduction: Arc::default(),
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
        clock,
    }
}

fn plain(rows: &Rows) -> Fixture {
    fixture(rows, Challenges::default())
}

fn id(text: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(text).expect("a ULID")
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn time(secs: i64) -> String {
    at(secs).to_string()
}

fn activated(seq: u64, member: PrincipalId, roles: &[Role]) -> MembershipRecord {
    MembershipRecord::new(
        Seal::grant(),
        seq,
        at(T0 - 1_000),
        MembershipEvent::Activated {
            member,
            invitation: None,
            roles: roles.iter().copied().collect::<BTreeSet<_>>(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0 - 1_000),
        },
    )
}

fn enrolled(member: &str, passkey: &str, alg: &str, cool_off_ends: Option<&str>) -> Json {
    json!({
        "member": member, "kind": "passkey", "credential": passkey, "public_key": KEY_REF,
        "alg": alg, "sign_count": 0, "cool_off_ends": cool_off_ends, "step_up": null,
    })
}

fn removed(member: &str, passkey: &str) -> Json {
    json!({
        "member": member, "credential": passkey,
        "step_up": {"assertion_id": "01JAA3P4A000000000000000S1", "authenticated_at": time(T0 - 50), "method": "passkey"},
    })
}

/// A control stream in `paper`.
fn opened() -> Rows {
    Rows::opened()
}

/// `rows` with one more passkey record, recorded at `recorded`.
fn with(mut rows: Rows, kind: &str, payload: Json, recorded: i64) -> Rows {
    rows.hosted(kind, recorded - 1, payload);
    if let Some(last) = rows.rows.last_mut() {
        last.recorded_at = time(recorded);
    }
    rows
}

fn enrol(rows: Rows, member: &str, passkey: &str, recorded: i64) -> Rows {
    with(
        rows,
        "CredentialEnrolled",
        enrolled(member, passkey, "es256", None),
        recorded,
    )
}

fn signed_in(f: &Fixture, who: &str) -> String {
    f.sessions
        .open(id(who), SUBJECT.to_owned(), None, f.clock.now_at())
        .expect("opened")
        .value()
        .to_owned()
}

/// `cookie`'s session refreshed at `secs`, with the clock set there: a refresh renews the tokens
/// and leaves the sign-in (DEC-2124 item 4).
fn refreshed(f: &Fixture, cookie: &str, secs: i64) -> String {
    f.clock.set(secs);
    f.sessions
        .refresh(
            cookie,
            Answer::Granted {
                subject: SUBJECT.to_owned(),
                principal: id(ALICE),
            },
            at(secs),
        )
        .expect("refreshed")
        .expect("a rotated cookie")
        .value()
        .to_owned()
}

fn challenges_path() -> String {
    format!("/v1/workspaces/{WORKSPACE}/passkeys/registration-challenges")
}

fn list_path() -> String {
    format!("/v1/workspaces/{WORKSPACE}/passkeys")
}

fn request(f: &Fixture, headers: &[(&str, &str)], body: &str) -> Reply {
    send(f.addr, "POST", &challenges_path(), headers, body.as_bytes())
}

fn ask(f: &Fixture, cookie: &str, key: &str) -> Reply {
    ask_with(f, cookie, key, "{}")
}

fn ask_with(f: &Fixture, cookie: &str, key: &str, body: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    request(
        f,
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", cookie.as_str()),
            ("Idempotency-Key", key),
        ],
        body,
    )
}

fn list(f: &Fixture, cookie: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "GET",
        &list_path(),
        &[("Origin", APP), ("Cookie", cookie.as_str())],
        b"",
    )
}

/// §3.4's derivation written out from the spec, independent of `mandate_api::idempotency`: the
/// canonical object of four ASCII members in byte order, its SHA-256 by `ring`, and the first 128
/// bits as 26 Crockford base-32 digits after two zero bits.
fn derived(principal: &str, key: &str) -> String {
    derived_in(WORKSPACE, principal, CHALLENGE_OPERATION, key)
}

fn derived_in(workspace: &str, principal: &str, operation: &str, key: &str) -> String {
    let object = format!(
        r#"{{"key":"{key}","operation":"{operation}","principal":"{principal}","workspace":"{workspace}"}}"#
    );
    let sha = ring::digest::digest(&ring::digest::SHA256, object.as_bytes());
    let (head, _) = sha
        .as_ref()
        .split_first_chunk::<16>()
        .expect("SHA-256 has 32 bytes");
    let value = u128::from_be_bytes(*head);
    let digits = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    (0..26_u32)
        .rev()
        .map(|place| {
            char::from(digits[usize::try_from((value >> (5 * place)) & 31).expect("a digit")])
        })
        .collect()
}

/// The oracle reproduces §3.4's worked vector before it is trusted.
#[test]
fn the_derivation_oracle_reproduces_the_specs_worked_vector() {
    assert_eq!(
        derived_in("ws_01", "user_7", "kill_switch", "k1Z-9_aaaaaaaaaa"),
        "7KXV6BGDW36KYC7RE5BPQG28Z0"
    );
}

/// The principal's 16 bytes, big-endian, as the tests' own unpadded base64url.
fn user_handle(principal: &str) -> String {
    b64url(&id(principal).0.to_be_bytes())
}

fn body(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn member(reply: &Reply, name: &str) -> Value {
    match body(reply) {
        Value::Object(members) => members
            .into_iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| value)
            .unwrap_or_else(|| panic!("no {name}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn text(reply: &Reply, name: &str) -> String {
    match member(reply, name) {
        Value::Str(text) => text,
        other => panic!("{name} is not a string: {other:?}"),
    }
}

fn json_of(value: &Value) -> Json {
    serde_json::from_slice(&to_canonical(value)).expect("JSON")
}

fn problem(reply: &Reply) -> Problem {
    decode::<Problem>(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn refused(reply: &Reply, status: u16, code: ProblemCode) {
    assert_eq!(
        reply.status,
        status,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let problem = problem(reply);
    assert_eq!(problem.code, code);
    assert_eq!(problem.effect, Effect::None);
}

fn issued(reply: &Reply) {
    assert_eq!(
        reply.status,
        201,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("cache-control"), Some("no-store"));
}

fn exclude(reply: &Reply) -> Vec<String> {
    match member(reply, "exclude") {
        Value::Array(items) => items
            .into_iter()
            .map(|item| match item {
                Value::Str(text) => text,
                other => panic!("not a passkey id: {other:?}"),
            })
            .collect(),
        other => panic!("exclude is not an array: {other:?}"),
    }
}

/// The watermark of the control stream's last row, as §6.1 writes one.
fn control_watermark(rows: &[StoredEvent]) -> Json {
    let last = rows.last().expect("an opened stream");
    json!([{
        "stream_id": last.stream_id, "seq": last.seq,
        "hash": format!("sha256:{}", last.hash), "recorded_at": last.recorded_at,
    }])
}

/// A registration challenge is journaled by nothing (DEC-2108 item 4).
fn nothing_journaled(f: &Fixture, rows: &Rows) {
    assert_eq!(f.journal.rows_of(&hosted_stream()).len(), rows.rows.len());
}

#[test]
fn a_member_with_no_passkey_obtains_a_challenge_bound_to_them_in_a_fresh_session() {
    let rows = opened();
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    let cookie = refreshed(&f, &cookie, T0 + 200);
    f.clock.set(T0 + FRESH_S);
    let reply = ask(&f, &cookie, KEY);
    issued(&reply);
    assert_eq!(text(&reply, "challenge_id"), derived(ALICE, KEY));
    let challenge = text(&reply, "webauthn_challenge");
    assert_eq!(challenge.len(), 43, "32 bytes, unpadded base64url");
    assert!(
        challenge
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    );
    assert_eq!(text(&reply, "rp_id"), RP_ID);
    assert_eq!(text(&reply, "user_handle"), user_handle(ALICE));
    assert!(exclude(&reply).is_empty());
    assert_eq!(member(&reply, "step_up_needed"), Value::Bool(false));
    assert_eq!(text(&reply, "issued_at"), time(T0 + FRESH_S));
    assert_eq!(text(&reply, "expires_at"), time(T0 + 2 * FRESH_S));
    assert_eq!(
        json_of(&member(&reply, "as_of")),
        control_watermark(&rows.rows)
    );
    let again = ask(&f, &cookie, KEY_2);
    issued(&again);
    assert_ne!(
        text(&again, "webauthn_challenge"),
        challenge,
        "each challenge is fresh bytes"
    );
    nothing_journaled(&f, &rows);
}

#[test]
fn exclude_and_step_up_needed_follow_the_members_own_passkeys() {
    let alice = encode_ulid(id(ALICE).0);
    let bob = encode_ulid(id(BOB).0);
    let rows = with(
        opened(),
        "CredentialEnrolled",
        enrolled(&alice, PASSKEY_A, "es256", Some(COOL_OFF_ENDS)),
        T0 - 400,
    );
    let rows = enrol(rows, &alice, PASSKEY_B, T0 - 300);
    let rows = with(
        rows,
        "CredentialRemoved",
        removed(&alice, PASSKEY_B),
        T0 - 200,
    );
    let rows = enrol(rows, &bob, PASSKEY_C, T0 - 100);
    let f = plain(&rows);
    f.clock.set(T0 + 10 * FRESH_S);
    let cookie = signed_in(&f, ALICE);
    let reply = ask(&f, &cookie, KEY);
    issued(&reply);
    assert_eq!(
        exclude(&reply),
        vec![PASSKEY_A.to_owned()],
        "her own, not removed, a cooling-off passkey included"
    );
    assert_eq!(member(&reply, "step_up_needed"), Value::Bool(true));
    let bob_cookie = signed_in(&f, BOB);
    let bobs = ask(&f, &bob_cookie, KEY);
    issued(&bobs);
    assert_eq!(exclude(&bobs), vec![PASSKEY_C.to_owned()]);
    assert_eq!(text(&bobs, "user_handle"), user_handle(BOB));
}

#[test]
fn a_member_with_no_passkey_past_three_hundred_seconds_from_sign_in_is_stale_after_a_refresh() {
    let rows = opened();
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    let cookie = refreshed(&f, &cookie, T0 + 200);
    f.clock.set(T0 + FRESH_S + 1);
    let reply = ask(&f, &cookie, KEY);
    refused(&reply, 401, ProblemCode::StepUpStale);
    let violations: Vec<(String, String)> = problem(&reply)
        .violations
        .into_iter()
        .filter_map(|v| match v {
            Violation::Schema { path, code, .. } => Some((path, code)),
            Violation::Rule { .. } | Violation::Policy { .. } => None,
        })
        .collect();
    assert_eq!(
        violations,
        vec![("/session".to_owned(), "step_up_stale".to_owned())],
        "one violation at /session tells it from an expired challenge (DEC-2118 item 4)"
    );
    let fresh = signed_in(&f, ALICE);
    issued(&ask(&f, &fresh, KEY_2));
    nothing_journaled(&f, &rows);
}

/// The 300 s are instants, not whole seconds: a sign-in at T0.0 is fresh at T0+300.0 and stale
/// one nanosecond later (DEC-2115 item 2, DEC-2124 item 4).
#[test]
fn the_first_enrolment_window_ends_at_its_exact_instant() {
    let rows = opened();
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    let cookie = refreshed(&f, &cookie, T0 + 200);
    f.clock.set(T0 + FRESH_S);
    issued(&ask(&f, &cookie, KEY));
    for nanos in [1, 900_000_000] {
        f.clock
            .set_at(UtcNanos::from_parts(T0 + FRESH_S, nanos).expect("a time"));
        refused(
            &ask(&f, &cookie, &format!("late-key-{nanos:09}")),
            401,
            ProblemCode::StepUpStale,
        );
    }
    nothing_journaled(&f, &rows);
}

/// A tombstone ages from the challenge's expiry, not from when the store next looked: with no
/// request between the expiry E and E + 1 h, the key is still a conflict at E + 24 h - 1 s and
/// issues fresh bytes at E + 24 h (DEC-2118 item 1).
#[test]
fn a_tombstone_ages_from_the_expiry_however_late_the_store_sees_it() {
    let alice = encode_ulid(id(ALICE).0);
    let rows = enrol(opened(), &alice, PASSKEY_A, T0 - 100);
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    issued(&ask(&f, &cookie, KEY));
    let expiry = T0 + FRESH_S;
    for (secs, conflict) in [
        (expiry + 3_600, true),
        (expiry + TOMBSTONE_S - 1, true),
        (expiry + TOMBSTONE_S, false),
    ] {
        f.clock.set(secs);
        let cookie = signed_in(&f, ALICE);
        let reply = ask(&f, &cookie, KEY);
        if conflict {
            refused(&reply, 409, ProblemCode::IdempotencyConflict);
        } else {
            issued(&reply);
        }
    }
}

#[test]
fn a_retry_answers_the_held_challenge_and_its_key_never_revives_it_after_expiry() {
    let alice = encode_ulid(id(ALICE).0);
    let rows = enrol(opened(), &alice, PASSKEY_A, T0 - 100);
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    let first = ask(&f, &cookie, KEY);
    issued(&first);
    f.clock.set(T0 + FRESH_S - 1);
    let retry = ask(&f, &cookie, KEY);
    issued(&retry);
    for name in [
        "challenge_id",
        "webauthn_challenge",
        "issued_at",
        "expires_at",
    ] {
        assert_eq!(
            text(&retry, name),
            text(&first, name),
            "the same challenge, unchanged: {name}"
        );
    }
    f.clock.set(T0 + FRESH_S);
    let cookie = signed_in(&f, ALICE);
    refused(
        &ask(&f, &cookie, KEY),
        409,
        ProblemCode::IdempotencyConflict,
    );
    f.clock.set(T0 + FRESH_S + TOMBSTONE_S - 1);
    let cookie = signed_in(&f, ALICE);
    refused(
        &ask(&f, &cookie, KEY),
        409,
        ProblemCode::IdempotencyConflict,
    );
    issued(&ask(&f, &cookie, KEY_2));
    f.clock.set(T0 + FRESH_S + TOMBSTONE_S);
    let cookie = signed_in(&f, ALICE);
    let revived = ask(&f, &cookie, KEY);
    issued(&revived);
    assert_eq!(text(&revived, "challenge_id"), derived(ALICE, KEY));
    assert_ne!(
        text(&revived, "webauthn_challenge"),
        text(&first, "webauthn_challenge"),
        "a forgotten tombstone answers fresh bytes, never the old ones"
    );
}

#[test]
fn a_principal_keeps_sixty_four_tombstones_and_drops_the_oldest_first() {
    assert_eq!((TOMBSTONE_S, TOMBSTONES_PER_PRINCIPAL), (86_400, 64));
    let alice = encode_ulid(id(ALICE).0);
    let rows = enrol(opened(), &alice, PASSKEY_A, T0 - 100);
    let f = plain(&rows);
    let keyed = |n: usize| format!("tomb-key-{n:08}");
    let total = TOMBSTONES_PER_PRINCIPAL + 1;
    let mut now = T0;
    for batch in (0..total).collect::<Vec<_>>().chunks(16) {
        f.clock.set(now);
        let cookie = signed_in(&f, ALICE);
        for n in batch {
            issued(&ask(&f, &cookie, &keyed(*n)));
        }
        now += FRESH_S;
    }
    f.clock.set(now);
    let cookie = signed_in(&f, ALICE);
    issued(&ask(&f, &cookie, &keyed(0)));
    refused(
        &ask(&f, &cookie, &keyed(1)),
        409,
        ProblemCode::IdempotencyConflict,
    );
    refused(
        &ask(&f, &cookie, &keyed(total - 1)),
        409,
        ProblemCode::IdempotencyConflict,
    );
}

#[test]
fn registration_and_step_up_challenges_count_under_one_bound() {
    let alice = encode_ulid(id(ALICE).0);
    let rows = enrol(opened(), &alice, PASSKEY_A, T0 - 100);
    let f = fixture(&rows, Challenges::bounded(3, 4));
    let cookie = signed_in(&f, ALICE);
    let approve = |key: &str| {
        let cookie = format!("{COOKIE_NAME}={cookie}");
        send(
            f.addr,
            "POST",
            &format!("/v1/workspaces/{WORKSPACE}/step-up/challenges"),
            &[
                ("Origin", APP),
                ("X-Mandate-Request", "1"),
                ("Cookie", cookie.as_str()),
                ("Idempotency-Key", key),
            ],
            br#"{"action":{"kind":"approve","digest":"sha256:5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e"}}"#,
        )
    };
    issued(&approve("step-key-00000001"));
    issued(&approve("step-key-00000002"));
    issued(&ask(&f, &cookie, KEY));
    refused(&ask(&f, &cookie, KEY_2), 429, ProblemCode::RateLimited);
    refused(&approve("step-key-00000003"), 429, ProblemCode::RateLimited);
    let bob = signed_in(&f, BOB);
    issued(&ask(&f, &bob, KEY));
    refused(&ask(&f, &bob, KEY_2), 429, ProblemCode::RateLimited);
    issued(&ask(&f, &cookie, KEY));
}

#[test]
fn the_session_the_csrf_mark_the_key_and_the_strict_body_come_first() {
    let rows = opened();
    let f = plain(&rows);
    let nowhere = send(f.addr, "GET", "/nowhere", &[], b"");
    let anonymous = request(&f, &[("Origin", APP), ("X-Mandate-Request", "1")], "{");
    assert_eq!(anonymous.status, 401);
    assert_eq!(anonymous.body, nowhere.body, "the one 401 (API-1)");
    let cookie = signed_in(&f, ALICE);
    let session = format!("{COOKIE_NAME}={cookie}");
    let unmarked = request(
        &f,
        &[
            ("Origin", APP),
            ("Cookie", &session),
            ("Idempotency-Key", KEY),
        ],
        "{}",
    );
    refused(&unmarked, 403, ProblemCode::Forbidden);
    let unkeyed = request(
        &f,
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", &session),
        ],
        "{}",
    );
    refused(&unkeyed, 422, ProblemCode::Invalid);
    for bad in ["{", "[]", r#"{"exclude":[]}"#] {
        refused(&ask_with(&f, &cookie, KEY, bad), 422, ProblemCode::Invalid);
    }
    issued(&ask(&f, &cookie, KEY));
    nothing_journaled(&f, &rows);
}

#[test]
fn a_reduction_only_session_neither_asks_nor_lists() {
    let rows = opened();
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    f.clock.set(T0 + 6 * 60);
    let paused = send(
        f.addr,
        "POST",
        &format!("/v1/workspaces/{WORKSPACE}/agents/agt_a/pause"),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", format!("{COOKIE_NAME}={cookie}").as_str()),
            ("Idempotency-Key", "lapse-pause-000001"),
        ],
        b"{}",
    );
    assert_ne!(paused.status, 401, "the pause takes the outage refresh");
    refused(&ask(&f, &cookie, KEY), 403, ProblemCode::ReductionOnly);
    refused(&list(&f, &cookie), 403, ProblemCode::ReductionOnly);
}

#[test]
fn a_stranger_and_another_workspace_find_nothing_and_every_role_holds_the_own_cell() {
    let rows = opened();
    let f = plain(&rows);
    let dave = signed_in(&f, DAVE);
    refused(&ask(&f, &dave, KEY), 404, ProblemCode::NotFound);
    refused(&list(&f, &dave), 404, ProblemCode::NotFound);
    let bob = signed_in(&f, BOB);
    issued(&ask(&f, &bob, KEY));
    assert_eq!(list(&f, &bob).status, 200, "a viewer lists their own");
    let other = send(
        f.addr,
        "POST",
        "/v1/workspaces/01JB3K6F0C9R2V7N4M8Q1T5W50/passkeys/registration-challenges",
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", format!("{COOKIE_NAME}={bob}").as_str()),
            ("Idempotency-Key", KEY_2),
        ],
        b"{}",
    );
    refused(&other, 404, ProblemCode::NotFound);
    nothing_journaled(&f, &rows);
}

#[test]
fn a_live_workspace_is_refused_after_the_row() {
    let mut rows = Rows { rows: Vec::new() };
    rows.push(
        &hosted_stream(),
        "live",
        "StreamOpened",
        T0 - 1_000,
        json!({"stream_type": "control"}),
    );
    let f = plain(&rows);
    let dave = signed_in(&f, DAVE);
    refused(&ask(&f, &dave, KEY), 404, ProblemCode::NotFound);
    let alice = signed_in(&f, ALICE);
    refused(&ask(&f, &alice, KEY), 409, ProblemCode::LiveUnavailable);
    nothing_journaled(&f, &rows);
}

#[test]
fn a_member_whose_records_break_their_rule_is_answered_journal_unavailable() {
    let alice = encode_ulid(id(ALICE).0);
    let bob = encode_ulid(id(BOB).0);
    let rows = enrol(opened(), &alice, PASSKEY_A, T0 - 300);
    let rows = enrol(rows, &alice, PASSKEY_A, T0 - 200);
    let rows = enrol(rows, &bob, PASSKEY_C, T0 - 100);
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    refused(&ask(&f, &cookie, KEY), 503, ProblemCode::JournalUnavailable);
    refused(&list(&f, &cookie), 503, ProblemCode::JournalUnavailable);
    let bob = signed_in(&f, BOB);
    issued(&ask(&f, &bob, KEY));
    assert_eq!(list(&f, &bob).status, 200, "another member is served");
    nothing_journaled(&f, &rows);
}

#[test]
fn a_control_stream_that_cannot_be_read_is_answered_journal_unavailable() {
    let rows = opened();
    let f = plain(&rows);
    let cookie = signed_in(&f, ALICE);
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(hosted_stream());
    refused(&ask(&f, &cookie, KEY), 503, ProblemCode::JournalUnavailable);
    refused(&list(&f, &cookie), 503, ProblemCode::JournalUnavailable);
}

#[test]
fn the_list_serves_the_members_passkeys_in_enrolment_order_dated_by_recorded_at() {
    let alice = encode_ulid(id(ALICE).0);
    let bob = encode_ulid(id(BOB).0);
    let rows = with(
        opened(),
        "CredentialEnrolled",
        enrolled(&alice, PASSKEY_B, "eddsa", Some(COOL_OFF_ENDS)),
        T0 - 500,
    );
    let rows = enrol(rows, &bob, PASSKEY_C, T0 - 400);
    let rows = enrol(rows, &alice, PASSKEY_A, T0 - 300);
    let rows = enrol(rows, &alice, "MDEyMzQ1Njc4OTo7PD0-Pw", T0 - 200);
    let rows = with(
        rows,
        "CredentialRemoved",
        removed(&alice, "MDEyMzQ1Njc4OTo7PD0-Pw"),
        T0 - 100,
    );
    let f = plain(&rows);
    f.clock.set(T0 + 10 * FRESH_S);
    let cookie = signed_in(&f, ALICE);
    let reply = list(&f, &cookie);
    assert_eq!(
        reply.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    assert_eq!(
        json_of(&member(&reply, "passkeys")),
        json!([
            {"passkey_id": PASSKEY_B, "alg": "eddsa", "enrolled_at": time(T0 - 500), "cool_off_ends": COOL_OFF_ENDS},
            {"passkey_id": PASSKEY_A, "alg": "es256", "enrolled_at": time(T0 - 300), "cool_off_ends": null},
        ])
    );
    assert_eq!(
        json_of(&member(&reply, "as_of")),
        control_watermark(&rows.rows)
    );
    let anonymous = send(f.addr, "GET", &list_path(), &[("Origin", APP)], b"");
    let nowhere = send(f.addr, "GET", "/nowhere", &[], b"");
    assert_eq!(anonymous.status, 401);
    assert_eq!(anonymous.body, nowhere.body, "the one 401 (API-1)");
}

/// One step of a random control stream: enrol or remove a passkey, by one of two members.
#[derive(Debug, Clone, Copy)]
enum Step {
    Enrol(bool, u8),
    Remove(bool, u8),
}

fn step() -> impl Strategy<Value = Step> {
    prop_oneof![
        (any::<bool>(), 0..4_u8).prop_map(|(m, k)| Step::Enrol(m, k)),
        (any::<bool>(), 0..4_u8).prop_map(|(m, k)| Step::Remove(m, k)),
    ]
}

fn passkey(k: u8) -> String {
    b64url(&[k; 16])
}

fn who(alice: bool) -> &'static str {
    if alice { ALICE } else { BOB }
}

/// The oracle, kept apart from `mandate-passkey`: rule 251 by each id's history, and each
/// member's passkeys, enrolled and not removed, in enrolment order with their row's instant; a
/// member any breaking record names is `None`.
fn oracle(steps: &[(Step, i64)]) -> BTreeMap<&'static str, Option<Vec<(String, i64)>>> {
    let mut history: BTreeMap<u8, Vec<(bool, &str)>> = BTreeMap::new();
    let mut held: Vec<(&str, String, i64)> = Vec::new();
    let mut broken: BTreeSet<&str> = BTreeSet::new();
    for (step, recorded) in steps {
        let (enrol, alice, k) = match *step {
            Step::Enrol(alice, k) => (true, alice, k),
            Step::Remove(alice, k) => (false, alice, k),
        };
        let member = who(alice);
        let events = history.entry(k).or_default();
        events.push((enrol, member));
        let keeps = match events.as_slice() {
            [(true, _)] => true,
            [(true, first), (false, by)] => first == by,
            _ => false,
        };
        if !keeps {
            broken.insert(member);
            if let Some((true, first)) = events.first() {
                broken.insert(first);
            }
        } else if enrol {
            held.push((member, passkey(k), *recorded));
        } else {
            held.retain(|(_, id, _)| *id != passkey(k));
        }
    }
    [ALICE, BOB]
        .into_iter()
        .map(|m| {
            let mine = (!broken.contains(m)).then(|| {
                held.iter()
                    .filter(|(owner, _, _)| *owner == m)
                    .map(|(_, id, at)| (id.clone(), *at))
                    .collect()
            });
            (m, mine)
        })
        .collect()
}

/// Over random streams of enrolments and removals by two members, the list, `exclude`, and
/// `step_up_needed` all read one view of each member's passkeys, the oracle's, and a member the
/// oracle finds broken is answered `journal_unavailable` by both routes (DEC-2124). It drives
/// proptest's runner itself, as a plain test, so a failure reports the server's own panic.
#[test]
fn both_routes_read_the_oracles_view_of_each_members_passkeys() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 48,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    let checked = runner.run(&prop::collection::vec(step(), 0..10), |steps| {
        let timed: Vec<(Step, i64)> = steps
            .iter()
            .zip(1_i64..)
            .map(|(s, n)| (*s, T0 - 1_000 + n))
            .collect();
        let mut rows = opened();
        for (step, recorded) in &timed {
            let (kind, payload) = match *step {
                Step::Enrol(alice, k) => (
                    "CredentialEnrolled",
                    enrolled(&encode_ulid(id(who(alice)).0), &passkey(k), "es256", None),
                ),
                Step::Remove(alice, k) => (
                    "CredentialRemoved",
                    removed(&encode_ulid(id(who(alice)).0), &passkey(k)),
                ),
            };
            rows = with(rows, kind, payload, *recorded);
        }
        let f = plain(&rows);
        for (m, view) in oracle(&timed) {
            let cookie = signed_in(&f, m);
            let listed = list(&f, &cookie);
            let asked = ask(&f, &cookie, KEY);
            match view {
                None => {
                    prop_assert_eq!(listed.status, 503);
                    prop_assert_eq!(problem(&listed).code, ProblemCode::JournalUnavailable);
                    prop_assert_eq!(asked.status, 503);
                }
                Some(mine) => {
                    prop_assert_eq!(
                        listed.status,
                        200,
                        "{:?}",
                        String::from_utf8_lossy(&listed.body)
                    );
                    let want: Vec<Json> = mine
                        .iter()
                        .map(|(id, at)| {
                            json!({
                                "passkey_id": id, "alg": "es256",
                                "enrolled_at": time(*at), "cool_off_ends": null,
                            })
                        })
                        .collect();
                    prop_assert_eq!(json_of(&member(&listed, "passkeys")), Json::Array(want));
                    prop_assert_eq!(asked.status, 201);
                    let ids: Vec<String> = mine.iter().map(|(id, _)| id.clone()).collect();
                    prop_assert_eq!(exclude(&asked), ids);
                    prop_assert_eq!(
                        member(&asked, "step_up_needed"),
                        Value::Bool(!mine.is_empty())
                    );
                }
            }
        }
        Ok(())
    });
    if let Err(failure) = checked {
        panic!("the passkey routes disagree with the oracle: {failure}");
    }
}
