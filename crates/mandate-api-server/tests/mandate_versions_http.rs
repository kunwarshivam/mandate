//! An agent's mandate versions (workspace API §4.7, [DEC-2147], E10-27): the deployment, then each
//! confirmation naming the agent, in control-stream order; the version in effect only once the
//! executor's `MandateVersionApplied` applied it; each classification computed from the stored
//! documents by mandate spec §9.2; authorized by the "View agents, positions, decisions" row;
//! another workspace's agent the one 404 (API-9); and an unreadable stream or record refused,
//! never served short. Read-only: nothing is ever appended or stored.
//!
//! The expected bodies are written out here from the rows each test seeds and from mandate spec
//! §9.2's table (a smaller `max_order_usd` reduces, a larger one increases, a new `name` is
//! neutral), never from the server's own folds.
//!
//! [DEC-2147]: ../../../docs/project/decisions/DEC-2147.md

mod common;

use std::collections::{BTreeMap, BTreeSet};
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
use mandate_journal::{ArtifactSource, StoredEvent};
use mandate_time::UtcNanos;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const OTHER_WORKSPACE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";
const AGENT: &str = "agt_a";
const RECORD: &str = "sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const DOCUMENT: &str = include_str!("fixtures/mandate.json");
const SCHEMA: &str =
    include_str!("../../../schemas/workspace-api/read-models/agent-mandate-versions.schema.json");
const ENVELOPE_MEMBERS: [&str; 4] = ["api_version", "as_of", "build", "served_at"];

type Records = Arc<Mutex<Result<Vec<MembershipRecord>, LookupFailed>>>;

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    records: Records,
    sessions: Arc<Sessions>,
    clock: Arc<common::ManualClock>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

/// A server over `journal`'s streams and a read store holding `documents`.
fn fixture(documents: &[&Document]) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
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
    let store: BTreeMap<Digest, Vec<u8>> = documents
        .iter()
        .map(|d| (Digest::of(&d.bytes), d.bytes.clone()))
        .collect();
    let artifacts: Arc<dyn ArtifactSource + Send + Sync> = Arc::new(store);
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
        artifacts: Some(artifacts),
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
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

/// `member` activated with `roles`, effective since before T0.
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

/// `alice` signed in as an operator.
fn operator(f: &Fixture) -> String {
    members(f, Ok(vec![activated(1, alice(), &[Role::Operator])]));
    signed_in(f, alice())
}

fn request(f: &Fixture, method: &str, path: &str, cookie: Option<&str>) -> Reply {
    let header = cookie.map(|c| format!("{COOKIE_NAME}={c}"));
    let mut headers = vec![("Origin", APP)];
    if let Some(header) = header.as_deref() {
        headers.push(("Cookie", header));
    }
    send(f.addr, method, path, &headers, b"")
}

fn get(f: &Fixture, path: &str, cookie: &str) -> Reply {
    request(f, "GET", path, Some(cookie))
}

fn path_in(workspace: &str, agent: &str) -> String {
    format!("/v1/workspaces/{workspace}/agents/{agent}/mandate-versions")
}

fn path(agent: &str) -> String {
    path_in(WORKSPACE, agent)
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn acct() -> String {
    format!("acct:{WORKSPACE}:acct_1")
}

/// A mandate document: the fixture with `edits` applied to its text, canonical, and its version.
struct Document {
    bytes: Vec<u8>,
    version: String,
}

fn document(edits: &[(&str, &str)]) -> Document {
    let mut text = DOCUMENT.to_owned();
    for (from, to) in edits {
        assert_eq!(
            text.matches(from).count(),
            1,
            "{from} is in the fixture once"
        );
        text = text.replace(from, to);
    }
    let bytes = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    let version = format!("sha256:{}", Digest::of(&bytes).to_hex());
    Document { bytes, version }
}

const ORDER_300: &str = r#""max_order_usd": "300""#;

/// The fixture as deployed.
fn base() -> Document {
    document(&[])
}

/// A smaller order cap than the fixture's: reducing (§9.2's maximums row).
fn smaller() -> Document {
    document(&[(ORDER_300, r#""max_order_usd": "200""#)])
}

/// A larger order cap than [`smaller`]'s and the fixture's: increasing from either.
fn larger() -> Document {
    document(&[(ORDER_300, r#""max_order_usd": "400""#)])
}

/// [`smaller`] renamed: neutral from it (§9.2's `name` row).
fn renamed() -> Document {
    document(&[
        (ORDER_300, r#""max_order_usd": "200""#),
        (r#""name": "tracer-aapl""#, r#""name": "tracer-aapl-two""#),
    ])
}

/// One stream's rows, built in order, `seq` from 1. Event ids carry the stream's own letter, so
/// no id on one stream is ever an id on another.
struct Stream {
    id: String,
    letter: char,
    day: u8,
    rows: Vec<StoredEvent>,
}

impl Stream {
    fn new(id: String, letter: char, day: u8) -> Self {
        Self {
            id,
            letter,
            day,
            rows: Vec::new(),
        }
    }

    fn control() -> Self {
        let mut stream = Self::new(ctl(), 'C', 17);
        stream.push("StreamOpened", 1, "{}", None);
        stream
    }

    fn account() -> Self {
        let mut stream = Self::new(acct(), 'A', 18);
        stream.push("StreamOpened", 1, "{}", None);
        stream
    }

    fn next_seq(&self) -> u64 {
        u64::try_from(self.rows.len()).expect("a count") + 1
    }

    /// The id and `recorded_at` of the row at `seq`.
    fn id_at(&self, seq: u64) -> String {
        format!("01JBH3BTZ8K2M4N6P8R0{}0{seq:05}", self.letter)
    }

    fn time_at(&self, seq: u64) -> String {
        format!(
            "2026-10-{:02}T10:{:02}:{:02}.000000000Z",
            self.day,
            seq / 60,
            seq % 60
        )
    }

    /// Appends a row and returns its event id.
    fn push(
        &mut self,
        event_type: &str,
        schema_version: u64,
        payload: &str,
        causation: Option<&str>,
    ) -> String {
        let seq = self.next_seq();
        let event_id = self.id_at(seq);
        let caused_by =
            causation.map_or_else(String::new, |id| format!(r#""causation_id":"{id}","#));
        let text = format!(
            r#"{{{caused_by}"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"schema_version":{schema_version},"stream_id":"{}"}}"#,
            self.id
        );
        let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
        self.rows.push(StoredEvent {
            stream_id: self.id.clone(),
            seq,
            event_id: event_id.clone(),
            event_type: event_type.to_owned(),
            schema_version,
            environment: "paper".to_owned(),
            recorded_at: self.time_at(seq),
            prev_hash: Digest::ZERO,
            hash: Digest::of(&body),
            body,
        });
        event_id
    }

    /// The `recorded_at` of the row `event_id` names.
    fn time_of(&self, event_id: &str) -> String {
        self.rows
            .iter()
            .find(|row| row.event_id == event_id)
            .map(|row| row.recorded_at.clone())
            .expect("a row with that id")
    }

    fn created(&mut self, version: &str) -> String {
        self.push(
            "MandateVersionCreated",
            1,
            &format!(
                r#"{{"mandate_version":"{version}","provenance":[],"record_ref":"{RECORD}"}}"#
            ),
            None,
        )
    }

    /// A new mandate's confirmation, version 1: no agent link.
    fn confirmed_new(&mut self, version: &str) -> String {
        self.push(
            "MandateConfirmed",
            1,
            &format!(
                r#"{{"confirmed_paths":[],"mandate_version":"{version}","record_ref":"{RECORD}"}}"#
            ),
            None,
        )
    }

    /// A confirmation at `schema_version` 2 or 3 naming `agent` (or none) and `base`.
    fn confirmed(
        &mut self,
        schema_version: u64,
        version: &str,
        agent: Option<&str>,
        base: Option<&str>,
    ) -> String {
        let quoted = |v: Option<&str>| v.map_or("null".to_owned(), |v| format!(r#""{v}""#));
        let step_up = if schema_version == 3 {
            r#","step_up":{"assertion_id":"as_1","authenticated_at":"2026-10-17T09:59:00Z","method":"passkey"}"#
        } else {
            ""
        };
        self.push(
            "MandateConfirmed",
            schema_version,
            &format!(
                r#"{{"agent_id":{},"base_version":{},"confirmed_paths":[],"mandate_version":"{version}","record_ref":"{RECORD}"{step_up}}}"#,
                quoted(agent),
                quoted(base)
            ),
            None,
        )
    }

    fn deployed(&mut self, agent: &str, version: &str) -> String {
        self.push(
            "AgentDeployed",
            1,
            &format!(
                r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{RECORD}"}}"#
            ),
            None,
        )
    }

    /// The document's connection, established at version 2 on `acct_1`.
    fn established(&mut self) -> String {
        self.push(
            "ConnectionEstablished",
            2,
            r#"{"account_ref":"acct_1","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper","scopes":["trade"]}"#,
            None,
        )
    }

    fn stopped(&mut self, agent: &str) -> String {
        self.push(
            "AgentStopped",
            1,
            &format!(
                r#"{{"agent_id":"{agent}","connection_id":"conn_alpaca_paper_01","reason":"owner_stop"}}"#
            ),
            None,
        )
    }

    /// The executor's application of `new` over `old`, caused by `confirmation`; `reason` `None`
    /// is `applied`. Its `classification` is `told`, whatever the documents say.
    fn applied(
        &mut self,
        agent: &str,
        (old, new): (&str, &str),
        confirmation: &str,
        reason: Option<&str>,
        told: &str,
    ) -> String {
        let (result, reason) = reason.map_or(("applied", "null".to_owned()), |r| {
            ("rejected", format!(r#""{r}""#))
        });
        self.push(
            "MandateVersionApplied",
            1,
            &format!(
                r#"{{"agent_id":"{agent}","allocation_change":null,"classification":"{told}","max_loss_from_allocation":null,"new_version":"{new}","old_version":"{old}","reason":{reason},"result":"{result}","risk_clock":"2026-10-18T10:00:00Z","step_up":null}}"#
            ),
            Some(confirmation),
        )
    }
}

fn install(f: &Fixture, streams: &[&Stream]) {
    let mut held = f.journal.streams.lock().expect("unpoisoned");
    for stream in streams {
        held.insert(stream.id.clone(), stream.rows.clone());
    }
}

fn json(reply: &Reply) -> Value {
    parse(&reply.body).expect("a JSON body")
}

fn value(text: &str) -> Value {
    parse(text.as_bytes()).expect("JSON")
}

fn quoted(v: Option<&str>) -> String {
    v.map_or("null".to_owned(), |v| format!(r#""{v}""#))
}

/// One served entry, as `read-models/agent-mandate-versions` states it.
struct Entry<'a> {
    version: &'a str,
    base: Option<&'a str>,
    classification: Option<&'a str>,
    created_at: Option<&'a str>,
    confirmed_at: Option<&'a str>,
    confirmation_id: Option<&'a str>,
    deployed_at: Option<&'a str>,
    application: Option<String>,
}

impl Entry<'_> {
    fn value(&self) -> Value {
        value(&format!(
            r#"{{"application":{},"base_version":{},"classification":{},"confirmation_id":{},"confirmed_at":{},"created_at":{},"deployed_at":{},"mandate_version":"{}"}}"#,
            self.application.as_deref().unwrap_or("null"),
            quoted(self.base),
            quoted(self.classification),
            quoted(self.confirmation_id),
            quoted(self.confirmed_at),
            quoted(self.created_at),
            quoted(self.deployed_at),
            self.version
        ))
    }
}

fn application(result: &str, reason: Option<&str>, recorded_at: &str) -> Option<String> {
    Some(format!(
        r#"{{"reason":{},"recorded_at":"{recorded_at}","result":"{result}"}}"#,
        quoted(reason)
    ))
}

/// A 200's body, after checking its status and that it holds exactly the members the schema
/// requires at the top and in each entry, and the envelope's.
fn served(reply: &Reply) -> Value {
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(reply.header("content-type"), Some("application/json"));
    let body = json(reply);
    let schema: serde_json::Value = serde_json::from_str(SCHEMA).expect("the schema");
    let names = |v: &Value| -> BTreeSet<String> {
        v.as_object()
            .expect("an object")
            .keys()
            .map(|k| k.as_str().to_owned())
            .collect()
    };
    let required = |v: &serde_json::Value| -> BTreeSet<String> {
        v.as_array()
            .expect("a required list")
            .iter()
            .map(|m| m.as_str().expect("a name").to_owned())
            .collect()
    };
    let mut top = required(&schema["required"]);
    top.extend(ENVELOPE_MEMBERS.iter().map(|m| (*m).to_owned()));
    assert_eq!(names(&body), top, "the top-level members");
    let entry = required(&schema["$defs"]["Deployed"]["required"]);
    assert_eq!(
        entry,
        required(&schema["$defs"]["Confirmed"]["required"]),
        "both entry shapes hold the same members"
    );
    let Some(Value::Array(versions)) = body.get("versions") else {
        panic!("versions is a list");
    };
    for served in versions {
        assert_eq!(names(served), entry, "an entry's members");
    }
    body
}

fn versions(body: &Value) -> &Vec<Value> {
    match body.get("versions") {
        Some(Value::Array(list)) => list,
        _ => panic!("versions is a list"),
    }
}

fn member<'v>(body: &'v Value, name: &str) -> &'v Value {
    body.get(name).unwrap_or_else(|| panic!("{name}"))
}

fn code(reply: &Reply) -> String {
    json(reply)
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

/// The stream ids `as_of` names, in order.
fn as_of(body: &Value) -> Vec<String> {
    match body.get("as_of") {
        Some(Value::Array(marks)) => marks
            .iter()
            .map(|m| {
                m.get("stream_id")
                    .and_then(Value::as_str)
                    .expect("a stream")
                    .to_owned()
            })
            .collect(),
        _ => panic!("as_of"),
    }
}

/// `agt_a` deployed with [`base`], its connection on `acct_1`, and then three confirmations
/// for it: [`smaller`] (reducing), [`larger`] (increasing), and [`renamed`] (neutral, over
/// [`smaller`] by the stale base the owner named). Around them, rows that are not the agent's
/// history: another agent's deployment and confirmation, a new mandate's confirmation after the
/// deployment, and a confirmation naming `agt_a` recorded before it was deployed.
struct Seeded {
    control: Stream,
    account: Stream,
    deployed: String,
    deployment_confirmed: String,
    confirmations: [String; 3],
    created: [String; 4],
}

fn seeded() -> Seeded {
    let (b, s, l, r) = (base(), smaller(), larger(), renamed());
    let mut control = Stream::control();
    control.established();
    let created_base = control.created(&b.version);
    control.confirmed_new(&b.version);
    let deployment_confirmed = control.confirmed(2, &b.version, None, None);
    control.confirmed(2, &s.version, Some(AGENT), Some(&b.version));
    let deployed = control.deployed(AGENT, &b.version);
    control.deployed("agt_b", &l.version);
    let created_smaller = control.created(&s.version);
    let first = control.confirmed(2, &s.version, Some(AGENT), Some(&b.version));
    control.confirmed(2, &l.version, Some("agt_b"), Some(&b.version));
    control.confirmed_new(&r.version);
    let created_larger = control.created(&l.version);
    control.created(&s.version);
    let second = control.confirmed(3, &l.version, Some(AGENT), Some(&s.version));
    let created_renamed = control.created(&r.version);
    let third = control.confirmed(2, &r.version, Some(AGENT), Some(&s.version));
    let mut account = Stream::account();
    account.push(
        "AccountSnapshotRecorded",
        1,
        r#"{"buying_power":"1000","cash":"1000","equity":"1000","risk_clock":"2026-10-18T10:00:00Z"}"#,
        None,
    );
    Seeded {
        control,
        account,
        deployed,
        deployment_confirmed,
        confirmations: [first, second, third],
        created: [
            created_base,
            created_smaller,
            created_larger,
            created_renamed,
        ],
    }
}

fn all_documents() -> [Document; 4] {
    [base(), smaller(), larger(), renamed()]
}

#[test]
#[ignore = "pending E10-27"]
fn the_history_is_the_deployment_then_each_confirmation_for_the_agent_in_order() {
    let world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    let c = &world.control;
    let [first, second, third] = &world.confirmations;
    let expected = vec![
        Entry {
            version: &b.version,
            base: None,
            classification: Some("first_version"),
            created_at: Some(&c.time_of(&world.created[0])),
            confirmed_at: Some(&c.time_of(&world.deployment_confirmed)),
            confirmation_id: Some(&world.deployment_confirmed),
            deployed_at: Some(&c.time_of(&world.deployed)),
            application: None,
        }
        .value(),
        Entry {
            version: &s.version,
            base: Some(&b.version),
            classification: Some("risk_reducing"),
            created_at: Some(&c.time_of(&world.created[1])),
            confirmed_at: Some(&c.time_of(first)),
            confirmation_id: Some(first),
            deployed_at: None,
            application: None,
        }
        .value(),
        Entry {
            version: &l.version,
            base: Some(&s.version),
            classification: Some("risk_increasing"),
            created_at: Some(&c.time_of(&world.created[2])),
            confirmed_at: Some(&c.time_of(second)),
            confirmation_id: Some(second),
            deployed_at: None,
            application: None,
        }
        .value(),
        Entry {
            version: &r.version,
            base: Some(&s.version),
            classification: Some("neutral"),
            created_at: Some(&c.time_of(&world.created[3])),
            confirmed_at: Some(&c.time_of(third)),
            confirmation_id: Some(third),
            deployed_at: None,
            application: None,
        }
        .value(),
    ];
    assert_eq!(
        versions(&body),
        &expected,
        "oldest first: the deployment, then agt_a's three confirmations recorded after it, with \
         the first create of each version and the last agentless confirmation before deploying"
    );
    assert_eq!(member(&body, "agent_id"), &Value::Str(AGENT.to_owned()));
    assert_eq!(
        member(&body, "account_ref"),
        &Value::Str("acct_1".to_owned())
    );
    assert_eq!(member(&body, "stopped_at"), &Value::Null);
    assert_eq!(as_of(&body), vec![ctl(), acct()]);
}

#[test]
#[ignore = "pending E10-27"]
fn a_confirmed_version_is_in_effect_only_once_the_executor_applied_it() {
    let mut world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let in_effect = |f: &Fixture| -> Value {
        member(&served(&get(f, &path(AGENT), &cookie)), "in_effect").clone()
    };
    assert_eq!(
        in_effect(&f),
        Value::Str(b.version.clone()),
        "three newer versions are confirmed and none applied: the deployed one is in effect"
    );
    let [first, second, third] = world.confirmations.clone();
    let applied = world.account.applied(
        AGENT,
        (&b.version, &s.version),
        &first,
        None,
        "risk_reducing",
    );
    install(&f, &[&world.account]);
    assert_eq!(in_effect(&f), Value::Str(s.version.clone()));
    let refused = world.account.applied(
        AGENT,
        (&s.version, &l.version),
        &second,
        Some("increase_blocked_while_latched"),
        "risk_increasing",
    );
    world
        .account
        .applied("agt_b", (&b.version, &r.version), &third, None, "neutral");
    install(&f, &[&world.account]);
    let body = served(&get(&f, &path(AGENT), &cookie));
    assert_eq!(
        member(&body, "in_effect"),
        &Value::Str(s.version.clone()),
        "a rejected application leaves the version in force, and another agent's applies nothing \
         to this one"
    );
    let a = &world.account;
    let applications: Vec<Value> = versions(&body)
        .iter()
        .map(|entry| member(entry, "application").clone())
        .collect();
    assert_eq!(
        applications,
        vec![
            Value::Null,
            value(&application("applied", None, &a.time_of(&applied)).expect("one")),
            value(
                &application(
                    "rejected",
                    Some("increase_blocked_while_latched"),
                    &a.time_of(&refused)
                )
                .expect("one")
            ),
            Value::Null,
        ],
        "each application is matched to its confirmation by causation_id, never by version or by \
         agent alone"
    );
    world
        .account
        .applied(AGENT, (&s.version, &r.version), &third, None, "neutral");
    install(&f, &[&world.account]);
    assert_eq!(in_effect(&f), Value::Str(r.version.clone()));
}

#[test]
#[ignore = "pending E10-27"]
fn a_version_confirmed_twice_is_listed_twice_each_with_its_own_application() {
    let (b, s) = (base(), smaller());
    let f = fixture(&[&b, &s]);
    let mut control = Stream::control();
    control.established();
    control.created(&b.version);
    let deploying = control.confirmed_new(&b.version);
    control.deployed(AGENT, &b.version);
    control.created(&s.version);
    let once = control.confirmed(2, &s.version, Some(AGENT), Some(&b.version));
    let again = control.confirmed(2, &s.version, Some(AGENT), Some(&b.version));
    let mut account = Stream::account();
    let refused = account.applied(
        AGENT,
        (&b.version, &s.version),
        &once,
        Some("would_trigger_limit"),
        "risk_reducing",
    );
    let applied = account.applied(
        AGENT,
        (&b.version, &s.version),
        &again,
        None,
        "risk_reducing",
    );
    install(&f, &[&control, &account]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    let listed: Vec<(Value, Value, Value)> = versions(&body)
        .iter()
        .map(|e| {
            (
                member(e, "mandate_version").clone(),
                member(e, "confirmation_id").clone(),
                member(e, "application").clone(),
            )
        })
        .collect();
    let named = |v: &str| Value::Str(v.to_owned());
    assert_eq!(
        listed,
        vec![
            (named(&b.version), named(&deploying), Value::Null),
            (
                named(&s.version),
                named(&once),
                value(
                    &application(
                        "rejected",
                        Some("would_trigger_limit"),
                        &account.time_of(&refused)
                    )
                    .expect("one")
                ),
            ),
            (
                named(&s.version),
                named(&again),
                value(&application("applied", None, &account.time_of(&applied)).expect("one")),
            ),
        ]
    );
    assert_eq!(member(&body, "in_effect"), &named(&s.version));
}

#[test]
#[ignore = "pending E10-27"]
fn each_classification_is_the_servers_from_the_stored_documents() {
    let world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    let mut account = world.account;
    let [first, second, _] = world.confirmations.clone();
    account.applied(
        AGENT,
        (&b.version, &s.version),
        &first,
        None,
        "risk_reducing",
    );
    account.applied(
        AGENT,
        (&s.version, &l.version),
        &second,
        Some("waiting_period"),
        "risk_increasing",
    );
    install(&f, &[&world.control, &account]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    let classifications: Vec<Value> = versions(&body)
        .iter()
        .map(|e| member(e, "classification").clone())
        .collect();
    let named = |c: &str| Value::Str(c.to_owned());
    assert_eq!(
        classifications,
        vec![
            named("first_version"),
            named("risk_reducing"),
            named("risk_increasing"),
            named("neutral"),
        ],
        "each from base_version to mandate_version by §9.2"
    );
}

#[test]
#[ignore = "pending E10-27"]
fn a_classification_the_store_cannot_compute_is_null_never_a_guess() {
    let world = seeded();
    let [b, _, l, _] = all_documents();
    let f = fixture(&[&b, &l]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    let classifications: Vec<Value> = versions(&body)
        .iter()
        .map(|e| member(e, "classification").clone())
        .collect();
    assert_eq!(
        classifications,
        vec![
            Value::Str("first_version".to_owned()),
            Value::Null,
            Value::Null,
            Value::Null,
        ],
        "the store lacks the reduced document every later entry names, as base or as version"
    );
    assert_eq!(member(&body, "in_effect"), &Value::Str(b.version.clone()));
}

#[test]
#[ignore = "pending E10-27"]
fn an_agent_whose_account_stream_is_not_found_has_nothing_in_effect() {
    let b = base();
    let f = fixture(&[&b]);
    let mut control = Stream::control();
    control.created(&b.version);
    control.confirmed_new(&b.version);
    control.deployed(AGENT, &b.version);
    install(&f, &[&control]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    assert_eq!(member(&body, "account_ref"), &Value::Null);
    assert_eq!(
        member(&body, "in_effect"),
        &Value::Null,
        "no bound account: an application could not be seen, so nothing is said to be in effect"
    );
    assert_eq!(versions(&body).len(), 1);
    assert_eq!(as_of(&body), vec![ctl()]);
}

#[test]
#[ignore = "pending E10-27"]
fn a_stopped_agent_keeps_its_history_and_has_nothing_in_effect() {
    let mut world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    let stop = world.control.stopped(AGENT);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    assert_eq!(versions(&body).len(), 4);
    assert_eq!(member(&body, "in_effect"), &Value::Null);
    assert_eq!(
        member(&body, "stopped_at"),
        &Value::Str(world.control.time_of(&stop))
    );
}

#[test]
#[ignore = "pending E10-27"]
fn an_agent_never_deployed_is_404_even_when_a_confirmation_names_it() {
    let world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    let mut control = world.control;
    control.confirmed(2, &s.version, Some("agt_ghost"), Some(&b.version));
    install(&f, &[&control, &world.account]);
    let cookie = operator(&f);
    let unknown = get(&f, &path("agt_unknown"), &cookie);
    assert_eq!(unknown.status, 404);
    assert_eq!(unknown.header("content-type"), Some(PROBLEM_JSON));
    assert_eq!(code(&unknown), "not_found");
    let ghost = get(&f, &path("agt_ghost"), &cookie);
    assert_eq!(ghost.status, 404);
    assert_eq!(ghost.body, unknown.body);
}

#[test]
#[ignore = "pending E10-27"]
fn an_unknown_or_malformed_agent_is_the_one_404() {
    let world = seeded();
    let f = fixture(&[]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let unknown = get(&f, &path("agt_zz"), &cookie);
    assert_eq!(unknown.status, 404);
    for agent in ["a".repeat(65), "agt%2Fa".to_owned(), "%FF".to_owned()] {
        let reply = get(&f, &path(&agent), &cookie);
        assert_eq!(reply.status, 404, "{agent}");
        assert_eq!(reply.body, unknown.body, "{agent}");
    }
}

#[test]
#[ignore = "pending E10-27"]
fn another_workspaces_agent_is_the_same_404_and_leaks_nothing() {
    let world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let absent = get(&f, &path("agt_zz"), &cookie);
    assert_eq!(absent.status, 404);
    let foreign_stream = format!("ctl:{OTHER_WORKSPACE}");
    let mut other = Stream::new(foreign_stream, 'F', 17);
    other.deployed(AGENT, &b.version);
    install(&f, &[&other]);
    for path in [
        path_in(OTHER_WORKSPACE, AGENT),
        path_in(OTHER_WORKSPACE, "agt_zz"),
        path_in("not-a-ulid", AGENT),
    ] {
        let reply = get(&f, &path, &cookie);
        assert_eq!(reply.status, 404, "{path}");
        assert_eq!(
            reply.body, absent.body,
            "{path}: a foreign id answers as an absent one"
        );
        let shown = String::from_utf8_lossy(&reply.body).into_owned();
        for secret in [&b.version, &s.version, AGENT, "acct_1"] {
            assert!(!shown.contains(secret), "{path} names {secret}: {shown}");
        }
    }
}

#[test]
#[ignore = "pending E10-27"]
fn a_member_of_another_workspace_only_is_404_and_a_non_member_too() {
    let world = seeded();
    let f = fixture(&[]);
    install(&f, &[&world.control, &world.account]);
    members(&f, Ok(vec![activated(1, bob(), &[Role::Operator])]));
    let alice_cookie = signed_in(&f, alice());
    let absent = {
        let bob_cookie = signed_in(&f, bob());
        get(&f, &path("agt_zz"), &bob_cookie)
    };
    let reply = get(&f, &path(AGENT), &alice_cookie);
    assert_eq!(
        reply.status, 404,
        "alice holds no membership of the hosted workspace, whatever else she belongs to"
    );
    assert_eq!(reply.body, absent.body);
}

#[test]
#[ignore = "pending E10-27"]
fn the_view_row_admits_every_role_it_lists_and_refuses_the_rest() {
    let world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    for role in [
        Role::WorkspaceAdmin,
        Role::Operator,
        Role::Approver,
        Role::Viewer,
        Role::Auditor,
    ] {
        members(&f, Ok(vec![activated(1, alice(), &[role])]));
        let cookie = signed_in(&f, alice());
        let reply = get(&f, &path(AGENT), &cookie);
        assert_eq!(reply.status, 200, "{role:?}");
    }
    for refused in [vec![], vec![Role::BillingAdmin]] {
        members(&f, Ok(vec![activated(1, alice(), &refused)]));
        let cookie = signed_in(&f, alice());
        let reply = get(&f, &path(AGENT), &cookie);
        assert_eq!(reply.status, 403, "{refused:?}");
        assert_eq!(code(&reply), "forbidden", "{refused:?}");
    }
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
    let cookie = signed_in(&f, alice());
    assert_eq!(get(&f, &path(AGENT), &cookie).status, 404);
}

#[test]
#[ignore = "pending E10-27"]
fn without_a_live_session_it_is_the_one_401_and_a_reduction_only_session_is_refused() {
    let world = seeded();
    let f = fixture(&[]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let one = send(f.addr, "GET", "/nowhere", &[], b"").body;
    for presented in [None, Some("forged")] {
        let reply = request(&f, "GET", &path(AGENT), presented);
        assert_eq!(reply.status, 401, "{presented:?}");
        assert_eq!(reply.body, one);
    }
    assert_eq!(
        f.sessions.refresh(
            &cookie,
            mandate_api_server::session::Answer::Unreachable,
            at(T0 + 1)
        ),
        Ok(None)
    );
    let reply = get(&f, &path(AGENT), &cookie);
    assert_eq!(reply.status, 403);
    assert_eq!(code(&reply), "reduction_only");
}

#[test]
#[ignore = "pending E10-27"]
fn an_unreadable_control_stream_fails_the_membership_read_first() {
    let world = seeded();
    let f = fixture(&[]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    f.journal.failing.lock().expect("unpoisoned").insert(ctl());
    let reply = get(&f, &path(AGENT), &cookie);
    assert_eq!(reply.status, 503);
    assert_eq!(code(&reply), "membership_unavailable");
}

#[test]
#[ignore = "pending E10-27"]
fn an_unreadable_account_stream_is_journal_unavailable_never_the_deployed_version() {
    let world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    f.journal.failing.lock().expect("unpoisoned").insert(acct());
    let reply = get(&f, &path(AGENT), &cookie);
    assert_eq!(reply.status, 503);
    assert_eq!(code(&reply), "journal_unavailable");
}

#[test]
#[ignore = "pending E10-27"]
fn a_folded_record_that_does_not_read_is_journal_unavailable_never_a_shorter_list() {
    let [b, s, l, r] = all_documents();
    let full = |result: &str, reason: &str| {
        format!(
            r#"{{"agent_id":"agt_b","allocation_change":null,"classification":"neutral","max_loss_from_allocation":null,"new_version":"{v}","old_version":"{v}","reason":{reason},"result":"{result}","risk_clock":"2026-10-18T10:00:00Z","step_up":null}}"#,
            v = RECORD
        )
    };
    let applied_with_a_reason = full("applied", r#""not_loosening""#);
    let rejected_without_one = full("rejected", "null");
    let broken: [(&str, &str, &str); 10] = [
        ("account", "MandateVersionApplied", &applied_with_a_reason),
        ("account", "MandateVersionApplied", &rejected_without_one),
        (
            "control",
            "MandateConfirmed",
            r#"{"agent_id":"agt_a","base_version":7}"#,
        ),
        ("control", "MandateConfirmed", "[]"),
        ("account", "MandateVersionApplied", r#""applied""#),
        ("control", "MandateVersionCreated", r#"{"provenance":[]}"#),
        ("control", "AgentDeployed", r#"{"agent_id":"agt_c"}"#),
        ("control", "AgentStopped", r#"{"reason":"owner_stop"}"#),
        (
            "account",
            "MandateVersionApplied",
            r#"{"agent_id":"agt_a","result":"applied"}"#,
        ),
        (
            "account",
            "MandateVersionApplied",
            r#"{"agent_id":"agt_b","new_version":null,"old_version":null,"reason":null,"result":"applied"}"#,
        ),
    ];
    for (on, event_type, payload) in broken {
        let mut world = seeded();
        let stream = if on == "control" {
            &mut world.control
        } else {
            &mut world.account
        };
        stream.push(event_type, 1, payload, None);
        let f = fixture(&[&b, &s, &l, &r]);
        install(&f, &[&world.control, &world.account]);
        let cookie = operator(&f);
        let reply = get(&f, &path(AGENT), &cookie);
        assert_eq!(reply.status, 503, "{event_type} {payload}");
        assert_eq!(
            code(&reply),
            "journal_unavailable",
            "{event_type} {payload}"
        );
    }
}

#[test]
#[ignore = "pending E10-27"]
fn a_query_is_invalid() {
    let world = seeded();
    let f = fixture(&[]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    for query in ["?limit=1", "?agent=agt_b", "?"] {
        let reply = get(&f, &format!("{}{query}", path(AGENT)), &cookie);
        assert_eq!(reply.status, 422, "{query}");
        assert_eq!(code(&reply), "invalid", "{query}");
    }
}

#[test]
#[ignore = "pending E10-27"]
fn the_read_writes_nothing_and_takes_no_other_method() {
    let world = seeded();
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let before = f.journal.streams.lock().expect("unpoisoned").clone();
    served(&get(&f, &path(AGENT), &cookie));
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let reply = request(&f, method, &path(AGENT), Some(&cookie));
        assert_eq!(reply.status, 401, "{method}: no route but the read");
    }
    assert!(
        f.journal.appends.lock().expect("unpoisoned").is_empty(),
        "nothing appended"
    );
    assert_eq!(
        *f.journal.streams.lock().expect("unpoisoned"),
        before,
        "every stream as it was"
    );
}

/// One contradicting application: its case, the old and new versions it names, its reason, and
/// the classification it tells.
type Contradiction<'a> = (&'a str, (&'a str, &'a str), Option<&'a str>, &'a str);

/// The seeded history's own answers: the reduction applied, the increase refused while latched.
fn answered(world: &mut Seeded) {
    let [b, s, l, _] = all_documents();
    let [first, second, _] = world.confirmations.clone();
    world.account.applied(
        AGENT,
        (&b.version, &s.version),
        &first,
        None,
        "risk_reducing",
    );
    world.account.applied(
        AGENT,
        (&s.version, &l.version),
        &second,
        Some("increase_blocked_while_latched"),
        "risk_increasing",
    );
}

#[test]
#[ignore = "pending E10-27"]
fn an_application_that_contradicts_its_entry_is_journal_unavailable() {
    let [b, s, l, r] = all_documents();
    let contradictions: [Contradiction<'_>; 4] = [
        (
            "a classification the documents do not give",
            (&b.version, &s.version),
            None,
            "neutral",
        ),
        (
            "another version than the one confirmed, with the entry's own classification",
            (&b.version, &l.version),
            None,
            "risk_reducing",
        ),
        (
            "another base than the one confirmed over",
            (&r.version, &s.version),
            None,
            "risk_reducing",
        ),
        (
            "a reduction refused for a reason only an increase meets",
            (&b.version, &s.version),
            Some("waiting_period"),
            "risk_increasing",
        ),
    ];
    for (case, versions, reason, told) in contradictions {
        let mut world = seeded();
        let [first, _, _] = world.confirmations.clone();
        world.account.applied(AGENT, versions, &first, reason, told);
        let f = fixture(&[&b, &s, &l, &r]);
        install(&f, &[&world.control, &world.account]);
        let cookie = operator(&f);
        let reply = get(&f, &path(AGENT), &cookie);
        assert_eq!(reply.status, 503, "{case}");
        assert_eq!(code(&reply), "journal_unavailable", "{case}");
    }
}

#[test]
#[ignore = "pending E10-27"]
fn an_application_no_listed_confirmation_caused_puts_nothing_in_effect() {
    let mut world = seeded();
    let [b, s, l, r] = all_documents();
    let elsewhere = world
        .control
        .confirmed(2, &l.version, Some("agt_b"), Some(&b.version));
    world.account.applied(
        AGENT,
        (&b.version, &l.version),
        &elsewhere,
        None,
        "risk_increasing",
    );
    world.account.push(
        "MandateVersionApplied",
        1,
        &format!(
            r#"{{"agent_id":"{AGENT}","allocation_change":null,"classification":"risk_reducing","max_loss_from_allocation":null,"new_version":"{}","old_version":"{}","reason":null,"result":"applied","risk_clock":"2026-10-18T10:00:00Z","step_up":null}}"#,
            s.version, b.version
        ),
        None,
    );
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    assert_eq!(
        member(&body, "in_effect"),
        &Value::Str(b.version.clone()),
        "an application caused by no confirmation of this agent's history, or by none, moves \
         nothing"
    );
}

/// The served versions' `mandate_version`s.
fn listed(body: &Value) -> Vec<Value> {
    versions(body)
        .iter()
        .map(|e| member(e, "mandate_version").clone())
        .collect()
}

#[test]
#[ignore = "pending E10-27"]
fn in_effect_is_always_a_listed_version_or_null() {
    let [b, s, l, r] = all_documents();
    let worlds: Vec<(&str, Seeded)> = vec![
        ("nothing applied", seeded()),
        ("answered", {
            let mut world = seeded();
            answered(&mut world);
            world
        }),
        ("the last confirmation applied", {
            let mut world = seeded();
            answered(&mut world);
            let [_, _, third] = world.confirmations.clone();
            world
                .account
                .applied(AGENT, (&s.version, &r.version), &third, None, "neutral");
            world
        }),
        ("stopped", {
            let mut world = seeded();
            answered(&mut world);
            world.control.stopped(AGENT);
            world
        }),
    ];
    for (case, world) in worlds {
        let f = fixture(&[&b, &s, &l, &r]);
        install(&f, &[&world.control, &world.account]);
        let cookie = operator(&f);
        let body = served(&get(&f, &path(AGENT), &cookie));
        let in_effect = member(&body, "in_effect");
        assert!(
            *in_effect == Value::Null || listed(&body).contains(in_effect),
            "{case}: {in_effect:?} is not listed"
        );
    }
}

#[test]
#[ignore = "pending E10-27"]
fn every_entry_keeps_the_cross_field_rules() {
    let mut world = seeded();
    answered(&mut world);
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let body = served(&get(&f, &path(AGENT), &cookie));
    let entries = versions(&body);
    let (first, later) = entries.split_first().expect("a deployment");
    assert_eq!(
        member(first, "classification"),
        &Value::Str("first_version".to_owned())
    );
    assert_eq!(member(first, "base_version"), &Value::Null);
    assert_eq!(member(first, "application"), &Value::Null);
    assert!(member(first, "deployed_at").as_str().is_some());
    let increase_only = [
        "increase_blocked_while_latched",
        "waiting_period",
        "still_below_new_floor",
    ];
    let mut answered_results = Vec::new();
    for entry in later {
        assert!(member(entry, "base_version").as_str().is_some());
        assert_eq!(member(entry, "deployed_at"), &Value::Null);
        let classification = member(entry, "classification");
        assert_ne!(
            classification,
            &Value::Str("first_version".to_owned()),
            "only the deployment is a first version"
        );
        let application = member(entry, "application");
        if *application == Value::Null {
            continue;
        }
        let result = member(application, "result").as_str().expect("a result");
        let reason = member(application, "reason");
        match result {
            "applied" => assert_eq!(reason, &Value::Null),
            "rejected" => {
                let reason = reason.as_str().expect("a rejection names its reason");
                if increase_only.contains(&reason) {
                    assert!(
                        matches!(classification, Value::Null)
                            || classification == &Value::Str("risk_increasing".to_owned()),
                        "{reason} is met only by an increase"
                    );
                }
            }
            other => panic!("result {other}"),
        }
        answered_results.push(result.to_owned());
    }
    assert_eq!(answered_results, vec!["applied", "rejected"]);
}

#[test]
#[ignore = "pending E10-27"]
fn the_same_agent_id_in_another_workspace_is_never_read_or_reflected() {
    let mut world = seeded();
    answered(&mut world);
    let [b, s, l, r] = all_documents();
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let alone = served(&get(&f, &path(AGENT), &cookie));
    let mut foreign_control = Stream::new(format!("ctl:{OTHER_WORKSPACE}"), 'F', 17);
    foreign_control.established();
    foreign_control.created(&l.version);
    foreign_control.deployed(AGENT, &l.version);
    let foreign_confirmed = foreign_control.confirmed(2, &r.version, Some(AGENT), Some(&l.version));
    let mut foreign_account = Stream::new(format!("acct:{OTHER_WORKSPACE}:acct_1"), 'G', 18);
    foreign_account.applied(
        AGENT,
        (&l.version, &r.version),
        &foreign_confirmed,
        None,
        "risk_reducing",
    );
    let [first, _, _] = world.confirmations.clone();
    foreign_account.applied(
        AGENT,
        (&b.version, &s.version),
        &first,
        Some("not_loosening"),
        "risk_reducing",
    );
    install(&f, &[&foreign_control, &foreign_account]);
    let beside = served(&get(&f, &path(AGENT), &cookie));
    assert_eq!(
        versions(&beside),
        versions(&alone),
        "workspace B's agent of the same id changes nothing in A's history"
    );
    assert_eq!(member(&beside, "in_effect"), member(&alone, "in_effect"));
    assert_eq!(as_of(&beside), vec![ctl(), acct()]);
    let shown = String::from_utf8_lossy(&get(&f, &path(AGENT), &cookie).body).into_owned();
    assert!(!shown.contains(OTHER_WORKSPACE), "{shown}");
    assert!(!shown.contains(&foreign_confirmed), "{shown}");
    let absent = get(&f, &path("agt_zz"), &cookie);
    let foreign = get(&f, &path_in(OTHER_WORKSPACE, AGENT), &cookie);
    assert_eq!(foreign.status, 404, "B's agent is not A's caller's to read");
    assert_eq!(foreign.body, absent.body);
}

#[test]
#[ignore = "pending E10-27"]
fn a_second_application_of_one_confirmation_is_journal_unavailable() {
    let mut world = seeded();
    let [b, s, l, r] = all_documents();
    let [first, _, _] = world.confirmations.clone();
    world.account.applied(
        AGENT,
        (&b.version, &s.version),
        &first,
        None,
        "risk_reducing",
    );
    world.account.applied(
        AGENT,
        (&b.version, &l.version),
        &first,
        None,
        "risk_increasing",
    );
    let f = fixture(&[&b, &s, &l, &r]);
    install(&f, &[&world.control, &world.account]);
    let cookie = operator(&f);
    let reply = get(&f, &path(AGENT), &cookie);
    assert_eq!(
        reply.status,
        503,
        "one confirmation answered twice cannot tell which version is in effect: {}",
        String::from_utf8_lossy(&reply.body)
    );
    assert_eq!(code(&reply), "journal_unavailable");
}
