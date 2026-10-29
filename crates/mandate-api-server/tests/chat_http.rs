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
use mandate_api_server::chat_watch::pass_once;
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
    app: Arc<App>,
    clock: Arc<common::ManualClock>,
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
    gateway_service_gated(script, None)
}

/// [`gateway_service`], answering its first call only once `gate` is sent to.
fn gateway_service_gated(
    script: Vec<Recorded>,
    gate: Option<std::sync::mpsc::Receiver<()>>,
) -> String {
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
        let mut gate = gate;
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            if let Some(held) = gate.take() {
                let _ = held.recv();
            }
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
    let store = Arc::new(Objects::new(BTreeMap::<Digest, Vec<u8>>::new()));
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
        artifacts: Some(
            Arc::clone(&store) as Arc<dyn mandate_journal::ArtifactSource + Send + Sync>
        ),
        objects: Some(store as Arc<dyn ObjectStore>),
        step_up: Arc::new(mandate_api_server::stepup::Unbound),
        challenges: Arc::default(),
        connect: None,
        reduction: Arc::default(),
        folds: Arc::default(),
        bars: None,
        chat: Arc::new(Chat::new(gateway, vec![model])),
    };
    let app = Arc::new(app);
    let f = Fixture {
        app: Arc::clone(&app),
        clock,
        addr: serve(http::api_shared(app, Drain::new())),
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
        .open(alice(), "sha256:aaaa".to_owned(), None, at(f.clock.secs()))
        .expect("opened")
        .value()
        .to_owned()
}

fn ctl() -> String {
    format!("ctl:{WORKSPACE}")
}

fn row(seq: u64, event_type: &str, payload: &str) -> StoredEvent {
    row_on(&ctl(), seq, event_type, payload, "")
}

/// A row of `stream`, its envelope carrying `extra` members (each followed by a comma) too.
fn row_on(stream: &str, seq: u64, event_type: &str, payload: &str, extra: &str) -> StoredEvent {
    let event_id = event_id_at(stream, seq);
    let text = format!(
        r#"{{{extra}"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"stream_id":"{stream}"}}"#
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

/// A distinct event id per stream and seq.
fn event_id_at(stream: &str, seq: u64) -> String {
    let lane = match stream.split(':').next() {
        Some("acct") => 'A',
        Some("agent") => 'G',
        _ => 'C',
    };
    format!("01JBH3BTZ8K2M4N6P8R0S2{lane}{seq:03}")
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

/// The thread once its run has ended, read every 25 ms for up to 20 s. The end is checked before
/// the thread is read, so the read holds everything the run journaled before its end.
fn settled(f: &Fixture, cookie: &str) -> Value {
    for _ in 0..800 {
        let ended = f
            .journal
            .rows_of(&ctl())
            .iter()
            .any(|row| row.event_type == "ChatRunEnded");
        let body = thread(f, cookie);
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
fn a_read_after_a_message_s_seq_serves_only_the_messages_after_it() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "hello").status, 202);
    let body = settled(&f, &cookie);
    let seqs: Vec<u64> = body
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("seq").and_then(Value::as_int))
        .collect();
    assert_eq!(seqs.len(), 2, "{body:?}");
    let header = format!("{COOKIE_NAME}={cookie}");
    let path = format!("{}?after={}", messages_path(), seqs[0]);
    let reply = send(f.addr, "GET", &path, &[("Cookie", &header)], b"");
    assert_eq!(reply.status, 200);
    let after = parse(&reply.body).expect("JSON");
    let served: Vec<u64> = after
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("seq").and_then(Value::as_int))
        .collect();
    assert_eq!(
        served,
        vec![seqs[1]],
        "the cursor's own message is not served again"
    );
}

/// A message to a setup thread at `query`, as the setup screen sends it.
fn post_setup(f: &Fixture, cookie: &str, query: &str, key: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "POST",
        &format!("/v1/workspaces/{WORKSPACE}/setup/messages{query}"),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie),
            ("Idempotency-Key", key),
        ],
        br#"{"text":"an agent for SPY"}"#,
    )
}

#[test]
fn a_setup_message_keeps_a_named_setup_and_refuses_a_name_that_is_not_an_identifier() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let named = post_setup(&f, &cookie, "?setup_id=setup_one-2", "setupkey00000000001");
    assert_eq!(
        named.status,
        202,
        "{}",
        String::from_utf8_lossy(&named.body)
    );
    let answer = parse(&named.body).expect("JSON");
    assert_eq!(
        answer.get("setup_id").and_then(Value::as_str),
        Some("setup_one-2")
    );
    for _ in 0..800 {
        if chat_rows(&f)
            .iter()
            .any(|row| row.event_type == "ChatRunEnded")
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let before = chat_rows(&f).len();
    for query in ["?setup_id=", "?setup_id=a%20b", "?setup_id=a.b"] {
        let refused = post_setup(&f, &cookie, query, "setupkey00000000002");
        assert_eq!(refused.status, 422, "{query}");
    }
    assert_eq!(
        chat_rows(&f).len(),
        before,
        "a refused setup id journals nothing"
    );
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

const AAPL: &str = "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415";

/// One watcher pass, on a blocking thread of a runtime of its own.
fn pass(f: &Fixture) -> usize {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("a runtime");
    let app = Arc::clone(&f.app);
    let posted = runtime.block_on(async move {
        tokio::task::spawn_blocking(move || pass_once(&app, &tokio::runtime::Handle::current()))
            .await
            .expect("the pass ran")
    });
    runtime.shutdown_timeout(Duration::from_secs(20));
    posted
}

fn push(f: &Fixture, stream: &str, rows: Vec<StoredEvent>) {
    f.journal
        .streams
        .lock()
        .expect("unpoisoned")
        .entry(stream.to_owned())
        .or_default()
        .extend(rows);
}

/// `agt_a`'s connection established on `acct_1`, and on it an order of the agent's own that filled.
fn seed_fill(f: &Fixture) {
    let mut established = row(
        3,
        "ConnectionEstablished",
        r#"{"account_ref":"acct_1","broker":"alpaca","connection_id":"conn_alpaca_paper_01","environment":"paper"}"#,
    );
    established.schema_version = 2;
    push(f, &ctl(), vec![established]);
    let acct = format!("acct:{WORKSPACE}:acct_1");
    let request = event_id_at(&acct, 2);
    push(
        f,
        &acct,
        vec![
            row_on(&acct, 1, "StreamOpened", "{}", ""),
            row_on(
                &acct,
                2,
                "OrderRequestRecorded",
                r#"{"agent_id":"agt_a","intent_id":"int_1","purpose":"entry"}"#,
                "",
            ),
            row_on(
                &acct,
                3,
                "OrderSubmitted",
                &format!(
                    r#"{{"client_order_id":"co_1","instrument_id":"{AAPL}","side":"buy","qty":"10","type":"limit","tif":"day","limit_price":"191"}}"#
                ),
                &format!(r#""causation_id":"{request}","#),
            ),
            row_on(
                &acct,
                4,
                "FillApplied",
                &format!(
                    r#"{{"fill_id":"f_1","client_order_id":"co_1","instrument_id":"{AAPL}","side":"buy","qty_gross":"10","price":"190.12"}}"#
                ),
                "",
            ),
        ],
    );
}

fn seed_approval(f: &Fixture) {
    let stream = format!("agent:{WORKSPACE}:agt_a");
    let hash = format!("sha256:{}", "ab".repeat(32));
    push(
        f,
        &stream,
        vec![
            row_on(&stream, 1, "StreamOpened", "{}", ""),
            row_on(
                &stream,
                2,
                "ApprovalRequested",
                &format!(
                    r#"{{"deadline":{},"content_hash":"{hash}","content":{{"action":{{"instrument":"{AAPL}","asset_class":"us_equity","side":"buy","qty":"5","limit":"191","order_usd":"955","purpose":"entry"}}}}}}"#,
                    T0 + 600
                ),
                "",
            ),
        ],
    );
}

/// Alice writes to `agt_a`, so her thread hears from it, and the run that answers ends.
fn alice_writes(f: &Fixture) -> String {
    let cookie = signed_in(f);
    assert_eq!(post_message(f, &cookie, "hello").status, 202);
    settled(f, &cookie);
    cookie
}

#[test]
fn a_fill_of_the_agents_own_order_is_said_once_with_the_fill_as_its_source() {
    let f = fixture(None);
    let cookie = alice_writes(&f);
    seed_fill(&f);
    assert_eq!(pass(&f), 1);
    assert_eq!(pass(&f), 0, "a second pass says nothing again");
    let body = thread(&f, &cookie);
    let said = texts(&body);
    assert_eq!(
        said.last().map(|t| t.1.as_str()),
        Some("Bought 10 AAPL at 190.12.")
    );
    let last = body
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|m| m.last().cloned())
        .expect("the message");
    assert_eq!(last.get("proactive"), Some(&Value::Bool(true)));
    let fill = event_id_at(&format!("acct:{WORKSPACE}:acct_1"), 4);
    assert_eq!(
        last.get("sources")
            .and_then(Value::as_array)
            .and_then(|s| s.first())
            .and_then(|s| s.get("ref"))
            .and_then(Value::as_str),
        Some(format!("ev:{fill}").as_str())
    );
    for row in chat_rows(&f) {
        assert!(Draft::parse(&row.body).is_ok(), "{}", row.event_type);
    }
}

#[test]
fn an_approval_the_agent_waits_on_is_asked_once_as_a_card() {
    let f = fixture(None);
    let cookie = alice_writes(&f);
    seed_approval(&f);
    assert_eq!(pass(&f), 1);
    assert_eq!(pass(&f), 0);
    let body = thread(&f, &cookie);
    let last = body
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|m| m.last().cloned())
        .expect("the message");
    assert_eq!(
        last.get("text").and_then(Value::as_str),
        Some("I'd like your OK to buy 5 AAPL.")
    );
    assert_eq!(
        last.get("card")
            .and_then(|c| c.get("kind"))
            .and_then(Value::as_str),
        Some("approval")
    );
}

#[test]
fn no_one_has_written_and_nothing_is_said() {
    let f = fixture(None);
    seed_approval(&f);
    assert_eq!(
        pass(&f),
        0,
        "an agent no member deployed or wrote to has no thread to post in"
    );
}

#[test]
fn after_the_close_on_a_weekday_the_daily_note_runs_once_and_its_marker_is_not_shown() {
    let gateway = gateway_service(vec![
        turn(
            &[(
                name::REPLY,
                r#"{"sources":[],"text":"Quiet day: nothing filled."}"#,
            )],
            None,
        ),
        turn(&[], Some(r#"{"watch":[]}"#)),
        turn(&[(name::REPLY, r#"{"sources":[],"text":"Hello."}"#)], None),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    alice_writes(&f);
    let monday_close = (T0.div_euclid(86_400) + 2) * 86_400 + 21 * 3_600 + 60;
    f.clock.set(monday_close);
    let cookie = signed_in(&f);
    assert_eq!(pass(&f), 1, "the note starts");
    assert_eq!(pass(&f), 0, "and only once that day");
    let said = texts(&thread(&f, &cookie));
    assert!(
        said.iter().any(|t| t.1 == "Quiet day: nothing filled."),
        "{said:?}"
    );
    assert!(
        said.iter().all(|t| !t.1.is_empty()),
        "the marker is not shown: {said:?}"
    );
}

/// The result each tool of `name` gave in the run, in order.
fn results_of<'a>(found: &'a [(String, String)], tool: &str) -> Vec<&'a str> {
    found
        .iter()
        .filter(|(n, _)| n == tool)
        .map(|(_, r)| r.as_str())
        .collect()
}

#[test]
fn the_read_tools_answer_from_the_agent_s_own_records() {
    let gateway = gateway_service(vec![
        turn(
            &[
                (
                    name::NOTE,
                    r#"{"goals":[{"text":"Hold AAPL","status":"active"}],"plan":"Wait for a dip.","watching":["AAPL"]}"#,
                ),
                (name::GOALS, "{}"),
                (name::SUMMARY, "{}"),
                (name::POSITIONS, "{}"),
                (name::ORDERS, "{}"),
                (name::MANDATE, "{}"),
            ],
            None,
        ),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    seed_fill(&f);
    seed_approval(&f);
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "how are we doing?").status, 202);
    settled(&f, &cookie);
    let found = results(&f);
    let one = |tool: &str| {
        results_of(&found, tool)
            .first()
            .copied()
            .unwrap_or_default()
            .to_owned()
    };
    assert_eq!(one(name::NOTE), r#"{"ok":true}"#);
    let goals = one(name::GOALS);
    assert!(
        goals.contains(r#""goals":[{"status":"active","text":"Hold AAPL"}]"#),
        "{goals}"
    );
    assert!(goals.contains(r#""plan":"Wait for a dip.""#), "{goals}");
    assert!(goals.contains(r#""watching":["AAPL"]"#), "{goals}");
    assert_eq!(
        one(name::SUMMARY),
        r#"{"approvals_waiting":1,"mode":"active","ok":true,"positions":[]}"#
    );
    assert_eq!(one(name::POSITIONS), r#"{"ok":true,"positions":[]}"#);
    let orders = one(name::ORDERS);
    assert!(
        orders.starts_with(r#"{"ok":true,"orders":[{"client_order_id":"co_1","#),
        "{orders}"
    );
    assert!(
        orders.contains(&format!(r#""instrument_id":"{AAPL}""#)),
        "{orders}"
    );
    let mandate = one(name::MANDATE);
    assert!(mandate.starts_with(r#"{"name":"tracer-aapl","ok":true,"pinned_universe":true,"summary":{"allocation_usd":"1000","#), "{mandate}");
}

#[test]
fn a_thread_with_a_run_in_flight_says_so_and_refuses_another_message() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let key = mandate_api_server::chat_run::Thread::agent("agt_a", ALICE).key();
    assert!(f.app.chat.claim(&key, "run_held"));
    let body = thread(&f, &cookie);
    assert_eq!(body.get("running"), Some(&Value::Bool(true)));
    assert_eq!(body.get("run_id").and_then(Value::as_str), Some("run_held"));
    let before = chat_rows(&f).len();
    let refused = post_message(&f, &cookie, "are you there?");
    assert_eq!(
        refused.status,
        429,
        "{}",
        String::from_utf8_lossy(&refused.body)
    );
    assert!(
        String::from_utf8_lossy(&refused.body).contains("rate_limited"),
        "{}",
        String::from_utf8_lossy(&refused.body)
    );
    assert_eq!(
        chat_rows(&f).len(),
        before,
        "nothing is journaled for a refused message"
    );
    f.app.chat.release(&key);
    let free = thread(&f, &cookie);
    assert_eq!(free.get("running"), Some(&Value::Bool(false)));
    assert_eq!(free.get("run_id"), Some(&Value::Null));
}

/// The agent's chat settings, as `GET .../settings` serves them.
fn get_settings(f: &Fixture, cookie: &str, agent: &str) -> Reply {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    send(
        f.addr,
        "GET",
        &format!("/v1/workspaces/{WORKSPACE}/agents/{agent}/settings"),
        &[("Cookie", &cookie)],
        b"",
    )
}

#[test]
fn an_agent_s_settings_are_its_own_or_the_defaults() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let first = get_settings(&f, &cookie, "agt_a");
    assert_eq!(
        first.status,
        200,
        "{}",
        String::from_utf8_lossy(&first.body)
    );
    let read = parse(&first.body).expect("JSON");
    assert_eq!(read.get("agent_id").and_then(Value::as_str), Some("agt_a"));
    assert_eq!(
        read.get("chat_model").and_then(Value::as_str),
        Some("llm.chat_sonnet")
    );
    assert_eq!(read.get("daily_cap_usd").and_then(Value::as_str), Some("5"));
    push(
        &f,
        &ctl(),
        vec![
            row(
                3,
                "ChatSettingsChanged",
                r#"{"agent_id":"agt_b","chat_model":"llm.chat_sonnet","daily_cap_usd":"9"}"#,
            ),
            row(
                4,
                "ChatSettingsChanged",
                r#"{"agent_id":"agt_a","chat_model":"llm.unknown","daily_cap_usd":"2.5"}"#,
            ),
            row(
                5,
                "ChatSettingsChanged",
                r#"{"agent_id":"agt_c","chat_model":"llm.chat_sonnet","daily_cap_usd":"7"}"#,
            ),
        ],
    );
    let own = parse(&get_settings(&f, &cookie, "agt_a").body).expect("JSON");
    assert_eq!(
        own.get("daily_cap_usd").and_then(Value::as_str),
        Some("2.5"),
        "the agent's own latest setting, not another agent's"
    );
    assert_eq!(
        own.get("chat_model").and_then(Value::as_str),
        Some("llm.chat_sonnet"),
        "a model the deployment no longer offers falls back to the default"
    );
    assert_eq!(get_settings(&f, &cookie, "agt_nope").status, 404);
}

#[test]
fn a_run_that_has_replied_says_nothing_more_when_it_ends_on_an_error() {
    let gateway = gateway_service(vec![turn(
        &[(name::REPLY, r#"{"sources":[],"text":"On it."}"#)],
        None,
    )]);
    let f = fixture(Some(gateway));
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "check the account").status, 202);
    let body = settled(&f, &cookie);
    assert_eq!(
        texts(&body),
        vec![
            ("owner".to_owned(), "check the account".to_owned()),
            ("agent".to_owned(), "On it.".to_owned()),
        ]
    );
    let ended = chat_rows(&f)
        .iter()
        .filter(|row| row.event_type == "ChatRunEnded")
        .filter_map(|row| parse(&row.body).ok())
        .filter_map(|e| {
            e.get("payload")
                .and_then(|p| p.get("reason"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect::<Vec<_>>();
    assert_eq!(ended, vec!["model_error".to_owned()]);
}

#[test]
fn a_run_that_ends_on_an_error_before_replying_says_why() {
    let f = fixture(Some(gateway_service(Vec::new())));
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "check the account").status, 202);
    let body = settled(&f, &cookie);
    let said = texts(&body);
    assert_eq!(said.len(), 2, "{said:?}");
    assert!(said[1].1.starts_with("I couldn't finish that"), "{said:?}");
}

/// The prompts of the run's model calls, oldest first, read back from the object store.
fn prompts(f: &Fixture) -> Vec<String> {
    let Some(store) = f.app.objects.as_ref() else {
        return Vec::new();
    };
    chat_rows(f)
        .iter()
        .filter(|row| row.event_type == "ChatTurnRecorded")
        .filter_map(|row| parse(&row.body).ok())
        .filter_map(|e| {
            e.get("payload")
                .and_then(|p| p.get("prompt_ref"))
                .and_then(Value::as_str)
                .and_then(mandate_journal::ArtifactRef::parse)
        })
        .filter_map(|reference| store.read_artifact(&reference).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .collect()
}

#[test]
fn each_call_s_prompt_holds_the_agent_s_context_and_its_thread() {
    let gateway = gateway_service(vec![
        turn(&[(name::REPLY, r#"{"sources":[],"text":"Hi."}"#)], None),
        turn(&[], Some(r#"{"watch":[]}"#)),
        turn(
            &[(name::REPLY, r#"{"sources":[],"text":"Still here."}"#)],
            None,
        ),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "hello").status, 202);
    settled(&f, &cookie);
    let cookie_header = format!("{COOKIE_NAME}={cookie}");
    let second = send(
        f.addr,
        "POST",
        &messages_path(),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", "k1Z-9_eeeeeeeeee"),
        ],
        br#"{"text":"and now?"}"#,
    );
    assert_eq!(second.status, 202);
    for _ in 0..800 {
        let ended = chat_rows(&f)
            .iter()
            .filter(|r| r.event_type == "ChatRunEnded")
            .count();
        if ended == 2 {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let all = prompts(&f);
    assert_eq!(all.len(), 4, "a prompt for every call");
    let first = &all[0];
    assert!(
        first.contains("Agent: tracer-aapl (agt_a). Environment: paper. Stopped: false."),
        "{first}"
    );
    assert!(first.contains("Mandate summary: {"), "{first}");
    assert!(
        first.contains(r"<<<data>>>\nOwner: hello\n<<<end data>>>"),
        "{first}"
    );
    let later = &all[2];
    assert!(
        later.contains(
            r"Conversation so far:\n<<<data>>>\nOwner: hello\nAgent: Hi.\nOwner: and now?\n"
        ),
        "the thread, oldest first, as the prompt's JSON text holds it: {later}"
    );
}

#[test]
fn the_owner_s_stop_ends_the_run_before_its_next_call() {
    let (release, gate) = std::sync::mpsc::channel();
    let gateway = gateway_service_gated(
        vec![
            turn(&[(name::GOALS, "{}")], None),
            turn(&[(name::REPLY, r#"{"sources":[],"text":"Done."}"#)], None),
            turn(&[], Some(r#"{"watch":[]}"#)),
        ],
        Some(gate),
    );
    let f = fixture(Some(gateway));
    let cookie = signed_in(&f);
    let sent = post_message(&f, &cookie, "think it over");
    assert_eq!(sent.status, 202);
    let run = parse(&sent.body)
        .ok()
        .and_then(|b| b.get("run_id").and_then(Value::as_str).map(str::to_owned))
        .expect("the run's id");
    let cookie_header = format!("{COOKIE_NAME}={cookie}");
    let stop = |run: &str, key: &str| {
        send(
            f.addr,
            "POST",
            &format!("/v1/workspaces/{WORKSPACE}/agents/agt_a/runs/{run}/stop"),
            &[
                ("Origin", APP),
                ("X-Mandate-Request", "1"),
                ("Content-Type", "application/json"),
                ("Cookie", &cookie_header),
                ("Idempotency-Key", key),
            ],
            b"{}",
        )
    };
    assert_eq!(
        stop("run_not_this_one", "k1Z-9_ffffffffff").status,
        404,
        "only the run in flight stops"
    );
    let stopped = stop(&run, "k1Z-9_gggggggggg");
    assert_eq!(
        stopped.status,
        202,
        "{}",
        String::from_utf8_lossy(&stopped.body)
    );
    release.send(()).expect("the gateway waits");
    let body = settled(&f, &cookie);
    assert_eq!(texts(&body).last().map(|t| t.1.as_str()), Some("Stopped."));
    let rows = chat_rows(&f);
    assert!(rows.iter().any(|r| r.event_type == "ChatRunStopRequested"));
    let reasons: Vec<String> = rows
        .iter()
        .filter(|r| r.event_type == "ChatRunEnded")
        .filter_map(|r| parse(&r.body).ok())
        .filter_map(|e| {
            e.get("payload")
                .and_then(|p| p.get("reason"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(reasons, vec!["stopped".to_owned()]);
    assert!(
        rows.iter()
            .filter(|r| r.event_type == "ChatTurnRecorded")
            .count()
            <= 1,
        "no call after the Stop: only the one held in flight, if the run had made it"
    );
}

const MSFT: &str = "5f1e4c2a-8d3b-4e7f-9a6c-2b1d0e9f8a7c";

/// Stores `object` in the run's store and answers its reference.
fn store(f: &Fixture, object: &str) -> String {
    let objects = f.app.objects.as_ref().expect("the store");
    let bytes = to_canonical(&parse(object.as_bytes()).expect("JSON"));
    objects.put(&bytes).expect("stored").to_string()
}

/// AAPL, which `agt_a`'s mandate pins, and MSFT, which it does not, as registered snapshots; a
/// thesis of the agent's; and two news items, AAPL's unlicensed with its headline stored and MSFT's
/// licensed.
fn seed_catalogue(f: &Fixture) {
    let snapshot = |id: &str, symbol: &str| {
        store(
            f,
            &format!(
                r#"{{"asset_class":"us_equity","etp":"plain","etp_classified_at":"2026-10-05T13:30:00Z","etp_source":"nasdaq_trader_symbol_directory","exchange":"nasdaq","increment":"whole","instrument_id":"{id}","symbol":"{symbol}"}}"#
            ),
        )
    };
    let registered = |seq: u64, hash: &str| {
        row(
            seq,
            "ConfigSnapshotRegistered",
            &format!(
                r#"{{"admits_instruments":null,"content_hash":"{hash}","kind":"instrument_snapshot","model_id":null,"model_version":null,"params":[]}}"#
            ),
        )
    };
    let aapl = snapshot(AAPL, "AAPL");
    let msft = snapshot(MSFT, "MSFT");
    push(
        f,
        &ctl(),
        vec![registered(10, &aapl), registered(11, &msft)],
    );
    let agent = format!("agent:{WORKSPACE}:agt_a");
    push(
        f,
        &agent,
        vec![row_on(
            &agent,
            9,
            "ThesisProposed",
            r#"{"thesis_id":"th_1","text":"Apple's services keep growing."}"#,
            "",
        )],
    );
    let headline = store(f, r#"{"headline":"Apple ships a thing"}"#);
    let news = format!("news:{WORKSPACE}");
    push(
        f,
        &news,
        vec![
            row_on(
                &news,
                1,
                "NewsItemStored",
                &format!(
                    r#"{{"item_id":"n_1","item_ref":"{headline}","licensed":false,"published_at":"2026-10-17T09:00:00Z","symbols":["AAPL"]}}"#
                ),
                "",
            ),
            row_on(
                &news,
                2,
                "NewsItemStored",
                &format!(
                    r#"{{"item_id":"n_2","item_ref":"{headline}","licensed":true,"published_at":"2026-10-17T09:30:00Z","symbols":["MSFT"]}}"#
                ),
                "",
            ),
        ],
    );
}

#[test]
fn the_catalogue_and_research_tools_answer_and_a_buy_is_checked_before_it_is_sent() {
    let gateway = gateway_service(vec![
        turn(
            &[
                (name::SEARCH, r#"{"query":"aa"}"#),
                (name::SEARCH, r#"{"query":""}"#),
                (name::SEARCH, r#"{"query":"zzz"}"#),
                (name::PRICE, r#"{"symbol":"aapl"}"#),
                (name::PRICE, r#"{"symbol":"ZZZ"}"#),
                (name::BUY, r#"{"symbol":"zzz","quantity":"1"}"#),
                (name::BUY, r#"{"symbol":"AAPL","quantity":"0"}"#),
                (name::BUY, r#"{"symbol":"MSFT","quantity":"1"}"#),
                (name::THESES, "{}"),
                (name::NEWS, r#"{"symbol":"aapl"}"#),
                (name::NEWS, r#"{"symbol":"TSLA"}"#),
                (name::NEWS, r#"{"symbol":""}"#),
                (name::SELL, r#"{"quantity":"all","symbol":"zzz"}"#),
            ],
            None,
        ),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    seed_catalogue(&f);
    let cookie = signed_in(&f);
    assert_eq!(post_message(&f, &cookie, "look around").status, 202);
    settled(&f, &cookie);
    let found = results(&f);
    let aapl = format!(r#"{{"asset_class":"us_equity","instrument_id":"{AAPL}","symbol":"AAPL"}}"#);
    let msft = format!(r#"{{"asset_class":"us_equity","instrument_id":"{MSFT}","symbol":"MSFT"}}"#);
    let thesis = event_id_at(&format!("agent:{WORKSPACE}:agt_a"), 9);
    let apple = r#"{"headline":"Apple ships a thing","published_at":"2026-10-17T09:00:00Z","ref":"n_1","symbols":["AAPL"]}"#;
    let expected: Vec<(&str, String)> = vec![
        (
            name::SEARCH,
            format!(r#"{{"instruments":[{aapl}],"ok":true}}"#),
        ),
        (
            name::SEARCH,
            format!(r#"{{"instruments":[{aapl},{msft}],"ok":true}}"#),
        ),
        (name::SEARCH, r#"{"instruments":[],"ok":true}"#.to_owned()),
        (
            name::PRICE,
            r#"{"ok":false,"refused":"no_store"}"#.to_owned(),
        ),
        (
            name::PRICE,
            r#"{"ok":false,"refused":"unknown_symbol"}"#.to_owned(),
        ),
        (
            name::BUY,
            r#"{"ok":false,"refused":"unknown_symbol"}"#.to_owned(),
        ),
        (
            name::BUY,
            r#"{"ok":false,"refused":"quantity_positive"}"#.to_owned(),
        ),
        (
            name::BUY,
            r#"{"ok":false,"refused":"not_in_mandate"}"#.to_owned(),
        ),
        (
            name::THESES,
            format!(
                r#"{{"ok":true,"theses":[{{"payload":{{"text":"Apple's services keep growing.","thesis_id":"th_1"}},"record":"{thesis}","type":"ThesisProposed"}}],"untrusted":true}}"#
            ),
        ),
        (
            name::NEWS,
            format!(r#"{{"news":[{apple}],"ok":true,"untrusted":true}}"#),
        ),
        (
            name::NEWS,
            r#"{"news":[],"ok":true,"untrusted":true}"#.to_owned(),
        ),
        (
            name::NEWS,
            format!(
                r#"{{"news":[{{"headline":null,"published_at":"2026-10-17T09:30:00Z","ref":"n_2","symbols":["MSFT"]}},{apple}],"ok":true,"untrusted":true}}"#
            ),
        ),
        (
            name::SELL,
            r#"{"ok":false,"refused":"unknown_symbol"}"#.to_owned(),
        ),
    ];
    let expected: Vec<(String, String)> = expected
        .into_iter()
        .map(|(n, r)| (n.to_owned(), r))
        .collect();
    assert_eq!(found, expected);
}

#[test]
fn the_owner_s_actions_commit_through_the_owner_s_own_routes() {
    let gateway = gateway_service(vec![
        turn(
            &[
                (name::BUY, r#"{"quantity":"2","symbol":"aapl"}"#),
                (name::SELL, r#"{"quantity":"all","symbol":"AAPL"}"#),
                (name::RESUME, "{}"),
            ],
            None,
        ),
        turn(&[(name::STOP, "{}")], None),
        turn(&[(name::KILL, "{}")], None),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    seed_catalogue(&f);
    let cookie = signed_in(&f);
    assert_eq!(
        post_message(&f, &cookie, "buy 2 AAPL, then sell it all").status,
        202
    );
    settled(&f, &cookie);
    let recorded: BTreeMap<String, String> = f
        .journal
        .streams
        .lock()
        .expect("unpoisoned")
        .values()
        .flatten()
        .map(|row| (row.event_id.clone(), row.event_type.clone()))
        .collect();
    let found = results(&f);
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [name::BUY, name::SELL, name::RESUME, name::STOP, name::KILL]
    );
    let prefix = r#"{"accepted":true,"note":"Recorded. The platform sizes, checks and places it; watch the order card for the result.","ok":true,"record":""#;
    for ((tool, result), journaled) in found.iter().zip([
        "OwnerRequestSubmitted",
        "OwnerCommandIssued",
        "OwnerCommandIssued",
        "OwnerCommandIssued",
        "OwnerCommandIssued",
    ]) {
        let record = result
            .strip_prefix(prefix)
            .and_then(|rest| rest.split('"').next())
            .unwrap_or_default();
        assert_eq!(
            recorded.get(record).map(String::as_str),
            Some(journaled),
            "{tool}: {result}"
        );
    }
    assert!(
        found[1]
            .1
            .ends_with("\nNote: a sale exits the whole position in AAPL."),
        "{}",
        found[1].1
    );
}

/// A `GET` of `path` under the workspace, signed in as `cookie`, answered as JSON with its status.
fn get_json(f: &Fixture, cookie: &str, path: &str) -> (u16, Value) {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    let reply = send(
        f.addr,
        "GET",
        &format!("/v1/workspaces/{WORKSPACE}{path}"),
        &[("Cookie", &cookie)],
        b"",
    );
    (reply.status, parse(&reply.body).unwrap_or(Value::Null))
}

/// A `PUT` of the agent's settings with `body`, under `key`.
fn put_settings(f: &Fixture, cookie: &str, agent: &str, body: &[u8], key: &str) -> (u16, Value) {
    let cookie = format!("{COOKIE_NAME}={cookie}");
    let reply = send(
        f.addr,
        "PUT",
        &format!("/v1/workspaces/{WORKSPACE}/agents/{agent}/settings"),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie),
            ("Idempotency-Key", key),
        ],
        body,
    );
    (reply.status, parse(&reply.body).unwrap_or(Value::Null))
}

fn member_text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |v, k| v.get(k))
        .and_then(Value::as_str)
}

#[test]
fn home_lists_each_agent_with_its_name_mode_last_message_unread_count_and_whether_it_waits() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let (status, quiet) = get_json(&f, &cookie, "/home");
    assert_eq!(status, 200);
    let agents = quiet
        .get("agents")
        .and_then(Value::as_array)
        .unwrap_or_default();
    assert_eq!(agents.len(), 1);
    let agent = &agents[0];
    assert_eq!(member_text(agent, &["agent_id"]), Some("agt_a"));
    assert_eq!(member_text(agent, &["name"]), Some("tracer-aapl"));
    assert_eq!(member_text(agent, &["mode"]), Some("active"));
    assert_eq!(agent.get("needs_you"), Some(&Value::Bool(false)));
    assert_eq!(agent.get("unread").and_then(Value::as_int), Some(0));
    assert_eq!(agent.get("last_message"), Some(&Value::Null));
    assert_eq!(post_message(&f, &cookie, "hello").status, 202);
    settled(&f, &cookie);
    seed_approval(&f);
    let (_, busy) = get_json(&f, &cookie, "/home");
    let agent = &busy
        .get("agents")
        .and_then(Value::as_array)
        .unwrap_or_default()[0];
    assert_eq!(agent.get("needs_you"), Some(&Value::Bool(true)));
    assert_eq!(
        agent.get("unread").and_then(Value::as_int),
        Some(1),
        "the agent's answer after the owner's message"
    );
    assert_eq!(member_text(agent, &["last_message", "from"]), Some("agent"));
    assert!(
        member_text(agent, &["last_message", "text"])
            .is_some_and(|t| t.contains("isn't connected")),
        "{agent:?}"
    );
    let cookie_header = format!("{COOKIE_NAME}={cookie}");
    let again = send(
        f.addr,
        "POST",
        &messages_path(),
        &[
            ("Origin", APP),
            ("X-Mandate-Request", "1"),
            ("Content-Type", "application/json"),
            ("Cookie", &cookie_header),
            ("Idempotency-Key", "k1Z-9_hhhhhhhhhh"),
        ],
        br#"{"text":"and again"}"#,
    );
    assert_eq!(again.status, 202);
    for _ in 0..800 {
        if chat_rows(&f)
            .iter()
            .filter(|r| r.event_type == "ChatRunEnded")
            .count()
            == 2
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let key = mandate_api_server::chat_run::Thread::agent("agt_a", ALICE).key();
    for _ in 0..800 {
        if f.app.chat.running(&key).is_none() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    push(
        &f,
        &ctl(),
        vec![row(
            90,
            "ChatMessagePosted",
            &format!(
                r#"{{"agent_id":"agt_a","author":"owner","card":null,"member":"{ALICE}","proactive":false,"run_id":null,"setup_id":null,"sources":[],"text":"last word"}}"#
            ),
        )],
    );
    let (_, after) = get_json(&f, &cookie, "/home");
    let agent = &after
        .get("agents")
        .and_then(Value::as_array)
        .unwrap_or_default()[0];
    assert_eq!(
        agent.get("unread").and_then(Value::as_int),
        Some(0),
        "nothing after the owner's last message"
    );
    assert_eq!(member_text(agent, &["last_message", "from"]), Some("owner"));
    assert_eq!(
        member_text(agent, &["last_message", "text"]),
        Some("last word")
    );
}

#[test]
fn the_goals_read_is_the_agent_s_latest_or_empty() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let (status, empty) = get_json(&f, &cookie, "/agents/agt_a/goals");
    assert_eq!(status, 200);
    assert_eq!(member_text(&empty, &["agent_id"]), Some("agt_a"));
    assert_eq!(empty.get("goals"), Some(&Value::Array(Vec::new())));
    assert_eq!(empty.get("plan"), Some(&Value::Null));
    assert_eq!(empty.get("watching"), Some(&Value::Array(Vec::new())));
    push(
        &f,
        &ctl(),
        vec![
            row(
                3,
                "AgentGoalsUpdated",
                r#"{"agent_id":"agt_a","goals":[{"status":"done","text":"old"}],"plan":"old plan","run_id":"run_1","watching":[]}"#,
            ),
            row(
                4,
                "AgentGoalsUpdated",
                r#"{"agent_id":"agt_a","goals":[{"status":"active","text":"Hold AAPL"}],"plan":"Wait.","run_id":"run_2","watching":["AAPL"]}"#,
            ),
            row(
                5,
                "AgentGoalsUpdated",
                r#"{"agent_id":"agt_b","goals":[],"plan":"another's","run_id":"run_3","watching":["MSFT"]}"#,
            ),
        ],
    );
    let (_, kept) = get_json(&f, &cookie, "/agents/agt_a/goals");
    assert_eq!(member_text(&kept, &["plan"]), Some("Wait."));
    assert_eq!(
        kept.get("watching"),
        Some(&Value::Array(vec![Value::Str("AAPL".to_owned())]))
    );
    assert_eq!(
        kept.get("goals")
            .and_then(Value::as_array)
            .map(<[Value]>::len),
        Some(1)
    );
    assert_eq!(get_json(&f, &cookie, "/agents/agt_nope/goals").0, 404);
}

#[test]
fn the_models_read_lists_the_offered_chat_models_and_their_prices() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let (status, body) = get_json(&f, &cookie, "/models");
    assert_eq!(status, 200);
    let models = body
        .get("models")
        .and_then(Value::as_array)
        .unwrap_or_default();
    assert_eq!(models.len(), 1);
    assert_eq!(
        member_text(&models[0], &["model_id"]),
        Some("llm.chat_sonnet")
    );
    assert_eq!(
        member_text(&models[0], &["label"]),
        Some("claude-sonnet-5.5")
    );
    assert_eq!(
        member_text(&models[0], &["input_per_million_usd"]),
        Some("3")
    );
    assert_eq!(
        member_text(&models[0], &["output_per_million_usd"]),
        Some("15")
    );
}

#[test]
fn a_settings_write_takes_an_offered_model_and_a_positive_cap_for_a_deployed_agent() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let (status, saved) = put_settings(
        &f,
        &cookie,
        "agt_a",
        br#"{"chat_model":"llm.chat_sonnet","daily_cap_usd":"2.50"}"#,
        "k1Z-9_iiiiiiiiii",
    );
    assert_eq!(status, 200, "{saved:?}");
    assert_eq!(member_text(&saved, &["agent_id"]), Some("agt_a"));
    assert_eq!(
        member_text(&saved, &["chat_model"]),
        Some("llm.chat_sonnet")
    );
    assert_eq!(member_text(&saved, &["daily_cap_usd"]), Some("2.50"));
    let read = parse(&get_settings(&f, &cookie, "agt_a").body).expect("JSON");
    assert_eq!(member_text(&read, &["daily_cap_usd"]), Some("2.50"));
    let rows = chat_rows(&f);
    let changed = rows
        .iter()
        .filter(|r| r.event_type == "ChatSettingsChanged")
        .count();
    let (unoffered, body) = put_settings(
        &f,
        &cookie,
        "agt_a",
        br#"{"chat_model":"llm.other","daily_cap_usd":"2"}"#,
        "k1Z-9_jjjjjjjjjj",
    );
    assert_eq!(unoffered, 422);
    assert!(format!("{body:?}").contains("not_offered"), "{body:?}");
    let (zero, _) = put_settings(
        &f,
        &cookie,
        "agt_a",
        br#"{"chat_model":"llm.chat_sonnet","daily_cap_usd":"0"}"#,
        "k1Z-9_kkkkkkkkkk",
    );
    assert_eq!(zero, 422);
    let (unknown, _) = put_settings(
        &f,
        &cookie,
        "agt_nope",
        br#"{"chat_model":"llm.chat_sonnet","daily_cap_usd":"2"}"#,
        "k1Z-9_llllllllll",
    );
    assert_eq!(unknown, 404);
    assert_eq!(
        chat_rows(&f)
            .iter()
            .filter(|r| r.event_type == "ChatSettingsChanged")
            .count(),
        changed,
        "a refused write journals nothing"
    );
}

#[test]
fn a_thread_serves_each_message_s_author_text_card_sources_and_place() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    push(
        &f,
        &ctl(),
        vec![
            row(
                3,
                "ChatMessagePosted",
                &format!(
                    r#"{{"agent_id":"agt_a","author":"owner","card":null,"member":"{ALICE}","proactive":false,"run_id":null,"setup_id":null,"sources":[],"text":"hi"}}"#
                ),
            ),
            row(
                4,
                "ChatMessagePosted",
                &format!(
                    r#"{{"agent_id":"agt_a","author":"agent","card":{{"kind":"fill"}},"member":"{ALICE}","proactive":true,"run_id":"run_1","setup_id":null,"sources":[{{"label":"Fill","ref":"ev:1"}}],"text":"Bought."}}"#
                ),
            ),
            row(
                5,
                "ChatMessagePosted",
                r#"{"agent_id":"agt_a","author":"owner","card":null,"member":"01JB3K6F0C9R2V7N4M8Q1T5W3Y","proactive":false,"run_id":null,"setup_id":null,"sources":[],"text":"another member's"}"#,
            ),
        ],
    );
    let (status, body) = get_json(&f, &cookie, "/agents/agt_a/messages");
    assert_eq!(status, 200);
    let messages = body
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or_default();
    assert_eq!(messages.len(), 2, "only the member's own thread: {body:?}");
    let owner = &messages[0];
    assert_eq!(member_text(owner, &["from"]), Some("owner"));
    assert_eq!(member_text(owner, &["text"]), Some("hi"));
    assert_eq!(owner.get("seq").and_then(Value::as_int), Some(3));
    assert_eq!(
        member_text(owner, &["id"]),
        Some(event_id_at(&ctl(), 3).as_str())
    );
    assert_eq!(
        member_text(owner, &["at"]),
        Some("2026-10-17T10:00:03.000000000Z")
    );
    assert_eq!(owner.get("proactive"), Some(&Value::Bool(false)));
    let agent = &messages[1];
    assert_eq!(member_text(agent, &["from"]), Some("agent"));
    assert_eq!(member_text(agent, &["card", "kind"]), Some("fill"));
    assert_eq!(agent.get("proactive"), Some(&Value::Bool(true)));
    assert_eq!(
        agent
            .get("sources")
            .and_then(Value::as_array)
            .and_then(|s| s.first())
            .and_then(|s| member_text(s, &["ref"])),
        Some("ev:1")
    );
    assert_eq!(body.get("cursor").and_then(Value::as_int), Some(5));
    assert_eq!(member_text(&body, &["agent_id"]), Some("agt_a"));
    assert_eq!(body.get("setup_id"), Some(&Value::Null));
    let (_, ahead) = get_json(&f, &cookie, "/agents/agt_a/messages?after=99");
    assert_eq!(
        ahead.get("cursor").and_then(Value::as_int),
        Some(99),
        "a cursor past the head stays"
    );
    assert_eq!(ahead.get("messages"), Some(&Value::Array(Vec::new())));
    assert_eq!(
        get_json(&f, &cookie, "/agents/agt_a/messages?after=x").0,
        422
    );
    assert_eq!(
        get_json(&f, &cookie, "/agents/agt_a/messages?before=1").0,
        422
    );
    assert_eq!(get_json(&f, &cookie, "/agents/agt_nope/messages").0, 404);
}

#[test]
fn a_setup_thread_is_read_by_its_own_id() {
    let f = fixture(None);
    let cookie = signed_in(&f);
    let sent = post_setup(&f, &cookie, "?setup_id=setup_read", "setupkey00000000009");
    assert_eq!(sent.status, 202);
    for _ in 0..800 {
        if chat_rows(&f).iter().any(|r| r.event_type == "ChatRunEnded") {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let (status, body) = get_json(&f, &cookie, "/setup/setup_read/messages");
    assert_eq!(status, 200);
    assert_eq!(member_text(&body, &["setup_id"]), Some("setup_read"));
    assert_eq!(body.get("agent_id"), Some(&Value::Null));
    let texts = texts(&body);
    assert_eq!(
        texts.first().map(|t| t.1.as_str()),
        Some("an agent for SPY")
    );
    let (_, other) = get_json(&f, &cookie, "/setup/setup_other/messages");
    assert_eq!(other.get("messages"), Some(&Value::Array(Vec::new())));
}

/// More of `acct_1`'s account stream after [`seed_fill`]: the agent's sale of MSFT, which its
/// mandate does not pin but the catalogue registers, filled late; another agent's order and its
/// fill; and the agent's order of an instrument no one registered, and its fill.
fn seed_more_fills(f: &Fixture) {
    let acct = format!("acct:{WORKSPACE}:acct_1");
    let caused = |seq: u64| format!(r#""causation_id":"{}","#, event_id_at(&acct, seq));
    let unknown = "00000000-0000-4000-8000-00000000abcd";
    push(
        f,
        &acct,
        vec![
            row_on(
                &acct,
                5,
                "OrderRequestRecorded",
                r#"{"agent_id":"agt_a","intent_id":"int_2","purpose":"exit"}"#,
                "",
            ),
            row_on(
                &acct,
                6,
                "OrderSubmitted",
                &format!(
                    r#"{{"client_order_id":"co_2","instrument_id":"{MSFT}","side":"sell","qty":"3","type":"limit","tif":"day","limit_price":"400"}}"#
                ),
                &caused(5),
            ),
            row_on(
                &acct,
                7,
                "LateFillApplied",
                &format!(
                    r#"{{"fill_id":"f_2","client_order_id":"co_2","instrument_id":"{MSFT}","side":"sell","qty_gross":"3","price":"401.5"}}"#
                ),
                "",
            ),
            row_on(
                &acct,
                8,
                "OrderRequestRecorded",
                r#"{"agent_id":"agt_other","intent_id":"int_3","purpose":"entry"}"#,
                "",
            ),
            row_on(
                &acct,
                9,
                "OrderSubmitted",
                &format!(
                    r#"{{"client_order_id":"co_3","instrument_id":"{AAPL}","side":"buy","qty":"1","type":"limit","tif":"day","limit_price":"191"}}"#
                ),
                &caused(8),
            ),
            row_on(
                &acct,
                10,
                "FillApplied",
                &format!(
                    r#"{{"fill_id":"f_3","client_order_id":"co_3","instrument_id":"{AAPL}","side":"buy","qty_gross":"1","price":"190"}}"#
                ),
                "",
            ),
            row_on(
                &acct,
                11,
                "OrderRequestRecorded",
                r#"{"agent_id":"agt_a","intent_id":"int_4","purpose":"entry"}"#,
                "",
            ),
            row_on(
                &acct,
                12,
                "OrderSubmitted",
                &format!(
                    r#"{{"client_order_id":"co_4","instrument_id":"{unknown}","side":"buy","qty":"2","type":"limit","tif":"day","limit_price":"5"}}"#
                ),
                &caused(11),
            ),
            row_on(
                &acct,
                13,
                "FillApplied",
                &format!(
                    r#"{{"fill_id":"f_4","client_order_id":"co_4","instrument_id":"{unknown}","side":"buy","qty_gross":"2","price":"5"}}"#
                ),
                "",
            ),
        ],
    );
}

#[test]
fn every_fill_of_the_agent_s_own_orders_is_said_in_its_words_and_no_other_agent_s() {
    let f = fixture(None);
    let cookie = alice_writes(&f);
    seed_fill(&f);
    seed_catalogue(&f);
    seed_more_fills(&f);
    assert_eq!(pass(&f), 3);
    assert_eq!(pass(&f), 0);
    let said: Vec<String> = texts(&thread(&f, &cookie))
        .into_iter()
        .filter(|(who, _)| who == "agent")
        .map(|(_, t)| t)
        .skip(1)
        .collect();
    assert_eq!(
        said,
        [
            "Bought 10 AAPL at 190.12.",
            "Sold 3 MSFT at 401.5.",
            "Bought 2 00000000-0000-4000-8000-00000000abcd at 5.",
        ]
    );
    let cards: Vec<Value> = thread(&f, &cookie)
        .get("messages")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|m| m.get("card").cloned())
        .filter(|c| c != &Value::Null)
        .collect();
    assert_eq!(
        cards
            .get(1)
            .map(|c| String::from_utf8_lossy(&to_canonical(c)).into_owned()),
        Some(
            r#"{"kind":"fill","price":"401.5","qty":"3","side":"sell","symbol":"MSFT"}"#.to_owned()
        )
    );
}

#[test]
fn a_stopped_agent_says_nothing_first() {
    let f = fixture(None);
    alice_writes(&f);
    seed_fill(&f);
    push(
        &f,
        &ctl(),
        vec![row(20, "AgentStopped", r#"{"agent_id":"agt_a"}"#)],
    );
    assert_eq!(pass(&f), 0);
}

#[test]
fn a_member_hears_only_from_agents_they_wrote_to_and_another_member_s_message_is_theirs() {
    let f = fixture(None);
    seed_fill(&f);
    push(
        &f,
        &ctl(),
        vec![row(
            20,
            "ChatMessagePosted",
            r#"{"agent_id":"agt_b","author":"owner","card":null,"member":"01JB3K6F0C9R2V7N4M8Q1T5W3Y","proactive":false,"run_id":null,"setup_id":null,"sources":[],"text":"to another agent"}"#,
        )],
    );
    assert_eq!(
        pass(&f),
        0,
        "no member wrote to agt_a, and a row with no user actor is no member's"
    );
}

/// The daily note's pass at `at`, with a gateway or none, and the mode the agent's stream last records, if any.
fn brief_passes(at: i64, gateway: bool, mode: Option<&str>) -> usize {
    let script = vec![
        turn(&[(name::REPLY, r#"{"sources":[],"text":"Hello."}"#)], None),
        turn(&[], Some(r#"{"watch":[]}"#)),
        turn(&[(name::REPLY, r#"{"sources":[],"text":"A note."}"#)], None),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ];
    let f = fixture(gateway.then(|| gateway_service(script)));
    alice_writes(&f);
    if let Some(mode) = mode {
        let stream = format!("agent:{WORKSPACE}:agt_a");
        push(
            &f,
            &stream,
            vec![
                row_on(&stream, 1, "StreamOpened", "{}", ""),
                row_on(
                    &stream,
                    2,
                    "AgentModeChanged",
                    &format!(r#"{{"from":"active","to":"{mode}"}}"#),
                    "",
                ),
            ],
        );
    }
    f.clock.set(at);
    pass(&f)
}

#[test]
fn the_daily_note_waits_for_a_weekday_s_close_a_model_and_an_active_agent() {
    let day = T0.div_euclid(86_400);
    let weekday = |offset: i64| (day + offset).saturating_add(3).rem_euclid(7);
    let monday = (0..7).find(|o| weekday(*o) == 0).unwrap_or_default();
    let at = |offset: i64, secs: i64| (day + monday + offset) * 86_400 + secs;
    let close = 21 * 3_600;
    assert_eq!(
        brief_passes(at(0, close + 60), true, None),
        1,
        "Monday after the close"
    );
    assert_eq!(
        brief_passes(at(4, close), true, None),
        1,
        "Friday at the close"
    );
    assert_eq!(
        brief_passes(at(0, close - 60), true, None),
        0,
        "before the close"
    );
    assert_eq!(brief_passes(at(5, close + 60), true, None), 0, "Saturday");
    assert_eq!(brief_passes(at(6, close + 60), true, None), 0, "Sunday");
    assert_eq!(
        brief_passes(at(0, close + 60), false, None),
        0,
        "no model service"
    );
    assert_eq!(
        brief_passes(at(0, close + 60), true, Some("paused")),
        0,
        "a paused agent"
    );
    assert_eq!(
        brief_passes(at(0, close + 60), true, Some("active")),
        1,
        "an agent made active again"
    );
}

/// `text` as a JSON string's members, escaped for a tool call's arguments.
fn json_text(text: &str) -> String {
    String::from_utf8_lossy(&to_canonical(&Value::Str(text.to_owned()))).into_owned()
}

#[test]
fn a_setup_run_offers_the_template_and_the_models_and_turns_a_valid_draft_into_a_card() {
    const CONTENT: &str = r#"{"authorship":"platform","code":[{"path":"crates/mandate-modelhost/src/ma_crossover.rs","sha256":"3434343434343434343434343434343434343434343434343434343434343434"}],"content_version":1,"inputs":{"bars":"daily_close"},"kind":"quant_model_content","methodology":"Long when the fast mean is above the slow mean.","model_id":"quant.ma_crossover","model_version":"1.0.0","output":{"flat":"none"},"params_schema":{"constraints":["fast_periods < slow_periods"],"params":[{"max":1000,"min":1,"name":"fast_periods","type":"integer"},{"max":1000,"min":2,"name":"slow_periods","type":"integer"}]}}"#;
    let content = format!(
        "sha256:{}",
        Digest::of(&to_canonical(&parse(CONTENT.as_bytes()).expect("JSON"))).to_hex()
    );
    let draft = |document: &str| format!(r#"{{"document":{}}}"#, json_text(document));
    let pinned = String::from_utf8_lossy(&to_canonical(
        &parse(PINNED_DOCUMENT.as_bytes()).expect("JSON"),
    ))
    .replace(AAPL, MSFT)
    .replace(r#""symbol":"AAPL""#, r#""symbol":"MSFT""#)
    .replace(
        "sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc",
        &content,
    );
    let gateway = gateway_service(vec![
        turn(
            &[
                (name::MODELS, "{}"),
                (name::TEMPLATE, "{}"),
                (name::DRAFT, &draft("not json")),
                (name::DRAFT, &draft(r#"{"name":"half"}"#)),
                (name::DRAFT, &draft(&pinned)),
            ],
            None,
        ),
        turn(&[], Some(r#"{"watch":[]}"#)),
    ]);
    let f = fixture(Some(gateway));
    seed_fill(&f);
    seed_catalogue(&f);
    assert_eq!(store(&f, CONTENT), content);
    push(
        &f,
        &ctl(),
        vec![row(
            12,
            "ConfigSnapshotRegistered",
            &format!(
                r#"{{"admits_instruments":false,"content_hash":"{content}","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}}"#
            ),
        )],
    );
    let cookie = signed_in(&f);
    let sent = post_setup(&f, &cookie, "?setup_id=setup_draft", "setupkey00000000011");
    assert_eq!(sent.status, 202, "{}", String::from_utf8_lossy(&sent.body));
    for _ in 0..800 {
        if chat_rows(&f).iter().any(|r| r.event_type == "ChatRunEnded") {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let found = results(&f);
    let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(
        names,
        [
            name::MODELS,
            name::TEMPLATE,
            name::DRAFT,
            name::DRAFT,
            name::DRAFT
        ]
    );
    assert!(
        found[0]
            .1
            .starts_with(r#"{"models":[{"admits_instruments":false,"#),
        "{}",
        found[0].1
    );
    assert!(
        found[0].1.contains(r#""model_id":"quant.ma_crossover""#),
        "{}",
        found[0].1
    );
    assert!(
        found[1].1.starts_with(r#"{"document":{"autonomy":"#),
        "{}",
        found[1].1
    );
    assert!(
        found[1].1.contains(r#""name":"example-agent""#),
        "{}",
        found[1].1
    );
    assert!(found[1].1.ends_with(r#""ok":true}"#), "{}", found[1].1);
    assert_eq!(found[2].1, r#"{"ok":false,"refused":"document_not_json"}"#);
    assert!(
        found[3].1.contains(r#""refused":"invalid_mandate""#),
        "{}",
        found[3].1
    );
    assert!(found[3].1.contains(r#""problems":[{"#), "{}", found[3].1);
    assert!(
        found[4].1.starts_with(r#"{"message_id":""#),
        "{}",
        found[4].1
    );
    assert!(
        found[4]
            .1
            .ends_with("\nThe draft is valid; the owner now sees a confirmation card."),
        "{}",
        found[4].1
    );
    let (_, body) = get_json(&f, &cookie, "/setup/setup_draft/messages");
    let card = body
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|m| m.last())
        .and_then(|m| m.get("card").cloned())
        .unwrap_or(Value::Null);
    assert_eq!(member_text(&card, &["kind"]), Some("confirm_mandate"));
    assert!(
        member_text(&card, &["version"]).is_some_and(|v| v.starts_with("sha256:")),
        "{card:?}"
    );
    assert!(
        card.get("summary").is_some_and(|s| s != &Value::Null),
        "{card:?}"
    );
}

#[test]
fn a_proactive_run_proposes_an_order_as_a_card_and_places_nothing() {
    let gateway = gateway_service(vec![
        turn(&[(name::REPLY, r#"{"sources":[],"text":"Hello."}"#)], None),
        turn(&[], Some(r#"{"watch":[]}"#)),
        turn(
            &[(
                name::PROPOSE,
                r#"{"quantity":"3","reason":"It dipped below its average.","side":"buy","symbol":"aapl"}"#,
            )],
            None,
        ),
        turn(&[], Some(r#"{"watch":["AAPL"]}"#)),
    ]);
    let f = fixture(Some(gateway));
    alice_writes(&f);
    let monday_close = (T0.div_euclid(86_400) + 2) * 86_400 + 21 * 3_600 + 60;
    f.clock.set(monday_close);
    let cookie = signed_in(&f);
    let before: Vec<String> = f
        .journal
        .rows_of(&ctl())
        .iter()
        .map(|r| r.event_type.clone())
        .collect();
    assert_eq!(pass(&f), 1);
    for _ in 0..800 {
        if chat_rows(&f)
            .iter()
            .filter(|r| r.event_type == "ChatRunEnded")
            .count()
            == 2
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    let found = results(&f);
    let proposed = results_of(&found, name::PROPOSE);
    assert_eq!(proposed.len(), 1, "{found:?}");
    assert!(
        proposed[0].starts_with(r#"{"message_id":""#),
        "{}",
        proposed[0]
    );
    let body = thread(&f, &cookie);
    let last = body
        .get("messages")
        .and_then(Value::as_array)
        .and_then(|m| m.last().cloned())
        .unwrap_or(Value::Null);
    assert_eq!(last.get("text"), Some(&Value::Null));
    assert_eq!(last.get("proactive"), Some(&Value::Bool(true)));
    assert_eq!(
        last.get("card").map(|c| String::from_utf8_lossy(&to_canonical(c)).into_owned()),
        Some(r#"{"kind":"proposal","qty":"3","reason":"It dipped below its average.","side":"buy","symbol":"AAPL"}"#.to_owned())
    );
    let after: Vec<String> = f
        .journal
        .rows_of(&ctl())
        .iter()
        .map(|r| r.event_type.clone())
        .collect();
    assert!(
        after[before.len()..].iter().all(|t| t.starts_with("Chat")),
        "a proactive run writes nothing but its own records: {:?}",
        &after[before.len()..]
    );
    let kinds: Vec<String> = chat_rows(&f)
        .iter()
        .filter(|r| r.event_type == "ChatTurnRecorded")
        .filter_map(|r| parse(&r.body).ok())
        .filter_map(|e| {
            e.get("payload")
                .and_then(|p| p.get("kind"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(kinds, ["agent", "agent", "proactive", "proactive"]);
}
