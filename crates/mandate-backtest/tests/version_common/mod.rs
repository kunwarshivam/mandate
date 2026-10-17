//! Inputs the version-backtest tests build on: two validated mandates from the reference bases
//! (`research_equity_pinned` and `research_equity`), Alpaca's equity capability profile, the
//! reference cases' gate and fee settings, daily bars, and recorded outputs and theses.
//!
//! Every constructor panics on bad text: a test that cannot build its own input has a bug in the
//! test (DEC-110).

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{
    AccountType, AssetClass, Config, CryptoFees, EquityFees, FeeRounding, InstrumentId, TafCapBasis,
};
use mandate_backtest::{
    EtpClassification, InstrumentReference, MetricsConfig, RecordedOutput, RecordedThesis,
    VersionInput, VersionRun, VersionSettings,
};
use mandate_builder::{Direction, ModelOutput, ModelVersion};
use mandate_canon::Digest;
use mandate_domain::{
    AssetId, CapabilityProfile, Cell, Environment, Idempotency, MarketSession, OrderType,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce,
};
use mandate_num::{
    Bps, Conviction, FeeCap, FeePerShare, FeeRate, Fraction, Price, Qty, Ratio, ShareIncrement,
    TickRule, Unit, Usd,
};
use mandate_risk::{DayTradeRegime, EtpClass, Exchange, GateConfig};
use mandate_sim::{FirstBarVolumes, Instrument, Nanos, Session, SimBar, SimConfig, Slippage};
use mandate_spec::document::{ModelId, ProvenanceMap};
use mandate_spec::{Mandate, ValidatedMandate, ValidationContext};
use mandate_time::{Date, TradingCalendar, UtcNanos};
use serde_json::Value as Json;

/// The pinned base's one instrument (`XYZ`).
pub const XYZ: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
pub const RESEARCH_HASH: &str = "4444444444444444444444444444444444444444444444444444444444444444";
pub const MOMENTUM_HASH: &str = "2222222222222222222222222222222222222222222222222222222222222222";

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap_or_else(|e| panic!("`{text}` is an instant: {e}"))
}

pub fn day(text: &str) -> Date {
    Date::parse(text).unwrap_or_else(|e| panic!("`{text}` is a date: {e}"))
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("`{text}` is a price: {e}"))
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("`{text}` is a quantity: {e}"))
}

pub fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("`{text}` is a USD amount: {e}"))
}

pub fn bps(text: &str) -> Bps {
    Bps::parse(text).unwrap_or_else(|e| panic!("`{text}` is bps: {e}"))
}

pub fn fraction(text: &str) -> Fraction {
    Fraction::parse(text).unwrap_or_else(|e| panic!("`{text}` is a fraction: {e}"))
}

pub fn asset(text: &str) -> AssetId {
    AssetId::parse(text).unwrap_or_else(|e| panic!("`{text}` is an asset id: {e}"))
}

fn fixture() -> Json {
    serde_json::from_str(include_str!("../../../../fixtures/refcases/mandate.json"))
        .unwrap_or_else(|e| panic!("the mandate reference fixture parses: {e}"))
}

/// A reference base's mandate document, as JSON, for a test to change before validating it.
pub fn base(name: &str) -> Json {
    fixture()
        .pointer(&format!("/bases/{name}/mandate"))
        .unwrap_or_else(|| panic!("the fixture has the `{name}` base"))
        .clone()
}

pub fn validate(document: &Json) -> ValidatedMandate {
    let encoded =
        serde_json::to_vec(document).unwrap_or_else(|e| panic!("the document serializes: {e}"));
    let canonical = mandate_canon::parse(&encoded)
        .unwrap_or_else(|e| panic!("the document is canonical JSON: {e}"));
    let mandate =
        Mandate::parse(&canonical).unwrap_or_else(|e| panic!("the document is a mandate: {e}"));
    let context = ValidationContext {
        account_equity_usd: usd("25000"),
        other_allocations_usd: Usd::ZERO,
        validation_date: day("2026-08-31"),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
    };
    ValidatedMandate::new(mandate, &context, &[])
        .unwrap_or_else(|e| panic!("the document validates: {e}"))
}

/// `research_equity_pinned`: one pinned US equity, a protective OCO (stop 5%, take-profit 10%),
/// and AUTO for an opening or an increase of at most 900 USD with a combined score of at least
/// 0.65; anything else asks.
pub fn pinned() -> ValidatedMandate {
    validate(&base("research_equity_pinned"))
}

/// The pinned base with every rule removed, so every opening asks (`default: ask`).
pub fn ask_only() -> ValidatedMandate {
    let mut document = base("research_equity_pinned");
    *document
        .pointer_mut("/autonomy/rules")
        .unwrap_or_else(|| panic!("the base has rules")) = Json::Array(Vec::new());
    validate(&document)
}

/// `research_equity`: the research agent's unpinned universe of up to five US equities.
pub fn research() -> ValidatedMandate {
    validate(&base("research_equity"))
}

/// The reference cases' `test_default` gate settings, with the participation caps and the
/// minimum resting time of `mandate-risk`'s own tests.
pub fn gate_config() -> GateConfig {
    GateConfig {
        price_floor: usd("5"),
        liquidity_floor_usd: usd("1000000"),
        crypto_liquidity_floor_usd: usd("2000000"),
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
    }
}

/// Alpaca's US-equity regular-session cells (`mandate-alpaca`'s profile, its equity row): a limit
/// order may carry an OCO, so a protection with a take-profit is placed as one.
pub fn alpaca_equity_profile() -> CapabilityProfile {
    let whole = |order_type, protection: &[ProtectionForm]| Cell {
        order_type,
        quantity_form: QuantityForm::Whole,
        times_in_force: [TimeInForce::Day, TimeInForce::Gtc].into(),
        protection_forms: protection.iter().copied().collect(),
    };
    let rows = vec![Row {
        asset_class: AssetClass::UsEquity,
        session: MarketSession::Regular,
        cells: vec![
            whole(OrderType::Market, &[ProtectionForm::Bracket]),
            whole(
                OrderType::Limit,
                &[ProtectionForm::Bracket, ProtectionForm::Oco],
            ),
            whole(OrderType::Stop, &[]),
            whole(OrderType::StopLimit, &[]),
        ],
    }];
    CapabilityProfile::new(
        1,
        rows,
        Idempotency {
            client_order_id: true,
            retry: Retry::Idempotent,
            query_by_client_order_id: true,
        },
    )
    .unwrap_or_else(|e| panic!("the profile is valid: {}", e.code()))
}

/// The reference cases' `us_2026` calendar.
pub fn us_2026() -> TradingCalendar {
    TradingCalendar::new(
        day("2026-09-01"),
        day("2026-12-31"),
        [day("2026-11-26"), day("2026-12-25")],
        [day("2026-10-12"), day("2026-11-11")],
    )
    .unwrap_or_else(|e| panic!("the calendar is valid: {}", e.code()))
}

pub fn fees() -> Config {
    Config {
        equities: EquityFees {
            sec_rate: FeeRate::parse("0.00003").unwrap_or_else(|e| panic!("{e}")),
            taf_per_share: FeePerShare::parse("0.0002").unwrap_or_else(|e| panic!("{e}")),
            taf_cap: FeeCap::parse("9.79").unwrap_or_else(|e| panic!("{e}")),
            fee_rounding: FeeRounding::DailyTotal,
            taf_cap_basis: TafCapBasis::PerExecution,
            cat_per_share: FeePerShare::parse("0.00001").unwrap_or_else(|e| panic!("{e}")),
        },
        crypto: CryptoFees {
            maker: bps("15"),
            taker: bps("25"),
        },
        calendar: us_2026(),
    }
}

pub fn settings() -> VersionSettings {
    VersionSettings {
        sim: SimConfig {
            decision_latency: Nanos::ZERO,
            approval_latency: Nanos::ZERO,
            slippage: Slippage::Fixed {
                half_spread_bps: bps("1"),
                impact_bps: bps("2"),
            },
            volume_cap_fraction: fraction("0.1"),
        },
        fees: fees(),
        gate: gate_config(),
        profile: alpaca_equity_profile(),
        account_type: AccountType::Margin,
        regime: DayTradeRegime::IntradayMargin {
            maintenance_excess: usd("100000"),
        },
        metrics: MetricsConfig {
            periods_per_year: 252,
            risk_free_per_period: Ratio::parse("0").unwrap_or_else(|e| panic!("{e}")),
        },
        min_order_usd: usd("1"),
        mark_max_age_s: 3600,
        benchmark_collar: bps("25"),
    }
}

/// A plain classification at 06:00 New York on each of the first `count` trading days.
pub fn daily_classifications(count: usize) -> Vec<EtpClassification> {
    trading_days(count)
        .into_iter()
        .map(|date| EtpClassification {
            at: at(&format!("{date}T10:00:00Z")),
            class: EtpClass::Plain,
        })
        .collect()
}

pub fn xyz() -> InstrumentReference {
    InstrumentReference {
        asset: asset(XYZ),
        ledger_id: InstrumentId::new("XYZ").unwrap_or_else(|e| panic!("{}", e.code())),
        instrument: Instrument {
            asset_class: AssetClass::UsEquity,
            increment: ShareIncrement::Whole,
        },
        tick: TickRule::RegNmsEquity,
        qty_increment: qty("1"),
        min_order_size: qty("1"),
        exchange: Exchange::Nasdaq,
        etp_classifications: daily_classifications(60),
        fractionable: true,
        ipo: false,
        ptp_no_exception: false,
    }
}

/// The weekdays from 2026-09-01 that NYSE trades, skipping Labor Day (2026-09-07), `count` of
/// them; every date is in daylight saving time when `count` is at most 40.
pub fn trading_days(count: usize) -> Vec<Date> {
    let mut days = Vec::new();
    let mut date = day("2026-09-01");
    while days.len() < count {
        if !date.is_weekend() && date != day("2026-09-07") {
            days.push(date);
        }
        date = date.next().unwrap_or_else(|e| panic!("{}", e.code()));
    }
    days
}

/// One daily regular-session bar of `date` starting at 09:30 New York (EDT), covering the
/// opening auction.
pub fn daily_bar(
    date: Date,
    open: &str,
    high: &str,
    low: &str,
    close: &str,
    volume: &str,
) -> SimBar {
    let start = at(&format!("{date}T13:30:00Z"));
    SimBar {
        start,
        open: price(open),
        high: price(high),
        low: price(low),
        close: price(close),
        volume: qty(volume),
        trade_date: date,
        session: Session::Regular,
        session_start: start,
        auction: true,
    }
}

/// `count` flat daily bars at 100 with a range of 99 to 101 and a million shares a day: liquid
/// enough for every gate floor.
pub fn flat_bars(count: usize) -> Vec<SimBar> {
    trading_days(count)
        .into_iter()
        .map(|date| daily_bar(date, "100", "101", "99", "100", "1000000"))
        .collect()
}

pub struct Median(pub Qty);

impl FirstBarVolumes for Median {
    fn median_at(&self, _bar_start: UtcNanos) -> Option<Qty> {
        Some(self.0)
    }
}

pub fn model_output(
    model: &str,
    version: &str,
    hash: &str,
    as_of: UtcNanos,
    conviction: &str,
) -> ModelOutput {
    ModelOutput {
        model_id: ModelId::parse(model).unwrap_or_else(|e| panic!("{e}")),
        model_version: ModelVersion::parse(version).unwrap_or_else(|e| panic!("{e:?}")),
        content_hash: Digest::from_hex(hash).unwrap_or_else(|| panic!("a hash")),
        instrument: asset(XYZ),
        as_of,
        expires_at: UtcNanos::from_parts(as_of.secs().saturating_add(86_400), 0)
            .unwrap_or_else(|e| panic!("{}", e.code())),
        direction: Direction::Long,
        conviction: Conviction::parse(conviction).unwrap_or_else(|e| panic!("{e}")),
        confidence: Unit::parse("0.9").unwrap_or_else(|e| panic!("{e}")),
    }
}

/// Both pinned models' outputs on each of `days`, at 09:20 New York with `conviction`, received
/// one minute later: fresh at the 09:30 decision for both models' ages.
pub fn daily_outputs(days: &[Date], conviction: &str) -> Vec<RecordedOutput> {
    days.iter()
        .flat_map(|date| {
            let as_of = at(&format!("{date}T13:20:00Z"));
            let received_at = at(&format!("{date}T13:21:00Z"));
            [
                ("llm.research_agent", "0.1.0", RESEARCH_HASH),
                ("quant.momentum", "1.0.0", MOMENTUM_HASH),
            ]
            .into_iter()
            .map(move |(model, version, hash)| RecordedOutput {
                received_at,
                output: model_output(model, version, hash, as_of, conviction),
            })
        })
        .collect()
}

pub fn thesis(received_at: UtcNanos, expires_at: UtcNanos, admitted: bool) -> RecordedThesis {
    RecordedThesis {
        thesis_id: "th_01".to_owned(),
        received_at,
        instrument: asset(XYZ),
        as_of: received_at,
        expires_at,
        admitted,
        confidence: Unit::parse("0.8").unwrap_or_else(|e| panic!("{e}")),
        invalidated_at: None,
    }
}

/// Runs `mandate` over `bars` with the default settings, the XYZ reference, and a first-bar median
/// of 10 million shares.
pub fn run(
    mandate: &ValidatedMandate,
    bars: &[SimBar],
    outputs: &[RecordedOutput],
    theses: &[RecordedThesis],
) -> Result<VersionRun, mandate_backtest::BacktestError> {
    run_with(mandate, &settings(), &xyz(), bars, outputs, theses, &[])
}

pub fn run_with(
    mandate: &ValidatedMandate,
    settings: &VersionSettings,
    instrument: &InstrumentReference,
    bars: &[SimBar],
    outputs: &[RecordedOutput],
    theses: &[RecordedThesis],
    corporate_actions: &[Date],
) -> Result<VersionRun, mandate_backtest::BacktestError> {
    let medians = Median(qty("10000000"));
    let coverage_start = bars
        .first()
        .map_or(at("2026-09-01T00:00:00Z"), |bar| bar.start);
    mandate_backtest::run_version(&VersionInput {
        mandate,
        settings,
        instrument,
        bars,
        coverage_start,
        first_bar_volumes: &medians,
        outputs,
        theses,
        corporate_actions,
    })
}

/// The pinned base with a ladder a 900 USD position can reach: scale sizes at 0.2% of equity,
/// exits only at 0.4%, flatten and pause at 0.6%, confirmed after 60 seconds, with the daily loss
/// and the protective stop (20% away) out of the way.
pub fn tight_ladder() -> ValidatedMandate {
    let mut document = base("research_equity_pinned");
    let rung = |at: &str, action: &str, factor: Json| serde_json::json!({"at": at, "action": action, "factor": factor});
    let set = |document: &mut Json, pointer: &str, value: Json| {
        *document
            .pointer_mut(pointer)
            .unwrap_or_else(|| panic!("the base has {pointer}")) = value;
    };
    set(
        &mut document,
        "/risk/drawdown_ladder",
        Json::Array(vec![
            rung("0.002", "scale_sizes", Json::String("0.5".to_owned())),
            rung("0.004", "exits_only", Json::Null),
            rung("0.006", "flatten_and_pause", Json::Null),
        ]),
    );
    set(
        &mut document,
        "/risk/max_drawdown",
        Json::String("0.006".to_owned()),
    );
    set(
        &mut document,
        "/risk/hysteresis",
        Json::String("0.001".to_owned()),
    );
    set(
        &mut document,
        "/risk/max_daily_loss",
        Json::String("0.5".to_owned()),
    );
    set(
        &mut document,
        "/protection/stop_distance",
        Json::String("0.2".to_owned()),
    );
    validate(&document)
}
