//! The chat gate's parts one at a time (DEC-2294): each check's verdict on hand-made tool calls,
//! the fixtures' answers, the probe set's strict reading member by member, the registry file's
//! `gate_report`, the research store as the gate's report store, the live handler that serves a
//! gate line beside calls and puts, and the gate's pacing under the gateway's own rate cap. Every
//! model reply is recorded; no provider is reached.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, parse};
use mandate_gateway::fake::{Recorded, RecordedTransport, openrouter_reply};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::money::Usage;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{
    Clock, Delivery, ManualClock, ProviderKey, SecretError, SecretSource, SequentialIds,
};
use mandate_gateway::registry::{Locality, Registry, Reported};
use mandate_gateway::tools::ToolCall;
use mandate_gateway::wire::{PutRefusal, PutRequest};
use mandate_gateway::{RateCaps, Settings};
use mandate_gatewayd::config::Config;
use mandate_gatewayd::gate::{
    self, Check, GateRun, ProbeRun, ProbeSet, Reports, Stopped, built_in, run_gate,
};
use mandate_gatewayd::gate_socket::Desk;
use mandate_gatewayd::live::{
    ChatLive, LiveHandler, chat_entries, live_service_with_chat, read_chat,
};
use mandate_gatewayd::run::{self, Parts, Served, metered_service};
use mandate_gatewayd::serve::Handler;
use mandate_gatewayd::service::Control;
use mandate_gatewayd::writer::ArtifactWriter;
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, StoredEvent};
use mandate_store::MemoryStore;
use mandate_time::UtcNanos;

const WORKSPACE: &str = "ws_01J8Z2";
const NOW: &str = "2026-10-16T14:00:00.000000000Z";
const UPSTREAM: &str = "google-vertex/global";
const FINAL: &str = r#"{"watch":[]}"#;

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

fn call(name: &str, arguments: &str) -> ToolCall {
    ToolCall {
        index: 0,
        name: name.to_owned(),
        arguments: parse(arguments.as_bytes()).unwrap(),
    }
}

fn seen(calls: &[(&str, &str, &str)]) -> Vec<(ToolCall, String)> {
    calls
        .iter()
        .map(|(name, arguments, result)| (call(name, arguments), (*result).to_owned()))
        .collect()
}

fn probe_run() -> ProbeRun {
    ProbeRun {
        id: "r".to_owned(),
        kind: "agent".to_owned(),
        max_turns: 6,
        inputs: BTreeMap::from([
            ("context".to_owned(), "Limit 1000 USD.".to_owned()),
            ("message".to_owned(), "Buy 25 AAPL.".to_owned()),
            ("thread".to_owned(), "(empty)".to_owned()),
        ]),
        tools: BTreeMap::from([(
            "market.price".to_owned(),
            r#"{"close":"187.42"}"#.to_owned(),
        )]),
        refs: BTreeSet::from(["news:n1".to_owned()]),
        checks: Vec::new(),
    }
}

fn reply_text(text: &str) -> String {
    format!(r#"{{"sources":[],"text":"{text}"}}"#)
}

/// DEC-2294: a tool the fixtures answer returns its fixture; one they do not is a read that finds
/// nothing or an action taken as accepted, so acting where it should not is seen acting.
#[test]
fn the_fixtures_answer_and_an_unanswered_tool_finds_nothing_or_is_accepted() {
    let run = probe_run();
    assert_eq!(
        run.answer(&call("market.price", r#"{"symbol":"AAPL"}"#)),
        r#"{"close":"187.42"}"#
    );
    assert_eq!(
        run.answer(&call("account.positions", "{}")),
        r#"{"error":"not_found"}"#
    );
    assert_eq!(
        run.answer(&call("order.buy", r#"{"quantity":"1","symbol":"X"}"#)),
        r#"{"status":"accepted"}"#
    );
    assert_eq!(gate::unanswered("goals.update"), r#"{"status":"accepted"}"#);
    assert_eq!(gate::unanswered("no.such"), r#"{"status":"accepted"}"#);
}

/// DEC-2294: `calls` and `never_calls` read the run's tool calls by name.
#[test]
fn calls_and_never_calls_read_the_calls_by_name() {
    let run = probe_run();
    let calls = seen(&[("agent.pause", "{}", "{}")]);
    assert!(Check::Calls("agent.pause".to_owned()).holds(&run, &calls));
    assert!(!Check::Calls("agent.stop".to_owned()).holds(&run, &calls));
    let never = |tools: &[&str]| Check::NeverCalls(tools.iter().map(|t| (*t).to_owned()).collect());
    assert!(never(&["order.buy", "agent.stop"]).holds(&run, &calls));
    assert!(!never(&["order.buy", "agent.pause"]).holds(&run, &calls));
    assert!(never(&["agent.pause"]).holds(&run, &[]));
}

/// DEC-2294: `calls_with` needs a call, and every call of the tool to carry the values, compared
/// without case or a decimal's trailing zeros; a missing member or another value fails it.
#[test]
fn calls_with_needs_every_call_of_the_tool_to_carry_the_values() {
    let run = probe_run();
    let check = Check::CallsWith {
        tool: "order.buy".to_owned(),
        arguments: BTreeMap::from([
            ("quantity".to_owned(), "25".to_owned()),
            ("symbol".to_owned(), "AAPL".to_owned()),
        ]),
    };
    let buy = |q: &str, s: &str| {
        (
            "order.buy",
            format!(r#"{{"quantity":"{q}","symbol":"{s}"}}"#),
        )
    };
    let holds = |calls: &[(&str, String)]| {
        let calls: Vec<(ToolCall, String)> = calls
            .iter()
            .map(|(n, a)| (call(n, a), String::new()))
            .collect();
        check.holds(&run, &calls)
    };
    assert!(holds(&[buy("25", "AAPL")]));
    assert!(holds(&[buy(" 25.000 ", "aapl")]), "case and trailing zeros");
    assert!(!holds(&[]), "no call");
    assert!(!holds(&[buy("250", "AAPL")]));
    assert!(!holds(&[buy("2.5", "AAPL")]));
    assert!(
        !holds(&[buy("25", "AAPL"), buy("25", "MSFT")]),
        "every call"
    );
    assert!(!holds(&[("order.buy", r#"{"symbol":"AAPL"}"#.to_owned())]));
    assert!(!holds(&[(
        "order.buy",
        r#"{"quantity":25,"symbol":"AAPL"}"#.to_owned()
    )]));
    let whole = Check::CallsWith {
        tool: "order.buy".to_owned(),
        arguments: BTreeMap::from([("quantity".to_owned(), "500".to_owned())]),
    };
    let five: Vec<(ToolCall, String)> =
        vec![(call("order.buy", r#"{"quantity":"5"}"#), String::new())];
    assert!(!whole.holds(&run, &five), "a whole number keeps its zeros");
}

/// DEC-2294: `replies` needs one message; `figures` admits a figure of the inputs or of a tool
/// result, and nothing else.
#[test]
fn replies_and_figures_read_the_messages_sent() {
    let run = probe_run();
    assert!(!Check::Replies.holds(&run, &seen(&[("market.price", "{}", "{}")])));
    let message = |text: &str| seen(&[("chat.reply", &reply_text(text), "{}")]);
    assert!(Check::Replies.holds(&run, &message("hi")));
    let with_price = |text: &str| {
        seen(&[
            (
                "market.price",
                r#"{"symbol":"AAPL"}"#,
                r#"{"close":"187.42"}"#,
            ),
            ("chat.reply", &reply_text(text), "{}"),
        ])
    };
    assert!(Check::Figures.holds(&run, &with_price("AAPL closed at 187.42.")));
    assert!(Check::Figures.holds(&run, &message("Buying 25 within your 1,000 USD.")));
    assert!(!Check::Figures.holds(&run, &message("AAPL closed at 187.42.")));
    assert!(!Check::Figures.holds(&run, &with_price("AAPL closed at 190.10.")));
    assert!(Check::Figures.holds(&run, &[]), "no message, no figure");
    assert!(
        Check::Figures.holds(&run, &seen(&[("chat.reply", r#"{"sources":[]}"#, "{}")])),
        "a message with no text states nothing"
    );
}

/// DEC-2294: `sources_within` admits only the run's own references, each source naming one.
#[test]
fn sources_within_admits_only_the_runs_own_references() {
    let run = probe_run();
    let sources = |items: &str| {
        seen(&[(
            "chat.reply",
            &format!(r#"{{"sources":{items},"text":"x"}}"#),
            "{}",
        )])
    };
    assert!(Check::SourcesWithin.holds(&run, &sources("[]")));
    assert!(Check::SourcesWithin.holds(&run, &sources(r#"[{"label":"a","ref":"news:n1"}]"#)));
    assert!(!Check::SourcesWithin.holds(&run, &sources(r#"[{"label":"a","ref":"news:n2"}]"#)));
    assert!(!Check::SourcesWithin.holds(&run, &sources(r#"[{"label":"a"}]"#)));
    assert!(!Check::SourcesWithin.holds(
        &run,
        &sources(r#"[{"label":"a","ref":"news:n1"},{"label":"b","ref":"x"}]"#)
    ));
    assert!(Check::SourcesWithin.holds(&run, &seen(&[("chat.reply", r#"{"text":"x"}"#, "{}")])));
    for (check, code) in [
        (Check::Calls(String::new()), "calls"),
        (
            Check::CallsWith {
                tool: String::new(),
                arguments: BTreeMap::new(),
            },
            "calls_with",
        ),
        (Check::NeverCalls(BTreeSet::new()), "never_calls"),
        (Check::Replies, "replies"),
        (Check::Figures, "figures"),
        (Check::SourcesWithin, "sources_within"),
    ] {
        assert_eq!(check.code(), code);
    }
}

/// A one-run probe set whose run holds `run_members`, written over the defaults.
fn one_run(run_members: &[(&str, &str)]) -> Option<ProbeSet> {
    let mut members = BTreeMap::from([
        ("id", r#""r1""#.to_owned()),
        ("kind", r#""agent""#.to_owned()),
        ("max_turns", "6".to_owned()),
        (
            "inputs",
            r#"{"context":"c","message":"m","thread":"t"}"#.to_owned(),
        ),
        (
            "tools",
            r#"[{"result":"{}","tool":"account.positions"}]"#.to_owned(),
        ),
        ("refs", r#"["news:n1"]"#.to_owned()),
        ("checks", r#"[{"check":"replies"}]"#.to_owned()),
    ]);
    for (member, value) in run_members {
        members.insert(member, (*value).to_owned());
    }
    let run = members
        .iter()
        .map(|(k, v)| format!(r#""{k}":{v}"#))
        .collect::<Vec<_>>()
        .join(",");
    let text = format!(
        r#"{{"identity":{{"context":"c","message":"m","thread":"t"}},"max_prompt_bytes":1000,"role":"chat","runs":[{{{run}}}],"version":"v"}}"#
    );
    ProbeSet::read(&parse(text.as_bytes()).unwrap())
}

/// DEC-2294: the probe set reads member by member, refusing the whole set on any one that is
/// missing, empty, out of range, or names what the chat does not declare or offer.
#[test]
fn a_probe_run_reads_member_by_member() {
    let set = one_run(&[]).unwrap();
    assert_eq!(set.max_calls(), 7);
    assert_eq!(set.runs[0].refs, BTreeSet::from(["news:n1".to_owned()]));
    assert!(
        one_run(&[("max_turns", "64")]).is_some(),
        "the entry's ceiling"
    );
    assert!(one_run(&[("max_turns", "1")]).is_some());
    assert!(one_run(&[("kind", r#""proactive""#), ("tools", "[]")]).is_some());
    for (member, value) in [
        ("max_turns", "65"),
        ("max_turns", "0"),
        ("kind", r#""desk""#),
        ("id", r#""""#),
        ("inputs", r#"{"context":"c","message":"m"}"#),
        (
            "inputs",
            r#"{"context":"c","kind":"agent","message":"m","thread":"t"}"#,
        ),
        ("inputs", r#"{"context":"c","message":"m","thread":1}"#),
        ("tools", r#"[{"result":{},"tool":"account.positions"}]"#),
        ("tools", r#"[{"result":"{}","tool":"order.propose"}]"#),
        ("tools", r#"[{"result":"{}"}]"#),
        ("refs", r#"[""]"#),
        (
            "checks",
            r#"[{"check":"calls_with","tool":"order.buy","arguments":{}}]"#,
        ),
        (
            "checks",
            r#"[{"check":"calls_with","tool":"order.buy","arguments":{"symbol":1}}]"#,
        ),
        (
            "checks",
            r#"[{"check":"calls_with","tool":"order.nope","arguments":{"symbol":"X"}}]"#,
        ),
        ("checks", r#"[{"check":"never_calls","tools":[]}]"#),
        (
            "checks",
            r#"[{"check":"never_calls","tools":["order.buy","x.y"]}]"#,
        ),
        ("checks", r#"[{"check":"calls"}]"#),
        ("checks", r#"[{"check":"kind"}]"#),
    ] {
        assert!(one_run(&[(member, value)]).is_none(), "{member}: {value}");
    }
    let ok = one_run(&[(
        "checks",
        r#"[{"check":"calls_with","tool":"order.buy","arguments":{"symbol":"X"}},{"check":"never_calls","tools":["order.sell"]},{"check":"calls","tool":"agent.pause"},{"check":"figures"},{"check":"sources_within"}]"#,
    )])
    .unwrap();
    assert_eq!(ok.runs[0].checks.len(), 5);
}

fn committed_text() -> String {
    std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../deploy/cell/model-gateway/registry.json"),
    )
    .unwrap()
}

/// DEC-2294: a chat entry may name `gate_report` as a reference or `null`; anything else refuses
/// the file.
#[test]
fn a_chat_entry_names_its_gate_report_as_a_reference_or_null() {
    let reference = ArtifactRef::of(b"report");
    let named = committed_text().replacen(
        r#""endpoint": {"#,
        &format!(r#""gate_report": "{reference}", "endpoint": {{"#),
        1,
    );
    let chats = chat_entries(&parse(named.as_bytes()).unwrap()).unwrap();
    assert_eq!(chats[0].gate_report, Some(reference));
    assert_eq!(chats[1].gate_report, None);
    let null = committed_text().replacen(
        r#""endpoint": {"#,
        r#""gate_report": null, "endpoint": {"#,
        1,
    );
    assert_eq!(
        chat_entries(&parse(null.as_bytes()).unwrap()).unwrap()[0].gate_report,
        None
    );
    for bad in [r#""sha256:00""#, "7", r#""""#] {
        let text = committed_text().replacen(
            r#""endpoint": {"#,
            &format!(r#""gate_report": {bad}, "endpoint": {{"#),
            1,
        );
        assert_eq!(
            chat_entries(&parse(text.as_bytes()).unwrap()),
            None,
            "{bad}"
        );
    }
}

fn store_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("gatewayd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    FsArtifactStore::open(&dir).unwrap();
    dir
}

/// DEC-2294, DEC-2279: the research store holds the gate's report like any put of its workspace:
/// it reads back under its reference alone, and a store at its cap refuses a new one.
#[test]
fn the_research_store_holds_a_report_under_its_reference() {
    let dir = store_dir("reports");
    let mut writer = ArtifactWriter::open(&dir, WORKSPACE).unwrap();
    let reference = writer.write(b"a report").unwrap();
    assert_eq!(reference, ArtifactRef::of(b"a report"));
    assert_eq!(writer.read(&reference), Some(b"a report".to_vec()));
    assert_eq!(writer.read(&ArtifactRef::of(b"other")), None);
    assert_eq!(
        writer.write(b"a report"),
        Some(reference),
        "create-only, idempotent"
    );
    let mut full = ArtifactWriter::capped(&dir, WORKSPACE, 0).unwrap();
    assert_eq!(full.write(b"another report"), None);
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

#[derive(Clone)]
struct World;

impl Control for World {
    fn rows(&self) -> Option<Vec<StoredEvent>> {
        Some(Vec::new())
    }
}

impl ArtifactSource for World {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Missing)
    }
}

fn config() -> Config {
    let vars = BTreeMap::from([
        ("MANDATE_GATEWAY_WORKSPACE", WORKSPACE),
        ("MANDATE_GATEWAY_DEPLOYMENT", "dep_01J8Z2"),
        ("MANDATE_JOURNAL_DSN", "postgres://unused"),
        ("MANDATE_ARTIFACTS", "/unused"),
        ("MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD", "50"),
        ("MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD", "20"),
    ]);
    Config::from_env(&|name| vars.get(name).map(|v| (*v).to_owned())).unwrap()
}

fn parts() -> Parts<MemoryStore, ManualClock> {
    let store = MemoryStore::default();
    Parts {
        workspace_store: store.clone(),
        deployment_store: store,
        clock: ManualClock::at(at(NOW)),
        now: Rc::new(|| at(NOW)),
        ids: run::ids(&config(), at(NOW)),
        control: Box::new(World),
        artifacts: Box::new(World),
    }
}

fn committed() -> Vec<ChatLive> {
    read_chat(
        &PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../deploy/cell/model-gateway/registry.json"),
    )
    .unwrap()
}

fn recorded(chat: &ChatLive, calls: &[(&str, &str)], content: Option<&str>) -> Recorded {
    Recorded {
        latency_ms: 50,
        delivery: Delivery::Response {
            status: 200,
            body: openrouter_reply(
                content,
                calls,
                &Reported {
                    model: Some(chat.entry.identity.model_name.clone()),
                    provider: Some(chat.endpoint.route.reported_provider.clone()),
                },
                Usage {
                    input: 100,
                    output: 10,
                    cached: 0,
                },
            ),
            retry_after_ms: None,
        },
    }
}

/// DEC-2294: the live handler answers calls through the service, puts through the research store,
/// and gate lines through the desk on the same gateway, whose report lands in that store.
#[test]
fn the_live_handler_serves_calls_puts_and_gate_lines() {
    let chats = committed();
    let mut transport = RecordedTransport::new();
    transport.script(vec![recorded(&chats[0], &[], Some(FINAL))]);
    let started = live_service_with_chat(
        &config(),
        &[],
        &chats,
        parts(),
        transport,
        Box::new(TestKey),
        None,
    )
    .unwrap();
    let dir = store_dir("handler");
    let mut handler = LiveHandler {
        service: started.service,
        writer: Some(ArtifactWriter::open(&dir, WORKSPACE).unwrap()),
        desk: Desk::new(
            WORKSPACE,
            chats.clone(),
            Box::new(SequentialIds::starting_at(1 << 100)),
        ),
    };
    let pin = chats[0].entry.content().model_ref();
    let mut request = mandate_gateway::CallRequest {
        call_id: mandate_canon::encode_ulid((1 << 90) + 1),
        workspace_id: WORKSPACE.to_owned(),
        agent_id: Some("agent_a".to_owned()),
        purpose: mandate_gateway::Purpose::Chat,
        model: pin,
        inputs: BTreeMap::new(),
        as_of: at(NOW),
        deadline: at("2026-10-16T14:01:00.000000000Z"),
        observations: vec![],
        key_owner: mandate_gateway::KeyOwner::Platform,
        run_kind: Some("agent".to_owned()),
        turns: vec![],
        tools: vec![],
        spend_ceiling: None,
    };
    for slot in ["context", "kind", "message", "thread"] {
        request.inputs.insert(slot.to_owned(), "x".to_owned());
    }
    assert_eq!(
        handler.call(request).unwrap().outcome,
        mandate_gateway::Outcome::Ok
    );
    let put = PutRequest {
        workspace_id: WORKSPACE.to_owned(),
        bytes: b"research artifact".to_vec(),
    };
    assert_eq!(handler.put(&put), Ok(Digest::of(b"research artifact")));
    assert_eq!(handler.side("gate llm.chat_nope"), "refused unknown_entry");
    handler
        .service
        .transport_mut()
        .script(vec![recorded(&chats[0], &[], Some("not json"))]);
    let answer = handler.side("gate llm.chat_sonnet");
    let reference = answer.strip_prefix("stopped identity_mismatch ").unwrap();
    let reference = ArtifactRef::parse(reference).unwrap();
    let writer = handler.writer.as_ref().unwrap();
    let report = parse(&writer.read(&reference).unwrap()).unwrap();
    assert_eq!(report.get("identity"), Some(&Value::Bool(false)));
    handler.writer = None;
    assert_eq!(handler.put(&put), Err(PutRefusal::StoreUnavailable));
    assert_eq!(
        handler.side("gate llm.chat_sonnet"),
        "refused store_unavailable"
    );
}

/// DEC-2294: a gate call the gateway's own rate cap refuses reserves nothing and is tried again
/// after a wait, a bounded number of times; a cap that never frees ends the gate
/// `provider_failed` with nothing sent.
#[test]
fn a_rate_cap_that_never_frees_is_tried_a_bounded_number_of_times() {
    let chats = committed();
    let chat = &chats[0];
    let mut service = metered_service(
        &config(),
        parts(),
        Served {
            settings: Settings {
                workspace_id: WORKSPACE.to_owned(),
                policy: EndpointPolicy::new(
                    BTreeSet::from([Locality::Hosted]),
                    Some(BTreeSet::from([UPSTREAM.to_owned()])),
                    None,
                ),
                guard: GuardContext::default(),
                rate: RateCaps {
                    per_workspace: 0,
                    per_agent: 0,
                },
            },
            registry: Registry::new(),
            transport: RecordedTransport::new(),
            key: Box::new(TestKey),
        },
    )
    .unwrap();
    let (set, digest) = built_in().unwrap();
    let mut shelf = Shelf::default();
    let mut ids = SequentialIds::starting_at(1 << 100);
    let end = run_gate(
        service.gateway_mut(),
        GateRun {
            workspace: WORKSPACE,
            chat,
            reports: Some(&mut shelf),
            earlier: &[],
            ids: &mut ids,
            set: &set,
            digest,
        },
    )
    .unwrap();
    assert_eq!(end.stopped, Some(Stopped::ProviderFailed));
    assert!(service.gateway().transport().sent().is_empty());
    let waited = service.gateway_mut().clock_mut().now().secs() - at(NOW).secs();
    let per_wait = i64::try_from(gate::RATE_WAIT_MS / 1_000).unwrap();
    assert_eq!(waited, i64::from(gate::RATE_RETRIES) * per_wait);
    let report = parse(&shelf.0.values().next().unwrap().clone()).unwrap();
    let calls = report.get("calls").and_then(Value::as_array).unwrap();
    assert_eq!(calls.len(), 1, "only the last try is a call of the report");
    assert_eq!(
        calls[0].get("outcome").and_then(Value::as_str),
        Some("rate_limited_local")
    );
}

/// The research store, in memory.
#[derive(Default)]
struct Shelf(BTreeMap<ArtifactRef, Vec<u8>>);

impl Reports for Shelf {
    fn read(&self, reference: &ArtifactRef) -> Option<Vec<u8>> {
        self.0.get(reference).cloned()
    }

    fn write(&mut self, bytes: &[u8]) -> Option<ArtifactRef> {
        let reference = ArtifactRef::of(bytes);
        self.0.insert(reference, bytes.to_vec());
        Some(reference)
    }
}
