//! The step-up challenge route over HTTP (workspace API spec §3.6, identity spec §7.2 steps 1 and
//! 2, API-17, DEC-2103): who obtains a challenge, what it binds, how a retry answers, and the
//! store's bounds. Expected ids, records, and challenge bytes are computed here from the specs:
//! §3.4's derivation, §7.2's canonical record, SHA-256 by `ring`, and the tests' own base64url.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, WORKSPACE, b64url, hosted, send, serve};
use mandate_api::envelope::StepUpActionKind as WireKind;
use mandate_api::problem::{Effect, Problem, ProblemCode, Violation};
use mandate_api::wire::decode;
use mandate_api_server::challenges::{Challenges, HELD, OPERATION, PER_PRINCIPAL};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role, StepUpActionKind};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const T0_TEXT: &str = "2026-10-03T04:00:00.000000000Z";
const T0_PLUS_300_TEXT: &str = "2026-10-03T04:05:00.000000000Z";
const KEY: &str = "k1Z-9_aaaaaaaaaa";
const KEY_2: &str = "k2Z-9_bbbbbbbbbb";
/// An operator and approver.
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
/// A viewer.
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
/// An approver only.
const CAROL: &str = "01JB3K6F0C9R2V7N4M8Q1T5W42";
/// No membership.
const DAVE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W43";
/// A workspace admin, operator, and approver: every workspace row with **S**.
const ERIN: &str = "01JB3K6F0C9R2V7N4M8Q1T5W44";
const ENVELOPE: &str = include_str!("../../../schemas/workspace-api/envelope.schema.json");
const CONTENT_HASH: &str =
    "sha256:5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e5e";
const OTHER_HASH: &str = "sha256:6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f6f";

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    sessions: Arc<Sessions>,
    clock: Arc<common::ManualClock>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture(environment: &str, challenges: Challenges) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-stepup-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x46_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    let records = vec![
        activated(1, id(ALICE), &[Role::Operator, Role::Approver]),
        activated(2, id(BOB), &[Role::Viewer]),
        activated(3, id(CAROL), &[Role::Approver]),
        activated(
            4,
            id(ERIN),
            &[Role::WorkspaceAdmin, Role::Operator, Role::Approver],
        ),
    ];
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let clock = common::ManualClock::at(T0);
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
        folds: Arc::default(),
    };
    let f = Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
        clock,
    };
    seed(&f, environment);
    f
}

fn paper() -> Fixture {
    fixture("paper", Challenges::default())
}

fn id(text: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(text).expect("a ULID")
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
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

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

/// The control stream's one row, in `environment`.
fn seed(f: &Fixture, environment: &str) {
    let text = format!(
        r#"{{"causation_id":null,"environment":"{environment}","event_id":"01JBH3BTZ8K2M4N6P8R0S20001","event_type":"StreamOpened","payload":{{}},"stream_id":"{}"}}"#,
        ctl()
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    let row = StoredEvent {
        stream_id: ctl(),
        seq: 1,
        event_id: "01JBH3BTZ8K2M4N6P8R0S20001".to_owned(),
        event_type: "StreamOpened".to_owned(),
        schema_version: 1,
        environment: environment.to_owned(),
        recorded_at: "2026-10-03T03:00:00.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    };
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    streams.insert(ctl(), vec![row]);
}

fn signed_in(f: &Fixture, who: &str) -> String {
    f.sessions
        .open(id(who), "sha256:aaaa".to_owned(), None, at(f.clock.secs()))
        .expect("opened")
        .value()
        .to_owned()
}

fn challenges_path() -> String {
    format!("/v1/workspaces/{WORKSPACE}/step-up/challenges")
}

fn body_for(kind: &str, digest: &str) -> String {
    format!(r#"{{"action":{{"kind":"{kind}","digest":"{digest}"}}}}"#)
}

fn request(f: &Fixture, headers: &[(&str, &str)], body: &str) -> Reply {
    send(f.addr, "POST", &challenges_path(), headers, body.as_bytes())
}

fn ask(f: &Fixture, cookie: &str, key: &str, body: &str) -> Reply {
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

fn approve(f: &Fixture, cookie: &str, key: &str) -> Reply {
    ask(f, cookie, key, &body_for("approve", CONTENT_HASH))
}

/// §3.4's id for `key` under `principal`, the route's operation, and the hosted workspace.
fn derived(principal: &str, key: &str) -> String {
    derived_in(WORKSPACE, principal, OPERATION, key)
}

/// §3.4's derivation written out from the spec, independent of `mandate_api::idempotency`: the
/// canonical object of four ASCII members in byte order, its SHA-256 by `ring`, and the first 128
/// bits as 26 Crockford base-32 digits after two zero bits.
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

/// Identity spec §7.2 step 2: SHA-256 of the canonical record, its seven members in byte order,
/// written out as text, in the tests' own base64url.
fn signed_challenge(
    challenge_id: &str,
    principal: &str,
    (kind, digest): (&str, &str),
    issued_at: &str,
    expires_at: &str,
) -> String {
    let record = format!(
        r#"{{"action_digest":"{digest}","action_kind":"{kind}","challenge_id":"{challenge_id}","expires_at":"{expires_at}","issued_at":"{issued_at}","principal_id":"{principal}","workspace_id":"{WORKSPACE}"}}"#
    );
    let sha = ring::digest::digest(&ring::digest::SHA256, record.as_bytes());
    b64url(sha.as_ref())
}

fn body(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn member(reply: &Reply, name: &str) -> String {
    match body(reply) {
        Value::Object(members) => members
            .iter()
            .find(|(key, _)| key.as_str() == name)
            .map(|(_, value)| String::from_utf8(to_canonical(value)).expect("UTF-8"))
            .unwrap_or_else(|| panic!("no {name}")),
        other => panic!("not an object: {other:?}"),
    }
}

fn text(reply: &Reply, name: &str) -> String {
    member(reply, name).trim_matches('"').to_owned()
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
}

/// The control stream holds only its seeded row: a challenge is never journaled when issued
/// (DEC-2103 item 2).
fn nothing_journaled(f: &Fixture) {
    let rows = f.journal.rows_of(&ctl());
    assert_eq!(rows.len(), 1, "{:?}", rows.last().map(|r| &r.event_type));
}

#[test]
fn an_approver_obtains_the_challenge_bound_to_the_approval_it_names() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    let reply = approve(&f, &cookie, KEY);
    issued(&reply);
    let challenge_id = derived(ALICE, KEY);
    assert_eq!(text(&reply, "challenge_id"), challenge_id);
    assert_eq!(text(&reply, "issued_at"), T0_TEXT);
    assert_eq!(text(&reply, "expires_at"), T0_PLUS_300_TEXT);
    assert_eq!(
        member(&reply, "action"),
        format!(r#"{{"digest":"{CONTENT_HASH}","kind":"approve"}}"#)
    );
    let expected = signed_challenge(
        &challenge_id,
        ALICE,
        ("approve", CONTENT_HASH),
        T0_TEXT,
        T0_PLUS_300_TEXT,
    );
    assert_eq!(text(&reply, "webauthn_challenge"), expected);
    assert_eq!(text(&reply, "api_version"), "v1");
    assert_eq!(member(&reply, "as_of"), "[]");
    let Value::Object(members) = body(&reply) else {
        panic!("an object")
    };
    let names: BTreeSet<&str> = members.keys().map(|key| key.as_str()).collect();
    let schema: BTreeSet<&str> = [
        "action",
        "api_version",
        "as_of",
        "build",
        "challenge_id",
        "expires_at",
        "issued_at",
        "served_at",
        "webauthn_challenge",
    ]
    .into();
    assert_eq!(names, schema, "no workspace or principal is echoed");
    nothing_journaled(&f);
}

#[test]
fn each_kind_is_bound_into_the_record_the_authenticator_signs() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    for (key, (kind, digest)) in [
        ("k3Z-9_cccccccccc", ("resume", OTHER_HASH)),
        ("k4Z-9_dddddddddd", ("deploy", CONTENT_HASH)),
        ("k5Z-9_eeeeeeeeee", ("owner_exit", OTHER_HASH)),
    ] {
        let reply = ask(&f, &cookie, key, &body_for(kind, digest));
        issued(&reply);
        let challenge_id = derived(ALICE, key);
        let expected = signed_challenge(
            &challenge_id,
            ALICE,
            (kind, digest),
            T0_TEXT,
            T0_PLUS_300_TEXT,
        );
        assert_eq!(text(&reply, "webauthn_challenge"), expected, "{kind}");
    }
}

#[test]
fn a_principal_who_may_not_act_cannot_obtain_a_challenge() {
    let f = paper();
    let viewer = signed_in(&f, BOB);
    for kind in ["approve", "resume", "deploy", "owner_exit"] {
        let reply = ask(&f, &viewer, KEY, &body_for(kind, CONTENT_HASH));
        refused(&reply, 403, ProblemCode::Forbidden);
    }
    let approver = signed_in(&f, CAROL);
    issued(&approve(&f, &approver, KEY));
    for kind in ["resume", "deploy", "owner_exit"] {
        let reply = ask(&f, &approver, KEY_2, &body_for(kind, CONTENT_HASH));
        refused(&reply, 403, ProblemCode::Forbidden);
    }
    let stranger = signed_in(&f, DAVE);
    refused(&approve(&f, &stranger, KEY), 404, ProblemCode::NotFound);
    nothing_journaled(&f);
}

/// API-17: the schema's kinds are exactly `mandate-identity`'s, so the two cannot drift, and every
/// one but the two `own` rows' issues a challenge to a principal holding its row, the record
/// binding that kind.
#[test]
fn every_kind_the_schema_names_issues_a_challenge_bound_to_it() {
    let schema: serde_json::Value = serde_json::from_str(ENVELOPE).expect("JSON");
    let listed: BTreeSet<String> = schema["$defs"]["StepUpAction"]["properties"]["kind"]["enum"]
        .as_array()
        .expect("an enum")
        .iter()
        .map(|kind| kind.as_str().expect("a name").to_owned())
        .collect();
    let codes: BTreeSet<String> = StepUpActionKind::ALL
        .iter()
        .map(|kind| kind.code().to_owned())
        .collect();
    assert_eq!(
        listed, codes,
        "the schema's kinds and StepUpActionKind::ALL"
    );
    let f = paper();
    let cookie = signed_in(&f, ERIN);
    let own = ["credential_enrol", "notification_address"];
    for (n, kind) in listed
        .iter()
        .filter(|kind| !own.contains(&kind.as_str()))
        .enumerate()
    {
        let key = keyed(100 + n);
        let reply = ask(&f, &cookie, &key, &body_for(kind, CONTENT_HASH));
        issued(&reply);
        let expected = signed_challenge(
            &derived(ERIN, &key),
            ERIN,
            (kind, CONTENT_HASH),
            T0_TEXT,
            T0_PLUS_300_TEXT,
        );
        assert_eq!(text(&reply, "webauthn_challenge"), expected, "{kind}");
    }
}

/// The wire's kind of the same name; the match is exhaustive, so a kind `mandate-identity` adds
/// stops this file compiling until the wire has it too.
fn wire_kind(kind: StepUpActionKind) -> WireKind {
    match kind {
        StepUpActionKind::ConfirmVersion => WireKind::ConfirmVersion,
        StepUpActionKind::Deploy => WireKind::Deploy,
        StepUpActionKind::Approve => WireKind::Approve,
        StepUpActionKind::Connection => WireKind::Connection,
        StepUpActionKind::Resume => WireKind::Resume,
        StepUpActionKind::Stop => WireKind::Stop,
        StepUpActionKind::Acknowledge => WireKind::Acknowledge,
        StepUpActionKind::OwnerExit => WireKind::OwnerExit,
        StepUpActionKind::KillSwitchPrivilege => WireKind::KillSwitchPrivilege,
        StepUpActionKind::Disclosure => WireKind::Disclosure,
        StepUpActionKind::PolicyLoosen => WireKind::PolicyLoosen,
        StepUpActionKind::MemberInvite => WireKind::MemberInvite,
        StepUpActionKind::RoleGrant => WireKind::RoleGrant,
        StepUpActionKind::ClientConnect => WireKind::ClientConnect,
        StepUpActionKind::CredentialEnrol => WireKind::CredentialEnrol,
        StepUpActionKind::NotificationAddress => WireKind::NotificationAddress,
        StepUpActionKind::BreakGlassApprove => WireKind::BreakGlassApprove,
        StepUpActionKind::LiftHold => WireKind::LiftHold,
    }
}

/// `mandate-identity`'s kind of the same name; exhaustive over the wire's kinds, as
/// [`wire_kind`] is over `mandate-identity`'s.
fn identity_kind(kind: WireKind) -> StepUpActionKind {
    match kind {
        WireKind::ConfirmVersion => StepUpActionKind::ConfirmVersion,
        WireKind::Deploy => StepUpActionKind::Deploy,
        WireKind::Approve => StepUpActionKind::Approve,
        WireKind::Connection => StepUpActionKind::Connection,
        WireKind::Resume => StepUpActionKind::Resume,
        WireKind::Stop => StepUpActionKind::Stop,
        WireKind::Acknowledge => StepUpActionKind::Acknowledge,
        WireKind::OwnerExit => StepUpActionKind::OwnerExit,
        WireKind::KillSwitchPrivilege => StepUpActionKind::KillSwitchPrivilege,
        WireKind::Disclosure => StepUpActionKind::Disclosure,
        WireKind::PolicyLoosen => StepUpActionKind::PolicyLoosen,
        WireKind::MemberInvite => StepUpActionKind::MemberInvite,
        WireKind::RoleGrant => StepUpActionKind::RoleGrant,
        WireKind::ClientConnect => StepUpActionKind::ClientConnect,
        WireKind::CredentialEnrol => StepUpActionKind::CredentialEnrol,
        WireKind::NotificationAddress => StepUpActionKind::NotificationAddress,
        WireKind::BreakGlassApprove => StepUpActionKind::BreakGlassApprove,
        WireKind::LiftHold => StepUpActionKind::LiftHold,
    }
}

/// API-17: `mandate_api::envelope::StepUpActionKind` and `mandate_identity::StepUpActionKind` have
/// the same variants under the same wire names, so the route's lookup by name finds every kind.
#[test]
fn the_wire_and_identity_kinds_are_the_same_variants() {
    let mut seen = BTreeSet::new();
    for kind in StepUpActionKind::ALL {
        let wire = wire_kind(kind);
        assert_eq!(identity_kind(wire), kind, "{kind:?} round-trips");
        assert_eq!(
            serde_json::to_value(wire).expect("a kind serializes"),
            serde_json::Value::String(kind.code().to_owned()),
            "{kind:?}'s wire name"
        );
        assert!(seen.insert(kind.code()), "{kind:?} is listed once");
    }
    assert_eq!(seen.len(), 18, "ALL holds every kind");
}

/// A challenge is never kept by a cache between the server and the browser (DEC-2103 item 3).
#[test]
fn a_challenge_is_answered_no_store() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    let reply = approve(&f, &cookie, KEY);
    issued(&reply);
    assert_eq!(reply.header("cache-control"), Some("no-store"));
    let retried = approve(&f, &cookie, KEY);
    assert_eq!(retried.header("cache-control"), Some("no-store"));
}

/// The two `own` rows are authorized at the principal's scope, which their own routes will use;
/// until then the route answers them the one 404 and issues nothing (DEC-2103 item 1).
#[test]
fn the_kinds_of_own_rows_wait_for_their_routes() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    for kind in ["credential_enrol", "notification_address"] {
        let reply = ask(&f, &cookie, KEY, &body_for(kind, CONTENT_HASH));
        refused(&reply, 404, ProblemCode::NotFound);
    }
    issued(&approve(&f, &cookie, KEY));
}

#[test]
fn a_reduction_only_session_cannot_obtain_a_challenge() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    f.clock.set(f.clock.secs() + 6 * 60);
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
    refused(&approve(&f, &cookie, KEY), 403, ProblemCode::ReductionOnly);
}

#[test]
fn the_session_the_csrf_mark_and_the_key_come_before_the_body() {
    let f = paper();
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
        "{",
    );
    refused(&unmarked, 403, ProblemCode::Forbidden);
    let elsewhere = request(
        &f,
        &[
            ("Origin", "https://evil.test"),
            ("X-Mandate-Request", "1"),
            ("Cookie", &session),
            ("Idempotency-Key", KEY),
        ],
        &body_for("approve", CONTENT_HASH),
    );
    refused(&elsewhere, 403, ProblemCode::Forbidden);
    let unkeyed = request(
        &f,
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", &session),
        ],
        &body_for("approve", CONTENT_HASH),
    );
    refused(&unkeyed, 422, ProblemCode::Invalid);
    for bad in [
        "short",
        "has space in it!!",
        "k1Z-9_aaaaaaaaaa,k1Z-9_aaaaaaaaaa",
    ] {
        refused(
            &ask(&f, &cookie, bad, &body_for("approve", CONTENT_HASH)),
            422,
            ProblemCode::Invalid,
        );
    }
    nothing_journaled(&f);
}

fn schema_paths(reply: &Reply) -> Vec<String> {
    refused(reply, 422, ProblemCode::Invalid);
    problem(reply)
        .violations
        .into_iter()
        .filter_map(|v| match v {
            Violation::Schema { path, .. } => Some(path),
            _ => None,
        })
        .collect()
}

#[test]
fn a_body_the_schema_refuses_is_invalid_at_its_member() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    let cases = [
        (body_for("sso_configuration", CONTENT_HASH), "/action/kind"),
        (body_for("approve", "sha256:5E"), "/action/digest"),
        (
            format!(
                r#"{{"action":{{"kind":"approve","digest":"{CONTENT_HASH}"}},"workspace_id":"{WORKSPACE}"}}"#
            ),
            "/workspace_id",
        ),
        (
            format!(
                r#"{{"action":{{"kind":"approve","digest":"{CONTENT_HASH}","principal_id":"{BOB}"}}}}"#
            ),
            "/action/principal_id",
        ),
        (r#"{"action":{"kind":"approve"}}"#.to_owned(), "/action"),
    ];
    for (bad, path) in cases {
        let paths = schema_paths(&ask(&f, &cookie, KEY, &bad));
        assert!(paths.iter().any(|p| p == path), "{bad}: {paths:?}");
    }
    nothing_journaled(&f);
}

#[test]
fn a_retry_answers_the_same_challenge_and_another_action_under_the_key_conflicts() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    let first = approve(&f, &cookie, KEY);
    issued(&first);
    f.clock.set(T0 + 10);
    let again = approve(&f, &cookie, KEY);
    issued(&again);
    for name in [
        "challenge_id",
        "webauthn_challenge",
        "issued_at",
        "expires_at",
    ] {
        assert_eq!(member(&again, name), member(&first, name), "{name}");
    }
    let other = ask(&f, &cookie, KEY, &body_for("approve", OTHER_HASH));
    refused(&other, 409, ProblemCode::IdempotencyConflict);
    let kind = ask(&f, &cookie, KEY, &body_for("resume", CONTENT_HASH));
    refused(&kind, 409, ProblemCode::IdempotencyConflict);
    let fresh = ask(&f, &cookie, KEY_2, &body_for("approve", OTHER_HASH));
    issued(&fresh);
    assert_ne!(text(&fresh, "challenge_id"), text(&first, "challenge_id"));
    let carol = signed_in(&f, CAROL);
    let theirs = approve(&f, &carol, KEY);
    issued(&theirs);
    assert_eq!(text(&theirs, "challenge_id"), derived(CAROL, KEY));
}

#[test]
fn an_expired_challenge_is_dropped_and_its_key_issues_a_new_one() {
    let f = paper();
    let cookie = signed_in(&f, ALICE);
    let first = approve(&f, &cookie, KEY);
    issued(&first);
    f.clock.set(T0 + 299);
    let held = ask(&f, &cookie, KEY, &body_for("approve", OTHER_HASH));
    refused(&held, 409, ProblemCode::IdempotencyConflict);
    f.clock.set(T0 + 300);
    let cookie = signed_in(&f, ALICE);
    let later = ask(&f, &cookie, KEY, &body_for("approve", OTHER_HASH));
    issued(&later);
    assert_eq!(text(&later, "challenge_id"), derived(ALICE, KEY));
    assert_eq!(text(&later, "issued_at"), "2026-10-03T04:05:00.000000000Z");
}

#[test]
fn a_live_workspace_is_refused_before_any_challenge_is_issued() {
    let f = fixture("live", Challenges::default());
    let cookie = signed_in(&f, ALICE);
    refused(
        &approve(&f, &cookie, KEY),
        409,
        ProblemCode::LiveUnavailable,
    );
    let viewer = signed_in(&f, BOB);
    refused(&approve(&f, &viewer, KEY), 403, ProblemCode::Forbidden);
}

/// A control stream opened in paper with a later `live` row is not a paper workspace: every row,
/// not the first, decides it (DEC-1143 item 2).
#[test]
fn a_later_live_row_makes_the_workspace_live() {
    let f = paper();
    {
        let mut streams = f.journal.streams.lock().expect("unpoisoned");
        let rows = streams.get_mut(&ctl()).expect("the seeded stream");
        let mut later = rows[0].clone();
        later.seq = 2;
        later.environment = "live".to_owned();
        rows.push(later);
    }
    let cookie = signed_in(&f, ALICE);
    refused(
        &approve(&f, &cookie, KEY),
        409,
        ProblemCode::LiveUnavailable,
    );
}

/// A workspace the server does not host is the one 404, whoever asks, and issues nothing
/// (workspace API spec §3.3).
#[test]
fn another_workspaces_path_is_not_found() {
    let f = paper();
    let cookie = format!("{COOKIE_NAME}={}", signed_in(&f, ERIN));
    let path = "/v1/workspaces/01JB3K6F0C9R2V7N4M8Q1T5W50/step-up/challenges";
    let reply = send(
        f.addr,
        "POST",
        path,
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", cookie.as_str()),
            ("Idempotency-Key", KEY),
        ],
        body_for("approve", CONTENT_HASH).as_bytes(),
    );
    refused(&reply, 404, ProblemCode::NotFound);
    nothing_journaled(&f);
}

#[test]
fn the_deployment_holds_thirty_two_a_principal_and_four_thousand_ninety_six_in_all() {
    assert_eq!((PER_PRINCIPAL, HELD), (32, 4_096));
}

fn keyed(n: usize) -> String {
    format!("bound-key-{n:08}")
}

#[test]
fn past_a_principals_bound_a_request_is_rate_limited_until_one_expires() {
    let f = fixture("paper", Challenges::bounded(3, 10));
    let alice = signed_in(&f, ALICE);
    for n in 0..3 {
        issued(&approve(&f, &alice, &keyed(n)));
    }
    refused(
        &approve(&f, &alice, &keyed(3)),
        429,
        ProblemCode::RateLimited,
    );
    issued(&approve(&f, &alice, &keyed(0)));
    let carol = signed_in(&f, CAROL);
    issued(&approve(&f, &carol, &keyed(3)));
    f.clock.set(T0 + 300);
    let alice = signed_in(&f, ALICE);
    for n in 4..7 {
        issued(&approve(&f, &alice, &keyed(n)));
    }
}

#[test]
fn past_the_stores_bound_a_request_is_rate_limited_for_everyone() {
    let f = fixture("paper", Challenges::bounded(3, 4));
    let alice = signed_in(&f, ALICE);
    let carol = signed_in(&f, CAROL);
    for n in 0..3 {
        issued(&approve(&f, &alice, &keyed(n)));
    }
    issued(&approve(&f, &carol, &keyed(3)));
    refused(
        &approve(&f, &carol, &keyed(4)),
        429,
        ProblemCode::RateLimited,
    );
    refused(
        &approve(&f, &alice, &keyed(5)),
        429,
        ProblemCode::RateLimited,
    );
    issued(&approve(&f, &carol, &keyed(3)));
}

/// A seeded walk over requests by two principals, each a key from a small pool, an action from
/// two, and a clock that moves 0 to 120 s, against a model kept apart from the store: every `201`
/// binds the requester and the requested action and is the model's answer, and the store never
/// holds more than its bounds, which the model counts from the answers it saw.
#[test]
fn the_store_answers_as_an_independent_model_of_held_challenges_does() {
    for seed in [1_u64, 7, 2026, 0x5eed] {
        let f = fixture("paper", Challenges::bounded(4, 6));
        let mut state = seed;
        let mut next = |bound: u64| {
            state = state
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            (state >> 33) % bound
        };
        let mut model: Vec<(String, String, String, i64)> = Vec::new();
        let mut now = T0;
        for step in 0..120 {
            now += i64::try_from(next(121)).expect("small");
            f.clock.set(now);
            let who = if next(2) == 0 { ALICE } else { CAROL };
            let key = keyed(usize::try_from(next(5)).expect("small"));
            let hash = if next(2) == 0 {
                CONTENT_HASH
            } else {
                OTHER_HASH
            };
            model.retain(|(_, _, _, issued)| now < issued + 300);
            let cookie = signed_in(&f, who);
            let reply = approve_with(&f, &cookie, &key, hash);
            let id = derived(who, &key);
            let found = model.iter().find(|(held, _, _, _)| *held == id).cloned();
            let theirs = model.iter().filter(|(_, by, _, _)| by == who).count();
            let context = format!("seed {seed} step {step}: {who} {key} {hash}");
            match found {
                Some((_, _, held_hash, issued_at)) if held_hash == hash => {
                    issued(&reply);
                    let issued_text = UtcNanos::from_parts(issued_at, 0).expect("t").to_string();
                    assert_eq!(text(&reply, "issued_at"), issued_text, "{context}");
                }
                Some(_) => refused(&reply, 409, ProblemCode::IdempotencyConflict),
                None if theirs >= 4 || model.len() >= 6 => {
                    refused(&reply, 429, ProblemCode::RateLimited)
                }
                None => {
                    issued(&reply);
                    model.push((id.clone(), who.to_owned(), hash.to_owned(), now));
                    let issued_text = UtcNanos::from_parts(now, 0).expect("t").to_string();
                    assert_eq!(text(&reply, "issued_at"), issued_text, "{context}");
                }
            }
            if reply.status == 201 {
                assert_eq!(text(&reply, "challenge_id"), id, "{context}");
                assert_eq!(
                    member(&reply, "action"),
                    format!(r#"{{"digest":"{hash}","kind":"approve"}}"#),
                    "{context}"
                );
            }
        }
        nothing_journaled(&f);
    }
}

fn approve_with(f: &Fixture, cookie: &str, key: &str, hash: &str) -> Reply {
    ask(f, cookie, key, &body_for("approve", hash))
}
