//! The chat entry pins and registers, each run kind offers its own tools, and a run goes through
//! the real gateway over a recorded OpenRouter fake: every model call is handed over before its
//! tools run, every result before the next call reads it, and the run ends on the model's summary,
//! the owner's Stop, or the spending cap (DEC-2293 items 5 to 7).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, encode_ulid};
use mandate_chat::entry::{self, AGENT, PROACTIVE, SETUP, name};
use mandate_chat::{ChatEntry, End, Halt, Host, Model, RunSpec, Toolbox, menu, run};
use mandate_gateway::fake::{Recorded, RecordedTransport, openrouter_reply};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::meter::{Caps, DeploymentMeter, MemoryMeter, Meter};
use mandate_gateway::money::{PriceTable, TokenPrice, Usage, Usd6};
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{
    Delivery, ManualClock, ProviderKey, SecretError, SecretSource, SequentialIds,
};
use mandate_gateway::registry::{
    Endpoint, Identity, Locality, ModelRef, Registry, Reported, Route, Snapshot, Status,
};
use mandate_gateway::tools::ToolCall;
use mandate_gateway::wire::Reply;
use mandate_gateway::{CallRecord, CallRequest, Gateway, KeyOwner, RateCaps, Settings};
use mandate_time::UtcNanos;

const WORKSPACE: &str = "ws_01J8Z2";
const AGENT_ID: &str = "agent_a";
const SLUG: &str = "anthropic/claude-sonnet-5.5";
const UPSTREAM: &str = "anthropic";
const PROVIDER: &str = "Anthropic";

type TestGateway = Gateway<RecordedTransport, MemoryMeter, ManualClock>;

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

fn gateway() -> (TestGateway, ModelRef) {
    let mut registry = Registry::new();
    let content = chat_entry().content();
    let model = content.model_ref();
    registry.register(content).unwrap();
    registry.set_status(&model, Status::Offered).unwrap();
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
            TokenPrice::per_million(Usd6::parse("3").unwrap()),
            TokenPrice::per_million(Usd6::parse("15").unwrap()),
            TokenPrice::per_million(Usd6::parse("0.3").unwrap()),
            Digest::of(b"chat price table"),
        )
        .unwrap(),
    };
    registry
        .add_endpoint(&model, endpoint, &[reported()])
        .unwrap();
    let settings = Settings {
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
    };
    let gateway = Gateway::new(
        settings,
        registry,
        Box::new(OpenRouter),
        RecordedTransport::new(),
        Box::new(TestKey),
        Meter::new(
            MemoryMeter::new(),
            Caps::new(Usd6::parse("50").unwrap()),
            DeploymentMeter::new(
                Box::new(MemoryMeter::new()),
                Usd6::parse("1000000").unwrap(),
                Box::new(SequentialIds::starting_at(1 << 100)),
            ),
        ),
        ManualClock::at(UtcNanos::parse("2026-10-29T14:00:00.000000000Z").unwrap()),
        Box::new(SequentialIds::starting_at(1 << 80)),
    );
    (gateway, model)
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

struct Through<'a>(&'a mut TestGateway);

impl Model for Through<'_> {
    fn call(&mut self, request: &CallRequest) -> Reply {
        self.0.call(request)
    }
}

#[derive(Default)]
struct Served {
    calls: Vec<String>,
}

impl Toolbox for Served {
    fn run(&mut self, call: &ToolCall) -> String {
        self.calls.push(call.name.clone());
        match call.name.as_str() {
            name::POSITIONS => r#"{"positions":[{"symbol":"AAPL","quantity":"10"}]}"#.to_owned(),
            _ => r#"{"ok":true}"#.to_owned(),
        }
    }
}

#[derive(Default)]
struct Journal {
    events: Vec<String>,
    stop: bool,
    spent: bool,
    ids: u128,
}

impl Host for Journal {
    fn invoked(&mut self, turn: u32, record: &CallRecord) -> Result<(), Halt> {
        self.events.push(format!("invoked {turn} {}", record.outcome.code()));
        Ok(())
    }
    fn results(&mut self, turn: u32, calls: &[ToolCall], results: &[String]) -> Result<(), Halt> {
        assert_eq!(calls.len(), results.len(), "one result per call");
        self.events.push(format!("results {turn} {}", calls.len()));
        Ok(())
    }
    fn stopped(&mut self) -> bool {
        self.stop
    }
    fn over_budget(&mut self) -> bool {
        self.spent
    }
    fn now(&mut self) -> UtcNanos {
        UtcNanos::parse("2026-10-29T14:00:00.000000000Z").unwrap()
    }
    fn call_id(&mut self) -> String {
        self.ids += 1;
        encode_ulid((1 << 90) + self.ids)
    }
}

fn spec(model: &ModelRef, kind: &str) -> RunSpec {
    RunSpec {
        workspace_id: WORKSPACE.to_owned(),
        agent_id: (kind != SETUP).then(|| AGENT_ID.to_owned()),
        kind: kind.to_owned(),
        model: model.clone(),
        inputs: BTreeMap::from([
            ("context".to_owned(), "Agent: Growth. Mode: active.".to_owned()),
            ("message".to_owned(), "how am I doing?".to_owned()),
            ("thread".to_owned(), "(empty)".to_owned()),
        ]),
        key_owner: KeyOwner::Platform,
        call_deadline_ms: 120_000,
    }
}

#[test]
fn the_entry_registers_and_each_kind_offers_its_own_tools() {
    let content = chat_entry().content();
    assert!(Registry::new().register(content).is_ok(), "the chat entry is one a registry pins");
    let agent = menu(AGENT);
    let proactive = menu(PROACTIVE);
    let setup = menu(SETUP);
    for owner_only in [name::BUY, name::SELL, name::PAUSE, name::RESUME, name::STOP, name::KILL] {
        assert!(agent.iter().any(|t| t == owner_only), "{owner_only} is the owner's chat's");
        assert!(!proactive.iter().any(|t| t == owner_only), "no proactive run can {owner_only}");
        assert!(!setup.iter().any(|t| t == owner_only), "no setup run can {owner_only}");
    }
    assert!(proactive.iter().any(|t| t == name::PROPOSE), "a proactive run proposes for approval");
    assert!(!agent.iter().any(|t| t == name::PROPOSE));
    assert!(setup.iter().any(|t| t == name::DRAFT), "only setup drafts a mandate");
    assert!(!agent.iter().any(|t| t == name::DRAFT));
    for kind in entry::KINDS {
        assert!(menu(kind).iter().any(|t| t == name::REPLY), "every kind can reply");
    }
}

#[test]
fn a_run_journals_each_call_before_its_tools_and_ends_on_the_summary() {
    let (mut gateway, model) = gateway();
    gateway.transport_mut().script(vec![
        turn(
            &[
                (name::POSITIONS, "{}"),
                (name::REPLY, r#"{"sources":[],"text":"You hold 10 AAPL."}"#),
            ],
            None,
        ),
        turn(&[], Some(r#"{"watch":["AAPL"]}"#)),
    ]);
    let mut tools = Served::default();
    let mut host = Journal::default();
    let end = run(&spec(&model, AGENT), 64, &mut Through(&mut gateway), &mut tools, &mut host);
    assert_eq!(
        end,
        End::Answered {
            watch: vec!["AAPL".to_owned()]
        }
    );
    assert_eq!(tools.calls, vec![name::POSITIONS, name::REPLY]);
    assert_eq!(host.events, vec!["invoked 0 ok", "results 0 2", "invoked 1 ok"]);
    assert_eq!(gateway.transport().sent().len(), 2, "two turns, two provider calls");
}

#[test]
fn a_tool_the_kind_does_not_offer_is_refused_by_the_gateway_and_never_runs() {
    let (mut gateway, model) = gateway();
    gateway
        .transport_mut()
        .script(vec![turn(&[(name::BUY, r#"{"quantity":"5","symbol":"NVDA"}"#)], None)]);
    let mut tools = Served::default();
    let mut host = Journal::default();
    let end = run(&spec(&model, PROACTIVE), 64, &mut Through(&mut gateway), &mut tools, &mut host);
    assert!(matches!(end, End::Model(_)), "a proactive buy is not offered: {end:?}");
    assert!(tools.calls.is_empty(), "the refused call never reached a tool");
}

#[test]
fn stop_and_the_spending_cap_end_a_run_before_any_call() {
    let (mut gateway, model) = gateway();
    let mut tools = Served::default();
    let mut stopped = Journal {
        stop: true,
        ..Journal::default()
    };
    assert_eq!(
        run(&spec(&model, AGENT), 64, &mut Through(&mut gateway), &mut tools, &mut stopped),
        End::Stopped
    );
    let mut spent = Journal {
        spent: true,
        ..Journal::default()
    };
    assert_eq!(
        run(&spec(&model, AGENT), 64, &mut Through(&mut gateway), &mut tools, &mut spent),
        End::Budget
    );
    assert!(gateway.transport().sent().is_empty(), "nothing left the deployment");
}

#[test]
fn a_halt_on_journaling_stops_before_any_tool_runs() {
    struct Failing;
    impl Host for Failing {
        fn invoked(&mut self, _: u32, _: &CallRecord) -> Result<(), Halt> {
            Err(Halt)
        }
        fn results(&mut self, _: u32, _: &[ToolCall], _: &[String]) -> Result<(), Halt> {
            Ok(())
        }
        fn stopped(&mut self) -> bool {
            false
        }
        fn over_budget(&mut self) -> bool {
            false
        }
        fn now(&mut self) -> UtcNanos {
            UtcNanos::parse("2026-10-29T14:00:00.000000000Z").unwrap()
        }
        fn call_id(&mut self) -> String {
            encode_ulid((1 << 90) + 7)
        }
    }
    let (mut gateway, model) = gateway();
    gateway.transport_mut().script(vec![turn(
        &[(name::PAUSE, "{}")],
        None,
    )]);
    let mut tools = Served::default();
    let end = run(&spec(&model, AGENT), 64, &mut Through(&mut gateway), &mut tools, &mut Failing);
    assert_eq!(end, End::Halted);
    assert!(tools.calls.is_empty(), "journal before acting: the pause never ran");
}
