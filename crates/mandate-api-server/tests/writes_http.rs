//! Pause and the kill switch over HTTP (§3.3, §3.4, §5, §5.4, §3.11 item 10, API-4,
//! API-7; DEC-1007): checked in §3.3's order, recorded on the control stream before the `202`,
//! replayed by key, and never gated on drain.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, Scripted, WORKSPACE, hosted, send, serve};
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::problem::{Effect, Problem, ProblemCode};
use mandate_api::responses::CommandAccepted;
use mandate_api::wire::{Id, decode};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{LookupFailed, MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_identity_seal::Seal;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const KEY: &str = "k1Z-9_aaaaaaaaaa";
/// The largest body a command route reads, 64 KiB.
const KIB_64: usize = 65_536;
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";

type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    records: Records,
    sessions: Arc<Sessions>,
    clock: Arc<common::ManualClock>,
    drain: Arc<Drain>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture() -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-writes-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x45_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    let records: Records = Arc::new(Mutex::new(Ok(vec![
        activated(1, alice(), &[Role::Operator]),
        activated(2, bob(), &[Role::Viewer]),
    ])));
    let parsed = Arc::clone(&records);
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let clock = common::ManualClock::at(T0);
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
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| parsed.lock().expect("unpoisoned").clone()),
        hosted: hosted(),
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
    };
    let drain = Drain::new();
    let f = Fixture {
        addr: serve(http::api(app, Arc::clone(&drain))),
        journal,
        records,
        sessions,
        clock,
        drain,
    };
    seed(&f);
    f
}

fn alice() -> PrincipalId {
    PrincipalId::from_ulid_text(ALICE).expect("a ULID")
}

fn bob() -> PrincipalId {
    PrincipalId::from_ulid_text(BOB).expect("a ULID")
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

fn signed_in(f: &Fixture, who: PrincipalId) -> String {
    f.sessions
        .open(who, "sha256:aaaa".to_owned(), None, at(f.clock.secs()))
        .expect("opened")
        .value()
        .to_owned()
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn row(seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{}"}}"#,
        ctl()
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: ctl(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: format!("2026-10-17T10:00:{seq:02}.000000000Z"),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// A paper workspace with the agent `agt_a` and the connection `conn_1`.
fn seed(f: &Fixture) {
    f.journal.streams.lock().expect("unpoisoned").insert(
        ctl(),
        vec![
            row(1, "StreamOpened", "{}"),
            row(
                2,
                "AgentDeployed",
                r#"{"agent_id":"agt_a","mandate_version":"sha256:01"}"#,
            ),
            row(3, "ConnectionRequested", r#"{"connection_id":"conn_1"}"#),
        ],
    );
}

fn ws(path: &str) -> String {
    format!("/v1/workspaces/{WORKSPACE}{path}")
}

/// A browser's command: the session cookie, the app origin, the CSRF header, and `key`.
fn post(f: &Fixture, path: &str, cookie: &str, key: Option<&str>, body: &[u8]) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    let mut headers = vec![
        ("Origin", APP),
        ("X-Mandate-Request", "1"),
        ("Cookie", cookie.as_str()),
        ("Content-Type", "application/json"),
    ];
    if let Some(key) = key {
        headers.push(("Idempotency-Key", key));
    }
    send(f.addr, "POST", path, &headers, body)
}

/// §3.4's id for `operation` under `key`, computed by `mandate-api`.
fn derived(operation: &str, principal: &str, key: &str) -> String {
    let workspace = Id::try_from(WORKSPACE.to_owned()).expect("an id");
    let principal = Id::try_from(principal.to_owned()).expect("an id");
    let key = IdempotencyKey::parse(key).expect("a key");
    let id = event_id(&Derivation {
        workspace: &workspace,
        principal: &principal,
        operation,
        key: &key,
        position: None,
    })
    .expect("an id");
    json_text(&mandate_api::wire::encode(&id).expect("JSON"))
}

fn json_text(bytes: &[u8]) -> String {
    parse(bytes)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .expect("a JSON string")
}

fn accepted(reply: &Reply) -> CommandAccepted {
    assert_eq!(
        reply.status,
        202,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("Content-Type"), Some("application/json"));
    decode::<CommandAccepted>(&reply.body).expect("a CommandAccepted")
}

fn body(reply: &Reply) -> Value {
    parse(&reply.body).expect("a JSON body")
}

fn problem(reply: &Reply) -> Problem {
    assert_eq!(reply.header("Content-Type"), Some(http::PROBLEM_JSON));
    decode::<Problem>(&reply.body).expect("a problem")
}

fn code(reply: &Reply) -> (u16, ProblemCode, Effect) {
    let problem = problem(reply);
    assert_eq!(problem.status, reply.status);
    (reply.status, problem.code, problem.effect)
}

/// The control stream's owner commands, as their parsed bodies.
fn commands(f: &Fixture) -> Vec<Value> {
    f.journal
        .rows_of(&ctl())
        .iter()
        .filter(|row| row.event_type == "OwnerCommandIssued")
        .map(|row| parse(&row.body).expect("JSON"))
        .collect()
}

fn member<'v>(value: &'v Value, path: &[&str]) -> &'v Value {
    path.iter()
        .try_fold(value, |v, k| v.get(k))
        .unwrap_or_else(|| panic!("{path:?}"))
}

fn text(value: &Value, path: &[&str]) -> String {
    member(value, path)
        .as_str()
        .unwrap_or_else(|| panic!("{path:?}"))
        .to_owned()
}

#[test]
fn a_pause_is_recorded_on_the_control_stream_before_its_202() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let reply = post(&f, &ws("/agents/agt_a/pause"), &cookie, Some(KEY), b"{}");
    let answer = body(&reply);
    let accepted = accepted(&reply);
    let id = derived("pause", ALICE, KEY);
    assert_eq!(text(&answer, &["command_id"]), id);
    assert_eq!(text(&answer, &["phase"]), "recorded");
    assert_eq!(answer.get("step_up_status"), None);
    assert_eq!(answer.get("dropped"), None);
    let rows = f.journal.rows_of(&ctl());
    let last = rows.last().expect("a row");
    assert_eq!((last.seq, last.event_id.as_str()), (4, id.as_str()));
    let as_of = member(&answer, &["as_of"]).as_array().expect("as_of");
    assert_eq!(as_of.len(), 1);
    assert_eq!(text(&as_of[0], &["stream_id"]), ctl());
    assert_eq!(member(&as_of[0], &["seq"]).as_int(), Some(4));
    assert_eq!(text(&as_of[0], &["hash"]), format!("sha256:{}", last.hash));
    let event = parse(&last.body).expect("JSON");
    assert_eq!(text(&event, &["event_type"]), "OwnerCommandIssued");
    assert_eq!(text(&event, &["environment"]), "paper");
    assert_eq!(text(&event, &["actor", "kind"]), "user");
    assert_eq!(text(&event, &["actor", "id"]), ALICE);
    let payload = parse(
        format!(
            r#"{{"agent":"agt_a","command":"pause","scope":"agent","subject":"agt_a","release":null,"warning_shown":null,"bid":null,"bid_size":null,"floor":null,"user":"{ALICE}","submitted_at":{T0},"step_up":null}}"#
        )
        .as_bytes(),
    )
    .expect("JSON");
    assert_eq!(member(&event, &["payload"]), &payload);
    let _ = accepted;
}

#[test]
fn a_kill_switch_names_its_scope() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let scopes = [
        (
            r#"{"kind":"workspace","id":null}"#,
            "workspace",
            WORKSPACE,
            Value::Null,
        ),
        (
            r#"{"kind":"agent","id":"agt_a"}"#,
            "agent",
            "agt_a",
            Value::Str("agt_a".into()),
        ),
        (
            r#"{"kind":"connection","id":"conn_1"}"#,
            "connection",
            "conn_1",
            Value::Null,
        ),
    ];
    for (n, (scope, kind, subject, _)) in scopes.iter().enumerate() {
        let key = format!("kill-key-000000000{n}");
        let reply = post(
            &f,
            &ws("/kill-switch"),
            &cookie,
            Some(&key),
            format!(r#"{{"scope":{scope}}}"#).as_bytes(),
        );
        let answer = body(&reply);
        accepted(&reply);
        assert_eq!(
            text(&answer, &["command_id"]),
            derived("kill_switch", ALICE, &key),
            "{kind}"
        );
        assert_eq!(text(&answer, &["step_up_status"]), "missing", "{kind}");
        let _ = subject;
    }
    let recorded = commands(&f);
    assert_eq!(recorded.len(), 3);
    for (event, (_, kind, subject, agent)) in recorded.iter().zip(&scopes) {
        assert_eq!(text(event, &["payload", "command"]), "kill_switch");
        assert_eq!(text(event, &["payload", "scope"]), *kind);
        assert_eq!(text(event, &["payload", "subject"]), *subject);
        assert_eq!(member(event, &["payload", "agent"]), agent);
        assert_eq!(member(event, &["payload", "step_up"]), &Value::Null);
    }
}

#[test]
fn a_kill_switch_forwards_no_step_up_and_says_it_is_unbound() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let reply = post(
        &f,
        &ws("/kill-switch"),
        &cookie,
        Some(KEY),
        br#"{"scope":{"kind":"workspace","id":null},"environment_shown":"paper","record":null,"owner_exit":null,"step_up":{"assertion_id":"01JBH3BTZ8K2M4N6P8R0S2T0AA","authenticated_at":"2026-10-17T10:00:00.000000000Z","method":"passkey"}}"#,
    );
    let answer = body(&reply);
    accepted(&reply);
    assert_eq!(text(&answer, &["step_up_status"]), "unbound");
    assert_eq!(answer.get("dropped"), None, "every member parsed");
    let recorded = commands(&f);
    assert_eq!(member(&recorded[0], &["payload", "step_up"]), &Value::Null);
}

#[test]
fn a_repeat_replays_its_command_and_another_body_under_the_key_conflicts() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let workspace = br#"{"scope":{"kind":"workspace","id":null}}"#;
    let first = post(&f, &ws("/kill-switch"), &cookie, Some(KEY), workspace);
    let id = text(&body(&first), &["command_id"]);
    f.clock.set(f.clock.secs() + 60);
    let again = post(
        &f,
        &ws("/kill-switch"),
        &cookie,
        Some(KEY),
        br#"{"scope":{"kind":"workspace","id":null},"environment_shown":"live","bogus":1}"#,
    );
    let answer = body(&again);
    accepted(&again);
    assert_eq!(text(&answer, &["command_id"]), id);
    let dropped: Vec<&str> = member(&answer, &["dropped"])
        .as_array()
        .expect("dropped")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(dropped, ["/bogus"]);
    assert_eq!(commands(&f).len(), 1, "a repeat commits nothing");
    let other = post(
        &f,
        &ws("/kill-switch"),
        &cookie,
        Some(KEY),
        br#"{"scope":{"kind":"agent","id":"agt_a"}}"#,
    );
    assert_eq!(
        code(&other),
        (409, ProblemCode::IdempotencyConflict, Effect::None)
    );
    assert_eq!(commands(&f).len(), 1);
    let pause = post(&f, &ws("/agents/agt_a/pause"), &cookie, Some(KEY), b"{}");
    assert_ne!(
        text(&body(&pause), &["command_id"]),
        id,
        "the operation is in the id"
    );
}

#[test]
fn the_session_is_checked_first_then_the_csrf_mark_then_the_key_and_body() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let path = ws("/agents/agt_a/pause");
    let anonymous = send(f.addr, "POST", &path, &[("Idempotency-Key", KEY)], b"{}");
    assert_eq!(anonymous.status, 401);
    assert_eq!(
        anonymous.body,
        send(f.addr, "GET", "/nowhere", &[], b"").body
    );
    let cookie_header = format!("{COOKIE_NAME}={cookie}");
    let unmarked = send(
        f.addr,
        "POST",
        &path,
        &[
            ("Origin", APP),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", KEY),
        ],
        b"{}",
    );
    assert_eq!(code(&unmarked), (403, ProblemCode::Forbidden, Effect::None));
    let foreign = send(
        f.addr,
        "POST",
        &path,
        &[
            ("Origin", "https://elsewhere.test"),
            ("X-Mandate-Request", "1"),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", KEY),
        ],
        b"{}",
    );
    assert_eq!(code(&foreign), (403, ProblemCode::Forbidden, Effect::None));
    let invalid = (422, ProblemCode::Invalid, Effect::None);
    assert_eq!(code(&post(&f, &path, &cookie, None, b"{}")), invalid);
    assert_eq!(
        code(&post(&f, &path, &cookie, Some("short"), b"{}")),
        invalid
    );
    let twice = send(
        f.addr,
        "POST",
        &path,
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", KEY),
            ("Idempotency-Key", KEY),
        ],
        b"{}",
    );
    assert_eq!(code(&twice), invalid);
    let unscoped = post(&f, &ws("/kill-switch"), &cookie, Some(KEY), b"{}");
    assert_eq!(code(&unscoped), invalid);
    let unknown_kind = post(
        &f,
        &ws("/kill-switch"),
        &cookie,
        Some(KEY),
        br#"{"scope":{"kind":"org","id":null}}"#,
    );
    assert_eq!(code(&unknown_kind), invalid);
    assert!(commands(&f).is_empty());
}

#[test]
fn a_pause_body_that_is_not_json_is_read_as_empty_and_says_so() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let reply = post(
        &f,
        &ws("/agents/agt_a/pause"),
        &cookie,
        Some(KEY),
        b"not json",
    );
    let answer = body(&reply);
    accepted(&reply);
    let dropped = member(&answer, &["dropped"]).as_array().expect("dropped");
    assert_eq!(dropped, [Value::Str(String::new())]);
    assert_eq!(commands(&f).len(), 1);
}

#[test]
fn a_scope_the_workspace_does_not_hold_is_404_and_records_nothing() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let not_found = (404, ProblemCode::NotFound, Effect::None);
    for path in ["/agents/agt_zz/pause", "/agents/a%20b/pause"] {
        assert_eq!(
            code(&post(&f, &ws(path), &cookie, Some(KEY), b"{}")),
            not_found,
            "{path}"
        );
    }
    for scope in [
        r#"{"kind":"agent","id":"conn_1"}"#,
        r#"{"kind":"connection","id":"agt_a"}"#,
    ] {
        let reply = post(
            &f,
            &ws("/kill-switch"),
            &cookie,
            Some(KEY),
            format!(r#"{{"scope":{scope}}}"#).as_bytes(),
        );
        assert_eq!(code(&reply), not_found, "{scope}");
    }
    let foreign = post(
        &f,
        &format!("/v1/workspaces/{OTHER_WORKSPACE}/kill-switch"),
        &cookie,
        Some(KEY),
        br#"{"scope":{"kind":"workspace","id":null}}"#,
    );
    assert_eq!(code(&foreign), not_found);
    f.journal.streams.lock().expect("unpoisoned").clear();
    let unopened = post(
        &f,
        &ws("/kill-switch"),
        &cookie,
        Some(KEY),
        br#"{"scope":{"kind":"workspace","id":null}}"#,
    );
    assert_eq!(code(&unopened), not_found);
    assert!(f.journal.appends.lock().expect("unpoisoned").is_empty());
}

#[test]
fn a_member_without_the_row_is_refused_and_a_stranger_is_404() {
    let f = fixture();
    let viewer = signed_in(&f, bob());
    let reply = post(&f, &ws("/agents/agt_a/pause"), &viewer, Some(KEY), b"{}");
    assert_eq!(code(&reply), (403, ProblemCode::Forbidden, Effect::None));
    *f.records.lock().expect("unpoisoned") = Ok(vec![activated(1, alice(), &[Role::Approver])]);
    let approver = signed_in(&f, alice());
    let paused = post(&f, &ws("/agents/agt_a/pause"), &approver, Some(KEY), b"{}");
    accepted(&paused);
    let killed = post(
        &f,
        &ws("/kill-switch"),
        &approver,
        Some(KEY),
        br#"{"scope":{"kind":"workspace","id":null}}"#,
    );
    assert_eq!(code(&killed), (403, ProblemCode::Forbidden, Effect::None));
    let stranger = post(&f, &ws("/agents/agt_a/pause"), &viewer, Some(KEY), b"{}");
    assert_eq!(code(&stranger), (404, ProblemCode::NotFound, Effect::None));
    assert_eq!(commands(&f).len(), 1);
}

#[test]
fn a_failed_membership_or_control_read_records_nothing() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    *f.records.lock().expect("unpoisoned") = Err(LookupFailed);
    let reply = post(&f, &ws("/agents/agt_a/pause"), &cookie, Some(KEY), b"{}");
    assert_eq!(
        code(&reply),
        (503, ProblemCode::MembershipUnavailable, Effect::None)
    );
    *f.records.lock().expect("unpoisoned") = Ok(vec![activated(1, alice(), &[Role::Operator])]);
    f.journal.failing.lock().expect("unpoisoned").insert(ctl());
    let reply = post(&f, &ws("/agents/agt_a/pause"), &cookie, Some(KEY), b"{}");
    assert_eq!(
        code(&reply),
        (503, ProblemCode::JournalUnavailable, Effect::None)
    );
    assert!(f.journal.appends.lock().expect("unpoisoned").is_empty());
}

#[test]
fn a_lapsed_session_still_pauses_and_kills() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    f.clock.set(f.clock.secs() + 6 * 60);
    let paused = post(&f, &ws("/agents/agt_a/pause"), &cookie, Some(KEY), b"{}");
    accepted(&paused);
    let killed = post(
        &f,
        &ws("/kill-switch"),
        &cookie,
        Some(KEY),
        br#"{"scope":{"kind":"agent","id":"agt_a"}}"#,
    );
    accepted(&killed);
    assert_eq!(commands(&f).len(), 2);
}

#[test]
fn an_append_that_does_not_settle_says_what_it_knows() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let path = ws("/agents/agt_a/pause");
    f.journal.script(&[
        Scripted::Fenced,
        Scripted::HeadMismatch,
        Scripted::Unavailable,
    ]);
    accepted(&post(
        &f,
        &path,
        &cookie,
        Some("retry-key-000000001"),
        b"{}",
    ));
    f.journal.script(&[Scripted::Unavailable; 5]);
    let none = post(&f, &path, &cookie, Some("retry-key-000000002"), b"{}");
    assert_eq!(
        code(&none),
        (503, ProblemCode::JournalUnavailable, Effect::None)
    );
    assert_eq!(problem(&none).event_id, None);
    f.journal.script(&[
        Scripted::Ambiguous,
        Scripted::Failed,
        Scripted::Failed,
        Scripted::Failed,
        Scripted::Failed,
    ]);
    let unknown = post(&f, &path, &cookie, Some("retry-key-000000003"), b"{}");
    assert_eq!(
        code(&unknown),
        (503, ProblemCode::OutcomeUnknown, Effect::Unknown)
    );
    let named = problem(&unknown).event_id.expect("the event id");
    assert_eq!(
        json_text(&mandate_api::wire::encode(&named).expect("JSON")),
        derived("pause", ALICE, "retry-key-000000003")
    );
    f.journal.script(&[Scripted::Invalid]);
    let refused = post(&f, &path, &cookie, Some("retry-key-000000004"), b"{}");
    assert_eq!(
        code(&refused),
        (503, ProblemCode::JournalUnavailable, Effect::None)
    );
    assert_eq!(commands(&f).len(), 1);
}

#[test]
fn a_concurrent_copy_replays_and_other_content_under_the_id_conflicts() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let path = ws("/agents/agt_a/pause");
    f.journal.script(&[Scripted::RaceSame]);
    let copy = post(&f, &path, &cookie, Some("race-key-000000001"), b"{}");
    let answer = body(&copy);
    accepted(&copy);
    assert_eq!(
        text(&answer, &["command_id"]),
        derived("pause", ALICE, "race-key-000000001")
    );
    assert_eq!(
        commands(&f).len(),
        1,
        "the copy that landed first is the one"
    );
    f.journal.script(&[Scripted::RaceOther]);
    let other = post(&f, &path, &cookie, Some("race-key-000000002"), b"{}");
    assert_eq!(
        code(&other),
        (409, ProblemCode::IdempotencyConflict, Effect::None)
    );
    f.journal.script(&[Scripted::Conflict]);
    let vanished = post(&f, &path, &cookie, Some("race-key-000000003"), b"{}");
    assert_eq!(
        code(&vanished),
        (503, ProblemCode::OutcomeUnknown, Effect::Unknown)
    );
}

#[test]
fn the_hold_is_not_served_until_the_runtime_folds_it() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let held = post(&f, &ws("/agents/agt_a/hold"), &cookie, Some(KEY), b"{}");
    assert_eq!(held.status, 401);
    assert_eq!(held.body, send(f.addr, "GET", "/nowhere", &[], b"").body);
    assert!(commands(&f).is_empty());
}

#[test]
fn the_body_is_read_only_after_authentication_and_bounded() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let path = ws("/agents/agt_a/pause");
    let large = vec![b' '; KIB_64 + 1];
    let anonymous = send(f.addr, "POST", &path, &[("Idempotency-Key", KEY)], &large);
    assert_eq!(anonymous.status, 401);
    let refused = post(&f, &path, &cookie, Some(KEY), &large);
    assert_eq!(code(&refused), (422, ProblemCode::Invalid, Effect::None));
    let mut padded = b"{}".to_vec();
    padded.resize(KIB_64, b' ');
    assert_eq!(mandate_api_server::writes::MAX_BODY, KIB_64);
    accepted(&post(&f, &path, &cookie, Some(KEY), &padded));
}

#[test]
fn a_draining_process_still_records_both() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    f.drain.begin();
    accepted(&post(
        &f,
        &ws("/agents/agt_a/pause"),
        &cookie,
        Some("drain-key-00000001"),
        b"{}",
    ));
    accepted(&post(
        &f,
        &ws("/kill-switch"),
        &cookie,
        Some("drain-key-00000003"),
        br#"{"scope":{"kind":"workspace","id":null}}"#,
    ));
    assert_eq!(commands(&f).len(), 2);
}

#[test]
fn a_get_on_a_command_route_is_the_one_401() {
    let f = fixture();
    for path in ["/agents/agt_a/pause", "/agents/agt_a/hold", "/kill-switch"] {
        let reply = send(f.addr, "GET", &ws(path), &[], b"");
        assert_eq!(reply.status, 401, "{path}");
    }
}

const APPROVAL: &str = "01JBH3BTZ8K2M4N6P8R0S2T7AA";

fn hash(byte: &str) -> String {
    format!("sha256:{}", byte.repeat(32))
}

/// `agt_a`'s stream with one approval request, and Alice an approver.
fn approvable(f: &Fixture) {
    let stream = format!("agent:{WORKSPACE}:agt_a");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{APPROVAL}","event_type":"ApprovalRequested","payload":{{"content":{{}},"content_hash":"{}","deadline":900}},"stream_id":"{stream}"}}"#,
        hash("ab")
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    let mut requested = row(1, "ApprovalRequested", "{}");
    requested.stream_id = stream.clone();
    requested.event_id = APPROVAL.to_owned();
    requested.hash = Digest::of(&body);
    requested.body = body;
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(stream, vec![requested]);
    *f.records.lock().expect("unpoisoned") = Ok(vec![
        activated(1, alice(), &[Role::Approver]),
        activated(2, bob(), &[Role::Operator]),
    ]);
}

fn responses(approval: &str) -> String {
    ws(&format!("/approvals/{approval}/responses"))
}

fn skipped() -> Vec<u8> {
    format!(
        r#"{{"verdict":"skipped","content_hash":"{}","step_up":null,"delegation":null}}"#,
        hash("ab")
    )
    .into_bytes()
}

/// The control stream's approval responses, as their parsed bodies.
fn answers(f: &Fixture) -> Vec<Value> {
    f.journal
        .rows_of(&ctl())
        .iter()
        .filter(|row| row.event_type == "ApprovalResponseSubmitted")
        .map(|row| parse(&row.body).expect("JSON"))
        .collect()
}

#[test]
fn a_skip_is_recorded_as_the_clis_answer_before_its_202() {
    let f = fixture();
    approvable(&f);
    let cookie = signed_in(&f, alice());
    let reply = post(&f, &responses(APPROVAL), &cookie, Some(KEY), &skipped());
    assert_eq!(
        reply.status,
        202,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    decode::<mandate_api::responses::ApprovalResponseAccepted>(&reply.body)
        .expect("an ApprovalResponseAccepted");
    let answer = body(&reply);
    assert_eq!(
        text(&answer, &["response_id"]),
        derived("approval_response", ALICE, KEY)
    );
    assert_eq!(text(&answer, &["phase"]), "recorded");
    assert_eq!(answer.get("command_id"), None);
    assert_eq!(answer.get("dropped"), None);
    let recorded = answers(&f);
    assert_eq!(recorded.len(), 1);
    let payload = parse(
        format!(
            r#"{{"agent":"agt_a","approval":"{APPROVAL}","verdict":"skipped","content_hash":"{}","submitted_at":{T0},"step_up":null,"responder":"{ALICE}","role":"approver"}}"#,
            hash("ab")
        )
        .as_bytes(),
    )
    .expect("JSON");
    assert_eq!(member(&recorded[0], &["payload"]), &payload);
    assert_eq!(text(&recorded[0], &["actor", "id"]), ALICE);
    let again = post(&f, &responses(APPROVAL), &cookie, Some(KEY), &skipped());
    assert_eq!(
        text(&body(&again), &["response_id"]),
        text(&answer, &["response_id"])
    );
    assert_eq!(answers(&f).len(), 1, "a repeat commits nothing");
}

#[test]
fn a_skip_drops_what_it_may_and_keeps_its_hard_members_strict() {
    let f = fixture();
    approvable(&f);
    let cookie = signed_in(&f, alice());
    let loose = format!(
        r#"{{"verdict":"skipped","content_hash":"{}","record":7,"bogus":1}}"#,
        hash("ab")
    );
    let reply = post(
        &f,
        &responses(APPROVAL),
        &cookie,
        Some(KEY),
        loose.as_bytes(),
    );
    assert_eq!(
        reply.status,
        202,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let dropped: Vec<String> = member(&body(&reply), &["dropped"])
        .as_array()
        .expect("dropped")
        .iter()
        .filter_map(|v| v.as_str().map(str::to_owned))
        .collect();
    assert_eq!(dropped, ["/record", "/bogus"]);
    let invalid = (422, ProblemCode::Invalid, Effect::None);
    let unhashed = br#"{"verdict":"skipped","content_hash":"nope"}"#;
    assert_eq!(
        code(&post(
            &f,
            &responses(APPROVAL),
            &cookie,
            Some("key-unhashed-0001"),
            unhashed
        )),
        invalid
    );
    let unknown = format!(r#"{{"verdict":"maybe","content_hash":"{}"}}"#, hash("ab"));
    assert_eq!(
        code(&post(
            &f,
            &responses(APPROVAL),
            &cookie,
            Some("key-unknown-00001"),
            unknown.as_bytes()
        )),
        invalid
    );
    assert_eq!(answers(&f).len(), 1);
}

#[test]
fn an_unknown_approval_is_404_and_a_grant_waits_for_step_up() {
    let f = fixture();
    approvable(&f);
    let cookie = signed_in(&f, alice());
    let not_found = (404, ProblemCode::NotFound, Effect::None);
    for approval in [
        "01JBH3BTZ8K2M4N6P8R0S2T001",
        "01JBH3BTZ8K2M4N6P8R0S2T7AB",
        "nope",
    ] {
        let reply = post(&f, &responses(approval), &cookie, Some(KEY), &skipped());
        assert_eq!(code(&reply), not_found, "{approval}");
    }
    let record = format!(
        r#"{{"artifact":"{}","ui_build":"{}"}}"#,
        hash("cd"),
        hash("ef")
    );
    let grant = |step_up: &str| {
        format!(
            r#"{{"verdict":"approved","content_hash":"{}","record":{record},"step_up":{step_up},"delegation":null}}"#,
            hash("ab")
        )
    };
    let bare = post(
        &f,
        &responses(APPROVAL),
        &cookie,
        Some(KEY),
        grant("null").as_bytes(),
    );
    assert_eq!(
        code(&bare),
        (401, ProblemCode::StepUpRequired, Effect::None)
    );
    let evidence = r#"{"assertion_id":"01JBH3BTZ8K2M4N6P8R0S2T0AA","authenticated_at":"2026-10-17T10:00:00.000000000Z","method":"passkey"}"#;
    let claimed = post(
        &f,
        &responses(APPROVAL),
        &cookie,
        Some(KEY),
        grant(evidence).as_bytes(),
    );
    assert_eq!(
        code(&claimed),
        (401, ProblemCode::StepUpMissing, Effect::None)
    );
    let elsewhere = post(
        &f,
        &responses("01JBH3BTZ8K2M4N6P8R0S2T7AB"),
        &cookie,
        Some(KEY),
        grant("null").as_bytes(),
    );
    assert_eq!(
        code(&elsewhere),
        not_found,
        "the approval is found before step-up is judged"
    );
    assert!(answers(&f).is_empty());
}

#[test]
fn only_the_skip_row_skips_and_a_reduction_only_session_cannot() {
    let f = fixture();
    approvable(&f);
    let operator = signed_in(&f, bob());
    let reply = post(&f, &responses(APPROVAL), &operator, Some(KEY), &skipped());
    assert_eq!(code(&reply), (403, ProblemCode::Forbidden, Effect::None));
    let approver = signed_in(&f, alice());
    f.clock.set(f.clock.secs() + 6 * 60);
    let paused = post(
        &f,
        &ws("/agents/agt_a/pause"),
        &approver,
        Some("lapse-pause-000001"),
        b"{}",
    );
    assert_eq!(paused.status, 202, "a pause records the outage refresh");
    let lapsed = post(&f, &responses(APPROVAL), &approver, Some(KEY), &skipped());
    assert_eq!(
        code(&lapsed),
        (403, ProblemCode::ReductionOnly, Effect::None)
    );
    assert!(answers(&f).is_empty());
}

#[test]
fn an_unreadable_agent_stream_records_nothing_and_a_drain_still_records() {
    let f = fixture();
    approvable(&f);
    let cookie = signed_in(&f, alice());
    let stream = format!("agent:{WORKSPACE}:agt_a");
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(stream.clone());
    let reply = post(&f, &responses(APPROVAL), &cookie, Some(KEY), &skipped());
    assert_eq!(
        code(&reply),
        (503, ProblemCode::JournalUnavailable, Effect::None)
    );
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .remove(&stream);
    f.drain.begin();
    let drained = post(&f, &responses(APPROVAL), &cookie, Some(KEY), &skipped());
    assert_eq!(drained.status, 202);
    assert_eq!(answers(&f).len(), 1);
}

#[test]
fn an_unreadable_stream_does_not_hide_an_approval_on_another() {
    let f = fixture();
    approvable(&f);
    let a = format!("agent:{WORKSPACE}:agt_a");
    let b = format!("agent:{WORKSPACE}:agt_b");
    {
        let mut streams = f.journal.streams.lock().expect("unpoisoned");
        let moved = streams.remove(&a).expect("agt_a's stream");
        let rows: Vec<StoredEvent> = moved
            .into_iter()
            .map(|mut row| {
                row.stream_id = b.clone();
                row
            })
            .collect();
        streams.insert(b.clone(), rows);
        let control = streams.get_mut(&ctl()).expect("the control stream");
        control.push(row(
            4,
            "AgentDeployed",
            r#"{"agent_id":"agt_b","mandate_version":"sha256:01"}"#,
        ));
    }
    f.journal.failing.lock().expect("unpoisoned").insert(a);
    let cookie = signed_in(&f, alice());
    let reply = post(&f, &responses(APPROVAL), &cookie, Some(KEY), &skipped());
    assert_eq!(
        reply.status,
        202,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let recorded = answers(&f);
    assert_eq!(text(&recorded[0], &["payload", "agent"]), "agt_b");
    let absent = post(
        &f,
        &responses("01JBH3BTZ8K2M4N6P8R0S2T7AB"),
        &cookie,
        Some("absent-key-0000001"),
        &skipped(),
    );
    assert_eq!(
        code(&absent),
        (503, ProblemCode::JournalUnavailable, Effect::None),
        "an absent approval cannot be told from one on the unreadable stream"
    );
}
