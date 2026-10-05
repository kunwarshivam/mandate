//! E7-7's binding §9.1 gate: full trusted inputs in, `mandate-risk` decides.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    AGENT, FixedInstruments, FixedMandate, Shell, TestIds, config, handoff, opening, ports,
    stream_opened,
};
use mandate_executor::{
    BindingGateInput, BindingGateSource, BrokerRequest, ExecutorError, Ports,
};
use mandate_num::{Fraction, Price, Qty, Ratio, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AccountState, AccountType, AgentId, AgentMode, AgentSnapshot, AssetClass,
    AssetId, ConductState, DayTradeLedger, DayTradeRegime, EtpClass, Exchange, GateConfig,
    InstrumentSnapshot, MarketSnapshot, QuoteCurrency, RiskSnapshot, SaneQuote, ValidatedMandate,
    WorkingUniverse,
};
use mandate_time::UtcNanos;

const AAPL: &str = "AAPL";
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";

struct GateFixture {
    input: Option<BindingGateInput>,
}

impl GateFixture {
    fn allowing() -> Self {
        Self {
            input: Some(input(AccountState::Active, false)),
        }
    }

    fn denying() -> Self {
        Self {
            input: Some(input(AccountState::Active, true)),
        }
    }
}

impl BindingGateSource for GateFixture {
    fn input(
        &self,
        _agent: &mandate_executor::AgentId,
        _instrument: &mandate_accounting::InstrumentId,
    ) -> Option<BindingGateInput> {
        self.input.clone()
    }
}

fn decimal<T>(raw: &str, parse: impl FnOnce(&str) -> Result<T, mandate_num::NumError>) -> T {
    parse(raw).unwrap_or_else(|error| panic!("{raw} parses: {error}"))
}

fn usd(raw: &str) -> Usd {
    decimal(raw, Usd::parse)
}

fn qty(raw: &str) -> Qty {
    decimal(raw, Qty::parse)
}

fn price(raw: &str) -> Price {
    decimal(raw, Price::parse)
}

fn fraction(raw: &str) -> Fraction {
    decimal(raw, Fraction::parse)
}

fn ratio(raw: &str) -> Ratio {
    decimal(raw, Ratio::parse)
}

fn at(raw: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(raw).unwrap_or_else(|error| panic!("{raw} parses: {error}"))
}

fn input(state: AccountState, halted: bool) -> BindingGateInput {
    let asset = AssetId::new(AAPL).unwrap_or_else(|error| panic!("asset: {error}"));
    let limits = RiskLimits {
        max_position_usd: usd("100000"),
        max_position_fraction: fraction("1"),
        max_order_usd: usd("100000"),
        max_gross_exposure_usd: usd("100000"),
        max_orders_per_day: 100,
        reentry_cooldown_s: 0,
        rebalance_band: fraction("0.05"),
        breach_confirm_s: 60,
        drawdown_ladder: Vec::new(),
    };
    BindingGateInput {
        now: at("2026-09-21T15:00:00Z"),
        config: GateConfig {
            price_floor: usd("5"),
            liquidity_floor_usd: usd("1000000"),
            crypto_liquidity_floor_usd: usd("1000000"),
            collar_liquid_threshold_usd: usd("50000000"),
            collar_liquid_x: fraction("0.01"),
            collar_other_x: fraction("0.02"),
            collar_crypto_x: fraction("0.02"),
            collar_passive_band: fraction("0.2"),
            opposite_fill_interval_s: 60,
            min_resting_time_s: 2,
            order_to_fill_max: 10,
            order_to_fill_min_orders: 20,
            order_size_participation: fraction("0.05"),
            daily_participation: fraction("0.05"),
            close_window_minutes: 10,
            legacy_pdt_equity_threshold: usd("25000"),
            etp_classification_max_age_s: 604_800,
        },
        mandate: ValidatedMandate::from_validated_parts(
            limits,
            GoalState::Running,
            false,
            false,
        ),
        risk: RiskSnapshot {
            agent_equity: usd("10000"),
            high_water_mark: usd("10000"),
            day_start_equity: usd("10000"),
            capital_base: usd("10000"),
            inherited_loss: Usd::ZERO,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: ratio("1"),
            agent_mode: AgentMode::Normal,
        },
        account: AccountSnapshot {
            account_type: AccountType::Margin,
            state,
            crypto_active: true,
            regime: DayTradeRegime::IntradayMargin {
                maintenance_excess: usd("100000"),
            },
            equity: usd("10000"),
            prior_close_equity: usd("10000"),
            model_buying_power: usd("100000"),
            broker_buying_power: usd("100000"),
            broker_non_marginable_buying_power: usd("100000"),
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        },
        agent: AgentSnapshot {
            agent: AgentId(1),
            mode: AgentMode::Normal,
            instrument_restrictions: BTreeMap::new(),
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            last_exit_fill_at: BTreeMap::new(),
            orders_today: 0,
            day_trades: DayTradeLedger::default(),
        },
        instrument: InstrumentSnapshot {
            instrument: asset.clone(),
            asset_class: AssetClass::UsEquity,
            exchange: Some(Exchange::Nasdaq),
            status_active: true,
            tradable: true,
            fractionable: false,
            ipo: false,
            ptp_no_exception: false,
            etp: EtpClass::Plain,
            etp_classified_at: Some(at("2026-09-21T00:00:00Z")),
            quote_currency: Some(QuoteCurrency::Usd),
            prior_close: Some(price("150")),
            median_dollar_volume_20d: Some(usd("90000000")),
            median_dollar_volume_30d: None,
            min_order_size: qty("1"),
            qty_increment: qty("1"),
            halted,
            status_feed_current: true,
        },
        market: MarketSnapshot {
            quote: Some(SaneQuote {
                bid: price("149.98"),
                ask: price("150.02"),
                at: at("2026-09-21T14:59:59Z"),
            }),
            last_trade: Some((price("150"), at("2026-09-21T14:59:59Z"))),
            trailing_5m_volume: Some(qty("100000")),
            adv_20d: Some(qty("1000000")),
        },
        conduct: ConductState::default(),
        universe: WorkingUniverse::Known {
            instruments: BTreeSet::from([asset]),
            pinned: true,
        },
        asset: AssetId::new(AAPL).unwrap_or_else(|error| panic!("asset: {error}")),
        gate_agent: AgentId(1),
        gate_client_order_id: mandate_risk::ClientOrderId(1),
        owner_confirmed_bid: None,
        fee_reservation: Usd::ZERO,
        data_profile: "iex".to_owned(),
        feed: "iex".to_owned(),
    }
}

fn bound_ports<'a>(
    ids: &'a TestIds,
    mandates: &'a FixedMandate,
    instruments: &'a FixedInstruments,
    config: &'a mandate_executor::ExecutorConfig,
    gate: Option<&'a dyn BindingGateSource>,
) -> Ports<'a> {
    let mut ports = ports(ids, mandates, instruments, config);
    ports.binding_gate = gate;
    ports
}

fn ready(gate: Option<&dyn BindingGateSource>) -> (Shell, Ports<'_>) {
    let ids = Box::leak(Box::new(TestIds));
    let mandates = Box::leak(Box::new(FixedMandate::covering(&[AAPL])));
    let instruments = Box::leak(Box::new(FixedInstruments));
    let executor_config = Box::leak(Box::new(config()));
    let ports = bound_ports(ids, mandates, instruments, executor_config, gate);
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).unwrap_or_else(|error| panic!("{error}"));
    (shell.restart_ready(&ports), ports)
}

fn submitted(ran: &common::Ran) -> bool {
    ran.requests
        .iter()
        .any(|request| matches!(request, BrokerRequest::Submit(_)))
}

#[test]
fn full_binding_input_allows_and_replaces_the_partial_evaluation() {
    let gate = GateFixture::allowing();
    let (mut shell, ports) = ready(Some(&gate));
    let ran = shell.run(handoff(INTENT, AGENT, opening(AAPL, "1", "150")), &ports);
    assert!(submitted(&ran));
    let decided = ran.draft("GateDecided").unwrap_or_else(|| panic!("gate decision"));
    assert_eq!(decided.payload.get("verdict").and_then(|v| v.as_str()), Some("allow"));
    assert!(
        decided.payload.get("evaluation").is_none(),
        "a full §9.1 decision is never labelled account_stream_only"
    );
}

#[test]
fn binding_denial_sends_no_order_even_when_the_account_stream_checks_allow() {
    let gate = GateFixture::denying();
    let (mut shell, ports) = ready(Some(&gate));
    let ran = shell.run(handoff(INTENT, AGENT, opening(AAPL, "1", "150")), &ports);
    assert!(!submitted(&ran));
    let decided = ran.draft("GateDecided").unwrap_or_else(|| panic!("gate decision"));
    assert_eq!(decided.payload.get("verdict").and_then(|v| v.as_str()), Some("deny"));
    assert_eq!(
        decided.payload.get("reason_code").and_then(|v| v.as_str()),
        Some("instrument_halted")
    );
}

#[test]
fn missing_binding_input_fails_closed_before_any_submission() {
    let (mut shell, ports) = ready(None);
    let error = shell
        .step(handoff(INTENT, AGENT, opening(AAPL, "1", "150")), &ports)
        .expect_err("an opening without §9.1 inputs must fail closed");
    assert_eq!(error, ExecutorError::BindingGateInputMissing);
    assert_eq!(shell.connector.total_accepted(), 0);
}

#[test]
fn replay_keeps_a_binding_denial_terminal() {
    let gate = GateFixture::denying();
    let (mut shell, ports) = ready(Some(&gate));
    let first = shell.run(handoff(INTENT, AGENT, opening(AAPL, "1", "150")), &ports);
    assert!(!submitted(&first));
    let mut replayed = shell.restart_ready(&ports);
    let again = replayed.run(handoff(INTENT, AGENT, opening(AAPL, "1", "150")), &ports);
    assert!(again.effects.is_empty());
    assert_eq!(replayed.connector.total_accepted(), 0);
}

#[test]
fn an_advisory_allow_has_no_api_that_can_override_the_binding_denial() {
    let advisory = mandate_executor::GateVerdict::Allow;
    assert_eq!(advisory, mandate_executor::GateVerdict::Allow);
    let gate = GateFixture::denying();
    let (mut shell, ports) = ready(Some(&gate));
    let ran = shell.run(handoff(INTENT, AGENT, opening(AAPL, "1", "150")), &ports);
    assert!(!submitted(&ran));
}
