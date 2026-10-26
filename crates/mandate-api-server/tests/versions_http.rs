//! Versions, the confirmation screen, confirm, deploy, and go-live over HTTP (§3.11 item 11, §4.1,
//! §4.2, §5.1, API-3, API-4, API-5, API-13, API-17, API-19; DEC-2101): checked in §3.3's order,
//! judged as the CLI judges them, step-up through the port last, and recorded on the control
//! stream, with the record in the store, before the answer. Every expected id, version, digest,
//! and member below is computed here from the spec's own definitions, not by the server's code.
//!
//! A confirm under passkey step-up ([DEC-2128] items 1 to 3) runs against the server's real
//! binding, its challenge from the challenge route and signed by a software authenticator.
//!
//! [DEC-2128]: ../../../docs/project/decisions/DEC-2128.md

mod common;
mod support_passkey;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, Scripted, WORKSPACE, hosted, send, serve};
use mandate_api::envelope::StepUpEvidence;
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::problem::{CurrentBase, Effect, Problem, ProblemCode, Violation};
use mandate_api::wire::{Id, decode};
use mandate_api_server::challenges::Challenges;
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::confirm::CliConfirm;
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::methods::{Methods, Passkey};
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::stepup::{Action, Judged, StepUp, Unbound};
use mandate_api_server::subject::{LinkRefusal, NamesPrincipal};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::MembershipState::Active;
use mandate_identity::SessionKind::Full;
use mandate_identity::{
    Authorized, MembershipEvent, MembershipRecord, OrgId, Permission, Principal, PrincipalId, Role,
    Scope, SessionRef, StepUpActionKind, WorkspaceId, authorize,
};
use mandate_identity_seal::Seal;
use mandate_identity_testkit::{StaticLookup, membership, session};
use mandate_journal::{ArtifactRef, ArtifactSource, Draft, StoredEvent, check_batch, get_artifact};
use mandate_time::UtcNanos;
use support_passkey::{CREDENTIAL, Signer, enrol, enrolled, unb64url};

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const KEY: &str = "k1Z-9_aaaaaaaaaa";
const KEY_2: &str = "k2Z-9_bbbbbbbbbb";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const MANDATE: &str = include_str!("../../mandate-cli/tests/fixtures/spy_mandate.json");
const SPY: &str = r#"{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"arca","increment":"whole","instrument_id":"b28f4066-5c6d-479b-a2af-85dc1a8f16fb","symbol":"SPY"}"#;
const MODEL: &str = r#"{"admits_instruments":false,"content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#;
const CONNECTION: &str = "conn_alpaca_paper_01";
const ACCOUNT: &str = "acct1";
/// The fixture's top-level members but the two system fields (mandate spec §4.1 V-020).
const PATHS: [&str; 11] = [
    "/autonomy",
    "/behavior",
    "/capital",
    "/connection_id",
    "/environment",
    "/goal",
    "/name",
    "/notifications",
    "/protection",
    "/risk",
    "/universe",
];
const RECORD: &str = r#"{"artifact":"sha256:abababababababababababababababababababababababababababababababab","ui_build":"sha256:cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd"}"#;
const EVIDENCE: &str = r#"{"assertion_id":"01JBH3BTZ8K2M4N6P8R0S2T4V6","authenticated_at":"2026-10-08T14:30:00.000000000Z","method":"passkey"}"#;

/// A port that accepts every assertion and remembers each action it was asked about.
#[derive(Default)]
struct Accepting(Mutex<Vec<(StepUpActionKind, Digest)>>);

impl StepUp for Accepting {
    fn verify(
        &self,
        _evidence: &StepUpEvidence,
        action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        self.0
            .lock()
            .expect("unpoisoned")
            .push((action.kind, action.digest));
        Ok(())
    }
}

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    store: Arc<Objects<FsArtifactStore>>,
    sessions: Arc<Sessions>,
    asked: Arc<Accepting>,
    challenges: Arc<Challenges>,
}

/// Subject links that name no one, so no `cli_confirm` evidence counts.
struct Unlinked;

impl NamesPrincipal for Unlinked {
    fn names(&self, _who: PrincipalId) -> Result<bool, LinkRefusal> {
        Ok(false)
    }
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// Which port, and whether the deployment names a store.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Port {
    Accepting,
    Unbound,
    /// The server's real binding, as `main.rs` binds it (DEC-2126 item 2).
    Methods,
}

fn fixture() -> Fixture {
    built(Port::Accepting, true)
}

fn built(port: Port, with_store: bool) -> Fixture {
    built_with(
        port,
        with_store,
        vec![
            activated(1, alice(), &[Role::Operator]),
            activated(2, bob(), &[Role::Viewer]),
        ],
    )
}

fn built_with(port: Port, with_store: bool, records: Vec<MembershipRecord>) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-versions-{}-{n}",
        std::process::id()
    ));
    drop(n);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a directory");
    let file = dir.join(SESSION_KEY_FILE);
    std::fs::write(&file, [0x45_u8; 32]).expect("the key");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).expect("mode");
    let key = read_session_key(&dir).expect("the key reads");
    let store = Arc::new(Objects::new(
        FsArtifactStore::open(dir.join("store")).expect("a store"),
    ));
    let journal = Arc::new(MemJournal::default());
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let clock = common::ManualClock::at(T0);
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let asked = Arc::new(Accepting::default());
    let challenges: Arc<Challenges> = Arc::default();
    let step_up: Arc<dyn StepUp> = match port {
        Port::Accepting => Arc::clone(&asked) as Arc<dyn StepUp>,
        Port::Unbound => Arc::new(Unbound),
        Port::Methods => Arc::new(Methods {
            passkey: Passkey {
                challenges: Arc::clone(&challenges),
                rp: support_passkey::rp(),
                keys: Some(Arc::clone(&store) as Arc<dyn ArtifactSource + Send + Sync>),
            },
            cli_confirm: CliConfirm {
                challenges: Arc::clone(&challenges),
                links: Arc::new(Unlinked),
            },
        }),
    };
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: clock as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| Ok(records.clone())),
        hosted: hosted(),
        artifacts: None,
        objects: with_store.then(|| Arc::clone(&store) as Arc<dyn ObjectStore>),
        step_up,
        challenges: Arc::clone(&challenges),
        connect: None,
        reduction: Arc::default(),
    };
    let f = Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        store,
        sessions,
        asked,
        challenges,
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
        .open(who, "sha256:aaaa".to_owned(), None, at(T0))
        .expect("opened")
        .value()
        .to_owned()
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn account() -> String {
    format!("acct:{WORKSPACE}:{ACCOUNT}")
}

fn json(text: &str) -> Value {
    parse(text.as_bytes()).expect("JSON")
}

/// `sha256:` and the digest of `text`'s canonical form.
fn reference(text: &str) -> String {
    format!("sha256:{}", Digest::of(&to_canonical(&json(text))).to_hex())
}

fn row(stream: &str, seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2T{seq:03}");
    let text = format!(
        r#"{{"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&json(&text));
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

/// A paper workspace that registers the fixture's model and SPY's snapshot, requests its
/// connection on account `acct1`, and whose account stream holds an equity snapshot.
fn seed(f: &Fixture) {
    f.store.put(SPY.as_bytes()).expect("SPY's snapshot");
    let snapshot = format!(
        r#"{{"admits_instruments":null,"content_hash":"{}","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}}"#,
        reference(SPY)
    );
    let connection = format!(r#"{{"account_ref":"{ACCOUNT}","connection_id":"{CONNECTION}"}}"#);
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    streams.insert(
        ctl(),
        vec![
            row(&ctl(), 1, "StreamOpened", "{}"),
            row(&ctl(), 2, "ConfigSnapshotRegistered", MODEL),
            row(&ctl(), 3, "ConfigSnapshotRegistered", &snapshot),
            row(&ctl(), 4, "ConnectionRequested", &connection),
        ],
    );
    streams.insert(
        account(),
        vec![row(
            &account(),
            1,
            "AccountSnapshotRecorded",
            r#"{"equity":"25000"}"#,
        )],
    );
}

/// Appends `event_type` with `payload` to the control stream, as another writer would.
fn seed_control(f: &Fixture, event_type: &str, payload: &str) -> StoredEvent {
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    let rows = streams.get_mut(&ctl()).expect("seeded");
    let seq = u64::try_from(rows.len()).expect("a count") + 1;
    let added = row(&ctl(), 10 + seq, event_type, payload);
    let added = StoredEvent { seq, ..added };
    rows.push(added.clone());
    added
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

fn get(f: &Fixture, path: &str, cookie: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(f.addr, "GET", path, &[("Cookie", cookie.as_str())], b"")
}

/// §3.4's id for `operation` by Alice under `key`, computed by `mandate-api`.
fn derived(operation: &str, key: &str) -> String {
    let workspace = Id::try_from(WORKSPACE.to_owned()).expect("an id");
    let principal = Id::try_from(ALICE.to_owned()).expect("an id");
    let key = IdempotencyKey::parse(key).expect("a key");
    let id = event_id(&Derivation {
        workspace: &workspace,
        principal: &principal,
        operation,
        key: &key,
        position: None,
    })
    .expect("an id");
    let text = String::from_utf8(mandate_api::wire::encode(&id).expect("encodes")).expect("UTF-8");
    text.trim_matches('"').to_owned()
}

fn body(reply: &Reply) -> Value {
    parse(&reply.body)
        .unwrap_or_else(|e| panic!("{e:?}: {:?}", String::from_utf8_lossy(&reply.body)))
}

fn text<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    path.iter()
        .try_fold(value, |v, k| v.get(k))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{path:?} in {value:?}"))
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

fn violations(reply: &Reply) -> Vec<(String, String)> {
    assert_eq!(
        reply.status,
        422,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    problem(reply)
        .violations
        .into_iter()
        .map(|v| match v {
            Violation::Schema { path, code, .. } | Violation::Rule { path, code, .. } => {
                (path, code)
            }
            Violation::Policy { key, .. } => (String::new(), key),
        })
        .collect()
}

/// The fixture with one member's value changed, by name.
fn document(name: &str) -> String {
    let (from, to) = match name {
        "v1" => return MANDATE.to_owned(),
        "v2" => (r#"max_order_usd":"300"#, r#"max_order_usd":"200"#),
        "qqq" => (r#""symbol":"SPY""#, r#""symbol":"QQQ""#),
        "big_order" => (r#""max_order_usd":"300""#, r#""max_order_usd":"2000""#),
        "live" => (r#""environment":"paper""#, r#""environment":"live""#),
        "wide" => (r#""max_instruments":1"#, r#""max_instruments":2"#),
        _ => panic!("{name}"),
    };
    assert!(MANDATE.contains(from), "{from}");
    MANDATE.replace(from, to)
}

fn version(name: &str) -> String {
    reference(&document(name))
}

fn control(f: &Fixture) -> Vec<StoredEvent> {
    f.journal.rows_of(&ctl())
}

/// The control stream's events of `event_type`.
fn events(f: &Fixture, event_type: &str) -> Vec<StoredEvent> {
    control(f)
        .into_iter()
        .filter(|row| row.event_type == event_type)
        .collect()
}

fn payload(row: &StoredEvent) -> Value {
    parse(&row.body)
        .expect("JSON")
        .get("payload")
        .cloned()
        .expect("a payload")
}

fn stored(f: &Fixture, reference: &str) -> Value {
    let bytes = get_artifact(
        f.store.as_ref(),
        &ArtifactRef::parse(reference).expect("a ref"),
    )
    .expect("stored");
    parse(&bytes).expect("JSON")
}

fn create(f: &Fixture, cookie: &str, key: &str, name: &str) -> Reply {
    post(
        f,
        &ws("/mandate-versions"),
        cookie,
        Some(key),
        document(name).as_bytes(),
    )
}

/// §5.1's confirm body, with `digest` as the screen digest shown.
fn confirm_body(digest: &str, step_up: &str) -> String {
    let paths: Vec<String> = PATHS.iter().map(|p| format!("\"{p}\"")).collect();
    format!(
        r#"{{"agent_id":null,"base_version":null,"confirmed_paths":[{}],"warnings_acknowledged":["W-002"],"classification_shown":"risk_increasing","screen_digest":"{digest}","record":{RECORD},"step_up":{step_up}}}"#,
        paths.join(",")
    )
}

/// The screen digest of a new mandate's version, which warns W-002 (its one warning): the SHA-256 of the canonical
/// object of the seven members DEC-2101 item 4 names.
fn screen_digest(name: &str) -> String {
    let paths: Vec<String> = PATHS.iter().map(|p| format!("\"{p}\"")).collect();
    reference(&format!(
        r#"{{"mandate_version":"{}","agent_id":null,"base_version":null,"classification":"risk_increasing","required_paths":[{}],"warnings":["W-002"],"step_up_required":true}}"#,
        version(name),
        paths.join(",")
    ))
}

fn confirm(f: &Fixture, cookie: &str, key: &str, name: &str, body: &str) -> Reply {
    post(
        f,
        &ws(&format!("/mandate-versions/{}/confirm", version(name))),
        cookie,
        Some(key),
        body.as_bytes(),
    )
}

fn deploy_body(agent: &str, name: &str, step_up: &str) -> String {
    format!(
        r#"{{"agent_id":"{agent}","mandate_version":"{}","backtest_id":null,"record":{RECORD},"step_up":{step_up}}}"#,
        version(name)
    )
}

fn deploy(f: &Fixture, cookie: &str, key: &str, body: &str) -> Reply {
    post(f, &ws("/agents"), cookie, Some(key), body.as_bytes())
}

/// `name` created and confirmed through the API.
fn confirmed(f: &Fixture, cookie: &str, name: &str) {
    assert_eq!(create(f, cookie, KEY, name).status, 202);
    let reply = confirm(
        f,
        cookie,
        KEY,
        name,
        &confirm_body(&screen_digest(name), EVIDENCE),
    );
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
}

/// Every row the server appended passes the journal's own draft checks (journal spec §9).
fn journal_accepts(f: &Fixture) {
    for row in control(f).iter().filter(|row| row.event_time_set()) {
        Draft::parse(&row.body).unwrap_or_else(|e| panic!("{}: {e:?}", row.event_type));
    }
}

/// The rows the server wrote, as opposed to the seeds (which carry no `event_time`).
trait Written {
    fn event_time_set(&self) -> bool;
}

impl Written for StoredEvent {
    fn event_time_set(&self) -> bool {
        parse(&self.body)
            .ok()
            .is_some_and(|event| event.get("event_time").is_some())
    }
}

#[test]
fn every_route_without_a_session_is_the_one_401() {
    let f = fixture();
    let v = version("v1");
    for (method, path) in [
        ("POST", ws("/mandate-versions")),
        ("GET", ws(&format!("/mandate-versions/{v}/confirmation"))),
        ("POST", ws(&format!("/mandate-versions/{v}/confirm"))),
        ("POST", ws("/agents")),
        ("POST", ws("/agents/agt_a/go-live")),
    ] {
        let reply = send(f.addr, method, &path, &[("Origin", APP)], b"{}");
        refused(&reply, 401, ProblemCode::Unauthenticated);
    }
    assert!(control(&f).len() == 4, "nothing appended");
}

#[test]
fn a_version_is_created_from_the_document_as_the_cli_records_it() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let reply = create(&f, &cookie, KEY, "v1");
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let answer = body(&reply);
    let v = version("v1");
    assert_eq!(text(&answer, &["mandate_version"]), v);
    assert_eq!(text(&answer, &["phase"]), "recorded");
    assert_eq!(
        text(&answer, &["command_id"]),
        derived("create_version", KEY)
    );
    assert!(answer.get("warnings").and_then(Value::as_array).is_some());
    let created = events(&f, "MandateVersionCreated");
    assert_eq!(created.len(), 1);
    let event = &created[0];
    assert_eq!(event.event_id, derived("create_version", KEY));
    let payload = payload(event);
    assert_eq!(text(&payload, &["mandate_version"]), v);
    let provenance = payload
        .get("provenance")
        .and_then(Value::as_array)
        .expect("a list");
    let paths: Vec<&str> = provenance.iter().map(|p| text(p, &["path"])).collect();
    assert_eq!(paths, PATHS.to_vec());
    assert!(
        provenance
            .iter()
            .all(|p| text(p, &["source"]) == "user_entered")
    );
    let record = stored(&f, text(&payload, &["record_ref"]));
    assert_eq!(
        record,
        json(&format!(
            r#"{{"kind":"mandate_version_record","mandate_version":"{v}","user":"{ALICE}"}}"#
        ))
    );
    assert_eq!(
        stored(&f, &v),
        json(MANDATE),
        "the canonical document is stored under its version"
    );
    let envelope = parse(&event.body).expect("JSON");
    assert_eq!(text(&envelope, &["actor", "id"]), ALICE);
    assert_eq!(text(&envelope, &["actor", "kind"]), "user");
    journal_accepts(&f);
}

#[test]
fn a_repeat_replays_and_a_recorded_version_is_not_recorded_twice() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let first = create(&f, &cookie, KEY, "v1");
    let again = create(&f, &cookie, KEY, "v1");
    assert_eq!(again.status, 202);
    assert_eq!(
        text(&body(&again), &["command_id"]),
        text(&body(&first), &["command_id"])
    );
    let other_key = create(&f, &cookie, KEY_2, "v1");
    assert_eq!(other_key.status, 200, "the version is already recorded");
    assert_eq!(
        text(&body(&other_key), &["command_id"]),
        derived("create_version", KEY)
    );
    assert_eq!(events(&f, "MandateVersionCreated").len(), 1);
    let conflict = create(&f, &cookie, KEY, "v2");
    refused(&conflict, 409, ProblemCode::IdempotencyConflict);
}

#[test]
fn a_document_that_is_not_a_paper_mandate_is_refused_with_nothing_recorded() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let live = create(&f, &cookie, KEY, "live");
    refused(&live, 409, ProblemCode::LiveUnavailable);
    let malformed = post(&f, &ws("/mandate-versions"), &cookie, Some(KEY), b"{");
    assert_eq!(
        violations(&malformed),
        vec![(String::new(), "malformed".to_owned())]
    );
    let not_mandate = post(&f, &ws("/mandate-versions"), &cookie, Some(KEY), b"{}");
    assert_eq!(
        violations(&not_mandate),
        vec![(String::new(), "mandate".to_owned())]
    );
    let big = create(&f, &cookie, KEY, "big_order");
    let found = violations(&big);
    assert!(
        !found.is_empty()
            && found
                .iter()
                .all(|(p, c)| p.is_empty() && c.starts_with("V-")),
        "{found:?}"
    );
    let qqq = create(&f, &cookie, KEY, "qqq");
    assert_eq!(
        violations(&qqq),
        vec![(
            "/universe/pinned_instruments".to_owned(),
            "instrument_unregistered".to_owned()
        )]
    );
    assert!(events(&f, "MandateVersionCreated").is_empty());
}

#[test]
fn creating_needs_the_drafting_role_the_csrf_mark_a_key_and_a_store() {
    let f = fixture();
    let viewer = signed_in(&f, bob());
    refused(&create(&f, &viewer, KEY, "v1"), 403, ProblemCode::Forbidden);
    let cookie = signed_in(&f, alice());
    let no_key = post(
        &f,
        &ws("/mandate-versions"),
        &cookie,
        None,
        MANDATE.as_bytes(),
    );
    refused(&no_key, 422, ProblemCode::Invalid);
    let cookie_header = format!("{COOKIE_NAME}={cookie}");
    let no_mark = send(
        f.addr,
        "POST",
        &ws("/mandate-versions"),
        &[
            ("Origin", APP),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", KEY),
        ],
        MANDATE.as_bytes(),
    );
    refused(&no_mark, 403, ProblemCode::Forbidden);
    let other = "/v1/workspaces/01JB3K6F0C9R2V7N4M8Q1T5W40/mandate-versions".to_owned();
    refused(
        &post(&f, &other, &cookie, Some(KEY), MANDATE.as_bytes()),
        404,
        ProblemCode::NotFound,
    );
    assert!(events(&f, "MandateVersionCreated").is_empty());

    let bare = built(Port::Accepting, false);
    let cookie = signed_in(&bare, alice());
    refused(
        &create(&bare, &cookie, KEY, "v1"),
        503,
        ProblemCode::JournalUnavailable,
    );
}

#[test]
fn the_confirmation_screen_serves_the_servers_classification_and_digest() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    let v = version("v1");
    let reply = get(
        &f,
        &ws(&format!("/mandate-versions/{v}/confirmation")),
        &cookie,
    );
    assert_eq!(
        reply.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let screen = body(&reply);
    assert_eq!(text(&screen, &["mandate_version"]), v);
    assert_eq!(screen.get("document"), Some(&json(MANDATE)));
    assert_eq!(screen.get("agent_id"), Some(&Value::Null));
    assert_eq!(screen.get("base_version"), Some(&Value::Null));
    assert_eq!(text(&screen, &["classification"]), "risk_increasing");
    assert_eq!(screen.get("step_up_required"), Some(&Value::Bool(true)));
    let paths: Vec<&str> = screen
        .get("required_paths")
        .and_then(Value::as_array)
        .expect("a list")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(paths, PATHS.to_vec());
    assert_eq!(screen.get("warnings"), Some(&json(r#"["W-002"]"#)));
    assert_eq!(text(&screen, &["screen_digest"]), screen_digest("v1"));
    let absent = get(
        &f,
        &ws(&format!("/mandate-versions/{}/confirmation", version("v2"))),
        &cookie,
    );
    refused(&absent, 404, ProblemCode::NotFound);
    let viewer = signed_in(&f, bob());
    refused(
        &get(
            &f,
            &ws(&format!("/mandate-versions/{v}/confirmation")),
            &viewer,
        ),
        403,
        ProblemCode::Forbidden,
    );
}

#[test]
fn a_confirm_is_refused_for_step_up_until_e9_4_binds_the_port() {
    let f = built(Port::Unbound, true);
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    let digest = screen_digest("v1");
    let without = confirm(&f, &cookie, KEY, "v1", &confirm_body(&digest, "null"));
    refused(&without, 401, ProblemCode::StepUpRequired);
    let with = confirm(&f, &cookie, KEY, "v1", &confirm_body(&digest, EVIDENCE));
    refused(&with, 401, ProblemCode::StepUpMissing);
    assert!(events(&f, "MandateConfirmed").is_empty());
}

#[test]
fn a_verified_confirm_commits_mandate_confirmed_bound_to_its_digest() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    let v = version("v1");
    let reply = confirm(
        &f,
        &cookie,
        KEY,
        "v1",
        &confirm_body(&screen_digest("v1"), EVIDENCE),
    );
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let answer = body(&reply);
    assert_eq!(
        text(&answer, &["command_id"]),
        derived("confirm_version", KEY)
    );
    assert_eq!(text(&answer, &["classification"]), "risk_increasing");
    assert_eq!(text(&answer, &["applies"]), "next_safe_point");
    let bound = Digest::of(&to_canonical(&json(&format!(
        r#"{{"agent_id":null,"base_version":null,"mandate_version":"{v}"}}"#
    ))));
    assert_eq!(
        *f.asked.0.lock().expect("unpoisoned"),
        vec![(StepUpActionKind::ConfirmVersion, bound)]
    );
    let confirmed = events(&f, "MandateConfirmed");
    assert_eq!(confirmed.len(), 1);
    let payload = payload(&confirmed[0]);
    assert_eq!(text(&payload, &["mandate_version"]), v);
    let paths: Vec<&str> = payload
        .get("confirmed_paths")
        .and_then(Value::as_array)
        .expect("a list")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(paths, PATHS.to_vec());
    let record = stored(&f, text(&payload, &["record_ref"]));
    assert_eq!(text(&record, &["kind"]), "mandate_confirmation_record");
    assert_eq!(text(&record, &["user"]), ALICE);
    assert_eq!(record.get("step_up"), Some(&json(EVIDENCE)));
    assert_eq!(record.get("record"), Some(&json(RECORD)));
    assert_eq!(text(&record, &["screen_digest"]), screen_digest("v1"));
    let envelope = parse(&confirmed[0].body).expect("JSON");
    let mut refs = [v.clone(), text(&payload, &["record_ref"]).to_owned()];
    refs.sort();
    let listed: Vec<&str> = envelope
        .get("artifact_refs")
        .and_then(Value::as_array)
        .expect("refs")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(listed, refs.iter().map(String::as_str).collect::<Vec<_>>());
    journal_accepts(&f);

    let again = confirm(
        &f,
        &cookie,
        KEY,
        "v1",
        &confirm_body(&screen_digest("v1"), EVIDENCE),
    );
    assert_eq!(again.status, 202, "a repeat replays");
    assert_eq!(
        f.asked.0.lock().expect("unpoisoned").len(),
        1,
        "before step-up is judged again"
    );
    let twice = confirm(
        &f,
        &cookie,
        KEY_2,
        "v1",
        &confirm_body(&screen_digest("v1"), EVIDENCE),
    );
    assert_eq!(twice.status, 200, "already the last confirmed version");
    assert_eq!(events(&f, "MandateConfirmed").len(), 1);
}

#[test]
fn a_confirm_is_judged_in_section_5_1s_order() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    let digest = screen_digest("v1");
    let good = confirm_body(&digest, EVIDENCE);
    let agent = good.replace(r#""agent_id":null"#, r#""agent_id":"agt_a""#);
    assert_eq!(
        violations(&confirm(&f, &cookie, KEY_2, "v1", &agent)),
        vec![("/agent_id".to_owned(), "agent_link_unavailable".to_owned())]
    );
    let base = good.replace(
        r#""base_version":null"#,
        &format!(r#""base_version":"{}""#, version("v2")),
    );
    assert_eq!(
        violations(&confirm(&f, &cookie, KEY_2, "v1", &base)),
        vec![("/base_version".to_owned(), "no_base".to_owned())]
    );
    refused(
        &confirm(&f, &cookie, KEY_2, "v2", &good),
        404,
        ProblemCode::NotFound,
    );
    let shown = good.replace(
        r#""classification_shown":"risk_increasing""#,
        r#""classification_shown":"neutral""#,
    );
    refused(
        &confirm(&f, &cookie, KEY_2, "v1", &shown),
        409,
        ProblemCode::ClassificationChanged,
    );
    let short = good
        .replace(r#""/universe""#, "")
        .replace(r#""/risk","#, r#""/risk""#);
    assert_eq!(
        violations(&confirm(&f, &cookie, KEY_2, "v1", &short)),
        vec![("/confirmed_paths".to_owned(), "V-020".to_owned())]
    );
    let moved = good.replace(&digest, &reference("{}"));
    refused(
        &confirm(&f, &cookie, KEY_2, "v1", &moved),
        409,
        ProblemCode::ClassificationChanged,
    );
    let extra_ack = good.replace(r#"["W-002"]"#, r#"["W-002","W-001"]"#);
    assert_eq!(
        violations(&confirm(&f, &cookie, KEY_2, "v1", &extra_ack)),
        vec![(
            "/warnings_acknowledged".to_owned(),
            "unacknowledged".to_owned()
        )],
        "a warning the screen never showed is not acknowledged"
    );
    let twice = good.replace(r#"["W-002"]"#, r#"["W-002","W-002"]"#);
    assert_eq!(
        violations(&confirm(&f, &cookie, KEY_2, "v1", &twice)),
        vec![(
            "/warnings_acknowledged".to_owned(),
            "unacknowledged".to_owned()
        )]
    );
    let unacknowledged = good.replace(r#"["W-002"]"#, "[]");
    assert_eq!(
        violations(&confirm(&f, &cookie, KEY_2, "v1", &unacknowledged)),
        vec![(
            "/warnings_acknowledged".to_owned(),
            "unacknowledged".to_owned()
        )]
    );
    let extra = good.replacen('{', r#"{"requested_by":"owner","#, 1);
    assert_eq!(
        violations(&confirm(&f, &cookie, KEY_2, "v1", &extra))[0].1,
        "unknown_member"
    );
    let viewer = signed_in(&f, bob());
    refused(
        &confirm(&f, &viewer, KEY_2, "v1", &good),
        403,
        ProblemCode::Forbidden,
    );
    assert!(events(&f, "MandateConfirmed").is_empty());
    assert!(
        f.asked.0.lock().expect("unpoisoned").is_empty(),
        "step-up is judged last"
    );
}

#[test]
fn a_deploy_is_refused_for_step_up_until_e9_4_binds_the_port() {
    let f = built(Port::Unbound, true);
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    seed_confirmation(&f, "v1");
    refused(
        &deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", "null")),
        401,
        ProblemCode::StepUpRequired,
    );
    refused(
        &deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE)),
        401,
        ProblemCode::StepUpMissing,
    );
    assert!(events(&f, "AgentDeployed").is_empty());
}

/// `name`'s `MandateConfirmed`, as the CLI would have committed it.
fn seed_confirmation(f: &Fixture, name: &str) {
    seed_control(
        f,
        "MandateConfirmed",
        &format!(
            r#"{{"confirmed_paths":[{}],"mandate_version":"{}","record_ref":"{}"}}"#,
            PATHS.map(|p| format!("\"{p}\"")).join(","),
            version(name),
            reference("{}")
        ),
    );
}

#[test]
fn a_verified_deploy_commits_agent_deployed_bound_to_its_version() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    f.asked.0.lock().expect("unpoisoned").clear();
    let v = version("v1");
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let answer = body(&reply);
    assert_eq!(text(&answer, &["command_id"]), derived("deploy", KEY));
    assert_eq!(text(&answer, &["phase"]), "recorded");
    let bound = Digest::of(&to_canonical(&json(&format!(
        r#"{{"agent_id":"agt_a","mandate_version":"{v}"}}"#
    ))));
    assert_eq!(
        *f.asked.0.lock().expect("unpoisoned"),
        vec![(StepUpActionKind::Deploy, bound)]
    );
    let deployed = events(&f, "AgentDeployed");
    assert_eq!(deployed.len(), 1);
    let payload = payload(&deployed[0]);
    assert_eq!(text(&payload, &["agent_id"]), "agt_a");
    assert_eq!(text(&payload, &["mandate_version"]), v);
    let envelope = parse(&deployed[0].body).expect("JSON");
    assert_eq!(text(&envelope, &["config_refs", "mandate_version"]), v);
    let record = stored(&f, text(&payload, &["record_ref"]));
    assert_eq!(text(&record, &["kind"]), "deployment_record");
    assert_eq!(record.get("step_up"), Some(&json(EVIDENCE)));
    assert_eq!(record.get("backtest_id"), Some(&Value::Null));
    journal_accepts(&f);

    let again = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(again.status, 202, "a repeat replays");
    assert_eq!(
        f.asked.0.lock().expect("unpoisoned").len(),
        1,
        "before step-up is judged again"
    );
    let same = deploy(&f, &cookie, KEY_2, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(same.status, 200, "the agent already runs this version");
    assert_eq!(events(&f, "AgentDeployed").len(), 1);
}

/// No route creates an agent (#1444): deploying names it, and the reads serve it from that
/// `AgentDeployed` alone, before any runtime opens its stream.
#[test]
fn deploying_an_agent_id_never_seen_creates_the_agent() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    assert_eq!(get(&f, &ws("/agents/agt_new"), &cookie).status, 404);
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_new", "v1", EVIDENCE));
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert!(
        !f.journal
            .streams
            .lock()
            .expect("unpoisoned")
            .contains_key(&format!("agent:{WORKSPACE}:agt_new")),
        "the deploy opens no agent stream; the runtime does"
    );
    let one = get(&f, &ws("/agents/agt_new"), &cookie);
    assert_eq!(one.status, 200, "{:?}", String::from_utf8_lossy(&one.body));
    let agent = body(&one);
    assert_eq!(text(&agent, &["agent", "agent_id"]), "agt_new");
    assert_eq!(text(&agent, &["agent", "mandate_version"]), version("v1"));
    assert_eq!(
        agent.get("agent").and_then(|a| a.get("mode")),
        Some(&Value::Null),
        "no mode is reported before the runtime opens the stream"
    );
    let listed = body(&get(&f, &ws("/agents"), &cookie));
    let ids: Vec<&str> = match listed.get("agents") {
        Some(Value::Array(agents)) => agents.iter().map(|a| text(a, &["agent_id"])).collect(),
        other => panic!("{other:?}"),
    };
    assert_eq!(ids, vec!["agt_new"]);
}

#[test]
fn a_deploy_takes_the_clis_checks_in_its_order() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    create(&f, &cookie, KEY_2, "v2");
    let unconfirmed = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(
        violations(&unconfirmed),
        vec![(
            "/mandate_version".to_owned(),
            "version_unconfirmed".to_owned()
        )]
    );
    seed_confirmation(&f, "v1");
    seed_confirmation(&f, "v2");
    let stale = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(stale.status, 409);
    let problem = problem(&stale);
    assert_eq!(problem.code, ProblemCode::StaleBase);
    let current = match problem.current_base {
        Some(CurrentBase::Ref(current)) => {
            String::from_utf8(mandate_api::wire::encode(&current).expect("encodes")).expect("UTF-8")
        }
        other => panic!("{other:?}"),
    };
    assert_eq!(current.trim_matches('"'), version("v2"));
    seed_control(
        &f,
        "AgentDeployed",
        &format!(
            r#"{{"agent_id":"agt_a","mandate_version":"{}","record_ref":"{}"}}"#,
            version("v1"),
            reference("{}")
        ),
    );
    let active = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v2", EVIDENCE));
    assert_eq!(
        violations(&active),
        vec![("/agent_id".to_owned(), "agent_active".to_owned())]
    );
    refused(
        &deploy(
            &f,
            &cookie,
            KEY,
            &deploy_body("agt_b", "big_order", EVIDENCE),
        ),
        404,
        ProblemCode::NotFound,
    );
    let unknown = deploy_body("agt_b", "v2", EVIDENCE).replacen('{', r#"{"extra":1,"#, 1);
    assert_eq!(
        violations(&deploy(&f, &cookie, KEY, &unknown)),
        vec![("/extra".to_owned(), "unknown_member".to_owned())]
    );
    let missing = deploy_body("agt_b", "v2", EVIDENCE).replace(r#""backtest_id":null,"#, "");
    assert_eq!(
        violations(&deploy(&f, &cookie, KEY, &missing)),
        vec![("/backtest_id".to_owned(), "missing".to_owned())]
    );
    let viewer = signed_in(&f, bob());
    refused(
        &deploy(&f, &viewer, KEY, &deploy_body("agt_b", "v2", EVIDENCE)),
        403,
        ProblemCode::Forbidden,
    );
    assert_eq!(
        events(&f, "AgentDeployed").len(),
        1,
        "only the seeded deployment"
    );
    assert!(
        f.asked.0.lock().expect("unpoisoned").is_empty(),
        "step-up is judged last"
    );
}

#[test]
fn a_deploy_checks_v_002_against_the_accounts_equity_snapshot() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .remove(&account());
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(
        violations(&reply),
        vec![(String::new(), "V-002".to_owned())]
    );
    f.journal.streams.lock().expect("unpoisoned").insert(
        account(),
        vec![row(
            &account(),
            1,
            "AccountSnapshotRecorded",
            r#"{"equity":"25000"}"#,
        )],
    );
    seed_control(
        &f,
        "ConnectionRevoked",
        &format!(r#"{{"connection_id":"{CONNECTION}"}}"#),
    );
    let revoked = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    let codes: Vec<String> = violations(&revoked).into_iter().map(|(_, c)| c).collect();
    assert!(
        codes.contains(&"V-002".to_owned()),
        "a snapshot before the revocation does not count: {codes:?}"
    );
    assert!(events(&f, "AgentDeployed").is_empty());
}

/// Go-live is always `live_unavailable`; while the gate is off, as here, its refusal is the one
/// record it leaves ([DEC-2134] item 4), and a refused role or a missing key leaves none.
///
/// [DEC-2134]: ../../../docs/project/decisions/DEC-2134.md
#[test]
fn go_live_is_always_live_unavailable() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    let reply = post(
        &f,
        &ws("/agents/agt_a/go-live"),
        &cookie,
        Some(KEY),
        b"not read",
    );
    refused(&reply, 409, ProblemCode::LiveUnavailable);
    let viewer = signed_in(&f, bob());
    refused(
        &post(&f, &ws("/agents/agt_a/go-live"), &viewer, Some(KEY), b""),
        403,
        ProblemCode::Forbidden,
    );
    refused(
        &post(&f, &ws("/agents/agt_a/go-live"), &cookie, None, b""),
        422,
        ProblemCode::Invalid,
    );
    let rows = control(&f);
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[4].event_type, "RealMoneyRefused");
}

#[test]
fn an_append_that_does_not_settle_reports_what_is_known() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    f.journal.script(&[Scripted::Unavailable; 5]);
    refused(
        &create(&f, &cookie, KEY, "v1"),
        503,
        ProblemCode::JournalUnavailable,
    );
    f.journal.script(&[Scripted::Ambiguous; 5]);
    let unknown = create(&f, &cookie, KEY, "v1");
    assert_eq!(unknown.status, 503);
    let problem = problem(&unknown);
    assert_eq!(problem.code, ProblemCode::OutcomeUnknown);
    assert_eq!(problem.effect, Effect::Unknown);
    f.journal.script(&[Scripted::CommittedThenAmbiguous]);
    let landed = create(&f, &cookie, KEY, "v1");
    assert_eq!(landed.status, 202, "the retry finds the commit that landed");
    assert_eq!(events(&f, "MandateVersionCreated").len(), 1);
}

#[test]
fn the_unbound_port_counts_no_evidence() {
    let evidence = decode::<StepUpEvidence>(EVIDENCE.as_bytes()).expect("evidence");
    let action = Action {
        kind: StepUpActionKind::Deploy,
        digest: Digest::ZERO,
    };
    let scope = Scope::Workspace {
        org: OrgId(1),
        workspace: WorkspaceId(2),
    };
    let held = membership(alice(), scope, Active, &[(Role::Operator, at(T0 - 1))]);
    let tenant = match authorize(
        &StaticLookup(vec![held.clone()]),
        &Principal::User { id: alice() },
        &session(SessionRef(3), Full, vec![held]),
        scope,
        Permission::Deploy,
        at(T0),
    ) {
        Ok(Authorized::Workspace { tenant, .. }) => tenant,
        other => panic!("the operator may deploy: {other:?}"),
    };
    let judged = Judged {
        tenant: &tenant,
        control: &[],
        now: at(T0),
    };
    assert_eq!(
        Unbound.verify(&evidence, &action, &judged),
        Err(ProblemCode::StepUpMissing)
    );
}

/// Records `name`'s document as the CLI would have: stored, created, and confirmed.
fn seed_version(f: &Fixture, name: &str) {
    let document = document(name);
    f.store
        .put(&to_canonical(&json(&document)))
        .expect("stored");
    seed_control(
        f,
        "MandateVersionCreated",
        &format!(
            r#"{{"mandate_version":"{}","provenance":[],"record_ref":"{}"}}"#,
            version(name),
            reference("{}")
        ),
    );
    seed_confirmation(f, name);
}

/// The control stream as one of `environment`: its first row's, which the server reads.
fn workspace_in(f: &Fixture, environment: &str) {
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    let rows = streams.get_mut(&ctl()).expect("seeded");
    rows[0].environment = environment.to_owned();
}

#[test]
fn a_document_of_another_environment_than_the_workspaces_is_refused() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    workspace_in(&f, "live");
    assert_eq!(
        violations(&create(&f, &cookie, KEY, "v1")),
        vec![("/environment".to_owned(), "environment".to_owned())]
    );
    assert!(events(&f, "MandateVersionCreated").is_empty());
}

#[test]
fn a_deploy_of_a_live_or_foreign_environment_version_is_refused() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    seed_version(&f, "live");
    refused(
        &deploy(&f, &cookie, KEY, &deploy_body("agt_a", "live", EVIDENCE)),
        409,
        ProblemCode::LiveUnavailable,
    );
    seed_version(&f, "v1");
    workspace_in(&f, "live");
    assert_eq!(
        violations(&deploy(
            &f,
            &cookie,
            KEY,
            &deploy_body("agt_a", "v1", EVIDENCE)
        )),
        vec![("/mandate_version".to_owned(), "environment".to_owned())]
    );
    assert!(events(&f, "AgentDeployed").is_empty());
    assert!(f.asked.0.lock().expect("unpoisoned").is_empty());
}

#[test]
fn a_deploy_records_the_backtest_shown() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    let shown = deploy_body("agt_a", "v1", EVIDENCE)
        .replace(r#""backtest_id":null"#, r#""backtest_id":"bt_1""#);
    assert_eq!(deploy(&f, &cookie, KEY, &shown).status, 202);
    let deployed = events(&f, "AgentDeployed");
    let record = stored(&f, text(&payload(&deployed[0]), &["record_ref"]));
    assert_eq!(text(&record, &["backtest_id"]), "bt_1");
    let malformed = deploy_body("agt_b", "v1", EVIDENCE)
        .replace(r#""backtest_id":null"#, r#""backtest_id":"a b""#);
    assert_eq!(
        violations(&deploy(&f, &cookie, KEY_2, &malformed)),
        vec![("/backtest_id".to_owned(), "non_canonical".to_owned())]
    );
}

#[test]
fn a_different_body_under_a_used_key_is_a_conflict() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    let other = confirm_body(&screen_digest("v1"), EVIDENCE)
        .replace(RECORD, &RECORD.replace("abab", "efef"));
    refused(
        &confirm(&f, &cookie, KEY, "v1", &other),
        409,
        ProblemCode::IdempotencyConflict,
    );
    assert_eq!(f.asked.0.lock().expect("unpoisoned").len(), 1);
    assert_eq!(events(&f, "MandateConfirmed").len(), 1);
}

#[test]
fn another_agents_stop_leaves_this_agents_deployment_active() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    seed_control(
        &f,
        "AgentDeployed",
        &format!(
            r#"{{"agent_id":"agt_a","mandate_version":"{}","record_ref":"{}"}}"#,
            version("v1"),
            reference("{}")
        ),
    );
    seed_control(
        &f,
        "AgentStopped",
        &format!(
            r#"{{"agent_id":"agt_b","connection_id":"{CONNECTION}","loss_added":"0","reason":"owner_stop","retired_on":"2026-09-20"}}"#
        ),
    );
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(
        reply.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
}

#[test]
fn only_a_snapshot_after_the_last_revocation_counts() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    let revoked = seed_control(
        &f,
        "ConnectionRevoked",
        &format!(r#"{{"connection_id":"{CONNECTION}"}}"#),
    );
    let snapshot_at = |recorded_at: &str| {
        let mut snapshot = row(
            &account(),
            2,
            "AccountSnapshotRecorded",
            r#"{"equity":"25000"}"#,
        );
        snapshot.recorded_at = recorded_at.to_owned();
        f.journal
            .streams
            .lock()
            .expect("unpoisoned")
            .insert(account(), vec![snapshot]);
    };
    snapshot_at(&revoked.recorded_at);
    let same_instant = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    let codes: Vec<String> = violations(&same_instant)
        .into_iter()
        .map(|(_, c)| c)
        .collect();
    assert!(codes.contains(&"V-002".to_owned()), "{codes:?}");
    snapshot_at("2026-10-17T11:00:00.000000000Z");
    let later = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    let codes: Vec<String> = violations(&later).into_iter().map(|(_, c)| c).collect();
    assert!(
        !codes.contains(&"V-002".to_owned()),
        "a later snapshot counts; the revoked connection fails on its own rules: {codes:?}"
    );
}

#[test]
fn another_connections_records_leave_the_documents_connection_unrecorded() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    seed_control(&f, "ConnectionRevoked", r#"{"connection_id":"conn_other"}"#);
    let reply = create(&f, &cookie, KEY, "v1");
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
}

/// A version-2 establishment of [`CONNECTION`] on [`ACCOUNT`], as rule 201 reads it.
fn established_payload() -> String {
    format!(
        r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","margin_attestation":null,"scopes":["data","trading"],"step_up":{EVIDENCE},"user":"alice"}}"#
    )
}

/// `seed_control` at schema version 2.
fn seed_control_v2(f: &Fixture, event_type: &str, payload: &str) {
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    let rows = streams.get_mut(&ctl()).expect("seeded");
    let seq = u64::try_from(rows.len()).expect("a count") + 1;
    let added = row(&ctl(), 10 + seq, event_type, payload);
    rows.push(StoredEvent {
        seq,
        schema_version: 2,
        ..added
    });
}

/// Journal spec v0.54 §9.24 (DEC-1171): the deployment is version 2, naming the document's
/// connection, exactly when the control stream holds a version-2 establishment of it whose latest
/// establishment or revocation is an establishment (rule 201); a requested connection, which
/// names an account but is not established, gives version 1, as before.
#[test]
fn a_deploy_names_its_connection_exactly_when_rule_201_holds() {
    let established = format!(
        r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","margin_attestation":null,"scopes":["data","trading"],"step_up":{EVIDENCE},"user":"alice"}}"#
    );
    let revoked = format!(r#"{{"connection_id":"{CONNECTION}"}}"#);
    let established_v1 = format!(
        r#"{{"broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","scopes":["data","trading"]}}"#
    );
    for (name, reconnect, expected) in [
        ("requested only", 0, (1, None)),
        ("established at version 1", 0, (1, None)),
        ("established", 1, (2, Some(CONNECTION))),
        ("reconnected", 2, (2, Some(CONNECTION))),
    ] {
        let f = fixture();
        let cookie = signed_in(&f, alice());
        if name == "established at version 1" {
            seed_control(&f, "ConnectionEstablished", &established_v1);
        }
        for nth in 0..reconnect {
            if nth > 0 {
                seed_control(&f, "ConnectionRevoked", &revoked);
            }
            seed_control_v2(&f, "ConnectionEstablished", &established);
        }
        let mut snapshot = row(
            &account(),
            2,
            "AccountSnapshotRecorded",
            r#"{"equity":"25000"}"#,
        );
        snapshot.recorded_at = "2026-10-17T11:00:00.000000000Z".to_owned();
        f.journal
            .streams
            .lock()
            .expect("unpoisoned")
            .insert(account(), vec![snapshot]);
        confirmed(&f, &cookie, "v1");
        let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
        assert_eq!(
            reply.status,
            202,
            "{name}: {:?}",
            String::from_utf8_lossy(&reply.body)
        );
        let deployed = events(&f, "AgentDeployed");
        let row = deployed.last().expect("one deployment");
        let connection = payload(row).get("connection_id").cloned();
        let got = (row.schema_version, connection);
        let want = (expected.0, expected.1.map(|c| Value::Str(c.to_owned())));
        assert_eq!(got, want, "{name}");
        journal_accepts(&f);
    }
}

/// DEC-2809 item 2: while an agent that has not stopped is deployed on the account, another
/// agent's deployment there is `invalid` at `/agent_id`, `second_agent_on_account`, and nothing is
/// written; the first agent's stop frees the account.
#[test]
fn a_second_running_agent_on_the_account_is_refused_at_deploy() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    seed_control_v2(&f, "ConnectionEstablished", &established_payload());
    f.journal.streams.lock().expect("unpoisoned").insert(
        account(),
        vec![row(
            &account(),
            1,
            "AccountSnapshotRecorded",
            r#"{"equity":"25000"}"#,
        )],
    );
    confirmed(&f, &cookie, "v1");
    let first = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(
        first.status,
        202,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let before = control(&f).len();
    let second = deploy(&f, &cookie, KEY_2, &deploy_body("agt_b", "v1", EVIDENCE));
    assert_eq!(
        violations(&second),
        vec![("/agent_id".to_owned(), "second_agent_on_account".to_owned())]
    );
    assert_eq!(control(&f).len(), before, "nothing written");
    seed_control(
        &f,
        "AgentStopped",
        &format!(
            r#"{{"agent_id":"agt_a","connection_id":"{CONNECTION}","loss_added":"0","reason":"owner_stop","retired_on":"2026-09-20"}}"#
        ),
    );
    let freed = deploy(
        &f,
        &cookie,
        "k4Z-9_cccccccccc",
        &deploy_body("agt_b", "v1", EVIDENCE),
    );
    assert!(
        !violations(&freed)
            .iter()
            .any(|(_, code)| code == "second_agent_on_account"),
        "the first agent's stop frees the account of this refusal: {}",
        String::from_utf8_lossy(&freed.body)
    );
}

/// DEC-2809 item 2's edges, each judged before any V-rule: an agent running on another account
/// does not hold this one; a connection established only at version 1 names no account; and a
/// version-1 deployment, which names no connection the executor follows, holds no account. None of
/// them is refused `second_agent_on_account`, whatever the later checks answer.
#[test]
fn another_account_or_a_version_1_record_holds_no_account() {
    let other = |establish_v2: bool, deployed_v2: bool, named: &str| {
        let f = fixture();
        let cookie = signed_in(&f, alice());
        seed_control_v2(&f, "ConnectionEstablished", &established_payload());
        f.journal.streams.lock().expect("unpoisoned").insert(
            account(),
            vec![row(
                &account(),
                1,
                "AccountSnapshotRecorded",
                r#"{"equity":"25000"}"#,
            )],
        );
        confirmed(&f, &cookie, "v1");
        let sibling = established_payload()
            .replace(CONNECTION, "conn_other")
            .replace(ACCOUNT, named);
        if establish_v2 {
            seed_control_v2(&f, "ConnectionEstablished", &sibling);
        } else {
            seed_control(
                &f,
                "ConnectionEstablished",
                r#"{"broker":"alpaca","connection_id":"conn_other","environment":"paper","scopes":["data","trading"]}"#,
            );
        }
        if deployed_v2 {
            let deployed = format!(
                r#"{{"agent_id":"agt_a","connection_id":"conn_other","mandate_version":"{}","record_ref":"{}"}}"#,
                version("v1"),
                reference("{}")
            );
            seed_control_v2(&f, "AgentDeployed", &deployed);
        } else {
            let deployed = format!(
                r#"{{"agent_id":"agt_a","mandate_version":"{}","record_ref":"{}"}}"#,
                version("v1"),
                reference("{}")
            );
            seed_control(&f, "AgentDeployed", &deployed);
        }
        let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_b", "v1", EVIDENCE));
        String::from_utf8_lossy(&reply.body).contains("second_agent_on_account")
    };
    assert!(
        other(true, true, ACCOUNT),
        "the same account through a sibling connection is held"
    );
    assert!(
        !other(true, true, "acct_elsewhere"),
        "another account is not"
    );
    assert!(
        !other(false, true, ACCOUNT),
        "a version-1 establishment names no account"
    );
    assert!(
        !other(true, false, ACCOUNT),
        "a version-1 deployment holds no account"
    );
}

/// A connection established and then revoked fails rule 201, but is never deployed at version 1:
/// the fold knows it with no environment, so V-001 refuses the deployment, 422, and nothing is
/// written (journal spec §9.24, "Revoked").
#[test]
fn a_revoked_connection_is_refused_rather_than_deployed_at_version_1() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    seed_control_v2(
        &f,
        "ConnectionEstablished",
        &format!(
            r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","margin_attestation":null,"scopes":["data","trading"],"step_up":{EVIDENCE},"user":"alice"}}"#
        ),
    );
    let mut snapshot = row(
        &account(),
        2,
        "AccountSnapshotRecorded",
        r#"{"equity":"25000"}"#,
    );
    snapshot.recorded_at = "2026-10-17T11:00:00.000000000Z".to_owned();
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(account(), vec![snapshot]);
    confirmed(&f, &cookie, "v1");
    seed_control(
        &f,
        "ConnectionRevoked",
        &format!(r#"{{"connection_id":"{CONNECTION}"}}"#),
    );
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    let body = String::from_utf8_lossy(&reply.body);
    assert_eq!(reply.status, 422, "{body}");
    assert!(body.contains(r#""code":"V-001""#), "{body}");
    assert!(events(&f, "AgentDeployed").is_empty(), "nothing is written");
    journal_accepts(&f);
}

/// An agent id is deployed once (DEC-1173 item 1): one an `AgentStopped` names is `invalid` at
/// `/agent_id`, whatever version, and nothing is written.
#[test]
fn a_stopped_agent_is_never_deployed_again() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    seed_control(
        &f,
        "AgentDeployed",
        &format!(
            r#"{{"agent_id":"agt_a","mandate_version":"{}","record_ref":"{}"}}"#,
            version("v1"),
            reference("{}")
        ),
    );
    seed_control(
        &f,
        "AgentStopped",
        &format!(
            r#"{{"agent_id":"agt_a","connection_id":"{CONNECTION}","loss_added":"0","reason":"owner_stop","retired_on":"2026-09-20"}}"#
        ),
    );
    let before = control(&f).len();
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(
        violations(&reply),
        vec![("/agent_id".to_owned(), "agent_stopped".to_owned())]
    );
    assert_eq!(control(&f).len(), before, "nothing written");
}

/// DEC-2809 item 1: a confirmed version whose universe may hold a second instrument is refused at
/// deploy, `invalid` at `/mandate_version` with `second_instrument`, and nothing is written.
#[test]
fn a_version_that_may_hold_a_second_instrument_is_refused_at_deploy() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "wide");
    let before = control(&f).len();
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "wide", EVIDENCE));
    assert_eq!(
        violations(&reply),
        vec![(
            "/mandate_version".to_owned(),
            "second_instrument".to_owned()
        )]
    );
    assert_eq!(control(&f).len(), before, "nothing written");
}

/// DEC-2809 item 1's place in the order: after the checks on the agent, so a stopped agent meeting
/// a wide version is still `agent_stopped`; and before the V-rules, so a wide version that also
/// fails one (V-002, with no account snapshot to read the equity from) answers `second_instrument`
/// alone.
#[test]
fn second_instrument_is_judged_after_the_agent_and_before_the_rules() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "wide");
    seed_control(
        &f,
        "AgentDeployed",
        &format!(
            r#"{{"agent_id":"agt_a","mandate_version":"{}","record_ref":"{}"}}"#,
            version("v1"),
            reference("{}")
        ),
    );
    seed_control(
        &f,
        "AgentStopped",
        &format!(
            r#"{{"agent_id":"agt_a","connection_id":"{CONNECTION}","loss_added":"0","reason":"owner_stop","retired_on":"2026-09-20"}}"#
        ),
    );
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "wide", EVIDENCE));
    assert_eq!(
        violations(&reply),
        vec![("/agent_id".to_owned(), "agent_stopped".to_owned())]
    );

    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "wide");
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .remove(&account());
    let reply = deploy(&f, &cookie, KEY, &deploy_body("agt_b", "wide", EVIDENCE));
    assert_eq!(
        violations(&reply),
        vec![(
            "/mandate_version".to_owned(),
            "second_instrument".to_owned()
        )]
    );
}

/// A version-2 deployment is appended at the head the request read the control stream at, so rule
/// 201 still holds at the append: a stream that moved first (another writer's revocation, say)
/// stores nothing and answers `journal_unavailable`, and a retry reads the rows again (#1748
/// review). A version-1 deployment settles at the head as it is, as every command does.
#[test]
fn a_version_2_deploy_is_appended_only_at_the_head_it_read() {
    let established = format!(
        r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","margin_attestation":null,"scopes":["data","trading"],"step_up":{EVIDENCE},"user":"alice"}}"#
    );
    let f = fixture();
    let cookie = signed_in(&f, alice());
    seed_control_v2(&f, "ConnectionEstablished", &established);
    let mut snapshot = row(
        &account(),
        2,
        "AccountSnapshotRecorded",
        r#"{"equity":"25000"}"#,
    );
    snapshot.recorded_at = "2026-10-17T11:00:00.000000000Z".to_owned();
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(account(), vec![snapshot]);
    confirmed(&f, &cookie, "v1");
    let head = control(&f).last().expect("rows").seq;
    let before = f.journal.appends.lock().expect("unpoisoned").len();
    f.journal.script(&[Scripted::HeadMismatch]);
    let moved = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    refused(&moved, 503, ProblemCode::JournalUnavailable);
    let appends = f.journal.appends.lock().expect("unpoisoned")[before..].to_vec();
    let heads: Vec<u64> = appends.iter().map(|(_, expected, _)| *expected).collect();
    assert_eq!(heads, vec![head], "one attempt, at the head it read");
    assert!(events(&f, "AgentDeployed").is_empty(), "nothing stored");
    let again = deploy(&f, &cookie, KEY, &deploy_body("agt_a", "v1", EVIDENCE));
    assert_eq!(
        again.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&again.body)
    );
    let deployed = events(&f, "AgentDeployed");
    assert_eq!(deployed.len(), 1);
    assert_eq!(deployed[0].schema_version, 2);
}

const CHALLENGE_KEY: &str = "k3Z-9_challeng01";

/// The real binding's fixture: `v1` created, and Alice's passkey enrolled as [`CREDENTIAL`] at
/// counter 0, past its cool-off, its key in the store. The binding folds the member's passkeys
/// from whole envelopes (journal spec §3), so the seeded control rows are written again as such.
struct Passkeyed {
    f: Fixture,
    signer: Signer,
    cookie: String,
}

fn passkeyed() -> Passkeyed {
    passkeyed_with(false)
}

/// [`passkeyed`], with [`CONNECTION`] established on [`ACCOUNT`] at version 2 before Alice
/// enrols when `established`, so a deployment of `v1` meets §9.24's rule 201.
fn passkeyed_with(established: bool) -> Passkeyed {
    let f = built(Port::Methods, true);
    let signer = Signer::es256();
    f.store.put(&signer.cose_key()).expect("the key");
    {
        let mut streams = f.journal.streams.lock().expect("unpoisoned");
        let seeded = streams.remove(&ctl()).expect("seeded");
        let mut rows = common::control::Rows::opened();
        for row in seeded.iter().skip(1) {
            let payload: serde_json::Value =
                serde_json::from_slice(&to_canonical(&payload(row))).expect("JSON");
            rows.hosted(&row.event_type, T0 - 1_000, payload);
        }
        if established {
            let payload: serde_json::Value =
                serde_json::from_str(&established_payload()).expect("JSON");
            rows.edited("ConnectionEstablished", T0 - 1_000, |envelope| {
                envelope["payload"] = payload;
                envelope["schema_version"] = serde_json::json!(2);
            });
        }
        enrol(
            &mut rows,
            T0 - 900,
            enrolled(alice(), CREDENTIAL, &signer, 0, Some(T0 - 800)),
        );
        streams.insert(ctl(), rows.rows);
    }
    let cookie = signed_in(&f, alice());
    assert_eq!(create(&f, &cookie, KEY, "v1").status, 202);
    Passkeyed { f, signer, cookie }
}

/// A challenge the challenge route issued: its id, the bytes the authenticator signs, and when.
struct Issued {
    id: String,
    bytes: Vec<u8>,
    issued_at: String,
}

impl Passkeyed {
    /// Alice's challenge to confirm `v1`, bound to §3.5's digest of the confirmation.
    fn challenge(&self) -> Issued {
        let bound = Digest::of(&to_canonical(&json(&format!(
            r#"{{"agent_id":null,"base_version":null,"mandate_version":"{}"}}"#,
            version("v1")
        ))));
        self.challenge_for("confirm_version", bound, CHALLENGE_KEY)
    }

    /// Alice's challenge to deploy `agt_a` on `v1`, bound to the deploy digest (§3.5).
    fn deploy_challenge(&self) -> Issued {
        let bound = Digest::of(&to_canonical(&json(&format!(
            r#"{{"agent_id":"agt_a","mandate_version":"{}"}}"#,
            version("v1")
        ))));
        self.challenge_for("deploy", bound, "k3Z-9_cccccccccc")
    }

    /// Alice's challenge of `kind` bound to `bound`, under the challenge route's `key`.
    fn challenge_for(&self, kind: &str, bound: Digest, key: &str) -> Issued {
        let body = format!(
            r#"{{"action":{{"kind":"{kind}","digest":"sha256:{}"}}}}"#,
            bound.to_hex()
        );
        let reply = post(
            &self.f,
            &ws("/step-up/challenges"),
            &self.cookie,
            Some(key),
            body.as_bytes(),
        );
        assert_eq!(
            reply.status,
            201,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        let answer = self::body(&reply);
        Issued {
            id: text(&answer, &["challenge_id"]).to_owned(),
            bytes: unb64url(text(&answer, &["webauthn_challenge"])),
            issued_at: text(&answer, &["issued_at"]).to_owned(),
        }
    }

    /// `v1`'s confirm body under `method` evidence for `issued`, with `assertion` beside it.
    fn body(&self, issued: &Issued, method: &str, assertion: Option<serde_json::Value>) -> String {
        let evidence = format!(
            r#"{{"assertion_id":"{}","authenticated_at":"{}","method":"{method}"}}"#,
            issued.id, issued.issued_at
        );
        with_assertion(&confirm_body(&screen_digest("v1"), &evidence), assertion)
    }

    fn confirm(&self, body: &str) -> Reply {
        confirm(&self.f, &self.cookie, KEY, "v1", body)
    }

    /// `v1` confirmed under passkey step-up, its assertion at counter 9.
    fn confirmed(&self) {
        let issued = self.challenge();
        let signed = self.signer.assert(&issued.bytes, 9);
        let reply = self.confirm(&self.body(&issued, "passkey", Some(signed.json(CREDENTIAL))));
        assert_eq!(
            reply.status,
            202,
            "{:?}",
            String::from_utf8_lossy(&reply.body)
        );
    }

    /// The deploy body of `agt_a` on `v1` under passkey evidence for `issued`, with `assertion`.
    fn deploy_body(&self, issued: &Issued, assertion: Option<serde_json::Value>) -> String {
        let evidence = format!(
            r#"{{"assertion_id":"{}","authenticated_at":"{}","method":"passkey"}}"#,
            issued.id, issued.issued_at
        );
        with_assertion(&deploy_body("agt_a", "v1", &evidence), assertion)
    }

    /// The account's equity snapshot V-002 reads, so a deployment passes its V-rules.
    fn funded(&self) {
        let mut snapshot = row(
            &account(),
            2,
            "AccountSnapshotRecorded",
            r#"{"equity":"25000"}"#,
        );
        snapshot.recorded_at = "2026-10-17T11:00:00.000000000Z".to_owned();
        self.f
            .journal
            .streams
            .lock()
            .expect("unpoisoned")
            .insert(account(), vec![snapshot]);
    }

    fn held(&self, issued: &Issued) -> bool {
        self.f.challenges.held(&issued.id).is_some()
    }
}

/// `body` with `step_up_assertion` set to `assertion`, or as it is for `None`.
fn with_assertion(body: &str, assertion: Option<serde_json::Value>) -> String {
    let mut body: serde_json::Value = serde_json::from_str(body).expect("JSON");
    if let Some(assertion) = assertion {
        body["step_up_assertion"] = assertion;
    }
    body.to_string()
}

/// `reply` is `invalid` with one violation, at `/step_up_assertion` (DEC-2128 item 1).
fn invalid_at_the_assertion(reply: &Reply) {
    assert_eq!(
        reply.status,
        422,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(problem(reply).code, ProblemCode::Invalid);
    let paths: Vec<String> = violations(reply)
        .into_iter()
        .map(|(path, _)| path)
        .collect();
    assert_eq!(paths, ["/step_up_assertion"]);
}

#[test]
fn an_assertion_beside_cli_confirm_evidence_on_a_confirm_is_invalid_and_confirms_nothing() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    let evidence = EVIDENCE.replace("\"passkey\"", "\"cli_confirm\"");
    let signed = Signer::es256().assert(b"any challenge", 9);
    let body = with_assertion(
        &confirm_body(&screen_digest("v1"), &evidence),
        Some(signed.json(CREDENTIAL)),
    );
    invalid_at_the_assertion(&confirm(&f, &cookie, KEY, "v1", &body));
    assert!(f.asked.0.lock().expect("unpoisoned").is_empty());
    assert!(events(&f, "MandateConfirmed").is_empty());
}

#[test]
fn an_assertion_beside_null_step_up_on_a_confirm_is_invalid_before_step_up_required() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    create(&f, &cookie, KEY, "v1");
    let signed = Signer::es256().assert(b"any challenge", 9);
    let body = with_assertion(
        &confirm_body(&screen_digest("v1"), "null"),
        Some(signed.json(CREDENTIAL)),
    );
    invalid_at_the_assertion(&confirm(&f, &cookie, KEY, "v1", &body));
    assert!(f.asked.0.lock().expect("unpoisoned").is_empty());
    assert!(events(&f, "MandateConfirmed").is_empty());
}

#[test]
fn passkey_evidence_without_an_assertion_on_a_confirm_is_step_up_missing() {
    let w = passkeyed();
    let issued = w.challenge();
    refused(
        &w.confirm(&w.body(&issued, "passkey", None)),
        401,
        ProblemCode::StepUpMissing,
    );
    assert!(events(&w.f, "MandateConfirmed").is_empty());
    assert!(w.held(&issued), "nothing is taken");
}

#[test]
fn a_refused_passkey_confirm_confirms_nothing_and_keeps_the_challenge() {
    let w = passkeyed();
    let issued = w.challenge();
    let mut signed = w.signer.assert(&issued.bytes, 9);
    let last = signed.signature.len() - 1;
    signed.signature[last] ^= 1;
    refused(
        &w.confirm(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL)))),
        401,
        ProblemCode::StepUpMissing,
    );
    assert!(events(&w.f, "MandateConfirmed").is_empty());
    assert!(events(&w.f, "StepUpAsserted").is_empty());
    assert!(
        get_artifact(
            w.f.store.as_ref(),
            &ArtifactRef::of(&signed.canonical(CREDENTIAL))
        )
        .is_err(),
        "no assertion is stored"
    );
    assert!(w.held(&issued), "a refused confirm keeps the challenge");
}

#[test]
fn a_confirm_whose_passkeys_cannot_be_read_is_journal_unavailable() {
    let w = passkeyed();
    {
        let mut streams = w.f.journal.streams.lock().expect("unpoisoned");
        let seeded = streams.remove(&ctl()).expect("seeded");
        let mut rows = common::control::Rows { rows: seeded };
        rows.hosted(
            "CredentialEnrolled",
            T0 - 500,
            serde_json::json!({"member": ALICE, "credential": CREDENTIAL, "alg": "rs256"}),
        );
        streams.insert(ctl(), rows.rows);
    }
    let issued = w.challenge();
    let signed = w.signer.assert(&issued.bytes, 9);
    refused(
        &w.confirm(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL)))),
        503,
        ProblemCode::JournalUnavailable,
    );
    assert!(events(&w.f, "MandateConfirmed").is_empty());
    assert!(w.held(&issued), "nothing is taken");
}

#[test]
fn a_passkey_confirm_commits_mandate_confirmed_then_its_assertion_and_stores_the_raw_assertion() {
    let w = passkeyed();
    let issued = w.challenge();
    let signed = w.signer.assert(&issued.bytes, 9);
    let reply = w.confirm(&w.body(&issued, "passkey", Some(signed.json(CREDENTIAL))));
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let rows = control(&w.f);
    let last_two: Vec<&str> = rows
        .iter()
        .rev()
        .take(2)
        .rev()
        .map(|row| row.event_type.as_str())
        .collect();
    assert_eq!(
        last_two,
        ["MandateConfirmed", "StepUpAsserted"],
        "the confirmation, then its assertion, in one batch (DEC-2128 item 3)"
    );
    let confirmed = &rows[rows.len() - 2];
    let assertion = &rows[rows.len() - 1];
    assert_eq!(confirmed.event_id, derived("confirm_version", KEY));
    assert_eq!(
        assertion.event_id,
        derived("confirm_version_step_up_asserted", KEY),
        "the assertion's id is §3.4's under its own operation (DEC-2128 item 3)"
    );
    assert_eq!(
        confirmed.schema_version, 3,
        "version 3 under passkey step-up (DEC-2131 item 3)"
    );
    let evidence = format!(
        r#"{{"assertion_id":"{}","authenticated_at":"{}","method":"passkey"}}"#,
        issued.id, issued.issued_at
    );
    let confirmation = payload(confirmed);
    assert_eq!(
        confirmation.get("step_up").map(to_canonical),
        Some(to_canonical(&parse(evidence.as_bytes()).expect("JSON"))),
        "the evidence the API judged, which rule 253 reads"
    );
    for member in ["agent_id", "base_version"] {
        assert_eq!(
            confirmation.get(member),
            Some(&Value::Null),
            "a new mandate's version 2 members: {member}"
        );
    }
    let canonical = signed.canonical(CREDENTIAL);
    let expected = format!(
        r#"{{"assertion":"sha256:{}","challenge":"{}","credential":"{CREDENTIAL}","sign_count":9}}"#,
        Digest::of(&canonical).to_hex(),
        issued.id
    );
    assert_eq!(to_canonical(&payload(assertion)), expected.into_bytes());
    let record = parse(&assertion.body).expect("JSON");
    assert_eq!(
        record.get("causation_id").and_then(Value::as_str),
        Some(confirmed.event_id.as_str()),
        "rule 253: caused by the confirmation it authorized"
    );
    assert_eq!(
        get_artifact(w.f.store.as_ref(), &ArtifactRef::of(&canonical)),
        Ok(canonical),
        "the raw assertion is stored under its own digest"
    );
    let drafts: Vec<Draft> = [confirmed, assertion]
        .into_iter()
        .map(|row| Draft::parse(&row.body).unwrap_or_else(|e| panic!("{}: {e:?}", row.event_type)))
        .collect();
    assert_eq!(
        check_batch(&drafts),
        Ok(()),
        "the journal's batch rules, rule 253 among them, admit the batch"
    );
    assert!(!w.held(&issued), "the challenge is taken");
}

/// A deployment under passkey step-up on a connection rule 201 admits (DEC-2128 item 3, DEC-2131
/// item 3): `AgentDeployed` version 3, naming the connection and carrying the evidence judged,
/// then its `StepUpAsserted` under §3.4's id for its own operation, in one batch the journal's
/// batch rules (rule 253) admit.
#[test]
fn a_passkey_deploy_commits_agent_deployed_version_3_then_its_assertion() {
    let w = passkeyed_with(true);
    w.funded();
    w.confirmed();
    let issued = w.deploy_challenge();
    let signed = w.signer.assert(&issued.bytes, 10);
    let reply = deploy(
        &w.f,
        &w.cookie,
        KEY_2,
        &w.deploy_body(&issued, Some(signed.json(CREDENTIAL))),
    );
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let rows = control(&w.f);
    let (deployed, assertion) = (&rows[rows.len() - 2], &rows[rows.len() - 1]);
    assert_eq!(
        (deployed.event_type.as_str(), assertion.event_type.as_str()),
        ("AgentDeployed", "StepUpAsserted")
    );
    assert_eq!(deployed.event_id, derived("deploy", KEY_2));
    assert_eq!(
        assertion.event_id,
        derived("deploy_step_up_asserted", KEY_2)
    );
    assert_eq!(deployed.schema_version, 3);
    let deployment = payload(deployed);
    assert_eq!(text(&deployment, &["connection_id"]), CONNECTION);
    let evidence = format!(
        r#"{{"assertion_id":"{}","authenticated_at":"{}","method":"passkey"}}"#,
        issued.id, issued.issued_at
    );
    assert_eq!(
        deployment.get("step_up").map(to_canonical),
        Some(to_canonical(&parse(evidence.as_bytes()).expect("JSON")))
    );
    let record = parse(&assertion.body).expect("JSON");
    assert_eq!(
        record.get("causation_id").and_then(Value::as_str),
        Some(deployed.event_id.as_str())
    );
    assert_eq!(text(&payload(assertion), &["challenge"]), issued.id);
    let drafts: Vec<Draft> = [deployed, assertion]
        .into_iter()
        .map(|row| Draft::parse(&row.body).unwrap_or_else(|e| panic!("{}: {e:?}", row.event_type)))
        .collect();
    assert_eq!(check_batch(&drafts), Ok(()));
    assert!(!w.held(&issued), "the challenge is taken");
}

/// [DEC-2133]: a deployment under passkey step-up on a connection rule 201 refuses has no version
/// that carries its evidence, so it is refused `invalid` at `/mandate_version`, before step-up is
/// judged: nothing is appended and the challenge is kept.
///
/// [DEC-2133]: ../../../docs/project/decisions/DEC-2133.md
#[test]
fn a_passkey_deploy_on_a_connection_rule_201_refuses_is_refused_and_keeps_its_challenge() {
    let w = passkeyed();
    w.funded();
    w.confirmed();
    let issued = w.deploy_challenge();
    let signed = w.signer.assert(&issued.bytes, 10);
    let reply = deploy(
        &w.f,
        &w.cookie,
        KEY_2,
        &w.deploy_body(&issued, Some(signed.json(CREDENTIAL))),
    );
    assert_eq!(
        reply.status,
        422,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(
        violations(&reply),
        [(
            "/mandate_version".to_owned(),
            "connection_unrecorded".to_owned()
        )]
    );
    assert!(events(&w.f, "AgentDeployed").is_empty());
    assert!(
        events(&w.f, "StepUpAsserted").len() == 1,
        "the confirmation's alone"
    );
    assert!(w.held(&issued), "the challenge is kept");
}

/// DEC-2128 item 1 on a deploy: an assertion beside `cli_confirm` evidence, or beside none, is
/// `invalid` with one violation at `/step_up_assertion`, before the port is asked, and nothing is
/// deployed.
#[test]
fn an_assertion_beside_cli_confirm_or_no_evidence_on_a_deploy_is_invalid_and_deploys_nothing() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    confirmed(&f, &cookie, "v1");
    f.asked.0.lock().expect("unpoisoned").clear();
    let signed = Signer::es256().assert(b"any challenge", 9);
    let cli = EVIDENCE.replace("\"passkey\"", "\"cli_confirm\"");
    for evidence in [cli.as_str(), "null"] {
        let body = with_assertion(
            &deploy_body("agt_a", "v1", evidence),
            Some(signed.json(CREDENTIAL)),
        );
        invalid_at_the_assertion(&deploy(&f, &cookie, KEY_2, &body));
    }
    assert!(f.asked.0.lock().expect("unpoisoned").is_empty());
    assert!(events(&f, "AgentDeployed").is_empty());
}

/// A passkey deploy repeated under its key answers as the first though its challenge is spent,
/// and appends no second batch (§3.4 step 1, DEC-2128 item 3).
#[test]
fn a_replayed_passkey_deploy_answers_as_the_first_though_its_challenge_is_gone() {
    let w = passkeyed_with(true);
    w.funded();
    w.confirmed();
    let issued = w.deploy_challenge();
    let signed = w.signer.assert(&issued.bytes, 10);
    let body = w.deploy_body(&issued, Some(signed.json(CREDENTIAL)));
    let first = deploy(&w.f, &w.cookie, KEY_2, &body);
    assert_eq!(
        first.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&first.body)
    );
    let committed = control(&w.f);
    let again = deploy(&w.f, &w.cookie, KEY_2, &body);
    assert_eq!(
        again.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&again.body)
    );
    assert_eq!(control(&w.f).len(), committed.len(), "no second batch");
}

#[test]
fn a_replayed_passkey_confirm_answers_as_the_first_though_its_challenge_is_gone() {
    let w = passkeyed();
    let issued = w.challenge();
    let signed = w.signer.assert(&issued.bytes, 9);
    let body = w.body(&issued, "passkey", Some(signed.json(CREDENTIAL)));
    let first = w.confirm(&body);
    assert_eq!(
        first.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&first.body)
    );
    let committed = control(&w.f);
    assert!(!w.held(&issued));
    let again = w.confirm(&body);
    assert_eq!(
        again.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&again.body)
    );
    assert_eq!(self::body(&again), self::body(&first), "the same answer");
    assert_eq!(control(&w.f), committed, "nothing more is appended");
}

const GATE_ON: &str =
    r#"{"decision":"DEC-2135","enabled":true,"operator":"01JAA3G0A000000000000000P1"}"#;
const GATE_OFF: &str =
    r#"{"decision":null,"enabled":false,"operator":"01JAA3G0A000000000000000P1"}"#;

/// The three routes here that can name `live`, as [DEC-2134] item 1 lists them.
///
/// [DEC-2134]: ../../../docs/project/decisions/DEC-2134.md
const LIVE_ACTIONS: [&str; 3] = ["create_version", "deploy", "go_live"];

/// `action`'s request naming `live`, under `key`; a deploy's live version is recorded first.
fn live_attempt(f: &Fixture, cookie: &str, key: &str, action: &str) -> Reply {
    match action {
        "create_version" => create(f, cookie, key, "live"),
        "deploy" => {
            if events(f, "MandateConfirmed").is_empty() {
                seed_version(f, "live");
            }
            deploy(f, cookie, key, &deploy_body("agt_a", "live", EVIDENCE))
        }
        _ => post(f, &ws("/agents/agt_a/go-live"), cookie, Some(key), b""),
    }
}

/// Each `RealMoneyRefused` on the control stream, read from its body as the journal holds it.
fn real_money_refusals(f: &Fixture) -> Vec<Value> {
    events(f, "RealMoneyRefused")
        .iter()
        .map(|row| json(std::str::from_utf8(&row.body).expect("UTF-8")))
        .collect()
}

/// Nothing a live attempt may never leave: no version, confirmation, deployment, or setting
/// written by the API, and no step-up asked about.
fn nothing_but_refusals(f: &Fixture, settings: usize) {
    for event_type in ["MandateVersionCreated", "MandateConfirmed", "AgentDeployed"] {
        let written = events(f, event_type)
            .iter()
            .filter(|row| {
                json(std::str::from_utf8(&row.body).expect("UTF-8"))
                    .get("actor")
                    .is_some()
            })
            .count();
        assert_eq!(written, 0, "the API wrote a {event_type}");
    }
    assert_eq!(events(f, "RealMoneyGateChanged").len(), settings);
    assert!(f.asked.0.lock().expect("unpoisoned").is_empty());
}

#[test]
fn while_the_gate_is_off_a_live_request_is_journaled_as_refused_then_answered_live_unavailable() {
    for setting in [None, Some(GATE_OFF)] {
        let f = fixture();
        if let Some(payload) = setting {
            seed_control(&f, "RealMoneyGateChanged", GATE_ON);
            seed_control(&f, "RealMoneyGateChanged", payload);
        }
        let cookie = signed_in(&f, alice());
        for (n, action) in LIVE_ACTIONS.into_iter().enumerate() {
            refused(
                &live_attempt(&f, &cookie, KEY, action),
                409,
                ProblemCode::LiveUnavailable,
            );
            let refusals = real_money_refusals(&f);
            assert_eq!(refusals.len(), n + 1, "{action}: one refusal per attempt");
            let refusal = refusals.last().expect("one");
            assert_eq!(text(refusal, &["payload", "action"]), action);
            assert_eq!(text(refusal, &["payload", "requested"]), "live");
            assert_eq!(text(refusal, &["payload", "principal"]), ALICE);
            assert_eq!(text(refusal, &["payload", "reason"]), "gate_off");
            assert_eq!(text(refusal, &["actor", "kind"]), "user");
            assert_eq!(text(refusal, &["actor", "id"]), ALICE);
            assert_eq!(text(refusal, &["environment"]), "paper");
            assert_eq!(
                text(refusal, &["event_id"]),
                derived(&format!("real_money_refused_{action}"), KEY),
                "{action}: the refusal's id is the key's (§3.4)"
            );
        }
        nothing_but_refusals(&f, if setting.is_some() { 2 } else { 0 });
    }
}

#[test]
fn while_the_gate_is_on_a_live_request_passes_it_and_is_still_live_unavailable() {
    let f = fixture();
    seed_control(&f, "RealMoneyGateChanged", GATE_OFF);
    seed_control(&f, "RealMoneyGateChanged", GATE_ON);
    let cookie = signed_in(&f, alice());
    for action in LIVE_ACTIONS {
        refused(
            &live_attempt(&f, &cookie, KEY, action),
            409,
            ProblemCode::LiveUnavailable,
        );
    }
    assert!(real_money_refusals(&f).is_empty());
    nothing_but_refusals(&f, 2);
}

#[test]
fn a_retry_under_its_key_finds_its_refusal_and_a_new_key_records_another() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    for action in LIVE_ACTIONS {
        let before = real_money_refusals(&f).len();
        for key in [KEY, KEY, KEY_2] {
            refused(
                &live_attempt(&f, &cookie, key, action),
                409,
                ProblemCode::LiveUnavailable,
            );
        }
        assert_eq!(real_money_refusals(&f).len(), before + 2, "{action}");
    }
}

/// A refusal that cannot be recorded is still a refusal, never an admission ([DEC-2134] item 4).
///
/// [DEC-2134]: ../../../docs/project/decisions/DEC-2134.md
#[test]
fn a_refusal_the_journal_cannot_take_is_journal_unavailable_and_admits_nothing() {
    let f = fixture();
    let cookie = signed_in(&f, alice());
    seed_version(&f, "live");
    for action in LIVE_ACTIONS {
        for script in [
            Scripted::Unavailable,
            Scripted::Ambiguous,
            Scripted::Conflict,
        ] {
            f.journal.script(&[script; 5]);
            refused(
                &live_attempt(&f, &cookie, KEY, action),
                503,
                ProblemCode::JournalUnavailable,
            );
            f.journal.script.lock().expect("unpoisoned").clear();
        }
    }
    assert!(real_money_refusals(&f).is_empty());
    nothing_but_refusals(&f, 0);
}

/// Every set of the workspace's roles, with no setting, with the gate turned off, and with it
/// on: only the drafting and deploying role (identity spec §4.2's Op column) reaches the gate,
/// every other set is the one 403, nothing is ever committed, and a refusal is journaled exactly
/// when the gate is off and the role reached it. No role sets the gate.
#[test]
fn no_role_set_passes_a_live_request_and_none_sets_the_gate() {
    const ROLES: [Role; 5] = [
        Role::WorkspaceAdmin,
        Role::Operator,
        Role::Approver,
        Role::Viewer,
        Role::Auditor,
    ];
    for mask in 0_u32..(1 << ROLES.len()) {
        let roles: Vec<Role> = ROLES
            .iter()
            .enumerate()
            .filter(|(i, _)| mask & (1 << i) != 0)
            .map(|(_, role)| *role)
            .collect();
        let reaches = roles.contains(&Role::Operator);
        for settings in [&[][..], &[GATE_ON, GATE_OFF][..], &[GATE_OFF, GATE_ON][..]] {
            let f = built_with(Port::Accepting, true, vec![activated(1, alice(), &roles)]);
            seed_version(&f, "live");
            for payload in settings {
                seed_control(&f, "RealMoneyGateChanged", payload);
            }
            let off = settings.last() != Some(&GATE_ON);
            let cookie = signed_in(&f, alice());
            for action in LIVE_ACTIONS {
                let reply = live_attempt(&f, &cookie, KEY, action);
                let label = format!("{roles:?}, {settings:?}, {action}");
                if reaches {
                    refused(&reply, 409, ProblemCode::LiveUnavailable);
                } else {
                    refused(&reply, 403, ProblemCode::Forbidden);
                }
                let journaled = real_money_refusals(&f)
                    .iter()
                    .filter(|r| text(r, &["payload", "action"]) == action)
                    .count();
                assert_eq!(journaled, usize::from(reaches && off), "{label}");
            }
            nothing_but_refusals(&f, settings.len());
        }
    }
}

const KEY_3: &str = "k4Z-9_dddddddddd";

impl Passkeyed {
    /// `agt_a` deployed on `v1` under passkey step-up, so an owner command may name it.
    fn deployed(&self) {
        self.funded();
        self.confirmed();
        let issued = self.deploy_challenge();
        let signed = self.signer.assert(&issued.bytes, 10);
        let reply = deploy(
            &self.f,
            &self.cookie,
            KEY_2,
            &self.deploy_body(&issued, Some(signed.json(CREDENTIAL))),
        );
        assert_eq!(
            reply.status,
            202,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
    }

    /// Alice's challenge of `kind` for `agt_a`'s command that `confirms`, bound to §3.5's object at
    /// the control stream's head now (DEC-2102 item 4).
    fn command_challenge(&self, kind: &str, confirms: &str) -> Issued {
        let head = control(&self.f).last().expect("rows").seq;
        let bound = Digest::of(&to_canonical(&json(&format!(
            r#"{{"agent":"agt_a","bid":null,"bid_size":null,"confirms":"{confirms}","control_head":"{head}","event":null,"floor":null,"instrument":null}}"#
        ))));
        self.challenge_for(kind, bound, "k5Z-9_eeeeeeeeee")
    }

    /// `method` evidence for `issued`.
    fn evidence(issued: &Issued, method: &str) -> String {
        format!(
            r#"{{"assertion_id":"{}","authenticated_at":"{}","method":"{method}"}}"#,
            issued.id, issued.issued_at
        )
    }
}

/// A resume's body under `evidence`, with `assertion` beside it.
fn resume_body(evidence: &str, assertion: Option<serde_json::Value>) -> String {
    with_assertion(
        &format!(r#"{{"record":null,"step_up":{evidence}}}"#),
        assertion,
    )
}

/// A Stop with release's body under `evidence`, naming the platform's warning, with `assertion`.
fn stop_body(evidence: &str, assertion: Option<serde_json::Value>) -> String {
    let warning = format!(
        "sha256:{}",
        Digest::of(mandate_api_server::commands::RELEASE_WARNING.as_bytes()).to_hex()
    );
    with_assertion(
        &format!(
            r#"{{"release":true,"warning_shown":"{warning}","record":null,"step_up":{evidence}}}"#
        ),
        assertion,
    )
}

/// The last two control rows, which a command under passkey step-up appends together: its
/// `OwnerCommandIssued`, then its `StepUpAsserted`, in one batch the journal's batch rules (rule
/// 253) admit, the assertion caused by the command and naming the challenge (DEC-2128 item 3).
fn asserted_command(w: &Passkeyed, issued: &Issued, command: &str) -> Value {
    let rows = control(&w.f);
    let (issued_row, assertion) = (&rows[rows.len() - 2], &rows[rows.len() - 1]);
    assert_eq!(
        (
            issued_row.event_type.as_str(),
            assertion.event_type.as_str()
        ),
        ("OwnerCommandIssued", "StepUpAsserted")
    );
    assert_eq!(issued_row.event_id, derived(command, KEY_3));
    assert_eq!(
        assertion.event_id,
        derived(&format!("{command}_step_up_asserted"), KEY_3),
        "the assertion's id is §3.4's under its own operation"
    );
    let record = parse(&assertion.body).expect("JSON");
    assert_eq!(
        record.get("causation_id").and_then(Value::as_str),
        Some(issued_row.event_id.as_str())
    );
    assert_eq!(text(&payload(assertion), &["challenge"]), issued.id);
    let drafts: Vec<Draft> = [issued_row, assertion]
        .into_iter()
        .map(|row| Draft::parse(&row.body).unwrap_or_else(|e| panic!("{}: {e:?}", row.event_type)))
        .collect();
    assert_eq!(check_batch(&drafts), Ok(()));
    assert!(!w.held(issued), "the challenge is taken");
    payload(issued_row)
}

/// A resume under passkey step-up records its evidence, which the runtime then judges, and commits
/// its `StepUpAsserted` beside it (DEC-2128 items 1 to 3 applied to resume, DEC-2102 item 4).
#[test]
fn a_passkey_resume_records_its_evidence_then_its_assertion() {
    let w = passkeyed_with(true);
    w.deployed();
    let issued = w.command_challenge("resume", "resume");
    let signed = w.signer.assert(&issued.bytes, 11);
    let evidence = Passkeyed::evidence(&issued, "passkey");
    let reply = post(
        &w.f,
        &ws("/agents/agt_a/resume"),
        &w.cookie,
        Some(KEY_3),
        resume_body(&evidence, Some(signed.json(CREDENTIAL))).as_bytes(),
    );
    assert_eq!(
        reply.status,
        202,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let command = asserted_command(&w, &issued, "resume");
    assert_eq!(text(&command, &["command"]), "resume");
    assert_eq!(
        command.get("step_up").map(to_canonical),
        Some(to_canonical(&json(&evidence)))
    );
}

/// A Stop with release under passkey step-up, bound to what it confirms, as the resume.
#[test]
fn a_passkey_stop_with_release_records_its_evidence_then_its_assertion() {
    let w = passkeyed_with(true);
    w.deployed();
    let issued = w.command_challenge("stop", "stop_release");
    let signed = w.signer.assert(&issued.bytes, 11);
    let evidence = Passkeyed::evidence(&issued, "passkey");
    let reply = post(
        &w.f,
        &ws("/agents/agt_a/stop"),
        &w.cookie,
        Some(KEY_3),
        stop_body(&evidence, Some(signed.json(CREDENTIAL))).as_bytes(),
    );
    assert_eq!(
        reply.status,
        202,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let command = asserted_command(&w, &issued, "stop");
    assert_eq!(text(&command, &["command"]), "stop");
    assert_eq!(command.get("release"), Some(&Value::Bool(true)));
    assert_eq!(
        command.get("step_up").map(to_canonical),
        Some(to_canonical(&json(&evidence)))
    );
}

/// An assertion beside evidence that is not a passkey's is `invalid` at `/step_up_assertion`, and
/// nothing is recorded (DEC-2128 item 1).
#[test]
fn an_assertion_beside_cli_confirm_evidence_on_a_resume_or_stop_is_invalid() {
    let w = passkeyed_with(true);
    w.deployed();
    let before = control(&w.f).len();
    let issued = w.command_challenge("resume", "resume");
    let signed = w.signer.assert(&issued.bytes, 11);
    let cli = Passkeyed::evidence(&issued, "cli_confirm");
    for (path, body) in [
        (
            "/agents/agt_a/resume",
            resume_body(&cli, Some(signed.json(CREDENTIAL))),
        ),
        (
            "/agents/agt_a/stop",
            stop_body(&cli, Some(signed.json(CREDENTIAL))),
        ),
        (
            "/agents/agt_a/resume",
            resume_body("null", Some(signed.json(CREDENTIAL))),
        ),
    ] {
        let reply = post(&w.f, &ws(path), &w.cookie, Some(KEY_3), body.as_bytes());
        invalid_at_the_assertion(&reply);
    }
    assert_eq!(control(&w.f).len(), before, "nothing is recorded");
    assert!(w.held(&issued), "nothing is taken");
}

/// A passkey resume whose assertion the port refuses, here one signed over another challenge, or
/// that carries none, is still recorded, with `null` evidence that the runtime refuses, and commits
/// no `StepUpAsserted` (DEC-2102 item 4): the brake's record is never lost to its step-up.
#[test]
fn an_unverified_passkey_resume_is_recorded_with_null_evidence_and_no_assertion() {
    let w = passkeyed_with(true);
    w.deployed();
    let issued = w.command_challenge("resume", "resume");
    let evidence = Passkeyed::evidence(&issued, "passkey");
    let forged = w.signer.assert(b"another challenge", 11);
    for (key, assertion) in [
        (KEY_3, Some(forged.json(CREDENTIAL))),
        ("k6Z-9_ffffffffff", None),
    ] {
        let reply = post(
            &w.f,
            &ws("/agents/agt_a/resume"),
            &w.cookie,
            Some(key),
            resume_body(&evidence, assertion).as_bytes(),
        );
        assert_eq!(
            reply.status,
            202,
            "{}",
            String::from_utf8_lossy(&reply.body)
        );
        let rows = control(&w.f);
        let last = rows.last().expect("rows");
        assert_eq!(last.event_type, "OwnerCommandIssued");
        assert_eq!(payload(last).get("step_up"), Some(&Value::Null));
    }
    assert!(
        events(&w.f, "StepUpAsserted").len() == 2,
        "only the confirm's and the deploy's"
    );
    assert!(w.held(&issued), "the challenge is kept");
}

/// A passkey resume sent again under its key answers as the first did, though its challenge is
/// gone, and appends nothing: no second command and no second `StepUpAsserted` (§3.4 step 1,
/// DEC-2140 item 3).
#[test]
fn a_replayed_passkey_resume_answers_as_the_first_and_appends_nothing() {
    let w = passkeyed_with(true);
    w.deployed();
    let issued = w.command_challenge("resume", "resume");
    let signed = w.signer.assert(&issued.bytes, 11);
    let body = resume_body(
        &Passkeyed::evidence(&issued, "passkey"),
        Some(signed.json(CREDENTIAL)),
    );
    let send = || {
        post(
            &w.f,
            &ws("/agents/agt_a/resume"),
            &w.cookie,
            Some(KEY_3),
            body.as_bytes(),
        )
    };
    let first = send();
    assert_eq!(
        first.status,
        202,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let after = control(&w.f).len();
    let again = send();
    assert_eq!(
        again.status,
        202,
        "{}",
        String::from_utf8_lossy(&again.body)
    );
    assert_eq!(
        text(&self::body(&again), &["command_id"]),
        text(&self::body(&first), &["command_id"])
    );
    assert_eq!(control(&w.f).len(), after, "nothing more is appended");
}
