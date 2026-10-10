//! Fixtures, an in-memory shell, and a fake connector, shared by the hand cases, the properties,
//! and the fault-injection suite.
//!
//! The shell is the task brief's thin process shell reduced to what a test needs: it folds a
//! journal, runs an effect list in order, drives the connector, and can crash at a named point
//! and restart. It holds no clock, no randomness, and no I/O, so a test is as deterministic as
//! the core it drives (ADR-0001 ES-19, ES-21).
//!
//! **The fake connector's counters are the first oracle.** It keeps its own
//! `BTreeMap<String, u32>` of how many distinct submissions it accepted per client order id and a
//! `BTreeSet` of the order bodies it saw, accumulated independently of the executor's state, so
//! "zero duplicates" is read off the broker side rather than off the journal: an executor that
//! journals one `OrderSubmitted` and sends twice still fails.
//!
//! Every decimal here is **canonical** text (`mandate_canon::DecStr`: no exponent, no trailing
//! fractional zeros, `0` for zero), so a fixture can never fail a test before the crate does.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mandate_accounting::{AccountType, AssetClass, InstrumentId, Side};
use mandate_canon::{Int, Key, Value};
use mandate_executor::{
    AccountRef, AccountScope, AgentId, BindingGateConfigRefs, BindingGateInput, BindingGateRequest,
    BindingGateSource, BrokerAccount, BrokerFill, BrokerOrder, BrokerOutcome, BrokerPosition,
    BrokerReject, BrokerRequest, BrokerSnapshot, BrokerUnknown, Effect, EventDraft, EventId,
    ExecutorConfig, ExecutorError, ExecutorState, ExitTier, FillId, FoldedEvent, IdGen, Input,
    InstrumentSnapshot, IntentId, MandateVersion, MandateView, MarketObservation, OrderState,
    Ports, ReconcileReason, RiskClock, Seq, TimeInForce, TimerId, TimerRequest, WorkspaceId,
    WriterEpoch, fold, handle,
};
use mandate_num::{Fraction, Price, Qty, Ratio, ShareIncrement, SignedQty, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits};
use mandate_risk::{
    AccountSnapshot, AccountState as GateAccountState, AgentId as GateAgentId, AgentMode,
    AgentSnapshot, AssetId, ClientOrderId as GateOrderId, ConductState, DayTradeLedger,
    DayTradeRegime, EtpClass, Exchange, GateConfig, InstrumentSnapshot as GateInstrumentSnapshot,
    MarketSnapshot, QuoteCurrency, RiskSnapshot, SaneQuote, ValidatedMandate, WorkingUniverse,
};
use mandate_time::{Date, UtcNanos};

pub mod golden;

pub const ACCOUNT_STREAM: &str = "acct:ws1:acct-1";
pub const AGENT_STREAM: &str = "agent:ws1:agent-a";
pub const OTHER_AGENT_STREAM: &str = "agent:ws1:agent-b";
pub const CONTROL_STREAM: &str = "ctl:ws1";
pub const CLOCK_STREAM: &str = "clock:ws1";
pub const ACCOUNT: &str = "acct-1";
pub const WORKSPACE: &str = "ws1";
pub const AGENT: &str = "agent-a";
pub const OTHER_AGENT: &str = "agent-b";
pub const VERSION: &str = "sha256:5555555555555555555555555555555555555555555555555555555555555555";
pub const ENVIRONMENT: &str = "paper";

pub struct MissingBindingGate;

impl BindingGateSource for MissingBindingGate {
    fn input(&self, _request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        None
    }
}

pub static MISSING_BINDING_GATE: MissingBindingGate = MissingBindingGate;

pub struct AllowingBindingGate;

impl BindingGateSource for AllowingBindingGate {
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if !request.purpose.adds_risk() {
            return None;
        }
        let asset = AssetId::new(request.instrument.as_str()).ok()?;
        let at = UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z").ok()?;
        let quote_at = UtcNanos::parse_rfc3339("2026-09-21T14:59:59Z").ok()?;
        let held = Qty::parse("1000000").ok()?;
        let equity = Usd::parse("1000000000").ok()?;
        let (asset_class, exchange) = if request.instrument.as_str() == FixedInstruments::CRYPTO {
            (AssetClass::Crypto, None)
        } else {
            (AssetClass::UsEquity, Some(Exchange::Nasdaq))
        };
        let positions = BTreeMap::new();
        let gate_agent = GateAgentId(1);
        Some(BindingGateInput {
            config_refs: BindingGateConfigRefs::complete(
                format!("sha256:{}", "1".repeat(64)),
                format!("sha256:{}", "2".repeat(64)),
                format!("sha256:{}", "3".repeat(64)),
                format!("sha256:{}", "4".repeat(64)),
                VERSION,
            ),
            now: at,
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
                order_size_participation: Fraction::parse("1").ok()?,
                daily_participation: Fraction::parse("1").ok()?,
                close_window_minutes: 0,
                legacy_pdt_equity_threshold: Usd::ZERO,
                etp_classification_max_age_s: u32::MAX,
            },
            mandate: ValidatedMandate::from_validated_parts(
                RiskLimits {
                    max_position_usd: equity,
                    max_position_fraction: Fraction::parse("1").ok()?,
                    max_order_usd: equity,
                    max_gross_exposure_usd: equity,
                    max_orders_per_day: u32::MAX,
                    reentry_cooldown_s: 0,
                    rebalance_band: Fraction::parse("1").ok()?,
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
                account_type: AccountType::Margin,
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
                asset_class,
                exchange,
                status_active: true,
                tradable: true,
                fractionable: true,
                ipo: false,
                ptp_no_exception: false,
                etp: EtpClass::Plain,
                etp_classified_at: Some(at),
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
                trailing_5m_volume: Some(held),
                adv_20d: Some(held),
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
            data_profile: "test".to_owned(),
            feed: "test".to_owned(),
        })
    }
}

pub static ALLOWING_BINDING_GATE: AllowingBindingGate = AllowingBindingGate;

/// The broker account every fixture is for.
pub fn scope() -> AccountScope {
    AccountScope {
        account: AccountRef(ACCOUNT.to_owned()),
        workspace: WorkspaceId(WORKSPACE.to_owned()),
    }
}

pub fn agent(name: &str) -> AgentId {
    AgentId(name.to_owned())
}

pub fn instrument(name: &str) -> InstrumentId {
    InstrumentId::new(name).unwrap_or_else(|e| panic!("instrument {name}: {e}"))
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("qty {text}: {e}"))
}

pub fn signed_qty(text: &str) -> SignedQty {
    SignedQty::parse(text).unwrap_or_else(|e| panic!("signed qty {text}: {e}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("price {text}: {e}"))
}

pub fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("usd {text}: {e}"))
}

pub fn fraction(text: &str) -> Fraction {
    Fraction::parse(text).unwrap_or_else(|e| panic!("fraction {text}: {e}"))
}

pub fn date(text: &str) -> Date {
    Date::parse(text).unwrap_or_else(|e| panic!("date {text}: {e}"))
}

pub fn key(name: &str) -> Key {
    Key::new(name).unwrap_or_else(|e| panic!("key {name}: {e}"))
}

pub fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

pub fn int(n: u64) -> Value {
    Value::Int(Int::new(n).unwrap_or_else(|| panic!("int {n} out of range")))
}

pub fn object(pairs: &[(&str, Value)]) -> Value {
    Value::Object(
        pairs
            .iter()
            .map(|(k, v)| (key(k), v.clone()))
            .collect::<BTreeMap<_, _>>(),
    )
}

pub fn clock(secs: i64) -> RiskClock {
    RiskClock::from_secs(secs)
}

/// One event as the journal would hand it back: already validated and sequenced.
pub fn event(stream: &str, seq: u64, event_type: &str, payload: Value) -> FoldedEvent {
    FoldedEvent {
        stream: stream.to_owned(),
        seq: Seq(seq),
        event_id: EventId(format!("{stream}-{seq}")),
        event_type: event_type.to_owned(),
        causation_id: None,
        payload,
    }
}

/// The same event, carrying the `causation_id` a copied cross-stream fact needs (journal §2).
pub fn copied(
    stream: &str,
    seq: u64,
    event_type: &str,
    payload: Value,
    origin: &EventId,
) -> FoldedEvent {
    FoldedEvent {
        causation_id: Some(origin.clone()),
        ..event(stream, seq, event_type, payload)
    }
}

/// Every account-stream risk input carries `risk_clock` (journal §2, mandate spec §5.2).
pub fn with_clock(pairs: &[(&str, Value)], at: i64) -> Value {
    let mut all = pairs.to_vec();
    let secs = u64::try_from(at).unwrap_or_else(|_| panic!("negative risk clock {at}"));
    all.push(("risk_clock", int(secs)));
    object(&all)
}

/// `StreamOpened`, which fixes the stream's `environment` for good (ADR-0001 ES-23).
pub fn stream_opened() -> FoldedEvent {
    event(
        ACCOUNT_STREAM,
        1,
        "StreamOpened",
        object(&[
            ("environment", text(ENVIRONMENT)),
            ("stream_type", text("account")),
            ("subject", text(ACCOUNT)),
        ]),
    )
}

/// The oracle's own derivation of an event id, written separately from the crate's so that the
/// two agreeing means something (DEC-131 item 6). It is alphanumeric, as a production ULID is, so
/// a protective order named for the `ProtectionChanged` that records it (`{entry}-p{protection}`,
/// trading-domain spec §2.3) can be derived from it; `ClientOrderId::for_protection` refuses any
/// other id rather than escaping it (DEC-345).
pub fn derived_id(epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
    EventId(format!("e{}h{}o{}", epoch.0, head.0, ordinal))
}

pub struct TestIds;

impl IdGen for TestIds {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        derived_id(epoch, head, ordinal)
    }
}

/// The mandate view stream F will provide, reduced to what this crate reads.
pub struct FixedMandate {
    pub universe: BTreeSet<InstrumentId>,
    pub crypto_offset: Fraction,
}

impl FixedMandate {
    pub fn covering(instruments: &[&str]) -> Self {
        Self {
            universe: instruments.iter().map(|n| instrument(n)).collect(),
            crypto_offset: fraction("0.005"),
        }
    }
}

impl MandateView for FixedMandate {
    fn version(&self, _agent: &AgentId) -> Option<MandateVersion> {
        Some(MandateVersion(VERSION.to_owned()))
    }

    fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
        Some(self.crypto_offset)
    }

    fn covers(&self, _agent: &AgentId, instrument: &InstrumentId) -> bool {
        self.universe.contains(instrument)
    }
}

/// The instrument snapshot: `AAPL` a whole-share liquid equity, `CPHC` a thin one, `BTC/USD`
/// crypto, and `FRAC` a fractionable equity. The three exit tiers are trading-domain spec §5.6's
/// table transcribed as data, which is what interpretation 22 says they are.
pub struct FixedInstruments;

impl FixedInstruments {
    pub const LIQUID_EQUITY: &'static str = "AAPL";
    pub const THIN_EQUITY: &'static str = "CPHC";
    pub const CRYPTO: &'static str = "BTCUSD";
    pub const FRACTIONABLE: &'static str = "FRAC";
}

impl InstrumentSnapshot for FixedInstruments {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        match instrument.as_str() {
            FixedInstruments::CRYPTO => Some(AssetClass::Crypto),
            FixedInstruments::LIQUID_EQUITY
            | FixedInstruments::THIN_EQUITY
            | FixedInstruments::FRACTIONABLE => Some(AssetClass::UsEquity),
            _ => None,
        }
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        match instrument.as_str() {
            FixedInstruments::FRACTIONABLE | FixedInstruments::CRYPTO => {
                Some(ShareIncrement::Fractional)
            }
            FixedInstruments::LIQUID_EQUITY | FixedInstruments::THIN_EQUITY => {
                Some(ShareIncrement::Whole)
            }
            _ => None,
        }
    }

    fn exit_tier(&self, instrument: &InstrumentId) -> Option<ExitTier> {
        match instrument.as_str() {
            FixedInstruments::LIQUID_EQUITY => Some(ExitTier {
                exit_offset: fraction("0.005"),
                exit_offset_step: fraction("0.005"),
                max_exit_offset: fraction("0.03"),
            }),
            FixedInstruments::THIN_EQUITY | FixedInstruments::FRACTIONABLE => Some(ExitTier {
                exit_offset: fraction("0.01"),
                exit_offset_step: fraction("0.01"),
                max_exit_offset: fraction("0.05"),
            }),
            FixedInstruments::CRYPTO => Some(ExitTier {
                exit_offset: fraction("0.01"),
                exit_offset_step: fraction("0.01"),
                max_exit_offset: fraction("0.05"),
            }),
            _ => None,
        }
    }
}

/// The parameters of trading-domain spec §5.4, §5.6, and §5.7, with the five the spec names
/// without a value set to the task brief's proposed defaults (Decisions needed 2, still
/// `Proposed (founder)`). They live here rather than in the crate because they are configuration,
/// not constants (interpretation 22).
pub fn config() -> ExecutorConfig {
    ExecutorConfig {
        max_intent_age_s: 120,
        unknown_absent_lookups: 3,
        unknown_absent_window_s: 15,
        protective_replace_buffer_trading_days: 5,
        restriction_403_threshold: 3,
        bracket_partial_fill_timeout_s: 60,
        max_unprotected_s: 60,
        stop_watchdog_s: 60,
        exit_step_s: 5,
        gtc_expiry_days: 90,
    }
}

/// The `test_default` fee configuration of the trading-domain reference cases, transcribed:
/// SEC 0.00003 × proceeds on sells, TAF 0.0002 per share sold capped at 9.79 per execution, CAT
/// 0.00001 per share on both sides, and crypto 15/25 bps, over the `us_2026` calendar (trading
/// spec §6.2, §6.3). It is the `fee` configuration ref the executor reads to book paper's
/// simulated regulatory fees (§10).
pub fn fee_config() -> &'static mandate_accounting::Config {
    static FEES: std::sync::OnceLock<mandate_accounting::Config> = std::sync::OnceLock::new();
    FEES.get_or_init(|| mandate_accounting::Config {
        equities: mandate_accounting::EquityFees {
            sec_rate: mandate_num::FeeRate::parse("0.00003").unwrap_or_else(|e| panic!("{e}")),
            taf_per_share: mandate_num::FeePerShare::parse("0.0002")
                .unwrap_or_else(|e| panic!("{e}")),
            taf_cap: mandate_num::FeeCap::parse("9.79").unwrap_or_else(|e| panic!("{e}")),
            taf_cap_basis: mandate_accounting::TafCapBasis::PerExecution,
            cat_per_share: mandate_num::FeePerShare::parse("0.00001")
                .unwrap_or_else(|e| panic!("{e}")),
        },
        crypto: mandate_accounting::CryptoFees {
            maker: mandate_num::Bps::parse("15").unwrap_or_else(|e| panic!("{e}")),
            taker: mandate_num::Bps::parse("25").unwrap_or_else(|e| panic!("{e}")),
        },
        calendar: mandate_time::TradingCalendar::new(
            date("2026-09-01"),
            date("2026-12-31"),
            [date("2026-11-26"), date("2026-12-25")],
            [date("2026-10-12"), date("2026-11-11")],
        )
        .unwrap_or_else(|e| panic!("the us_2026 calendar: {e}")),
    })
}

pub fn ports<'a>(
    ids: &'a TestIds,
    mandates: &'a FixedMandate,
    instruments: &'a FixedInstruments,
    config: &'a ExecutorConfig,
) -> Ports<'a> {
    Ports {
        ids,
        mandates,
        instruments,
        config,
        fees: fee_config(),
    }
}

/// One order as the broker would describe it, with a canonical decimal on every number.
pub fn broker_order(
    broker_id: &str,
    client_order_id: Option<&str>,
    name: &str,
    side: Side,
    quantity: &str,
    filled: &str,
    status: &str,
) -> BrokerOrder {
    BrokerOrder {
        broker_order_id: broker_id.to_owned(),
        client_order_id: client_order_id.map(str::to_owned),
        instrument: instrument(name),
        side,
        qty: qty(quantity),
        filled_qty: qty(filled),
        limit_price: Some(price("155")),
        stop_price: None,
        status: status.to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: Some(date("2026-09-22")),
    }
}

pub fn broker_fill(
    id: &str,
    client_order_id: Option<&str>,
    quantity: &str,
    at: &str,
) -> BrokerFill {
    BrokerFill {
        fill_id: FillId(id.to_owned()),
        client_order_id: client_order_id.map(str::to_owned),
        instrument: instrument(FixedInstruments::LIQUID_EQUITY),
        side: Side::Buy,
        qty: qty(quantity),
        price: price(at),
        fees: usd("0"),
        trade_date: date("2026-09-22"),
    }
}

pub fn broker_position(name: &str, quantity: &str) -> BrokerPosition {
    BrokerPosition {
        instrument: instrument(name),
        qty: signed_qty(quantity),
        avg_entry_price: price("150"),
    }
}

/// An `ACTIVE` account with no restriction flag set.
pub fn broker_account() -> BrokerAccount {
    BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "ACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 1,
        equity: usd("20000"),
        cash: usd("20000"),
        buying_power: usd("20000"),
        non_marginable_buying_power: usd("20000"),
        accrued_fees: usd("0"),
        last_equity: usd("20000"),
        maintenance_margin: usd("0"),
    }
}

pub fn broker_reject(
    client_order_id: Option<&str>,
    http_status: u16,
    message: &str,
) -> BrokerReject {
    BrokerReject {
        client_order_id: client_order_id.map(str::to_owned),
        http_status,
        code: None,
        message: message.to_owned(),
    }
}

/// A reconciliation snapshot taken at `head`, which is the `expected_head` the run is appended
/// with (interpretation 15).
pub fn snapshot(head: u64, reason: ReconcileReason) -> BrokerSnapshot {
    BrokerSnapshot {
        open_orders: Vec::new(),
        positions: Vec::new(),
        account: broker_account(),
        fills: Vec::new(),
        cursor: mandate_executor::ActivityCursor("cursor-1".to_owned()),
        reason,
        taken_at_head: Seq(head),
    }
}

/// A fresh, sane quote, which is the ladder's first reference (§5.6).
pub fn quote(name: &str, bid: &str, ask: &str, at: i64) -> MarketObservation {
    MarketObservation {
        instrument: instrument(name),
        bid: Some(price(bid)),
        bid_size: Some(qty("100")),
        ask: Some(price(ask)),
        last_trade: Some(price(bid)),
        mark: Some(price(bid)),
        sane: true,
        observed_at: clock(at),
    }
}

/// The same quote marked not sane, which is what makes the ladder fall back (§5.6).
pub fn stale_quote(name: &str, bid: &str, at: i64) -> MarketObservation {
    MarketObservation {
        sane: false,
        ..quote(name, bid, bid, at)
    }
}

/// An intent handoff, as the shell's adapter takes it from stream I's sink.
pub fn handoff(intent: &str, who: &str, body: mandate_executor::IntentBody) -> Input {
    Input::Intent(mandate_executor::IntentHandoff {
        intent_id: IntentId(EventId(intent.to_owned())),
        agent: agent(who),
        tif: Some(TimeInForce::Day),
        body,
    })
}

/// An opening limit buy that carries **no** protective prices, the ordinary case every
/// idempotency test starts from. The executor sends it as a plain order and never invents a
/// bracket for it (DEC-133's ruling on protective prices).
pub fn opening(name: &str, quantity: &str, limit: &str) -> mandate_executor::IntentBody {
    mandate_executor::IntentBody::Order {
        instrument: instrument(name),
        side: Side::Buy,
        qty: qty(quantity),
        limit: price(limit),
        purpose: mandate_executor::Purpose::Open,
        protection: None,
    }
}

/// An opening limit buy carrying the protective prices the order builder computed from the
/// mandate's distances (mandate spec §3, RC-14, RC-21): a stop, and a take-profit unless the
/// instrument is crypto, whose take-profit the runtime watches.
pub fn protected_opening(
    name: &str,
    quantity: &str,
    limit: &str,
    stop: &str,
    take_profit: Option<&str>,
) -> mandate_executor::IntentBody {
    mandate_executor::IntentBody::Order {
        instrument: instrument(name),
        side: Side::Buy,
        qty: qty(quantity),
        limit: price(limit),
        purpose: mandate_executor::Purpose::Open,
        protection: Some(mandate_executor::ProtectionPrices {
            stop: price(stop),
            take_profit: take_profit.map(price),
        }),
    }
}

/// The same entry as an add to a position that is already held (§5.4's tranche model).
pub fn protected_add(
    name: &str,
    quantity: &str,
    limit: &str,
    stop: &str,
    take_profit: Option<&str>,
) -> mandate_executor::IntentBody {
    match protected_opening(name, quantity, limit, stop, take_profit) {
        mandate_executor::IntentBody::Order {
            instrument,
            side,
            qty,
            limit,
            protection,
            ..
        } => mandate_executor::IntentBody::Order {
            instrument,
            side,
            qty,
            limit,
            purpose: mandate_executor::Purpose::Increase,
            protection,
        },
        other @ mandate_executor::IntentBody::Flatten(_) => other,
    }
}

/// A risk-reducing sell, which `AGENTS.md` rule 13 exempts from every pacing control.
pub fn risk_exit(name: &str, quantity: &str, limit: &str) -> mandate_executor::IntentBody {
    mandate_executor::IntentBody::Order {
        instrument: instrument(name),
        side: Side::Sell,
        qty: qty(quantity),
        limit: price(limit),
        purpose: mandate_executor::Purpose::RiskExit,
        protection: None,
    }
}

/// A discretionary sell, which conduct controls may pace but never deny.
pub fn discretionary_exit(name: &str, quantity: &str, limit: &str) -> mandate_executor::IntentBody {
    mandate_executor::IntentBody::Order {
        instrument: instrument(name),
        side: Side::Sell,
        qty: qty(quantity),
        limit: price(limit),
        purpose: mandate_executor::Purpose::DiscretionaryExit,
        protection: None,
    }
}

/// A `ProtectionChanged`'s instrument: §9.5's `instrument_id` or legacy `instrument` (DEC-446).
pub fn protected_instrument(payload: &Value) -> Option<&str> {
    payload
        .get("instrument_id")
        .or_else(|| payload.get("instrument"))
        .and_then(Value::as_str)
}

/// The client order ids a member names: §9.5's list or legacy joined text (DEC-446 item 7).
pub fn named_orders(payload: &Value, member: &str) -> Vec<String> {
    match payload.get(member) {
        Some(Value::Array(ids)) => ids
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        Some(Value::Str(joined)) => joined
            .split([' ', ','])
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

/// Whether a request is one of the broker's two account-wide endpoints, read off the variant
/// rather than off anything the crate reports about itself.
pub fn is_account_wide(request: &BrokerRequest) -> bool {
    matches!(
        request,
        BrokerRequest::CancelAll(_) | BrokerRequest::ClosePosition(_, _)
    )
}

/// What the shell did with one effect list, so a test can assert on order as well as content.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ran {
    pub drafts: Vec<EventDraft>,
    pub requests: Vec<BrokerRequest>,
    pub timers: Vec<TimerRequest>,
    pub notifications: Vec<&'static str>,
    pub effects: Vec<Effect>,
}

impl Ran {
    pub fn draft_types(&self) -> Vec<&str> {
        self.drafts.iter().map(|d| d.event_type.as_str()).collect()
    }

    pub fn draft(&self, event_type: &str) -> Option<&EventDraft> {
        self.drafts.iter().find(|d| d.event_type == event_type)
    }

    pub fn submissions(&self) -> Vec<&mandate_executor::SubmitOrder> {
        self.requests
            .iter()
            .filter_map(|r| match r {
                BrokerRequest::Submit(order) => Some(order),
                _ => None,
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

/// How an append answered, so a test can put a batch in doubt (journal spec §5.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppendOutcome {
    Committed,
    Unresolved,
    Fenced,
    /// The head moved under the batch, which is what a `ReconciliationRun` appended at a stale
    /// snapshot head answers (interpretation 15).
    HeadMismatch,
}

/// The twelve points of the task brief's fault-injection table, each a named position in one
/// submission's effect pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashPoint {
    BeforeIntentReceived,
    IntentReceivedBeforeGate,
    GateBeforeOrderSubmitted,
    JournalBeforeRequest,
    RequestNoResponse,
    RequestAmbiguousResponse,
    ResponseBeforeStateChange,
    StateChangeBeforeFill,
    CancelBeforeConfirmation,
    ConfirmationBeforeExitSubmit,
    BetweenEntryFillAndOco,
    MidReconciliationBeforeCompensatingEvent,
}

impl CrashPoint {
    /// Every point, so a property can draw one and a suite can walk them all.
    pub const ALL: [Self; 12] = [
        Self::BeforeIntentReceived,
        Self::IntentReceivedBeforeGate,
        Self::GateBeforeOrderSubmitted,
        Self::JournalBeforeRequest,
        Self::RequestNoResponse,
        Self::RequestAmbiguousResponse,
        Self::ResponseBeforeStateChange,
        Self::StateChangeBeforeFill,
        Self::CancelBeforeConfirmation,
        Self::ConfirmationBeforeExitSubmit,
        Self::BetweenEntryFillAndOco,
        Self::MidReconciliationBeforeCompensatingEvent,
    ];

    /// Whether the shell stops **before** running this effect. Written from the table rather than
    /// from the crate, so the point a test names is the point the spec names.
    pub fn stops_before(self, effect: &Effect) -> bool {
        match (self, effect) {
            (Self::BeforeIntentReceived, Effect::Journal(d)) => d.event_type == "IntentReceived",
            (Self::IntentReceivedBeforeGate, Effect::Journal(d)) => d.event_type == "GateDecided",
            (Self::GateBeforeOrderSubmitted, Effect::Journal(d)) => {
                d.event_type == "OrderSubmitted"
            }
            (Self::JournalBeforeRequest, Effect::Broker(BrokerRequest::Submit(_))) => true,
            (Self::ResponseBeforeStateChange, Effect::Journal(d)) => {
                d.event_type == "OrderStateChanged"
            }
            (Self::StateChangeBeforeFill, Effect::Journal(d)) => d.event_type == "FillApplied",
            (Self::CancelBeforeConfirmation, Effect::Broker(BrokerRequest::Cancel { .. })) => true,
            (Self::ConfirmationBeforeExitSubmit, Effect::Broker(BrokerRequest::Submit(_))) => true,
            (Self::BetweenEntryFillAndOco, Effect::Journal(d)) => {
                d.event_type == "ProtectionChanged"
            }
            (Self::MidReconciliationBeforeCompensatingEvent, Effect::Journal(d)) => {
                d.event_type == "CompensatingEvent"
            }
            _ => false,
        }
    }

    /// Two of the twelve are not a stopped effect list but an answer the connector gave: the
    /// request left and nothing came back (5), or something ambiguous did (6).
    pub fn broker_answer(self) -> Option<Result<BrokerOutcome, BrokerUnknown>> {
        match self {
            Self::RequestNoResponse => Some(Err(BrokerUnknown::Timeout)),
            Self::RequestAmbiguousResponse => Some(Err(BrokerUnknown::Ambiguous)),
            _ => None,
        }
    }
}

/// The fake connector: scripted, in-process, deterministic, and its own oracle.
///
/// `accepted` counts distinct submissions per client order id and `bodies` records each order
/// body it saw, both accumulated here rather than derived from the journal. "Zero duplicates" is
/// read off this map (task brief, Oracles 1).
#[derive(Debug, Default)]
pub struct FakeConnector {
    pub accepted: BTreeMap<String, u32>,
    pub bodies: BTreeSet<String>,
    pub requests: Vec<String>,
    replies: VecDeque<Result<BrokerOutcome, BrokerUnknown>>,
}

impl FakeConnector {
    pub fn serving(
        replies: impl IntoIterator<Item = Result<BrokerOutcome, BrokerUnknown>>,
    ) -> Self {
        Self {
            replies: replies.into_iter().collect(),
            ..Self::default()
        }
    }

    pub fn push(&mut self, reply: Result<BrokerOutcome, BrokerUnknown>) {
        self.replies.push_back(reply);
    }

    /// How many distinct submissions the broker accepted for one client order id. Anything above
    /// one is a duplicate, whatever the journal says.
    pub fn accepted_for(&self, client_order_id: &str) -> u32 {
        self.accepted.get(client_order_id).copied().unwrap_or(0)
    }

    pub fn total_accepted(&self) -> u32 {
        self.accepted.values().sum()
    }

    /// The counter's own rendering of one request, independent of the crate's `Debug`.
    pub fn render(request: &BrokerRequest) -> String {
        match request {
            BrokerRequest::Submit(order) => format!(
                "submit {} {} {} {} {:?} {:?}",
                order.client_order_id.as_str(),
                order.instrument.as_str(),
                order.qty,
                order.side == Side::Buy,
                order.order_type,
                order.tif
            ),
            BrokerRequest::Cancel { client_order_id } => {
                format!("cancel {}", client_order_id.as_str())
            }
            BrokerRequest::AcknowledgeReplace { replaced } => {
                format!("acknowledge_replace {}", replaced.as_str())
            }
            BrokerRequest::GetOrderByClientId(id) => format!("get {}", id.as_str()),
            BrokerRequest::ListOrders(listing) => {
                format!("list {}", listing.client_order_id.as_str())
            }
            BrokerRequest::ListOpenOrders => "list_open_orders".to_owned(),
            BrokerRequest::ListPositions => "list_positions".to_owned(),
            BrokerRequest::GetAccount => "get_account".to_owned(),
            BrokerRequest::ListActivities { since } => format!("list_activities {}", since.0),
            BrokerRequest::CancelAll(_) => "cancel_all".to_owned(),
            BrokerRequest::ClosePosition(_, name) => format!("close_position {}", name.as_str()),
        }
    }

    /// Runs one request, counting it on the broker side before answering.
    pub fn drive(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, BrokerUnknown> {
        self.requests.push(Self::render(request));
        if let BrokerRequest::Submit(order) = request {
            let id = order.client_order_id.as_str().to_owned();
            let body = Self::render(request);
            let seen = self.bodies.contains(&body);
            let counter = self.accepted.entry(id).or_insert(0);
            if !seen {
                *counter = counter.saturating_add(1);
            }
            self.bodies.insert(body);
        }
        self.replies
            .pop_front()
            .unwrap_or(Err(BrokerUnknown::Timeout))
    }
}

/// The in-memory shell: one fenced writer over the account stream, the streams it follows, the
/// connector, and the timers.
pub struct Shell {
    pub state: ExecutorState,
    pub epoch: WriterEpoch,
    pub account_journal: Vec<FoldedEvent>,
    pub followed: Vec<FoldedEvent>,
    pub armed: BTreeMap<TimerId, RiskClock>,
    pub next_append: AppendOutcome,
    pub connector: FakeConnector,
    pub stopped: bool,
    /// Where the shell stopped running the effect list, for the fault-injection suite.
    pub crashed_at: Option<CrashPoint>,
}

impl Shell {
    pub fn new(epoch: u64) -> Self {
        Self {
            state: ExecutorState::new(scope()),
            epoch: WriterEpoch(epoch),
            account_journal: Vec::new(),
            followed: Vec::new(),
            armed: BTreeMap::new(),
            next_append: AppendOutcome::Committed,
            connector: FakeConnector::default(),
            stopped: false,
            crashed_at: None,
        }
    }

    pub fn head(&self) -> Seq {
        let len = u64::try_from(self.account_journal.len()).unwrap_or(u64::MAX);
        Seq(len)
    }

    /// Folds one event, exactly as the tailer would.
    ///
    /// An account-stream event goes into `account_journal` and anything else into `followed`, so
    /// [`Self::head`] is the head the executor itself sees. Getting this wrong is what made the
    /// first draft of this harness feed the core a `seq` two apart from its own.
    pub fn fold_one(&mut self, event: &FoldedEvent) -> Result<(), ExecutorError> {
        if event.stream == ACCOUNT_STREAM {
            self.account_journal.push(event.clone());
        } else {
            self.followed.push(event.clone());
        }
        fold(&mut self.state, event)
    }

    /// One step, running the effect list in order: appends first, then broker requests, then
    /// timers. A submission is issued only after the append that records it has committed, which
    /// is the shell's half of write-before-acting (ES-06, journal spec §5.2).
    pub fn step(&mut self, input: Input, ports: &Ports<'_>) -> Result<Ran, ExecutorError> {
        self.step_with_binding(input, ports, &ALLOWING_BINDING_GATE)
    }

    pub fn step_with_binding(
        &mut self,
        input: Input,
        ports: &Ports<'_>,
        binding_gate: &dyn BindingGateSource,
    ) -> Result<Ran, ExecutorError> {
        let effects = handle(&mut self.state, input, ports, binding_gate)?;
        self.play(effects, None)
    }

    /// The same, stopping before the effect the crash point names.
    pub fn step_crashing(
        &mut self,
        input: Input,
        ports: &Ports<'_>,
        at: CrashPoint,
    ) -> Result<Ran, ExecutorError> {
        let effects = handle(&mut self.state, input, ports, &ALLOWING_BINDING_GATE)?;
        self.play(effects, Some(at))
    }

    fn play(
        &mut self,
        effects: Vec<Effect>,
        crash: Option<CrashPoint>,
    ) -> Result<Ran, ExecutorError> {
        let mut ran = Ran {
            effects: effects.clone(),
            ..Ran::default()
        };
        for effect in &effects {
            if crash.is_some_and(|point| point.stops_before(effect)) {
                self.crashed_at = crash;
                return Ok(ran);
            }
            match effect {
                Effect::Journal(draft) => match self.next_append {
                    AppendOutcome::Committed => {
                        let seq = self.head().0.saturating_add(1);
                        let stored = FoldedEvent {
                            stream: ACCOUNT_STREAM.to_owned(),
                            seq: Seq(seq),
                            event_id: draft.event_id.clone(),
                            event_type: draft.event_type.clone(),
                            causation_id: draft.causation_id.clone(),
                            payload: draft.payload.clone(),
                        };
                        self.account_journal.push(stored.clone());
                        fold(&mut self.state, &stored).unwrap_or_else(|e| {
                            panic!(
                                "the writer cannot fold back its own {}: {e}",
                                stored.event_type
                            )
                        });
                        ran.drafts.push(draft.clone());
                    }
                    AppendOutcome::Unresolved | AppendOutcome::HeadMismatch => {
                        ran.drafts.push(draft.clone());
                        return Ok(ran);
                    }
                    AppendOutcome::Fenced => {
                        self.stopped = true;
                        return Ok(ran);
                    }
                },
                Effect::Broker(request) => {
                    let _answer = self.connector.drive(request);
                    ran.requests.push(request.clone());
                }
                Effect::Timer(request) => {
                    match request {
                        TimerRequest::Arm { id, at } => {
                            self.armed.insert(id.clone(), *at);
                        }
                        TimerRequest::Cancel { id } => {
                            self.armed.remove(id);
                        }
                    }
                    ran.timers.push(request.clone());
                }
                Effect::Notify(reference) => ran.notifications.push(reference.message_key),
            }
        }
        Ok(ran)
    }

    /// The same as [`Self::step`], panicking on a refusal, for the many cases whose subject is
    /// the effect list. A pending test dies here, on the crate's `Unimplemented` error.
    pub fn run(&mut self, input: Input, ports: &Ports<'_>) -> Ran {
        self.step(input, ports)
            .unwrap_or_else(|e| panic!("step refused with {}: {e}", e.code()))
    }

    pub fn run_with_binding(
        &mut self,
        input: Input,
        ports: &Ports<'_>,
        binding_gate: &dyn BindingGateSource,
    ) -> Ran {
        self.step_with_binding(input, ports, binding_gate)
            .unwrap_or_else(|e| panic!("step refused with {}: {e}", e.code()))
    }

    /// A crash and restart: a new process, a new epoch, the same journal, folded from seq 1, then
    /// `Input::Started`, which queries rather than resubmits.
    pub fn restart(&self, ports: &Ports<'_>) -> (Self, Ran) {
        self.restart_with(FakeConnector::default(), ports)
    }

    /// A restart that carries the broker across, which is what lets a fault case assert "at most
    /// one accepted submission per client order id" over the **whole run** rather than over one
    /// process.
    ///
    /// The broker is handed to the new process **before** `Input::Started` runs, so anything
    /// recovery sends is counted against what the broker already holds. (The first draft moved
    /// the counters across after `Started` and overwrote whatever recovery had sent, so a planted
    /// blind resubmission at restart vanished from the counter.)
    pub fn restart_keeping_broker(&mut self, ports: &Ports<'_>) -> (Self, Ran) {
        let broker = core::mem::take(&mut self.connector);
        self.restart_with(broker, ports)
    }

    /// A restart made ready to open, as a production shell starts: `Input::Started`, then the
    /// startup reconciliation, then the broker's account reported. The executor holds every
    /// opening until both have happened since the start (§11, the coordinator's rulings on #174),
    /// so a case whose subject is not the startup itself begins here.
    pub fn restart_ready(&self, ports: &Ports<'_>) -> Self {
        let (mut next, _) = self.restart(ports);
        next.ready(ports);
        next
    }

    /// The last `ProtectionChanged placed` payload naming `id`, from which `ready()` describes a
    /// resting protective order as the broker holds it: a GTC OCO at its take-profit with its stop
    /// as the child leg, or a stop for crypto until the payload carries its limit.
    fn placed_protection(&self, id: &str) -> Option<&Value> {
        self.account_journal
            .iter()
            .rev()
            .filter(|event| event.event_type == "ProtectionChanged")
            .map(|event| &event.payload)
            .find(|payload| {
                matches!(payload.get("action"), Some(Value::Str(action)) if action == "placed")
                    && named_orders(payload, "orders")
                        .iter()
                        .any(|named| named == id)
            })
    }

    /// The startup reconciliation and then the account report, on a process already started,
    /// against a broker that agrees with the journal: it holds the positions the fold holds and
    /// lists every live order the fold has, so the run pauses nothing and differs on nothing. The
    /// one adoption is the known exception: an order the journal left `Submitting` or `Unknown` is
    /// listed as `accepted`, which the run adopts with an `OrderStateChanged` and its
    /// `CompensatingEvent`. The run is asserted to draft only its record, the positions it
    /// observed, the account snapshot when an account was already reported, and that adoption
    /// when such an order exists, so a case's own subject is never changed behind its back.
    pub fn ready(&mut self, ports: &Ports<'_>) {
        self.ready_with(ports, broker_account());
    }

    /// [`Self::ready`] with the broker reporting `account`, both in the startup run's snapshot and
    /// in the account report after it: a reference case starts from its own `initial.account`.
    pub fn ready_with(&mut self, ports: &Ports<'_>, account: BrokerAccount) {
        let reported = self.state.observed_account().is_some();
        let in_doubt = self
            .state
            .orders()
            .values()
            .any(|order| matches!(order.state, OrderState::Submitting | OrderState::Unknown));
        let mut startup = snapshot(self.head().0, ReconcileReason::Startup);
        startup.account = account.clone();
        startup.positions = self
            .state
            .positions()
            .iter()
            .filter(|(_, held)| **held != SignedQty::ZERO)
            .map(|(held, quantity)| BrokerPosition {
                instrument: held.clone(),
                qty: *quantity,
                avg_entry_price: price("150"),
            })
            .collect();
        startup.open_orders = self
            .state
            .orders()
            .values()
            .filter_map(|order| {
                let status = match order.state {
                    OrderState::Submitting | OrderState::Accepted | OrderState::Unknown => {
                        "accepted"
                    }
                    OrderState::PartiallyFilled => "partially_filled",
                    OrderState::PendingCancel => "pending_cancel",
                    OrderState::PendingReplace => "pending_replace",
                    _ => return None,
                };
                let id = order.client_order_id.as_str();
                let placed = self.placed_protection(id);
                let text_of = |key: &str| {
                    placed.and_then(|payload| match payload.get(key) {
                        Some(Value::Str(raw)) => Some(raw.clone()),
                        _ => None,
                    })
                };
                let take_profit = text_of("take_profit");
                let stop = text_of("stop");
                Some(BrokerOrder {
                    broker_order_id: format!("b-{id}"),
                    client_order_id: Some(id.to_owned()),
                    instrument: order.instrument.clone(),
                    side: order.side,
                    qty: order.qty,
                    filled_qty: order.filled_qty,
                    limit_price: match (&placed, &take_profit) {
                        (Some(_), Some(target)) => Some(price(target)),
                        (Some(_), None) => None,
                        (None, _) => Some(price("150")),
                    },
                    stop_price: stop.as_deref().map(price),
                    status: status.to_owned(),
                    reject_code: None,
                    replaced_by_broker_order_id: None,
                    legs: match (&take_profit, &stop) {
                        (Some(_), Some(stop)) => vec![BrokerOrder {
                            broker_order_id: format!("b-{id}-sl"),
                            client_order_id: Some(format!("broker-named-{id}-sl")),
                            instrument: order.instrument.clone(),
                            side: Side::Sell,
                            qty: order.qty,
                            filled_qty: Qty::ZERO,
                            limit_price: None,
                            stop_price: Some(price(stop)),
                            status: "held".to_owned(),
                            reject_code: None,
                            replaced_by_broker_order_id: None,
                            legs: Vec::new(),
                            created_on: None,
                        }],
                        _ => Vec::new(),
                    },
                    created_on: Some(
                        text_of("created_on").map_or_else(|| date("2026-09-22"), |day| date(&day)),
                    ),
                })
            })
            .collect();
        let ran = self.run(Input::BrokerSnapshot(startup), ports);
        let unexpected: Vec<&str> = ran
            .draft_types()
            .into_iter()
            .filter(|kind| match *kind {
                "ReconciliationRun" | "BrokerPositionObserved" => false,
                "AccountSnapshotRecorded" => !reported,
                "OrderStateChanged" | "CompensatingEvent" => !in_doubt,
                _ => true,
            })
            .collect();
        assert!(
            unexpected.is_empty(),
            "the startup run of a ready shell records and adopts nothing else: {unexpected:?}"
        );
        let report = self.run(
            Input::BrokerUpdate(mandate_executor::BrokerUpdate::Account(account)),
            ports,
        );
        assert_eq!(
            report.draft_types(),
            vec!["AccountStateObserved"],
            "the account report of a ready shell records the account and restricts nothing"
        );
    }

    fn restart_with(&self, broker: FakeConnector, ports: &Ports<'_>) -> (Self, Ran) {
        let mut next = Self::new(self.epoch.0.saturating_add(1));
        next.connector = broker;
        next.followed = self.followed.clone();
        next.account_journal = self.account_journal.clone();
        for event in self.account_journal.iter().chain(self.followed.iter()) {
            fold(&mut next.state, event)
                .unwrap_or_else(|e| panic!("replay refused with {}: {e}", e.code()));
        }
        let epoch = next.epoch;
        let ran = next.run(Input::Started(epoch), ports);
        (next, ran)
    }

    /// A replay of the journal alone, for the properties that compare a fold against a live run.
    pub fn replay(&self) -> Result<ExecutorState, ExecutorError> {
        let mut state = ExecutorState::new(scope());
        for event in self.account_journal.iter().chain(self.followed.iter()) {
            fold(&mut state, event)?;
        }
        Ok(state)
    }
}
