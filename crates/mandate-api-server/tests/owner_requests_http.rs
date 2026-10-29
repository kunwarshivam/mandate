//! An owner's request over HTTP (workspace API §3.11 item 13, §4.6, [DEC-2151], E10-30): checked
//! in item 13's order, refused with nothing committed, and otherwise recorded as one
//! `OwnerRequestSubmitted` (journal spec §9.9) before its `202`, replayed by key. The request is
//! for the agent's own pinned instrument, a buy, and carries a positive size (DEC-2151 items 1, 3
//! and 7); who asked comes from the session, never the body (API-6).
//!
//! The expected records are written out here from journal spec §9.9's members and the fixture's
//! pinned instrument, never from the server's own code.
//!
//! [DEC-2151]: ../../../docs/project/decisions/DEC-2151.md

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::problem::{Problem, ProblemCode};
use mandate_api::responses::CommandAccepted;
use mandate_api::wire::{Id, decode};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::objects::{ObjectStore, Objects};
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
const OTHER_KEY: &str = "k1Z-9_bbbbbbbbbb";
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
/// The instrument the pinned fixture mandate pins (AAPL).
const PINNED: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";
/// A well-formed asset id no fixture pins.
const UNPINNED: &str = "8ccae427-5dd0-45b3-b5fe-7c0c3a8ee5c4";
const PINNED_DOCUMENT: &str = include_str!("fixtures/mandate.json");
const RESEARCH_DOCUMENT: &str = include_str!("fixtures/research_mandate.json");

type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    records: Records,
    sessions: Arc<Sessions>,
    clock: Arc<common::ManualClock>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// A server over a `environment` workspace: `agt_a` deployed with the pinned document, `agt_r`
/// with the unpinned research document, `agt_x` with a version the store does not hold, and
/// `agt_s` deployed and then stopped.
fn fixture(environment: &str) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-owner-requests-{}-{n}",
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
    let store = Objects::new(std::collections::BTreeMap::<Digest, Vec<u8>>::new());
    for document in [PINNED_DOCUMENT, RESEARCH_DOCUMENT] {
        store
            .put(&to_canonical(&parse(document.as_bytes()).expect("JSON")))
            .expect("stored");
    }
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
        artifacts: Some(Arc::new(store)),
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
    };
    let f = Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        records,
        sessions,
        clock,
    };
    seed(&f, environment);
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

fn version_of(document: &str) -> String {
    let bytes = to_canonical(&parse(document.as_bytes()).expect("JSON"));
    format!("sha256:{}", Digest::of(&bytes).to_hex())
}

fn row(environment: &str, seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"{environment}","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{}"}}"#,
        ctl()
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: ctl(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: environment.to_owned(),
        recorded_at: format!("2026-10-17T10:00:{seq:02}.000000000Z"),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

fn seed(f: &Fixture, environment: &str) {
    let deployed = |seq, agent: &str, version: &str| {
        row(
            environment,
            seq,
            "AgentDeployed",
            &format!(r#"{{"agent_id":"{agent}","mandate_version":"{version}"}}"#),
        )
    };
    let absent = format!("sha256:{}", "ee".repeat(32));
    f.journal.streams.lock().expect("unpoisoned").insert(
        ctl(),
        vec![
            row(environment, 1, "StreamOpened", "{}"),
            deployed(2, "agt_a", &version_of(PINNED_DOCUMENT)),
            deployed(3, "agt_r", &version_of(RESEARCH_DOCUMENT)),
            deployed(4, "agt_x", &absent),
            deployed(5, "agt_s", &version_of(PINNED_DOCUMENT)),
            row(
                environment,
                6,
                "AgentStopped",
                r#"{"agent_id":"agt_s","connection_id":"conn_alpaca_paper_01","reason":"owner_stop"}"#,
            ),
        ],
    );
}

fn ws_in(workspace: &str, agent: &str) -> String {
    format!("/v1/workspaces/{workspace}/agents/{agent}/requests")
}

fn ws(agent: &str) -> String {
    ws_in(WORKSPACE, agent)
}

/// A browser's command: the session cookie, the app origin, the CSRF header, and `key`.
fn post(f: &Fixture, path: &str, cookie: Option<&str>, key: Option<&str>, body: &[u8]) -> Reply {
    let cookie = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let mut headers = vec![
        ("Origin", APP),
        ("X-Mandate-Request", "1"),
        ("Content-Type", "application/json"),
    ];
    if let Some(cookie) = cookie.as_deref() {
        headers.push(("Cookie", cookie));
    }
    if let Some(key) = key {
        headers.push(("Idempotency-Key", key));
    }
    send(f.addr, "POST", path, &headers, body)
}

fn request(instrument: &str, side: &str, size: &str) -> Vec<u8> {
    format!(r#"{{"instrument":"{instrument}","side":"{side}","size":{size}}}"#).into_bytes()
}

fn buy(size: &str) -> Vec<u8> {
    request(PINNED, "buy", &format!(r#""{size}""#))
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
    parse(&mandate_api::wire::encode(&id).expect("JSON"))
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
    decode::<CommandAccepted>(&reply.body).expect("a CommandAccepted")
}

fn problem(reply: &Reply) -> Problem {
    assert_eq!(
        reply.header("Content-Type"),
        Some(http::PROBLEM_JSON),
        "{} {}",
        reply.status,
        String::from_utf8_lossy(&reply.body)
    );
    decode::<Problem>(&reply.body).expect("a problem")
}

fn code(reply: &Reply) -> (u16, ProblemCode) {
    let problem = problem(reply);
    (reply.status, problem.code)
}

/// The paths of an `invalid` problem's violations.
fn violation_paths(reply: &Reply) -> Vec<String> {
    let body = parse(&reply.body).expect("JSON");
    match body.get("violations") {
        Some(Value::Array(list)) => list
            .iter()
            .filter_map(|v| v.get("path").and_then(Value::as_str).map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// The control stream's owner requests, as their parsed bodies.
fn requests(f: &Fixture) -> Vec<Value> {
    f.journal
        .rows_of(&ctl())
        .iter()
        .filter(|row| row.event_type == "OwnerRequestSubmitted")
        .map(|row| parse(&row.body).expect("JSON"))
        .collect()
}

fn nothing_appended(f: &Fixture) -> bool {
    f.journal.appends.lock().expect("unpoisoned").is_empty() && requests(f).is_empty()
}

fn member<'v>(value: &'v Value, path: &[&str]) -> &'v Value {
    path.iter()
        .try_fold(value, |v, k| v.get(k))
        .unwrap_or_else(|| panic!("{path:?}"))
}

#[test]
fn a_request_is_recorded_on_the_control_stream_before_its_202() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let reply = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("2"));
    let answer = parse(&reply.body).expect("JSON");
    accepted(&reply);
    let id = derived("owner_request", ALICE, KEY);
    assert_eq!(member(&answer, &["command_id"]).as_str(), Some(id.as_str()));
    assert_eq!(member(&answer, &["phase"]).as_str(), Some("recorded"));
    let rows = f.journal.rows_of(&ctl());
    let last = rows.last().expect("a row");
    assert_eq!((last.seq, last.event_id.as_str()), (7, id.as_str()));
    assert_eq!(last.event_type, "OwnerRequestSubmitted");
    assert_eq!(last.schema_version, 1);
    let event = parse(&last.body).expect("JSON");
    assert_eq!(member(&event, &["environment"]).as_str(), Some("paper"));
    assert_eq!(member(&event, &["actor", "kind"]).as_str(), Some("user"));
    assert_eq!(member(&event, &["actor", "id"]).as_str(), Some(ALICE));
    let payload = parse(
        format!(
            r#"{{"agent_id":"agt_a","instrument_id":"{PINNED}","side":"buy","quantity":"2","requested_by":"owner","client_id":null}}"#
        )
        .as_bytes(),
    )
    .expect("JSON");
    assert_eq!(member(&event, &["payload"]), &payload);
}

#[test]
fn a_fractional_size_is_recorded_as_asked() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    accepted(&post(
        &f,
        &ws("agt_a"),
        Some(&cookie),
        Some(KEY),
        &request(PINNED, "buy", r#""0.5""#),
    ));
    let recorded = requests(&f);
    assert_eq!(recorded.len(), 1);
    assert_eq!(
        member(&recorded[0], &["payload", "quantity"]).as_str(),
        Some("0.5")
    );
}

#[test]
fn a_sell_is_invalid_at_side_and_judged_before_the_size() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    for size in [r#""1""#, r#""0""#] {
        let reply = post(
            &f,
            &ws("agt_a"),
            Some(&cookie),
            Some(KEY),
            &request(PINNED, "sell", size),
        );
        assert_eq!(code(&reply), (422, ProblemCode::Invalid), "{size}");
        assert_eq!(
            violation_paths(&reply),
            vec!["/side".to_owned()],
            "{size}: an owner's sale is the owner exit's (DEC-2151 item 7)"
        );
    }
    assert!(nothing_appended(&f));
}

#[test]
fn a_repeat_replays_its_request_and_another_body_under_the_key_conflicts() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let first = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("2"));
    let id = accepted(&first);
    let again = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("2"));
    assert_eq!(accepted(&again), id, "the same answer for the same key");
    assert_eq!(requests(&f).len(), 1, "recorded once");
    let other = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("3"));
    assert_eq!(code(&other), (409, ProblemCode::IdempotencyConflict));
    assert_eq!(requests(&f).len(), 1);
    accepted(&post(
        &f,
        &ws("agt_a"),
        Some(&cookie),
        Some(OTHER_KEY),
        &buy("3"),
    ));
    assert_eq!(requests(&f).len(), 2, "a new key is a new request");
}

#[test]
fn a_size_that_is_missing_or_not_positive_is_invalid_at_size() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let bodies = [
        format!(r#"{{"instrument":"{PINNED}","side":"buy"}}"#).into_bytes(),
        request(PINNED, "buy", "null"),
        buy("0"),
        buy("0.000"),
        buy("-1"),
    ];
    for body in bodies {
        let reply = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &body);
        assert_eq!(
            code(&reply),
            (422, ProblemCode::Invalid),
            "{}",
            String::from_utf8_lossy(&body)
        );
        assert_eq!(violation_paths(&reply), vec!["/size".to_owned()]);
    }
    assert!(nothing_appended(&f));
}

#[test]
fn an_instrument_the_agent_does_not_pin_is_invalid_at_instrument() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    for (agent, instrument) in [("agt_a", UNPINNED), ("agt_r", PINNED), ("agt_r", UNPINNED)] {
        let reply = post(
            &f,
            &ws(agent),
            Some(&cookie),
            Some(KEY),
            &request(instrument, "buy", r#""1""#),
        );
        assert_eq!(
            code(&reply),
            (422, ProblemCode::Invalid),
            "{agent} {instrument}"
        );
        assert_eq!(
            violation_paths(&reply),
            vec!["/instrument".to_owned()],
            "{agent} {instrument}: outside the agent's pinned universe"
        );
    }
    assert!(nothing_appended(&f));
}

#[test]
fn the_size_is_judged_before_the_instrument() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let reply = post(
        &f,
        &ws("agt_a"),
        Some(&cookie),
        Some(KEY),
        &request(UNPINNED, "buy", r#""0""#),
    );
    assert_eq!(violation_paths(&reply), vec!["/size".to_owned()]);
}

#[test]
fn an_unknown_stopped_or_foreign_agent_is_the_one_404() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let unknown = post(&f, &ws("agt_zz"), Some(&cookie), Some(KEY), &buy("1"));
    assert_eq!(code(&unknown), (404, ProblemCode::NotFound));
    for path in [
        ws("agt_s"),
        ws_in(OTHER_WORKSPACE, "agt_a"),
        ws_in("not-a-ulid", "agt_a"),
    ] {
        let reply = post(&f, &path, Some(&cookie), Some(KEY), &buy("1"));
        assert_eq!(reply.status, 404, "{path}");
        assert_eq!(reply.body, unknown.body, "{path}");
    }
    let stopped_and_unsized = post(&f, &ws("agt_s"), Some(&cookie), Some(KEY), &buy("0"));
    assert_eq!(
        code(&stopped_and_unsized),
        (404, ProblemCode::NotFound),
        "the agent is judged before the size"
    );
    assert!(nothing_appended(&f));
}

#[test]
fn a_live_workspace_is_live_unavailable_and_records_nothing() {
    let f = fixture("live");
    let cookie = signed_in(&f, alice());
    let reply = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("1"));
    assert_eq!(code(&reply), (409, ProblemCode::LiveUnavailable));
    let zero_size = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("0"));
    assert_eq!(
        code(&zero_size),
        (409, ProblemCode::LiveUnavailable),
        "the environment is judged before the size"
    );
    assert!(nothing_appended(&f));
}

#[test]
fn a_document_the_store_cannot_give_is_journal_unavailable() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let reply = post(&f, &ws("agt_x"), Some(&cookie), Some(KEY), &buy("1"));
    assert_eq!(code(&reply), (503, ProblemCode::JournalUnavailable));
    assert!(nothing_appended(&f));
}

#[test]
fn only_the_request_row_may_ask_and_a_stranger_is_404() {
    let f = fixture("paper");
    let viewer = signed_in(&f, bob());
    let reply = post(&f, &ws("agt_a"), Some(&viewer), Some(KEY), &buy("1"));
    assert_eq!(code(&reply), (403, ProblemCode::Forbidden));
    for roles in [
        vec![Role::WorkspaceAdmin],
        vec![Role::Approver],
        vec![Role::Auditor],
    ] {
        *f.records.lock().expect("unpoisoned") = Ok(vec![activated(1, bob(), &roles)]);
        let reply = post(&f, &ws("agt_a"), Some(&viewer), Some(KEY), &buy("1"));
        assert_eq!(code(&reply), (403, ProblemCode::Forbidden), "{roles:?}");
    }
    *f.records.lock().expect("unpoisoned") = Ok(vec![activated(1, bob(), &[Role::Operator])]);
    let stranger = signed_in(&f, alice());
    let reply = post(&f, &ws("agt_a"), Some(&stranger), Some(KEY), &buy("1"));
    assert_eq!(
        code(&reply),
        (404, ProblemCode::NotFound),
        "alice holds no membership"
    );
    assert!(nothing_appended(&f));
}

#[test]
fn the_session_csrf_key_and_body_are_checked_first() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let one = send(f.addr, "GET", "/nowhere", &[], b"").body;
    let unauthenticated = post(&f, &ws("agt_zz"), None, Some(KEY), &buy("0"));
    assert_eq!(unauthenticated.status, 401);
    assert_eq!(unauthenticated.body, one);
    let unmarked = send(
        f.addr,
        "POST",
        &ws("agt_a"),
        &[
            ("Origin", APP),
            ("Cookie", &format!("{COOKIE_NAME}={cookie}")),
            ("Content-Type", "application/json"),
            ("Idempotency-Key", KEY),
        ],
        &buy("1"),
    );
    assert_eq!(unmarked.status, 403, "no CSRF mark");
    let keyless = post(&f, &ws("agt_a"), Some(&cookie), None, &buy("1"));
    assert_eq!(code(&keyless).1, ProblemCode::Invalid, "no Idempotency-Key");
    for body in [
        format!(r#"{{"instrument":"{PINNED}","side":"buy","size":"1","requested_by":"client"}}"#)
            .into_bytes(),
        format!(r#"{{"instrument":"{PINNED}","side":"hold","size":"1"}}"#).into_bytes(),
        br#"{"instrument":"AAPL","side":"buy","size":"1"}"#.to_vec(),
        b"not json".to_vec(),
    ] {
        let reply = post(&f, &ws("agt_zz"), Some(&cookie), Some(KEY), &body);
        assert_eq!(
            code(&reply),
            (422, ProblemCode::Invalid),
            "the body is judged before the agent: {}",
            String::from_utf8_lossy(&body)
        );
    }
    assert!(nothing_appended(&f));
}

#[test]
fn a_reduction_only_session_cannot_ask() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    assert_eq!(
        f.sessions.refresh(
            &cookie,
            mandate_api_server::session::Answer::Unreachable,
            at(T0 + 1)
        ),
        Ok(None)
    );
    let reply = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("1"));
    assert_eq!(code(&reply), (403, ProblemCode::ReductionOnly));
    assert!(nothing_appended(&f));
}

#[test]
fn a_failed_membership_or_control_read_records_nothing() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    *f.records.lock().expect("unpoisoned") = Err(LookupFailed);
    let reply = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("1"));
    assert_eq!(code(&reply), (503, ProblemCode::MembershipUnavailable));
    *f.records.lock().expect("unpoisoned") = Ok(vec![activated(1, alice(), &[Role::Operator])]);
    f.journal.failing.lock().expect("unpoisoned").insert(ctl());
    let reply = post(&f, &ws("agt_a"), Some(&cookie), Some(KEY), &buy("1"));
    assert_eq!(code(&reply), (503, ProblemCode::JournalUnavailable));
    assert!(f.journal.appends.lock().expect("unpoisoned").is_empty());
}

/// One post that fails two adjacent checks of item 13, and the refusal of the earlier one.
struct Ordered {
    earlier_first: &'static str,
    environment: &'static str,
    viewer: bool,
    agent: &'static str,
    body: Vec<u8>,
    status: u16,
    code: ProblemCode,
    paths: &'static [&'static str],
}

#[test]
fn the_checks_are_judged_in_order() {
    let hold = format!(r#"{{"instrument":"{PINNED}","side":"hold","size":"1"}}"#).into_bytes();
    let cases = [
        Ordered {
            earlier_first: "1 before 2: a viewer's invalid body is invalid, not forbidden",
            environment: "paper",
            viewer: true,
            agent: "agt_a",
            body: hold,
            status: 422,
            code: ProblemCode::Invalid,
            paths: &[],
        },
        Ordered {
            earlier_first: "2 before 3: a viewer on a missing agent is forbidden, not 404",
            environment: "paper",
            viewer: true,
            agent: "agt_zz",
            body: buy("1"),
            status: 403,
            code: ProblemCode::Forbidden,
            paths: &[],
        },
        Ordered {
            earlier_first: "3 before 4: an unknown agent in a live workspace is 404",
            environment: "live",
            viewer: false,
            agent: "agt_zz",
            body: buy("1"),
            status: 404,
            code: ProblemCode::NotFound,
            paths: &[],
        },
        Ordered {
            earlier_first: "3 before 4: a stopped agent in a live workspace is 404",
            environment: "live",
            viewer: false,
            agent: "agt_s",
            body: buy("1"),
            status: 404,
            code: ProblemCode::NotFound,
            paths: &[],
        },
        Ordered {
            earlier_first: "4 before 5: a sell in a live workspace is live_unavailable",
            environment: "live",
            viewer: false,
            agent: "agt_a",
            body: request(PINNED, "sell", r#""1""#),
            status: 409,
            code: ProblemCode::LiveUnavailable,
            paths: &[],
        },
        Ordered {
            earlier_first: "6 before 7: a zero size for an unreadable document is invalid at /size",
            environment: "paper",
            viewer: false,
            agent: "agt_x",
            body: buy("0"),
            status: 422,
            code: ProblemCode::Invalid,
            paths: &["/size"],
        },
    ];
    for case in cases {
        let f = fixture(case.environment);
        let who = if case.viewer { bob() } else { alice() };
        let cookie = signed_in(&f, who);
        let reply = post(&f, &ws(case.agent), Some(&cookie), Some(KEY), &case.body);
        assert_eq!(
            code(&reply),
            (case.status, case.code),
            "{}",
            case.earlier_first
        );
        if !case.paths.is_empty() {
            assert_eq!(
                violation_paths(&reply),
                case.paths
                    .iter()
                    .map(|p| (*p).to_owned())
                    .collect::<Vec<_>>(),
                "{}",
                case.earlier_first
            );
        }
        assert!(nothing_appended(&f), "{}", case.earlier_first);
    }
}

#[test]
fn a_get_on_the_request_route_is_the_one_401() {
    let f = fixture("paper");
    let cookie = signed_in(&f, alice());
    let one = send(f.addr, "GET", "/nowhere", &[], b"").body;
    let reply = send(
        f.addr,
        "GET",
        &ws("agt_a"),
        &[
            ("Origin", APP),
            ("Cookie", &format!("{COOKIE_NAME}={cookie}")),
        ],
        b"",
    );
    assert_eq!(reply.status, 401);
    assert_eq!(reply.body, one);
}
