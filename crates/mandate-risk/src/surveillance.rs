//! §9.6's daily surveillance report: a pure function of the day's folded figures that states them
//! and flags each threshold crossed. It makes no judgement and supervises nothing, because "the
//! platform does not supervise users' trading"; routing the report to the owner and journaling the
//! acknowledgment are the runtime's (DEC-129 item 13).

use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{Fraction, NumError, Rounding, Usd};
use mandate_time::Date;

use crate::{
    AssetId, GateConfig, GateError, OrderCounts, SurveillanceBreach, SurveillanceInput,
    SurveillanceReport,
};

/// The places a concentration figure is stated to: a [`Fraction`] holds 9.
const CONCENTRATION_PLACES: u32 = 9;

/// The report for `day` (DEC-163 item 8):
///
/// - **Order-to-fill:** per agent and instrument, `orders ÷ max(fills, 1)` truncated, where orders
///   are the submitted ones less the exit-sequence and kill-switch cancels §9.6 excludes; flagged
///   when at least `order_to_fill_min_orders` were sent and the ratio exceeds `order_to_fill_max`,
///   compared without dividing, exactly as the gate's own check 6 compares it.
/// - **Self-trade:** every instrument the fold saw resting on both sides across related accounts,
///   each flagged.
/// - **Close-window activity:** the counts as folded; any order there is flagged for the owner to
///   review, since §9.6 names no count that would be normal.
/// - **Concentration:** each end-of-day market value as a fraction of its agent's equity, rounded
///   up so it is never understated, and flagged where it cannot be one: a position worth more than
///   the agent's equity, or an agent with no positive equity recorded, stated as `1`.
///
/// # Errors
/// `negative` when more cancels are excluded than orders were submitted or a market value is
/// negative, both of which a long-only fold cannot produce; the arithmetic's own errors otherwise.
pub(crate) fn report(
    day: Date,
    config: &GateConfig,
    input: &SurveillanceInput,
) -> Result<SurveillanceReport, GateError> {
    let mut breaches = BTreeSet::new();
    let mut order_to_fill = BTreeMap::new();
    for (key, counts) in &input.orders {
        let (ratio, breached) = order_to_fill_of(config, *counts)?;
        order_to_fill.insert(key.clone(), ratio);
        if breached {
            breaches.insert(SurveillanceBreach::OrderToFill);
        }
    }
    let self_trade_instruments: BTreeSet<AssetId> = input
        .opposite_side_rests
        .iter()
        .filter(|(_, agents)| !agents.is_empty())
        .map(|(instrument, _)| instrument.clone())
        .collect();
    if !self_trade_instruments.is_empty() {
        breaches.insert(SurveillanceBreach::SelfTrade);
    }
    if input.close_window_orders.values().any(|count| *count > 0) {
        breaches.insert(SurveillanceBreach::CloseWindow);
    }
    let mut concentration = BTreeMap::new();
    for ((agent, instrument), value) in &input.end_of_day_market_values {
        let (share, breached) = share_of_equity(input.agent_equity.get(agent), *value)?;
        concentration.insert((*agent, instrument.clone()), share);
        if breached {
            breaches.insert(SurveillanceBreach::Concentration);
        }
    }
    Ok(SurveillanceReport {
        day,
        order_to_fill,
        self_trade_instruments,
        close_window_orders: input.close_window_orders.clone(),
        concentration,
        breaches,
    })
}

fn order_to_fill_of(config: &GateConfig, counts: OrderCounts) -> Result<(u32, bool), GateError> {
    let orders = counts
        .submitted
        .checked_sub(counts.cancels_excluded)
        .ok_or(NumError::Negative)?;
    let fills = counts.filled.max(1);
    let ratio = orders.checked_div(fills).ok_or(NumError::DivisionByZero)?;
    let breached = orders >= config.order_to_fill_min_orders
        && u64::from(orders) > u64::from(config.order_to_fill_max).saturating_mul(u64::from(fills));
    Ok((ratio, breached))
}

/// `value ÷ equity`, rounded up at 9 places, and whether it crossed the threshold of being no
/// fraction of equity at all.
///
/// `mandate-num` has no conversion from a [`mandate_num::Ratio`] to a [`Fraction`], so the quotient
/// goes through its canonical text, which is the exact value and which [`Fraction::parse`] reads
/// back exactly (backlog: the conversion belongs in `mandate-num`).
fn share_of_equity(equity: Option<&Usd>, value: Usd) -> Result<(Fraction, bool), GateError> {
    let Some(equity) = equity.filter(|e| !e.is_negative() && !e.is_zero()) else {
        return Ok((Fraction::ONE, true));
    };
    let ratio = value.ratio_to(*equity, CONCENTRATION_PLACES, Rounding::Ceiling)?;
    match Fraction::parse(&ratio.to_string()) {
        Ok(share) => Ok((share, false)),
        Err(NumError::AboveOne) => Ok((Fraction::ONE, true)),
        Err(other) => Err(other.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AgentId;

    fn config() -> Result<GateConfig, GateError> {
        let usd = Usd::parse;
        let fraction = Fraction::parse;
        Ok(GateConfig {
            price_floor: usd("5")?,
            liquidity_floor_usd: usd("1000000")?,
            crypto_liquidity_floor_usd: usd("2000000")?,
            collar_liquid_threshold_usd: usd("50000000")?,
            collar_liquid_x: fraction("0.01")?,
            collar_other_x: fraction("0.02")?,
            collar_crypto_x: fraction("0.02")?,
            collar_passive_band: fraction("0.2")?,
            opposite_fill_interval_s: 60,
            min_resting_time_s: 2,
            order_to_fill_max: 10,
            order_to_fill_min_orders: 20,
            order_size_participation: fraction("0.05")?,
            daily_participation: fraction("0.05")?,
            close_window_minutes: 10,
            legacy_pdt_equity_threshold: usd("25000")?,
            etp_classification_max_age_s: 604_800,
        })
    }

    fn id(text: &str) -> Result<AssetId, GateError> {
        AssetId::new(text).map_err(|_| GateError::InstrumentUnknown)
    }

    fn day() -> Result<Date, GateError> {
        Ok(Date::parse("2026-09-21")?)
    }

    /// Excluded cancels come off the submitted count before the ratio: 30 submitted with 10 of
    /// them excluded is 20 orders, 20 ÷ 2 = 10, at the limit and not above it; with 9 excluded it
    /// is 21 orders, above 10 × 2. Excluding more than were submitted is an error, never a zero.
    #[test]
    fn excluded_cancels_come_off_the_order_count() -> Result<(), GateError> {
        let counts = |cancels_excluded| OrderCounts {
            submitted: 30,
            filled: 2,
            cancels_excluded,
        };
        let config = config()?;
        assert_eq!(
            (
                order_to_fill_of(&config, counts(10))?,
                order_to_fill_of(&config, counts(9))?,
            ),
            ((10, false), (10, true)),
            "20 orders to 2 fills is exactly the limit; 21 is over it and still truncates to 10"
        );
        assert!(
            matches!(
                order_to_fill_of(&config, counts(31)),
                Err(GateError::Num(NumError::Negative))
            ),
            "31 cancels excluded from 30 orders is an inconsistent fold"
        );
        Ok(())
    }

    /// The self-trade and close-window figures are stated as folded and flagged when present;
    /// an instrument with no agents listed and a zero count flag nothing.
    #[test]
    fn self_trade_and_close_window_activity_are_flagged_when_present() -> Result<(), GateError> {
        let mut quiet = SurveillanceInput::default();
        quiet.opposite_side_rests.insert(id("a")?, BTreeSet::new());
        quiet.close_window_orders.insert((AgentId(1), id("a")?), 0);
        let mut busy = SurveillanceInput::default();
        busy.opposite_side_rests
            .insert(id("a")?, [AgentId(1), AgentId(2)].into());
        busy.close_window_orders.insert((AgentId(1), id("a")?), 1);
        let (q, b) = (
            report(day()?, &config()?, &quiet)?,
            report(day()?, &config()?, &busy)?,
        );
        assert_eq!(
            (
                q.breaches,
                q.self_trade_instruments,
                b.breaches,
                b.self_trade_instruments,
                b.close_window_orders,
            ),
            (
                BTreeSet::new(),
                BTreeSet::new(),
                [
                    SurveillanceBreach::SelfTrade,
                    SurveillanceBreach::CloseWindow
                ]
                .into(),
                [id("a")?].into(),
                [((AgentId(1), id("a")?), 1)].into(),
            ),
            "an empty rest set and a zero count are nothing to report"
        );
        Ok(())
    }

    /// Concentration is `value ÷ equity` rounded up: 1 ÷ 3 of equity is 0.333333334, a position of
    /// exactly the agent's equity is 1 and unflagged, and a position above it, an agent with zero
    /// equity, or one with no equity recorded is stated as 1 and flagged.
    #[test]
    fn concentration_is_rounded_up_and_flagged_past_equity() -> Result<(), GateError> {
        let usd = Usd::parse;
        let mut input = SurveillanceInput::default();
        let values = [
            (AgentId(1), "1", "3"),
            (AgentId(2), "3", "3"),
            (AgentId(3), "3.000000001", "3"),
            (AgentId(4), "1", "0"),
        ];
        for (agent, value, equity) in values {
            input
                .end_of_day_market_values
                .insert((agent, id("a")?), usd(value)?);
            input.agent_equity.insert(agent, usd(equity)?);
        }
        input
            .end_of_day_market_values
            .insert((AgentId(5), id("a")?), usd("1")?);
        let figures = |i: &SurveillanceInput| -> Result<Vec<(Fraction, bool)>, GateError> {
            (1..=5)
                .map(|n| share_of_equity(i.agent_equity.get(&AgentId(n)), usd_of(i, n)?))
                .collect()
        };
        let one_third = Fraction::parse("0.333333334")?;
        assert_eq!(
            figures(&input)?,
            vec![
                (one_third, false),
                (Fraction::ONE, false),
                (Fraction::ONE, true),
                (Fraction::ONE, true),
                (Fraction::ONE, true),
            ],
            "rounded up, exactly one unflagged, and past equity or without it flagged"
        );
        let whole = report(day()?, &config()?, &input)?;
        assert_eq!(
            (
                whole.concentration.get(&(AgentId(1), id("a")?)),
                whole.breaches
            ),
            (Some(&one_third), [SurveillanceBreach::Concentration].into()),
            "the report states the figure and flags the threshold"
        );
        let mut negative = SurveillanceInput::default();
        negative
            .end_of_day_market_values
            .insert((AgentId(1), id("a")?), usd("-1")?);
        negative.agent_equity.insert(AgentId(1), usd("3")?);
        assert!(
            matches!(
                report(day()?, &config()?, &negative),
                Err(GateError::Num(NumError::Negative))
            ),
            "a negative market value is no long-only position"
        );
        Ok(())
    }

    fn usd_of(input: &SurveillanceInput, agent: u64) -> Result<Usd, GateError> {
        input
            .end_of_day_market_values
            .get(&(AgentId(agent), id("a")?))
            .copied()
            .ok_or(GateError::InstrumentUnknown)
    }
}
