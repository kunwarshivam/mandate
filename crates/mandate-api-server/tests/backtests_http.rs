//! `GET /v1/workspaces/{ws}/backtests/{id}` and `…/evaluations/{id}` (§4.2, §4.7, API-2, API-9,
//! API-14; DEC-1001 item 5, DEC-1002 items 6 and 7, DEC-1143): authenticated by the session,
//! authorized from the control stream's memberships, folded from its rows, with the report read
//! from the store and checked against its digest.
//!
//! The oracle for every body is the schema's own example (which `check_examples.py` validates
//! against the read model's schema), with the reference to the report as the test stores it, and
//! the members each schema requires and allows, read from the schema files.

mod backtest_rows;
mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use backtest_rows::{
    BACKTEST_EXAMPLE, BACKTEST_ID, BACKTEST_SCHEMA, BACKTEST_VALID, ENVELOPE_SCHEMA,
    EVALUATION_EXAMPLE, EVALUATION_ID, EVALUATION_SCHEMA, at, backtest_payload, ctl,
    evaluation_payload, example_report, example_version, reference, row, row_in, set,
};
use common::{MemJournal, Reply, WORKSPACE, send, serve};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, PROBLEM_JSON, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{LookupFailed, MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_identity_seal::Seal;
use mandate_journal::{ArtifactSource, StoredEvent};
use mandate_time::UtcNanos;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";

type Json = serde_json::Value;
type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;
type Store = Arc<Mutex<BTreeMap<Digest, Vec<u8>>>>;

/// A store the test can change after the server starts.
struct Shared(Store);

impl ArtifactSource for Shared {
    fn read_artifact(
        &self,
        reference: &mandate_journal::ArtifactRef,
    ) -> Result<Vec<u8>, mandate_journal::ArtifactError> {
        self.0.lock().expect("unpoisoned").read_artifact(reference)
    }
}

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    records: Records,
    sessions: Arc<Sessions>,
    store: Store,
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// What a fixture is built with.
#[derive(Clone, Copy)]
struct Deployment {
    internal_paper: bool,
    with_store: bool,
}

const PLAIN: Deployment = Deployment {
    internal_paper: false,
    with_store: true,
};

const INTERNAL: Deployment = Deployment {
    internal_paper: true,
    with_store: true,
};

fn fixture(deployment: Deployment) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-backtests-{}-{n}",
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
    let records: Records = Arc::new(Mutex::new(Ok(Vec::new())));
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let store: Store = Arc::new(Mutex::new(BTreeMap::new()));
    let app = app(&dir, deployment, &journal, &records, &sessions, &store);
    Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        records,
        sessions,
        store,
    }
}

/// The one place the test builds its `App`.
fn app(
    dir: &std::path::Path,
    deployment: Deployment,
    journal: &Arc<MemJournal>,
    records: &Records,
    sessions: &Arc<Sessions>,
    store: &Store,
) -> App {
    let parsed = Arc::clone(records);
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let mut hosted = common::hosted();
    hosted.internal_paper = deployment.internal_paper;
    let artifacts: Option<Arc<dyn ArtifactSource + Send + Sync>> = deployment
        .with_store
        .then(|| Arc::new(Shared(Arc::clone(store))) as Arc<dyn ArtifactSource + Send + Sync>);
    App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: common::ManualClock::at(T0) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| parsed.lock().expect("unpoisoned").clone()),
        hosted,
        artifacts,
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
    }
}

fn alice() -> PrincipalId {
    PrincipalId::from_ulid_text("01JB3K6F0C9R2V7N4M8Q1T5W3Z").expect("a ULID")
}

fn bob() -> PrincipalId {
    PrincipalId::from_ulid_text("01JB3K6F0C9R2V7N4M8Q1T5W41").expect("a ULID")
}

fn at_secs(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn member(f: &Fixture, who: PrincipalId, roles: &[Role]) {
    let record = MembershipRecord::new(
        Seal::grant(),
        1,
        at_secs(T0 - 1_000),
        MembershipEvent::Activated {
            member: who,
            invitation: None,
            roles: roles.iter().copied().collect::<BTreeSet<_>>(),
            independent_approval_required: false,
            cool_off_ends_at: at_secs(T0 - 1_000),
        },
    );
    *f.records.lock().expect("unpoisoned") = Ok(vec![record]);
}

fn signed_in(f: &Fixture, who: PrincipalId) -> String {
    f.sessions
        .open(who, "sha256:aaaa".to_owned(), None, at_secs(T0))
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

/// The reply's status is `status`; the message carries the body.
fn status(reply: &Reply, status: u16, what: &str) {
    assert_eq!(
        reply.status,
        status,
        "{what}: {}",
        String::from_utf8_lossy(&reply.body)
    );
}

/// A 200's JSON body.
fn ok(reply: &Reply, what: &str) -> Value {
    status(reply, 200, what);
    assert_eq!(reply.header("content-type"), Some("application/json"));
    parse(&reply.body).expect("a JSON body")
}

/// A problem document's code, at `status`.
fn problem(reply: &Reply, code: u16, what: &str) -> String {
    status(reply, code, what);
    assert_eq!(reply.header("content-type"), Some(PROBLEM_JSON), "{what}");
    let body = parse(&reply.body).expect("a JSON body");
    at(&body, "code").as_str().expect("a code").to_owned()
}

/// A schema file, or a served body, as `serde_json` reads it: the schemas' keys (`$defs`,
/// `Backtest`) are outside `mandate-canon`'s key grammar.
fn json(text: &[u8]) -> Json {
    serde_json::from_slice(text).expect("JSON")
}

fn names(value: &Json) -> BTreeSet<String> {
    value
        .as_object()
        .expect("an object")
        .keys()
        .cloned()
        .collect()
}

fn listed(value: &Json) -> BTreeSet<String> {
    value
        .as_array()
        .expect("a list")
        .iter()
        .map(|name| name.as_str().expect("a name").to_owned())
        .collect()
}

/// The body against `schema`: the envelope's required members and the response's, nothing the
/// schema does not list, the item's members exactly its `$defs` entry's required (and closed)
/// list, and an `as_of` naming the control stream's last row.
fn conforms(body: &Value, schema: &str, item: &str, def: &str, control: &[StoredEvent]) {
    let body = json(&to_canonical(body));
    let schema = json(schema.as_bytes());
    let envelope = json(ENVELOPE_SCHEMA.as_bytes());
    let mut required = listed(&envelope["$defs"]["Envelope"]["required"]);
    required.extend(listed(&schema["required"]));
    let mut allowed = names(&envelope["$defs"]["Envelope"]["properties"]);
    allowed.extend(names(&schema["properties"]));
    let served = names(&body);
    assert!(required.is_subset(&served), "{required:?} in {served:?}");
    assert!(served.is_subset(&allowed), "{served:?} within {allowed:?}");
    let shape = &schema["$defs"][def];
    assert_eq!(shape["additionalProperties"], Json::Bool(false));
    assert_eq!(names(&body[item]), listed(&shape["required"]));
    assert_eq!(body["api_version"], "v1");
    assert_eq!(body["served_at"], "2026-10-03T04:00:00.000000000Z");
    let last = control.last().expect("a row");
    let mark = serde_json::json!([{
        "stream_id": last.stream_id,
        "seq": last.seq,
        "hash": format!("sha256:{}", last.hash),
        "recorded_at": last.recorded_at,
    }]);
    assert_eq!(body["as_of"], mark);
}

/// The example's `member`, with `report_ref` the reference the test stores the report under.
fn expected(example: &str, member: &str, report_ref: &str) -> Value {
    let mut value = parse(example.as_bytes()).expect("the example");
    set(
        &mut value,
        &format!("{member}/report_ref"),
        Value::Str(report_ref.into()),
    );
    at(&value, member).clone()
}

/// The control stream: opened, then `records`.
fn seed(f: &Fixture, records: Vec<StoredEvent>) -> Vec<StoredEvent> {
    let mut rows = vec![row(1, "StreamOpened", "{}")];
    rows.extend(records);
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .insert(ctl(), rows.clone());
    rows
}

fn store(f: &Fixture, bytes: Vec<u8>) {
    f.store
        .lock()
        .expect("unpoisoned")
        .insert(Digest::of(&bytes), bytes);
}

fn completed(seq: u64, report: &str) -> StoredEvent {
    row(
        seq,
        "BacktestRunRecorded",
        &backtest_payload(
            BACKTEST_ID,
            &example_version(),
            "completed",
            Some(report),
            None,
        ),
    )
}

fn refused(seq: u64) -> StoredEvent {
    row(
        seq,
        "BacktestRunRecorded",
        &backtest_payload(
            "bt_refused",
            &example_version(),
            "refused",
            None,
            Some("corporate_action_in_range"),
        ),
    )
}

fn evaluated(seq: u64, report: &str) -> StoredEvent {
    row(
        seq,
        "EvaluationRunRecorded",
        &evaluation_payload(EVALUATION_ID, report),
    )
}

#[test]
fn a_completed_backtest_serves_its_report_from_the_store() {
    let f = fixture(PLAIN);
    let (bytes, report) = example_report(BACKTEST_EXAMPLE, "backtest");
    store(&f, bytes);
    let control = seed(&f, vec![completed(2, &report), refused(3)]);
    member(&f, alice(), &[Role::Viewer]);
    let cookie = signed_in(&f, alice());
    let reply = get(&f, &ws(&format!("/backtests/{BACKTEST_ID}")), Some(&cookie));
    let body = ok(&reply, "completed");
    assert_eq!(reply.header("access-control-allow-origin"), Some(APP));
    assert_eq!(
        at(&body, "backtest"),
        &expected(BACKTEST_EXAMPLE, "backtest", &report)
    );
    conforms(&body, BACKTEST_SCHEMA, "backtest", "Backtest", &control);
}

#[test]
fn a_refused_backtest_serves_its_code_and_no_report_even_without_a_store() {
    for with_store in [true, false] {
        let f = fixture(Deployment {
            internal_paper: false,
            with_store,
        });
        let control = seed(&f, vec![refused(2)]);
        member(&f, alice(), &[Role::Operator]);
        let cookie = signed_in(&f, alice());
        let body = ok(
            &get(&f, &ws("/backtests/bt_refused"), Some(&cookie)),
            "refused",
        );
        let cases = json(BACKTEST_VALID.as_bytes());
        let case = cases
            .as_array()
            .expect("a list")
            .iter()
            .find(|case| case["label"] == "a refused backtest")
            .expect("the refused case");
        let mut value = json(BACKTEST_EXAMPLE.as_bytes());
        for (pointer, to) in case["set"].as_object().expect("an object") {
            *value.pointer_mut(pointer).expect("the member") = to.clone();
        }
        value["backtest"]["backtest_id"] = Json::from("bt_refused");
        assert_eq!(
            json(&to_canonical(at(&body, "backtest"))),
            value["backtest"],
            "{with_store}"
        );
        conforms(&body, BACKTEST_SCHEMA, "backtest", "Backtest", &control);
    }
}

#[test]
fn an_unknown_id_another_workspace_and_no_membership_are_the_same_404() {
    let f = fixture(INTERNAL);
    let (bytes, report) = example_report(BACKTEST_EXAMPLE, "backtest");
    store(&f, bytes);
    let (bytes, evaluation) = example_report(EVALUATION_EXAMPLE, "evaluation");
    store(&f, bytes);
    seed(&f, vec![completed(2, &report), evaluated(3, &evaluation)]);
    member(&f, alice(), &[Role::Operator]);
    let cookie = signed_in(&f, alice());
    let absent = get(&f, &ws("/backtests/bt_absent"), Some(&cookie));
    assert_eq!(problem(&absent, 404, "absent"), "not_found");
    let other = |kind: &str, id: &str| format!("/v1/workspaces/{OTHER_WORKSPACE}/{kind}/{id}");
    for path in [
        ws("/evaluations/ev_absent"),
        ws(&format!("/backtests/{}", "a".repeat(65))),
        ws("/backtests/bt%2Fx"),
        other("backtests", BACKTEST_ID),
        other("evaluations", EVALUATION_ID),
        format!("/v1/workspaces/not-a-ulid/backtests/{BACKTEST_ID}"),
        ws(&format!("/backtests/{EVALUATION_ID}")),
        ws(&format!("/evaluations/{BACKTEST_ID}")),
    ] {
        let reply = get(&f, &path, Some(&cookie));
        status(&reply, 404, &path);
        assert_eq!(reply.body, absent.body, "{path}");
    }
    member(&f, bob(), &[Role::Operator]);
    for path in [
        ws(&format!("/backtests/{BACKTEST_ID}")),
        ws(&format!("/evaluations/{EVALUATION_ID}")),
    ] {
        let reply = get(&f, &path, Some(&cookie));
        status(&reply, 404, "alice holds no membership here");
        assert_eq!(reply.body, absent.body, "{path}");
    }
}

#[test]
fn without_a_session_both_reads_are_the_one_401_and_a_member_without_the_view_row_403() {
    let f = fixture(INTERNAL);
    let (bytes, report) = example_report(BACKTEST_EXAMPLE, "backtest");
    store(&f, bytes);
    seed(&f, vec![completed(2, &report)]);
    member(&f, alice(), &[]);
    let one = send(f.addr, "GET", "/nowhere", &[], b"").body;
    let cookie = signed_in(&f, alice());
    for path in [
        ws(&format!("/backtests/{BACKTEST_ID}")),
        ws(&format!("/evaluations/{EVALUATION_ID}")),
        ws("/backtests/%FF"),
    ] {
        for presented in [None, Some("forged")] {
            let reply = get(&f, &path, presented);
            status(&reply, 401, &path);
            assert_eq!(reply.body, one, "{path}");
        }
    }
    for path in [
        ws(&format!("/backtests/{BACKTEST_ID}")),
        ws(&format!("/evaluations/{EVALUATION_ID}")),
    ] {
        let reply = get(&f, &path, Some(&cookie));
        assert_eq!(problem(&reply, 403, &path), "forbidden");
    }
    let undecodable = get(&f, &ws("/evaluations/%FF"), Some(&cookie));
    assert_eq!(problem(&undecodable, 404, "undecodable"), "not_found");
}

#[test]
fn a_completed_run_whose_report_is_absent_or_fails_its_check_is_503_never_a_null_report() {
    let f = fixture(PLAIN);
    let (bytes, report) = example_report(BACKTEST_EXAMPLE, "backtest");
    seed(&f, vec![completed(2, &report)]);
    member(&f, alice(), &[Role::Operator]);
    let cookie = signed_in(&f, alice());
    let path = ws(&format!("/backtests/{BACKTEST_ID}"));
    let reply = get(&f, &path, Some(&cookie));
    assert_eq!(problem(&reply, 503, "absent"), "journal_unavailable");
    let mut corrupt = bytes.clone();
    corrupt.push(b' ');
    f.store
        .lock()
        .expect("unpoisoned")
        .insert(Digest::of(&bytes), corrupt);
    let reply = get(&f, &path, Some(&cookie));
    assert_eq!(problem(&reply, 503, "corrupt"), "journal_unavailable");
    let mut other = parse(&bytes).expect("JSON");
    set(&mut other, "mandate_version", Value::Str(reference(0x77)));
    let other = mandate_canon::to_canonical(&other);
    let other_ref = format!("sha256:{}", Digest::of(&other).to_hex());
    store(&f, other);
    seed(&f, vec![completed(2, &other_ref)]);
    let reply = get(&f, &path, Some(&cookie));
    assert_eq!(
        problem(&reply, 503, "another version's report"),
        "journal_unavailable"
    );
    store(&f, bytes);
    seed(&f, vec![completed(2, &report)]);
    ok(&get(&f, &path, Some(&cookie)), "once stored");

    let without = fixture(Deployment {
        internal_paper: false,
        with_store: false,
    });
    seed(&without, vec![completed(2, &report)]);
    member(&without, alice(), &[Role::Operator]);
    let cookie = signed_in(&without, alice());
    let reply = get(&without, &path, Some(&cookie));
    assert_eq!(problem(&reply, 503, "no store"), "journal_unavailable");
}

#[test]
fn a_second_record_with_the_same_id_is_503_and_a_malformed_one_too() {
    let f = fixture(PLAIN);
    let (bytes, report) = example_report(BACKTEST_EXAMPLE, "backtest");
    store(&f, bytes);
    seed(&f, vec![completed(2, &report), completed(3, &report)]);
    member(&f, alice(), &[Role::Operator]);
    let cookie = signed_in(&f, alice());
    let path = ws(&format!("/backtests/{BACKTEST_ID}"));
    let reply = get(&f, &path, Some(&cookie));
    assert_eq!(problem(&reply, 503, "twice"), "journal_unavailable");
    let mut malformed = completed(2, &report);
    malformed.schema_version = 2;
    seed(&f, vec![malformed]);
    let reply = get(&f, &path, Some(&cookie));
    assert_eq!(problem(&reply, 503, "malformed"), "journal_unavailable");
}

#[test]
fn a_control_stream_that_cannot_be_read_is_503_never_absent() {
    let f = fixture(INTERNAL);
    seed(&f, vec![]);
    member(&f, alice(), &[Role::Operator]);
    let cookie = signed_in(&f, alice());
    f.journal.failing.lock().expect("unpoisoned").insert(ctl());
    for path in [
        ws(&format!("/backtests/{BACKTEST_ID}")),
        ws(&format!("/evaluations/{EVALUATION_ID}")),
    ] {
        let reply = get(&f, &path, Some(&cookie));
        assert_eq!(problem(&reply, 503, &path), "membership_unavailable");
    }
}

#[test]
fn an_evaluation_is_served_in_an_internal_paper_workspace() {
    let f = fixture(INTERNAL);
    let (bytes, report) = example_report(EVALUATION_EXAMPLE, "evaluation");
    store(&f, bytes);
    let control = seed(&f, vec![evaluated(2, &report)]);
    member(&f, alice(), &[Role::Viewer]);
    let cookie = signed_in(&f, alice());
    let body = ok(
        &get(
            &f,
            &ws(&format!("/evaluations/{EVALUATION_ID}")),
            Some(&cookie),
        ),
        "on",
    );
    assert_eq!(
        at(&body, "evaluation"),
        &expected(EVALUATION_EXAMPLE, "evaluation", &report)
    );
    conforms(
        &body,
        EVALUATION_SCHEMA,
        "evaluation",
        "Evaluation",
        &control,
    );
}

#[test]
fn with_the_setting_off_an_evaluation_is_the_404_of_an_absent_id_even_when_recorded() {
    let f = fixture(PLAIN);
    let (bytes, report) = example_report(EVALUATION_EXAMPLE, "evaluation");
    store(&f, bytes);
    seed(&f, vec![evaluated(2, &report)]);
    member(&f, alice(), &[Role::Operator]);
    let cookie = signed_in(&f, alice());
    let absent = get(&f, &ws("/backtests/bt_absent"), Some(&cookie));
    status(&absent, 404, "absent");
    for id in [EVALUATION_ID, "ev_absent"] {
        let reply = get(&f, &ws(&format!("/evaluations/{id}")), Some(&cookie));
        status(&reply, 404, id);
        assert_eq!(reply.body, absent.body, "{id}");
    }
}

#[test]
fn with_the_setting_on_a_stream_that_is_not_paper_throughout_answers_404() {
    let f = fixture(INTERNAL);
    let (bytes, report) = example_report(EVALUATION_EXAMPLE, "evaluation");
    store(&f, bytes);
    member(&f, alice(), &[Role::Operator]);
    let cookie = signed_in(&f, alice());
    let path = ws(&format!("/evaluations/{EVALUATION_ID}"));
    let live = row_in(
        "live",
        2,
        "EvaluationRunRecorded",
        &evaluation_payload(EVALUATION_ID, &report),
    );
    seed(&f, vec![live]);
    assert_eq!(
        problem(&get(&f, &path, Some(&cookie)), 404, "live"),
        "not_found"
    );
    seed(
        &f,
        vec![
            evaluated(2, &report),
            row_in("live", 3, "AgentDeployed", "{}"),
        ],
    );
    assert_eq!(
        problem(&get(&f, &path, Some(&cookie)), 404, "mixed"),
        "not_found"
    );
    seed(&f, vec![evaluated(2, &report)]);
    ok(&get(&f, &path, Some(&cookie)), "paper throughout");
}

#[test]
fn an_evaluation_whose_report_is_absent_fails_its_check_or_is_doubled_is_503() {
    let f = fixture(INTERNAL);
    let (bytes, report) = example_report(EVALUATION_EXAMPLE, "evaluation");
    seed(&f, vec![evaluated(2, &report)]);
    member(&f, alice(), &[Role::Operator]);
    let cookie = signed_in(&f, alice());
    let path = ws(&format!("/evaluations/{EVALUATION_ID}"));
    assert_eq!(
        problem(&get(&f, &path, Some(&cookie)), 503, "absent"),
        "journal_unavailable"
    );
    let mut corrupt = bytes.clone();
    corrupt.push(b' ');
    f.store
        .lock()
        .expect("unpoisoned")
        .insert(Digest::of(&bytes), corrupt);
    assert_eq!(
        problem(&get(&f, &path, Some(&cookie)), 503, "corrupt"),
        "journal_unavailable"
    );
    store(&f, bytes);
    ok(&get(&f, &path, Some(&cookie)), "stored");
    seed(&f, vec![evaluated(2, &report), evaluated(3, &report)]);
    assert_eq!(
        problem(&get(&f, &path, Some(&cookie)), 503, "twice"),
        "journal_unavailable"
    );
    let (backtest_bytes, backtest_ref) = example_report(BACKTEST_EXAMPLE, "backtest");
    store(&f, backtest_bytes);
    seed(&f, vec![evaluated(2, &backtest_ref)]);
    assert_eq!(
        problem(&get(&f, &path, Some(&cookie)), 503, "a backtest's report"),
        "journal_unavailable"
    );
    let without = fixture(Deployment {
        internal_paper: true,
        with_store: false,
    });
    seed(&without, vec![evaluated(2, &report)]);
    member(&without, alice(), &[Role::Operator]);
    let cookie = signed_in(&without, alice());
    assert_eq!(
        problem(&get(&without, &path, Some(&cookie)), 503, "no store"),
        "journal_unavailable"
    );
}
