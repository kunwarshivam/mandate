//! The fill model itself (trading-domain spec §6.4, rules 1 to 9).

use crate::{
    CanceledLeg, Eligibility, FirstBarVolumes, Instrument, OcoLeg, OrderEnd, OrderKind, OrderRef,
    Session, SimBar, SimConfig, SimError, SimFill, SimOrder, SimOutcome, Slippage, TimeInForce,
};
use mandate_accounting::{AssetClass, Liquidity, Side};
use mandate_num::{Adverse, Bps, Fraction, Price, Qty, ShareIncrement};
use mandate_time::UtcNanos;

/// Walks `bars` once, filling `orders` under spec §6.4, and returns every fill, every canceled OCO
/// leg, and each order's end state.
///
/// The rules, in the order the walk applies them to a bar:
///
/// 1. **Timing.** An order is eligible from the first bar starting at or after its decision time
///    plus the configured latencies (rule 1). The bar that produced the decision starts strictly
///    earlier, so it never fills.
/// 2. **Sessions.** A bar fills an order only in a session that order may trade: the regular
///    session, plus the extended sessions when `extended_hours` is set, and any session for a
///    crypto instrument, whose bars are continuous (rule 2, spec §4.3). A day order's remainder is
///    canceled after the last bar of its last eligible session.
/// 3. **Volume cap.** Each bar fills at most `truncate(fraction × reference volume, increment)`
///    across every order in the instrument, taken in submission order. The reference volume is the
///    most recent earlier bar of the same session and trading day; for a session's first covered
///    bar it is the 20-session median from `first_bar_volumes`; when neither exists the cap is 0
///    (rule 3, DEC-106 item 4). `coverage_start` is the first instant the data covers: a bar whose
///    session began before it has no known previous bar and no median, so its cap is 0.
/// 4. **Market orders** fill at the open moved against the order by `s` (rule 4), taker.
/// 5. **Limit orders** marketable on arrival fill at `min(limit, open ± s)` as taker and keep doing
///    so while later bars open within the limit; a bar that opens beyond the limit turns the
///    remainder into a resting limit. A resting limit fills at its limit as maker when the bar's
///    extreme passes it strictly; a touch is not a fill. On an auction bar a resting limit the open
///    gaps through fills at the open, with no slippage and no liquidity flag (rule 5, DEC-106
///    items 5 and 6).
/// 6. **Stop orders** fill at the open moved by `s` when the open has already passed the stop, and
///    at the stop moved by `s` when the bar's extreme reaches it. An equity stop triggers only on a
///    regular-session bar (rule 6).
/// 7. **Stop-limit orders** whose trigger bar opens beyond the limit fill nothing in that bar, even
///    if it later prints through the limit, and rest as a limit at `L` from the **next** bar;
///    otherwise the triggered fill is at the limit or the triggered price, whichever is better for
///    the order (rule 7, DEC-106 item 7).
/// 8. **OCO pairs.** If the open reaches a leg, that leg fills first under its own rule; otherwise,
///    with both legs reachable inside the bar, the stop fills first. The first fill of either leg,
///    partial or whole, cancels the other (rule 8, DEC-106 item 8).
/// 9. Fill prices are **not tick-rounded** (rule 9). A price needing more than the 9 places spec
///    §2.1 allows is rounded against the order (DEC-106 item 2).
///
/// A trigger or a change of phase happens on the bar that causes it even when the cap lets nothing
/// fill there, and a triggered stop's remainder — an OCO's too, once its stop leg fills — is a
/// market order (DEC-106 item 10).
///
/// Errors: a bar sequence that is not strictly increasing, inconsistent, or labelled with a session
/// that starts after it; an order outside the v1 policy of spec §5.1 and §5.2 (no quantity, a
/// quantity off the instrument's increment, `extended_hours` on anything but a limit order, crossed
/// stop-limit or OCO prices, an OCO on a fractional instrument, a day order on a continuous
/// instrument, or a resting bar the sequence does not hold); and any arithmetic that is not exact.
pub fn simulate(
    config: &SimConfig,
    instrument: &Instrument,
    bars: &[SimBar],
    coverage_start: UtcNanos,
    first_bar_volumes: &dyn FirstBarVolumes,
    orders: &[SimOrder],
) -> Result<SimOutcome, SimError> {
    check_bars(bars)?;
    for order in orders {
        check_order(order, instrument, bars.len())?;
    }
    let mut rooms = rooms(
        bars,
        config,
        instrument.increment,
        coverage_start,
        first_bar_volumes,
    )?;
    let mut working = orders
        .iter()
        .map(|order| Working::arriving(order, bars, config))
        .collect::<Result<Vec<Working<'_>>, SimError>>()?;
    let mut fills = Vec::new();
    let mut canceled_legs = Vec::new();
    for (index, bar) in bars.iter().enumerate() {
        for (position, order) in working.iter_mut().enumerate() {
            let Some(room) = rooms.get_mut(index) else {
                continue;
            };
            let step = order.on_bar(OrderRef::new(position), index, bar, config, room)?;
            fills.extend(step.fill);
            canceled_legs.extend(step.canceled);
        }
    }
    Ok(SimOutcome {
        fills,
        canceled_legs,
        ends: working.iter().map(Working::end).collect(),
    })
}

/// Spec §4.1: bars arrive in strictly increasing order, each one consistent in itself, and each at
/// or after the session it is labelled with (DEC-108 item 3 puts the label on the bar). A consistent
/// bar keeps its low at or below both its open and its close and its high at or above both, which is
/// what the trigger rules rely on when they take a reach inside the bar as covering a gapped open.
fn check_bars(bars: &[SimBar]) -> Result<(), SimError> {
    let mut previous: Option<&SimBar> = None;
    for (index, bar) in bars.iter().enumerate() {
        if previous.is_some_and(|earlier| bar.start <= earlier.start) {
            return Err(SimError::BarsOutOfOrder(index));
        }
        if bar.low > bar.open.min(bar.close) || bar.high < bar.open.max(bar.close) {
            return Err(SimError::InconsistentBar(index));
        }
        if bar.start < bar.session_start {
            return Err(SimError::BarBeforeItsSession(index));
        }
        previous = Some(bar);
    }
    Ok(())
}

/// The session-of-asset-class check E4-3 adds (the gap DEC-114 item 2 discloses; DEC-377): spec
/// §4.3 gives crypto the continuous session alone and a US equity the four New York sessions
/// alone, so a bar labelled with a session its instrument's asset class never trades contradicts
/// the spec and is refused with [`SimError::SessionOffAssetClass`] naming the first such bar's
/// index, rather than simulated: an equity stop must never fill on a session no equity bar can
/// carry. An empty bar sequence carries no session and is `Ok`.
///
/// The implementation PR replaces this stub's body and calls it from [`simulate`] **after** the
/// order-policy loop (`check_order` over every order), not beside `check_bars`: an order the v1
/// policy refuses keeps its own cause as the first failing cause, so a crypto order the policy
/// refuses on a regular-session bar (an OCO on a fractional instrument, a day order on a continuous
/// instrument) still reports that cause, as the live
/// `the_model_rejects_bars_and_orders_it_cannot_simulate` pins. Until then it reports itself,
/// which is what the pending gate reads (DEC-137).
pub fn check_sessions_of_asset_class(
    instrument: &Instrument,
    bars: &[SimBar],
) -> Result<(), SimError> {
    let _ = (instrument, bars);
    Err(SimError::Unimplemented)
}

/// The v1 order policy of spec §5.1 and §5.2, checked before anything is simulated, each cause with
/// its own error and stable code (DEC-108 item 7). An order the platform would never place is
/// rejected rather than simulated, so no backtest can rest on one.
fn check_order(order: &SimOrder, instrument: &Instrument, bars: usize) -> Result<(), SimError> {
    if order.qty.is_zero() {
        return Err(SimError::ZeroQuantity);
    }
    if order.qty.portion(Fraction::ONE, instrument.increment)? != order.qty {
        return Err(SimError::QuantityOffIncrement);
    }
    if order.extended_hours && !matches!(order.kind, OrderKind::Limit { .. }) {
        return Err(SimError::ExtendedHoursNeedsALimit);
    }
    match order.kind {
        OrderKind::StopLimit { stop, limit } if stop_limit_crossed(order.side, stop, limit) => {
            return Err(SimError::StopLimitCrossed);
        }
        OrderKind::Oco { limit, stop } if oco_legs_crossed(order.side, stop, limit) => {
            return Err(SimError::OcoLegsCrossed);
        }
        OrderKind::Oco { .. } if instrument.increment == ShareIncrement::Fractional => {
            return Err(SimError::OcoOnAFractionalInstrument);
        }
        _ => {}
    }
    if order.tif == TimeInForce::Day && instrument.asset_class == AssetClass::Crypto {
        return Err(SimError::DayOrderOnAContinuousInstrument);
    }
    if let Eligibility::Resting { from_bar } = order.eligible_from
        && from_bar >= bars
    {
        return Err(SimError::RestingBarOutOfRange(from_bar));
    }
    Ok(())
}

/// A bar's volume-cap state (spec §6.4 rule 3): the reference volume the cap came from, which the
/// `sqrt` impact model needs too, and how much of the cap later orders may still take.
struct Room {
    reference: Qty,
    left: Qty,
}

/// Every bar's cap and the reference volume it came from (spec §6.4 rule 3, DEC-106 item 4): the
/// previous bar of the same session and trading day, else the 20-session median when the session's
/// start is covered by the data, else nothing, which caps the bar at zero — and a zero cap fills
/// nothing, so a fill never divides by a reference volume of zero.
fn rooms(
    bars: &[SimBar],
    config: &SimConfig,
    increment: ShareIncrement,
    coverage_start: UtcNanos,
    first_bar_volumes: &dyn FirstBarVolumes,
) -> Result<Vec<Room>, SimError> {
    bars.iter()
        .enumerate()
        .map(|(index, bar)| {
            let previous = bars
                .get(..index)
                .unwrap_or_default()
                .iter()
                .rev()
                .find(|earlier| {
                    earlier.trade_date == bar.trade_date && earlier.session == bar.session
                });
            let reference = match previous {
                Some(earlier) => Some(earlier.volume),
                None if bar.session_start >= coverage_start => {
                    first_bar_volumes.median_at(bar.start)
                }
                None => None,
            }
            .unwrap_or(Qty::ZERO);
            Ok(Room {
                left: reference.portion(config.volume_cap_fraction, increment)?,
                reference,
            })
        })
        .collect()
}

/// Whether `subject` lies on the favourable side of `level` for `side`: strictly below it for a buy
/// and strictly above it for a sell. This is the one comparison spec §6.4 draws between a price and a
/// level, and every rule below is written from it, so a buy and a sell cannot drift apart. Equality
/// is on neither side: that is why a touch is not a fill (rule 5).
fn favours(side: Side, subject: Price, level: Price) -> bool {
    match side {
        Side::Buy => subject < level,
        Side::Sell => subject > level,
    }
}

/// Spec §5.1: a stop-limit's limit is crossed when it lies on the far side of its stop, so the order
/// could only ever fill at a price the stop was meant to protect against.
fn stop_limit_crossed(side: Side, stop: Price, limit: Price) -> bool {
    favours(side, limit, stop)
}

/// Spec §5.4: a protective pair's legs are crossed when its stop lies on the take-profit's side of
/// the market, so both legs describe the same exit.
fn oco_legs_crossed(side: Side, stop: Price, limit: Price) -> bool {
    favours(side, stop, limit)
}

/// One bar as one side of the market meets it (spec §6.4 rules 5 to 8).
struct Seen<'b> {
    bar: &'b SimBar,
    side: Side,
}

impl Seen<'_> {
    /// The bar's extreme in this side's favour: its low for a buy, its high for a sell.
    fn favourable_extreme(&self) -> Price {
        match self.side {
            Side::Buy => self.bar.low,
            Side::Sell => self.bar.high,
        }
    }

    /// [`favours`] for this bar's side, which every rule below reads the bar through.
    fn favours(&self, subject: Price, level: Price) -> bool {
        favours(self.side, subject, level)
    }

    /// The bar's extreme passed `level` strictly, so it traded there: a touch is not a fill (rule 5).
    fn through(&self, level: Price) -> bool {
        self.favours(self.favourable_extreme(), level)
    }

    /// The open is already past `level`, which in continuous trading would have printed through a
    /// resting order (rule 5).
    fn gapped(&self, level: Price) -> bool {
        self.favours(self.bar.open, level)
    }

    /// The open is at or within `limit`, so the order can take the open (rule 5), and the open
    /// reaches a take-profit leg (rule 8).
    fn marketable(&self, limit: Price) -> bool {
        !self.favours(limit, self.bar.open)
    }

    /// The open has already passed `stop`: rule 6's gap branch, and the open reaching a stop leg
    /// (rule 8).
    fn stop_passed(&self, stop: Price) -> bool {
        !self.gapped(stop)
    }

    /// The bar reached `stop` inside itself (rule 6). A stop is reached on a touch, unlike a resting
    /// limit, and a bar whose open has already passed the stop has reached it too, its adverse
    /// extreme lying at or beyond its open.
    fn stop_reached(&self, stop: Price) -> bool {
        let adverse_extreme = match self.side {
            Side::Buy => self.bar.high,
            Side::Sell => self.bar.low,
        };
        !self.favours(adverse_extreme, stop)
    }

    /// Whichever of the two prices this side would rather have: rule 5's `min(limit, open × (1+s))`
    /// and rule 7's `max(L, …)`.
    fn better(&self, one: Price, other: Price) -> Price {
        if self.favours(one, other) { one } else { other }
    }

    /// Whichever it would rather not: rule 7's `min(open, S)`.
    fn worse(&self, one: Price, other: Price) -> Price {
        if self.favours(one, other) { other } else { one }
    }

    /// Which way slippage moves a price for this side, always against the order (rules 4 to 7,
    /// DEC-106 item 2).
    fn against(&self) -> Adverse {
        match self.side {
            Side::Buy => Adverse::Up,
            Side::Sell => Adverse::Down,
        }
    }
}

/// How a fill is priced once the volume cap has settled its quantity, which the `sqrt` impact model
/// needs before it can compute slippage at all (DEC-106 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Basis {
    /// A price moved against the order by the slippage (rules 4 and 6).
    Slipped(Price),
    /// The same, held at `limit` when the slippage would carry it past (rules 5 and 7).
    SlippedWithin { level: Price, limit: Price },
    /// Exactly this price, with no slippage: a resting limit at its limit, or an auction open
    /// (rule 5, DEC-106 item 5).
    Exact(Price),
}

impl Basis {
    /// The fill price, at the 9 places a price holds and not tick-rounded (rules 4 to 7 and 9).
    fn priced(self, seen: &Seen<'_>, slippage: Bps) -> Result<Price, SimError> {
        match self {
            Self::Slipped(level) => Ok(level.slipped(slippage, seen.against())?),
            Self::SlippedWithin { level, limit } => {
                Ok(seen.better(level.slipped(slippage, seen.against())?, limit))
            }
            Self::Exact(level) => Ok(level),
        }
    }
}

/// What one leg would do on one bar, before the volume cap is applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Action {
    basis: Basis,
    liquidity: Option<Liquidity>,
}

impl Action {
    /// A fill that took liquidity: a market order, a triggered stop or stop-limit, or a marketable
    /// limit (DEC-106 item 6).
    fn taker(basis: Basis) -> Self {
        Self {
            basis,
            liquidity: Some(Liquidity::Taker),
        }
    }

    /// A resting limit filling at its own price, which made liquidity (DEC-106 item 6).
    fn maker(level: Price) -> Self {
        Self {
            basis: Basis::Exact(level),
            liquidity: Some(Liquidity::Maker),
        }
    }

    /// An auction fill, at the open and neither maker nor taker (DEC-106 item 5).
    fn at_auction(open: Price) -> Self {
        Self {
            basis: Basis::Exact(open),
            liquidity: None,
        }
    }
}

/// A leg's state between bars (spec §6.4 rules 5 to 7, DEC-106 items 7 and 10).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Submitted and not yet classified: a limit order still faces the marketable-on-arrival test,
    /// and a stop has not triggered.
    Fresh,
    /// Marketable on arrival at this limit, taking the slipped open while the open stays within it.
    Marketable(Price),
    /// Resting at this price, filling there when a bar passes it strictly.
    Resting(Price),
    /// A market order for whatever remains: rule 4, and a stop that has triggered (DEC-106
    /// item 10).
    Working,
}

/// The single-leg orders the model actually simulates: an OCO is two of them (spec §6.4 rule 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LegKind {
    Market,
    Limit { limit: Price },
    Stop { stop: Price },
    StopLimit { stop: Price, limit: Price },
}

/// One leg of a submitted order: what it is, which OCO leg it reports as, and where it stands.
struct Leg {
    reports_as: Option<OcoLeg>,
    kind: LegKind,
    phase: Phase,
}

impl Leg {
    /// The only leg of an order, which reports no OCO leg.
    fn only(kind: LegKind, phase: Phase) -> Self {
        Self {
            reports_as: None,
            kind,
            phase,
        }
    }

    /// One leg of a protective pair, which reports which leg it is (spec §6.4 rule 8).
    fn of_pair(reports_as: OcoLeg, kind: LegKind, phase: Phase) -> Self {
        Self {
            reports_as: Some(reports_as),
            kind,
            phase,
        }
    }
}

/// What `leg` does on this bar, and the phase it leaves behind: a trigger or a change of phase
/// happens on the bar that causes it, even when the cap lets nothing fill there (DEC-106 item 10).
///
/// Spec §6.4 rule 6 holds an equity stop to the regular session, and DEC-106 item 9 lets a
/// continuously traded instrument's stop trigger on any of its bars. Neither needs a test here: the
/// caller only reaches this on a bar the order may trade (rule 2), and only a limit order may carry
/// `extended_hours` (spec §5.2, DEC-108 item 7), so a stop order's tradable bars are exactly its
/// triggerable ones — the regular session for an equity, the continuous session for crypto
/// (DEC-114 item 2).
fn leg_action(leg: &mut Leg, seen: &Seen<'_>) -> Option<Action> {
    match leg.phase {
        Phase::Working => Some(Action::taker(Basis::Slipped(seen.bar.open))),
        Phase::Marketable(limit) => {
            if seen.marketable(limit) {
                Some(Action::taker(Basis::SlippedWithin {
                    level: seen.bar.open,
                    limit,
                }))
            } else {
                leg.phase = Phase::Resting(limit);
                resting_action(seen, limit)
            }
        }
        Phase::Resting(level) => resting_action(seen, level),
        Phase::Fresh => arriving_action(leg, seen),
    }
}

/// What a leg does on the first bar it meets: a limit takes spec §6.4 rule 5's
/// marketable-on-arrival test, and a stop or a stop-limit triggers or waits (rules 4, 6, and 7).
fn arriving_action(leg: &mut Leg, seen: &Seen<'_>) -> Option<Action> {
    match leg.kind {
        LegKind::Market => {
            leg.phase = Phase::Working;
            Some(Action::taker(Basis::Slipped(seen.bar.open)))
        }
        LegKind::Limit { limit } => {
            leg.phase = if seen.marketable(limit) {
                Phase::Marketable(limit)
            } else {
                Phase::Resting(limit)
            };
            leg_action(leg, seen)
        }
        LegKind::Stop { stop } => {
            if !seen.stop_reached(stop) {
                return None;
            }
            leg.phase = Phase::Working;
            let triggered = if seen.stop_passed(stop) {
                seen.bar.open
            } else {
                stop
            };
            Some(Action::taker(Basis::Slipped(triggered)))
        }
        LegKind::StopLimit { stop, limit } => {
            if !seen.stop_reached(stop) {
                return None;
            }
            leg.phase = Phase::Resting(limit);
            if !seen.marketable(limit) {
                return None;
            }
            Some(Action::taker(Basis::SlippedWithin {
                level: seen.worse(seen.bar.open, stop),
                limit,
            }))
        }
    }
}

/// A resting order on this bar (spec §6.4 rule 5): the auction exception first, where the open gaps
/// through the level and fills there with no slippage and neither liquidity flag (DEC-106 item 5);
/// otherwise a strict pass of the level fills at it as maker, and a touch is not a fill.
fn resting_action(seen: &Seen<'_>, level: Price) -> Option<Action> {
    if seen.bar.auction && seen.gapped(level) {
        Some(Action::at_auction(seen.bar.open))
    } else if seen.through(level) {
        Some(Action::maker(level))
    } else {
        None
    }
}

/// Spec §6.4 rule 8: the open reaching a leg fills that leg under its own rule; otherwise, with both
/// legs reachable inside the bar, the stop fills first, adverse first.
fn pick_leg(
    candidates: &[(Option<OcoLeg>, Action)],
    limit: Price,
    stop: Price,
    seen: &Seen<'_>,
) -> Option<(Option<OcoLeg>, Action)> {
    let of = |wanted: OcoLeg| {
        candidates
            .iter()
            .find(|(leg, _)| *leg == Some(wanted))
            .copied()
    };
    if seen.marketable(limit)
        && let Some(found) = of(OcoLeg::Limit)
    {
        return Some(found);
    }
    if seen.stop_passed(stop)
        && let Some(found) = of(OcoLeg::Stop)
    {
        return Some(found);
    }
    of(OcoLeg::Stop).or_else(|| of(OcoLeg::Limit))
}

/// `s = half-spread + impact` for a fill of `filled` against the bar's reference volume (spec §6.4,
/// DEC-106 item 3). The `sqrt` model needs the quantity, which the cap has already settled, so the
/// rule is not circular.
fn slippage_of(config: &SimConfig, filled: Qty, reference: Qty) -> Result<Bps, SimError> {
    let (half_spread, impact) = match config.slippage {
        Slippage::Fixed {
            half_spread_bps,
            impact_bps,
        } => (half_spread_bps, impact_bps),
        Slippage::Sqrt {
            half_spread_bps,
            coefficient_bps,
        } => (
            half_spread_bps,
            Bps::sqrt_impact(coefficient_bps, filled, reference)?,
        ),
    };
    Ok(half_spread.checked_add(impact)?)
}

/// Whether `order` may trade in this bar's session (spec §4.3, §5.2, §6.4 rule 2, DEC-30): the
/// regular session always, the extended sessions only for an order marked for them, a continuously
/// traded instrument's session always, and the overnight session never.
fn tradable(order: &SimOrder, bar: &SimBar) -> bool {
    match bar.session {
        Session::Regular | Session::Continuous => true,
        Session::PreMarket | Session::AfterHours => order.extended_hours,
        Session::Overnight => false,
    }
}

/// What one order did on one bar: at most one fill (spec §6.4 rule 3 caps a bar), and the OCO leg
/// that fill cost it (rule 8).
struct Step {
    fill: Option<SimFill>,
    canceled: Vec<CanceledLeg>,
}

/// One submitted order as the walk carries it from bar to bar.
struct Working<'o> {
    order: &'o SimOrder,
    remaining: Qty,
    first: Option<usize>,
    expiry: Option<usize>,
    legs: Vec<Leg>,
    protective: Option<(Price, Price)>,
}

impl<'o> Working<'o> {
    /// The order as it arrives: its first eligible bar (spec §6.4 rule 1), the bar a day order's
    /// remainder is canceled after (rule 2), and the phase each of its legs starts in. An order
    /// already resting never takes the marketable-on-arrival test (DEC-108 item 5), and neither leg
    /// of a protective pair ever does (DEC-108 item 8).
    fn arriving(
        order: &'o SimOrder,
        bars: &[SimBar],
        config: &SimConfig,
    ) -> Result<Self, SimError> {
        let first = first_bar(order, bars, config)?;
        let expiry = match (order.tif, first) {
            (TimeInForce::Day, Some(from)) => expiry_bar(order, bars, from),
            _ => None,
        };
        let on_arrival = |limit: Price| match order.eligible_from {
            Eligibility::DecidedAt { .. } => Phase::Fresh,
            Eligibility::Resting { .. } => Phase::Resting(limit),
        };
        let (legs, protective) = match order.kind {
            OrderKind::Oco { limit, stop } => (
                vec![
                    Leg::of_pair(
                        OcoLeg::Limit,
                        LegKind::Limit { limit },
                        Phase::Resting(limit),
                    ),
                    Leg::of_pair(OcoLeg::Stop, LegKind::Stop { stop }, Phase::Fresh),
                ],
                Some((limit, stop)),
            ),
            OrderKind::Limit { limit } => (
                vec![Leg::only(LegKind::Limit { limit }, on_arrival(limit))],
                None,
            ),
            OrderKind::Market => (vec![Leg::only(LegKind::Market, Phase::Fresh)], None),
            OrderKind::Stop { stop } => {
                (vec![Leg::only(LegKind::Stop { stop }, Phase::Fresh)], None)
            }
            OrderKind::StopLimit { stop, limit } => (
                vec![Leg::only(LegKind::StopLimit { stop, limit }, Phase::Fresh)],
                None,
            ),
        };
        Ok(Self {
            order,
            remaining: order.qty,
            first,
            expiry,
            legs,
            protective,
        })
    }

    /// What this order does on bar `index`: its legs' phases advance whether or not the cap allows a
    /// fill (DEC-106 item 10), and the fill it takes consumes the bar's shared cap, which earlier
    /// orders have already drawn on (spec §6.4 rule 3).
    fn on_bar(
        &mut self,
        reference: OrderRef,
        index: usize,
        bar: &SimBar,
        config: &SimConfig,
        room: &mut Room,
    ) -> Result<Step, SimError> {
        let nothing = Step {
            fill: None,
            canceled: Vec::new(),
        };
        if self.remaining.is_zero() || !self.reaches(index, bar) {
            return Ok(nothing);
        }
        let seen = Seen {
            bar,
            side: self.order.side,
        };
        let candidates: Vec<(Option<OcoLeg>, Action)> = self
            .legs
            .iter_mut()
            .filter_map(|leg| leg_action(leg, &seen).map(|a| (leg.reports_as, a)))
            .collect();
        let Some((leg, action)) = self.choose(&candidates, &seen) else {
            return Ok(nothing);
        };
        let filled = self.remaining.min(room.left);
        if filled.is_zero() {
            return Ok(nothing);
        }
        room.left = room.left.checked_sub(filled)?;
        self.remaining = self.remaining.checked_sub(filled)?;
        let price = action
            .basis
            .priced(&seen, slippage_of(config, filled, room.reference)?)?;
        Ok(Step {
            fill: Some(SimFill {
                order: reference,
                leg,
                bar: index,
                qty: filled,
                price,
                liquidity: action.liquidity,
            }),
            canceled: match leg {
                Some(filled_leg) => self.cancel_the_other_leg(reference, filled_leg, index),
                None => Vec::new(),
            },
        })
    }

    /// Whether bar `index` can fill this order: it is at or after the first eligible bar (spec §6.4
    /// rule 1), the order may trade the bar's session, and a day order's last eligible session has
    /// not ended (rule 2).
    fn reaches(&self, index: usize, bar: &SimBar) -> bool {
        self.first.is_some_and(|first| index >= first)
            && !self.expiry.is_some_and(|last| index > last)
            && tradable(self.order, bar)
    }

    /// Which of the legs that would act on this bar actually fills: a protective pair follows spec
    /// §6.4 rule 8, and every other order has one leg.
    fn choose(
        &self,
        candidates: &[(Option<OcoLeg>, Action)],
        seen: &Seen<'_>,
    ) -> Option<(Option<OcoLeg>, Action)> {
        match self.protective {
            Some((limit, stop)) => pick_leg(candidates, limit, stop, seen),
            None => candidates.first().copied(),
        }
    }

    /// Spec §6.4 rule 8 and DEC-106 item 8: the first fill of one leg, partial or whole, cancels the
    /// other, which is then gone for good. A stop leg's remainder keeps working as the market order
    /// its `Working` phase already makes it (item 10).
    fn cancel_the_other_leg(
        &mut self,
        order: OrderRef,
        filled: OcoLeg,
        bar: usize,
    ) -> Vec<CanceledLeg> {
        let lost: Vec<CanceledLeg> = self
            .legs
            .iter()
            .filter_map(|leg| leg.reports_as)
            .filter(|leg| *leg != filled)
            .map(|leg| CanceledLeg { order, leg, bar })
            .collect();
        self.legs.retain(|leg| leg.reports_as == Some(filled));
        lost
    }

    /// How the order stood when the bars ran out: a day order that did not fill lost its remainder
    /// at the end of its last eligible session (spec §6.4 rule 2).
    fn end(&self) -> OrderEnd {
        match (self.remaining.is_zero(), self.expiry) {
            (true, _) => OrderEnd::Filled,
            (false, Some(last)) => OrderEnd::Expired { at_bar: last },
            (false, None) => OrderEnd::Open,
        }
    }
}

/// Spec §6.4 rules 1 and 2: the first bar that may fill `order` — the first bar starting at or after
/// the decision plus the configured latencies, or the bar it was already resting from, and then the
/// first bar from there in a session the order may trade.
fn first_bar(
    order: &SimOrder,
    bars: &[SimBar],
    config: &SimConfig,
) -> Result<Option<usize>, SimError> {
    let from = match order.eligible_from {
        Eligibility::DecidedAt {
            at,
            approval_required,
        } => {
            let decided = config.decision_latency.after(at)?;
            let ready = if approval_required {
                config.approval_latency.after(decided)?
            } else {
                decided
            };
            bars.iter().position(|bar| bar.start >= ready)
        }
        Eligibility::Resting { from_bar } => Some(from_bar),
    };
    Ok(from.and_then(|start| {
        (start..bars.len()).find(|index| bars.get(*index).is_some_and(|bar| tradable(order, bar)))
    }))
}

/// Spec §6.4 rule 2: the last bar of the last session a day order may trade on the trading day it
/// became eligible in. Its remainder is canceled after that bar.
fn expiry_bar(order: &SimOrder, bars: &[SimBar], first: usize) -> Option<usize> {
    let day = bars.get(first)?.trade_date;
    (first..bars.len()).rev().find(|index| {
        bars.get(*index)
            .is_some_and(|bar| bar.trade_date == day && tradable(order, bar))
    })
}
