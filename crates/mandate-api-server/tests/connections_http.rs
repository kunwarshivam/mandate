//! The connections read over HTTP (§4.5, DEC-2620, DEC-2631): the workspace admin only, the role
//! before the id, one `not_found` for every id the workspace's control stream does not name, the
//! fold's view against literal oracles, and nothing secret-shaped in any answer.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, PROBLEM_JSON, Settings};
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
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";
const C1: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W50";
const C2: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W51";
const C3: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W52";
/// A connection only the other workspace's control stream names.
const FOREIGN: &str = "conn_01JB3K6F0C9R2V7N4M8Q1T5W59";
/// Journaled but never served (DEC-2620 item 4): each answer is scanned for it.
const CANARY: &str = "CANARY";

type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    records: Records,
    sessions: Arc<Sessions>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture() -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-connections-{}-{n}",
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
    let records: Records = Arc::new(Mutex::new(Ok(vec![
        activated(1, principal(ALICE), &[Role::WorkspaceAdmin]),
        activated(2, principal(BOB), &[Role::Viewer]),
        activated(3, principal(CAROL), &[Role::Operator]),
    ])));
    let parsed = Arc::clone(&records);
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
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
        clock: common::ManualClock::at(T0) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| parsed.lock().expect("unpoisoned").clone()),
        hosted: hosted(),
        artifacts: None,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
        bars: None,
        chat: Default::default(),
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        records,
        sessions,
    }
}

const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const CAROL: &str = "01JB3K6F0C9R2V7N4M8Q1T5W42";
const DAVE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W43";

fn principal(id: &str) -> PrincipalId {
    PrincipalId::from_ulid_text(id).expect("a ULID")
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

fn signed_in(f: &Fixture, who: &str) -> String {
    f.sessions
        .open(principal(who), "sha256:aaaa".to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned()
}

fn get(f: &Fixture, path: &str, cookie: Option<&str>) -> Reply {
    let header = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let mut headers = vec![("Origin", APP)];
    if let Some(header) = header.as_deref() {
        headers.push(("Cookie", header));
    }
    send(f.addr, "GET", path, &headers, b"")
}

fn ws(path: &str) -> String {
    format!("/v1/workspaces/{WORKSPACE}{path}")
}

fn ctl(workspace: &str) -> String {
    format!("ctl:{workspace}")
}

fn row(stream: &str, seq: u64, event_type: &str, version: u64, payload: &str) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: version,
        environment: "paper".to_owned(),
        recorded_at: format!("2026-10-17T10:00:{seq:02}.000000000Z"),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// What a request journals beside what is served: each carries [`CANARY`].
fn journaled_only() -> String {
    format!(
        r#""account_ref":"acct_{CANARY}","user":"usr_{CANARY}","step_up":{{"assertion_id":"sa_{CANARY}","authenticated_at":"2026-10-17T10:00:00.000000000Z","method":"passkey"}}"#
    )
}

fn requested(seq: u64, id: &str, environment: &str) -> StoredEvent {
    row(
        &ctl(WORKSPACE),
        seq,
        "ConnectionRequested",
        2,
        &format!(
            r#"{{"connection_id":"{id}","broker":"alpaca","environment":"{environment}",{},"credential":"oauth","breadth":{{"document":"alpaca_oauth_token_breadth","version":"1"}}}}"#,
            journaled_only()
        ),
    )
}

fn established(seq: u64, version: u64, id: &str, environment: &str, scopes: &str) -> StoredEvent {
    row(
        &ctl(WORKSPACE),
        seq,
        "ConnectionEstablished",
        version,
        &format!(
            r#"{{"connection_id":"{id}","broker":"alpaca","environment":"{environment}","scopes":{scopes},{},"secret_token":"{CANARY}"}}"#,
            journaled_only()
        ),
    )
}

fn refused(seq: u64, id: &str, environment: &str, occasion: &str) -> StoredEvent {
    row(
        &ctl(WORKSPACE),
        seq,
        "ConnectionRefused",
        1,
        &format!(
            r#"{{"connection_id":"{id}","broker":"alpaca","environment":"{environment}","occasion":"{occasion}","check":"scope","reason":"scope_mismatch","existing_connection_id":"conn_{CANARY}",{}}}"#,
            journaled_only()
        ),
    )
}

fn revoked(seq: u64, id: &str, reason: &str) -> StoredEvent {
    row(
        &ctl(WORKSPACE),
        seq,
        "ConnectionRevoked",
        2,
        &format!(r#"{{"connection_id":"{id}","reason":"{reason}","user":"usr_{CANARY}"}}"#),
    )
}

fn deployed(seq: u64, version: u64, agent: &str, id: &str) -> StoredEvent {
    let version_hash = format!("sha256:{}", "ab".repeat(32));
    let named = if version == 2 {
        format!(r#","connection_id":"{id}""#)
    } else {
        String::new()
    };
    row(
        &ctl(WORKSPACE),
        seq,
        "AgentDeployed",
        version,
        &format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version_hash}","record_ref":"{version_hash}"{named}}}"#
        ),
    )
}

fn stopped(seq: u64, agent: &str) -> StoredEvent {
    row(
        &ctl(WORKSPACE),
        seq,
        "AgentStopped",
        1,
        &format!(r#"{{"agent_id":"{agent}"}}"#),
    )
}

/// The workspace's control stream, and the other workspace's naming [`FOREIGN`].
///
/// - C1: requested, established at version 2 with both scopes, then established again at version
///   1 with only `trading`, so the latest grant is served; `agt_a`'s version-2 deployment names it
///   and a version-1 redeployment then names nothing; `agt_b` names it, stops, and is redeployed
///   naming it again.
/// - C2: requested on paper, then refused at connect naming `live`, so the refusal's environment
///   is served.
/// - C3: requested, established, then revoked by its owner; a `reconnect` refusal after that leaves
///   it revoked.
fn seed(f: &Fixture) {
    let rows = vec![
        row(&ctl(WORKSPACE), 1, "StreamOpened", 1, "{}"),
        requested(2, C1, "paper"),
        established(3, 2, C1, "paper", r#"["data","trading"]"#),
        requested(4, C2, "paper"),
        refused(5, C2, "live", "connect"),
        deployed(6, 2, "agt_a", C1),
        deployed(7, 2, "agt_b", C1),
        requested(8, C3, "paper"),
        established(9, 2, C3, "paper", r#"["data","trading"]"#),
        established(10, 1, C1, "paper", r#"["trading"]"#),
        deployed(11, 1, "agt_a", C1),
        stopped(12, "agt_b"),
        revoked(13, C3, "owner"),
        refused(14, C3, "paper", "reconnect"),
        deployed(15, 2, "agt_b", C1),
    ];
    let other = vec![
        row(&ctl(OTHER_WORKSPACE), 1, "StreamOpened", 1, "{}"),
        row(
            &ctl(OTHER_WORKSPACE),
            2,
            "ConnectionRequested",
            2,
            &format!(
                r#"{{"connection_id":"{FOREIGN}","broker":"alpaca","environment":"paper",{}}}"#,
                journaled_only()
            ),
        ),
    ];
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    streams.insert(ctl(WORKSPACE), rows);
    streams.insert(ctl(OTHER_WORKSPACE), other);
}

/// The three connections [`seed`] names, written out by hand (DEC-2620 items 2 and 3).
fn expected() -> Value {
    parse(
        format!(
            r#"[
            {{"connection_id":"{C1}","broker":"alpaca","environment":"paper","state":"established",
              "scopes":["trading"],"since":"2026-10-17T10:00:10.000000000Z","refusal":null,
              "revoked_reason":null,"agents":["agt_b"]}},
            {{"connection_id":"{C2}","broker":"alpaca","environment":"live","state":"refused",
              "scopes":null,"since":"2026-10-17T10:00:05.000000000Z",
              "refusal":{{"check":"scope","reason":"scope_mismatch"}},"revoked_reason":null,"agents":[]}},
            {{"connection_id":"{C3}","broker":"alpaca","environment":"paper","state":"revoked",
              "scopes":["data","trading"],"since":"2026-10-17T10:00:13.000000000Z","refusal":null,
              "revoked_reason":"owner","agents":[]}}
        ]"#
        )
        .as_bytes(),
    )
    .expect("JSON")
}

fn json(reply: &Reply) -> Value {
    parse(&reply.body).expect("a JSON body")
}

fn code(reply: &Reply) -> String {
    assert_eq!(reply.header("content-type"), Some(PROBLEM_JSON));
    json(reply)
        .get("code")
        .and_then(Value::as_str)
        .expect("a code")
        .to_owned()
}

#[test]
fn without_a_live_session_both_reads_are_the_one_401() {
    let f = fixture();
    seed(&f);
    let one = send(f.addr, "GET", "/nowhere", &[], b"").body;
    for path in [ws("/connections"), ws(&format!("/connections/{C1}"))] {
        let reply = get(&f, &path, None);
        assert_eq!(reply.status, 401, "{path}");
        assert_eq!(reply.body, one, "{path}");
        let reply = get(&f, &path, Some("not-a-session"));
        assert_eq!(reply.status, 401, "{path}");
    }
}

/// A client's credential never reaches either read: the routes admit only a session, so a
/// bearer token, with or without a cookie that is not a session, is the one 401 and reads nothing
/// (DEC-2631 item 1; §3.8 gives a client's `read` scope no connections).
#[test]
fn a_client_credential_is_the_one_401_for_both_reads() {
    let f = fixture();
    seed(&f);
    let one = send(f.addr, "GET", "/nowhere", &[], b"").body;
    let token =
        "eyJhbGciOiJFUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiJjbGllbnQtMSIsInNjb3BlIjoicmVhZCJ9.c2ln";
    let bearer = format!("Bearer {token}");
    for path in [ws("/connections"), ws(&format!("/connections/{C1}"))] {
        for cookie in [None, Some(format!("{COOKIE_NAME}=not-a-session"))] {
            let mut headers = vec![("Origin", APP), ("Authorization", bearer.as_str())];
            if let Some(cookie) = cookie.as_deref() {
                headers.push(("Cookie", cookie));
            }
            let reply = send(f.addr, "GET", &path, &headers, b"");
            assert_eq!(reply.status, 401, "{path} {cookie:?}");
            assert_eq!(reply.body, one, "{path} {cookie:?}");
            assert!(
                !String::from_utf8_lossy(&reply.body).contains(C1),
                "{path} {cookie:?}"
            );
        }
    }
}

/// A viewer and an operator are refused before any id is looked up: a known id, an unknown one,
/// another workspace's, and one that does not decode all answer the same 403 (DEC-2631).
#[test]
fn the_role_is_checked_before_the_id() {
    let f = fixture();
    seed(&f);
    for who in [BOB, CAROL] {
        let cookie = signed_in(&f, who);
        let list = get(&f, &ws("/connections"), Some(&cookie));
        assert_eq!(list.status, 403, "{who}");
        assert_eq!(code(&list), "forbidden", "{who}");
        for id in [C1, "conn_01JB3K6F0C9R2V7N4M8Q1T5W5Z", FOREIGN, "conn%2Fx"] {
            let reply = get(&f, &ws(&format!("/connections/{id}")), Some(&cookie));
            assert_eq!(reply.status, 403, "{who} {id}");
            assert_eq!(reply.body, list.body, "{who} {id}");
        }
    }
}

/// For the admin, an id the control stream does not name, another workspace's id, and an id
/// that does not decode are one and the same `not_found`.
#[test]
fn an_unknown_a_foreign_and_a_malformed_id_are_the_same_404() {
    let f = fixture();
    seed(&f);
    let cookie = signed_in(&f, ALICE);
    let unknown = get(
        &f,
        &ws("/connections/conn_01JB3K6F0C9R2V7N4M8Q1T5W5Z"),
        Some(&cookie),
    );
    assert_eq!(unknown.status, 404);
    assert_eq!(code(&unknown), "not_found");
    for id in [FOREIGN.to_owned(), "conn%2Fx".to_owned(), "a".repeat(65)] {
        let reply = get(&f, &ws(&format!("/connections/{id}")), Some(&cookie));
        assert_eq!(reply.status, 404, "{id}");
        assert_eq!(reply.body, unknown.body, "{id}");
    }
}

/// A non-member, a deactivated member, and another workspace's path are the same `not_found`;
/// memberships that cannot be read are `membership_unavailable`, never an empty list.
#[test]
fn a_stranger_finds_nothing_and_unreadable_memberships_are_503() {
    let f = fixture();
    seed(&f);
    let stranger = signed_in(&f, DAVE);
    let reply = get(&f, &ws("/connections"), Some(&stranger));
    assert_eq!(reply.status, 404);
    assert_eq!(code(&reply), "not_found");
    let admin = signed_in(&f, ALICE);
    let foreign = get(
        &f,
        &format!("/v1/workspaces/{OTHER_WORKSPACE}/connections"),
        Some(&admin),
    );
    assert_eq!(foreign.status, 404);
    assert_eq!(foreign.body, reply.body);
    *f.records.lock().expect("unpoisoned") = Err(LookupFailed);
    let unreadable = get(&f, &ws("/connections"), Some(&admin));
    assert_eq!(unreadable.status, 503);
    assert_eq!(code(&unreadable), "membership_unavailable");
}

/// The list serves every connection in first-named order with the control stream's watermark,
/// and one connection is that list's member, each against [`expected`].
#[test]
fn the_admin_reads_every_connection_and_one_as_the_fold_states_them() {
    let f = fixture();
    seed(&f);
    let cookie = signed_in(&f, ALICE);
    let list = get(&f, &ws("/connections"), Some(&cookie));
    assert_eq!(list.status, 200, "{}", String::from_utf8_lossy(&list.body));
    let body = json(&list);
    assert_eq!(body.get("connections"), Some(&expected()));
    let as_of = body.get("as_of").and_then(Value::as_array).expect("as_of");
    assert_eq!(as_of.len(), 1, "the control stream alone");
    assert_eq!(
        as_of[0].get("stream_id").and_then(Value::as_str),
        Some(ctl(WORKSPACE).as_str())
    );
    let Value::Array(each) = expected() else {
        panic!("an array")
    };
    for (id, view) in [C1, C2, C3].into_iter().zip(each) {
        let reply = get(&f, &ws(&format!("/connections/{id}")), Some(&cookie));
        assert_eq!(reply.status, 200, "{id}");
        let body = json(&reply);
        assert_eq!(body.get("connection"), Some(&view), "{id}");
        assert!(body.get("as_of").is_some(), "{id}");
    }
}

/// Neither answer carries what is journaled but not served: the account reference, the user, the
/// step-up, the existing connection, or a secret-shaped member a record carries (DEC-2620 item 4).
#[test]
fn no_answer_carries_a_journaled_only_member() {
    let f = fixture();
    seed(&f);
    let cookie = signed_in(&f, ALICE);
    for path in [
        ws("/connections"),
        ws(&format!("/connections/{C1}")),
        ws(&format!("/connections/{C2}")),
        ws(&format!("/connections/{C3}")),
    ] {
        let reply = get(&f, &path, Some(&cookie));
        assert_eq!(reply.status, 200, "{path}");
        let text = String::from_utf8_lossy(&reply.body);
        assert!(!text.contains(CANARY), "{path}: {text}");
    }
}

#[test]
fn a_control_stream_that_cannot_be_read_is_503() {
    let f = fixture();
    seed(&f);
    let cookie = signed_in(&f, ALICE);
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(ctl(WORKSPACE));
    for path in [ws("/connections"), ws(&format!("/connections/{C1}"))] {
        let reply = get(&f, &path, Some(&cookie));
        assert_eq!(reply.status, 503, "{path}");
        assert_eq!(code(&reply), "membership_unavailable", "{path}");
    }
}
