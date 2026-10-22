//! The read routes (§4.3, §4.7, API-1, API-2, API-9, API-14; DEC-979): authenticated by the
//! session, authorized from the control stream's memberships, and served from the journal alone.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, ORG, Reply, WORKSPACE, hosted, send, serve};
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
/// A whole `content.action`, which every served approval binds (DEC-1021).
const ACTION: &str = r#""action":{"instrument":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","asset_class":"us_equity","side":"buy","qty":"3","limit":"101.5","order_usd":"304.5","purpose":"open"}"#;

const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";

type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    records: Records,
    sessions: Arc<Sessions>,
    clock: Arc<common::ManualClock>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture() -> Fixture {
    fixture_with(None)
}

fn fixture_with(
    artifacts: Option<Arc<dyn mandate_journal::ArtifactSource + Send + Sync>>,
) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-reads-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x44_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let journal = Arc::new(MemJournal::default());
    let records: Records = Arc::new(Mutex::new(Ok(Vec::new())));
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
        artifacts,
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
        clock,
    }
}

fn alice() -> PrincipalId {
    PrincipalId::from_ulid_text("01JB3K6F0C9R2V7N4M8Q1T5W3Z").expect("a ULID")
}

fn bob() -> PrincipalId {
    PrincipalId::from_ulid_text("01JB3K6F0C9R2V7N4M8Q1T5W41").expect("a ULID")
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

/// `member` activated by the founding grant with `roles`, effective since before T0.
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

fn members(f: &Fixture, records: Result<Vec<MembershipRecord>, LookupFailed>) {
    *f.records.lock().expect("unpoisoned") = records;
}

fn signed_in(f: &Fixture, who: PrincipalId) -> String {
    f.sessions
        .open(who, "sha256:aaaa".to_owned(), None, at(f.clock.secs()))
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

fn row(stream: &str, seq: u64, event_type: &str, payload: &str) -> StoredEvent {
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
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: format!("2026-10-17T10:00:{seq:02}.000000000Z"),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

fn version(n: u8) -> String {
    format!("sha256:{}", format!("{n:02x}").repeat(32))
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn agent_stream(agent: &str) -> String {
    format!("agent:{WORKSPACE}:{agent}")
}

/// A workspace with two agents: `agt_a` paused with two asks (one pending), `agt_b` not yet
/// reporting.
fn seed(f: &Fixture) {
    let deploy = |seq, agent: &str, n| {
        row(
            &ctl(),
            seq,
            "AgentDeployed",
            &format!(
                r#"{{"agent_id":"{agent}","mandate_version":"{}","record_ref":"{}"}}"#,
                version(n),
                version(0xee)
            ),
        )
    };
    let a = agent_stream("agt_a");
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    streams.insert(
        ctl(),
        vec![
            row(&ctl(), 1, "StreamOpened", "{}"),
            deploy(2, "agt_a", 1),
            deploy(3, "agt_b", 2),
        ],
    );
    streams.insert(
        a.clone(),
        vec![
            row(
                &a,
                1,
                "AgentModeChanged",
                r#"{"from":"normal","to":"paused","reason":"x"}"#,
            ),
            row(
                &a,
                2,
                "ApprovalRequested",
                &format!(
                    r#"{{"content":{{{ACTION}}},"content_hash":"{}","deadline":900}}"#,
                    version(7)
                ),
            ),
            row(
                &a,
                3,
                "ApprovalRequested",
                &format!(
                    r#"{{"content":{{{ACTION}}},"content_hash":"{}","deadline":800}}"#,
                    version(8)
                ),
            ),
            row(
                &a,
                4,
                "ApprovalTimedOut",
                r#"{"approval":"01JBH3BTZ8K2M4N6P8R0S2T003","on_timeout":"skip"}"#,
            ),
        ],
    );
}

fn json(reply: &Reply) -> Value {
    parse(&reply.body).expect("a JSON body")
}

fn text<'v>(value: &'v Value, key: &str) -> &'v str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{key}"))
}

fn unauthenticated(f: &Fixture) -> Vec<u8> {
    send(f.addr, "GET", "/nowhere", &[], b"").body
}

const READS: [&str; 4] = [
    "/agents",
    "/agents/agt_a",
    "/approvals",
    "/approvals/01JBH3BTZ8K2M4N6P8R0S2T002",
];

#[test]
fn every_read_without_a_live_session_is_the_one_401() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let one = unauthenticated(&f);
    let cookie = signed_in(&f, alice());
    f.clock.set(T0 + 301);
    for path in READS {
        for presented in [None, Some("forged"), Some(cookie.as_str())] {
            let reply = get(&f, &ws(path), presented);
            assert_eq!(reply.status, 401, "{path} {presented:?}");
            assert_eq!(reply.body, one, "{path}");
        }
    }
}

#[test]
fn the_agents_list_states_each_deployed_agent_from_the_journal() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Viewer])]));
    let cookie = signed_in(&f, alice());
    let reply = get(&f, &ws("/agents"), Some(&cookie));
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("content-type"), Some("application/json"));
    assert_eq!(reply.header("access-control-allow-origin"), Some(APP));
    let body = json(&reply);
    assert_eq!(text(&body, "api_version"), "v1");
    assert!(text(&body, "build").starts_with("sha256:"));
    assert_eq!(text(&body, "build").len(), 71);
    assert_eq!(text(&body, "served_at"), "2026-10-03T04:00:00.000000000Z");
    let expected = parse(
        format!(
            r#"[{{"agent_id":"agt_a","deployed_at":"2026-10-17T10:00:02.000000000Z","environment":"paper","label":null,"mandate":null,"mandate_version":"{}","mode":"paused","pending_approvals":1}},{{"agent_id":"agt_b","deployed_at":"2026-10-17T10:00:03.000000000Z","environment":"paper","label":null,"mandate":null,"mandate_version":"{}","mode":null,"pending_approvals":0}}]"#,
            version(1),
            version(2)
        )
        .as_bytes(),
    )
    .expect("JSON");
    assert_eq!(body.get("agents"), Some(&expected));
    let as_of = body.get("as_of").expect("as_of");
    let streams: Vec<&str> = match as_of {
        Value::Array(marks) => marks.iter().map(|m| text(m, "stream_id")).collect(),
        _ => panic!("as_of is a list"),
    };
    assert_eq!(
        streams,
        vec![ctl().as_str(), agent_stream("agt_a").as_str()]
    );
    let control = match as_of {
        Value::Array(marks) => &marks[0],
        _ => unreachable!(),
    };
    let rows = f.journal.streams.lock().expect("unpoisoned")[&ctl()].clone();
    assert_eq!(control.get("seq").and_then(Value::as_int), Some(3));
    assert_eq!(text(control, "hash"), format!("sha256:{}", rows[2].hash));
    assert_eq!(text(control, "recorded_at"), rows[2].recorded_at);
}

#[test]
fn one_agent_reads_alone_and_an_unknown_or_malformed_id_is_404() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    let reply = get(&f, &ws("/agents/agt_b"), Some(&cookie));
    assert_eq!(reply.status, 200);
    let body = json(&reply);
    assert_eq!(text(body.get("agent").expect("agent"), "agent_id"), "agt_b");
    assert_eq!(
        body.get("agent").and_then(|a| a.get("mode")),
        Some(&Value::Null)
    );
    let missing = get(&f, &ws("/agents/agt_zz"), Some(&cookie));
    assert_eq!(missing.status, 404);
    assert_eq!(missing.header("content-type"), Some(PROBLEM_JSON));
    for path in [
        format!("/agents/{}", "a".repeat(65)),
        "/agents/agt%2Fa".to_owned(),
    ] {
        let reply = get(&f, &ws(&path), Some(&cookie));
        assert_eq!(reply.status, 404, "{path}");
        assert_eq!(reply.body, missing.body, "{path}");
    }
}

#[test]
fn a_foreign_absent_or_unjoined_workspace_is_the_same_404() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, bob(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    let unjoined = get(&f, &ws("/agents"), Some(&cookie));
    assert_eq!(unjoined.status, 404, "alice holds no membership here");
    let body = String::from_utf8_lossy(&unjoined.body).into_owned();
    assert!(body.contains(r#""code":"not_found""#), "{body}");
    for path in [
        format!("/v1/workspaces/{OTHER_WORKSPACE}/agents"),
        "/v1/workspaces/not-a-ulid/agents".to_owned(),
        format!("/v1/workspaces/{OTHER_WORKSPACE}/approvals/01JBH3BTZ8K2M4N6P8R0S2T002"),
    ] {
        let reply = get(&f, &path, Some(&cookie));
        assert_eq!(reply.status, 404, "{path}");
        assert_eq!(reply.body, unjoined.body, "{path}");
    }
}

#[test]
fn a_member_without_the_view_row_is_403_and_an_inactive_one_404() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[])]));
    let cookie = signed_in(&f, alice());
    let reply = get(&f, &ws("/agents"), Some(&cookie));
    assert_eq!(reply.status, 403);
    assert!(String::from_utf8_lossy(&reply.body).contains(r#""code":"forbidden""#));
    let deactivated = MembershipRecord::new(
        Seal::grant(),
        2,
        at(T0 - 10),
        MembershipEvent::Deactivated { member: alice() },
    );
    members(
        &f,
        Ok(vec![activated(1, alice(), &[Role::Operator]), deactivated]),
    );
    assert_eq!(get(&f, &ws("/agents"), Some(&cookie)).status, 404);
}

#[test]
fn a_membership_that_cannot_be_read_is_503_never_empty() {
    let f = fixture();
    seed(&f);
    members(&f, Err(LookupFailed));
    let cookie = signed_in(&f, alice());
    let reply = get(&f, &ws("/agents"), Some(&cookie));
    assert_eq!(reply.status, 503);
    let body = String::from_utf8_lossy(&reply.body).into_owned();
    assert!(
        body.contains(r#""code":"membership_unavailable""#),
        "{body}"
    );
    assert!(body.contains(r#""retryable":true"#), "{body}");
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    f.journal.failing.lock().expect("unpoisoned").insert(ctl());
    let reply = get(&f, &ws("/agents"), Some(&cookie));
    assert_eq!(
        reply.status, 503,
        "the control stream is the membership store"
    );
    assert!(String::from_utf8_lossy(&reply.body).contains("membership_unavailable"));
}

#[test]
fn an_agent_stream_that_cannot_be_read_is_503_journal_unavailable() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(agent_stream("agt_a"));
    for path in READS {
        let reply = get(&f, &ws(path), Some(&cookie));
        assert_eq!(reply.status, 503, "{path}");
        let body = String::from_utf8_lossy(&reply.body).into_owned();
        assert!(
            body.contains(r#""code":"journal_unavailable""#),
            "{path}: {body}"
        );
    }
}

#[test]
fn the_approvals_inbox_and_one_approval_read_from_the_agent_streams() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Approver])]));
    let cookie = signed_in(&f, alice());
    let ids = |path: &str| -> Vec<String> {
        let reply = get(&f, &ws(path), Some(&cookie));
        assert_eq!(reply.status, 200, "{path}");
        match json(&reply).get("approvals") {
            Some(Value::Array(list)) => list
                .iter()
                .map(|a| {
                    assert!(a.get("content").is_none(), "the inbox carries no content");
                    assert_eq!(a.get("order"), Some(&bound_order()), "but its order");
                    text(a, "approval_id").to_owned()
                })
                .collect(),
            _ => panic!("approvals"),
        }
    };
    let pending = "01JBH3BTZ8K2M4N6P8R0S2T002".to_owned();
    let timed_out = "01JBH3BTZ8K2M4N6P8R0S2T003".to_owned();
    assert_eq!(ids("/approvals"), vec![pending.clone(), timed_out.clone()]);
    assert_eq!(ids("/approvals?state=pending"), vec![pending.clone()]);
    assert_eq!(ids("/approvals?state=resolved"), vec![timed_out.clone()]);
    let reply = get(&f, &ws(&format!("/approvals/{timed_out}")), Some(&cookie));
    assert_eq!(reply.status, 200);
    let one = json(&reply);
    let approval = one.get("approval").expect("approval");
    assert_eq!(text(approval, "state"), "timed_out");
    assert_eq!(text(approval, "agent_id"), "agt_a");
    assert_eq!(approval.get("deadline").and_then(Value::as_int), Some(800));
    assert_eq!(text(approval, "content_hash"), version(8));
    assert_eq!(
        approval.get("content"),
        Some(&parse(format!("{{{ACTION}}}").as_bytes()).expect("JSON"))
    );
    assert_eq!(approval.get("order"), Some(&bound_order()));
    for absent in ["01JBH3BTZ8K2M4N6P8R0S2T099", "not-an-id"] {
        assert_eq!(
            get(&f, &ws(&format!("/approvals/{absent}")), Some(&cookie)).status,
            404,
            "{absent}"
        );
    }
}

#[test]
fn an_unknown_state_or_query_member_is_invalid() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    for query in [
        "?state=open",
        "?state=",
        "?status=pending",
        "?state=pending&state=resolved",
    ] {
        let reply = get(&f, &ws(&format!("/approvals{query}")), Some(&cookie));
        assert_eq!(reply.status, 422, "{query}");
        assert!(
            String::from_utf8_lossy(&reply.body).contains(r#""code":"invalid""#),
            "{query}"
        );
    }
}

#[test]
fn a_reduction_only_session_reads_nothing() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    assert_eq!(
        f.sessions.refresh(
            &cookie,
            mandate_api_server::session::Answer::Unreachable,
            at(T0 + 1)
        ),
        Ok(None)
    );
    let reply = get(&f, &ws("/agents"), Some(&cookie));
    assert_eq!(
        reply.status, 403,
        "an outage session reaches pause and the kill switch only"
    );
    assert!(String::from_utf8_lossy(&reply.body).contains(r#""code":"reduction_only""#));
    assert_eq!(ORG.len(), 26);
}

#[test]
fn a_path_that_does_not_decode_is_the_one_401_or_the_404() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    let one = unauthenticated(&f);
    for path in [
        "/v1/workspaces/%FF/agents",
        "/v1/workspaces/%FF/approvals",
        &format!("/v1/workspaces/{WORKSPACE}/agents/%FF"),
        &format!("/v1/workspaces/{WORKSPACE}/approvals/%FF"),
    ] {
        let without = get(&f, path, None);
        assert_eq!(without.status, 401, "{path}");
        assert_eq!(without.body, one, "{path}");
        let with = get(&f, path, Some(&cookie));
        assert_eq!(with.status, 404, "{path}");
        assert!(String::from_utf8_lossy(&with.body).contains(r#""code":"not_found""#));
    }
}

#[test]
fn an_undecodable_path_on_a_reduction_only_session_is_reduction_only() {
    let f = fixture();
    seed(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    assert_eq!(
        f.sessions.refresh(
            &cookie,
            mandate_api_server::session::Answer::Unreachable,
            at(T0 + 1)
        ),
        Ok(None)
    );
    let reply = get(&f, "/v1/workspaces/%FF/agents", Some(&cookie));
    assert_eq!(reply.status, 403);
    assert!(String::from_utf8_lossy(&reply.body).contains(r#""code":"reduction_only""#));
}

/// The order [`ACTION`] binds, as `read-models/approval` states it: no execution plan in a
/// version-1 request.
fn bound_order() -> Value {
    parse(br#"{"instrument":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","asset_class":"us_equity","side":"buy","qty":"3","limit":"101.5","order_usd":"304.5","purpose":"open","tif":null,"protection":null}"#)
        .expect("JSON")
}

const DOCUMENT: &str = include_str!("fixtures/mandate.json");

/// The mandate document in canonical form, and the version that names it.
fn document() -> (Vec<u8>, String) {
    let bytes = to_canonical(&parse(DOCUMENT.as_bytes()).expect("JSON"));
    let version = format!("sha256:{}", Digest::of(&bytes).to_hex());
    (bytes, version)
}

/// `agt_a` deployed with the stored document's version, `agt_b` with one the store lacks, the
/// document's connection established at version 2 on `acct_1`, and two reconciled holdings.
fn seed_documents(f: &Fixture, deployed_version: &str) {
    let mut established = row(
        &ctl(),
        4,
        "ConnectionEstablished",
        r#"{"connection_id":"conn_alpaca_paper_01","account_ref":"acct_1"}"#,
    );
    established.schema_version = 2;
    let acct = format!("acct:{WORKSPACE}:acct_1");
    let observed = |seq, instrument: &str, qty: &str| {
        row(
            &acct,
            seq,
            "BrokerPositionObserved",
            &format!(
                r#"{{"instrument":"{instrument}","broker_qty":"{qty}","model_qty":"{qty}","mismatch":false,"risk_clock":"2026-10-17T10:00:00Z"}}"#
            ),
        )
    };
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    streams.insert(
        ctl(),
        vec![
            row(&ctl(), 1, "StreamOpened", "{}"),
            row(
                &ctl(),
                2,
                "AgentDeployed",
                &format!(r#"{{"agent_id":"agt_a","mandate_version":"{deployed_version}"}}"#),
            ),
            row(
                &ctl(),
                3,
                "AgentDeployed",
                &format!(
                    r#"{{"agent_id":"agt_b","mandate_version":"{}"}}"#,
                    version(2)
                ),
            ),
            established,
        ],
    );
    streams.insert(
        acct.clone(),
        vec![
            row(&acct, 1, "StreamOpened", "{}"),
            observed(2, "b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081", "3"),
        ],
    );
}

fn envelope_value() -> Value {
    parse(br#"{"allocation_usd":"1000","max_loss_from_allocation":"0.1","goal":"continuous","goal_end_date":null,"asset_classes":["us_equity"],"max_position_usd":"1000","max_order_usd":"300","max_orders_per_day":50,"max_daily_loss":"0.02","max_drawdown":"0.08"}"#)
        .expect("JSON")
}

fn stored(bytes: Vec<u8>) -> Option<Arc<dyn mandate_journal::ArtifactSource + Send + Sync>> {
    Some(Arc::new(std::collections::BTreeMap::from([(
        Digest::of(&bytes),
        bytes,
    )])))
}

#[test]
fn an_agent_carries_its_mandates_envelope_and_its_accounts_positions() {
    let (bytes, version) = document();
    let f = fixture_with(stored(bytes));
    seed_documents(&f, &version);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    let list = json(&get(&f, &ws("/agents"), Some(&cookie)));
    let Some(Value::Array(agents)) = list.get("agents") else {
        panic!("agents");
    };
    assert_eq!(
        agents[0].get("label"),
        Some(&Value::Str("tracer-aapl".into()))
    );
    assert_eq!(agents[0].get("mandate"), Some(&envelope_value()));
    assert_eq!(
        agents[0].get("account_positions"),
        None,
        "the list carries no positions"
    );
    assert_eq!(
        agents[1].get("label"),
        Some(&Value::Null),
        "a document the store lacks"
    );
    assert_eq!(agents[1].get("mandate"), Some(&Value::Null));

    let one = json(&get(&f, &ws("/agents/agt_a"), Some(&cookie)));
    let agent = one.get("agent").expect("agent");
    assert_eq!(agent.get("label"), Some(&Value::Str("tracer-aapl".into())));
    let positions = parse(
        br#"[{"instrument":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","qty":"3","broker_qty":"3","mismatch":false,"observed_at":"2026-10-17T10:00:02.000000000Z"}]"#,
    )
    .expect("JSON");
    assert_eq!(agent.get("account_positions"), Some(&positions));
    let Some(Value::Array(as_of)) = one.get("as_of") else {
        panic!("as_of");
    };
    let streams: Vec<&str> = as_of.iter().map(|w| text(w, "stream_id")).collect();
    let acct = format!("acct:{WORKSPACE}:acct_1");
    assert!(streams.contains(&acct.as_str()), "{streams:?}");

    let other = json(&get(&f, &ws("/agents/agt_b"), Some(&cookie)));
    assert_eq!(
        other.get("agent").and_then(|a| a.get("account_positions")),
        Some(&Value::Null),
        "no document, so no connection to read"
    );

    f.journal.failing.lock().expect("unpoisoned").insert(acct);
    let unread = get(&f, &ws("/agents/agt_a"), Some(&cookie));
    assert_eq!(unread.status, 503);
    assert_eq!(text(&json(&unread), "code"), "journal_unavailable");
}

#[test]
fn without_a_store_nothing_from_the_document_is_reported() {
    let (_, version) = document();
    let f = fixture();
    seed_documents(&f, &version);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    let cookie = signed_in(&f, alice());
    let one = json(&get(&f, &ws("/agents/agt_a"), Some(&cookie)));
    let agent = one.get("agent").expect("agent");
    for member in ["label", "mandate", "account_positions"] {
        assert_eq!(agent.get(member), Some(&Value::Null), "{member}");
    }
}

/// `row` with its envelope's `causation_id` set to `cause`, re-encoded and re-hashed.
fn caused(mut row: StoredEvent, cause: &str) -> StoredEvent {
    let mut envelope = parse(&row.body).expect("JSON");
    if let Value::Object(members) = &mut envelope {
        members.insert(
            mandate_canon::Key::new("causation_id").expect("a key"),
            Value::Str(cause.into()),
        );
    }
    row.body = to_canonical(&envelope);
    row.hash = Digest::of(&row.body);
    row
}

/// `agt_a`'s order `o-1` and another agent's `o-2` on `acct_1`, after `seed_documents`'s rows.
fn seed_orders(f: &Fixture) {
    let acct = format!("acct:{WORKSPACE}:acct_1");
    let companion = |seq, agent: &str| {
        row(
            &acct,
            seq,
            "OrderRequestRecorded",
            &format!(
                r#"{{"agent_id":"{agent}","intent_id":null,"purpose":"open","extended_hours":false,"stop_price":null,"order_class":null,"take_profit":null,"stop":null,"rung":null,"at_floor":null,"risk_clock":"2026-10-17T10:00:00Z"}}"#
            ),
        )
    };
    let submitted = |seq, id: &str, cause: &StoredEvent| {
        let mut row = caused(
            row(
                &acct,
                seq,
                "OrderSubmitted",
                &format!(
                    r#"{{"client_order_id":"{id}","attempt":1,"instrument_id":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","side":"buy","type":"limit","tif":"day","qty":"3","limit_price":"101.5","risk_clock":"2026-10-17T10:00:00Z"}}"#
                ),
            ),
            &cause.event_id,
        );
        row.schema_version = 2;
        row
    };
    let mine = companion(3, "agt_a");
    let theirs = companion(5, "agt_b");
    let rows = vec![
        mine.clone(),
        submitted(4, "o-1", &mine),
        theirs.clone(),
        submitted(6, "o-2", &theirs),
    ];
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .get_mut(&acct)
        .expect("seeded")
        .extend(rows);
}

#[test]
fn an_agents_orders_are_its_own_on_its_documents_account() {
    let (bytes, version) = document();
    let f = fixture_with(stored(bytes));
    seed_documents(&f, &version);
    seed_orders(&f);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Viewer])]));
    let cookie = signed_in(&f, alice());
    let reply = get(&f, &ws("/agents/agt_a/orders"), Some(&cookie));
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let served = json(&reply);
    assert_eq!(text(&served, "agent_id"), "agt_a");
    assert_eq!(text(&served, "account_ref"), "acct_1");
    let Some(Value::Array(orders)) = served.get("orders") else {
        panic!("orders");
    };
    let ids: Vec<&str> = orders.iter().map(|o| text(o, "client_order_id")).collect();
    assert_eq!(
        ids,
        ["o-1"],
        "agt_b's order on the same account is not served"
    );
    let Some(Value::Array(as_of)) = served.get("as_of") else {
        panic!("as_of");
    };
    let streams: Vec<&str> = as_of.iter().map(|w| text(w, "stream_id")).collect();
    let acct = format!("acct:{WORKSPACE}:acct_1");
    assert_eq!(streams, [ctl().as_str(), acct.as_str()]);

    let undocumented = json(&get(&f, &ws("/agents/agt_b/orders"), Some(&cookie)));
    assert_eq!(
        undocumented.get("account_ref"),
        Some(&Value::Null),
        "no document to name it"
    );
    assert_eq!(undocumented.get("orders"), Some(&Value::Array(Vec::new())));

    let absent = get(&f, &ws("/agents/agt_x/orders"), Some(&cookie));
    assert_eq!(
        absent.status, 404,
        "an agent the control stream does not name"
    );

    f.journal.failing.lock().expect("unpoisoned").insert(acct);
    let unread = get(&f, &ws("/agents/agt_a/orders"), Some(&cookie));
    assert_eq!(unread.status, 503);
    assert_eq!(text(&json(&unread), "code"), "journal_unavailable");

    members(&f, Ok(Vec::new()));
    let stranger = get(&f, &ws("/agents/agt_a/orders"), Some(&cookie));
    assert_eq!(stranger.status, 404, "no membership is API-9's 404");
}

#[test]
fn an_agents_positions_are_its_own_fills_beside_the_accounts_holdings() {
    let (bytes, version) = document();
    let f = fixture_with(stored(bytes));
    seed_documents(&f, &version);
    seed_orders(&f);
    let acct = format!("acct:{WORKSPACE}:acct_1");
    let fill = |seq, id: &str, qty: &str| {
        row(
            &acct,
            seq,
            "FillApplied",
            &format!(
                r#"{{"fill_id":"f-{seq}","client_order_id":"{id}","instrument_id":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","side":"buy","qty_gross":"{qty}","price":"101.5","trade_date":"2026-10-17","risk_clock":"2026-10-17T10:00:00Z","fees":[]}}"#
            ),
        )
    };
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .get_mut(&acct)
        .expect("seeded")
        .extend([fill(7, "o-1", "2"), fill(8, "o-2", "1")]);
    members(&f, Ok(vec![activated(1, alice(), &[Role::Viewer])]));
    let cookie = signed_in(&f, alice());
    let reply = get(&f, &ws("/agents/agt_a/positions"), Some(&cookie));
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let served = json(&reply);
    assert_eq!(text(&served, "agent_id"), "agt_a");
    assert_eq!(text(&served, "account_ref"), "acct_1");
    let positions = parse(
        br#"[{"instrument_id":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","filled_qty":"2","cost_basis":null,"risk_mark":null,"protection":null}]"#,
    )
    .expect("JSON");
    assert_eq!(
        served.get("positions"),
        Some(&positions),
        "agt_b's fill is not agt_a's"
    );
    let holdings = parse(
        br#"[{"instrument":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","qty":"3","broker_qty":"3","mismatch":false,"observed_at":"2026-10-17T10:00:02.000000000Z"}]"#,
    )
    .expect("JSON");
    assert_eq!(
        served.get("account_positions"),
        Some(&holdings),
        "the account's, as on the agent read"
    );
    let Some(Value::Array(as_of)) = served.get("as_of") else {
        panic!("as_of");
    };
    let streams: Vec<&str> = as_of.iter().map(|w| text(w, "stream_id")).collect();
    assert_eq!(streams, [ctl().as_str(), acct.as_str()]);

    let undocumented = json(&get(&f, &ws("/agents/agt_b/positions"), Some(&cookie)));
    assert_eq!(undocumented.get("account_ref"), Some(&Value::Null));
    assert_eq!(
        undocumented.get("positions"),
        Some(&Value::Array(Vec::new()))
    );
    assert_eq!(undocumented.get("account_positions"), Some(&Value::Null));
    assert_eq!(
        get(&f, &ws("/agents/agt_x/positions"), Some(&cookie)).status,
        404
    );

    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .get_mut(&acct)
        .expect("seeded")
        .push(row(
            &acct,
            9,
            "FillApplied",
            r#"{"fill_id":"f-9","client_order_id":"o-1","instrument_id":"b0c2a3e4-5f60-4718-9a2b-3c4d5e6f7081","side":"short","qty_gross":"1","price":"1","trade_date":"2026-10-17","risk_clock":"2026-10-17T10:00:00Z","fees":[]}"#,
        ));
    let unreadable = get(&f, &ws("/agents/agt_a/positions"), Some(&cookie));
    assert_eq!(unreadable.status, 503, "no position from part of its fills");
    assert_eq!(text(&json(&unreadable), "code"), "journal_unavailable");
}
