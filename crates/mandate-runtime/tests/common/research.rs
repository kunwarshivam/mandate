//! The research loop's harness: the in-memory shell extended with a [`ResearchState`], the worker
//! over recorded fixtures, a recorded gateway, the research agent's admission context, and an order
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
use mandate_canon::{Digest, Value, parse};
use mandate_domain::{AssetClass, AssetId as DomainAsset, AutonomyDecision, MarketSession};
use mandate_num::{CostBasis, FeeRate, MarkPrice, Price, Qty, Signed, SizeFraction, Unit, Usd};
use mandate_research::{
    AdmissionFacts, AllowlistVersion, AssetId, LineageState, MandateEnvelope, PolicyOverlay,
    ResearchEnvelope, SourceAllowlist, SourceId as ResearchSource, ValidatedMandate,
    WorkingUniverse,
};
use mandate_research_run::{
    Allowlist, Bar, BarReader, CallContext, CallOutcome, Filing, FilingReader, GatewayReply,
    InstrumentFact, InstrumentReader, ItemId, ModelGateway, ModelPin, NewsItem, NewsReader,
    OutputBounds, ReadError, Readers, ResearchCall, RetrievalPlan, Scope, SourceId, Symbol,
    TextQuery, retrieve, settle,
};
use mandate_runtime::{
    ActiveThesis, ActorKind, Autonomy, Classified, DecisionClip, Effect, EventDraft, FoldedEvent,
    JournalCatalog, MandateView, ModelDirection, OrderExecution, OrderPlan, Ports, Preconditions,
    Proposal, Purpose, RecordCatalog, ResearchConfig, ResearchContext, ResearchInput,
    ResearchRecord, ResearchRequest, ResearchState, ResearchStep, RetrievedObservation, RiskClock,
    RuntimeError, Seq, SignalInputs, TimeInForce, fold, handle_research,
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

pub fn pin() -> ModelPin {
    ModelPin {
        model_id: RESEARCH_MODEL.to_owned(),
        model_version: "0.1.0".to_owned(),
        content_hash: Digest::of(b"llm.research_agent:0.1.0"),
    }
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

/// A gateway double that answers every call with one recorded outcome. `answer` builds it from the
/// request, so a test can choose a reply, a failure, a late reply, or another identity.
pub struct RecordedGateway<F: Fn(&ResearchCall) -> CallOutcome> {
    pub answer: F,
    pub calls: RefCell<Vec<ResearchCall>>,
}

impl<F: Fn(&ResearchCall) -> CallOutcome> ModelGateway for RecordedGateway<F> {
    fn call(&self, request: &ResearchCall) -> CallOutcome {
        self.calls.borrow_mut().push(request.clone());
        (self.answer)(request)
    }
}

/// An `ok` reply carrying `output`, from the pin, 30 seconds after dispatch.
pub fn reply_with(request: &ResearchCall, output: &[u8]) -> GatewayReply {
    GatewayReply {
        call_id: request.call_id.clone(),
        reported: request.model.clone(),
        output: output.to_vec(),
        request_digest: Digest::of(request.call_id.as_bytes()),
        prompt_ref: Digest::of(b"prompt"),
        response_ref: Digest::of(output),
        cost_usd: usd("0.03"),
        completed_at: at(request.deadline.secs().saturating_sub(90)),
    }
}

pub fn recorded<'a>(
    output: &'a str,
) -> RecordedGateway<impl Fn(&ResearchCall) -> CallOutcome + 'a> {
    RecordedGateway {
        answer: move |request: &ResearchCall| Ok(reply_with(request, output.as_bytes())),
        calls: RefCell::new(Vec::new()),
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
    Preconditions::read(&JournalCatalog)
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
/// retrieval, the captured observations handed in, the call request's call, and its settled
/// answer handed in. Returns the requests the core emitted on the way.
pub fn worker_run(
    lp: &mut Loop,
    now: RiskClock,
    ports: &Ports<'_>,
    world: &World,
    plane: &RecordedPlane,
    gateway: &dyn ModelGateway,
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
    let call = ResearchCall::build(&plan, &retrieval, world.call_context(), at(cut_off.secs()))
        .unwrap_or_else(|e| panic!("the call builds from its journaled items: {e}"));
    let outcome = settle(&call, gateway.call(&call));
    let called = lp.run(
        ResearchInput::Called {
            run_id,
            at: answered_at,
            outcome,
        },
        ports,
        world,
    );
    seen.extend(called.requests.clone());
    seen
}

/// A whole run and its judging: the worker's run, then the poll that judges the committed call.
pub fn full_run(
    lp: &mut Loop,
    now: RiskClock,
    ports: &Ports<'_>,
    world: &World,
    plane: &RecordedPlane,
    gateway: &dyn ModelGateway,
) {
    let answered = clock(now.secs().saturating_add(30));
    worker_run(lp, now, ports, world, plane, gateway, answered);
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
