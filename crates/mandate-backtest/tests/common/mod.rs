//! Builders shared by the backtest-loop tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use mandate_accounting::{
    AccountType, AssetClass, Config, CryptoFees, EquityFees, InstrumentId, TafCapBasis,
};
use mandate_backtest::{
    BacktestInput, BacktestRun, MetricsConfig, Observation, RunConfig, Strategy, StrategyConfig,
};
use mandate_num::{
    Bps, FeeCap, FeePerShare, FeeRate, Fraction, Price, Qty, Ratio, ShareIncrement, TickRule, Usd,
};
use mandate_sim::{
    FirstBarVolumes, Instrument, Nanos, Session, SimBar, SimConfig, Slippage, simulate,
};
use mandate_time::{Date, TradingCalendar, UtcNanos};

/// 09:30 America/New_York on 2026-09-21, the Monday the fill model's cases sit in; this suite walks
/// the week from it, so a settlement and a fee charge both fall inside a run.
pub const FIRST_TRADING_DAY: &str = "2026-09-21";
/// 04:00 ET, the start of the pre-market session (spec §4.3).
pub const PRE_MARKET_CLOCK: &str = "04:00";
/// 09:30 ET, the regular session's open.
pub const REGULAR_CLOCK: &str = "09:30";
/// 16:00 ET, the start of after hours.
pub const AFTER_HOURS_CLOCK: &str = "16:00";

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

pub fn d(text: &str) -> Date {
    Date::parse(text).unwrap()
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap()
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap()
}

pub fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap()
}

pub fn bps(text: &str) -> Bps {
    Bps::parse(text).unwrap()
}

pub fn ratio(text: &str) -> Ratio {
    Ratio::parse(text).unwrap()
}

pub fn id(text: &str) -> InstrumentId {
    InstrumentId::new(text).unwrap()
}

/// `hh:mm` New York on `day`, which is what a bar start is written as here.
pub fn et(day: &str, clock: &str) -> String {
    format!("{day}T{clock}:00-04:00")
}

/// A regular-session bar of `day`, not an auction bar.
pub fn bar(day: &str, clock: &str, ohlcv: [&str; 5]) -> SimBar {
    let [open, high, low, close, volume] = ohlcv;
    SimBar {
        start: at(&et(day, clock)),
        trade_date: d(day),
        open: price(open),
        high: price(high),
        low: price(low),
        close: price(close),
        volume: qty(volume),
        session: Session::Regular,
        session_start: at(&et(day, REGULAR_CLOCK)),
        auction: false,
    }
}

/// The first regular bar of a covered trading day, which covers the opening auction (spec §6.4 rule
/// 5, DEC-106 item 5).
pub fn auction(day: &str, clock: &str, ohlcv: [&str; 5]) -> SimBar {
    SimBar {
        auction: true,
        ..bar(day, clock, ohlcv)
    }
}

/// An after-hours bar of `day`: it never closes an equity period, because §8.2's end of day is the
/// official close.
pub fn after_hours(day: &str, clock: &str, ohlcv: [&str; 5]) -> SimBar {
    SimBar {
        session: Session::AfterHours,
        session_start: at(&et(day, AFTER_HOURS_CLOCK)),
        ..bar(day, clock, ohlcv)
    }
}

/// A pre-market bar of `day`.
pub fn pre_market(day: &str, clock: &str, ohlcv: [&str; 5]) -> SimBar {
    SimBar {
        session: Session::PreMarket,
        session_start: at(&et(day, PRE_MARKET_CLOCK)),
        ..bar(day, clock, ohlcv)
    }
}

/// An overnight bar of `day` (20:00 to 04:00 ET, spec §4.3): nothing fills there (DEC-30), but its
/// mark and any fee charge its instant reaches still land.
///
/// Its trade date is **not** its calendar day: spec §2.2 gives a fill between 20:00 and 23:59:59 ET
/// the next trading day, which is what `TradingCalendar::equity_trade_date` returns and what DEC-127
/// item 20 makes the loop check, so the builder takes the date from the calendar rather than from the
/// clock.
pub fn overnight(day: &str, clock: &str, ohlcv: [&str; 5]) -> SimBar {
    let start = at(&et(day, clock));
    SimBar {
        session: Session::Overnight,
        session_start: at(&et(day, "20:00")),
        trade_date: us_2026().equity_trade_date(start).unwrap(),
        ..bar(day, clock, ohlcv)
    }
}

/// A crypto bar, in UTC: crypto trades continuously and its day ends at 00:00 UTC (spec §2.2, §4.3).
pub fn continuous(day: &str, clock: &str, ohlcv: [&str; 5]) -> SimBar {
    let [open, high, low, close, volume] = ohlcv;
    SimBar {
        start: at(&format!("{day}T{clock}:00Z")),
        trade_date: d(day),
        open: price(open),
        high: price(high),
        low: price(low),
        close: price(close),
        volume: qty(volume),
        session: Session::Continuous,
        session_start: at(&format!("{day}T00:00:00Z")),
        auction: false,
    }
}

/// A whole-share US equity.
pub fn equity() -> Instrument {
    Instrument {
        asset_class: AssetClass::UsEquity,
        increment: ShareIncrement::Whole,
    }
}

/// A fractionable crypto instrument.
pub fn crypto() -> Instrument {
    Instrument {
        asset_class: AssetClass::Crypto,
        increment: ShareIncrement::Fractional,
    }
}

/// The `test_default` backtest configuration of the reference cases: no latency, a half-spread of
/// 1 bps and a fixed impact of 2 bps, and a cap of 10% of the reference volume.
pub fn test_default_sim() -> SimConfig {
    SimConfig {
        decision_latency: Nanos::ZERO,
        approval_latency: Nanos::ZERO,
        slippage: Slippage::Fixed {
            half_spread_bps: bps("1"),
            impact_bps: bps("2"),
        },
        volume_cap_fraction: fraction("0.1"),
    }
}

pub fn fraction(text: &str) -> Fraction {
    Fraction::parse(text).unwrap()
}

/// The reference cases' `us_2026` calendar.
pub fn us_2026() -> TradingCalendar {
    TradingCalendar::new(
        d("2026-09-01"),
        d("2026-12-31"),
        [d("2026-11-26"), d("2026-12-25")],
        [d("2026-10-12"), d("2026-11-11")],
    )
    .unwrap()
}

/// The reference cases' `test_default` fee configuration.
pub fn test_default_fees() -> Config {
    Config {
        equities: EquityFees {
            sec_rate: FeeRate::parse("0.00003").unwrap(),
            taf_per_share: FeePerShare::parse("0.0002").unwrap(),
            taf_cap: FeeCap::parse("9.79").unwrap(),
            taf_cap_basis: TafCapBasis::PerExecution,
            cat_per_share: FeePerShare::parse("0.00001").unwrap(),
        },
        crypto: CryptoFees {
            maker: bps("15"),
            taker: bps("25"),
        },
        calendar: us_2026(),
    }
}

/// The same configuration with every equity fee at zero, for a case that isolates the loop from the
/// fee arithmetic E3-1 already covers.
pub fn no_equity_fees() -> Config {
    let mut fees = test_default_fees();
    fees.equities.sec_rate = FeeRate::parse("0").unwrap();
    fees.equities.taf_per_share = FeePerShare::parse("0").unwrap();
    fees.equities.cat_per_share = FeePerShare::parse("0").unwrap();
    fees
}

/// FINRA TAF alone, capped at `cap` under `basis`, with no SEC and no CAT: a configuration where the
/// reported fee total *is* the TAF, so a case can tell a cap counted per order from one counted per
/// execution (spec §6.2, DEC-87).
pub fn taf_only_fees(cap: &str, basis: TafCapBasis) -> Config {
    let mut fees = no_equity_fees();
    fees.equities.taf_per_share = FeePerShare::parse("0.0002").unwrap();
    fees.equities.taf_cap = FeeCap::parse(cap).unwrap();
    fees.equities.taf_cap_basis = basis;
    fees
}

/// A crossover with windows of two and three periods, a 25 bps collar, and a target notional of
/// 50,000 USD.
pub fn crossover() -> Strategy {
    Strategy::MovingAverageCrossover(StrategyConfig {
        fast_periods: 2,
        slow_periods: 3,
        collar: bps("25"),
        target_notional: usd("50000"),
    })
}

/// The benchmark: one GTC limit buy at the first bar's close plus a 25 bps collar.
pub fn buy_and_hold() -> Strategy {
    Strategy::BuyAndHold { collar: bps("25") }
}

/// Daily periods of a US equity year, and no risk-free rate (DEC-127 item 3).
pub fn daily_equity_metrics() -> MetricsConfig {
    MetricsConfig {
        periods_per_year: 252,
        risk_free_per_period: Ratio::ZERO,
    }
}

/// Crypto days, of which there are 365 in a year.
pub fn daily_crypto_metrics() -> MetricsConfig {
    MetricsConfig {
        periods_per_year: 365,
        risk_free_per_period: Ratio::ZERO,
    }
}

/// A margin account holding `cash`, trading `instrument` under the `test_default` configurations
/// (DEC-127 item 19).
pub fn run_config(instrument: Instrument, strategy: Strategy, cash: &str) -> RunConfig {
    RunConfig {
        sim: test_default_sim(),
        fees: test_default_fees(),
        account_type: AccountType::Margin,
        instrument_id: if instrument.asset_class == AssetClass::Crypto {
            id("BTC/USD")
        } else {
            id("SPY")
        },
        instrument,
        tick: if instrument.asset_class == AssetClass::Crypto {
            TickRule::Increment(price("0.01"))
        } else {
            TickRule::RegNmsEquity
        },
        starting_cash: usd(cash),
        strategy,
        metrics: if instrument.asset_class == AssetClass::Crypto {
            daily_crypto_metrics()
        } else {
            daily_equity_metrics()
        },
    }
}

/// No 20-session median at any minute, so a session's first bar caps at 0 (spec §6.4 rule 3).
pub struct NoMedian;

impl FirstBarVolumes for NoMedian {
    fn median_at(&self, _bar_start: UtcNanos) -> Option<Qty> {
        None
    }
}

/// One median for every session-first bar, so bar 0 of a covered session can fill.
pub struct Median(pub Qty);

impl FirstBarVolumes for Median {
    fn median_at(&self, _bar_start: UtcNanos) -> Option<Qty> {
        Some(self.0)
    }
}

/// Runs `bars` with `config`, taking the coverage start from the first bar and a median of 10,000 at
/// every session-first bar, which is how the reference-case harness supplies one (DEC-108 item 6).
pub fn run(
    config: &RunConfig,
    bars: &[SimBar],
) -> Result<BacktestRun, mandate_backtest::BacktestError> {
    let medians = Median(qty("10000"));
    let coverage_start = bars
        .first()
        .map(|b| b.start)
        .unwrap_or(at("2026-09-21T00:00:00Z"));
    mandate_backtest::run(&BacktestInput {
        config,
        bars,
        coverage_start,
        first_bar_volumes: &medians,
    })
}

/// The same with a stated median, which decides the cap of every bar that opens a session.
pub fn run_with_median(
    config: &RunConfig,
    bars: &[SimBar],
    median: &str,
) -> Result<BacktestRun, mandate_backtest::BacktestError> {
    let medians = Median(qty(median));
    let coverage_start = bars
        .first()
        .map(|b| b.start)
        .unwrap_or(at("2026-09-21T00:00:00Z"));
    mandate_backtest::run(&BacktestInput {
        config,
        bars,
        coverage_start,
        first_bar_volumes: &medians,
    })
}

/// The same, with no median: only bars that follow another bar of their session can fill.
pub fn run_without_median(
    config: &RunConfig,
    bars: &[SimBar],
) -> Result<BacktestRun, mandate_backtest::BacktestError> {
    let coverage_start = bars
        .first()
        .map(|b| b.start)
        .unwrap_or(at("2026-09-21T00:00:00Z"));
    mandate_backtest::run(&BacktestInput {
        config,
        bars,
        coverage_start,
        first_bar_volumes: &NoMedian,
    })
}

/// The fill model run directly over the same bars and one order, which is what the loop must agree
/// with bar for bar (DEC-127 item 18).
pub fn simulate_directly(
    config: &RunConfig,
    bars: &[SimBar],
    orders: &[mandate_sim::SimOrder],
) -> Result<mandate_sim::SimOutcome, mandate_sim::SimError> {
    let medians = Median(qty("10000"));
    let coverage_start = bars
        .first()
        .map(|b| b.start)
        .unwrap_or(at("2026-09-21T00:00:00Z"));
    simulate(
        &config.sim,
        &config.instrument,
        bars,
        coverage_start,
        &medians,
        orders,
    )
}

/// The equity of each period, as the run observed it.
pub fn equities(observations: &[Observation]) -> Vec<Usd> {
    observations.iter().map(|o| o.equity).collect()
}
