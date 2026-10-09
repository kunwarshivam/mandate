//! The tracer end to end, through the production adapters, over recorded Alpaca paper fixtures and
//! a bar dataset written by `mandate-marketdata`'s own writer (task brief, "The CI end-to-end
//! test"). No test here touches a network: the transport is scripted, and `AlpacaPaperHttp` is never
//! constructed (ADR-0001 ES-19).
//!
//! The full production path runs against recorded fixtures. The one pending outlier case remains a
//! failing E2-14 test until its founder-gated price-trust rule lands; the shell cannot invent price
//! arithmetic (DEC-138 item 3).
//!
//! The fail-closed suite is not here. It lives in `src/stages/fail_closed.rs`, because its
//! permissive doubles must not be reachable from a build that ships (task brief item 5).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use mandate_accounting::{InstrumentId as RuntimeInstrumentId, Side};
use mandate_alpaca::{HttpRequest, Method, Response, TradingTransport, TransportError};
use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, ActionContext, Market as BuilderMarket, RequestedBy,
    RiskContext as BuilderRiskContext,
};
use mandate_canon::{DecStr, Digest, Value};
use mandate_domain::{AssetClass as DomainAssetClass, AssetId, MarketSession, Purpose};
use mandate_journal::{
    AppendOutcome, ArtifactSource, Environment, StoredEvent, TrustedStart, verify_events,
};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_num::{
    Adverse, Conviction, CostBasis, FeeRate, Fraction, MarkPrice, Price, Qty, Ratio,
    ShareIncrement, Signed, SizeFraction, Unit, Usd,
};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AccountState as GateAccountState, AgentId as GateAgentId, AgentMode,
    AgentSnapshot, AssetId as GateAssetId, ClientOrderId as GateOrderId, ConductState,
    DayTradeLedger, DayTradeRegime, EtpClass, Exchange, GateConfig, GatePass,
    InstrumentSnapshot as GateInstrumentSnapshot, MarketSnapshot, Origin, ProposedKind,
    QuoteCurrency, RiskSnapshot, SaneQuote, TimeInForce as GateTimeInForce,
    ValidatedMandate as GateMandate, WorkingUniverse,
};
use mandate_runtime::{
    AgentId, Autonomy, Classified, ConnectionId, Deployment, ExitOrigin, MandateView, ModelOutput,
    OrderExecution, Proposal, ProtectionPrices, Purpose as RuntimePurpose, SignalInputs,
    TimeInForce, WorkspaceId,
};
use mandate_shell::adapters::{
    AdvisoryGateContext, AdvisoryOrderFacts, AlpacaConnector, BuilderContext, BuilderPlan,
    DecisionContext, ExecutorContext, RunContext, Sources, SpecMandate, production,
};
use mandate_shell::control::{Governance, Registered};
use mandate_shell::envelope::{IdSpace, Ids};
use mandate_shell::stages::{Classifier, JournalWriter, MandateSource, Sizing, Stage, Stages};
use mandate_shell::{Cause, ProductionCycle, Report, Setup, ShellError, production_cycle, run};
use mandate_spec::ValidationContext;
use mandate_spec::document::ProvenanceMap;
use mandate_spec::policy::{LevelName, PolicyKey, PolicyLevel, PolicyValue};
use mandate_time::{Date, UtcNanos};

const AGENT: &str = "tracer-aapl";
const WORKSPACE: &str = "tracer";
const ACCOUNT_REF: &str = "tracer-paper";
const NOW: &str = "2026-09-25T20:00:00.000000000Z";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tracer")
}

fn model_artifact() -> (Digest, Vec<u8>) {
    let bytes = fs::read(fixtures().join("model-artifact.json"))
        .unwrap_or_else(|error| panic!("the deterministic model artifact is readable: {error}"));
    (Digest::of(&bytes), bytes)
}

fn executor_artifacts(
    mandate: &str,
) -> (
    mandate_executor::BindingGateConfigRefs,
    BTreeMap<Digest, Vec<u8>>,
) {
    let mandate_source = fs::read(fixtures().join(mandate)).unwrap();
    let mandate_value = mandate_canon::parse(&mandate_source).unwrap();
    let mandate_document = mandate_spec::Mandate::parse(&mandate_value).unwrap();
    let mandate_bytes = mandate_document.canonical_bytes().unwrap();
    let config = fixtures().join("config");
    let named = [
        (
            "fee_config",
            fs::read(config.join("fee-config.json")).unwrap(),
        ),
        (
            "trading_calendar",
            fs::read(config.join("trading-calendar.json")).unwrap(),
        ),
        (
            "instrument_snapshot",
            fs::read(config.join("instrument-snapshot.json")).unwrap(),
        ),
        ("rule_set", fs::read(config.join("rule-set.json")).unwrap()),
        ("mandate_version", mandate_bytes),
    ];
    let refs = named
        .iter()
        .map(|(name, bytes)| ((*name).to_owned(), Digest::of(bytes)))
        .collect::<BTreeMap<_, _>>();
    let artifacts = named
        .into_iter()
        .map(|(_, bytes)| (Digest::of(&bytes), bytes))
        .collect();
    let reference = |name: &str| {
        format!(
            "sha256:{}",
            refs.get(name)
                .unwrap_or_else(|| panic!("{name} fixture digest"))
                .to_hex()
        )
    };
    (
        mandate_executor::BindingGateConfigRefs::complete(
            reference("fee_config"),
            reference("trading_calendar"),
            reference("instrument_snapshot"),
            reference("rule_set"),
            reference("mandate_version"),
        ),
        artifacts,
    )
}

fn agent_stream() -> String {
    format!("agent:{WORKSPACE}:{AGENT}")
}

fn account_stream() -> String {
    format!("acct:{WORKSPACE}:{ACCOUNT_REF}")
}

fn run_context(with_builder: bool) -> RunContext {
    let validation = ValidationContext {
        account_equity_usd: Usd::parse("100000").unwrap(),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-25").unwrap(),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: Default::default(),
        instrument_groups: Default::default(),
        claimed_by_other_agents: Default::default(),
        connection_environment: Some(mandate_domain::Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: Default::default(),
        previous_version: None,
        current_mandate_version: None,
    };
    let decision = with_builder.then(|| {
        let instrument = AssetId::parse("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415").unwrap();
        let builder = BuilderContext {
            account: BuilderAccountSnapshot {
                agent_equity: Usd::parse("1000").unwrap(),
                position_qty: Qty::ZERO,
                cost_basis: CostBasis::ZERO,
                risk_mark: MarkPrice::parse("255.1").unwrap(),
                gross_usd: Usd::ZERO,
                working_opening_cost: Usd::ZERO,
                goal_spent_usd: Usd::ZERO,
            },
            market: BuilderMarket {
                instrument: instrument.clone(),
                asset_class: DomainAssetClass::UsEquity,
                session: MarketSession::Regular,
                in_close_window: false,
                bid: Price::parse("255.1").unwrap(),
                ask: Price::parse("255.2").unwrap(),
                increment: Qty::parse("1").unwrap(),
                min_order_usd: Usd::parse("1").unwrap(),
                fee_rate_cash: FeeRate::parse("0").unwrap(),
                fee_rate_asset: FeeRate::parse("0").unwrap(),
            },
            risk: BuilderRiskContext {
                size_factor: SizeFraction::ONE,
                drawdown: Unit::ZERO,
                daily_pnl_fraction: Signed::ZERO,
                position_pnl_fraction: Signed::ZERO,
                bought_today_usd: Usd::ZERO,
                has_prior_fill: false,
                new_instrument: false,
                thesis_confidence: Unit::ZERO,
                risk_day: Date::parse("2026-09-25").unwrap(),
            },
            action: ActionContext {
                purpose: Purpose::Open,
                order_usd: Usd::parse("255.2").unwrap(),
                combined_score: Unit::ONE,
                instrument,
                asset_class: DomainAssetClass::UsEquity,
                session: MarketSession::Regular,
                first_trade_in_instrument: true,
                new_instrument: false,
                thesis_confidence: Unit::ZERO,
                drawdown: Unit::ZERO,
                daily_pnl_fraction: Signed::ZERO,
                position_usd_after: Usd::parse("255.2").unwrap(),
                gross_usd_after: Usd::parse("255.2").unwrap(),
                bought_today_usd: Usd::parse("255.2").unwrap(),
                position_pnl_fraction: Signed::ZERO,
                requested_by: RequestedBy::Agent,
                risk_day: Date::parse("2026-09-25").unwrap(),
            },
            model_content_hashes: BTreeMap::from([(
                ("quant.ma_crossover".to_owned(), "1.0.0".to_owned()),
                model_artifact().0,
            )]),
            execution: OrderExecution {
                asset_class: DomainAssetClass::UsEquity,
                tif: TimeInForce::Day,
                protection_required: true,
                protection: Some(ProtectionPrices {
                    stop: Price::parse("242.44").unwrap(),
                    take_profit: Some(Price::parse("280.72").unwrap()),
                }),
            },
        };
        DecisionContext {
            builder: Some(builder),
            gate: Some(advisory_gate_context()),
        }
    });
    RunContext {
        validation,
        policies: Vec::new(),
        author: "user-author".to_owned(),
        restricted_instruments: Default::default(),
        decision,
        governance: None,
    }
}

/// Trusted, effective-dated facts for the executor half of the full tracer fixture. The binding
/// gate receives snapshots only; `mandate-executor` still constructs the proposal and calls
/// `mandate-risk::evaluate` itself.
struct TrustedExecutorFixture {
    config_refs: mandate_executor::BindingGateConfigRefs,
}

impl mandate_executor::MandateView for TrustedExecutorFixture {
    fn version(
        &self,
        _agent: &mandate_executor::AgentId,
    ) -> Option<mandate_executor::MandateVersion> {
        self.config_refs
            .mandate_version
            .clone()
            .map(mandate_executor::MandateVersion)
    }

    fn crypto_stop_limit_offset(&self, _agent: &mandate_executor::AgentId) -> Option<Fraction> {
        None
    }

    fn covers(
        &self,
        agent: &mandate_executor::AgentId,
        instrument: &mandate_accounting::InstrumentId,
    ) -> bool {
        agent.0 == AGENT && instrument.as_str() == "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"
    }
}

impl mandate_executor::InstrumentSnapshot for TrustedExecutorFixture {
    fn asset_class(
        &self,
        instrument: &mandate_accounting::InstrumentId,
    ) -> Option<mandate_accounting::AssetClass> {
        (instrument.as_str() == "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
            .then_some(mandate_accounting::AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &mandate_accounting::InstrumentId) -> Option<ShareIncrement> {
        (instrument.as_str() == "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415")
            .then_some(ShareIncrement::Whole)
    }

    fn exit_tier(
        &self,
        _instrument: &mandate_accounting::InstrumentId,
    ) -> Option<mandate_executor::ExitTier> {
        None
    }
}

impl mandate_executor::BindingGateSource for TrustedExecutorFixture {
    fn input(
        &self,
        request: &mandate_executor::BindingGateRequest<'_>,
    ) -> Option<mandate_executor::BindingGateInput> {
        let asset = GateAssetId::new(request.instrument.as_str()).ok()?;
        let now = UtcNanos::parse("2026-09-25T19:00:00.000000000Z").ok()?;
        let quote_at = UtcNanos::parse("2026-09-25T18:59:59.000000000Z").ok()?;
        let volume = Qty::parse("1000000").ok()?;
        let equity = Usd::parse("1000000").ok()?;
        let positions = BTreeMap::new();
        let gate_agent = GateAgentId(1);
        Some(mandate_executor::BindingGateInput {
            config_refs: self.config_refs.clone(),
            now,
            config: GateConfig {
                price_floor: Usd::parse("0.01").ok()?,
                liquidity_floor_usd: Usd::ZERO,
                crypto_liquidity_floor_usd: Usd::ZERO,
                collar_liquid_threshold_usd: Usd::ZERO,
                collar_liquid_x: Fraction::parse("0.5").ok()?,
                collar_other_x: Fraction::parse("0.5").ok()?,
                collar_crypto_x: Fraction::parse("0.5").ok()?,
                collar_passive_band: Fraction::parse("0.5").ok()?,
                opposite_fill_interval_s: 0,
                min_resting_time_s: 0,
                order_to_fill_max: u32::MAX,
                order_to_fill_min_orders: u32::MAX,
                order_size_participation: Fraction::ONE,
                daily_participation: Fraction::ONE,
                close_window_minutes: 0,
                legacy_pdt_equity_threshold: Usd::ZERO,
                etp_classification_max_age_s: u32::MAX,
            },
            mandate: GateMandate::from_validated_parts(
                RiskLimits {
                    max_position_usd: equity,
                    max_position_fraction: Fraction::ONE,
                    max_order_usd: equity,
                    max_gross_exposure_usd: equity,
                    max_orders_per_day: u32::MAX,
                    reentry_cooldown_s: 0,
                    rebalance_band: Fraction::ONE,
                    breach_confirm_s: 0,
                    drawdown_ladder: Vec::new(),
                },
                GoalState::Running,
                true,
                true,
            ),
            risk: RiskSnapshot {
                agent_equity: equity,
                high_water_mark: equity,
                day_start_equity: equity,
                capital_base: equity,
                inherited_loss: Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: Ratio::parse("1").ok()?,
                agent_mode: AgentMode::Normal,
            },
            account: AccountSnapshot {
                account_type: mandate_accounting::AccountType::Margin,
                state: GateAccountState::Active,
                crypto_active: true,
                regime: DayTradeRegime::IntradayMargin {
                    maintenance_excess: equity,
                },
                equity,
                prior_close_equity: equity,
                model_buying_power: equity,
                broker_buying_power: equity,
                broker_non_marginable_buying_power: equity,
                positions: positions.clone(),
                market_values: BTreeMap::new(),
                working_orders: BTreeMap::new(),
                unknown_orders: BTreeSet::new(),
                related_account_resting: BTreeMap::new(),
            },
            agent: AgentSnapshot {
                agent: gate_agent,
                mode: AgentMode::Normal,
                instrument_restrictions: BTreeMap::new(),
                positions,
                market_values: BTreeMap::new(),
                working_orders: BTreeSet::new(),
                instrument_groups: BTreeMap::new(),
                last_exit_fill_at: BTreeMap::new(),
                orders_today: 0,
                day_trades: DayTradeLedger::default(),
            },
            instrument: GateInstrumentSnapshot {
                instrument: asset.clone(),
                asset_class: mandate_risk::AssetClass::UsEquity,
                exchange: Some(Exchange::Nasdaq),
                status_active: true,
                tradable: true,
                fractionable: true,
                ipo: false,
                ptp_no_exception: false,
                etp: EtpClass::Plain,
                etp_classified_at: Some(now),
                quote_currency: Some(QuoteCurrency::Usd),
                prior_close: Some(request.limit),
                median_dollar_volume_20d: Some(equity),
                median_dollar_volume_30d: Some(equity),
                min_order_size: Qty::parse("0.000000001").ok()?,
                qty_increment: Qty::parse("0.000000001").ok()?,
                halted: false,
                status_feed_current: true,
            },
            market: MarketSnapshot {
                quote: Some(SaneQuote {
                    bid: request.limit,
                    ask: request.limit,
                    at: quote_at,
                }),
                last_trade: Some((request.limit, quote_at)),
                trailing_5m_volume: Some(volume),
                adv_20d: Some(volume),
            },
            conduct: ConductState::default(),
            universe: WorkingUniverse::Known {
                instruments: BTreeSet::from([asset.clone()]),
                pinned: true,
            },
            asset,
            gate_agent,
            gate_client_order_id: GateOrderId(1),
            owner_confirmed_bid: None,
            fee_reservation: Usd::ZERO,
            data_profile: "tracer-fixture".to_owned(),
            feed: "iex-fixture".to_owned(),
        })
    }
}

fn executor_context(mandate: &str) -> ExecutorContext {
    let first = Date::parse("2026-01-01").unwrap();
    let last = Date::parse("2026-12-31").unwrap();
    let calendar = mandate_time::TradingCalendar::new(first, last, [], []).unwrap();
    let fees = mandate_executor::paper_only_fee_config("paper", calendar, "2026-01-01").unwrap();
    let source = Rc::new(TrustedExecutorFixture {
        config_refs: executor_artifacts(mandate).0,
    });
    ExecutorContext::new(
        Rc::new(Ids {
            space: IdSpace::Account,
        }),
        source.clone(),
        source.clone(),
        source,
        mandate_executor::ExecutorConfig::PROPOSED,
        fees,
    )
}

fn advisory_gate_context() -> AdvisoryGateContext {
    let instrument =
        mandate_accounting::InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415").unwrap();
    let agent = mandate_executor::AgentId(AGENT.to_owned());
    let limit = Price::parse("255.2").unwrap();
    let request = mandate_executor::BindingGateRequest {
        agent: &agent,
        instrument: &instrument,
        side: mandate_accounting::Side::Buy,
        qty: Qty::parse("1").unwrap(),
        limit,
        purpose: mandate_executor::Purpose::Open,
        tif: mandate_executor::TimeInForce::Day,
        protection: None,
    };
    let fixture = TrustedExecutorFixture {
        config_refs: executor_artifacts("mandate.json").0,
    };
    let trusted = mandate_executor::BindingGateSource::input(&fixture, &request)
        .expect("the trusted executor fixture supplies the same opening gate facts");
    AdvisoryGateContext {
        now: trusted.now,
        pass: GatePass::First,
        config: trusted.config,
        mandate: trusted.mandate,
        risk: trusted.risk,
        account: trusted.account,
        agent: trusted.agent,
        instrument: trusted.instrument,
        market: trusted.market,
        conduct: trusted.conduct,
        universe: trusted.universe,
        order: AdvisoryOrderFacts {
            kind: ProposedKind::Plain,
            tif: GateTimeInForce::Day,
            extended_hours: false,
            origin: Origin::OrderBuilder,
            owner_confirmed_bid: None,
            client_order_id: GateOrderId(1),
            fee_reservation: Usd::ZERO,
        },
    }
}

/// A scratch directory under the system's temporary directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "mandate-shell-tracer-{name}-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn dec(text: &str) -> DecStr {
    DecStr::parse(text).unwrap()
}

/// Writes one daily bar per weekday, starting 2026-08-24, with these closes, through
/// `mandate-marketdata`'s own writer, so the tracer reads the real format.
fn bars(dir: &Path, closes: &[&str]) -> PathBuf {
    bars_from(dir, "2026-08-24", closes)
}

fn bars_from(dir: &Path, first: &str, closes: &[&str]) -> PathBuf {
    let dataset = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars("1Day".parse().unwrap()),
        Symbol::parse("AAPL").unwrap(),
    )
    .unwrap();
    let store = Store::new(dir);
    let mut day = Date::parse(first).unwrap();
    for close in closes {
        while day.is_weekend() {
            day = day.next().unwrap();
        }
        let bar = Bar {
            start: UtcNanos::parse_rfc3339(&format!("{day}T04:00:00Z")).unwrap(),
            open: dec(close),
            high: dec(close),
            low: dec(close),
            close: dec(close),
            volume: dec("50000000"),
            vwap: dec(close),
            trade_count: 400_000,
        };
        store
            .put_day(&dataset, day, &Records::Bars(vec![bar]))
            .unwrap();
        day = day.next().unwrap();
    }
    store.dataset_dir(&dataset)
}

/// Twenty-five closes rising one dollar a day to the fixture's 255.20, so the five-day average
/// is above the twenty-day one at the last close: E4-2's baseline says `Long`.
fn rising() -> Vec<String> {
    (0..25).map(|i| format!("{}.20", 231 + i)).collect()
}

fn falling() -> Vec<String> {
    (0..25).map(|i| format!("{}.20", 279 - i)).collect()
}

/// What the scripted broker answers, by endpoint rather than by position, so the test pins what
/// the tracer asks for without pinning the order the executor asks it in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Broker {
    /// A fresh paper account: active, flat, nothing open; a submission is accepted.
    Fresh,
    /// The broker already holds one share of AAPL.
    HoldsAapl,
    /// The submission times out, and the order query finds nothing.
    TimeoutThenAbsent,
}

#[derive(Default)]
struct Seen {
    requests: Vec<(Method, String, Option<String>)>,
}

impl Seen {
    fn posts(&self) -> usize {
        self.requests
            .iter()
            .filter(|(method, path, _)| *method == Method::Post && path == "/v2/orders")
            .count()
    }

    fn order_queries(&self) -> usize {
        self.requests
            .iter()
            .filter(|(_, path, _)| path.starts_with("/v2/orders:by_client_order_id"))
            .count()
    }
}

/// The scripted transport. It counts every `POST /v2/orders` it is handed before `mandate-alpaca`
/// interprets anything, which is the submission oracle the shell cannot reach.
#[derive(Clone)]
struct Scripted {
    broker: Broker,
    seen: Rc<RefCell<Seen>>,
}

impl Scripted {
    fn new(broker: Broker) -> Scripted {
        Scripted {
            broker,
            seen: Rc::default(),
        }
    }

    fn answer(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let path = request.path_and_query();
        let file = |name: &str| fs::read(fixtures().join("alpaca").join(name)).unwrap();
        let ok = |body: Vec<u8>| Ok(Response { status: 200, body });
        if path == "/v2/account" {
            return ok(file("account.json"));
        }
        if path == "/v2/positions" {
            return match self.broker {
                Broker::HoldsAapl => ok(file("position-aapl.json")),
                Broker::Fresh | Broker::TimeoutThenAbsent => ok(b"[]".to_vec()),
            };
        }
        if path.starts_with("/v2/orders?") {
            return ok(b"[]".to_vec());
        }
        if path.starts_with("/v2/account/activities?") {
            return ok(b"[]".to_vec());
        }
        if path == "/v2/orders" && request.method() == Method::Post {
            if self.broker == Broker::TimeoutThenAbsent {
                return Err(TransportError::Timeout);
            }
            let sent: Value = mandate_canon::parse(request.body().unwrap().as_bytes()).unwrap();
            let id = sent.get("client_order_id").and_then(Value::as_str).unwrap();
            let template = String::from_utf8(file("order.json")).unwrap();
            let body = template.replace("md-e144b97773a6f87c1978cc2831", id);
            return ok(body.into_bytes());
        }
        if path.starts_with("/v2/orders:by_client_order_id") {
            return Ok(Response {
                status: 404,
                body: file("order-absent.json"),
            });
        }
        panic!("the scripted broker has no answer for {path}");
    }
}

impl TradingTransport for Scripted {
    fn send(
        &self,
        request: &HttpRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        self.seen.borrow_mut().requests.push((
            request.method(),
            request.path_and_query().to_owned(),
            request.body().map(str::to_owned),
        ));
        let answer = self.answer(request);
        async move { answer }
    }
}

#[derive(Clone, Copy)]
struct RequiresTokioIo;

impl TradingTransport for RequiresTokioIo {
    async fn send(&self, _request: &HttpRequest) -> Result<Response, TransportError> {
        let (stream, _peer) = StdUnixStream::pair().map_err(|_| TransportError::Connect)?;
        stream
            .set_nonblocking(true)
            .map_err(|_| TransportError::Connect)?;
        let _registered =
            tokio::net::UnixStream::from_std(stream).map_err(|_| TransportError::Connect)?;
        Err(TransportError::RefusedPath)
    }
}

fn setup(place_one_order: bool) -> Setup {
    Setup {
        deployment: Deployment {
            agent: AgentId(AGENT.to_owned()),
            connection: ConnectionId("conn_alpaca_paper_01".to_owned()),
            workspace: WorkspaceId(WORKSPACE.to_owned()),
        },
        account_ref: ACCOUNT_REF.to_owned(),
        now: UtcNanos::parse(NOW).unwrap(),
        place_one_order,
        new_cycle: false,
    }
}

fn stages(mandate: &str, dataset: PathBuf, transport: Scripted) -> Stages {
    governed_stages(mandate, dataset, transport, run_context(true), None)
}

type Artifacts = Option<Arc<dyn ArtifactSource + Send + Sync>>;

fn governed_stages(
    mandate: &str,
    dataset: PathBuf,
    transport: Scripted,
    run: RunContext,
    artifacts: Artifacts,
) -> Stages {
    production(Sources {
        mandate: fixtures().join(mandate),
        dataset,
        journal: None,
        recorded_at: UtcNanos::parse(NOW).unwrap(),
        agent: AgentId(AGENT.to_owned()),
        workspace: WORKSPACE.to_owned(),
        account_ref: ACCOUNT_REF.to_owned(),
        executor: Some(executor_context(mandate)),
        run: Some(run),
        artifacts,
        transport,
    })
}

fn cycle(mandate: &str, dataset: PathBuf, transport: Scripted) -> ProductionCycle {
    production_cycle(
        Sources {
            mandate: fixtures().join(mandate),
            dataset,
            journal: None,
            recorded_at: UtcNanos::parse(NOW).unwrap(),
            agent: AgentId(AGENT.to_owned()),
            workspace: WORKSPACE.to_owned(),
            account_ref: ACCOUNT_REF.to_owned(),
            executor: Some(executor_context(mandate)),
            run: Some(run_context(true)),
            artifacts: None,
            transport,
        },
        setup(true),
    )
}

struct UnreachableJournal;

impl JournalWriter for UnreachableJournal {
    fn take_ownership(&mut self, _stream: &str) -> Result<u64, Cause> {
        panic!("the temporary replacement journal is never used")
    }

    fn read(&self, _stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        panic!("the temporary replacement journal is never used")
    }

    fn append(
        &mut self,
        _stream: &str,
        _expected_head: u64,
        _writer_epoch: u64,
        _drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        panic!("the temporary replacement journal is never used")
    }
}

struct RecordingJournal {
    inner: Box<dyn JournalWriter>,
    batches: Rc<RefCell<Vec<Vec<String>>>>,
}

impl JournalWriter for RecordingJournal {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        self.inner.take_ownership(stream)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        self.inner.read(stream)
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let types = drafts
            .iter()
            .map(|bytes| {
                mandate_canon::parse(bytes)
                    .ok()
                    .and_then(|body| {
                        body.get("event_type")
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                    })
                    .unwrap_or_else(|| "<invalid>".to_owned())
            })
            .collect();
        self.batches.borrow_mut().push(types);
        self.inner
            .append(stream, expected_head, writer_epoch, drafts)
    }
}

fn record_batches(stages: &mut Stages) -> Rc<RefCell<Vec<Vec<String>>>> {
    let batches = Rc::new(RefCell::new(Vec::new()));
    let inner = std::mem::replace(&mut stages.journal, Box::new(UnreachableJournal));
    stages.journal = Box::new(RecordingJournal {
        inner,
        batches: Rc::clone(&batches),
    });
    batches
}

fn committed(stages: &Stages, stream: &str) -> Vec<StoredEvent> {
    stages.journal.read(stream).unwrap()
}

fn of_type(rows: &[StoredEvent], event_type: &str) -> Vec<Value> {
    rows.iter()
        .map(|row| mandate_canon::parse(&row.body).unwrap())
        .filter(|body| body.get("event_type").and_then(Value::as_str) == Some(event_type))
        .collect()
}

fn payload_field(body: &Value, name: &str) -> Option<String> {
    body.get("payload")
        .and_then(|payload| payload.get(name))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The run's refusal, or a failure naming what the run did instead.
fn refused(outcome: Result<Report, ShellError>) -> ShellError {
    match outcome {
        Err(error) => error,
        Ok(report) => panic!("the run placed {report:?}"),
    }
}

/// Asserts the refusal's code, printing the refusal itself, so a run that stopped somewhere else
/// shows where and why.
fn assert_refused(outcome: Result<Report, ShellError>, code: &str) {
    let error = refused(outcome);
    assert_eq!(error.code(), code, "{error}");
}

fn stream_bytes(stages: &Stages) -> Vec<u8> {
    let mut bytes = Vec::new();
    for stream in [agent_stream(), account_stream()] {
        for row in committed(stages, &stream) {
            bytes.extend_from_slice(&row.body);
        }
    }
    bytes
}

fn builder_fixture() -> (MandateView, ModelOutput) {
    let admitted = SpecMandate {
        path: fixtures().join("mandate.json"),
        context: Some(Rc::new(run_context(false))),
    }
    .admitted()
    .unwrap();
    let now = mandate_runtime::RiskClock::from_secs(UtcNanos::parse(NOW).unwrap().secs());
    let instrument = admitted.view.working_universe.first().unwrap().clone();
    let output = mandate_shell::map::model_output(
        mandate_backtest::Signal::Long,
        &admitted.model,
        &instrument,
        now,
    )
    .unwrap();
    (admitted.view, output)
}

fn size_output(
    context: RunContext,
    view: &MandateView,
    output: ModelOutput,
    outer_model: String,
    outer_instrument: RuntimeInstrumentId,
) -> Result<Option<Proposal>, Cause> {
    let now = output.as_of;
    let inputs = SignalInputs {
        outputs: BTreeMap::from([(outer_model, BTreeMap::from([(outer_instrument, output)]))]),
        output_events: BTreeMap::new(),
        now,
    };
    BuilderPlan {
        mandate: fixtures().join("mandate.json"),
        context: Some(Rc::new(context)),
    }
    .size(view, &inputs)
}

fn assert_absent<T: std::fmt::Debug>(result: Result<T, Cause>, expected: &'static str) {
    match result {
        Err(Cause::Absent { what }) => assert_eq!(what, expected),
        other => panic!("expected Cause::Absent({expected:?}), got {other:?}"),
    }
}

/// Step 1: the fixture mandate validates with no violation before anything else uses it.
#[test]
fn the_fixture_mandate_validates_with_no_violation() {
    let admitted = SpecMandate {
        path: fixtures().join("mandate.json"),
        context: Some(Rc::new(run_context(false))),
    }
    .admitted()
    .unwrap();
    assert_eq!(admitted.environment, Environment::Paper);
    assert_eq!(admitted.model.id, "quant.ma_crossover");
    assert_eq!(admitted.model.version, "1.0.0");
    assert_eq!(admitted.view.working_universe.len(), 1);
}

/// Step 5, by hand (`generate.py` asserts the same against `ref.py`): the cap is
/// min(1000, 1 × 1000) = 1000, one fresh output of conviction 1 and confidence 1 gives a buy
/// conviction of 1 and a target of 1000, the budget is min(1000, 300, 1000, 1000) = 300, and
/// trunc(300 / 255.20) = 1 share, whose 255.20 clears the 50 band (0.05 × 1000).
#[test]
fn the_fixture_sizes_to_exactly_one_share() {
    let admitted = SpecMandate {
        path: fixtures().join("mandate.json"),
        context: Some(Rc::new(run_context(true))),
    }
    .admitted()
    .unwrap();
    let now = mandate_runtime::RiskClock::from_secs(UtcNanos::parse(NOW).unwrap().secs());
    let instrument = admitted.view.working_universe.first().unwrap().clone();
    let output = mandate_shell::map::model_output(
        mandate_backtest::Signal::Long,
        &admitted.model,
        &instrument,
        now,
    )
    .unwrap();
    let inputs = SignalInputs {
        outputs: BTreeMap::from([(
            output.model_id.clone(),
            BTreeMap::from([(instrument, output)]),
        )]),
        output_events: BTreeMap::new(),
        now,
    };
    let plan = BuilderPlan {
        mandate: fixtures().join("mandate.json"),
        context: Some(Rc::new(run_context(true))),
    };
    let proposal: Proposal = plan.size(&admitted.view, &inputs).unwrap().unwrap();
    assert_eq!(proposal.qty.to_string(), "1");
    assert_eq!(proposal.limit.to_string(), "255.2");
    let take_profit = Price::parse("255.2")
        .unwrap()
        .collar_bound(Fraction::parse("0.1").unwrap(), Adverse::Up)
        .unwrap();
    assert_eq!(
        take_profit.to_string(),
        "280.72",
        "255.20 × (1 + 0.10) is exact in the fixed-point price calculation"
    );
    assert_eq!(
        proposal.execution,
        Some(OrderExecution {
            asset_class: DomainAssetClass::UsEquity,
            tif: TimeInForce::Day,
            protection_required: true,
            protection: Some(ProtectionPrices {
                stop: Price::parse("242.44").unwrap(),
                take_profit: Some(take_profit),
            }),
        })
    );
    assert_eq!(
        plan.classify(&admitted.view, &proposal).unwrap(),
        mandate_runtime::Classified {
            autonomy: mandate_runtime::Autonomy::Auto,
            decided_by: Some("rule:routine".to_owned()),
        }
    );
}

#[test]
fn sizing_refuses_a_model_output_without_its_trusted_content_hash() {
    let admitted = SpecMandate {
        path: fixtures().join("mandate.json"),
        context: Some(Rc::new(run_context(false))),
    }
    .admitted()
    .unwrap();
    let now = mandate_runtime::RiskClock::from_secs(UtcNanos::parse(NOW).unwrap().secs());
    let instrument = admitted.view.working_universe.first().unwrap().clone();
    let output = mandate_shell::map::model_output(
        mandate_backtest::Signal::Long,
        &admitted.model,
        &instrument,
        now,
    )
    .unwrap();
    let inputs = SignalInputs {
        outputs: BTreeMap::from([(
            output.model_id.clone(),
            BTreeMap::from([(instrument, output)]),
        )]),
        output_events: BTreeMap::new(),
        now,
    };
    let mut context = run_context(true);
    context
        .decision
        .as_mut()
        .unwrap()
        .builder
        .as_mut()
        .unwrap()
        .model_content_hashes
        .clear();
    let result = BuilderPlan {
        mandate: fixtures().join("mandate.json"),
        context: Some(Rc::new(context)),
    }
    .size(&admitted.view, &inputs);
    assert!(matches!(
        result,
        Err(mandate_shell::Cause::Absent {
            what: "the model output's trusted content hash"
        })
    ));
}

#[test]
fn sizing_refuses_each_inconsistent_output_key() {
    let (view, output) = builder_fixture();
    let expected = "a consistently keyed model output";
    let canonical_model = output.model_id.clone();
    let canonical_instrument = output.instrument_id.clone();

    let mut wrong_model = output.clone();
    wrong_model.model_id = "quant.untrusted".to_owned();
    assert_absent(
        size_output(
            run_context(true),
            &view,
            wrong_model,
            canonical_model.clone(),
            canonical_instrument.clone(),
        ),
        expected,
    );

    let wrong_outer_instrument =
        RuntimeInstrumentId::new("8f475fc4-8bad-4ec1-bbeb-8023ad80310f").unwrap();
    assert_absent(
        size_output(
            run_context(true),
            &view,
            output,
            canonical_model,
            wrong_outer_instrument,
        ),
        expected,
    );
}

#[test]
fn sizing_refuses_each_unpinned_model_identity_field() {
    let (view, output) = builder_fixture();
    let expected = "a model output pinned by the validated mandate";

    let mut wrong_version = output.clone();
    wrong_version.model_version = "2.0.0".to_owned();
    let mut wrong_version_context = run_context(true);
    wrong_version_context
        .decision
        .as_mut()
        .unwrap()
        .builder
        .as_mut()
        .unwrap()
        .model_content_hashes
        .insert(
            (
                wrong_version.model_id.clone(),
                wrong_version.model_version.clone(),
            ),
            wrong_version.content_hash,
        );
    assert_absent(
        size_output(
            wrong_version_context,
            &view,
            wrong_version.clone(),
            wrong_version.model_id.clone(),
            wrong_version.instrument_id.clone(),
        ),
        expected,
    );

    let mut wrong_model = output;
    wrong_model.model_id = "quant.untrusted".to_owned();
    let mut wrong_model_context = run_context(true);
    wrong_model_context
        .decision
        .as_mut()
        .unwrap()
        .builder
        .as_mut()
        .unwrap()
        .model_content_hashes
        .insert(
            (
                wrong_model.model_id.clone(),
                wrong_model.model_version.clone(),
            ),
            wrong_model.content_hash,
        );
    assert_absent(
        size_output(
            wrong_model_context,
            &view,
            wrong_model.clone(),
            wrong_model.model_id.clone(),
            wrong_model.instrument_id.clone(),
        ),
        expected,
    );
}

#[test]
fn sizing_refuses_each_execution_policy_mismatch() {
    let (view, output) = builder_fixture();
    let expected = "the mandate-derived execution policy";

    let mut wrong_asset = run_context(true);
    let execution = &mut wrong_asset
        .decision
        .as_mut()
        .unwrap()
        .builder
        .as_mut()
        .unwrap()
        .execution;
    execution.asset_class = DomainAssetClass::Crypto;
    execution.tif = TimeInForce::Gtc;
    assert_absent(
        size_output(
            wrong_asset,
            &view,
            output.clone(),
            output.model_id.clone(),
            output.instrument_id.clone(),
        ),
        expected,
    );

    let mut missing_protection = run_context(true);
    let execution = &mut missing_protection
        .decision
        .as_mut()
        .unwrap()
        .builder
        .as_mut()
        .unwrap()
        .execution;
    execution.protection_required = false;
    execution.protection = None;
    assert_absent(
        size_output(
            missing_protection,
            &view,
            output.clone(),
            output.model_id.clone(),
            output.instrument_id.clone(),
        ),
        expected,
    );
}

#[test]
fn sizing_accepts_an_unprotected_sell() {
    let (view, mut output) = builder_fixture();
    output.conviction = Conviction::MINUS_ONE;
    let mut context = run_context(true);
    let builder = context.decision.as_mut().unwrap().builder.as_mut().unwrap();
    builder.account.position_qty = Qty::parse("1").unwrap();
    builder.account.cost_basis = CostBasis::parse("255.1").unwrap();
    builder.account.gross_usd = Usd::parse("255.1").unwrap();
    builder.action.purpose = Purpose::DiscretionaryExit;
    builder.action.order_usd = Usd::parse("255.1").unwrap();
    builder.action.position_usd_after = Usd::ZERO;
    builder.action.gross_usd_after = Usd::ZERO;
    builder.execution.protection_required = false;
    builder.execution.protection = None;

    let proposal = size_output(
        context,
        &view,
        output.clone(),
        output.model_id.clone(),
        output.instrument_id.clone(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(proposal.side, mandate_accounting::Side::Sell);
    assert_eq!(proposal.purpose, RuntimePurpose::DiscretionaryExit);
    assert_eq!(
        proposal.execution,
        Some(OrderExecution {
            asset_class: DomainAssetClass::UsEquity,
            tif: TimeInForce::Day,
            protection_required: false,
            protection: None,
        })
    );
}

#[test]
fn classification_refuses_each_mismatched_action_fact() {
    let (view, output) = builder_fixture();
    let context = run_context(true);
    let proposal = size_output(
        context.clone(),
        &view,
        output.clone(),
        output.model_id.clone(),
        output.instrument_id.clone(),
    )
    .unwrap()
    .unwrap();
    let plan = BuilderPlan {
        mandate: fixtures().join("mandate.json"),
        context: Some(Rc::new(context)),
    };
    let expected = "classification facts for the proposed action";

    let mut wrong_instrument = proposal.clone();
    wrong_instrument.instrument =
        RuntimeInstrumentId::new("8f475fc4-8bad-4ec1-bbeb-8023ad80310f").unwrap();
    assert_absent(plan.classify(&view, &wrong_instrument), expected);

    let mut wrong_asset = proposal.clone();
    wrong_asset.asset_class = DomainAssetClass::Crypto;
    assert_absent(plan.classify(&view, &wrong_asset), expected);

    let mut wrong_purpose = proposal.clone();
    wrong_purpose.purpose = RuntimePurpose::Increase;
    assert_absent(plan.classify(&view, &wrong_purpose), expected);

    let mut wrong_score = proposal;
    wrong_score.combined_score = Value::Str("0".to_owned());
    assert_absent(plan.classify(&view, &wrong_score), expected);
}

/// The whole path: one order, its intent journaled before it was sent, both streams verifying,
/// every draft paper, and no account number or credential anywhere in the record (TI-1, TI-7,
/// TI-8).
#[test]
fn happy() {
    let scratch = Scratch::new("happy");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    let batches = record_batches(&mut stages);
    let report = run(&mut stages, &setup(true)).unwrap();
    assert_eq!(seen.borrow().posts(), 1);
    assert_eq!(report.submitted.len(), 1);
    assert!(
        batches
            .borrow()
            .iter()
            .any(|batch| batch == &["DecisionMade".to_owned(), "IntentProposed".to_owned()]),
        "the decision and its caused intent are one journal append batch: {:?}",
        batches.borrow()
    );
    assert!(
        batches.borrow().iter().any(|batch| {
            batch
                .windows(2)
                .any(|pair| pair == ["OrderRequestRecorded", "OrderSubmitted"])
        }),
        "the request companion and submission are one journal append batch before the broker call: {:?}",
        batches.borrow()
    );

    let agent = committed(&stages, &agent_stream());
    let account = committed(&stages, &account_stream());
    assert_eq!(
        agent.first().map(|row| row.event_type.as_str()),
        Some("StreamOpened"),
        "the agent stream opens before the runtime journals its startup mode"
    );
    assert_eq!(
        account.first().map(|row| row.event_type.as_str()),
        Some("StreamOpened"),
        "the account stream opens before the executor journals reconciliation"
    );
    assert!(
        agent.iter().all(|row| row.schema_version == 1),
        "runtime agent drafts retain schema version 1: {agent:?}"
    );
    for event_type in ["IntentReceived", "GateDecided", "OrderSubmitted"] {
        let rows: Vec<_> = account
            .iter()
            .filter(|row| row.event_type == event_type)
            .collect();
        assert!(!rows.is_empty(), "{event_type} is journaled");
        assert!(
            rows.iter().all(|row| row.schema_version == 2),
            "{event_type} is executor-owned schema version 2: {rows:?}"
        );
    }
    for event_type in [
        "StreamOpened",
        "ReconciliationRun",
        "ProtectionChanged",
        "OrderRequestRecorded",
    ] {
        let rows: Vec<_> = account
            .iter()
            .filter(|row| row.event_type == event_type)
            .collect();
        assert!(!rows.is_empty(), "{event_type} is journaled");
        assert!(
            rows.iter().all(|row| row.schema_version == 1),
            "{event_type} retains its registered schema version: {rows:?}"
        );
    }
    let intents = of_type(&agent, "IntentProposed");
    assert_eq!(intents.len(), 1);
    let submitted = of_type(&account, "OrderSubmitted");
    assert_eq!(submitted.len(), 1);
    let client_order_id = payload_field(&submitted[0], "client_order_id").unwrap();
    assert_eq!(report.submitted, std::slice::from_ref(&client_order_id));
    let posted = seen
        .borrow()
        .requests
        .iter()
        .find_map(|(method, _, body)| (*method == Method::Post).then(|| body.clone().unwrap()));
    assert!(posted.unwrap().contains(&client_order_id));

    let (model_digest, model_bytes) = model_artifact();
    let (config_refs, mut artifacts) = executor_artifacts("mandate.json");
    artifacts.insert(model_digest, model_bytes);
    for rows in [&agent, &account] {
        verify_events(rows, TrustedStart::GENESIS, &artifacts).unwrap();
        for row in rows.iter() {
            assert_eq!(row.environment, "paper");
        }
    }
    let fee_digest = config_refs
        .fee_config
        .as_deref()
        .and_then(|reference| reference.strip_prefix("sha256:"))
        .and_then(Digest::from_hex)
        .unwrap();
    let mut wrong = artifacts.clone();
    wrong.insert(fee_digest, b"wrong fee config".to_vec());
    assert!(
        verify_events(&account, TrustedStart::GENESIS, &wrong).is_err(),
        "verification rejects bytes that do not hash to the referenced fee configuration"
    );
    let written = String::from_utf8(stream_bytes(&stages)).unwrap();
    for secret in [
        "pii:account_number",
        "pii:account_id",
        "account_number",
        "PKSENTINEL",
    ] {
        assert!(!written.contains(secret), "{secret} reached the journal");
    }
}

/// E7-19 slice 1: production execution starts from a registered model output supplied across the
/// public cycle boundary. It does not read bars or run a strategy in the execution shell.
#[test]
fn the_production_cycle_takes_model_output_as_an_input() {
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut cycle = cycle(
        "mandate.json",
        PathBuf::from("the-production-cycle-does-not-read-a-dataset"),
        transport,
    );
    let (_, output) = builder_fixture();

    let report = cycle.run(output).unwrap();

    assert_eq!(seen.borrow().posts(), 1);
    assert_eq!(report.submitted.len(), 1);
}

/// A caller cannot use the production boundary to substitute a model outside the confirmed
/// mandate. The runtime may record the opinion, but the builder emits no intent and nothing sends.
#[test]
fn the_production_cycle_refuses_an_unconfirmed_model_before_intent() {
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut cycle = cycle(
        "mandate.json",
        PathBuf::from("the-production-cycle-does-not-read-a-dataset"),
        transport,
    );
    let (_, mut output) = builder_fixture();
    output.model_id = "quant.not-confirmed".to_owned();

    assert_refused(cycle.run(output), "no_proposal");

    assert_eq!(seen.borrow().posts(), 0);
}

/// TI-10: the same fixtures, mandate, and clock journal byte-identical drafts.
#[test]
fn happy_is_deterministic() {
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let mut journals = Vec::new();
    for name in ["deterministic-a", "deterministic-b"] {
        let scratch = Scratch::new(name);
        let dataset = bars(&scratch.0, &closes);
        let mut stages = stages("mandate.json", dataset, Scripted::new(Broker::Fresh));
        run(&mut stages, &setup(true)).unwrap();
        journals.push(stream_bytes(&stages));
    }
    assert!(!journals[0].is_empty());
    assert_eq!(journals[0], journals[1]);
}

/// A planning run sends nothing and reports the order it would place.
#[test]
fn a_planning_run_sends_nothing() {
    let scratch = Scratch::new("planning");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    let report = run(&mut stages, &setup(false)).unwrap();
    assert_eq!(seen.borrow().posts(), 0);
    assert_eq!(report.would_place.unwrap().qty.to_string(), "1");
}

/// TI-9, PB-6: the mandate classifies the opening ASK, and the tracer has no escalation: the
/// request is journaled and nothing is sent.
#[test]
fn autonomy_ask() {
    let scratch = Scratch::new("ask");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate-ask.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "autonomy_not_auto");
    assert_eq!(seen.borrow().posts(), 0);
    let agent = committed(&stages, &agent_stream());
    assert_eq!(of_type(&agent, "ApprovalRequested").len(), 1);
    assert_eq!(of_type(&agent, "IntentProposed").len(), 0);
}

/// PB-7: falling closes put the five-day average below the twenty-day one; flat opens nothing.
#[test]
fn signal_flat() {
    let scratch = Scratch::new("flat");
    let closes = falling();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "no_long_signal");
    assert_eq!(seen.borrow().posts(), 0);
}

/// PB-7: ten closes are fewer than the twenty the slow window needs, so the signal is undecided,
/// never a buy.
#[test]
fn signal_undecided() {
    let scratch = Scratch::new("undecided");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().skip(15).map(String::as_str).collect();
    let dataset = bars_from(&scratch.0, "2026-09-14", &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "no_long_signal");
    assert_eq!(seen.borrow().posts(), 0);
}

/// Stored bars must reach the last completed equity session at the run's injected clock. A clean,
/// internally gapless dataset ending one session earlier is stale and sends nothing.
#[test]
fn stale_stored_bars_are_refused() {
    let scratch = Scratch::new("stale-bars");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    let mut stale = setup(true);
    stale.now = UtcNanos::parse("2026-09-29T13:00:00.000000000Z").unwrap();
    assert_refused(run(&mut stages, &stale), "market_data_untrusted");
    assert_eq!(seen.borrow().posts(), 0);
}

/// A mandate whose order cap is below one share: the builder holds, so nothing is proposed and
/// nothing is sent. A cap is a limit, which the shell never judges (PB-14).
#[test]
fn oversized_proposal() {
    let scratch = Scratch::new("oversized");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate-small-orders.json", dataset, transport);
    let report = run(&mut stages, &setup(true)).unwrap();
    assert!(report.submitted.is_empty());
    assert_eq!(seen.borrow().posts(), 0);
    assert_eq!(
        of_type(&committed(&stages, &agent_stream()), "IntentProposed").len(),
        0
    );
}

/// PB-15: one close ten times the others is coverage the market-data stage cannot trust.
#[test]
#[ignore = "pending E2-14"]
fn outlier_close() {
    let scratch = Scratch::new("outlier");
    let mut closes = rising();
    closes[24] = "2552.00".to_owned();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "market_data_untrusted");
    assert_eq!(seen.borrow().posts(), 0);
}

/// TI-6, PB-3: a second run over the same journal sends nothing further; across both runs there is
/// exactly one submission and one intent, and the broker's duplicate check is never needed.
#[test]
fn duplicate_after_restart() {
    let scratch = Scratch::new("restart");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    run(&mut stages, &setup(true)).unwrap();
    assert_refused(run(&mut stages, &setup(true)), "cycle_already_open");
    assert_eq!(seen.borrow().posts(), 1);
    assert_eq!(
        of_type(&committed(&stages, &agent_stream()), "IntentProposed").len(),
        1
    );
}

/// TI-12, PB-16: a fresh journal against a broker that already holds the position is a
/// reconciliation mismatch, never a clean start.
#[test]
fn fresh_journal_with_broker_position() {
    let scratch = Scratch::new("fresh-journal");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::HoldsAapl);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "reconciliation_mismatch");
    assert_eq!(seen.borrow().posts(), 0);
}

/// PB-11: the submission times out, the query finds nothing, and one absence never resubmits.
#[test]
fn broker_unknown_then_absent() {
    let scratch = Scratch::new("unknown");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::TimeoutThenAbsent);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    let report = run(&mut stages, &setup(true)).unwrap();
    assert_eq!(report.submitted.len(), 1);
    assert_eq!(seen.borrow().posts(), 1);
    assert!(seen.borrow().order_queries() >= 1);
}

/// PB-12: after a mismatch the agent stays paused and alerted, and nothing in the tracer lifts it:
/// its agent stream's last mode is `paused`.
#[test]
fn reconcile_mismatch_pauses() {
    let scratch = Scratch::new("mismatch");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let mut stages = stages("mandate.json", dataset, Scripted::new(Broker::HoldsAapl));
    let error = refused(run(&mut stages, &setup(true)));
    assert_eq!(error.code(), "reconciliation_mismatch", "{error}");
    let modes: Vec<String> = of_type(&committed(&stages, &agent_stream()), "AgentModeChanged")
        .iter()
        .filter_map(|body| payload_field(body, "to"))
        .collect();
    assert_eq!(modes.last().map(String::as_str), Some("paused"));
}

/// The connector type the manual run uses is the one CI drives, over any transport.
#[test]
fn the_connector_is_the_alpaca_client_over_the_given_transport() {
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut connector = AlpacaConnector { transport };
    let answer = mandate_shell::stages::Connector::call(
        &mut connector,
        &mandate_executor::BrokerRequest::GetAccount,
    );
    assert!(answer.is_ok(), "{answer:?}");
    assert_eq!(seen.borrow().requests.len(), 1);
}

#[test]
fn the_connector_runtime_drives_tokio_io_without_network() {
    let mut connector = AlpacaConnector {
        transport: RequiresTokioIo,
    };
    let answer = mandate_shell::stages::Connector::call(
        &mut connector,
        &mandate_executor::BrokerRequest::GetAccount,
    );
    assert_eq!(
        answer,
        Err(mandate_executor::ConnectorError::NotSent {
            code: "refused_path"
        })
    );
}

/// A policy set of no levels, which the first paper trade registers, so the overlay neither
/// narrows nor denies anything on it.
const OPEN_POLICY: &str = r#"{"kind":"policy_set","levels":[],"policy_set_version":1}"#;
/// An earlier registered policy set that a later one replaced, so it never governs.
const OLDER_POLICY: &str = r#"{"kind":"policy_set","levels":[{"level":"workspace","policy_schema_version":1,"values":{"max_orders_per_day":100}}],"policy_set_version":1}"#;
/// A workspace level that forbids `auto`, which the E7-7 mandate's `auto` default exceeds.
const NO_AUTO_POLICY: &str = r#"{"kind":"policy_set","levels":[{"level":"workspace","policy_schema_version":1,"values":{"auto_allowed":false}}],"policy_set_version":1}"#;

/// `object`'s canonical bytes, registered at `seq` under their hash, as D4b returns them.
fn registered(seq: u64, object: &str) -> Registered {
    let bytes = mandate_canon::to_canonical(&mandate_canon::parse(object.as_bytes()).unwrap());
    Registered {
        seq,
        content_hash: Digest::of(&bytes),
        bytes,
    }
}

/// The registry with the one entry for the pinned model, as its registration states it.
fn registry() -> String {
    let entry = format!(
        r#"{{"admits_instruments":false,"content_hash":"sha256:{}","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}}"#,
        model_artifact().0
    );
    format!(r#"{{"kind":"model_registry","model_registry_version":1,"models":[{entry}]}}"#)
}

/// What D4b reads for the E7-7 mandate under `policy`, whose levels are `levels`.
fn governance(policy: &str, levels: &[PolicyLevel]) -> Governance {
    let source = fs::read(fixtures().join("mandate.json")).unwrap();
    let mandate = mandate_spec::Mandate::parse(&mandate_canon::parse(&source).unwrap()).unwrap();
    let checked = mandate_spec::policy::check(&mandate, levels).unwrap();
    Governance {
        policy_set: registered(2, policy),
        model_registry: registered(3, &registry()),
        overlay: checked.overlay,
        violations: checked.violations,
    }
}

/// The artifact store a governed run was registered in: the model, the policy set, the registry.
fn governed_store(governance: &Governance) -> BTreeMap<Digest, Vec<u8>> {
    BTreeMap::from([
        model_artifact(),
        (
            governance.policy_set.content_hash,
            governance.policy_set.bytes.clone(),
        ),
        (
            governance.model_registry.content_hash,
            governance.model_registry.bytes.clone(),
        ),
    ])
}

/// A governed run of the happy path with `store`, and the scripted broker's record of it.
fn governed_run(
    name: &str,
    store: Artifacts,
) -> (Result<Report, ShellError>, Stages, Rc<RefCell<Seen>>) {
    governed_run_under(name, governance(OPEN_POLICY, &[]), store)
}

/// [`governed_run`] under `governed` rather than the open policy set.
fn governed_run_under(
    name: &str,
    governed: Governance,
    store: Artifacts,
) -> (Result<Report, ShellError>, Stages, Rc<RefCell<Seen>>) {
    let scratch = Scratch::new(name);
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut context = run_context(true);
    context.governance = Some(governed);
    let mut stages = governed_stages("mandate.json", dataset, transport, context, store);
    let outcome = run(&mut stages, &setup(true));
    (outcome, stages, seen)
}

/// DEC-484 items 4 and 5 (D4c): a governed run writes `ModelOutputRecorded` and `DecisionMade`
/// at schema version 2, referencing the registered model registry and, for the decision, the
/// policy set, beside the mandate version. A version-2 record commits only through the
/// artifact-aware append, which finds both objects in the store; every other agent record stays
/// at version 1, and both streams verify against the store.
#[test]
fn a_governed_run_writes_version_2_records_with_the_registered_refs() {
    let governed = governance(OPEN_POLICY, &[]);
    let older = registered(1, OLDER_POLICY);
    assert_ne!(older.content_hash, governed.policy_set.content_hash);
    let mut store = governed_store(&governed);
    store.insert(older.content_hash, older.bytes.clone());
    let (outcome, stages, seen) = governed_run("governed", Some(Arc::new(store.clone())));
    assert_eq!(outcome.unwrap().submitted.len(), 1);
    assert_eq!(seen.borrow().posts(), 1);
    let agent = committed(&stages, &agent_stream());
    let intent = of_type(&agent, "IntentProposed");
    let refs_of = |body: &Value| body.get("config_refs").cloned();
    let version = refs_of(&intent[0]).unwrap();
    let version = version.get("mandate_version").and_then(Value::as_str);
    let version = version.unwrap().to_owned();
    let registry = format!(
        r#""model_registry":"sha256:{}""#,
        governed.model_registry.content_hash
    );
    let policy = format!(
        r#""policy_set":"sha256:{}""#,
        governed.policy_set.content_hash
    );
    let mandate = format!(r#""mandate_version":"{version}""#);
    let output = format!("{{{mandate},{registry}}}");
    let decision = format!("{{{mandate},{registry},{policy}}}");
    for (event_type, refs) in [("ModelOutputRecorded", output), ("DecisionMade", decision)] {
        let rows = of_type(&agent, event_type);
        assert_eq!(rows.len(), 1, "{event_type}");
        let refs = mandate_canon::parse(refs.as_bytes()).unwrap();
        assert_eq!(refs_of(&rows[0]), Some(refs), "{event_type}");
    }
    let decision = refs_of(&of_type(&agent, "DecisionMade")[0]).unwrap();
    let policy_ref = decision.get("policy_set").and_then(Value::as_str);
    let effective = format!("sha256:{}", governed.policy_set.content_hash);
    assert_eq!(
        policy_ref,
        Some(effective.as_str()),
        "the seq-2 set, never the older one"
    );
    for row in &agent {
        let governed = ["ModelOutputRecorded", "DecisionMade"].contains(&row.event_type.as_str());
        let version = if governed { 2 } else { 1 };
        assert_eq!(row.schema_version, version, "{}", row.event_type);
    }
    let (_, mut artifacts) = executor_artifacts("mandate.json");
    artifacts.extend(store);
    for stream in [agent_stream(), account_stream()] {
        let rows = committed(&stages, &stream);
        verify_events(&rows, TrustedStart::GENESIS, &artifacts).unwrap();
    }
}

/// Journal spec §5.1 and §11 check 6 (D4c): a governed run whose store lacks the registry or the
/// policy set, or that has no store, stops at the first version-2 append that names the missing
/// object. Nothing is proposed, handed to the executor or sent to the broker.
#[test]
fn a_registry_or_policy_missing_from_the_store_stops_the_run_before_any_order() {
    let governed = governance(OPEN_POLICY, &[]);
    let full = governed_store(&governed);
    let without = |digest: Digest| {
        let mut store = full.clone();
        store.remove(&digest);
        store
    };
    let cases: [(&str, Artifacts, usize); 3] = [
        ("no store", None, 0),
        (
            "no registry",
            Some(Arc::new(without(governed.model_registry.content_hash))),
            0,
        ),
        (
            "no policy set",
            Some(Arc::new(without(governed.policy_set.content_hash))),
            1,
        ),
    ];
    for (name, store, outputs) in cases {
        let (outcome, stages, seen) = governed_run("governed-missing", store);
        let error = refused(outcome);
        assert_eq!(error.code(), "append_not_committed", "{name}: {error}");
        assert_eq!(seen.borrow().posts(), 0, "{name}");
        let agent = committed(&stages, &agent_stream());
        assert_eq!(
            of_type(&agent, "ModelOutputRecorded").len(),
            outputs,
            "{name}"
        );
        assert_eq!(of_type(&agent, "DecisionMade").len(), 0, "{name}");
        assert_eq!(of_type(&agent, "IntentProposed").len(), 0, "{name}");
        let account = committed(&stages, &account_stream());
        assert_eq!(of_type(&account, "IntentReceived").len(), 0, "{name}");
    }
}

/// DEC-534 item 2 and `AGENTS.md` rules 2 and 13 (D4c): every exit is classified `auto`, decided
/// by the built-in risk-reducing step, while the confirmed mandate is nonconforming and the overlay
/// forbids `auto`: the discretionary sell the builder sized, and the same sell as a risk exit, a
/// protective order and an owner exit. The opening that the same governance denies is D4d's.
/// The same holds with no governance at all, which a run with an open position may have (D4d).
#[test]
fn an_exit_is_auto_while_the_mandate_is_nonconforming() {
    let no_auto = PolicyLevel {
        name: LevelName::Workspace,
        values: BTreeMap::from([(PolicyKey::AutoAllowed, PolicyValue::Flag(false))]),
    };
    let governed = governance(NO_AUTO_POLICY, &[no_auto]);
    assert!(!governed.violations.is_empty());
    assert!(!governed.overlay.auto_allowed());
    let (view, mut output) = builder_fixture();
    output.conviction = Conviction::MINUS_ONE;
    let mut context = run_context(true);
    let builder = context.decision.as_mut().unwrap().builder.as_mut().unwrap();
    builder.account.position_qty = Qty::parse("1").unwrap();
    builder.account.cost_basis = CostBasis::parse("255.1").unwrap();
    builder.account.gross_usd = Usd::parse("255.1").unwrap();
    builder.action.purpose = Purpose::DiscretionaryExit;
    builder.action.order_usd = Usd::parse("255.1").unwrap();
    builder.action.position_usd_after = Usd::ZERO;
    builder.action.gross_usd_after = Usd::ZERO;
    builder.execution.protection_required = false;
    builder.execution.protection = None;
    context.governance = Some(governed);
    let (model, instrument) = (output.model_id.clone(), output.instrument_id.clone());
    let sized = size_output(context.clone(), &view, output, model, instrument);
    let sized = sized.unwrap().unwrap();
    assert_eq!(sized.purpose, RuntimePurpose::DiscretionaryExit);
    let classified = Classified {
        autonomy: Autonomy::Auto,
        decided_by: Some("builtin_risk_reducing".to_owned()),
    };
    let exits = [
        (
            Purpose::DiscretionaryExit,
            RuntimePurpose::DiscretionaryExit,
        ),
        (Purpose::RiskExit, RuntimePurpose::RiskExit),
        (Purpose::Protective, RuntimePurpose::Protective),
        (Purpose::OwnerExit, RuntimePurpose::OwnerExit),
    ];
    for (governance, (purpose, runtime)) in [context.governance.clone(), None]
        .into_iter()
        .flat_map(|governance| exits.map(|exit| (governance.clone(), exit)))
    {
        let mut context = context.clone();
        context.governance = governance.clone();
        let builder = context.decision.as_mut().unwrap().builder.as_mut().unwrap();
        builder.action.purpose = purpose;
        let proposal = Proposal {
            purpose: runtime,
            ..sized.clone()
        };
        let plan = BuilderPlan {
            mandate: fixtures().join("mandate.json"),
            context: Some(Rc::new(context)),
        };
        let answer = plan
            .classify(&view, &proposal)
            .map_err(|cause| cause.to_string());
        let governed = governance.is_some();
        assert_eq!(
            answer,
            Ok(classified.clone()),
            "{purpose:?}, governed {governed}"
        );
    }
}

/// DEC-534 and `AGENTS.md` rules 1 and 2 (D4c, held for D4d): an opening under a confirmed
/// mandate that the effective policy set makes nonconforming places nothing. The run reaches the
/// decision, since the model output is journaled, and then no intent is proposed or received, no
/// order is recorded or submitted, the broker sees no order, and the run reports none. It
/// asserts what is absent, never the refusal's cause, so D4c's fail-closed stop and D4d's
/// `policy_overlay` deny both pass it, and D4d's own tests pin the cause.
#[test]
fn a_nonconforming_mandate_places_no_opening_order() {
    let no_auto = PolicyLevel {
        name: LevelName::Workspace,
        values: BTreeMap::from([(PolicyKey::AutoAllowed, PolicyValue::Flag(false))]),
    };
    let governed = governance(NO_AUTO_POLICY, &[no_auto]);
    assert!(
        !governed.violations.is_empty(),
        "the mandate is nonconforming"
    );
    let store = governed_store(&governed);
    let (outcome, stages, seen) =
        governed_run_under("governed-nonconforming", governed, Some(Arc::new(store)));
    let outcome = outcome.map_err(|error| format!("{error}: {error:?}"));
    let agent = committed(&stages, &agent_stream());
    assert_eq!(
        of_type(&agent, "ModelOutputRecorded").len(),
        1,
        "the opening reached the decision; the run answered {outcome:?}"
    );
    assert!(
        outcome.is_err(),
        "the run reported a placed order: {outcome:?}"
    );
    assert_eq!(seen.borrow().posts(), 0, "the broker saw an order");
    assert_eq!(
        of_type(&agent, "IntentProposed").len(),
        0,
        "an intent was proposed"
    );
    let account = committed(&stages, &account_stream());
    for event_type in ["IntentReceived", "OrderRequestRecorded", "OrderSubmitted"] {
        assert_eq!(
            of_type(&account, event_type).len(),
            0,
            "{event_type} was journaled"
        );
    }
}

/// What D4b reads for the E7-7 mandate under a workspace level that forbids `auto`: the mandate's
/// `auto` rule exceeds it, so the confirmed version is nonconforming (DEC-534 item 1).
fn nonconforming() -> Governance {
    let no_auto = PolicyLevel {
        name: LevelName::Workspace,
        values: BTreeMap::from([(PolicyKey::AutoAllowed, PolicyValue::Flag(false))]),
    };
    let governed = governance(NO_AUTO_POLICY, &[no_auto]);
    assert!(!governed.violations.is_empty(), "the mandate conforms");
    governed
}

/// Mandate spec §6.2 step 5c and DEC-536 item 2: the deny the overlay itself decided.
fn overlay_deny() -> Classified {
    Classified {
        autonomy: Autonomy::Deny,
        decided_by: Some("policy_overlay".to_owned()),
    }
}

/// `mandate`'s classification of `proposal` under `context`, against the view `mandate` admits,
/// with a refusal printed as the cause reports it.
fn classify_under(
    mandate: &str,
    context: &RunContext,
    proposal: &Proposal,
) -> Result<Classified, String> {
    let path = fixtures().join(mandate);
    let context = Some(Rc::new(context.clone()));
    let source = SpecMandate {
        path: path.clone(),
        context: context.clone(),
    };
    let view = source.admitted().unwrap().view;
    let plan = BuilderPlan {
        mandate: path,
        context,
    };
    plan.classify(&view, proposal)
        .map_err(|cause| format!("{cause}: {cause:?}"))
}

/// The opening the builder sizes on the fixture, re-labelled `purpose`, with `context`'s
/// classification facts naming the same purpose.
fn proposal_as(context: &mut RunContext, purpose: Purpose, runtime: RuntimePurpose) -> Proposal {
    let (view, output) = builder_fixture();
    let (model, instrument) = (output.model_id.clone(), output.instrument_id.clone());
    let sized = size_output(context.clone(), &view, output, model, instrument);
    let sized = sized.unwrap().unwrap();
    assert_eq!(sized.purpose, RuntimePurpose::Open);
    let builder = context.decision.as_mut().unwrap().builder.as_mut().unwrap();
    builder.action.purpose = purpose;
    Proposal {
        purpose: runtime,
        ..sized
    }
}

/// The run's own builder, with each proposal it sizes turned into `purpose` on `side`, so a run
/// reaches the decision for an increase or an exit the fresh fixture cannot size itself.
struct Retarget {
    inner: Box<dyn Sizing>,
    side: Side,
    purpose: RuntimePurpose,
}

impl Sizing for Retarget {
    fn size(&self, view: &MandateView, inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        let exit_origin =
            (self.purpose == RuntimePurpose::DiscretionaryExit).then_some(ExitOrigin::Signal);
        Ok(self.inner.size(view, inputs)?.map(|proposal| Proposal {
            side: self.side,
            purpose: self.purpose,
            exit_origin,
            ..proposal
        }))
    }
}

/// [`governed_run_under`] with every sized proposal retargeted to `purpose`, and the advisory
/// gate holding the one share an increase adds to or an exit sells.
fn retargeted_run(
    name: &str,
    governed: Option<Governance>,
    store: Artifacts,
    (purpose, runtime, side): (Purpose, RuntimePurpose, Side),
) -> (Result<Report, ShellError>, Stages, Rc<RefCell<Seen>>) {
    let scratch = Scratch::new(name);
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut context = run_context(true);
    context.governance = governed;
    let decision = context.decision.as_mut().unwrap();
    decision.builder.as_mut().unwrap().action.purpose = purpose;
    let gate = decision.gate.as_mut().unwrap();
    let held = gate.instrument.instrument.clone();
    gate.agent.positions.insert(held, Qty::parse("1").unwrap());
    let mut stages = governed_stages("mandate.json", dataset, transport, context, store);
    let inner = std::mem::replace(&mut stages.sizing, Box::new(UnsizedPlan));
    stages.sizing = Box::new(Retarget {
        inner,
        side,
        purpose: runtime,
    });
    let outcome = run(&mut stages, &setup(true));
    (outcome, stages, seen)
}

/// The placeholder sizing that stands in only while [`retargeted_run`] wraps the run's own.
struct UnsizedPlan;

impl Sizing for UnsizedPlan {
    fn size(&self, _view: &MandateView, _inputs: &SignalInputs) -> Result<Option<Proposal>, Cause> {
        panic!("the placeholder sizing is never asked")
    }
}

/// What the account stream holds after reconciliation and before any intent reaches it.
const RECONCILED: [&str; 4] = [
    "StreamOpened",
    "AccountStateObserved",
    "AccountSnapshotRecorded",
    "ReconciliationRun",
];

/// Mandate spec §6.2 step 5c, DEC-534 item 2 and DEC-536: the run's one decision is a version-2
/// `deny` of `purpose` by `policy_overlay`, bound to the effective policy set and registry; no
/// intent or approval request follows, the account stream holds nothing past reconciliation, the
/// broker sees no order, and the run stops at the classification as a deny.
fn assert_overlay_denied(
    outcome: Result<Report, ShellError>,
    stages: &Stages,
    seen: &Rc<RefCell<Seen>>,
    governed: &Governance,
    purpose: &str,
) {
    let error = refused(outcome);
    let denied = matches!(
        &error,
        ShellError::Refused {
            stage: Stage::Classify,
            cause: Cause::NotAuto { autonomy: "deny" },
        }
    );
    assert!(denied, "{purpose}: {error}: {error:?}");
    assert_eq!(seen.borrow().posts(), 0, "{purpose}");
    let agent = committed(stages, &agent_stream());
    let rows: Vec<&StoredEvent> = agent
        .iter()
        .filter(|row| row.event_type == "DecisionMade")
        .collect();
    assert_eq!(rows.len(), 1, "{purpose}");
    assert_eq!(rows[0].schema_version, 2, "{purpose}");
    let decision = mandate_canon::parse(&rows[0].body).unwrap();
    for (field, expected) in [
        ("autonomy", "deny"),
        ("decided_by", "policy_overlay"),
        ("purpose", purpose),
    ] {
        let found = payload_field(&decision, field);
        assert_eq!(found.as_deref(), Some(expected), "{purpose}: {field}");
    }
    let refs = decision.get("config_refs").unwrap();
    for (field, digest) in [
        ("policy_set", governed.policy_set.content_hash),
        ("model_registry", governed.model_registry.content_hash),
    ] {
        let expected = format!("sha256:{digest}");
        let found = refs.get(field).and_then(Value::as_str);
        assert_eq!(found, Some(expected.as_str()), "{purpose}: {field}");
    }
    for event_type in ["IntentProposed", "ApprovalRequested"] {
        let found = of_type(&agent, event_type).len();
        assert_eq!(found, 0, "{purpose}: {event_type}");
    }
    let account = committed(stages, &account_stream());
    let types: Vec<&str> = account.iter().map(|row| row.event_type.as_str()).collect();
    assert_eq!(types, RECONCILED, "{purpose}");
    let (_, mut artifacts) = executor_artifacts("mandate.json");
    artifacts.extend(governed_store(governed));
    for stream in [agent_stream(), account_stream()] {
        let rows = committed(stages, &stream);
        verify_events(&rows, TrustedStart::GENESIS, &artifacts).unwrap();
    }
}

/// Mandate spec §6.2 step 5c, DEC-534 item 2, DEC-536 (D4d): an opening under a nonconforming
/// mandate is denied by the overlay, whatever the rules decided first: the E7-7 mandate's `auto`
/// rule and the ask fixture's `ask` rule both become `deny`, named `policy_overlay`. The run
/// records the deny and places nothing.
#[test]
fn an_opening_under_a_nonconforming_mandate_is_denied_by_the_policy_overlay() {
    let mut context = run_context(true);
    context.governance = Some(nonconforming());
    let opening = proposal_as(&mut context, Purpose::Open, RuntimePurpose::Open);
    for mandate in ["mandate.json", "mandate-ask.json"] {
        let answer = classify_under(mandate, &context, &opening);
        assert_eq!(answer, Ok(overlay_deny()), "{mandate}");
    }
    let governed = nonconforming();
    let store = Some(Arc::new(governed_store(&governed)) as Arc<dyn ArtifactSource + Send + Sync>);
    let (outcome, stages, seen) = governed_run_under("overlay-open", governed.clone(), store);
    assert_overlay_denied(outcome, &stages, &seen, &governed, "open");
}

/// DEC-534 item 2 (D4d): an increase is denied the same way. No fixture sizes an increase, since
/// the tracer's account starts flat, so the run's own sizing is retargeted to one.
#[test]
fn an_increase_under_a_nonconforming_mandate_is_denied_by_the_policy_overlay() {
    let mut context = run_context(true);
    context.governance = Some(nonconforming());
    let increase = proposal_as(&mut context, Purpose::Increase, RuntimePurpose::Increase);
    let answer = classify_under("mandate.json", &context, &increase);
    assert_eq!(answer, Ok(overlay_deny()));
    let governed = nonconforming();
    let store = Some(Arc::new(governed_store(&governed)) as Arc<dyn ArtifactSource + Send + Sync>);
    let increase = (Purpose::Increase, RuntimePurpose::Increase, Side::Buy);
    let (outcome, stages, seen) =
        retargeted_run("overlay-increase", Some(governed.clone()), store, increase);
    assert_overlay_denied(outcome, &stages, &seen, &governed, "increase");
}

/// What D4b reads for the deny fixture under the workspace level that forbids `auto`: its later
/// `routine` rule still says `auto`, so this mandate is nonconforming in its own right.
fn nonconforming_deny_fixture() -> Governance {
    let source = fs::read(fixtures().join("mandate-deny.json")).unwrap();
    let mandate = mandate_spec::Mandate::parse(&mandate_canon::parse(&source).unwrap()).unwrap();
    let no_auto = PolicyLevel {
        name: LevelName::Workspace,
        values: BTreeMap::from([(PolicyKey::AutoAllowed, PolicyValue::Flag(false))]),
    };
    let checked = mandate_spec::policy::check(&mandate, &[no_auto]).unwrap();
    assert!(!checked.violations.is_empty(), "the deny fixture conforms");
    Governance {
        overlay: checked.overlay,
        violations: checked.violations,
        ..nonconforming()
    }
}

/// Mandate spec §6.2 step 5c and DEC-536 item 2 (D4d): the overlay names itself only when it
/// changed the decision. Under the deny fixture, nonconforming because its `routine` rule says
/// `auto`, an opening that its first rule `no_opens` already denies stays `deny` labelled
/// `rule:no_opens`, never `policy_overlay`, and the run journals that label and places nothing.
/// An increase, which `routine` makes `auto`, is the overlay's own deny, so the same fixture
/// shows the label follows the step that decided.
#[test]
#[ignore = "pending E7-19"]
fn a_deny_the_rules_decided_under_a_nonconforming_mandate_keeps_the_rule_label() {
    let governed = nonconforming_deny_fixture();
    let by_rule = Classified {
        autonomy: Autonomy::Deny,
        decided_by: Some("rule:no_opens".to_owned()),
    };
    let mut context = run_context(true);
    context.governance = Some(governed.clone());
    let opening = proposal_as(&mut context, Purpose::Open, RuntimePurpose::Open);
    let answer = classify_under("mandate-deny.json", &context, &opening);
    assert_eq!(answer, Ok(by_rule));
    let mut context = run_context(true);
    context.governance = Some(governed.clone());
    let increase = proposal_as(&mut context, Purpose::Increase, RuntimePurpose::Increase);
    let answer = classify_under("mandate-deny.json", &context, &increase);
    assert_eq!(answer, Ok(overlay_deny()));
    let scratch = Scratch::new("deny-keeps-rule");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let store = Some(Arc::new(governed_store(&governed)) as Arc<dyn ArtifactSource + Send + Sync>);
    let mut context = run_context(true);
    context.governance = Some(governed);
    let mut stages = governed_stages("mandate-deny.json", dataset, transport, context, store);
    let error = refused(run(&mut stages, &setup(true)));
    let denied = matches!(
        &error,
        ShellError::Refused {
            stage: Stage::Classify,
            cause: Cause::NotAuto { autonomy: "deny" },
        }
    );
    assert!(denied, "{error}: {error:?}");
    assert_eq!(seen.borrow().posts(), 0, "the broker saw an order");
    let agent = committed(&stages, &agent_stream());
    let decisions = of_type(&agent, "DecisionMade");
    assert_eq!(decisions.len(), 1);
    for (field, expected) in [
        ("autonomy", "deny"),
        ("decided_by", "rule:no_opens"),
        ("purpose", "open"),
    ] {
        let found = payload_field(&decisions[0], field);
        assert_eq!(found.as_deref(), Some(expected), "{field}");
    }
}

/// `AGENTS.md` rules 3 and 13, DEC-534 item 2 (D4d): a discretionary exit goes through the whole
/// agent side of a run under a nonconforming mandate and under no governance at all. It is
/// decided `auto` by the built-in risk-reducing step, proposed, and handed to the account stream,
/// whose binding gate then decides it on the account's own fold.
#[test]
fn an_exit_is_decided_and_handed_under_a_nonconforming_or_ungoverned_run() {
    let governed = nonconforming();
    let store = Some(Arc::new(governed_store(&governed)) as Arc<dyn ArtifactSource + Send + Sync>);
    let cases = [
        ("nonconforming", Some(governed), store),
        ("ungoverned", None, None),
    ];
    for (name, governed, store) in cases {
        let exit = (
            Purpose::DiscretionaryExit,
            RuntimePurpose::DiscretionaryExit,
            Side::Sell,
        );
        let (outcome, stages, _) = retargeted_run("exit-through", governed, store, exit);
        let outcome = outcome.map_err(|error| format!("{error}: {error:?}"));
        assert!(outcome.is_ok(), "{name}: {outcome:?}");
        let agent = committed(&stages, &agent_stream());
        let decisions = of_type(&agent, "DecisionMade");
        assert_eq!(decisions.len(), 1, "{name}");
        for (field, expected) in [
            ("autonomy", "auto"),
            ("decided_by", "builtin_risk_reducing"),
            ("purpose", "discretionary_exit"),
        ] {
            let found = payload_field(&decisions[0], field);
            assert_eq!(found.as_deref(), Some(expected), "{name}: {field}");
        }
        assert_eq!(of_type(&agent, "IntentProposed").len(), 1, "{name}");
        let account = committed(&stages, &account_stream());
        assert_eq!(of_type(&account, "IntentReceived").len(), 1, "{name}");
    }
}
