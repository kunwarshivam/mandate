//! Builders shared by the fill-model tests.

#![allow(dead_code, reason = "each test binary uses a different subset")]

use mandate_accounting::{AssetClass, Liquidity, Side};
use mandate_num::{Bps, Fraction, Price, Qty, ShareIncrement};
use mandate_sim::{
    Eligibility, FirstBarVolumes, Instrument, Nanos, OcoLeg, OrderKind, OrderRef, Session, SimBar,
    SimConfig, SimError, SimOrder, SimOutcome, Slippage, TimeInForce, simulate,
};
use mandate_time::UtcNanos;

/// 09:30 America/New_York on 2026-09-21, the Monday every reference-case bar sits in.
pub const REGULAR_OPEN: &str = "2026-09-21T09:30:00-04:00";
/// 04:00 ET the same day (spec §4.3).
pub const PRE_MARKET_OPEN: &str = "2026-09-21T04:00:00-04:00";
/// 16:00 ET the same day (spec §4.3).
pub const AFTER_HOURS_OPEN: &str = "2026-09-21T16:00:00-04:00";
/// 00:00 UTC on 2026-09-21: the start of a crypto reporting day (spec §2.2).
pub const CRYPTO_DAY_START: &str = "2026-09-21T00:00:00Z";

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// `hh:mm` on 2026-09-21 in New York, the trading day every hand-calculated case sits in.
pub fn et(clock: &str) -> String {
    format!("2026-09-21T{clock}:00-04:00")
}

pub fn price(text: &str) -> Price {
    Price::parse(text).unwrap()
}

pub fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap()
}

pub fn bps(text: &str) -> Bps {
    Bps::parse(text).unwrap()
}

pub fn fraction(text: &str) -> Fraction {
    Fraction::parse(text).unwrap()
}

/// A regular-session bar of the reference cases' trading day, not an auction bar.
pub fn bar(clock: &str, ohlcv: [&str; 5]) -> SimBar {
    bar_at(&et(clock), ohlcv)
}

/// A regular-session bar of the reference cases' trading day, from a full instant.
pub fn bar_at(start: &str, ohlcv: [&str; 5]) -> SimBar {
    let [open, high, low, close, volume] = ohlcv;
    SimBar {
        start: at(start),
        trade_date: at(start).date(),
        open: price(open),
        high: price(high),
        low: price(low),
        close: price(close),
        volume: qty(volume),
        session: Session::Regular,
        session_start: at(REGULAR_OPEN),
        auction: false,
    }
}

/// The same bar relabelled into `session`, which began at `session_start`, on the trading day that
/// session start falls in.
pub fn in_session(session: Session, session_start: &str, of: SimBar) -> SimBar {
    SimBar {
        session,
        session_start: at(session_start),
        trade_date: at(session_start).date(),
        ..of
    }
}

/// A regular-session bar marked as covering an auction (spec §6.4 rule 5).
pub fn auction(start: &str, ohlcv: [&str; 5]) -> SimBar {
    SimBar {
        auction: true,
        ..bar(start, ohlcv)
    }
}

/// A pre-market bar of the same trading day.
pub fn pre_market(start: &str, ohlcv: [&str; 5]) -> SimBar {
    in_session(Session::PreMarket, PRE_MARKET_OPEN, bar(start, ohlcv))
}

/// A crypto bar: crypto trades continuously (spec §4.3), and its reporting day ends at 00:00 UTC
/// (spec §2.2), so every bar of one day shares that session start.
pub fn continuous(clock: &str, ohlcv: [&str; 5]) -> SimBar {
    in_session(
        Session::Continuous,
        CRYPTO_DAY_START,
        bar_at(&format!("2026-09-21T{clock}:00Z"), ohlcv),
    )
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

/// The reference cases' `test_default` backtest configuration: no latency, a half-spread of 1 bps
/// and a fixed impact of 2 bps (so s = 3 bps), and a volume cap of 10% of the reference volume.
pub fn test_default() -> SimConfig {
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

/// No 20-session median for any minute: a session's first bar caps at 0 (spec §6.4 rule 3).
pub struct NoMedian;

impl FirstBarVolumes for NoMedian {
    fn median_at(&self, _bar_start: UtcNanos) -> Option<Qty> {
        None
    }
}

/// One median volume, for every session-first bar, as a reference case's
/// `first_bar_reference_volume` supplies it.
pub struct Median(pub Qty);

impl FirstBarVolumes for Median {
    fn median_at(&self, _bar_start: UtcNanos) -> Option<Qty> {
        Some(self.0)
    }
}

pub fn market() -> OrderKind {
    OrderKind::Market
}

pub fn limit(at_price: &str) -> OrderKind {
    OrderKind::Limit {
        limit: price(at_price),
    }
}

pub fn stop(at_price: &str) -> OrderKind {
    OrderKind::Stop {
        stop: price(at_price),
    }
}

pub fn stop_limit(stop_price: &str, limit_price: &str) -> OrderKind {
    OrderKind::StopLimit {
        stop: price(stop_price),
        limit: price(limit_price),
    }
}

pub fn oco(limit_price: &str, stop_price: &str) -> OrderKind {
    OrderKind::Oco {
        limit: price(limit_price),
        stop: price(stop_price),
    }
}

/// Decided at `hh:mm` New York, with no approval step.
pub fn decided(clock: &str) -> Eligibility {
    Eligibility::DecidedAt {
        at: at(&et(clock)),
        approval_required: false,
    }
}

/// Decided at `when`, and approved, so the approval latency applies too (spec §6.4 rule 1).
pub fn approved(clock: &str) -> Eligibility {
    Eligibility::DecidedAt {
        at: at(&et(clock)),
        approval_required: true,
    }
}

/// Already resting at the start of bar 0 (`resting_since: previous_trading_day`).
pub fn resting() -> Eligibility {
    Eligibility::Resting { from_bar: 0 }
}

/// Already resting at the start of bar `from_bar` (`resting_since_bar`).
pub fn resting_from(from_bar: usize) -> Eligibility {
    Eligibility::Resting { from_bar }
}

pub fn sell(kind: OrderKind, quantity: &str, eligible_from: Eligibility) -> SimOrder {
    SimOrder {
        side: Side::Sell,
        qty: qty(quantity),
        kind,
        tif: TimeInForce::Gtc,
        extended_hours: false,
        eligible_from,
    }
}

pub fn buy(kind: OrderKind, quantity: &str, eligible_from: Eligibility) -> SimOrder {
    SimOrder {
        side: Side::Buy,
        ..sell(kind, quantity, eligible_from)
    }
}

/// The same order as a day order (spec §6.4 rule 2).
pub fn for_the_day(order: SimOrder) -> SimOrder {
    SimOrder {
        tif: TimeInForce::Day,
        ..order
    }
}

/// The same order allowed into the extended sessions (spec §4.3, DEC-37).
pub fn in_extended_hours(order: SimOrder) -> SimOrder {
    SimOrder {
        extended_hours: true,
        ..order
    }
}

/// Simulates `orders` against `bars`, with coverage starting at the first bar, as a reference case
/// does.
pub fn run(
    config: &SimConfig,
    instrument: &Instrument,
    bars: &[SimBar],
    volumes: &dyn FirstBarVolumes,
    orders: &[SimOrder],
) -> Result<SimOutcome, SimError> {
    let coverage_start = bars.first().map_or(at(&et("09:30")), |b| b.start);
    simulate(config, instrument, bars, coverage_start, volumes, orders)
}

/// `test_default` on a whole-share equity with no median volume: the shortest form a hand test
/// needs.
pub fn fills(bars: &[SimBar], orders: &[SimOrder]) -> Vec<(usize, String, String, &'static str)> {
    reported(&run(&test_default(), &equity(), bars, &NoMedian, orders).unwrap())
}

/// The same, with one 20-session median for every session-first bar.
pub fn fills_with_median(
    median: &str,
    bars: &[SimBar],
    orders: &[SimOrder],
) -> Vec<(usize, String, String, &'static str)> {
    reported(
        &run(
            &test_default(),
            &equity(),
            bars,
            &Median(qty(median)),
            orders,
        )
        .unwrap(),
    )
}

/// Bar, quantity, price, and liquidity of each fill, in the order the model reports them.
pub fn reported(outcome: &SimOutcome) -> Vec<(usize, String, String, &'static str)> {
    outcome
        .fills
        .iter()
        .map(|f| {
            (
                f.bar,
                f.qty.to_string(),
                f.price.to_string(),
                liquidity(f.liquidity),
            )
        })
        .collect()
}

/// Bar, leg, quantity, price, and liquidity of each fill, for an OCO pair.
pub fn reported_legs(
    outcome: &SimOutcome,
) -> Vec<(usize, &'static str, String, String, &'static str)> {
    outcome
        .fills
        .iter()
        .map(|f| {
            (
                f.bar,
                f.leg.map_or("none", leg),
                f.qty.to_string(),
                f.price.to_string(),
                liquidity(f.liquidity),
            )
        })
        .collect()
}

/// The legs an OCO lost, with the bar of the fill that canceled them.
pub fn canceled(outcome: &SimOutcome) -> Vec<(usize, &'static str)> {
    outcome
        .canceled_legs
        .iter()
        .map(|c| (c.bar, leg(c.leg)))
        .collect()
}

pub fn leg(of: OcoLeg) -> &'static str {
    match of {
        OcoLeg::Limit => "limit",
        OcoLeg::Stop => "stop",
    }
}

/// `auction` for a fill that is neither maker nor taker (DEC-106 item 5).
pub fn liquidity(of: Option<Liquidity>) -> &'static str {
    match of {
        Some(Liquidity::Maker) => "maker",
        Some(Liquidity::Taker) => "taker",
        None => "auction",
    }
}

pub fn first_order() -> OrderRef {
    OrderRef::new(0)
}
