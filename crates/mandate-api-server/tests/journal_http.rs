//! A stream's events page over the workspace API (workspace API spec §4.8, §4.8.1; API-9, API-15,
//! AU-1, AU-2; DEC-760): the records row's roles only, checked before any id; query members
//! checked before any id; one `not_found` for every id outside the workspace's streams; and pages
//! that concatenate to the stream and chain from 64 zeros.

mod common;

use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
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
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";

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
        "mandate-api-server-journal-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x55_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    let records: Records = Arc::new(Mutex::new(Ok(Vec::new())));
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
    };
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        records,
        sessions,
    }
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn principal(n: u8) -> PrincipalId {
    PrincipalId(u128::from(n) + 100)
}

/// A session for principal `n`, whose membership holds `roles`, or no membership when `None`.
fn member(f: &Fixture, n: u8, roles: Option<&[Role]>) -> String {
    let records = roles.map(|roles| {
        MembershipRecord::new(
            Seal::grant(),
            2,
            at(T0 - 1_000),
            MembershipEvent::Activated {
                member: principal(n),
                invitation: None,
                roles: roles.iter().copied().collect(),
                independent_approval_required: false,
                cool_off_ends_at: at(T0 - 1_000),
            },
        )
    });
    *f.records.lock().expect("unpoisoned") = Ok(records.into_iter().collect());
    f.sessions
        .open(principal(n), "sha256:aaaa".to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned()
}

fn auditor(f: &Fixture) -> String {
    member(f, 1, Some(&[Role::Auditor]))
}

fn get(f: &Fixture, cookie: &str, stream: &str, query: &str) -> Reply {
    let path = format!("/v1/workspaces/{WORKSPACE}/journal/streams/{stream}/events{query}");
    let header = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "GET",
        &path,
        &[("Origin", APP), ("Cookie", &header)],
        b"",
    )
}

fn ctl(workspace: &str) -> String {
    format!("ctl:{workspace}")
}

/// `n` rows of `stream`, each body naming the previous row's hash, the first after 64 zeros.
fn chain(stream: &str, n: u64) -> Vec<StoredEvent> {
    let mut prev = Digest::ZERO;
    (1..=n)
        .map(|seq| {
            let event_id = format!("01JBH3BTZ8K2M4N6P8R0S{seq:05}");
            let text = format!(
                r#"{{"environment":"paper","event_id":"{event_id}","event_type":"Noted","payload":{{"n":{seq}}},"prev_hash":"{prev}","seq":{seq},"stream_id":"{stream}"}}"#
            );
            let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
            let row = StoredEvent {
                stream_id: stream.to_owned(),
                seq,
                event_id,
                event_type: "Noted".to_owned(),
                schema_version: 1,
                environment: "paper".to_owned(),
                recorded_at: format!("2026-10-19T10:{:02}:{:02}.000000000Z", seq / 60, seq % 60),
                prev_hash: prev,
                hash: Digest::of(&body),
                body,
            };
            prev = row.hash;
            row
        })
        .collect()
}

fn seed(f: &Fixture, rows: Vec<StoredEvent>) {
    let stream = rows
        .first()
        .map(|row| row.stream_id.clone())
        .expect("a row");
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(stream, rows);
}

fn json(reply: &Reply) -> Value {
    parse(&reply.body).unwrap_or_else(|_| panic!("a JSON body, status {}", reply.status))
}

fn member_of<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter()
        .fold(value, |v, key| v.get(key).unwrap_or(&Value::Null))
}

fn text<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    member_of(value, path).as_str().unwrap_or_default()
}

fn int(value: &Value, path: &[&str]) -> Option<u64> {
    member_of(value, path).as_int()
}

/// RFC 4648 §4 standard base64 with padding, written here as the test's own oracle.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(
                    ALPHABET[usize::try_from((n >> (18 - 6 * i)) & 63).expect("six bits")],
                ));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// The page's events as `(seq, event_id, event_type, recorded_at, prev_hash, hash, body)`.
fn events(page: &Value) -> Vec<(u64, String, String, String, String, String, String)> {
    let Some(Value::Array(items)) = page.get("events") else {
        return Vec::new();
    };
    items
        .iter()
        .map(|e| {
            let t = |k: &str| text(e, &[k]).to_owned();
            let seq = int(e, &["seq"]).unwrap_or_default();
            (
                seq,
                t("event_id"),
                t("event_type"),
                t("recorded_at"),
                t("prev_hash"),
                t("hash"),
                t("body"),
            )
        })
        .collect()
}

fn expected(row: &StoredEvent) -> (u64, String, String, String, String, String, String) {
    (
        row.seq,
        row.event_id.clone(),
        row.event_type.clone(),
        row.recorded_at.clone(),
        row.prev_hash.to_string(),
        row.hash.to_string(),
        base64(&row.body),
    )
}

#[test]
fn a_page_serves_the_stored_bodies_after_its_cursor_with_the_head() {
    let f = fixture();
    let rows = chain(&ctl(WORKSPACE), 3);
    seed(&f, rows.clone());
    let cookie = auditor(&f);
    let reply = get(&f, &cookie, &ctl(WORKSPACE), "?after_seq=1&limit=1");
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let page = json(&reply);
    assert_eq!(text(&page, &["stream_id"]), ctl(WORKSPACE));
    assert_eq!(int(&page, &["head", "seq"]), Some(3));
    assert_eq!(
        text(&page, &["head", "hash"]),
        rows[2].hash.to_string(),
        "bare hex"
    );
    assert_eq!(text(&page, &["head", "recorded_at"]), rows[2].recorded_at);
    assert_eq!(events(&page), vec![expected(&rows[1])]);
    assert_eq!(int(&page, &["next_after_seq"]), Some(2));
    assert_eq!(member_of(&page, &["at_head"]), &Value::Bool(false));
    let as_of = member_of(&page, &["as_of"])
        .as_array()
        .map(<[Value]>::to_vec)
        .unwrap_or_default();
    assert_eq!(as_of.len(), 1, "one watermark, this stream's");
    assert_eq!(text(&as_of[0], &["stream_id"]), ctl(WORKSPACE));
    assert_eq!(int(&as_of[0], &["seq"]), Some(3));
    assert_eq!(
        text(&as_of[0], &["hash"]),
        format!("sha256:{}", rows[2].hash)
    );
    assert_eq!(text(&page, &["api_version"]), "v1");
    let whole = json(&get(&f, &cookie, &ctl(WORKSPACE), ""));
    assert_eq!(
        events(&whole),
        rows.iter().map(expected).collect::<Vec<_>>(),
        "after_seq 0"
    );
    assert_eq!(member_of(&whole, &["at_head"]), &Value::Bool(true));
}

#[test]
fn a_page_at_or_past_the_head_is_empty_at_the_head_and_never_404() {
    let f = fixture();
    let rows = chain(&ctl(WORKSPACE), 3);
    seed(&f, rows);
    let cookie = auditor(&f);
    for after in [3_u64, 4, 9_007_199_254_740_991] {
        let reply = get(&f, &cookie, &ctl(WORKSPACE), &format!("?after_seq={after}"));
        assert_eq!(reply.status, 200, "after_seq {after}");
        let page = json(&reply);
        assert_eq!(events(&page), vec![]);
        assert_eq!(int(&page, &["next_after_seq"]), Some(after));
        assert_eq!(member_of(&page, &["at_head"]), &Value::Bool(true));
        assert_eq!(int(&page, &["head", "seq"]), Some(3));
    }
}

#[test]
fn the_page_size_defaults_to_100_and_takes_up_to_1000() {
    let f = fixture();
    seed(&f, chain(&ctl(WORKSPACE), 1_001));
    let cookie = auditor(&f);
    let default = json(&get(&f, &cookie, &ctl(WORKSPACE), ""));
    assert_eq!(events(&default).len(), 100);
    assert_eq!(int(&default, &["next_after_seq"]), Some(100));
    let most = json(&get(&f, &cookie, &ctl(WORKSPACE), "?limit=1000"));
    assert_eq!(events(&most).len(), 1_000);
    assert_eq!(member_of(&most, &["at_head"]), &Value::Bool(false));
}

#[test]
fn only_the_records_roles_read_the_journal_and_a_role_is_checked_before_any_id() {
    let f = fixture();
    seed(&f, chain(&ctl(WORKSPACE), 2));
    for (n, roles) in [
        (2, &[Role::Viewer][..]),
        (3, &[Role::Operator][..]),
        (4, &[Role::Approver][..]),
    ] {
        let cookie = member(&f, n, Some(roles));
        for stream in [ctl(WORKSPACE), "agent:nope".to_owned()] {
            let reply = get(&f, &cookie, &stream, "");
            assert_eq!(reply.status, 403, "{roles:?} reading {stream}");
            assert_eq!(text(&json(&reply), &["code"]), "forbidden");
        }
    }
    for (n, role) in [(5, Role::Auditor), (6, Role::WorkspaceAdmin)] {
        let cookie = member(&f, n, Some(&[role]));
        assert_eq!(
            get(&f, &cookie, &ctl(WORKSPACE), "").status,
            200,
            "{role:?}"
        );
    }
    let stranger = member(&f, 7, None);
    let reply = get(&f, &stranger, &ctl(WORKSPACE), "");
    assert_eq!(reply.status, 404, "no membership is API-9's 404");
}

#[test]
fn every_id_outside_the_workspaces_streams_is_the_same_not_found() {
    let f = fixture();
    seed(&f, chain(&ctl(WORKSPACE), 2));
    seed(&f, chain(&ctl(OTHER_WORKSPACE), 2));
    seed(&f, chain(&format!("agent:{OTHER_WORKSPACE}:agt_a"), 1));
    let cookie = auditor(&f);
    let ids = [
        ctl(OTHER_WORKSPACE),
        format!("agent:{OTHER_WORKSPACE}:agt_a"),
        format!("agent:{WORKSPACE}:agt_absent"),
        format!("ctl:{WORKSPACE}x"),
        format!("acct:{WORKSPACE}"),
        "not-a-stream".to_owned(),
    ];
    let bodies: Vec<Vec<u8>> = ids
        .iter()
        .map(|id| {
            let reply = get(&f, &cookie, id, "");
            assert_eq!(reply.status, 404, "{id}");
            reply.body
        })
        .collect();
    assert!(
        bodies.windows(2).all(|w| w[0] == w[1]),
        "one body, byte for byte (AU-1)"
    );
    let problem = parse(&bodies[0]).expect("a problem");
    assert_eq!(text(&problem, &["code"]), "not_found");
    assert_eq!(
        member_of(&problem, &["violations"]),
        &Value::Array(Vec::new())
    );
}

#[test]
fn query_members_are_refused_before_any_id_is_resolved_and_never_clamped() {
    let f = fixture();
    seed(&f, chain(&ctl(WORKSPACE), 2));
    let cookie = auditor(&f);
    let cases = [
        ("?limit=0", "/limit"),
        ("?limit=1001", "/limit"),
        ("?limit=-1", "/limit"),
        ("?limit=ten", "/limit"),
        ("?after_seq=9007199254740992", "/after_seq"),
        ("?after_seq=-1", "/after_seq"),
        ("?after_seq=1.5", "/after_seq"),
    ];
    for stream in [
        ctl(WORKSPACE),
        ctl(OTHER_WORKSPACE),
        "not-a-stream".to_owned(),
    ] {
        for (query, path) in cases {
            let reply = get(&f, &cookie, &stream, query);
            assert_eq!(reply.status, 422, "{query} on {stream}");
            let problem = json(&reply);
            assert_eq!(text(&problem, &["code"]), "invalid");
            let violation = member_of(&problem, &["violations"])
                .as_array()
                .and_then(|v| v.first().cloned());
            assert_eq!(
                violation.as_ref().map(|v| text(v, &["path"]).to_owned()),
                Some(path.to_owned())
            );
        }
    }
}

/// Walks `stream` page by page with `sizes` (cycled) and returns every event served.
fn walk(
    f: &Fixture,
    cookie: &str,
    stream: &str,
    sizes: &[u16],
) -> Result<Vec<Value>, TestCaseError> {
    let mut after = 0_u64;
    let mut served = Vec::new();
    for size in sizes.iter().cycle().take(2_000) {
        let reply = get(
            f,
            cookie,
            stream,
            &format!("?after_seq={after}&limit={size}"),
        );
        prop_assert_eq!(reply.status, 200);
        let page = json(&reply);
        if let Some(Value::Array(items)) = page.get("events") {
            served.extend(items.iter().cloned());
        }
        after = int(&page, &["next_after_seq"]).unwrap_or_default();
        if member_of(&page, &["at_head"]) == &Value::Bool(true) {
            return Ok(served);
        }
    }
    Err(TestCaseError::fail("the walk never reached the head"))
}

#[test]
fn pages_concatenate_to_the_stream_and_chain_from_zeros() {
    let f = fixture();
    let cookie = auditor(&f);
    let config = ProptestConfig {
        cases: 48,
        failure_persistence: None,
        ..ProptestConfig::default()
    };
    let cases = (1_u64..40, proptest::collection::vec(1_u16..8, 1..5));
    let outcome = TestRunner::new(config).run(&cases, |(n, sizes)| {
        let rows = chain(&ctl(WORKSPACE), n);
        seed(&f, rows.clone());
        let served = walk(&f, &cookie, &ctl(WORKSPACE), &sizes)?;
        let seqs: Vec<u64> = served.iter().filter_map(|e| int(e, &["seq"])).collect();
        prop_assert_eq!(
            seqs,
            (1..=n).collect::<Vec<_>>(),
            "each event once, in seq order"
        );
        let mut prev = Digest::ZERO.to_string();
        for (event, row) in served.iter().zip(&rows) {
            prop_assert_eq!(text(event, &["body"]), base64(&row.body));
            prop_assert_eq!(
                text(event, &["prev_hash"]),
                prev.as_str(),
                "chained from 64 zeros"
            );
            prop_assert_eq!(text(event, &["hash"]), Digest::of(&row.body).to_string());
            prev = text(event, &["hash"]).to_owned();
        }
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}

fn one(f: &Fixture, cookie: &str, event_id: &str) -> Reply {
    let path = format!("/v1/workspaces/{WORKSPACE}/journal/events/{event_id}");
    let header = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "GET",
        &path,
        &[("Origin", APP), ("Cookie", &header)],
        b"",
    )
}

/// `stream`'s row at `seq` (after `prev`), its envelope naming `artifacts` and `config` as stored.
fn with_refs(stream: &str, seq: u64, prev: Digest, artifacts: &str, config: &str) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0T{seq:05}");
    let text = format!(
        r#"{{"artifact_refs":{artifacts},"config_refs":{config},"environment":"paper","event_id":"{event_id}","event_type":"Noted","payload":{{}},"prev_hash":"{prev}","seq":{seq},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id,
        event_type: "Noted".to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: format!("2026-10-19T11:00:{seq:02}.000000000Z"),
        prev_hash: prev,
        hash: Digest::of(&body),
        body,
    }
}

const ARTIFACTS: &str =
    r#"["sha256:1111111111111111111111111111111111111111111111111111111111111111"]"#;
const CONFIG: &str = r#"{"mandate_version":"sha256:2222222222222222222222222222222222222222222222222222222222222222"}"#;

#[test]
fn one_event_is_served_with_its_stream_and_its_refs_as_stored() {
    let f = fixture();
    let agent = format!("agent:{WORKSPACE}:agt_a");
    let first = chain(&agent, 1).remove(0);
    let second = with_refs(&agent, 2, first.hash, ARTIFACTS, CONFIG);
    seed(&f, vec![first.clone(), second.clone()]);
    let cookie = auditor(&f);
    let reply = one(&f, &cookie, &second.event_id);
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let served = json(&reply);
    let event = member_of(&served, &["event"]);
    assert_eq!(text(event, &["stream_id"]), agent);
    assert_eq!(int(event, &["seq"]), Some(2));
    assert_eq!(text(event, &["event_id"]), second.event_id);
    assert_eq!(text(event, &["event_type"]), "Noted");
    assert_eq!(text(event, &["recorded_at"]), second.recorded_at);
    assert_eq!(text(event, &["prev_hash"]), first.hash.to_string());
    assert_eq!(text(event, &["hash"]), second.hash.to_string());
    assert_eq!(text(event, &["body"]), base64(&second.body));
    assert_eq!(
        member_of(event, &["artifact_refs"]),
        &parse(ARTIFACTS.as_bytes()).expect("JSON")
    );
    assert_eq!(
        member_of(event, &["config_refs"]),
        &parse(CONFIG.as_bytes()).expect("JSON")
    );
    let as_of = member_of(&served, &["as_of"])
        .as_array()
        .map(<[Value]>::to_vec)
        .unwrap_or_default();
    assert_eq!(as_of.len(), 1, "one watermark, the event's stream");
    assert_eq!(text(&as_of[0], &["stream_id"]), agent);
    assert_eq!(int(&as_of[0], &["seq"]), Some(2));
    let bare = json(&one(&f, &cookie, &first.event_id));
    let event = member_of(&bare, &["event"]);
    assert_eq!(int(event, &["seq"]), Some(1));
    assert_eq!(
        member_of(event, &["artifact_refs"]),
        &Value::Null,
        "none stored"
    );
    assert_eq!(
        member_of(event, &["config_refs"]),
        &Value::Null,
        "none stored"
    );
}

#[test]
fn an_event_outside_the_workspaces_streams_is_the_same_not_found() {
    let f = fixture();
    let ours = chain(&ctl(WORKSPACE), 1).remove(0);
    let theirs = with_refs(&ctl(OTHER_WORKSPACE), 1, Digest::ZERO, ARTIFACTS, CONFIG);
    let prefixed = with_refs(&format!("ctl:{WORKSPACE}x"), 1, Digest::ZERO, "[]", "{}");
    seed(&f, vec![ours]);
    seed(&f, vec![theirs.clone()]);
    seed(&f, vec![prefixed.clone()]);
    let cookie = auditor(&f);
    let ids = [
        theirs.event_id,
        prefixed.event_id,
        "01JBH3BTZ8K2M4N6P8R0Z99999".to_owned(),
        "not-an-id".to_owned(),
    ];
    let bodies: Vec<Vec<u8>> = ids
        .iter()
        .map(|id| {
            let reply = one(&f, &cookie, id);
            assert_eq!(reply.status, 404, "{id}");
            reply.body
        })
        .collect();
    assert!(
        bodies.windows(2).all(|w| w[0] == w[1]),
        "one body, byte for byte (AU-1)"
    );
    assert_eq!(
        text(&parse(&bodies[0]).expect("a problem"), &["code"]),
        "not_found"
    );
}

#[test]
fn only_the_records_roles_read_one_event_whatever_the_id() {
    let f = fixture();
    let ours = chain(&ctl(WORKSPACE), 1).remove(0);
    seed(&f, vec![ours.clone()]);
    for (n, role) in [(2, Role::Viewer), (3, Role::Operator), (4, Role::Approver)] {
        let cookie = member(&f, n, Some(&[role]));
        for id in [ours.event_id.as_str(), "not-an-id"] {
            let reply = one(&f, &cookie, id);
            assert_eq!(reply.status, 403, "{role:?} reading {id}");
            assert_eq!(text(&json(&reply), &["code"]), "forbidden");
        }
    }
    let admin = member(&f, 5, Some(&[Role::WorkspaceAdmin]));
    assert_eq!(one(&f, &admin, &ours.event_id).status, 200);
    let stranger = member(&f, 6, None);
    assert_eq!(one(&f, &stranger, &ours.event_id).status, 404, "API-9");
}

/// Journal spec v0.53 §2: the deployment meter stream is no workspace's. A member who reads the
/// workspace's records reads neither its events nor any one of them, even when its deployment id
/// is the workspace's own segment: both are the one `not_found`, as any id outside the workspace's
/// streams is.
#[test]
fn the_deployment_meter_stream_is_never_served_whatever_its_id() {
    let f = fixture();
    let stream = format!("dmeter:{WORKSPACE}");
    let rows = chain(&stream, 2);
    let event_id = rows[0].event_id.clone();
    seed(&f, rows);
    let cookie = auditor(&f);
    let page = get(&f, &cookie, &stream, "");
    assert_eq!(page.status, 404, "the stream's events");
    assert_eq!(text(&json(&page), &["code"]), "not_found");
    let event = one(&f, &cookie, &event_id);
    assert_eq!(event.status, 404, "one of its events");
    assert_eq!(text(&json(&event), &["code"]), "not_found");
}
