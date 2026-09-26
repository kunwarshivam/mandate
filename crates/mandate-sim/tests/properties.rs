//! The fill model's invariants as properties (trading-domain spec §6.4 rules 1 to 9, DEC-106), over
//! random bar sequences, orders, and configurations.
//!
//! The oracle in this file is a second fill model, written differently on purpose. It holds prices,
//! quantities, and volumes as `i128` integers in units of 10⁻⁹ and basis points in units of 10⁻⁸,
//! does its slippage and cap arithmetic with integer ceiling division, and walks **order-major**:
//! each order in submission order consumes what it can from every bar's remaining cap before the
//! next order is considered. Rule 3 gives every order strict priority over the ones submitted after
//! it on every bar, so that order-major walk must produce the same allocation as the bar-major walk
//! the crate makes, and any disagreement is a defect in one of them.
//!
//! Every property runs the model through `simulated`, which first checks the number of fills against
//! the oracle's, so no property can pass on an empty fill list.
//!
//! Generated scenarios use the `fixed` impact model. The `sqrt` model needs an 18-place square root,
//! whose independent oracle would need arbitrary-precision integers; it is covered instead by
//! hand-computed cases in `hand.rs` and in `mandate-num`'s own tests.
//!
//! **Planted bugs.** Each of the thirty below was broken on purpose in a throwaway implementation of
//! this story's stubs, kept out of this change (DEC-83), one at a time, and every one was caught. The tests that failed
//! are named in brackets; `oracle` is `fills_match_the_independent_simulator`, `harness rc_10` and
//! `harness rc_12` are the two tests in `mandate-refcases/tests/harness.rs`, and `RC-nn` is that
//! reference case run with `--include-ignored`.
//!
//! 1. A resting limit fills on a touch [RC-10, RC-19, `a_touch_is_never_a_fill`,
//!    `rc_10_a_touch_is_not_a_fill_and_the_next_bar_through_the_limit_is`,
//!    `rc_19_a_touch_on_the_arrival_bar_is_not_a_fill`, the shared-cap hand test, harness rc_10].
//! 2. The bar that produced the decision fills [`no_fill_before_eligibility`, oracle,
//!    `day_orders_expire_at_session_end`, the latency hand test, harness rc_10].
//! 3. A bar's volume cap is not shared across orders [`fills_never_exceed_the_shared_volume_cap`,
//!    oracle, `a_bars_volume_cap_is_shared_across_orders_in_submission_order`].
//! 4. Sell slippage is added instead of taken away [RC-10, RC-12, `slippage_and_rounding_are_adverse`,
//!    oracle, seven hand tests, harness rc_12].
//! 5. An equity stop triggers outside the regular session [`fills_respect_sessions`, oracle].
//! 6. An OCO fill leaves the other leg working [RC-12, `oco_fills_at_most_one_leg`, oracle, both OCO
//!    hand tests, harness rc_12].
//! 7. The auction exception applies on every bar [RC-12, oracle, three hand tests, harness rc_12].
//! 8. The volume cap rounds instead of truncating [`fills_never_exceed_the_shared_volume_cap`,
//!    oracle, `a_volume_cap_is_the_truncated_product_and_never_above_it`,
//!    `hand_calculated_slippage_and_volume_caps`].
//! 9. The reference volume is the previous bar whatever its session
//!    [`fills_never_exceed_the_shared_volume_cap`, oracle].
//! 10. A marketable remainder stays marketable past its limit
//!     [`a_resting_remainder_does_not_become_marketable_again`, oracle].
//! 11. A session's first-bar median is used even when the session began before the data
//!     [`fills_never_exceed_the_shared_volume_cap`, oracle].
//! 12. A day order's remainder survives its last eligible session [`day_orders_expire_at_session_end`,
//!     oracle, `a_day_orders_remainder_is_canceled_after_its_last_eligible_session`].
//! 13. A fill price rounds half-even instead of against the order
//!     [`slippage_moves_a_price_against_the_order_by_a_rounded_up_amount`, oracle].
//! 14. The `sqrt` root is truncated instead of rounded up [`hand_calculated_sqrt_impacts`].
//! 15. to 20. The harness never checks a bar's stated session, ignores
//!     `first_bar_reference_volume`, never compares a fill's price, never compares canceled legs,
//!     accepts a sell above the position held, or ignores `decided_at` [harness rc_10 or rc_12, and
//!     RC-10, RC-12, and RC-19 for the two that change a fill].
//!
//! A second sweep, after the first independent review revised DEC-106 items 7 to 11, planted ten
//! more; every one was caught too.
//!
//! 21. A stop-limit fills in its trigger bar, item 7's earlier reading
//!     [`a_stop_limit_that_gaps_beyond_its_limit_does_not_fill_in_its_trigger_bar`, oracle, and six
//!     more properties].
//! 22. An OCO's take-profit is marketable on arrival, against item 8
//!     [`an_ocos_take_profit_is_never_marketable_on_arrival`, oracle].
//! 23. A triggered stop's remainder stops filling, against item 10
//!     [`a_triggered_stops_remainder_is_a_market_order`,
//!     `a_partial_oco_fill_cancels_the_other_leg_and_leaves_a_market_remainder`, oracle, nine more
//!     properties].
//! 24. `extended_hours` is accepted on any order kind, against §5.2
//!     [`the_model_rejects_bars_and_orders_it_cannot_simulate`].
//! 25. A quantity off the instrument's increment is accepted [the same test].
//! 26. The extended sessions are open to every order, not only those marked for them
//!     [`a_market_order_waits_for_the_regular_session`,
//!     `an_exit_marked_for_extended_hours_fills_after_hours`,
//!     `a_day_orders_remainder_is_canceled_after_its_last_eligible_session`, oracle, eight more
//!     properties]. The first of those tests could not catch it until the independent re-review
//!     pointed out that its pre-market bar began before the data, which capped that bar at 0 under
//!     item 4 and let both readings fill on the regular bar; the bar now opens its own session.
//! 27. The overnight session trades, against DEC-30 [`an_overnight_bar_never_fills`, oracle, eight
//!     more properties].
//! 28. The auction exception never applies [RC-12,
//!     `rc_12_a_resting_limit_gapped_through_at_an_auction_fills_at_the_open`, oracle, harness
//!     rc_12]. The oracle catching it is also what shows generated scenarios reach auction bars.
//! 29. An OCO on a fractional instrument is accepted, against §5.4 and DEC-36 [the rejection test].
//! 30. A day order on a continuous instrument is accepted [the rejection test].
//!
//! Bug 10 survived the first sweep: RC-19's `marketable_remainder_becomes_resting` cannot tell it
//! apart, because no later bar there opens back inside the limit. It is what
//! `a_resting_remainder_does_not_become_marketable_again` was written for, and the generator now
//! clusters bar opens and order prices around one level and prefers small cap fractions, so partial
//! fills whose marketability flips between bars are common and the oracle catches it too.
//!
//! Writing that implementation also found two defects in the tests' own first reading of §6.4, both
//! caught by the reference cases: the marketable-on-arrival test compares the open with the limit in
//! the opposite direction from a stop's trigger, and an order that is already resting
//! (`resting_since_bar`) must never take that test at all.

mod common;

use common::{
    AFTER_HOURS_OPEN, Median, NoMedian, PRE_MARKET_OPEN, REGULAR_OPEN, at, bps, fraction, price,
    qty, run,
};
use mandate_accounting::{Liquidity, Side};
use mandate_num::{Bps, Price, Qty};
use mandate_sim::{
    Eligibility, FirstBarVolumes, Instrument, Nanos, OcoLeg, OrderEnd, OrderKind, Session, SimBar,
    SimConfig, SimFill, SimOrder, SimOutcome, Slippage, TimeInForce,
};
use mandate_time::UtcNanos;
use proptest::collection::vec;
use proptest::prelude::*;

/// Prices, quantities, and volumes are integers in units of 10⁻⁹.
const UNIT: i128 = 1_000_000_000;
/// Basis points are integers in units of 10⁻⁸ bps, so `s` is `bps_units × 10⁻¹²`.
const BPS_UNIT: i128 = 100_000_000;
/// `price_units × bps_units ÷ SLIP_DIVISOR` is the slippage in units of 10⁻⁹.
const SLIP_DIVISOR: i128 = 1_000_000_000_000;
/// A cent in units of 10⁻⁹.
const CENT: i128 = UNIT / 100;

const PRE_MARKET: u8 = 0;
const REGULAR: u8 = 1;
const AFTER_HOURS: u8 = 2;
const OVERNIGHT: u8 = 3;

const MAKER: u8 = 0;
const TAKER: u8 = 1;
const AUCTION: u8 = 2;

const LIMIT_LEG: u8 = 0;
const STOP_LEG: u8 = 1;

/// Every minute a generated bar may occupy: its session, its start, its session's start, and its
/// trading day. Two trading days, so a day order's expiry has a day after it.
const SLOTS: [(u8, &str, &str, u8); 11] = [
    (PRE_MARKET, "2026-09-21T09:00:00-04:00", PRE_MARKET_OPEN, 0),
    (PRE_MARKET, "2026-09-21T09:15:00-04:00", PRE_MARKET_OPEN, 0),
    (REGULAR, REGULAR_OPEN, REGULAR_OPEN, 0),
    (REGULAR, "2026-09-21T09:31:00-04:00", REGULAR_OPEN, 0),
    (REGULAR, "2026-09-21T09:33:00-04:00", REGULAR_OPEN, 0),
    (REGULAR, "2026-09-21T09:34:00-04:00", REGULAR_OPEN, 0),
    (
        AFTER_HOURS,
        "2026-09-21T16:00:00-04:00",
        AFTER_HOURS_OPEN,
        0,
    ),
    (
        AFTER_HOURS,
        "2026-09-21T16:05:00-04:00",
        AFTER_HOURS_OPEN,
        0,
    ),
    (
        OVERNIGHT,
        "2026-09-21T20:05:00-04:00",
        "2026-09-21T20:00:00-04:00",
        0,
    ),
    (
        REGULAR,
        "2026-09-22T09:30:00-04:00",
        "2026-09-22T09:30:00-04:00",
        1,
    ),
    (
        REGULAR,
        "2026-09-22T09:31:00-04:00",
        "2026-09-22T09:30:00-04:00",
        1,
    ),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GBar {
    slot: usize,
    start: i64,
    session: u8,
    session_start: i64,
    day: u8,
    auction: bool,
    open: i128,
    high: i128,
    low: i128,
    close: i128,
    volume: i128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GKind {
    Market,
    Limit(i128),
    Stop(i128),
    StopLimit { stop: i128, limit: i128 },
    Oco { limit: i128, stop: i128 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GElig {
    Decided { at: i64, approval: bool },
    Resting(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct GOrder {
    buy: bool,
    qty: i128,
    kind: GKind,
    day_order: bool,
    extended: bool,
    elig: GElig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Scenario {
    bars: Vec<GBar>,
    orders: Vec<GOrder>,
    fraction: i128,
    half_spread: i128,
    impact: i128,
    latency_ms: u64,
    approval_ms: u64,
    median: Option<i128>,
}

fn secs(text: &str) -> i64 {
    at(text).secs()
}

/// Canonical decimal text for `units` in 10⁻⁹, as `mandate-num` requires.
fn dec(units: i128, scale: u32) -> String {
    let negative = units < 0;
    let magnitude = units.unsigned_abs();
    let divisor = 10u128.pow(scale);
    let int = magnitude / divisor;
    let frac = format!("{:0width$}", magnitude % divisor, width = scale as usize);
    let frac = frac.trim_end_matches('0');
    let sign = if negative { "-" } else { "" };
    if frac.is_empty() {
        format!("{sign}{int}")
    } else {
        format!("{sign}{int}.{frac}")
    }
}

fn dec9(units: i128) -> String {
    dec(units, 9)
}

fn price_of(units: i128) -> Price {
    price(&dec9(units))
}

fn qty_of(units: i128) -> Qty {
    qty(&dec9(units))
}

fn bps_of(units: i128) -> Bps {
    bps(&dec(units, 8))
}

/// `ceil(numerator ÷ divisor)` for non-negative values.
fn ceil_div(numerator: i128, divisor: i128) -> i128 {
    let floor = numerator / divisor;
    if numerator % divisor == 0 {
        floor
    } else {
        floor.saturating_add(1)
    }
}

/// The oracle's slippage: `ceil(price × bps ÷ 10⁴)` at 9 places, which moves the price against the
/// order whichever way it goes (DEC-106 item 2).
fn slip(price_units: i128, bps_units: i128) -> i128 {
    ceil_div(price_units.saturating_mul(bps_units), SLIP_DIVISOR)
}

fn slipped(price_units: i128, bps_units: i128, buy: bool) -> i128 {
    let amount = slip(price_units, bps_units);
    if buy {
        price_units.saturating_add(amount)
    } else {
        price_units.saturating_sub(amount)
    }
}

impl Scenario {
    fn slippage_bps(&self) -> i128 {
        self.half_spread.saturating_add(self.impact)
    }

    fn coverage_start(&self) -> i64 {
        self.bars.first().map_or(0, |b| b.start)
    }

    /// Spec §6.4 rule 3, computed the oracle's way: the previous bar of the same session on the same
    /// trading day, else the 20-session median when the session's start is covered, else nothing,
    /// then truncated to whole shares.
    fn cap_at(&self, index: usize) -> i128 {
        let Some(bar) = self.bars.get(index) else {
            return 0;
        };
        let previous = self
            .bars
            .iter()
            .take(index)
            .rev()
            .find(|p| p.day == bar.day && p.session == bar.session);
        let reference = match previous {
            Some(p) => Some(p.volume),
            None if bar.session_start >= self.coverage_start() => self.median,
            None => None,
        };
        match reference {
            Some(volume) => {
                let exact = self.fraction.saturating_mul(volume) / UNIT;
                exact / UNIT * UNIT
            }
            None => 0,
        }
    }

    /// Whether `order` may trade in the session of bar `index` (spec §4.3, §6.4 rule 2).
    fn tradable(&self, order: &GOrder, index: usize) -> bool {
        match self.bars.get(index).map(|b| b.session) {
            Some(REGULAR) => true,
            Some(PRE_MARKET | AFTER_HOURS) => order.extended,
            _ => false,
        }
    }

    /// Spec §5.2: only a limit order may carry `extended_hours`, so the generator never pairs them
    /// with anything else and the model rejects the pair.
    fn well_formed(order: &GOrder) -> bool {
        !order.extended || matches!(order.kind, GKind::Limit(_))
    }

    /// An equity stop triggers only on a regular-session bar (spec §6.4 rule 6).
    fn stop_allowed(&self, index: usize) -> bool {
        self.bars.get(index).map(|b| b.session) == Some(REGULAR)
    }

    /// The first bar an order may fill on: from rule 1's timing, then the first bar in a session it
    /// may trade.
    fn first_bar(&self, order: &GOrder) -> Option<usize> {
        let from = match order.elig {
            GElig::Decided { at, approval } => {
                let mut ready =
                    at.saturating_add(i64::try_from(self.latency_ms).unwrap_or(0) / 1000);
                if approval {
                    ready =
                        ready.saturating_add(i64::try_from(self.approval_ms).unwrap_or(0) / 1000);
                }
                self.bars.iter().position(|b| b.start >= ready)?
            }
            GElig::Resting(bar) => bar,
        };
        (from..self.bars.len()).find(|i| self.tradable(order, *i))
    }

    /// The last bar of the last session a day order may trade on its own trading day (rule 2).
    fn expiry_bar(&self, order: &GOrder) -> Option<usize> {
        let first = self.first_bar(order)?;
        let day = self.bars.get(first)?.day;
        (first..self.bars.len())
            .rev()
            .find(|i| self.bars.get(*i).is_some_and(|b| b.day == day) && self.tradable(order, *i))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// A leg's state between bars. A stop stays `Fresh` until it triggers, which is why there is no
/// separate waiting state and no `(waiting, limit)` pair to rule out.
enum Phase {
    Fresh,
    Marketable,
    Resting(i128),
    Working,
    Gone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OFill {
    order: usize,
    leg: Option<u8>,
    bar: usize,
    qty: i128,
    price: i128,
    liquidity: u8,
}

/// What one leg would do on one bar, before the volume cap is applied.
fn leg_action(
    phase: &mut Phase,
    kind: GKind,
    bar: &GBar,
    buy: bool,
    slippage: i128,
    stop_allowed: bool,
) -> Option<(i128, u8)> {
    let through = |level: i128| {
        if buy {
            bar.low < level
        } else {
            bar.high > level
        }
    };
    let gapped = |level: i128| {
        if buy {
            bar.open < level
        } else {
            bar.open > level
        }
    };
    let marketable = |limit: i128| {
        if buy {
            bar.open <= limit
        } else {
            bar.open >= limit
        }
    };
    let stop_passed = |stop: i128| {
        if buy {
            bar.open >= stop
        } else {
            bar.open <= stop
        }
    };
    let stop_touched = |stop: i128| {
        if buy {
            bar.high >= stop
        } else {
            bar.low <= stop
        }
    };
    let resting_fill = |level: i128| {
        if bar.auction && gapped(level) {
            Some((bar.open, AUCTION))
        } else if through(level) {
            Some((level, MAKER))
        } else {
            None
        }
    };
    let limit_of = |kind: GKind| match kind {
        GKind::Limit(limit) | GKind::StopLimit { limit, .. } | GKind::Oco { limit, .. } => limit,
        GKind::Market | GKind::Stop(_) => 0,
    };
    match (*phase, kind) {
        (Phase::Gone, _) => None,
        (_, GKind::Market) | (Phase::Working, GKind::Stop(_)) => {
            if matches!(kind, GKind::Stop(_)) && !stop_allowed {
                return None;
            }
            *phase = Phase::Working;
            Some((slipped(bar.open, slippage, buy), TAKER))
        }
        (Phase::Fresh, GKind::Limit(limit) | GKind::Oco { limit, .. }) => {
            *phase = if marketable(limit) {
                Phase::Marketable
            } else {
                Phase::Resting(limit)
            };
            leg_action(phase, kind, bar, buy, slippage, stop_allowed)
        }
        (Phase::Marketable, _) => {
            let limit = limit_of(kind);
            if marketable(limit) {
                let slipped_open = slipped(bar.open, slippage, buy);
                let price = if buy {
                    slipped_open.min(limit)
                } else {
                    slipped_open.max(limit)
                };
                Some((price, TAKER))
            } else {
                *phase = Phase::Resting(limit);
                resting_fill(limit)
            }
        }
        (Phase::Resting(limit), _) => resting_fill(limit),
        (Phase::Fresh, GKind::Stop(stop)) => {
            if !stop_allowed {
                return None;
            }
            if stop_passed(stop) {
                *phase = Phase::Working;
                Some((slipped(bar.open, slippage, buy), TAKER))
            } else if stop_touched(stop) {
                *phase = Phase::Working;
                Some((slipped(stop, slippage, buy), TAKER))
            } else {
                None
            }
        }
        (Phase::Fresh, GKind::StopLimit { stop, limit }) => {
            if !stop_allowed || !(stop_passed(stop) || stop_touched(stop)) {
                return None;
            }
            if !marketable(limit) {
                *phase = Phase::Resting(limit);
                return None;
            }
            let trigger = if buy {
                bar.open.max(stop)
            } else {
                bar.open.min(stop)
            };
            let slipped_trigger = slipped(trigger, slippage, buy);
            *phase = Phase::Resting(limit);
            Some((
                if buy {
                    slipped_trigger.min(limit)
                } else {
                    slipped_trigger.max(limit)
                },
                TAKER,
            ))
        }
        (Phase::Working, _) => Some((slipped(bar.open, slippage, buy), TAKER)),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Oracle {
    fills: Vec<OFill>,
    canceled: Vec<(usize, u8, usize)>,
    ends: Vec<OrderEnd>,
}

/// The whole fill model again, order-major and in integers.
fn oracle(s: &Scenario) -> Oracle {
    let mut cap: Vec<i128> = (0..s.bars.len()).map(|i| s.cap_at(i)).collect();
    let mut fills = Vec::new();
    let mut canceled = Vec::new();
    let mut ends = Vec::new();
    for (index, order) in s.orders.iter().enumerate() {
        let mut remaining = order.qty;
        let mut end = OrderEnd::Open;
        let arriving = matches!(order.elig, GElig::Decided { .. });
        let start_of = |limit: i128| {
            if arriving {
                Phase::Fresh
            } else {
                Phase::Resting(limit)
            }
        };
        let (mut legs, single) = match order.kind {
            GKind::Oco { limit, stop } => (
                vec![
                    (Some(LIMIT_LEG), GKind::Limit(limit), Phase::Resting(limit)),
                    (Some(STOP_LEG), GKind::Stop(stop), Phase::Fresh),
                ],
                false,
            ),
            GKind::Limit(limit) => (vec![(None, GKind::Limit(limit), start_of(limit))], true),
            kind => (vec![(None, kind, Phase::Fresh)], true),
        };
        let Some(first) = s.first_bar(order) else {
            ends.push(end);
            continue;
        };
        let expiry = if order.day_order {
            s.expiry_bar(order)
        } else {
            None
        };
        for bar_index in first..s.bars.len() {
            if remaining == 0 {
                break;
            }
            if expiry.is_some_and(|last| bar_index > last) {
                end = OrderEnd::Expired {
                    at_bar: expiry.unwrap_or(bar_index),
                };
                break;
            }
            if !s.tradable(order, bar_index) {
                continue;
            }
            let Some(bar) = s.bars.get(bar_index) else {
                continue;
            };
            let stop_allowed = s.stop_allowed(bar_index);
            let mut candidates = Vec::new();
            for (leg, kind, phase) in &mut legs {
                let action =
                    leg_action(phase, *kind, bar, order.buy, s.slippage_bps(), stop_allowed);
                if let Some((price, liquidity)) = action {
                    candidates.push((*leg, price, liquidity));
                }
            }
            let chosen = if single {
                candidates.first().copied()
            } else {
                pick_oco_leg(&candidates, order, bar)
            };
            let Some((leg, price, liquidity)) = chosen else {
                continue;
            };
            let room = cap.get(bar_index).copied().unwrap_or(0);
            let filled = remaining.min(room);
            if filled > 0 {
                if let Some(slot) = cap.get_mut(bar_index) {
                    *slot = room.saturating_sub(filled);
                }
                remaining = remaining.saturating_sub(filled);
                fills.push(OFill {
                    order: index,
                    leg,
                    bar: bar_index,
                    qty: filled,
                    price,
                    liquidity,
                });
                if !single && let Some(filled_leg) = leg {
                    for (other, _, phase) in &mut legs {
                        if *other != Some(filled_leg) {
                            if let Some(other_leg) = *other {
                                canceled.push((index, other_leg, bar_index));
                            }
                            *phase = Phase::Gone;
                        }
                    }
                    legs.retain(|(_, _, phase)| *phase != Phase::Gone);
                }
                if remaining == 0 {
                    end = OrderEnd::Filled;
                }
            }
        }
        if remaining > 0
            && let Some(last) = expiry
            && s.bars.len() > last
            && end == OrderEnd::Open
        {
            end = OrderEnd::Expired { at_bar: last };
        }
        ends.push(end);
    }
    fills.sort_by_key(|f| (f.bar, f.order));
    canceled.sort_by_key(|(order, _, bar)| (*bar, *order));
    Oracle {
        fills,
        canceled,
        ends,
    }
}

/// Spec §6.4 rule 8: the open reaching a leg fills that leg first; otherwise, with both reachable
/// inside the bar, the stop fills first.
fn pick_oco_leg(
    candidates: &[(Option<u8>, i128, u8)],
    order: &GOrder,
    bar: &GBar,
) -> Option<(Option<u8>, i128, u8)> {
    let GKind::Oco { limit, stop } = order.kind else {
        return candidates.first().copied();
    };
    let leg = |wanted: u8| {
        candidates
            .iter()
            .find(|(l, _, _)| *l == Some(wanted))
            .copied()
    };
    let limit_reached = if order.buy {
        bar.open <= limit
    } else {
        bar.open >= limit
    };
    let stop_reached = if order.buy {
        bar.open >= stop
    } else {
        bar.open <= stop
    };
    if limit_reached && let Some(found) = leg(LIMIT_LEG) {
        return Some(found);
    }
    if stop_reached && let Some(found) = leg(STOP_LEG) {
        return Some(found);
    }
    leg(STOP_LEG).or_else(|| leg(LIMIT_LEG))
}

fn session_of(code: u8) -> Session {
    match code {
        PRE_MARKET => Session::PreMarket,
        AFTER_HOURS => Session::AfterHours,
        OVERNIGHT => Session::Overnight,
        _ => Session::Regular,
    }
}

fn sim_bars(s: &Scenario) -> Vec<SimBar> {
    s.bars
        .iter()
        .map(|b| {
            let (_, start, session_start, _) = SLOTS.get(b.slot).copied().unwrap_or(SLOTS[0]);
            SimBar {
                start: at(start),
                open: price_of(b.open),
                high: price_of(b.high),
                low: price_of(b.low),
                close: price_of(b.close),
                volume: qty_of(b.volume),
                trade_date: at(session_start).date(),
                session: session_of(b.session),
                session_start: at(session_start),
                auction: b.auction,
            }
        })
        .collect()
}

fn sim_orders(s: &Scenario) -> Vec<SimOrder> {
    s.orders
        .iter()
        .map(|o| SimOrder {
            side: if o.buy { Side::Buy } else { Side::Sell },
            qty: qty_of(o.qty),
            kind: match o.kind {
                GKind::Market => OrderKind::Market,
                GKind::Limit(limit) => OrderKind::Limit {
                    limit: price_of(limit),
                },
                GKind::Stop(stop) => OrderKind::Stop {
                    stop: price_of(stop),
                },
                GKind::StopLimit { stop, limit } => OrderKind::StopLimit {
                    stop: price_of(stop),
                    limit: price_of(limit),
                },
                GKind::Oco { limit, stop } => OrderKind::Oco {
                    limit: price_of(limit),
                    stop: price_of(stop),
                },
            },
            tif: if o.day_order {
                TimeInForce::Day
            } else {
                TimeInForce::Gtc
            },
            extended_hours: o.extended,
            eligible_from: match o.elig {
                GElig::Decided { at: when, approval } => Eligibility::DecidedAt {
                    at: UtcNanos::from_parts(when, 0).unwrap(),
                    approval_required: approval,
                },
                GElig::Resting(bar) => Eligibility::Resting { from_bar: bar },
            },
        })
        .collect()
}

fn sim_config(s: &Scenario) -> SimConfig {
    SimConfig {
        decision_latency: Nanos::from_millis(s.latency_ms).unwrap(),
        approval_latency: Nanos::from_millis(s.approval_ms).unwrap(),
        slippage: Slippage::Fixed {
            half_spread_bps: bps_of(s.half_spread),
            impact_bps: bps_of(s.impact),
        },
        volume_cap_fraction: fraction(&dec9(s.fraction)),
    }
}

/// Runs the model, and checks the number of fills against the oracle's before any property looks at
/// them: a property about "no fill that breaks rule R" would otherwise pass on an empty list.
fn simulated(s: &Scenario) -> Result<SimOutcome, TestCaseError> {
    let outcome = ran(s)?;
    let expected = oracle(s).fills.len();
    if outcome.fills.len() != expected {
        return Err(TestCaseError::fail(format!(
            "the model reported {} fills where the oracle expects {expected}",
            outcome.fills.len()
        )));
    }
    Ok(outcome)
}

fn ran(s: &Scenario) -> Result<SimOutcome, TestCaseError> {
    let bars = sim_bars(s);
    let orders = sim_orders(s);
    let median = s.median.map(qty_of);
    let volumes: Box<dyn FirstBarVolumes> = match median {
        Some(volume) => Box::new(Median(volume)),
        None => Box::new(NoMedian),
    };
    run(
        &sim_config(s),
        &Instrument {
            asset_class: mandate_accounting::AssetClass::UsEquity,
            increment: mandate_num::ShareIncrement::Whole,
        },
        &bars,
        volumes.as_ref(),
        &orders,
    )
    .map_err(|e| TestCaseError::fail(format!("{e} ({})", e.code())))
}

/// Reads a reported quantity or price back into units of 10⁻⁹, so comparisons stay exact.
fn units(text: &str) -> i128 {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (int, frac) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    assert!(frac.len() <= 9, "{text} has more than 9 places");
    let value: i128 = format!("{int}{frac:0<9}").parse().unwrap();
    if negative { -value } else { value }
}

fn reported(outcome: &SimOutcome) -> Vec<OFill> {
    outcome
        .fills
        .iter()
        .map(|f| OFill {
            order: f.order.index(),
            leg: f.leg.map(|l| match l {
                OcoLeg::Limit => LIMIT_LEG,
                OcoLeg::Stop => STOP_LEG,
            }),
            bar: f.bar,
            qty: units(&f.qty.to_string()),
            price: units(&f.price.to_string()),
            liquidity: match f.liquidity {
                Some(Liquidity::Maker) => MAKER,
                Some(Liquidity::Taker) => TAKER,
                None => AUCTION,
            },
        })
        .collect()
}

/// The limit an order or a leg may never trade through, if it has one.
fn limit_of(order: &GOrder, leg: Option<u8>) -> Option<i128> {
    match (order.kind, leg) {
        (GKind::Limit(limit), _) | (GKind::StopLimit { limit, .. }, _) => Some(limit),
        (GKind::Oco { limit, .. }, Some(LIMIT_LEG)) => Some(limit),
        _ => None,
    }
}

fn order_of<'a>(s: &'a Scenario, fill: &SimFill) -> &'a GOrder {
    s.orders.get(fill.order.index()).unwrap_or(&s.orders[0])
}

fn bar_of<'a>(s: &'a Scenario, fill: &SimFill) -> &'a GBar {
    s.bars.get(fill.bar).unwrap_or(&s.bars[0])
}

/// Order levels sit within 0.60 of the scenario's base price, as the bars' opens do, so a limit or a
/// stop is reached on some bars and not on others.
fn kind(base: i128) -> impl Strategy<Value = GKind> {
    let level = (-60i128..=60).prop_map(move |delta| base + delta * CENT);
    prop_oneof![
        Just(GKind::Market),
        level.clone().prop_map(GKind::Limit),
        level.clone().prop_map(GKind::Stop),
        (level.clone(), 1i128..200).prop_map(|(stop, gap)| GKind::StopLimit {
            stop,
            limit: stop - gap * CENT,
        }),
        (level, 1i128..400).prop_map(|(stop, gap)| GKind::Oco {
            limit: stop + gap * CENT,
            stop,
        }),
    ]
}

/// `kind()` generates the geometry a long position exits with: a stop-limit's limit below its stop
/// and a take-profit above it. A buy mirrors both around the stop, so no generated order is one the
/// model rejects.
fn mirrored(kind: GKind, buy: bool) -> GKind {
    match (buy, kind) {
        (true, GKind::StopLimit { stop, limit }) => GKind::StopLimit {
            stop,
            limit: stop + (stop - limit),
        },
        (true, GKind::Oco { limit, stop }) => GKind::Oco {
            limit: stop - (limit - stop),
            stop,
        },
        (_, other) => other,
    }
}

fn order(bars: usize, base: i128) -> impl Strategy<Value = GOrder> {
    (
        any::<bool>(),
        1i128..2000,
        kind(base),
        any::<bool>(),
        any::<bool>(),
        0usize..(bars.max(1)),
        prop_oneof![Just(None), (0usize..SLOTS.len()).prop_map(Some)],
        any::<bool>(),
    )
        .prop_map(
            |(buy, shares, kind, day_order, extended, resting, decided, approval)| GOrder {
                buy,
                qty: shares * UNIT,
                kind: mirrored(kind, buy),
                extended: extended && matches!(mirrored(kind, buy), GKind::Limit(_)),
                day_order,
                elig: match decided {
                    Some(slot) => GElig::Decided {
                        at: secs(SLOTS.get(slot).map_or(REGULAR_OPEN, |s| s.1)),
                        approval,
                    },
                    None => GElig::Resting(resting),
                },
            },
        )
}

/// A scenario: which minutes traded, their prices and volumes, the orders, and the configuration.
fn scenario() -> impl Strategy<Value = Scenario> {
    (
        vec(any::<bool>(), SLOTS.len()),
        (
            500i128..4000,
            vec(
                (-50i128..=50, 0i128..30, 0i128..30, 0i128..20000),
                SLOTS.len(),
            ),
        ),
        prop_oneof![
            Just(0),
            Just(UNIT / 10_000),
            Just(UNIT / 1_000),
            Just(UNIT / 100),
            Just(UNIT / 10),
            Just(UNIT)
        ],
        prop_oneof![
            Just(0),
            Just(BPS_UNIT),
            Just(2 * BPS_UNIT),
            Just(123_456_789)
        ],
        prop_oneof![Just(0), Just(2 * BPS_UNIT), Just(5 * BPS_UNIT)],
        prop_oneof![Just(0u64), Just(60_000), Just(120_000)],
        prop_oneof![Just(0u64), Just(60_000)],
        prop_oneof![Just(None), Just(Some(1000)), Just(Some(9000))],
    )
        .prop_flat_map(
            |(
                included,
                (base, candles),
                fraction,
                half_spread,
                impact,
                latency_ms,
                approval_ms,
                median,
            )| {
                let mut bars = Vec::new();
                let mut seen_regular: Vec<u8> = Vec::new();
                let coverage = included.iter().position(|keep| *keep);
                for (slot, keep) in included.iter().enumerate() {
                    if !keep {
                        continue;
                    }
                    let (session, start, session_start, day) =
                        SLOTS.get(slot).copied().unwrap_or(SLOTS[0]);
                    let (delta, up, down, volume) =
                        candles.get(slot).copied().unwrap_or((0, 10, 10, 1000));
                    let open = (base + delta) * CENT;
                    let covered = coverage.is_some_and(|first| {
                        secs(session_start) >= secs(SLOTS.get(first).map_or(REGULAR_OPEN, |s| s.1))
                    });
                    let first_regular_of_the_day =
                        session == REGULAR && !seen_regular.contains(&day);
                    let auction = first_regular_of_the_day && covered;
                    if first_regular_of_the_day {
                        seen_regular.push(day);
                    }
                    bars.push(GBar {
                        slot,
                        start: secs(start),
                        session,
                        session_start: secs(session_start),
                        day,
                        auction,
                        open,
                        high: open + up * CENT,
                        low: (open - down * CENT).max(CENT),
                        close: open,
                        volume: volume * UNIT,
                    });
                }
                let count = bars.len();
                (
                    Just(bars),
                    vec(order(count, base * CENT), 1..=3),
                    Just(fraction),
                    Just(half_spread),
                    Just(impact),
                    Just(latency_ms),
                    Just(approval_ms),
                    Just(median),
                )
            },
        )
        .prop_map(
            |(bars, orders, fraction, half_spread, impact, latency_ms, approval_ms, median)| {
                let count = bars.len();
                Scenario {
                    bars,
                    orders: orders
                        .into_iter()
                        .map(|o| GOrder {
                            elig: match o.elig {
                                GElig::Resting(bar) if count > 0 => GElig::Resting(bar % count),
                                other => other,
                            },
                            ..o
                        })
                        .collect(),
                    fraction,
                    half_spread,
                    impact,
                    latency_ms,
                    approval_ms,
                    median: median.map(|volume: i128| volume * UNIT),
                }
            },
        )
        .prop_filter("at least one bar", |s| !s.bars.is_empty())
        .prop_filter("only a limit order carries extended hours", |s| {
            s.orders.iter().all(Scenario::well_formed)
        })
}

proptest! {
    /// Spec §6.4 rule 1: nothing fills before the first bar starting at or after the decision time
    /// plus the decision latency, and the approval latency too when approval was required. The bar
    /// that produced the decision starts strictly earlier than the decision, so it never fills.
    #[test]
    fn no_fill_before_eligibility(s in scenario()) {
        let outcome = simulated(&s)?;
        for fill in &outcome.fills {
            let order = order_of(&s, fill);
            let first = s.first_bar(order);
            prop_assert!(
                first.is_some_and(|first| fill.bar >= first),
                "fill on bar {} before the first eligible bar {first:?}", fill.bar
            );
            if let GElig::Decided { at: decided, approval } = order.elig {
                let mut ready = decided + i64::try_from(s.latency_ms).unwrap_or(0) / 1000;
                if approval {
                    ready += i64::try_from(s.approval_ms).unwrap_or(0) / 1000;
                }
                prop_assert!(
                    bar_of(&s, fill).start >= ready,
                    "fill on a bar starting before the order was eligible"
                );
            }
        }
    }

    /// Spec §6.4 rule 3: what a bar fills across every order never passes
    /// truncate(fraction × reference volume, increment), and is 0 when the reference volume is
    /// unavailable (DEC-106 item 4).
    #[test]
    fn fills_never_exceed_the_shared_volume_cap(s in scenario()) {
        let outcome = simulated(&s)?;
        for index in 0..s.bars.len() {
            let filled: i128 = reported(&outcome)
                .iter()
                .filter(|f| f.bar == index)
                .map(|f| f.qty)
                .sum();
            prop_assert!(
                filled <= s.cap_at(index),
                "bar {index} filled {filled} against a cap of {}", s.cap_at(index)
            );
        }
    }

    /// An order never fills more than it asked for, and every fill is a positive multiple of the
    /// instrument's quantity increment (spec §2.1, §6.4 rule 3).
    #[test]
    fn orders_never_overfill(s in scenario()) {
        let outcome = simulated(&s)?;
        for (index, order) in s.orders.iter().enumerate() {
            let filled: i128 = reported(&outcome)
                .iter()
                .filter(|f| f.order == index)
                .map(|f| f.qty)
                .sum();
            prop_assert!(filled <= order.qty, "order {index} filled {filled} of {}", order.qty);
            prop_assert_eq!(
                filled == order.qty,
                outcome.end_of(mandate_sim::OrderRef::new(index)) == Some(OrderEnd::Filled)
            );
        }
        for fill in reported(&outcome) {
            prop_assert!(fill.qty > 0, "a fill of {}", fill.qty);
            prop_assert_eq!(fill.qty % UNIT, 0, "a fill off the whole-share increment");
        }
    }

    /// Spec §6.4 rule 5: a resting limit fills only when the bar's extreme passes it **strictly**. A
    /// bar whose low (buy) or high (sell) merely touches the limit is not a fill.
    #[test]
    fn a_touch_is_never_a_fill(s in scenario()) {
        let outcome = simulated(&s)?;
        for fill in &outcome.fills {
            if fill.liquidity != Some(Liquidity::Maker) {
                continue;
            }
            let order = order_of(&s, fill);
            let bar = bar_of(&s, fill);
            let level = units(&fill.price.to_string());
            let passed = if order.buy { bar.low < level } else { bar.high > level };
            prop_assert!(passed, "a maker fill at {level} on a bar that only touched it");
        }
    }

    /// Spec §6.4 rule 5 and rule 7: a buy never pays above its limit and a sell never receives below
    /// it, whatever the slippage.
    #[test]
    fn limit_prices_are_never_violated(s in scenario()) {
        let outcome = simulated(&s)?;
        for fill in &outcome.fills {
            let order = order_of(&s, fill);
            let leg = fill.leg.map(|l| match l {
                OcoLeg::Limit => LIMIT_LEG,
                OcoLeg::Stop => STOP_LEG,
            });
            let Some(limit) = limit_of(order, leg) else {
                continue;
            };
            let paid = units(&fill.price.to_string());
            if order.buy {
                prop_assert!(paid <= limit, "a buy paid {paid} above its limit {limit}");
            } else {
                prop_assert!(paid >= limit, "a sell received {paid} below its limit {limit}");
            }
        }
    }

    /// DEC-106 item 2, and the spec's slippage: the schedule of fills does not depend on slippage at
    /// all — marketability, triggering, and the cap are decided by the bar and the order's prices —
    /// while every price moves against the order. Compared with the same scenario at zero slippage,
    /// each buy pays at least as much and each sell receives at most as much.
    #[test]
    fn slippage_and_rounding_are_adverse(s in scenario()) {
        let frictionless = Scenario { half_spread: 0, impact: 0, ..s.clone() };
        let with_slippage = reported(&simulated(&s)?);
        let without = reported(&simulated(&frictionless)?);
        prop_assert_eq!(
            with_slippage.iter().map(|f| (f.bar, f.order, f.leg, f.qty)).collect::<Vec<_>>(),
            without.iter().map(|f| (f.bar, f.order, f.leg, f.qty)).collect::<Vec<_>>(),
            "slippage changed which bars filled what"
        );
        for (slipped_fill, base) in with_slippage.iter().zip(&without) {
            let order = s.orders.get(slipped_fill.order).copied();
            let buy = order.is_some_and(|o| o.buy);
            if buy {
                prop_assert!(slipped_fill.price >= base.price, "a buy paid less with slippage");
            } else {
                prop_assert!(slipped_fill.price <= base.price, "a sell received more with slippage");
            }
        }
    }

    /// Spec §6.4 rule 2 and rule 6: a fill only ever happens in a session the order may trade, and a
    /// stop only ever fills on a regular-session bar for an equity.
    #[test]
    fn fills_respect_sessions(s in scenario()) {
        let outcome = simulated(&s)?;
        for fill in &outcome.fills {
            let order = order_of(&s, fill);
            prop_assert!(s.tradable(order, fill.bar), "a fill in a session the order may not trade");
            let from_a_stop = matches!(order.kind, GKind::Stop(_) | GKind::StopLimit { .. })
                || fill.leg == Some(OcoLeg::Stop);
            if from_a_stop && fill.liquidity == Some(Liquidity::Taker) {
                prop_assert!(
                    s.stop_allowed(fill.bar),
                    "a stop triggered outside the regular session"
                );
            }
        }
    }

    /// Spec §6.4 rule 2: a day order fills nothing after the last bar of its last eligible session,
    /// and its remainder is reported canceled at that bar.
    #[test]
    fn day_orders_expire_at_session_end(s in scenario()) {
        let outcome = simulated(&s)?;
        for (index, order) in s.orders.iter().enumerate() {
            if !order.day_order {
                continue;
            }
            let Some(last) = s.expiry_bar(order) else {
                continue;
            };
            let mine: Vec<&SimFill> = outcome
                .fills_of(mandate_sim::OrderRef::new(index))
                .collect();
            prop_assert!(
                mine.iter().all(|f| f.bar <= last),
                "a day order filled after bar {last}"
            );
            let filled: i128 = mine.iter().map(|f| units(&f.qty.to_string())).sum();
            let end = outcome.end_of(mandate_sim::OrderRef::new(index));
            if filled < order.qty && s.bars.len() > last {
                prop_assert_eq!(end, Some(OrderEnd::Expired { at_bar: last }));
            }
        }
    }

    /// Spec §6.4 rule 8 and DEC-106 item 8: at most one leg of an OCO ever fills, and the first fill
    /// of that leg cancels the other, at that bar.
    #[test]
    fn oco_fills_at_most_one_leg(s in scenario()) {
        let outcome = simulated(&s)?;
        for (index, order) in s.orders.iter().enumerate() {
            if !matches!(order.kind, GKind::Oco { .. }) {
                continue;
            }
            let reference = mandate_sim::OrderRef::new(index);
            let legs: Vec<Option<OcoLeg>> =
                outcome.fills_of(reference).map(|f| f.leg).collect();
            prop_assert!(
                legs.windows(2).all(|pair| pair.first() == pair.last()),
                "two legs of one OCO filled"
            );
            let canceled: Vec<&mandate_sim::CanceledLeg> = outcome
                .canceled_legs
                .iter()
                .filter(|c| c.order == reference)
                .collect();
            match (legs.first(), outcome.fills_of(reference).next()) {
                (Some(Some(filled)), Some(first)) => {
                    prop_assert_eq!(canceled.len(), 1, "one leg filled, the other was not canceled");
                    let lost = canceled.first().map(|c| (c.leg, c.bar));
                    prop_assert_eq!(
                        lost,
                        Some((if *filled == OcoLeg::Limit { OcoLeg::Stop } else { OcoLeg::Limit }, first.bar))
                    );
                }
                _ => prop_assert!(canceled.is_empty(), "a leg was canceled with no fill"),
            }
        }
    }

    /// Replay (ADR-0001 ES-21): the same inputs give the same fills, cancellations, and end states.
    #[test]
    fn identical_inputs_give_identical_fills(s in scenario()) {
        prop_assert_eq!(ran(&s)?, ran(&s)?);
    }

    /// Every fill, cancellation, and end state matches the order-major integer simulator in this
    /// file, which computes them its own way.
    #[test]
    fn fills_match_the_independent_simulator(s in scenario()) {
        let outcome = ran(&s)?;
        let expected = oracle(&s);
        prop_assert_eq!(reported(&outcome), expected.fills, "fills");
        prop_assert_eq!(
            outcome
                .canceled_legs
                .iter()
                .map(|c| (
                    c.order.index(),
                    if c.leg == OcoLeg::Limit { LIMIT_LEG } else { STOP_LEG },
                    c.bar
                ))
                .collect::<Vec<_>>(),
            expected.canceled,
            "canceled legs"
        );
        prop_assert_eq!(outcome.ends, expected.ends, "end states");
    }
}
