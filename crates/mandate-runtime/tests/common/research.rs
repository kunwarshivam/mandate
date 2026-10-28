//! The research loop's harness: the in-memory shell extended with a [`ResearchState`], the worker
//! over recorded fixtures, `mandate-gateway` over its recorded fake (the default in every test and
//! CI job, with no network to any model provider), the research agent's admission context, and an order
//! plan that sizes through the real `conviction_linear` builder and classifies through the real
//! autonomy rules (`mandate-builder`), so a test runs bars and news to an ask with nothing faked
//! between the thesis and the gate but the gate itself.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{AssetClass as AccountingClass, InstrumentId, Side};
use mandate_builder::{
    AccountSnapshot, Action, ActionContext, BuilderMandate, GoalKind, Limits, Market,
    ModelOutput as BuilderOutput, ModelVersion as BuilderVersion, RiskContext, SignalModel, Sizing,
    classify, propose,
};
use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_domain::{AssetClass, AssetId as DomainAsset, AutonomyDecision, MarketSession};
use mandate_gateway::fake::{Recorded, RecordedTransport};
use mandate_gateway::guard::GuardContext;
use mandate_gateway::meter::{Caps, DeploymentMeter, MemoryMeter, Meter, MeterStore, StoreError};
use mandate_gateway::money::{PriceTable, TokenPrice, Usage, Usd6};
use mandate_gateway::openrouter::OpenRouter;
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::{
    Clock, Delivery, HttpRequest, ManualClock, NoKeys, ProviderKey, SecretError, SecretSource,
    SequentialIds, Transport,
};
use mandate_gateway::registry::{
    Endpoint, Identity, Locality, PinnedContent, Registry, Reported, Route, Snapshot, Status,
};
use mandate_gateway::{CallRecord, Gateway, Outcome, RateCaps, Settings};
use mandate_num::{CostBasis, FeeRate, MarkPrice, Price, Qty, Signed, SizeFraction, Unit, Usd};
use mandate_research::{
    AdmissionFacts, AllowlistVersion, AssetId, LineageState, MandateEnvelope, PolicyOverlay,
    ResearchEnvelope, SourceAllowlist, SourceId as ResearchSource, ValidatedMandate,
    WorkingUniverse,
};
use mandate_research_run::{
    Allowlist, Answer, Bar, BarReader, CallContext, CallPlan, Filing, FilingReader, GatewayParts,
    InstrumentFact, InstrumentReader, ItemId, ModelPin, NewsItem, NewsReader, NoHostedTransport,
    OutputBounds, ReadError, Readers, ResearchEntry, ResearchTransport, Retrieval, RetrievalPlan,
    Scope, SourceId, Symbol, TextQuery, call, recorded_reply, research_gateway, retrieve,
};
use mandate_runtime::{
    ActiveThesis, ActorKind, Autonomy, Classified, DecisionClip, Effect, EventDraft, EventTriggers,
    FoldedEvent, JournalCatalog, MandateView, ModelDirection, OrderExecution, OrderPlan, Ports,
    Preconditions, Proposal, Purpose, RecordCatalog, ResearchConfig, ResearchContext,
    ResearchInput, ResearchRecord, ResearchRequest, ResearchState, ResearchStep,
    RetrievedObservation, RiskClock, RuntimeError, Seq, SignalInputs, TimeInForce, fold,
    handle_research,
};
use mandate_spec::condition::{Condition, ConditionField, ConditionValue, Operator};
use mandate_spec::document::{
    Approval, ApproverRef, Autonomy as Policy, ModelId, OnTimeout, Rule, RuleId, SizingMethod,
};
use mandate_time::{Date, UtcNanos};

use super::{AGENT_STREAM, Shell, clock};

/// The recorded fixtures' cut-off: 2026-10-16 20:00 UTC, after Friday's close.
pub const CUT_OFF: i64 = 1_792_180_800;
pub const AAPL: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
pub const MSFT: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
pub const RESEARCH_MODEL: &str = "llm.research_agent";
pub const INTERVAL_S: u32 = 3_600;
pub const MAX_ORDER_USD: &str = "1000";
pub const MAX_POSITION_USD: &str = "1500";
pub const EQUITY_USD: &str = "10000";

const BARS: &str = include_str!("../fixtures/research/bars.json");
const NEWS: &str = include_str!("../fixtures/research/news.json");
const INSTRUMENTS: &str = include_str!("../fixtures/research/instruments.json");
pub const RESPONSE_AAPL: &str = include_str!("../fixtures/research/response-aapl.json");
pub const RESPONSE_INJECTION: &str = include_str!("../fixtures/research/response-injection.json");

/// The wall-clock instant of `secs`, as the gateway's clock reads it.
pub fn instant(secs: i64) -> UtcNanos {
    at(secs)
}

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap_or_else(|e| panic!("{secs} is an instant: {e}"))
}

fn symbol(text: &str) -> Symbol {
    Symbol::new(text).unwrap_or_else(|e| panic!("`{text}` is a symbol: {e}"))
}

fn text_of<'a>(value: &'a Value, key: &str) -> &'a str {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("fixture member {key} is text"))
}

fn int_of(value: &Value, key: &str) -> i64 {
    value
        .get(key)
        .and_then(Value::as_int)
        .and_then(|n| i64::try_from(n).ok())
        .unwrap_or_else(|| panic!("fixture member {key} is an integer"))
}

fn flag_of(value: &Value, key: &str) -> bool {
    matches!(value.get(key), Some(Value::Bool(true)))
}

fn rows(text: &str) -> Vec<Value> {
    parse(text.as_bytes())
        .unwrap_or_else(|e| panic!("a fixture is strict JSON: {e:?}"))
        .as_array()
        .unwrap_or_else(|| panic!("a fixture is an array"))
        .to_vec()
}

fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("`{text}` is a price: {e}"))
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("`{text}` is USD: {e}"))
}

/// The recorded data plane: bars, news (one item off the allowlist, one injection, one known after
/// the cut-off), and the instrument snapshot.
pub struct RecordedPlane {
    pub bars: Vec<Bar>,
    pub news: Vec<NewsItem>,
    pub filings: Vec<Filing>,
    pub instruments: Vec<InstrumentFact>,
}

impl RecordedPlane {
    pub fn load() -> Self {
        let bars = rows(BARS)
            .iter()
            .map(|row| Bar {
                symbol: symbol(text_of(row, "symbol")),
                knowledge_at: at(int_of(row, "knowledge_at")),
                open: price(text_of(row, "open")),
                high: price(text_of(row, "high")),
                low: price(text_of(row, "low")),
                close: price(text_of(row, "close")),
                volume: Qty::parse(text_of(row, "volume")).unwrap_or_else(|e| panic!("{e}")),
            })
            .collect();
        let news = rows(NEWS)
            .iter()
            .map(|row| {
                let id = text_of(row, "item_id");
                NewsItem {
                    item_id: ItemId::new(id).unwrap_or_else(|e| panic!("{e}")),
                    source: SourceId::new(text_of(row, "source")).unwrap_or_else(|e| panic!("{e}")),
                    symbols: row
                        .get("symbols")
                        .and_then(Value::as_array)
                        .unwrap_or_default()
                        .iter()
                        .filter_map(Value::as_str)
                        .map(symbol)
                        .collect(),
                    knowledge_at: at(int_of(row, "knowledge_at")),
                    published_at: at(int_of(row, "published_at")),
                    headline: text_of(row, "headline").to_owned(),
                    summary: text_of(row, "summary").to_owned(),
                    link: text_of(row, "link").to_owned(),
                    content_hash: Digest::of(id.as_bytes()),
                    licensed: flag_of(row, "licensed"),
                }
            })
            .collect();
        let instruments = rows(INSTRUMENTS)
            .iter()
            .map(|row| InstrumentFact {
                asset_id: DomainAsset::parse(text_of(row, "asset_id"))
                    .unwrap_or_else(|e| panic!("{e}")),
                symbol: symbol(text_of(row, "symbol")),
                asset_class: AssetClass::parse(text_of(row, "asset_class"))
                    .unwrap_or_else(|e| panic!("{e}")),
                eligible: flag_of(row, "eligible"),
                group: None,
                leveraged_or_inverse_etp: flag_of(row, "leveraged_or_inverse_etp"),
            })
            .collect();
        Self {
            bars,
            news,
            filings: Vec::new(),
            instruments,
        }
    }

    pub fn readers(&self) -> Readers<'_> {
        Readers {
            bars: self,
            news: self,
            filings: self,
            instruments: self,
        }
    }
}

impl BarReader for RecordedPlane {
    fn daily_bars(
        &self,
        name: &Symbol,
        _cut_off: UtcNanos,
        _limit: u32,
    ) -> Result<Vec<Bar>, ReadError> {
        Ok(self
            .bars
            .iter()
            .filter(|bar| bar.symbol == *name)
            .cloned()
            .collect())
    }
}

impl NewsReader for RecordedPlane {
    fn news(&self, _query: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
        Ok(self.news.clone())
    }
}

impl FilingReader for RecordedPlane {
    fn filings(&self, _query: &TextQuery) -> Result<Vec<Filing>, ReadError> {
        Ok(self.filings.clone())
    }
}

impl InstrumentReader for RecordedPlane {
    fn instruments(&self, _symbols: &BTreeSet<Symbol>) -> Result<Vec<InstrumentFact>, ReadError> {
        Ok(self.instruments.clone())
    }
}

/// The research entry's pinned model: one OpenRouter slug served by one upstream (DEC-997 item 2).
pub const SLUG: &str = "anthropic/claude-sonnet-4.5";
pub const UPSTREAM: &str = "anthropic";
pub const REPORTED_PROVIDER: &str = "Anthropic";
/// How long a recorded reply takes; the call's deadline is 120 s after dispatch.
pub const LATENCY_MS: u64 = 30_000;

pub fn entry() -> ResearchEntry {
    ResearchEntry {
        model_id: RESEARCH_MODEL.to_owned(),
        model_version: "0.1.0".to_owned(),
        identity: Identity {
            provider: "anthropic".to_owned(),
            model_name: SLUG.to_owned(),
            snapshot: Snapshot::Dated("claude-sonnet-4-5-20250929".to_owned()),
            quantization: None,
        },
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        seed: Some(7),
        plan: RetrievalPlan::v0_1(),
        bounds: OutputBounds::v0_1(),
        agentic: None,
    }
}

pub fn pin() -> ModelPin {
    entry().content().model_ref()
}

fn endpoint() -> Endpoint {
    let usd6 = |t: &str| Usd6::parse(t).unwrap_or_else(|e| panic!("{e}"));
    Endpoint {
        id: "openrouter:anthropic".to_owned(),
        host: "openrouter.ai".to_owned(),
        region: "us".to_owned(),
        locality: Locality::Hosted,
        route: Route {
            upstream: UPSTREAM.to_owned(),
            reported_provider: REPORTED_PROVIDER.to_owned(),
        },
        prices: PriceTable::new(
            TokenPrice::per_million(usd6("3")),
            TokenPrice::per_million(usd6("15")),
            TokenPrice::per_million(usd6("0.3")),
            Digest::of(b"openrouter anthropic price table 1"),
        )
        .unwrap_or_else(|e| panic!("{e}")),
    }
}

fn registry() -> Registry {
    registry_of(entry().content())
}

/// An entry whose output schema also admits `predecessor_thesis_id`, as one would once §8.6 item
/// 5 allows revisions, so a revision reaches the harness's own check.
pub fn revisable_content() -> PinnedContent {
    let mut content = entry().content();
    content.model_version = "0.1.1".to_owned();
    let schema =
        String::from_utf8(to_canonical(&content.output_schema)).unwrap_or_else(|e| panic!("{e}"));
    let thesis_text = r#""invalidation":{"max_length":2000,"type":"string"},"thesis":"#;
    assert_eq!(schema.matches(thesis_text).count(), 1, "{schema}");
    let widened = schema.replace(
        thesis_text,
        r#""invalidation":{"max_length":2000,"type":"string"},"predecessor_thesis_id":{"max_length":128,"type":"string"},"thesis":"#,
    );
    content.output_schema = parse(widened.as_bytes()).unwrap_or_else(|e| panic!("{e:?}"));
    content
}

fn registry_of(content: PinnedContent) -> Registry {
    let pin = content.model_ref();
    let mut registry = Registry::new();
    registry
        .register(content)
        .unwrap_or_else(|e| panic!("the research entry registers: {e}"));
    let probe = Reported {
        model: Some(SLUG.to_owned()),
        provider: Some(REPORTED_PROVIDER.to_owned()),
    };
    registry
        .add_endpoint(&pin, endpoint(), &[probe])
        .unwrap_or_else(|e| panic!("{e}"));
    registry
        .set_status(&pin, Status::Offered)
        .unwrap_or_else(|e| panic!("{e}"));
    registry
}

/// What the model and the gateway do with the run's one call: the pinned model answers with this
/// output, or the call ends with this outcome, produced the way the gateway itself produces it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Model {
    Output(String),
    Outcome(Outcome),
    /// The pinned model's reply carrying this output, read after the call's deadline.
    Late(String),
    /// A reply carrying this output from a model other than the pin's (INF-2).
    Impostor(String),
    /// The pinned model's reply carrying this output, through [`revisable_content`]'s entry.
    Revisable(String),
}

pub fn recorded(output: &str) -> Model {
    Model::Output(output.to_owned())
}

pub fn failing(outcome: Outcome) -> Model {
    Model::Outcome(outcome)
}

pub type TestGateway = Gateway<ResearchTransport<NoHostedTransport>, MemoryMeter, ManualClock>;

fn usage() -> Usage {
    Usage {
        input: 2_000,
        output: 300,
        cached: 0,
    }
}

/// The pinned model's reply carrying `output`.
pub fn reply(output: &str) -> Vec<u8> {
    recorded_reply(output, SLUG, REPORTED_PROVIDER, usage())
}

fn delivered(status: u16, body: Vec<u8>, latency_ms: u64) -> Recorded {
    Recorded {
        latency_ms,
        delivery: Delivery::Response {
            status,
            body,
            retry_after_ms: None,
        },
    }
}

/// The gateway over the recorded fake, its clock at `now`, set up to end the next call as `model`
/// says. Only `credential_invalid` needs a vault with no key behind the hosted transport's place,
/// which no default gateway has, so that one is built by hand.
pub fn gateway_for(model: &Model, now: UtcNanos) -> TestGateway {
    let mut recorded = RecordedTransport::new();
    let mut caps = budget();
    let mut policy = EndpointPolicy::new(
        BTreeSet::from([Locality::Hosted]),
        Some(BTreeSet::from([UPSTREAM.to_owned()])),
        None,
    );
    let mut guard = GuardContext::default();
    let mut rate = RateCaps {
        per_workspace: 100,
        per_agent: 100,
    };
    match model {
        Model::Output(output) => recorded.script(vec![delivered(200, reply(output), LATENCY_MS)]),
        Model::Revisable(output) => {
            recorded.script(vec![delivered(200, reply(output), LATENCY_MS)])
        }
        Model::Late(output) => recorded.script(vec![delivered(200, reply(output), 200_000)]),
        Model::Impostor(output) => recorded.script(vec![delivered(
            200,
            recorded_reply(output, "anthropic/claude-substitute", REPORTED_PROVIDER, usage()),
            LATENCY_MS,
        )]),
        Model::Outcome(outcome) => match outcome {
            Outcome::Ok => recorded.script(vec![delivered(
                200,
                reply(r#"{"invalidations":[],"theses":[]}"#),
                LATENCY_MS,
            )]),
            Outcome::PolicyDenied => policy = EndpointPolicy::local_only(),
            Outcome::BudgetExhausted => caps = Caps::new(Usd6::from_micros(1)),
            Outcome::RateLimitedLocal => {
                rate = RateCaps {
                    per_workspace: 0,
                    per_agent: 0,
                }
            }
            Outcome::InputRejected => guard.account_identifiers = vec!["large caps".to_owned()],
            Outcome::DeadlineExceeded => {
                recorded.script(vec![delivered(200, reply("{}"), 200_000)])
            }
            Outcome::RateLimitedProvider => recorded.script(vec![Recorded {
                latency_ms: 10,
                delivery: Delivery::Response {
                    status: 429,
                    body: Vec::new(),
                    retry_after_ms: Some(600_000),
                },
            }]),
            Outcome::ContentRefused => recorded.script(vec![delivered(
                200,
                format!(
                    r#"{{"choices":[{{"finish_reason":"content_filter","index":0,"message":{{"content":null,"role":"assistant"}}}}],"id":"gen-refused","model":"{SLUG}","provider":"{REPORTED_PROVIDER}","usage":{{"completion_tokens":0,"prompt_tokens":10}}}}"#
                )
                .into_bytes(),
                LATENCY_MS,
            )]),
            Outcome::SchemaInvalid => {
                recorded.script(vec![delivered(200, reply("not json"), LATENCY_MS)])
            }
            Outcome::IdentityMismatch => recorded.script(vec![delivered(
                200,
                recorded_reply(
                    r#"{"invalidations":[],"theses":[]}"#,
                    "anthropic/claude-other",
                    REPORTED_PROVIDER,
                    usage(),
                ),
                LATENCY_MS,
            )]),
            Outcome::ProviderUnavailable => recorded.script(
                (0..3)
                    .map(|_| Recorded {
                        latency_ms: 10_000,
                        delivery: Delivery::ConnectFailed,
                    })
                    .collect(),
            ),
            Outcome::ModelWithdrawn
            | Outcome::MeterUnavailable
            | Outcome::CredentialInvalid => {}
        },
    }
    let mut meter = MemoryMeter::new();
    if *model == Model::Outcome(Outcome::MeterUnavailable) {
        meter.fail_with(Some(StoreError::Unavailable));
    }
    let parts = GatewayParts {
        settings: Settings {
            workspace_id: super::WORKSPACE.to_owned(),
            policy,
            guard,
            rate,
        },
        registry: match model {
            Model::Revisable(_) => registry_of(revisable_content()),
            _ => registry(),
        },
        meter: Meter::new(meter, caps, deployment_meter()),
        clock: ManualClock::at(now),
        ids: Box::new(SequentialIds::starting_at(1 << 80)),
    };
    let mut gateway = if *model == Model::Outcome(Outcome::CredentialInvalid) {
        Gateway::new(
            parts.settings,
            parts.registry,
            Box::new(OpenRouter),
            ResearchTransport::Recorded(recorded),
            Box::new(NoKeys),
            parts.meter,
            parts.clock,
            parts.ids,
        )
    } else {
        research_gateway(
            parts,
            Box::new(NoKeys),
            None::<NoHostedTransport>,
            recorded,
            UPSTREAM,
        )
        .0
    };
    if *model == Model::Outcome(Outcome::ModelWithdrawn) {
        gateway.withdraw(&pin()).unwrap_or_else(|e| panic!("{e}"));
    }
    gateway
}

/// A call record for a call no run planned: what a stray or substituted answer looks like.
pub fn stray_record(call_id: &str, world: &World) -> CallRecord {
    let plan = CallPlan {
        run_id: call_id.to_owned(),
        call_id: call_id.to_owned(),
        workspace_id: super::WORKSPACE.to_owned(),
        agent_id: super::AGENT.to_owned(),
        model: pin(),
        as_of: at(CUT_OFF),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        observations: Vec::new(),
    };
    let retrieval = Retrieval {
        cut_off: at(CUT_OFF),
        items: Vec::new(),
        drops: Vec::new(),
    };
    record_for(
        &failing(Outcome::ProviderUnavailable),
        &plan,
        &retrieval,
        world,
        at(CUT_OFF),
    )
}

/// The gateway's record of one call for `plan`, dispatched at `dispatched`, as `model` says.
pub fn record_for(
    model: &Model,
    plan: &CallPlan,
    retrieval: &Retrieval,
    world: &World,
    dispatched: UtcNanos,
) -> CallRecord {
    let mut gateway = gateway_for(model, dispatched);
    match call(&mut gateway, plan, retrieval, world.call_context()) {
        Ok((_, Answer::Recorded(record))) => record,
        other => panic!("the test gateway records every call it is handed: {other:?}"),
    }
}

pub fn config() -> ResearchConfig {
    ResearchConfig {
        pin: pin(),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        plan: RetrievalPlan::v0_1(),
        bounds: OutputBounds::v0_1(),
        description: "Long-only US large caps, on public news and filings.".to_owned(),
        revisions_enabled: false,
        agentic: None,
    }
}

/// A catalogue double: `true` registers both records, which no production catalogue does yet.
pub struct Catalog(pub bool);

impl RecordCatalog for Catalog {
    fn registers(&self, _record: ResearchRecord) -> bool {
        self.0
    }
}

pub fn open_preconditions() -> Preconditions {
    Preconditions::read(&Catalog(true))
}

pub fn production_preconditions() -> Preconditions {
    Preconditions::read(&JournalCatalog::JOURNAL)
}

/// The `research_equity` envelope: five instruments, US equities, a research envelope with a
/// one-hour interval and a 5 USD daily cap, admission `ask`.
pub fn envelope() -> MandateEnvelope {
    MandateEnvelope {
        universe_pinned: false,
        max_instruments: 5,
        asset_classes: BTreeSet::from([mandate_research::AssetClass::UsEquity]),
        leveraged_etps_enabled: false,
        leveraged_etp_disclosure_version: None,
        admits_instruments: true,
        research: Some(ResearchEnvelope {
            interval_s: INTERVAL_S,
            cost_cap_usd_per_day: usd("5"),
            max_revisions_per_lineage: 3,
        }),
        admission: mandate_research::AutonomyDecision::Ask,
    }
}

pub fn allowlist_sources() -> BTreeSet<&'static str> {
    BTreeSet::from(["src.newswire", "src.wire_two", "src.edgar"])
}

pub fn facts() -> AdmissionFacts {
    AdmissionFacts {
        allowlist: SourceAllowlist {
            version: AllowlistVersion(3),
            sources: allowlist_sources()
                .into_iter()
                .map(|s| ResearchSource::new(s).unwrap_or_else(|e| panic!("{e}")))
                .collect(),
        },
        eligibility_failures: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        halted_instruments: BTreeSet::new(),
        disclosures_accepted: BTreeSet::new(),
        data_universe: Some(
            [AAPL, MSFT]
                .iter()
                .map(|id| AssetId::new(id).unwrap_or_else(|e| panic!("{e}")))
                .collect(),
        ),
        research_spend_usd_today: usd("0"),
    }
}

/// Everything a research step reads besides the core's state, owned so a test can edit any part.
pub struct World {
    pub config: Option<ResearchConfig>,
    pub preconditions: Preconditions,
    pub mandate: ValidatedMandate,
    pub overlay: PolicyOverlay,
    pub universe: WorkingUniverse,
    pub facts: AdmissionFacts,
    pub lineages: LineageState,
    pub active: Vec<ActiveThesis>,
    pub last_reservation: Option<RiskClock>,
    pub data_symbols: BTreeSet<Symbol>,
    pub event_triggers: Option<EventTriggers>,
}

impl World {
    pub fn open() -> Self {
        Self {
            config: Some(config()),
            preconditions: open_preconditions(),
            mandate: ValidatedMandate::from_validated_envelope(envelope()),
            overlay: PolicyOverlay::internal_research_profile(),
            universe: WorkingUniverse::Known {
                instruments: BTreeSet::new(),
                pinned: false,
            },
            facts: facts(),
            lineages: LineageState::default(),
            active: Vec::new(),
            last_reservation: None,
            data_symbols: BTreeSet::from([symbol("AAPL"), symbol("MSFT")]),
            event_triggers: None,
        }
    }

    pub fn context(&self) -> ResearchContext<'_> {
        ResearchContext {
            config: self.config.as_ref(),
            preconditions: &self.preconditions,
            mandate: &self.mandate,
            overlay: &self.overlay,
            universe: &self.universe,
            facts: &self.facts,
            lineages: &self.lineages,
            active_theses: &self.active,
            last_reservation: self.last_reservation,
            data_symbols: &self.data_symbols,
            event_triggers: self.event_triggers.as_ref(),
        }
    }

    /// The executor's copy of an admission (§6.2 step 6): the account stream's working universe
    /// then holds the instrument.
    pub fn admit(&mut self, instrument: &str) {
        if let WorkingUniverse::Known { instruments, .. } = &mut self.universe {
            instruments.insert(AssetId::new(instrument).unwrap_or_else(|e| panic!("{e}")));
        }
    }

    pub fn scope(&self, cut_off: RiskClock) -> Scope {
        Scope {
            workspace: "ws1".to_owned(),
            agent: "agent-a".to_owned(),
            cut_off: at(cut_off.secs()),
            data_universe: BTreeSet::from([symbol("AAPL"), symbol("MSFT")]),
            held: BTreeSet::new(),
            allowlist: Allowlist {
                version: 3,
                sources: allowlist_sources()
                    .into_iter()
                    .map(|s| SourceId::new(s).unwrap_or_else(|e| panic!("{e}")))
                    .collect(),
            },
        }
    }

    pub fn call_context(&self) -> CallContext {
        CallContext {
            description: config().description,
            data_universe: vec![symbol("AAPL"), symbol("MSFT")],
            asset_classes: vec![AssetClass::UsEquity],
            max_instruments: 5,
            working_universe: Vec::new(),
            held: Vec::new(),
            theses: Vec::new(),
        }
    }
}

/// The shell and the research state of one process.
pub struct Loop {
    pub shell: Shell,
    pub research: ResearchState,
    pub steps: Vec<ResearchStep>,
}

impl Loop {
    pub fn new(epoch: u64) -> Self {
        Self {
            shell: Shell::new(epoch),
            research: ResearchState::new(),
            steps: Vec::new(),
        }
    }

    /// One research step: drafts appended and folded back into both states, in order, as the
    /// shell's tailer would; the requests are returned for the worker.
    pub fn step(
        &mut self,
        input: ResearchInput,
        ports: &Ports<'_>,
        world: &World,
    ) -> Result<ResearchStep, RuntimeError> {
        let step = handle_research(
            &mut self.shell.state,
            &mut self.research,
            input,
            ports,
            &world.context(),
        )?;
        for effect in &step.effects {
            if let Effect::Journal(draft) = effect {
                self.append(draft);
            }
        }
        self.steps.push(step.clone());
        Ok(step)
    }

    pub fn run(&mut self, input: ResearchInput, ports: &Ports<'_>, world: &World) -> ResearchStep {
        self.step(input, ports, world)
            .unwrap_or_else(|e| panic!("research step refused with {}: {e}", e.code()))
    }

    fn append(&mut self, draft: &EventDraft) {
        let seq = self.shell.head().0.saturating_add(1);
        let stored = FoldedEvent {
            stream: AGENT_STREAM.to_owned(),
            seq: Seq(seq),
            event_id: draft.event_id.clone(),
            event_type: draft.event_type.clone(),
            causation_id: draft.causation_id.clone(),
            actor: ActorKind::Agent,
            payload: draft.payload.clone(),
        };
        self.shell.agent_journal.push(stored.clone());
        fold(&mut self.shell.state, &stored).unwrap_or_else(|e| {
            panic!(
                "the writer cannot fold back its own {}: {e}",
                stored.event_type
            )
        });
        self.research
            .fold(&stored)
            .unwrap_or_else(|e| panic!("the research fold refused {}: {e}", stored.event_type));
    }

    /// The journal's event types, in `seq` order.
    pub fn types(&self) -> Vec<String> {
        self.shell
            .agent_journal
            .iter()
            .map(|event| event.event_type.clone())
            .collect()
    }

    pub fn events(&self, event_type: &str) -> Vec<&FoldedEvent> {
        self.shell
            .agent_journal
            .iter()
            .filter(|event| event.event_type == event_type)
            .collect()
    }
}

/// The worker's half of one run, as the shell's research task runs it: the start request's
/// retrieval, the captured observations handed in, the call request sent through the gateway at
/// the run's cut-off, and the gateway's answer handed in at `answered_at`. Returns the requests the
/// core emitted on the way.
pub fn worker_run(
    lp: &mut Loop,
    now: RiskClock,
    ports: &Ports<'_>,
    world: &World,
    plane: &RecordedPlane,
    model: &Model,
    answered_at: RiskClock,
) -> Vec<ResearchRequest> {
    let mut gateway = gateway_for(model, at(now.secs()));
    worker_run_on(lp, now, ports, world, plane, &mut gateway, answered_at)
}

/// [`worker_run`] through a gateway the caller built, whose clock is moved to the run's cut-off.
pub fn worker_run_on<T: Transport, S: MeterStore, C: Clock>(
    lp: &mut Loop,
    now: RiskClock,
    ports: &Ports<'_>,
    world: &World,
    plane: &RecordedPlane,
    gateway: &mut Gateway<T, S, C>,
    answered_at: RiskClock,
) -> Vec<ResearchRequest> {
    let mut seen = Vec::new();
    let started = lp.run(ResearchInput::Poll(now), ports, world);
    seen.extend(started.requests.clone());
    let Some(ResearchRequest::Start { run_id, cut_off }) = started
        .requests
        .iter()
        .find(|request| matches!(request, ResearchRequest::Start { .. }))
        .cloned()
    else {
        return seen;
    };
    let scope = world.scope(cut_off);
    let retrieval = retrieve(&config().plan, &scope, plane.readers())
        .unwrap_or_else(|e| panic!("the recorded plane retrieves: {e}"));
    let items = retrieval
        .items
        .iter()
        .map(|item| {
            RetrievedObservation::capture(item, cut_off)
                .unwrap_or_else(|e| panic!("every recorded item is captured: {e}"))
                .0
        })
        .collect();
    let retrieved = lp.run(
        ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items,
        },
        ports,
        world,
    );
    seen.extend(retrieved.requests.clone());
    let Some(ResearchRequest::Call(plan)) = retrieved
        .requests
        .iter()
        .find(|request| matches!(request, ResearchRequest::Call(_)))
        .cloned()
    else {
        return seen;
    };
    gateway.clock_mut().wait_until(at(cut_off.secs()));
    let (_, answer) = call(gateway, &plan, &retrieval, world.call_context())
        .unwrap_or_else(|e| panic!("the call builds from its journaled items: {e}"));
    let input = match answer {
        Answer::Recorded(record) => ResearchInput::Called {
            run_id,
            at: answered_at,
            record: Box::new(record),
        },
        Answer::Refused(error) => ResearchInput::Refused {
            run_id,
            at: answered_at,
            error,
        },
    };
    let called = lp.run(input, ports, world);
    seen.extend(called.requests.clone());
    seen
}

/// A run whose call is answered `error` instead of sent: started at `now`, retrieved from `plane`,
/// and, once the core asks for its call, handed the refusal at `answered_at`, as a worker hands
/// what its port returned. The step that took the refusal is the loop's last.
pub fn refused_run(
    lp: &mut Loop,
    now: RiskClock,
    ports: &Ports<'_>,
    world: &World,
    plane: &RecordedPlane,
    error: mandate_gateway::RequestError,
    answered_at: RiskClock,
) {
    let started = lp.run(ResearchInput::Poll(now), ports, world);
    let Some(ResearchRequest::Start { run_id, cut_off }) = started
        .requests
        .iter()
        .find(|request| matches!(request, ResearchRequest::Start { .. }))
        .cloned()
    else {
        panic!("no run started: {started:?}");
    };
    let scope = world.scope(cut_off);
    let retrieval = retrieve(&config().plan, &scope, plane.readers())
        .unwrap_or_else(|e| panic!("the recorded plane retrieves: {e}"));
    let items = retrieval
        .items
        .iter()
        .map(|item| {
            RetrievedObservation::capture(item, cut_off)
                .unwrap_or_else(|e| panic!("every recorded item is captured: {e}"))
                .0
        })
        .collect();
    let retrieved = lp.run(
        ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items,
        },
        ports,
        world,
    );
    assert!(
        retrieved
            .requests
            .iter()
            .any(|request| matches!(request, ResearchRequest::Call(_))),
        "the core asks for its call: {retrieved:?}"
    );
    lp.run(
        ResearchInput::Refused {
            run_id,
            at: answered_at,
            error,
        },
        ports,
        world,
    );
}

/// A whole run: the worker's run, answered 30 seconds after the start, then a poll.
pub fn full_run(
    lp: &mut Loop,
    now: RiskClock,
    ports: &Ports<'_>,
    world: &World,
    plane: &RecordedPlane,
    model: &Model,
) {
    let answered = clock(now.secs().saturating_add(30));
    worker_run(lp, now, ports, world, plane, model, answered);
    lp.run(
        ResearchInput::Poll(clock(now.secs().saturating_add(31))),
        ports,
        world,
    );
}

/// An order plan over the real builder: one research model at weight 1, `conviction_linear` with
/// thresholds 0.3 and band 0.05, the four limits, a flat 10000 USD agent, and a policy whose rules
/// make every opening AUTO, so only the admission ceiling can make the first order an ASK.
pub struct BuilderPlan {
    pub mandate: BuilderMandate,
    pub policy: Policy,
    pub bid: Price,
    pub ask: Price,
    last: RefCell<Option<ActionContext>>,
}

impl BuilderPlan {
    pub fn new() -> Self {
        let fraction = |t: &str| SizeFraction::parse(t).unwrap_or_else(|e| panic!("{e}"));
        Self {
            mandate: BuilderMandate {
                models: vec![SignalModel {
                    id: ModelId::parse(RESEARCH_MODEL).unwrap_or_else(|e| panic!("{e}")),
                    version: BuilderVersion::parse("0.1.0").unwrap_or_else(|e| panic!("{e:?}")),
                    content_hash: pin().content_hash,
                    weight: fraction("1"),
                    max_output_age_s: 86_400,
                }],
                sizing: Sizing {
                    method: SizingMethod::ConvictionLinear,
                    entry_threshold: fraction("0.3"),
                    exit_threshold: fraction("0.3"),
                    rebalance_band: fraction("0.05"),
                },
                limits: Limits {
                    max_position_usd: usd(MAX_POSITION_USD),
                    max_position_fraction: fraction("0.2"),
                    max_order_usd: usd(MAX_ORDER_USD),
                    max_gross_exposure_usd: usd("2000"),
                },
                goal: GoalKind::Continuous,
            },
            policy: Policy {
                rules: vec![Rule {
                    id: RuleId::parse("routine").unwrap_or_else(|e| panic!("{e}")),
                    when: Condition::Compare {
                        field: ConditionField::Purpose,
                        op: Operator::In,
                        value: ConditionValue::List(vec!["increase".to_owned(), "open".to_owned()]),
                    },
                    then: AutonomyDecision::Auto,
                }],
                default: AutonomyDecision::Auto,
                admission: AutonomyDecision::Ask,
                approval: Approval {
                    timeout_s: 600,
                    on_timeout: OnTimeout::Skip,
                    approvers: vec![
                        ApproverRef::parse("role:approver").unwrap_or_else(|e| panic!("{e}")),
                    ],
                    two_approver_above_usd: None,
                },
                review_by: None,
                delegations: Vec::new(),
                tripwires: Vec::new(),
            },
            bid: price("235.4"),
            ask: price("235.5"),
            last: RefCell::new(None),
        }
    }

    fn builder_outputs(inputs: &SignalInputs, instrument: &InstrumentId) -> Vec<BuilderOutput> {
        inputs
            .outputs
            .values()
            .filter_map(|by_instrument| by_instrument.get(instrument))
            .filter(|output| output.ignored.is_none() && output.direction == ModelDirection::Long)
            .filter_map(|output| {
                Some(BuilderOutput {
                    model_id: ModelId::parse(&output.model_id).ok()?,
                    model_version: BuilderVersion::parse(&output.model_version).ok()?,
                    content_hash: output.content_hash,
                    instrument: DomainAsset::parse(output.instrument_id.as_str()).ok()?,
                    as_of: UtcNanos::from_parts(output.as_of.secs(), 0).ok()?,
                    expires_at: UtcNanos::from_parts(output.expires_at.secs(), 0).ok()?,
                    direction: mandate_builder::Direction::Long,
                    conviction: output.conviction,
                    confidence: output.confidence,
                })
            })
            .collect()
    }
}

impl Default for BuilderPlan {
    fn default() -> Self {
        Self::new()
    }
}

impl OrderPlan for BuilderPlan {
    fn plan(&self, view: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        let now = UtcNanos::from_parts(inputs.now.secs(), 0).ok()?;
        for instrument in &view.working_universe {
            let outputs = Self::builder_outputs(inputs, instrument);
            let asset = DomainAsset::parse(instrument.as_str()).ok()?;
            let market = Market {
                instrument: asset,
                asset_class: AssetClass::UsEquity,
                session: MarketSession::Regular,
                in_close_window: false,
                bid: self.bid,
                ask: self.ask,
                increment: Qty::parse("1").ok()?,
                min_order_usd: usd("1"),
                fee_rate_cash: FeeRate::parse("0").ok()?,
                fee_rate_asset: FeeRate::parse("0").ok()?,
            };
            let account = AccountSnapshot {
                agent_equity: usd(EQUITY_USD),
                position_qty: Qty::parse("0").ok()?,
                cost_basis: CostBasis::parse("0").ok()?,
                risk_mark: MarkPrice::parse(&self.bid.to_string()).ok()?,
                gross_usd: usd("0"),
                working_opening_cost: usd("0"),
                goal_spent_usd: usd("0"),
            };
            let confidence = outputs.first().map_or(Unit::ZERO, |o| o.confidence);
            let risk = RiskContext {
                size_factor: SizeFraction::ONE,
                drawdown: Unit::ZERO,
                daily_pnl_fraction: Signed::ZERO,
                position_pnl_fraction: Signed::ZERO,
                bought_today_usd: usd("0"),
                has_prior_fill: false,
                new_instrument: true,
                thesis_confidence: confidence,
                risk_day: Date::parse("2026-10-19").ok()?,
            };
            let proposal = propose(&self.mandate, &account, &market, &risk, &outputs, now).ok()?;
            if let Action::Buy {
                qty,
                limit_price,
                action,
                ..
            } = proposal.action
            {
                *self.last.borrow_mut() = Some(action);
                let text = |s: String| Value::Str(s);
                return Some(Proposal {
                    instrument: instrument.clone(),
                    asset_class: AccountingClass::UsEquity,
                    side: Side::Buy,
                    qty,
                    limit: limit_price,
                    purpose: Purpose::Open,
                    exit_origin: None,
                    exit_conviction: Some(text(proposal.combined.exit_conviction.to_string())),
                    buy_conviction: Some(text(proposal.combined.buy_conviction.to_string())),
                    combined_score: text(proposal.combined.score.to_string()),
                    outputs_used: proposal
                        .combined
                        .outputs_used
                        .iter()
                        .map(|id| id.as_str().to_owned())
                        .collect(),
                    model_weights: BTreeMap::from([(
                        RESEARCH_MODEL.to_owned(),
                        text("1".to_owned()),
                    )]),
                    clips_applied: if proposal.clipped_by.is_empty() {
                        Vec::new()
                    } else {
                        vec![DecisionClip::MaxOrderUsd]
                    },
                    execution: Some(OrderExecution {
                        asset_class: AccountingClass::UsEquity,
                        tif: TimeInForce::Day,
                        protection_required: false,
                        protection: None,
                    }),
                });
            }
        }
        None
    }

    fn classify(&self, _view: &MandateView, _proposal: &Proposal) -> Classified {
        let Some(action) = self.last.borrow().clone() else {
            return Classified {
                autonomy: Autonomy::Deny,
                decided_by: None,
            };
        };
        match classify(&self.policy, &action) {
            Ok(classification) => Classified {
                autonomy: match classification.decision {
                    AutonomyDecision::Auto => Autonomy::Auto,
                    AutonomyDecision::Ask => Autonomy::Ask,
                    AutonomyDecision::Deny => Autonomy::Deny,
                },
                decided_by: Some(classification.by.label()),
            },
            Err(_) => Classified {
                autonomy: Autonomy::Deny,
                decided_by: None,
            },
        }
    }
}

/// The trading loop's view once the executor has copied the admissions.
pub fn view_of(world: &World) -> MandateView {
    let instruments = match &world.universe {
        WorkingUniverse::Known { instruments, .. } => instruments
            .iter()
            .map(|id| InstrumentId::new(id.as_str()).unwrap_or_else(|e| panic!("{e}")))
            .collect(),
        WorkingUniverse::Unavailable => BTreeSet::new(),
    };
    MandateView {
        working_universe: instruments,
        ..super::universe(&[])
    }
}

/// The `UniverseChanged` copies the executor would make: every admitted thesis record's instrument.
pub fn copy_admissions(lp: &Loop, world: &mut World) {
    for event in lp.events("ThesisProposed") {
        if matches!(event.payload.get("admitted"), Some(Value::Bool(true))) {
            let instrument = event
                .payload
                .get("instrument_id")
                .and_then(Value::as_str)
                .unwrap_or_else(|| panic!("a thesis record names its instrument"));
            world.admit(instrument);
        }
    }
}

/// The gateway's parts as a demo cell configures them: the research entry on its one OpenRouter
/// endpoint, the $50 monthly cap, and a manual clock at `now`.
pub fn demo_parts(now: UtcNanos) -> GatewayParts<MemoryMeter, ManualClock> {
    GatewayParts {
        settings: Settings {
            workspace_id: super::WORKSPACE.to_owned(),
            policy: EndpointPolicy::new(
                BTreeSet::from([Locality::Hosted]),
                Some(BTreeSet::from([UPSTREAM.to_owned()])),
                None,
            ),
            guard: GuardContext::default(),
            rate: RateCaps {
                per_workspace: 100,
                per_agent: 100,
            },
        },
        registry: registry(),
        meter: Meter::new(MemoryMeter::new(), budget(), deployment_meter()),
        clock: ManualClock::at(now),
        ids: Box::new(SequentialIds::starting_at(1 << 80)),
    }
}

/// A transport replaying the pinned model's reply carrying `output`, once.
pub fn scripted(output: &str) -> RecordedTransport {
    let mut transport = RecordedTransport::new();
    transport.script(vec![delivered(200, reply(output), LATENCY_MS)]);
    transport
}

/// A key source holding one fixture key, never a real credential: the vault's stand-in.
pub struct FixtureVault;

pub const FIXTURE_KEY: &str = "fixture-key-not-a-secret-0001";

impl SecretSource for FixtureVault {
    fn provider_key(&self, upstream: &str) -> Result<ProviderKey, SecretError> {
        if upstream == UPSTREAM {
            Ok(ProviderKey::new(FIXTURE_KEY.to_owned()))
        } else {
            Err(SecretError::Missing)
        }
    }
}

/// A stand-in for the cell's hosted transport, with no network: it replays like the recorded fake
/// and keeps every key it was handed, so a test can show which transport a call went through.
#[derive(Default)]
pub struct HostedDouble {
    pub inner: RecordedTransport,
    pub keys: Vec<String>,
}

impl Transport for HostedDouble {
    fn send(
        &mut self,
        request: &HttpRequest,
        key: Option<&ProviderKey>,
        deadline: UtcNanos,
        clock: &mut dyn Clock,
    ) -> Delivery {
        self.keys
            .extend(key.map(|key| key.expose_secret().to_owned()));
        self.inner.send(request, key, deadline, clock)
    }
}

/// A workspace's monthly model budget for the tests, a setting (DEC-2207 item 1): $50 here.
pub fn budget() -> Caps {
    Caps::new(Usd6::dollars(50).unwrap_or_else(|e| panic!("{e}")))
}

/// A deployment meter of its own, with a cap far above any workspace's, so the tests meet only the
/// workspace's caps (DEC-2207).
pub fn deployment_meter() -> DeploymentMeter {
    DeploymentMeter::new(
        Box::new(MemoryMeter::new()),
        Usd6::dollars(1_000_000).unwrap_or_else(|e| panic!("{e}")),
        Box::new(SequentialIds::starting_at(1 << 100)),
    )
}
