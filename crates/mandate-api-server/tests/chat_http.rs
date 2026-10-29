//! The agent's chat over HTTP ([DEC-2293], workspace API §4.6.2): the owner's message is journaled
//! before its `202`, its run goes to the model gateway service over the frame protocol, every model
//! call and every tool result is journaled before anything acts on it, the agent's reply is
//! journaled before the thread serves it, and every record passes the journal's own §9.39 schemas
//! and rules. The gateway here is the real one, over a recorded OpenRouter fake.
//!
//! [DEC-2293]: ../../../docs/project/decisions/DEC-2293.md

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{MemJournal, Reply, WORKSPACE, hosted, send, serve};
use mandate_api_server::chat::{Chat, ChatModel};
use mandate_api_server::config::{SESSION_KEY_FILE, read_session_key};
use mandate_api_server::http::{self, App, Settings};
use mandate_api_server::lifecycle::Drain;
use mandate_api_server::objects::{ObjectStore, Objects};
use mandate_api_server::session::{COOKIE_NAME, OsRandom, Sessions};
use mandate_api_server::token::{FixedKeys, Issuer};
use mandate_authn::{Algorithm, IssuerConfig};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_chat::ChatEntry;
use mandate_chat::entry::name;
use mandate_gateway::fake::{Recorded, RecordedTransport, openrouter_reply};
use mandate_gateway::frame;
use mandate_gateway::guard::GuardContext;
use mandate_gateway::meter::{Caps, DeploymentMeter, MemoryMeter, Meter};
use mandate_gateway::money::{PriceTable, TokenPrice, Usage, Usd6};
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{
    Delivery, ManualClock, ProviderKey, SecretError, SecretSource, SequentialIds,
};
use mandate_gateway::registry::{
    Endpoint, Identity, Locality, Registry, Reported, Route, Snapshot, Status,
};
use mandate_gateway::{Gateway, RateCaps, Settings as GatewaySettings};
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId, Role};
use mandate_identity_seal::Seal;
use mandate_journal::{Draft, StoredEvent};
use mandate_time::UtcNanos;

const APP: &str = "https://app.owlhead.test";
const T0: i64 = 1_791_000_000;
const KEY: &str = "k1Z-9_cccccccccc";
const ALICE: &str = "01JB3K6F0C9R2V7N4M8Q1T5W3Z";
const PINNED_DOCUMENT: &str = include_str!("fixtures/mandate.json");
const SLUG: &str = "anthropic/claude-sonnet-5.5";
const UPSTREAM: &str = "anthropic";
const PROVIDER: &str = "Anthropic";

struct Fixture {
    addr: std::net::SocketAddr,
    journal: Arc<MemJournal>,
    sessions: Arc<Sessions>,
}

static DIRS: Mutex<u32> = Mutex::new(0);

fn chat_entry() -> ChatEntry {
    ChatEntry {
        model_id: "llm.chat_sonnet".to_owned(),
        model_version: "1.0.0".to_owned(),
        identity: Identity {
            provider: "anthropic".to_owned(),
            model_name: SLUG.to_owned(),
            snapshot: Snapshot::Dated("claude-sonnet-5-5".to_owned()),
            quantization: None,
        },
        deadline_ms: 120_000,
        max_output_tokens: 4_000,
    }
}

fn reported() -> Reported {
    Reported {
        model: Some(SLUG.to_owned()),
        provider: Some(PROVIDER.to_owned()),
    }
}

struct TestKey;

impl SecretSource for TestKey {
    fn provider_key(&self, upstream: &str) -> Result<ProviderKey, SecretError> {
        if upstream == UPSTREAM {
            Ok(ProviderKey::new("test-key-not-a-secret-0001".to_owned()))
        } else {
            Err(SecretError::Missing)
        }
    }
}

fn turn(calls: &[(&str, &str)], content: Option<&str>) -> Recorded {
    Recorded {
        latency_ms: 50,
        delivery: Delivery::Response {
            status: 200,
            body: openrouter_reply(
                content,
                calls,
                &reported(),
                Usage {
                    input: 1_000,
                    output: 100,
                    cached: 0,
                },
            ),
            retry_after_ms: None,
        },
    }
}

/// The model gateway service: the real gateway, offering the chat entry on one recorded endpoint
/// that answers `script` in order, served one framed call per connection.
fn gateway_service(script: Vec<Recorded>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let address = listener.local_addr().expect("its address").to_string();
    std::thread::spawn(move || {
        let mut registry = Registry::new();
        let content = chat_entry().content();
        let model = content.model_ref();
        registry.register(content).expect("registers");
        registry
            .set_status(&model, Status::Offered)
            .expect("offered");
        let endpoint = Endpoint {
            id: "openrouter:anthropic:chat".to_owned(),
            host: "openrouter.ai".to_owned(),
            region: "us".to_owned(),
            locality: Locality::Hosted,
            route: Route {
                upstream: UPSTREAM.to_owned(),
                reported_provider: PROVIDER.to_owned(),
            },
            prices: PriceTable::new(
                TokenPrice::per_million(Usd6::parse("3").expect("a price")),
                TokenPrice::per_million(Usd6::parse("15").expect("a price")),
                TokenPrice::per_million(Usd6::parse("0.3").expect("a price")),
                Digest::of(b"chat price table"),
            )
            .expect("a table"),
        };
        registry
            .add_endpoint(&model, endpoint, &[reported()])
            .expect("an endpoint");
        let mut gateway = Gateway::new(
            GatewaySettings {
                workspace_id: WORKSPACE.to_owned(),
                policy: EndpointPolicy::new(
                    BTreeSet::from([Locality::Hosted]),
                    Some(BTreeSet::from([UPSTREAM.to_owned()])),
                    None,
                ),
                guard: GuardContext::default(),
                rate: RateCaps {
                    per_workspace: 1_000,
                    per_agent: 1_000,
                },
            },
            registry,
            Box::new(OpenRouter),
            RecordedTransport::new(),
            Box::new(TestKey),
            Meter::new(
                MemoryMeter::new(),
                Caps::new(Usd6::parse("50").expect("a cap")),
                DeploymentMeter::new(
                    Box::new(MemoryMeter::new()),
                    Usd6::parse("1000000").expect("a cap"),
                    Box::new(SequentialIds::starting_at(1 << 100)),
                ),
            ),
            ManualClock::at(UtcNanos::from_parts(T0, 0).expect("a time")),
            Box::new(SequentialIds::starting_at(1 << 80)),
        );
        gateway.transport_mut().script(script);
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let _ = frame::serve(&mut stream, |request| gateway.call(&request));
        }
    });
    address
}

/// A server over a paper workspace with `agt_a` deployed, its chat served by `gateway`.
fn fixture(gateway: Option<String>) -> Fixture {
    let mut n = DIRS.lock().expect("unpoisoned");
    *n += 1;
    let dir = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "mandate-api-server-chat-{}-{n}",
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
    let records = vec![activated(1, alice(), &[Role::Operator])];
    let sessions = Arc::new(Sessions::new(&key, Box::new(OsRandom::new())));
    let clock = common::ManualClock::at(T0);
    let config = IssuerConfig::new(
        "https://issuer.test/auth/v1",
        &["authenticated"],
        &[Algorithm::Es256],
    )
    .expect("the issuer");
    let store = Objects::new(BTreeMap::<Digest, Vec<u8>>::new());
    store
        .put(&to_canonical(
            &parse(PINNED_DOCUMENT.as_bytes()).expect("JSON"),
        ))
        .expect("stored");
    let model = ChatModel {
        entry: chat_entry(),
        label: "claude-sonnet-5.5".to_owned(),
        input_per_million: "3".to_owned(),
        output_per_million: "15".to_owned(),
    };
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
        artifacts: Some(Arc::new(store)),
        objects: None,
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
        bars: None,
        chat: Arc::new(Chat::new(gateway, vec![model])),
    };
    let f = Fixture {
        addr: serve(http::api(app, Drain::new())),
        journal,
        sessions,
    };
    seed(&f);
    f
}

fn alice() -> PrincipalId {
    PrincipalId::from_ulid_text(ALICE).expect("a ULID")
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

fn signed_in(f: &Fixture) -> String {
    f.sessions
        .open(alice(), "sha256:aaaa".to_owned(), None, at(T0))
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

fn seed(f: &Fixture) {
    let bytes = to_canonical(&parse(PINNED_DOCUMENT.as_bytes()).expect("JSON"));
    let version = format!("sha256:{}", Digest::of(&bytes).to_hex());
    f.journal.streams.lock().expect("unpoisoned").insert(
        ctl(),
        vec![
            row(1, "StreamOpened", "{}"),
            row(
                2,
                "AgentDeployed",
                &format!(r#"{{"agent_id":"agt_a","mandate_version":"{version}"}}"#),
            ),
        ],
    );
}

fn messages_path() -> String {
    format!("/v1/workspaces/{WORKSPACE}/agents/agt_a/messages")
}

fn post_message(f: &Fixture, cookie: &str, text: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    let body = format!(r#"{{"text":"{text}"}}"#);
    send(
        f.addr,
        "POST",
        &messages_path(),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie),
            ("Idempotency-Key", KEY),
        ],
        body.as_bytes(),
    )
}

fn thread(f: &Fixture, cookie: &str) -> Value {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    let reply = send(f.addr, "GET", &messages_path(), &[("Cookie", &cookie)], b"");
    assert_eq!(
        reply.status,
        200,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    parse(&reply.body).expect("JSON")
}

/// The thread once its run has ended, read every 25 ms for up to 20 s.
fn settled(f: &Fixture, cookie: &str) -> Value {
    for _ in 0..800 {
        let body = thread(f, cookie);
        let ended = f
            .journal
            .rows_of(&ctl())
            .iter()
            .any(|row| row.event_type == "ChatRunEnded");
        if ended && body.get("running") == Some(&Value::Bool(false)) {
            return body;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    panic!("the run did not end within 20 seconds");
}

fn chat_rows(f: &Fixture) -> Vec<StoredEvent> {
    f.journal
        .rows_of(&ctl())
        .into_iter()
        .filter(|row| row.event_type.starts_with("Chat") || row.event_type == "AgentGoalsUpdated")
        .collect()
}

fn texts(thread: &Value) -> Vec<(String, String)> {
    thread
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .map(|m| {
            let text = |k: &str| {
                m.get(k)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            (text("from"), text("text"))
        })
        .collect()
}

#[test]
fn a_message_runs_through_the_gateway_and_every_step_is_journaled_before_it_shows() {
    let gateway = gateway_service(vec![
        turn(
            &[
                (name::POSITIONS, "{}"),
                (
                    name::REPLY,
                    r#"{"sources":[],"text":"You hold nothing yet."}"#,
                ),
            ],
            None,
        ),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    let cookie = signed_in(&f);
    let reply = post_message(&f, &cookie, "how am I doing?");
    assert_eq!(
        reply.status,
        202,
        "{}",
        String::from_utf8_lossy(&reply.body)
    );
    let accepted = parse(&reply.body).expect("JSON");
    assert!(
        accepted
            .get("run_id")
            .and_then(Value::as_str)
            .is_some_and(|r| r.starts_with("run_")),
        "{accepted:?}"
    );
    let body = settled(&f, &cookie);
    assert_eq!(
        texts(&body),
        [
            ("owner".to_owned(), "how am I doing?".to_owned()),
            ("agent".to_owned(), "You hold nothing yet.".to_owned()),
        ]
    );
    let types: Vec<String> = chat_rows(&f).into_iter().map(|r| r.event_type).collect();
    assert_eq!(
        types,
        [
            "ChatMessagePosted",
            "ChatTurnRecorded",
            "ChatMessagePosted",
            "ChatToolResultsRecorded",
            "ChatTurnRecorded",
            "ChatRunEnded",
        ],
        "the owner's message, then each call before its tools, the reply before the results it \
         belongs to, and the end"
    );
    for row in chat_rows(&f) {
        assert!(
            Draft::parse(&row.body).is_ok(),
            "{} passes journal §9.39: {:?}",
            row.event_type,
            Draft::parse(&row.body).err()
        );
    }
    let ended = chat_rows(&f).pop().expect("the end");
    let payload = parse(&ended.body).expect("JSON");
    assert_eq!(
        payload
            .get("payload")
            .and_then(|p| p.get("reason"))
            .and_then(Value::as_str),
        Some("answered")
    );
}

#[test]
fn with_no_gateway_the_run_says_so_and_ends_refused() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "hello").status, 202);
    let body = settled(&f, &cookie);
    let said = texts(&body);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[1].1.contains("isn't connected"), "{said:?}");
}

#[test]
fn a_blank_message_or_an_unknown_agent_is_refused_with_nothing_journaled() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "   ").status, 422);
    let cookie_header = format!("{COOKIE_NAME}={cookie}");
    let unknown = send(
        f.addr,
        "POST",
        &format!("/v1/workspaces/{WORKSPACE}/agents/agt_nope/messages"),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", KEY),
        ],
        br#"{"text":"hi"}"#,
    );
    assert_eq!(unknown.status, 404);
    assert!(chat_rows(&f).is_empty());
}

/// The `ChatToolResultsRecorded` results, as `(name, result)`.
fn results(f: &Fixture) -> Vec<(String, String)> {
    chat_rows(f)
        .iter()
        .filter(|row| row.event_type == "ChatToolResultsRecorded")
        .filter_map(|row| parse(&row.body).ok())
        .flat_map(|e| {
            e.get("payload")
                .and_then(|p| p.get("results"))
                .and_then(Value::as_array)
                .unwrap_or_default()
                .iter()
                .map(|r| {
                    let text = |k: &str| {
                        r.get(k)
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    };
                    (text("name"), text("result"))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn the_owners_pause_commits_the_pause_command_after_its_call_is_journaled() {
    let gateway = gateway_service(vec![
        turn(&[(name::PAUSE, "{}")], None),
        turn(&[(name::REPLY, r#"{"sources":[],"text":"Paused."}"#)], None),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "pause for now").status, 202);
    let body = settled(&f, &cookie);
    assert_eq!(texts(&body).last().map(|t| t.1.as_str()), Some("Paused."));
    let found = results(&f);
    let pause = found
        .iter()
        .find(|(n, _)| n == name::PAUSE)
        .expect("the pause's result is journaled");
    assert!(pause.1.contains(r#""ok":true"#), "{pause:?}");
    let rows = f.journal.rows_of(&ctl());
    let turn_at = rows
        .iter()
        .position(|r| r.event_type == "ChatTurnRecorded")
        .expect("the call");
    let command_at = rows
        .iter()
        .position(|r| !r.event_type.starts_with("Chat") && r.seq > 2)
        .expect("the pause command's record");
    assert!(turn_at < command_at, "the call is journaled before it acts");
}

#[test]
fn a_run_past_its_daily_cap_spends_nothing_and_says_so() {
    let gateway = gateway_service(Vec::new());
    let f = fixture(Some(gateway));
    let cookie = signed_in(&f);
    let cookie_header = format!("{COOKIE_NAME}={cookie}");
    let settings = send(
        f.addr,
        "PUT",
        &format!("/v1/workspaces/{WORKSPACE}/agents/agt_a/settings"),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", "k1Z-9_dddddddddd"),
        ],
        br#"{"chat_model":"llm.chat_sonnet","daily_cap_usd":"0.000001"}"#,
    );
    assert_eq!(
        settings.status,
        200,
        "{}",
        String::from_utf8_lossy(&settings.body)
    );
    let mut spent = row(
        3,
        "ChatTurnRecorded",
        r#"{"agent_id":"agt_a","cost_usd":"0.01"}"#,
    );
    spent.recorded_at = UtcNanos::from_parts(T0, 0).expect("a time").to_string();
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .get_mut(&ctl())
        .expect("the control stream")
        .push(spent);
    assert_eq!(post_message(&f, &cookie, "anything new?").status, 202);
    let body = settled(&f, &cookie);
    assert!(
        texts(&body)
            .last()
            .is_some_and(|t| t.1.contains("spending cap")),
        "{body:?}"
    );
}
