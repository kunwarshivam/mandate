//! Mandate spec §5.5's `trim_to_target` (DEC-56, DEC-65): once a `trim_to_target` rung has been
//! active for `breach_confirm_s`, a position whose market value exceeds `factor × cap` by at least
//! `rebalance_band × cap` is sold down to `factor × cap` as a risk exit, the quantity rounded **up**
//! to the increment, if the sell meets the instrument's minimum, for an equity only in the regular
//! session, and never while the goal is `Holding`.
//!
//! The figures are exact, as everywhere in the gate (§5.2): `cap` is check 2's
//! `min(max_position_usd, max_position_fraction × E)`, the target and the sell stay on
//! [`UsdExact`] until the one rounding up to the increment (DEC-399).

use std::collections::BTreeMap;

use mandate_num::{Qty, UsdExact};
use mandate_time::UtcNanos;

use crate::spec_types::{GoalState, RiskLimits, ScaleAction};
use crate::{
    AgentSnapshot, AssetClass, AssetId, GateConfig, GateError, InstrumentSnapshot, Purpose,
    RiskSnapshot, Session, TrimProposal, ValidatedMandate, session,
};

/// One trim per position far enough above its target, in instrument order (DEC-399 item 1).
pub(crate) fn proposals(
    now: UtcNanos,
    config: &GateConfig,
    mandate: &ValidatedMandate,
    risk: &RiskSnapshot,
    agent: &AgentSnapshot,
    instruments: &BTreeMap<AssetId, InstrumentSnapshot>,
) -> Result<Vec<TrimProposal>, GateError> {
    let limits = mandate.risk();
    if mandate.goal_state() == GoalState::Holding || !a_trimming_rung_is_confirmed(limits, risk) {
        return Ok(Vec::new());
    }
    let cap = limits.max_position_usd.min(
        risk.agent_equity
            .times_fraction(limits.max_position_fraction)?,
    );
    let target = UsdExact::of(cap).checked_mul(UsdExact::of_ratio(risk.size_factor))?;
    let band = UsdExact::of(cap.times_fraction(limits.rebalance_band)?);
    let mut trims = Vec::new();
    for (id, value) in &agent.market_values {
        let market_value = UsdExact::of(*value);
        let excess = market_value.checked_sub(target)?;
        if !market_value.is_positive()? || excess.is_below(band)? {
            continue;
        }
        let instrument = instruments.get(id).ok_or(GateError::InstrumentUnknown)?;
        if instrument.asset_class == AssetClass::UsEquity
            && session::derive(now, config, instrument.asset_class)?.session != Session::Regular
        {
            continue;
        }
        let held = agent.positions.get(id).copied().unwrap_or(Qty::ZERO);
        let qty = excess
            .checked_mul(UsdExact::of_qty(held))?
            .ceiled_quotient(market_value, increment(instrument)?)?
            .min(held);
        if qty.is_zero() || qty < instrument.min_order_size {
            continue;
        }
        trims.push(TrimProposal {
            instrument: id.clone(),
            qty,
            purpose: Purpose::RiskExit,
        });
    }
    Ok(trims)
}

/// §5.5: "only once the rung has been active for `breach_confirm_s`", read as a `trim_to_target`
/// rung active for at least that long, the boundary allowing like every limit here (DEC-129 item
/// 15). A `limit_buys` rung scales the order builder's targets and never trims.
fn a_trimming_rung_is_confirmed(limits: &RiskLimits, risk: &RiskSnapshot) -> bool {
    limits.drawdown_ladder.iter().any(|rung| {
        rung.scale_action == Some(ScaleAction::TrimToTarget)
            && risk
                .active_rungs
                .get(&rung.index)
                .is_some_and(|active_s| *active_s >= u64::from(limits.breach_confirm_s))
    })
}

/// The instrument's quantity grid: whole shares unless it is fractionable, as the participation
/// slice reads it (trading-domain spec §2.1).
fn increment(instrument: &InstrumentSnapshot) -> Result<Qty, GateError> {
    let grid = if instrument.fractionable {
        "0.000000001"
    } else {
        "1"
    };
    Ok(Qty::parse(grid)?)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_num::{Fraction, Ratio, Usd};

    use super::*;
    use crate::spec_types::{Rung, RungAction};
    use crate::{AgentId, AgentMode, DayTradeLedger, EtpClass, Exchange, QuoteCurrency};

    const HELD: &str = "held";

    /// `two_stock_swing`'s limits with one `trim_to_target` rung at 0.5, confirmed: the cap is
    /// min(1500, 0.2 × 10000) = 1500, the target 750 and the band 0.05 × 1500 = 75.
    struct Scene {
        now: UtcNanos,
        limits: RiskLimits,
        risk: RiskSnapshot,
        agent: AgentSnapshot,
        instrument: InstrumentSnapshot,
    }

    impl Scene {
        fn new(held: &str, market_value: &str) -> Result<Self, GateError> {
            let id = AssetId::new(HELD).map_err(|_| GateError::InstrumentUnknown)?;
            Ok(Self {
                now: UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?,
                limits: RiskLimits {
                    max_position_usd: Usd::parse("1500")?,
                    max_position_fraction: Fraction::parse("0.2")?,
                    max_order_usd: Usd::parse("1000")?,
                    max_gross_exposure_usd: Usd::parse("2000")?,
                    max_orders_per_day: 50,
                    reentry_cooldown_s: 3600,
                    rebalance_band: Fraction::parse("0.05")?,
                    breach_confirm_s: 60,
                    drawdown_ladder: vec![Rung {
                        index: 0,
                        at: Fraction::parse("0.03")?,
                        action: RungAction::ScaleSizes,
                        factor: Some(Fraction::parse("0.5")?),
                        scale_action: Some(ScaleAction::TrimToTarget),
                    }],
                },
                risk: RiskSnapshot {
                    agent_equity: Usd::parse("10000")?,
                    high_water_mark: Usd::parse("10500")?,
                    day_start_equity: Usd::parse("10000")?,
                    capital_base: Usd::parse("10000")?,
                    inherited_loss: Usd::ZERO,
                    latched: BTreeSet::new(),
                    active_rungs: BTreeMap::from([(0, 60)]),
                    size_factor: Ratio::parse("0.5")?,
                    agent_mode: AgentMode::Normal,
                },
                agent: AgentSnapshot {
                    agent: AgentId(1),
                    mode: AgentMode::Normal,
                    instrument_restrictions: BTreeMap::new(),
                    positions: BTreeMap::from([(id.clone(), Qty::parse(held)?)]),
                    market_values: BTreeMap::from([(id.clone(), Usd::parse(market_value)?)]),
                    working_orders: BTreeSet::new(),
                    instrument_groups: BTreeMap::new(),
                    last_exit_fill_at: BTreeMap::new(),
                    orders_today: 0,
                    day_trades: DayTradeLedger::default(),
                },
                instrument: InstrumentSnapshot {
                    instrument: id,
                    asset_class: AssetClass::UsEquity,
                    exchange: Some(Exchange::Nasdaq),
                    status_active: true,
                    tradable: true,
                    fractionable: false,
                    ipo: false,
                    ptp_no_exception: false,
                    etp: EtpClass::Plain,
                    etp_classified_at: None,
                    quote_currency: Some(QuoteCurrency::Usd),
                    prior_close: None,
                    median_dollar_volume_20d: None,
                    median_dollar_volume_30d: None,
                    min_order_size: Qty::parse("1")?,
                    halted: false,
                    status_feed_current: true,
                },
            })
        }

        fn crypto(mut self) -> Self {
            self.instrument.asset_class = AssetClass::Crypto;
            self.instrument.exchange = None;
            self.instrument.fractionable = true;
            self
        }

        fn trims(&self) -> Result<Vec<Qty>, GateError> {
            let config = GateConfig {
                price_floor: Usd::parse("5")?,
                liquidity_floor_usd: Usd::parse("1000000")?,
                crypto_liquidity_floor_usd: Usd::parse("1000000")?,
                collar_liquid_threshold_usd: Usd::parse("50000000")?,
                collar_liquid_x: Fraction::parse("0.01")?,
                collar_other_x: Fraction::parse("0.02")?,
                collar_crypto_x: Fraction::parse("0.02")?,
                collar_passive_band: Fraction::parse("0.2")?,
                opposite_fill_interval_s: 60,
                min_resting_time_s: 2,
                order_to_fill_max: 10,
                order_to_fill_min_orders: 20,
                order_size_participation: Fraction::parse("0.05")?,
                daily_participation: Fraction::parse("0.05")?,
                close_window_minutes: 10,
                legacy_pdt_equity_threshold: Usd::parse("25000")?,
                etp_classification_max_age_s: 604_800,
            };
            let mandate = ValidatedMandate::from_validated_parts(
                self.limits.clone(),
                GoalState::Running,
                false,
                false,
            );
            let instruments =
                BTreeMap::from([(self.instrument.instrument.clone(), self.instrument.clone())]);
            let trims = proposals(
                self.now,
                &config,
                &mandate,
                &self.risk,
                &self.agent,
                &instruments,
            )?;
            for trim in &trims {
                assert_eq!(
                    (trim.instrument.as_str(), trim.purpose),
                    (HELD, Purpose::RiskExit),
                    "a trim sells the held position as a risk exit"
                );
            }
            Ok(trims.into_iter().map(|trim| trim.qty).collect())
        }
    }

    fn qty(text: &str) -> Result<Vec<Qty>, GateError> {
        Ok(vec![Qty::parse(text)?])
    }

    /// §5.5's "≥ `rebalance_band` × cap": 75 over the 750 target is exactly the band and trims one
    /// share of eleven at 75; a cent less is inside the band.
    #[test]
    fn an_excess_of_exactly_the_band_trims() -> Result<(), GateError> {
        assert_eq!(Scene::new("11", "825")?.trims()?, qty("1")?);
        assert_eq!(Scene::new("11", "824.99")?.trims()?, Vec::new());
        Ok(())
    }

    /// A crypto trim runs outside the regular session, on its own fractional grid: 250 over a 1000
    /// position of 10 is 2.5 units exactly, where the whole-share grid would sell 3.
    #[test]
    fn a_crypto_trim_runs_after_hours_on_a_fractional_grid() -> Result<(), GateError> {
        let mut equity = Scene::new("10", "1000")?;
        equity.now = UtcNanos::parse_rfc3339("2026-09-22T21:00:00Z")?;
        assert_eq!(
            equity.trims()?,
            Vec::new(),
            "an equity waits for the session"
        );
        let mut crypto = Scene::new("10", "1000")?.crypto();
        crypto.now = UtcNanos::parse_rfc3339("2026-09-22T21:00:00Z")?;
        assert_eq!(crypto.trims()?, qty("2.5")?);
        Ok(())
    }

    /// "Only if the order meets the minimum": a 3-share trim is proposed at a minimum of 3 and not
    /// at 4.
    #[test]
    fn a_trim_below_the_minimum_is_not_proposed() -> Result<(), GateError> {
        let mut scene = Scene::new("10", "1000")?;
        scene.instrument.min_order_size = Qty::parse("3")?;
        assert_eq!(scene.trims()?, qty("3")?);
        scene.instrument.min_order_size = Qty::parse("4")?;
        assert_eq!(scene.trims()?, Vec::new());
        Ok(())
    }

    /// Rounding up never sells more than is held: at a factor of 0 the whole 2.5 goes, not the 3
    /// the whole-share grid rounds 2.5 up to.
    #[test]
    fn a_trim_never_sells_more_than_the_position() -> Result<(), GateError> {
        let mut scene = Scene::new("2.5", "250")?;
        scene.risk.size_factor = Ratio::ZERO;
        assert_eq!(scene.trims()?, qty("2.5")?);
        Ok(())
    }

    /// A market value with no quantity sells nothing, even at a minimum of zero; one of zero is
    /// never divided by, even with a zero target and band.
    #[test]
    fn a_position_with_nothing_to_sell_is_not_trimmed() -> Result<(), GateError> {
        let mut unheld = Scene::new("10", "1000")?;
        unheld.agent.positions.clear();
        unheld.instrument.min_order_size = Qty::ZERO;
        assert_eq!(unheld.trims()?, Vec::new());
        let mut worthless = Scene::new("10", "0")?;
        worthless.risk.size_factor = Ratio::ZERO;
        worthless.limits.rebalance_band = Fraction::ZERO;
        assert_eq!(worthless.trims()?, Vec::new());
        Ok(())
    }

    /// A position over its target with no instrument snapshot cannot be sized or timed, so the
    /// trims are refused rather than guessed; one inside the band never needs the snapshot.
    #[test]
    fn a_trim_without_its_instrument_is_refused() -> Result<(), GateError> {
        let mut over = Scene::new("10", "1000")?;
        over.instrument.instrument =
            AssetId::new("other").map_err(|_| GateError::InstrumentUnknown)?;
        assert!(matches!(over.trims(), Err(GateError::InstrumentUnknown)));
        let mut inside = Scene::new("7", "700")?;
        inside.instrument.instrument = over.instrument.instrument;
        assert_eq!(inside.trims()?, Vec::new());
        Ok(())
    }

    /// The rung that confirms must be the trimming one: a confirmed `limit_buys` rung beside an
    /// unconfirmed `trim_to_target` rung trims nothing until the latter has held.
    #[test]
    fn only_a_confirmed_trimming_rung_trims() -> Result<(), GateError> {
        let mut scene = Scene::new("10", "1000")?;
        let mut buys = scene
            .limits
            .drawdown_ladder
            .first()
            .cloned()
            .ok_or(GateError::ConfigOutOfRange)?;
        buys.scale_action = Some(ScaleAction::LimitBuys);
        for rung in &mut scene.limits.drawdown_ladder {
            rung.index = 1;
        }
        scene.limits.drawdown_ladder.insert(0, buys);
        scene.risk.active_rungs = BTreeMap::from([(0, 600), (1, 59)]);
        assert_eq!(scene.trims()?, Vec::new());
        scene.risk.active_rungs = BTreeMap::from([(0, 0), (1, 60)]);
        assert_eq!(scene.trims()?, qty("3")?);
        Ok(())
    }
}
