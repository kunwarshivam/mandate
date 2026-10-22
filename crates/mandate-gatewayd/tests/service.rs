//! The service's answer to one request over a real gateway (DEC-2272 items 4 to 6, E21-62): a
//! research call is held to the cap of the version deployed now, read before anything is reserved;
//! a cap that does not read refuses the call `meter_unavailable` with nothing reserved; other
//! purposes and other workspaces' requests are not read for; and the start refuses unless the
//! control stream and every deployed agent's cap read.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use mandate_canon::{Digest, parse};
use mandate_gateway::fake::RecordedTransport;
use mandate_gateway::guard::GuardContext;
use mandate_gateway::kinds::Outcome;
use mandate_gateway::meter::{Caps, DeploymentMeter, MemoryMeter, Meter, MeterStore};
use mandate_gateway::money::{PriceTable, TokenPrice, Usd6};
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{
    ManualClock, ProviderKey, SecretError, SecretSource, SequentialIds,
};
use mandate_gateway::registry::{
    Endpoint, Identity, Locality, Mode, ModelRef, PinnedContent, Registry, Reported, Route,
    Snapshot, Status, Template,
};
use mandate_gateway::{CallRequest, Gateway, KeyOwner, Purpose, RateCaps, RequestError, Settings};
use mandate_gatewayd::cap::CapError;
use mandate_gatewayd::service::{Control, Service};
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, Environment, StoredEvent};
use mandate_time::UtcNanos;

const WORKSPACE: &str = "ws_01J8Z2";
const AGENT: &str = "agent_a";
const UPSTREAM: &str = "anthropic";
const SLUG: &str = "anthropic/claude-sonnet-4.5";
const MANDATE: &str = include_str!("../../mandate-daemon/tests/fixtures/mandate.json");

type TestService = Service<RecordedTransport, MemoryMeter, ManualClock>;

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

fn content() -> PinnedContent {
    PinnedContent {
        model_id: "llm.research_agent".to_owned(),
        model_version: "0.1.0".to_owned(),
        identity: Identity {
            provider: "anthropic".to_owned(),
            model_name: SLUG.to_owned(),
            snapshot: Snapshot::Dated("claude-sonnet-4-5-20250929".to_owned()),
            quantization: None,
        },
        template: Template {
            system: "You write theses as JSON.".to_owned(),
            user: "Market data:\n{{bars}}".to_owned(),
            inputs: vec!["bars".to_owned()],
        },
        retrieval_plan: parse(br#"{"bars":{"days":30}}"#).unwrap(),
        output_schema: parse(br#"{"type":"object","properties":{}}"#).unwrap(),
        validation_bounds: parse(b"{}").unwrap(),
        params_schema: parse(b"{}").unwrap(),
        deadline_ms: 60_000,
        max_output_tokens: 1_000,
        seed: Some(7),
        top_p: None,
        reasoning: None,
        mode: Mode::SingleCall,
        methodology: "Reads bars; proposes theses.".to_owned(),
        authorship: "platform".to_owned(),
    }
}

fn registry() -> (Registry, ModelRef) {
    let mut registry = Registry::new();
    let pinned = content();
    let model = pinned.model_ref();
    registry.register(pinned).unwrap();
    let prices = PriceTable::new(
        TokenPrice::per_million(Usd6::parse("3").unwrap()),
        TokenPrice::per_million(Usd6::parse("15").unwrap()),
        TokenPrice::per_million(Usd6::parse("0.3").unwrap()),
        Digest::of(b"prices"),
    )
    .unwrap();
    let endpoint = Endpoint {
        id: "openrouter:anthropic".to_owned(),
        host: "openrouter.ai".to_owned(),
        region: "us".to_owned(),
        locality: Locality::Hosted,
        route: Route {
            upstream: UPSTREAM.to_owned(),
            reported_provider: "Anthropic".to_owned(),
        },
        prices,
    };
    let reported = Reported {
        model: Some(SLUG.to_owned()),
        provider: Some("Anthropic".to_owned()),
    };
    registry
        .add_endpoint(&model, endpoint, &[reported])
        .unwrap();
    registry.set_status(&model, Status::Offered).unwrap();
    (registry, model)
}

/// A key source holding one test key; the key is a fixture, never a real credential.
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

fn gateway(registry: Registry) -> Gateway<RecordedTransport, MemoryMeter, ManualClock> {
    let settings = Settings {
        workspace_id: WORKSPACE.to_owned(),
        policy: EndpointPolicy::new(
            BTreeSet::from([Locality::Hosted]),
            Some(BTreeSet::from([UPSTREAM.to_owned()])),
            None,
        ),
        guard: GuardContext {
            account_identifiers: vec![],
            personal_data: vec![],
            foreign_values: vec![],
        },
        rate: RateCaps {
            per_workspace: 1_000,
            per_agent: 1_000,
        },
    };
    Gateway::new(
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
                Usd6::parse("1000").unwrap(),
                Box::new(SequentialIds::starting_at(1 << 100)),
            ),
        ),
        ManualClock::at(at("2026-10-16T14:00:00.000000000Z")),
        Box::new(SequentialIds::starting_at(1 << 80)),
    )
}

/// The control stream and the artifact store a test changes between calls, and how often the
/// service read the stream.
#[derive(Clone, Default)]
struct World {
    control: Rc<RefCell<Option<Vec<StoredEvent>>>>,
    store: Rc<RefCell<BTreeMap<Digest, Vec<u8>>>>,
    reads: Rc<Cell<u32>>,
}

impl Control for World {
    fn rows(&self) -> Option<Vec<StoredEvent>> {
        self.reads.set(self.reads.get() + 1);
        self.control.borrow().clone()
    }
}

impl ArtifactSource for World {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.store.borrow().read_artifact(reference)
    }
}

impl World {
    fn new() -> Self {
        let world = Self::default();
        *world.control.borrow_mut() = Some(Vec::new());
        world
    }

    /// Stores the fixture mandate with this daily cap, as canonical bytes, and returns its version.
    fn version(&self, cap: &str) -> String {
        let text = MANDATE.trim_end().replace(
            r#""research":null"#,
            &format!(r#""research":{{"cost_cap_usd_per_day":"{cap}"}}"#),
        );
        let bytes = mandate_canon::to_canonical(&parse(text.as_bytes()).unwrap());
        let reference = ArtifactRef::of(&bytes);
        self.store.borrow_mut().insert(reference.digest(), bytes);
        reference.to_string()
    }

    fn deploy(&self, agent: &str, version: &str) {
        let payload = format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{version}"}}"#
        );
        self.push("AgentDeployed", &payload);
    }

    /// Appends a control-stream row of `event_type` carrying `payload`.
    fn push(&self, event_type: &str, payload: &str) {
        let mut control = self.control.borrow_mut();
        let rows = control.get_or_insert_with(Vec::new);
        let seq = u64::try_from(rows.len()).unwrap() + 1;
        let event_id = format!("01J8Z3MC{seq:018}");
        let body = format!(
            r#"{{"actor":{{"build":null,"id":"user-owner","kind":"user","version":"1"}},"artifact_refs":[],"causation_id":null,"clock_source":"system","config_refs":{{}},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"{event_id}","event_time":"2026-10-16T13:00:00.000000000Z","event_type":"{event_type}","payload":{payload},"pii_refs":[],"schema_version":1,"stream_id":"ctl:{WORKSPACE}"}}"#
        );
        rows.push(StoredEvent {
            stream_id: format!("ctl:{WORKSPACE}"),
            seq,
            event_id,
            event_type: event_type.into(),
            schema_version: 1,
            environment: "paper".into(),
            recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
            prev_hash: Digest::of(b"prev"),
            hash: Digest::of(body.as_bytes()),
            body: body.into_bytes(),
        });
    }

    fn service(&self) -> (TestService, ModelRef) {
        let (registry, model) = registry();
        let service = Service::new(
            WORKSPACE,
            Environment::Paper,
            gateway(registry),
            Box::new(self.clone()),
            Box::new(self.clone()),
        );
        (service, model)
    }
}

fn request(model: &ModelRef, n: u128, purpose: Purpose) -> CallRequest {
    CallRequest {
        call_id: mandate_canon::encode_ulid((1 << 90) + n),
        workspace_id: WORKSPACE.to_owned(),
        agent_id: Some(AGENT.to_owned()),
        purpose,
        model: model.clone(),
        inputs: BTreeMap::from([("bars".to_owned(), "AAPL 2026-10-15 close 231.4".to_owned())]),
        as_of: at("2026-10-16T13:55:00.000000000Z"),
        deadline: at("2026-10-16T14:01:00.000000000Z"),
        observations: vec![],
        key_owner: KeyOwner::Platform,
        run_kind: None,
        turns: vec![],
        tools: vec![],
        spend_ceiling: None,
    }
}

fn outcome(service: &mut TestService, request: &CallRequest) -> Outcome {
    service.answer(request).unwrap().outcome
}

fn reserved(service: &TestService) -> usize {
    service.gateway().meter().store().read().unwrap().len()
}

/// DEC-2272 items 1 and 5: a research call is held to the cap of the version deployed when it
/// arrives. A cap below the call's maximum refuses it `budget_exhausted` with nothing reserved; a
/// version deployed since with a higher cap admits the next call.
#[test]
fn a_research_call_is_held_to_the_cap_of_the_version_deployed_now() {
    let world = World::new();
    world.deploy(AGENT, &world.version("0.001"));
    let (mut service, model) = world.service();
    assert_eq!(
        outcome(&mut service, &request(&model, 1, Purpose::Research)),
        Outcome::BudgetExhausted
    );
    assert_eq!(reserved(&service), 0);

    world.deploy(AGENT, &world.version("5"));
    let admitted = outcome(&mut service, &request(&model, 2, Purpose::Research));
    assert!(
        !matches!(
            admitted,
            Outcome::BudgetExhausted | Outcome::MeterUnavailable
        ),
        "{admitted:?}"
    );
    assert!(reserved(&service) > 0, "the call was reserved");

    world.deploy(AGENT, &world.version("0.001"));
    assert_eq!(
        outcome(&mut service, &request(&model, 3, Purpose::Research)),
        Outcome::BudgetExhausted,
        "a lowered version binds the next call"
    );
}

/// DEC-2272 item 5: a cap that does not read refuses the research call `meter_unavailable`, with
/// nothing reserved, whether the control stream is down or the deployed document is missing, and
/// never falls back on the cap read before.
#[test]
fn a_cap_that_does_not_read_refuses_meter_unavailable_and_reserves_nothing() {
    let world = World::new();
    world.deploy(AGENT, &world.version("5"));
    let (mut service, model) = world.service();
    let first = outcome(&mut service, &request(&model, 1, Purpose::Research));
    assert!(!matches!(first, Outcome::MeterUnavailable), "{first:?}");
    let before = reserved(&service);

    let rows = world.control.borrow_mut().take();
    assert_eq!(
        outcome(&mut service, &request(&model, 2, Purpose::Research)),
        Outcome::MeterUnavailable,
        "the control stream is down"
    );
    *world.control.borrow_mut() = rows;

    world.deploy(AGENT, &ArtifactRef::of(b"never stored").to_string());
    assert_eq!(
        outcome(&mut service, &request(&model, 3, Purpose::Research)),
        Outcome::MeterUnavailable,
        "the deployed version's document is missing"
    );
    assert_eq!(reserved(&service), before, "nothing reserved");
}

/// DEC-2272 item 6, INF-9: only this workspace's research calls are read for; another purpose or a
/// foreign workspace's request reaches the gateway as it came, with no control read.
#[test]
fn only_this_workspace_s_research_calls_read_the_cap() {
    let world = World::new();
    let (mut service, model) = world.service();
    *world.control.borrow_mut() = None;
    let signal = service.answer(&request(&model, 1, Purpose::LlmSignal));
    assert!(
        !matches!(&signal, Ok(record) if record.outcome == Outcome::MeterUnavailable),
        "{signal:?}"
    );
    let mut foreign = request(&model, 2, Purpose::Research);
    foreign.workspace_id = "ws_other".to_owned();
    assert_eq!(
        service.answer(&foreign),
        Err(RequestError::ForeignWorkspace)
    );
    assert_eq!(world.reads.get(), 0, "nothing was read for either");
}

/// DEC-2272 item 4: the start refuses unless the control stream reads and every agent it names as
/// deployed has a cap that reads, a document whose hash differs from its journaled version
/// included; with no agents deployed, it starts.
#[test]
fn the_start_refuses_unless_every_deployed_agent_s_cap_reads() {
    let world = World::new();
    assert_eq!(world.service().0.ready(), Ok(()), "no agents deployed");

    world.deploy(AGENT, &world.version("5"));
    world.deploy("agent_b", &world.version("3"));
    assert_eq!(world.service().0.ready(), Ok(()));

    let journaled = ArtifactRef::of(b"the document the core validated");
    world
        .store
        .borrow_mut()
        .insert(journaled.digest(), b"another document".to_vec());
    world.deploy("agent_c", &journaled.to_string());
    assert_eq!(
        world.service().0.ready(),
        Err(CapError::DocumentUnavailable),
        "a document whose hash differs from the journaled version"
    );

    *world.control.borrow_mut() = None;
    assert_eq!(world.service().0.ready(), Err(CapError::ControlUnreadable));
}

/// DEC-2272 item 4: the caps the start reads are the ones its first calls are held to.
#[test]
fn the_start_holds_each_deployed_agent_s_cap() {
    let world = World::new();
    world.deploy(AGENT, &world.version("0.001"));
    let (mut service, model) = world.service();
    service.ready().unwrap();
    assert!(!service.gateway().meter().caps().research_unreadable(AGENT));
    assert_eq!(
        outcome(&mut service, &request(&model, 1, Purpose::Research)),
        Outcome::BudgetExhausted
    );
}

/// DEC-2272 item 3, the coordinator's bound: research calls naming agents never deployed are
/// refused `budget_exhausted` with nothing reserved and nothing held, whether the control stream
/// reads or not, so what the service holds is bounded by the deployed agents alone; a stopped
/// agent is held no longer.
#[test]
fn unknown_agent_names_leave_the_held_caps_unchanged() {
    let world = World::new();
    world.deploy(AGENT, &world.version("5"));
    let (mut service, model) = world.service();
    service.ready().unwrap();
    let known = BTreeSet::from([AGENT.to_owned()]);
    let ghost = |n: u128| {
        let mut call = request(&model, n, Purpose::Research);
        call.agent_id = Some(format!("ghost_{n}"));
        call
    };
    for n in 0..30 {
        assert_eq!(outcome(&mut service, &ghost(n)), Outcome::BudgetExhausted);
    }
    let rows = world.control.borrow_mut().take();
    for n in 30..60 {
        assert_eq!(outcome(&mut service, &ghost(n)), Outcome::BudgetExhausted);
    }
    assert_eq!(service.gateway().meter().caps().research_agents(), known);
    assert_eq!(service.daily_caps().held(), 1);
    assert_eq!(reserved(&service), 0, "nothing reserved for any of them");

    assert_eq!(
        outcome(&mut service, &request(&model, 60, Purpose::Research)),
        Outcome::MeterUnavailable,
        "the deployed agent, with the stream down"
    );
    *world.control.borrow_mut() = rows;

    let payload = format!(
        r#"{{"agent_id":"{AGENT}","connection_id":"conn_alpaca_paper_01","loss_added":"0","reason":"owner_stop","retired_on":"2026-10-16"}}"#
    );
    world.push("AgentStopped", &payload);
    assert_eq!(
        outcome(&mut service, &request(&model, 61, Purpose::Research)),
        Outcome::BudgetExhausted,
        "stopped since"
    );
    assert!(
        service
            .gateway()
            .meter()
            .caps()
            .research_agents()
            .is_empty()
    );
    assert_eq!(service.daily_caps().held(), 0);
}

/// The coordinator's bound, on a stop: every control read prunes what the service holds to the
/// agents deployed now, so an agent stopped since leaves no entry even when it never calls again,
/// and the counts stay at the deployed count after calls from many unique names.
#[test]
fn every_control_read_prunes_what_is_held_to_the_deployed_agents() {
    let world = World::new();
    world.deploy(AGENT, &world.version("5"));
    world.deploy("agent_b", &world.version("3"));
    let (mut service, model) = world.service();
    service.ready().unwrap();
    assert_eq!(service.gateway().meter().caps().research_agents().len(), 2);
    assert_eq!(service.daily_caps().held(), 2);

    let payload = r#"{"agent_id":"agent_b","connection_id":"conn_alpaca_paper_01","loss_added":"0","reason":"owner_stop","retired_on":"2026-10-16"}"#;
    world.push("AgentStopped", payload);
    let first = outcome(&mut service, &request(&model, 1, Purpose::Research));
    assert!(
        !matches!(first, Outcome::BudgetExhausted | Outcome::MeterUnavailable),
        "{first:?}"
    );
    assert_eq!(
        service.gateway().meter().caps().research_agents(),
        BTreeSet::from([AGENT.to_owned()]),
        "agent_b, stopped and silent since, is pruned by agent_a's call"
    );
    assert_eq!(service.daily_caps().held(), 1);

    for n in 2..40 {
        let mut call = request(&model, n, Purpose::Research);
        call.agent_id = Some(format!("ghost_{n}"));
        assert_eq!(outcome(&mut service, &call), Outcome::BudgetExhausted);
    }
    assert_eq!(service.gateway().meter().caps().research_agents().len(), 1);
    assert_eq!(service.daily_caps().held(), 1);
}
