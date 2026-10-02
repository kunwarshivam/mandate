//! Mandate spec §5.5's `trim_to_target` (DEC-56, DEC-65): once a `trim_to_target` rung has been
//! active for `breach_confirm_s`, a position whose market value exceeds `factor × cap` by at least
//! `rebalance_band × cap` is sold down to `factor × cap` as a risk exit, the quantity rounded **up**
//! to the increment, if the sell meets the instrument's minimum, for an equity only in the regular
//! session, and never while the goal is `Holding`. The agent's own open non-protective sells in the
//! instrument are subtracted from the excess first, so a trim already working is never proposed
//! again (DEC-399 item 7).
//!
//! The figures are exact, as everywhere in the gate (§5.2): `cap` is check 2's
//! `min(max_position_usd, max_position_fraction × E)`, the target and the sell stay on
//! [`UsdExact`] until the one rounding up to the increment (DEC-399).

use std::collections::BTreeMap;

use mandate_num::{NumError, Qty, UsdExact};
use mandate_time::UtcNanos;

use crate::limits::position_cap;
use crate::spec_types::{GoalState, RiskLimits, ScaleAction};
use crate::{
    AccountSnapshot, AgentSnapshot, AssetClass, AssetId, GateConfig, GateError, InstrumentSnapshot,
    Purpose, RiskSnapshot, Session, Side, TrimProposal, ValidatedMandate, session,
};

/// One trim per position far enough above its target, in instrument order (DEC-399 item 1).
pub(crate) fn proposals(
    now: UtcNanos,
    config: &GateConfig,
    mandate: &ValidatedMandate,
    risk: &RiskSnapshot,
    agent: &AgentSnapshot,
    account: &AccountSnapshot,
    instruments: &BTreeMap<AssetId, InstrumentSnapshot>,
) -> Result<Vec<TrimProposal>, GateError> {
    let limits = mandate.risk();
    if mandate.goal_state() == GoalState::Holding || !a_trimming_rung_is_confirmed(limits, risk) {
        return Ok(Vec::new());
    }
    let cap = position_cap(limits, risk)?;
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
        let on_sale = open_sells(agent, account, id)?;
        let unsold = match held.checked_sub(on_sale) {
            Ok(unsold) => unsold,
            Err(NumError::Negative) => Qty::ZERO,
            Err(other) => return Err(other.into()),
        };
        let qty = excess
            .checked_mul(UsdExact::of_qty(held))?
            .checked_sub(UsdExact::of_qty(on_sale).checked_mul(market_value)?)?
            .ceiled_quotient(market_value, increment(instrument)?)?
            .min(unsold);
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

/// The agent's own open non-protective sells in the instrument, read as §5.3 rule 4's
/// `sell_available` reads them at the re-run: a trim already working is part of the sell-down, so
/// it comes off the excess before the next trim is sized (DEC-399 item 7). Protective orders are
/// not a trim in progress and are not counted.
fn open_sells(
    agent: &AgentSnapshot,
    account: &AccountSnapshot,
    instrument: &AssetId,
) -> Result<Qty, GateError> {
    let mut on_sale = Qty::ZERO;
    for (_, order) in account.working_orders.iter().filter(|(id, order)| {
        agent.working_orders.contains(id)
            && order.instrument == *instrument
            && order.side == Side::Sell
            && !order.protective
    }) {
        on_sale = on_sale.checked_add(order.open_qty)?;
    }
    Ok(on_sale)
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
    use crate::{
        AccountState, AccountType, AgentId, AgentMode, ClientOrderId, DayTradeLedger,
        DayTradeRegime, EtpClass, Exchange, QuoteCurrency, WorkingOrder,
    };

    const HELD: &str = "held";

    /// `two_stock_swing`'s limits with one `trim_to_target` rung at 0.5, confirmed: the cap is
    /// min(1500, 0.2 × 10000) = 1500, the target 750 and the band 0.05 × 1500 = 75.
    struct Scene {
        now: UtcNanos,
        limits: RiskLimits,
        risk: RiskSnapshot,
        agent: AgentSnapshot,
        account: AccountSnapshot,
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
                account: AccountSnapshot {
                    account_type: AccountType::Margin,
                    state: AccountState::Active,
                    crypto_active: true,
                    regime: DayTradeRegime::IntradayMargin {
                        maintenance_excess: Usd::parse("100000")?,
                    },
                    equity: Usd::parse("10000")?,
                    prior_close_equity: Usd::parse("10000")?,
                    model_buying_power: Usd::parse("1000000")?,
                    broker_buying_power: Usd::parse("1000000")?,
                    broker_non_marginable_buying_power: Usd::parse("1000000")?,
                    positions: BTreeMap::new(),
                    market_values: BTreeMap::new(),
                    working_orders: BTreeMap::new(),
                    unknown_orders: BTreeSet::new(),
                    related_account_resting: BTreeMap::new(),
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

        /// A working order resting in the account, the agent's own when `mine`.
        fn resting(
            &mut self,
            id: u64,
            qty: &str,
            side: Side,
            protective: bool,
            mine: bool,
        ) -> Result<(), GateError> {
            let order = WorkingOrder {
                agent: AgentId(if mine { 1 } else { 2 }),
                instrument: self.instrument.instrument.clone(),
                side,
                max_cost: Usd::ZERO,
                open_qty: Qty::parse(qty)?,
                protective,
                opening: false,
                submitted_on: mandate_time::Date::parse("2026-09-21")?,
            };
            self.account.working_orders.insert(ClientOrderId(id), order);
            if mine {
                self.agent.working_orders.insert(ClientOrderId(id));
            }
            Ok(())
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
                &self.account,
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

    /// A trim resting for the whole quantity this evaluation would ask for proposes nothing more:
    /// 250 over the 750 target at 100 a share is 3 shares, and 3 are already on sale.
    #[test]
    fn a_resting_trim_for_the_full_quantity_proposes_nothing_more() -> Result<(), GateError> {
        let mut scene = Scene::new("10", "1000")?;
        assert_eq!(scene.trims()?, qty("3")?);
        scene.resting(7, "3", Side::Sell, false, true)?;
        assert_eq!(scene.trims()?, Vec::new());
        scene.resting(8, "9", Side::Sell, false, true)?;
        assert_eq!(
            scene.trims()?,
            Vec::new(),
            "more on sale than is held sells nothing more"
        );
        Ok(())
    }

    /// A resting partial trim leaves only the remainder: with 1 of the 2.5 shares' excess on sale,
    /// 1.5 rounds up to 2. At a factor of 0 the remainder is capped at what is not yet on sale:
    /// 2.5 held with 1 on sale leaves 1.5, not the 2 the whole-share ceiling of 1.5 asks for.
    #[test]
    fn a_resting_partial_trim_proposes_only_the_remainder() -> Result<(), GateError> {
        let mut scene = Scene::new("10", "1000")?;
        scene.resting(7, "1", Side::Sell, false, true)?;
        assert_eq!(scene.trims()?, qty("2")?);
        let mut whole = Scene::new("2.5", "250")?;
        whole.risk.size_factor = Ratio::ZERO;
        whole.resting(7, "1", Side::Sell, false, true)?;
        assert_eq!(whole.trims()?, qty("1.5")?);
        Ok(())
    }

    /// Evaluation after evaluation with nothing filling, the agent's open sells never exceed what
    /// one evaluation proposes on its own: the trims accumulate to 3 shares and stop there.
    #[test]
    fn repeated_evaluations_never_put_more_than_one_trim_on_sale() -> Result<(), GateError> {
        let mut scene = Scene::new("10", "1000")?;
        let single = scene.trims()?;
        let once = single.first().copied().ok_or(GateError::ConfigOutOfRange)?;
        let mut on_sale = Qty::ZERO;
        for evaluation in 0..5_u64 {
            for trim in scene.trims()? {
                on_sale = on_sale.checked_add(trim)?;
                scene.resting(evaluation, &trim.to_string(), Side::Sell, false, true)?;
            }
            assert!(
                on_sale <= once,
                "evaluation {evaluation}: {on_sale} on sale against {once}"
            );
        }
        assert_eq!(
            on_sale, once,
            "the one trim the first evaluation proposed stays on sale"
        );
        Ok(())
    }

    /// Only the agent's own non-protective sells in the instrument are a trim in progress: a
    /// resting stop, another agent's sell and a buy each leave the 3-share trim whole.
    #[test]
    fn only_the_agents_own_unprotected_sells_are_a_trim_in_progress() -> Result<(), GateError> {
        let mut scene = Scene::new("10", "1000")?;
        scene.resting(1, "10", Side::Sell, true, true)?;
        scene.resting(2, "3", Side::Sell, false, false)?;
        scene.resting(3, "3", Side::Buy, false, true)?;
        assert_eq!(scene.trims()?, qty("3")?);
        let mut elsewhere = Scene::new("10", "1000")?;
        elsewhere.resting(4, "3", Side::Sell, false, true)?;
        let other = AssetId::new("other").map_err(|_| GateError::InstrumentUnknown)?;
        for order in elsewhere.account.working_orders.values_mut() {
            order.instrument = other.clone();
        }
        assert_eq!(elsewhere.trims()?, qty("3")?);
        Ok(())
    }
}
