//! Resume, Stop, owner exit, acknowledgment, and command status over HTTP (§3.11 item 12, §4.2,
//! §5.4, §5.5, API-6, API-7, API-12; DEC-2102): recorded as the CLI records them, with step-up
//! forwarded only once the port verifies it, and status read from the owning streams' direct
//! causation alone. Expected payloads, digests, and phases are written out here from the spec.

mod common;

use std::collections::BTreeSet;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
use mandate_api::envelope::StepUpEvidence;
use mandate_api::idempotency::{Derivation, IdempotencyKey, event_id};
use mandate_api::problem::{Effect, Problem, ProblemCode, Violation};
use mandate_api::wire::{Id, decode};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::status::{Phase, phase, phase_of, reason};
use mandate_api_server::stepup::{Action, Judged, StepUp, Unbound};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role, StepUpActionKind};
use mandate_identity_seal::Seal;
use mandate_journal::{Draft, StoredEvent};
use mandate_time::UtcNanos;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const KEY: &str = "k1Z-9_aaaaaaaaaa";
const KEY_2: &str = "k2Z-9_bbbbbbbbbb";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const BOB: &str = "01JB3K6F0C9R2V7N4M8Q1T5W41";
const SPY: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const EVIDENCE: &str = r#"{"assertion_id":"01JBH3BTZ8K2M4N6P8R0S2T4V6","authenticated_at":"2026-10-08T14:30:00.000000000Z","method":"passkey"}"#;
/// The CLI's release warning (`mandate agent stop --release`, DEC-136), written out.
const WARNING: &str = "The positions become yours and unprotected: this agent's protective orders are canceled and nothing watches them. This warning is recorded with your confirmation.";
const ACKNOWLEDGED: &str = "01JBH3BTZ8K2M4N6P8R0S2T9Z9";

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
    sessions: Arc<Sessions>,
    asked: Arc<Accepting>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn fixture(accepting: bool) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::env::temp_dir().join(format!(
        "mandate-api-server-owner-{}-{n}",
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
    let records = vec![
        activated(1, alice(), &[Role::Operator]),
        activated(2, bob(), &[Role::Viewer]),
    ];
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let asked = Arc::new(Accepting::default());
    let step_up: Arc<dyn StepUp> = if accepting {
        Arc::clone(&asked) as Arc<dyn StepUp>
    } else {
        Arc::new(Unbound)
    };
    let app = App {
        settings: Settings {
            app_origin: APP.to_owned(),
        },
        sessions: Arc::clone(&sessions),
        tokens: Arc::new(Issuer::new(config, Arc::new(FixedKeys(None)))),
        links: Arc::new(common::LinksAt(dir.join("absent"))),
        clock: common::ManualClock::at(T0) as Arc<dyn mandate_api_server::clock::Clock>,
        journal: Arc::clone(&journal) as Arc<dyn mandate_api_server::journal::Journal>,
        members: Arc::new(move |_: &[StoredEvent]| Ok(records.clone())),
        hosted: hosted(),
        artifacts: None,
        objects: None,
        step_up,
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
        bars: None,
    };
    let f = Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
        asked,
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

fn agent_stream() -> String {
    format!("agent:{WORKSPACE}:agt_a")
}

fn account() -> String {
    format!("acct:{WORKSPACE}:acct1")
}

fn json(text: &str) -> Value {
    parse(text.as_bytes()).expect("JSON")
}

fn reference(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

fn row(
    stream: &str,
    seq: u64,
    event_type: &str,
    payload: &str,
    cause: Option<&str>,
) -> StoredEvent {
    let event_id = format!("01JBH3BTZ8K2M4N6P8R0S2{seq:04}");
    let cause = cause.map_or("null".to_owned(), |c| format!("\"{c}\""));
    let text = format!(
        r#"{{"causation_id":{cause},"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
    );
    let body = to_canonical(&json(&text));
    StoredEvent {
        stream_id: stream.to_owned(),
        seq,
        event_id,
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: format!("2026-10-17T10:00:{:02}.000000000Z", seq % 60),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// A paper workspace with the agent `agt_a` on the connection's account `acct1`.
fn seed(f: &Fixture) {
    let mut streams = f.journal.streams.lock().expect("unpoisoned");
    streams.insert(
        ctl(),
        vec![
            row(&ctl(), 1, "StreamOpened", "{}", None),
            row(
                &ctl(),
                2,
                "AgentDeployed",
                r#"{"agent_id":"agt_a","mandate_version":"sha256:01"}"#,
                None,
            ),
            row(
                &ctl(),
                3,
                "ConnectionRequested",
                r#"{"account_ref":"acct1","connection_id":"conn_1"}"#,
                None,
            ),
        ],
    );
    streams.insert(
        agent_stream(),
        vec![row(&agent_stream(), 1, "StreamOpened", "{}", None)],
    );
    streams.insert(
        account(),
        vec![row(&account(), 1, "StreamOpened", "{}", None)],
    );
}

fn ws(path: &str) -> String {
    format!("/v1/workspaces/{WORKSPACE}{path}")
}

fn post(f: &Fixture, path: &str, cookie: &str, key: &str, body: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "POST",
        path,
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Cookie", cookie.as_str()),
            ("Idempotency-Key", key),
        ],
        body.as_bytes(),
    )
}

fn get(f: &Fixture, path: &str, cookie: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(f.addr, "GET", path, &[("Cookie", cookie.as_str())], b"")
}

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

fn schema_codes(reply: &Reply) -> Vec<(String, String)> {
    assert_eq!(
        reply.status,
        422,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    problem(reply)
        .violations
        .into_iter()
        .filter_map(|v| match v {
            Violation::Schema { path, code, .. } => Some((path, code)),
            _ => None,
        })
        .collect()
}

/// The control stream's last row, which must be the one the server wrote, checked by the
/// journal's own draft rules.
fn written(f: &Fixture) -> (StoredEvent, Value) {
    let row = f.journal.rows_of(&ctl()).last().cloned().expect("a row");
    Draft::parse(&row.body).unwrap_or_else(|e| panic!("{}: {e:?}", row.event_type));
    let event = parse(&row.body).expect("JSON");
    (row, event)
}

fn member<'a>(value: &'a Value, path: &[&str]) -> &'a Value {
    path.iter()
        .try_fold(value, |v, k| v.get(k))
        .unwrap_or_else(|| panic!("{path:?} in {value:?}"))
}

/// The OwnerCommandIssued payload §9.11 and the CLI's `issued` give these members.
fn issued(command: &str, scope: &str, subject: &str, rest: &str, step_up: &str) -> Value {
    json(&format!(
        r#"{{"agent":"agt_a","command":"{command}","scope":"{scope}","subject":"{subject}",{rest},"user":"{ALICE}","submitted_at":{T0},"step_up":{step_up}}}"#
    ))
}

const NO_BID: &str = r#""bid":null,"bid_size":null,"floor":null"#;

/// DEC-279 item 1's object for `confirms`, at the control stream's head `head`, digested.
fn bound(confirms: &str, head: u64) -> Digest {
    Digest::of(&to_canonical(&json(&format!(
        r#"{{"confirms":"{confirms}","agent":"agt_a","control_head":"{head}","instrument":null,"bid":null,"bid_size":null,"floor":null,"event":null}}"#
    ))))
}

#[test]
fn every_route_without_a_session_is_the_one_401() {
    let f = fixture(false);
    for (method, path) in [
        ("POST", ws("/agents/agt_a/resume")),
        ("POST", ws("/agents/agt_a/stop")),
        ("POST", ws("/agents/agt_a/exits")),
        ("GET", ws(&format!("/commands/{ACKNOWLEDGED}"))),
    ] {
        let reply = send(f.addr, method, &path, &[("Origin", APP)], b"{}");
        refused(&reply, 401, ProblemCode::Unauthenticated);
    }
}

#[test]
fn a_resume_forwards_null_until_the_port_verifies_its_evidence() {
    let f = fixture(false);
    let cookie = signed_in(&f, alice());
    let reply = post(
        &f,
        &ws("/agents/agt_a/resume"),
        &cookie,
        KEY,
        &format!(r#"{{"record":null,"step_up":{EVIDENCE}}}"#),
    );
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(
        member(&body(&reply), &["command_id"]),
        &Value::Str(derived("resume", KEY))
    );
    let (row, event) = written(&f);
    assert_eq!(row.event_type, "OwnerCommandIssued");
    let rest = format!(r#""release":null,"warning_shown":null,{NO_BID}"#);
    assert_eq!(
        member(&event, &["payload"]),
        &issued("resume", "agent", "agt_a", &rest, "null")
    );
}

#[test]
fn a_verified_resume_forwards_its_evidence_bound_to_the_command_and_replays_by_key() {
    let f = fixture(true);
    let cookie = signed_in(&f, alice());
    let body_text = format!(r#"{{"record":null,"step_up":{EVIDENCE}}}"#);
    let reply = post(&f, &ws("/agents/agt_a/resume"), &cookie, KEY, &body_text);
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let (_, event) = written(&f);
    let rest = format!(r#""release":null,"warning_shown":null,{NO_BID}"#);
    assert_eq!(
        member(&event, &["payload"]),
        &issued("resume", "agent", "agt_a", &rest, EVIDENCE)
    );
    assert_eq!(
        *f.asked.0.lock().expect("unpoisoned"),
        vec![(StepUpActionKind::Resume, bound("resume", 3))]
    );
    let again = post(&f, &ws("/agents/agt_a/resume"), &cookie, KEY, &body_text);
    assert_eq!(again.status, 202);
    assert_eq!(
        f.asked.0.lock().expect("unpoisoned").len(),
        1,
        "a replay asks the port nothing"
    );
    assert_eq!(f.journal.rows_of(&ctl()).len(), 4);
}

#[test]
fn a_stop_with_release_names_the_platforms_warning() {
    let f = fixture(false);
    let cookie = signed_in(&f, alice());
    let warning = reference(WARNING.as_bytes());
    let releasing =
        format!(r#"{{"release":true,"warning_shown":"{warning}","record":null,"step_up":null}}"#);
    let reply = post(&f, &ws("/agents/agt_a/stop"), &cookie, KEY, &releasing);
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let (_, event) = written(&f);
    let rest = format!(r#""release":true,"warning_shown":"{warning}",{NO_BID}"#);
    assert_eq!(
        member(&event, &["payload"]),
        &issued("stop", "agent", "agt_a", &rest, "null")
    );
    assert_eq!(
        member(&event, &["artifact_refs"]),
        &json(&format!(r#"["{warning}"]"#))
    );
    let made_up = releasing.replace(&warning, &reference(b"another warning"));
    assert_eq!(
        schema_codes(&post(
            &f,
            &ws("/agents/agt_a/stop"),
            &cookie,
            KEY_2,
            &made_up
        )),
        vec![("/warning_shown".to_owned(), "warning_unknown".to_owned())]
    );
    let plain = r#"{"release":false,"warning_shown":null,"record":null,"step_up":null}"#;
    let reply = post(&f, &ws("/agents/agt_a/stop"), &cookie, KEY_2, plain);
    assert_eq!(reply.status, 202);
    let (_, event) = written(&f);
    let rest = format!(r#""release":false,"warning_shown":null,{NO_BID}"#);
    assert_eq!(
        member(&event, &["payload"]),
        &issued("stop", "agent", "agt_a", &rest, "null")
    );
    assert_eq!(member(&event, &["artifact_refs"]), &json("[]"));
}

#[test]
fn an_owner_exit_is_lenient_and_never_forwards_step_up() {
    let f = fixture(true);
    let cookie = signed_in(&f, alice());
    let full = format!(
        r#"{{"instrument":"{SPY}","bid":"501.2","bid_size":"300","quoted_at":"2026-10-17T10:00:00.000000000Z","floor":"496.19","record":null,"step_up":{EVIDENCE}}}"#
    );
    let reply = post(&f, &ws("/agents/agt_a/exits"), &cookie, KEY, &full);
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    let (_, event) = written(&f);
    let rest =
        r#""release":null,"warning_shown":null,"bid":"501.2","bid_size":"300","floor":"496.19""#;
    assert_eq!(
        member(&event, &["payload"]),
        &issued("owner_exit", "instrument", SPY, rest, "null")
    );
    assert!(f.asked.0.lock().expect("unpoisoned").is_empty());
    let partial = format!(r#"{{"instrument":"{SPY}","bid":"501.2","extra":true}}"#);
    let reply = post(&f, &ws("/agents/agt_a/exits"), &cookie, KEY_2, &partial);
    assert_eq!(
        reply.status,
        202,
        "{:?}",
        String::from_utf8_lossy(&reply.body)
    );
    assert!(
        body(&reply).get("dropped").is_some(),
        "the dropped members are listed"
    );
    let (_, event) = written(&f);
    let rest = format!(r#""release":null,"warning_shown":null,{NO_BID}"#);
    assert_eq!(
        member(&event, &["payload"]),
        &issued("owner_exit", "instrument", SPY, &rest, "null")
    );
}

/// An acknowledgment whose holds the server cannot read, here with no document store to find the
/// agent's account, fails closed: `journal_unavailable`, nothing recorded, and the step-up port
/// never asked (§3.11 item 12, DEC-2144).
#[test]
fn an_acknowledgment_whose_holds_cannot_be_read_commits_nothing() {
    let f = fixture(true);
    let cookie = signed_in(&f, alice());
    let ack = format!(
        r#"{{"record":null,"step_up":{EVIDENCE},"subject":"limit","acknowledged":"{ACKNOWLEDGED}"}}"#
    );
    let reply = post(&f, &ws("/agents/agt_a/acknowledgments"), &cookie, KEY, &ack);
    refused(&reply, 503, ProblemCode::JournalUnavailable);
    assert_eq!(f.journal.rows_of(&ctl()).len(), 3, "nothing recorded");
    assert!(f.asked.0.lock().expect("unpoisoned").is_empty());
}

#[test]
fn a_command_needs_its_row_its_agent_and_a_strict_body() {
    let f = fixture(false);
    let cookie = signed_in(&f, alice());
    let viewer = signed_in(&f, bob());
    let resume = r#"{"record":null,"step_up":null}"#;
    refused(
        &post(&f, &ws("/agents/agt_a/resume"), &viewer, KEY, resume),
        403,
        ProblemCode::Forbidden,
    );
    refused(
        &post(&f, &ws("/agents/agt_x/resume"), &cookie, KEY, resume),
        404,
        ProblemCode::NotFound,
    );
    refused(
        &post(
            &f,
            &ws("/agents/agt_x/exits"),
            &cookie,
            KEY,
            &format!(r#"{{"instrument":"{SPY}"}}"#),
        ),
        404,
        ProblemCode::NotFound,
    );
    let extra = r#"{"record":null,"step_up":null,"requested_by":"owner"}"#;
    assert_eq!(
        schema_codes(&post(&f, &ws("/agents/agt_a/resume"), &cookie, KEY, extra))[0].1,
        "unknown_member"
    );
    let unwarned = r#"{"release":true,"warning_shown":null,"record":null,"step_up":null}"#;
    assert_eq!(
        post(&f, &ws("/agents/agt_a/stop"), &cookie, KEY, unwarned).status,
        422
    );
    assert_eq!(f.journal.rows_of(&ctl()).len(), 3, "nothing recorded");
}

#[test]
fn command_status_reads_the_owning_streams_direct_causation() {
    let f = fixture(false);
    let cookie = signed_in(&f, alice());
    let reply = post(
        &f,
        &ws("/agents/agt_a/resume"),
        &cookie,
        KEY,
        r#"{"record":null,"step_up":null}"#,
    );
    let command = derived("resume", KEY);
    assert_eq!(reply.status, 202);
    let status = get(&f, &ws(&format!("/commands/{command}")), &cookie);
    assert_eq!(
        status.status,
        200,
        "{:?}",
        String::from_utf8_lossy(&status.body)
    );
    let answer = body(&status);
    assert_eq!(member(&answer, &["phase"]), &Value::Str("recorded".into()));
    assert_eq!(member(&answer, &["steps"]), &json("[]"));
    {
        let mut streams = f.journal.streams.lock().expect("unpoisoned");
        let rows = streams.get_mut(&agent_stream()).expect("seeded");
        rows.push(row(
            &agent_stream(),
            2,
            "OwnerCommandRefused",
            r#"{"command":"resume","reason":"step_up_missing"}"#,
            Some(&command),
        ));
        rows.push(row(
            &agent_stream(),
            3,
            "ObservationRecorded",
            "{}",
            Some("01JBH3BTZ8K2M4N6P8R0S2T000"),
        ));
    }
    let answer = body(&get(&f, &ws(&format!("/commands/{command}")), &cookie));
    assert_eq!(member(&answer, &["phase"]), &Value::Str("refused".into()));
    let steps = member(&answer, &["steps"]).as_array().expect("steps");
    assert_eq!(steps.len(), 1, "only direct causation counts");
    assert_eq!(
        member(&steps[0], &["stream_id"]),
        &Value::Str(agent_stream())
    );
    assert_eq!(
        member(&steps[0], &["event_type"]),
        &Value::Str("OwnerCommandRefused".into())
    );
    assert_eq!(
        member(&steps[0], &["reason"]),
        &Value::Str("step_up_missing".into())
    );
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .get_mut(&account())
        .expect("seeded")
        .push(row(
            &account(),
            2,
            "KillSwitchActivated",
            "{}",
            Some(&command),
        ));
    let answer = body(&get(&f, &ws(&format!("/commands/{command}")), &cookie));
    assert_eq!(
        member(&answer, &["steps"]).as_array().expect("steps").len(),
        2,
        "the account stream is read"
    );
    assert_eq!(
        member(&answer, &["phase"]),
        &Value::Str("refused".into()),
        "refused outranks applied"
    );
    refused(
        &get(&f, &ws(&format!("/commands/{ACKNOWLEDGED}")), &cookie),
        404,
        ProblemCode::NotFound,
    );
    f.journal
        .failing
        .lock()
        .expect("unpoisoned")
        .insert(account());
    refused(
        &get(&f, &ws(&format!("/commands/{command}")), &cookie),
        503,
        ProblemCode::JournalUnavailable,
    );
}

#[test]
fn the_phase_is_the_most_final_steps() {
    let none = Value::Null;
    let result = |r: &str| json(&format!(r#"{{"result":"{r}"}}"#));
    let cases = [
        ("OwnerCommandRefused", none.clone(), Phase::Refused),
        ("ApprovalResponded", result("refused"), Phase::Refused),
        ("ApprovalResponded", result("admitted"), Phase::Taken),
        ("MandateVersionApplied", result("rejected"), Phase::Refused),
        ("MandateVersionApplied", result("applied"), Phase::Applied),
        ("AgentModeChanged", none.clone(), Phase::Applied),
        ("AgentModeApplied", none.clone(), Phase::Applied),
        ("KillSwitchActivated", none.clone(), Phase::Applied),
        ("ApprovalRevalidated", result("act"), Phase::Applied),
        ("ApprovalRevalidated", result("skip"), Phase::Ended),
        ("ApprovalTimedOut", none.clone(), Phase::Ended),
        ("ApprovalCanceled", none.clone(), Phase::Ended),
        ("OrderStateChanged", none.clone(), Phase::Taken),
    ];
    for (event_type, payload, expected) in &cases {
        assert_eq!(phase_of(event_type, payload), *expected, "{event_type}");
    }
    assert_eq!(phase(std::iter::empty()), Phase::Recorded);
    let applied = ("AgentModeChanged", &none);
    let ended = ("ApprovalTimedOut", &none);
    let taken = ("OrderStateChanged", &none);
    assert_eq!(phase([taken, ended, applied]), Phase::Applied);
    assert_eq!(phase([taken, ended]), Phase::Ended);
    assert_eq!(phase([taken]), Phase::Taken);
    let names: Vec<&str> = [
        Phase::Recorded,
        Phase::Taken,
        Phase::Ended,
        Phase::Applied,
        Phase::Refused,
    ]
    .iter()
    .map(|p| p.name())
    .collect();
    assert_eq!(names, ["recorded", "taken", "ended", "applied", "refused"]);
    assert_eq!(
        reason(&json(r#"{"reason":"owner_pause"}"#)),
        Some("owner_pause")
    );
    assert_eq!(reason(&json(r#"{"reason":"Owner"}"#)), None);
    assert_eq!(reason(&json(r#"{"reason":"a b"}"#)), None);
    assert_eq!(reason(&json(r#"{"reason":"9x"}"#)), None);
    assert_eq!(reason(&json(r#"{"reason":"x9_"}"#)), Some("x9_"));
    assert_eq!(reason(&json(r#"{"reason":""}"#)), None);
    assert_eq!(reason(&json(r#"{"reason":3}"#)), None);
}

#[test]
fn an_absent_agent_is_the_one_404_before_the_port_is_asked() {
    let f = fixture(true);
    let cookie = signed_in(&f, alice());
    let resume = format!(r#"{{"record":null,"step_up":{EVIDENCE}}}"#);
    refused(
        &post(&f, &ws("/agents/agt_x/resume"), &cookie, KEY, &resume),
        404,
        ProblemCode::NotFound,
    );
    let stop =
        format!(r#"{{"release":false,"warning_shown":null,"record":null,"step_up":{EVIDENCE}}}"#);
    refused(
        &post(&f, &ws("/agents/agt_x/stop"), &cookie, KEY, &stop),
        404,
        ProblemCode::NotFound,
    );
    assert!(
        f.asked.0.lock().expect("unpoisoned").is_empty(),
        "no assertion is spent on a 404"
    );
}

#[test]
fn a_verified_stop_binds_what_it_confirms() {
    let f = fixture(true);
    let cookie = signed_in(&f, alice());
    let warning = reference(WARNING.as_bytes());
    let releasing = format!(
        r#"{{"release":true,"warning_shown":"{warning}","record":null,"step_up":{EVIDENCE}}}"#
    );
    assert_eq!(
        post(&f, &ws("/agents/agt_a/stop"), &cookie, KEY, &releasing).status,
        202
    );
    let plain =
        format!(r#"{{"release":false,"warning_shown":null,"record":null,"step_up":{EVIDENCE}}}"#);
    assert_eq!(
        post(&f, &ws("/agents/agt_a/stop"), &cookie, KEY_2, &plain).status,
        202
    );
    assert_eq!(
        *f.asked.0.lock().expect("unpoisoned"),
        vec![
            (StepUpActionKind::Stop, bound("stop_release", 3)),
            (StepUpActionKind::Stop, bound("stop", 4)),
        ]
    );
    let (_, event) = written(&f);
    assert_eq!(member(&event, &["payload", "step_up"]), &json(EVIDENCE));
}

#[test]
fn command_status_nulls_a_non_code_reason_and_refuses_an_unparsable_row() {
    let f = fixture(false);
    let cookie = signed_in(&f, alice());
    let reply = post(
        &f,
        &ws("/agents/agt_a/resume"),
        &cookie,
        KEY,
        r#"{"record":null,"step_up":null}"#,
    );
    assert_eq!(reply.status, 202);
    let command = derived("resume", KEY);
    {
        let mut streams = f.journal.streams.lock().expect("unpoisoned");
        let rows = streams.get_mut(&agent_stream()).expect("seeded");
        rows.push(row(
            &agent_stream(),
            2,
            "AgentModeChanged",
            r#"{"reason":"Not A Code"}"#,
            Some(&command),
        ));
    }
    let answer = body(&get(&f, &ws(&format!("/commands/{command}")), &cookie));
    assert_eq!(member(&answer, &["phase"]), &Value::Str("applied".into()));
    let steps = member(&answer, &["steps"]).as_array().expect("steps");
    assert_eq!(
        member(&steps[0], &["reason"]),
        &Value::Null,
        "a reason that is not a code is null"
    );
    {
        let mut streams = f.journal.streams.lock().expect("unpoisoned");
        let rows = streams.get_mut(&agent_stream()).expect("seeded");
        let mut broken = row(&agent_stream(), 3, "ObservationRecorded", "{}", None);
        broken.body = b"not json".to_vec();
        rows.push(broken);
    }
    refused(
        &get(&f, &ws(&format!("/commands/{command}")), &cookie),
        503,
        ProblemCode::JournalUnavailable,
    );
}
