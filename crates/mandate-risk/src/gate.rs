//! [`evaluate`]: §9.1's eight checks in order, stopping at the first that fails.
//!
//! Each check is one function returning `Ok(None)` when it passes and `Ok(Some(stop))` when it
//! decides, so the loop in [`evaluate`] is the only place the order lives. A check a later story
//! owns passes here until that story lands; the task brief's evaluation-order table names which.

use mandate_num::Qty;

use crate::{
    AccountState, AgentMode, Check, CheckOutcome, Computed, Decision, GateError, GateInput,
    InstrumentRestriction, Origin, Purpose, ReasonCode, Side, Verdict, WorkingUniverse,
};

/// The eight checks, in §9.1's order.
const ORDER: [Check; 8] = [
    Check::AccountAndMode,
    Check::UniverseAndLimits,
    Check::SessionAndHalt,
    Check::OrderConstraints,
    Check::MarkAndCollar,
    Check::ConductControls,
    Check::BuyingPowerAndExposure,
    Check::DayTradeBudget,
];

/// A check's decision when it does not pass.
pub(crate) type Stop = (Verdict, ReasonCode);

/// §9.1: the first failing check decides, and every check after it is listed as not reached.
pub(crate) fn evaluate(input: &GateInput<'_>) -> Result<Decision, GateError> {
    let held = input
        .agent
        .positions
        .get(&input.proposed.instrument)
        .copied()
        .unwrap_or(Qty::ZERO);
    let p = &input.proposed;
    let purpose = assign_purpose(p.origin, p.side, p.qty, held);
    let mut computed = Computed::default();
    let mut checks = Vec::with_capacity(ORDER.len());
    let mut stop: Option<Stop> = None;
    for check in ORDER {
        if stop.is_some() {
            checks.push(CheckOutcome::NotReached(check));
            continue;
        }
        match run(check, input, purpose, &mut computed)? {
            None => checks.push(CheckOutcome::Passed(check)),
            Some((verdict, reason)) => {
                checks.push(CheckOutcome::Failed(check, reason));
                stop = Some((verdict, reason));
            }
        }
    }
    let (verdict, reason) = stop.map_or((Verdict::Allow, None), |(v, r)| (v, Some(r)));
    Ok(Decision {
        verdict,
        reason,
        purpose,
        pacing: None,
        checks,
        computed,
    })
}

fn run(
    check: Check,
    input: &GateInput<'_>,
    purpose: Purpose,
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    let opening = matches!(purpose, Purpose::Open | Purpose::Increase);
    match check {
        Check::AccountAndMode => Ok(account_and_mode(input, purpose, opening)),
        Check::UniverseAndLimits if opening => working_universe(input, computed),
        Check::OrderConstraints => Ok(order_constraints(input, opening)),
        _ => Ok(None),
    }
}

/// Check 1. A blocked account is the broker arm of MI-1's list, so it denies every purpose; then
/// the mode rule of the brief's "exemptions, in one place", on the stricter of the two modes the
/// gate is handed, so neither snapshot can soften the other (MI-6).
fn account_and_mode(input: &GateInput<'_>, purpose: Purpose, opening: bool) -> Option<Stop> {
    if input.account.state == AccountState::Blocked {
        return Some((Verdict::Deny, ReasonCode::AccountTradingBlocked));
    }
    let exempt_from_paused = purpose == Purpose::Protective
        || (matches!(purpose, Purpose::RiskExit | Purpose::OwnerExit)
            && input.proposed.origin.is_kill_switch());
    match input.agent.mode.max(input.risk.agent_mode) {
        AgentMode::Stopped => Some((Verdict::Hold, ReasonCode::AgentStopped)),
        AgentMode::Paused if !exempt_from_paused => Some((Verdict::Hold, ReasonCode::AgentPaused)),
        AgentMode::ExitsOnly if opening => Some((Verdict::Deny, ReasonCode::AgentExitsOnly)),
        _ => None,
    }
}

/// Check 2's first item: the working universe, a removed instrument included (DEC-129 item 23).
/// An unread universe is an error, never a decision (item 3); only an opening reaches here.
fn working_universe(
    input: &GateInput<'_>,
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    let instrument = &input.proposed.instrument;
    let WorkingUniverse::Known { instruments, .. } = input.universe else {
        return Err(GateError::WorkingUniverseUnavailable);
    };
    let removed = input
        .agent
        .instrument_restrictions
        .get(instrument)
        .is_some_and(|r| r.contains(&InstrumentRestriction::RemovedInstrument));
    if removed || !instruments.contains(instrument) {
        computed.instrument = Some(instrument.clone());
        return Ok(Some((Verdict::Deny, ReasonCode::NotInWorkingUniverse)));
    }
    Ok(None)
}

/// Check 4, the rules this story owns: rule 3 (a sell above the position is typed an opening by
/// [`crate::assign_purpose`], so it is the only sell that reaches here as one), then rule 9, which
/// denies an opening and holds a reduction under one code (DEC-129 item 22).
fn order_constraints(input: &GateInput<'_>, opening: bool) -> Option<Stop> {
    if opening && input.proposed.side == Side::Sell {
        return Some((Verdict::Deny, ReasonCode::WouldCrossZero));
    }
    if input
        .account
        .unknown_orders
        .contains(&input.proposed.instrument)
    {
        let verdict = if opening {
            Verdict::Deny
        } else {
            Verdict::Hold
        };
        return Some((verdict, ReasonCode::UnknownOrderInFlight));
    }
    None
}

/// §9.1's purpose table. A sell above the agent's position would open a short, which v1 does not
/// hold (DEC-32), so it is typed an opening and check 4 denies it `would_cross_zero`: no reducing
/// purpose is ever what a zero crossing is denied under, which keeps MI-1 true as stated. Typed an
/// opening, it meets check 1 before check 4, so under `paused` or `stopped` it is held (and under
/// `exits_only` denied `agent_exits_only`) rather than denied `would_cross_zero`: §9.1's order
/// applied as written, and deliberate.
pub(crate) fn assign_purpose(origin: Origin, side: Side, qty: Qty, position: Qty) -> Purpose {
    match (origin, side) {
        (Origin::ProtectiveLeg, _) => Purpose::Protective,
        (_, Side::Buy) if position.is_zero() => Purpose::Open,
        (_, Side::Buy) => Purpose::Increase,
        (_, Side::Sell) if qty > position => Purpose::Open,
        (
            Origin::RiskEngine
            | Origin::TrimToTarget
            | Origin::StopWatchdog
            | Origin::AutomatedKillSwitch,
            Side::Sell,
        ) => Purpose::RiskExit,
        (Origin::OwnerClose | Origin::OwnerKillSwitch, Side::Sell) => Purpose::OwnerExit,
        (Origin::OrderBuilder | Origin::GoalCompletion | Origin::RemovedInstrument, Side::Sell) => {
            Purpose::DiscretionaryExit
        }
    }
}

/// What the tests PR's files cannot pin on this PR's code, each shown by a planted bug that every
/// live test passed: the eight check ids in §9.1's order, the order in which two failing checks
/// report, the stricter of the two mode snapshots, and `exits_only` denying an opening.
#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use mandate_num::{Fraction, Price, Ratio, Usd};
    use mandate_time::UtcNanos;

    use super::*;
    use crate::spec_types::{GoalState, RiskLimits, RiskSnapshot, ValidatedMandate};
    use crate::{
        AccountSnapshot, AccountType, AgentId, AgentSnapshot, AssetClass, AssetId, ClientOrderId,
        ConductState, DayTradeLedger, DayTradeRegime, EtpClass, GateConfig, GatePass,
        InstrumentSnapshot, MarketSnapshot, ProposedKind, ProposedOrder, TimeInForce,
    };

    /// Every input of one decision, owned, so a test changes only the field it is about.
    struct Owned {
        config: GateConfig,
        mandate: ValidatedMandate,
        risk: RiskSnapshot,
        account: AccountSnapshot,
        agent: AgentSnapshot,
        instrument: InstrumentSnapshot,
        market: MarketSnapshot,
        universe: WorkingUniverse,
        proposed: ProposedOrder,
    }

    impl Owned {
        fn decide(&self) -> Result<Decision, GateError> {
            evaluate(&GateInput {
                now: UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?,
                pass: GatePass::First,
                config: &self.config,
                mandate: &self.mandate,
                risk: &self.risk,
                account: &self.account,
                agent: &self.agent,
                instrument: &self.instrument,
                market: &self.market,
                conduct: &ConductState::default(),
                universe: &self.universe,
                proposed: &self.proposed,
            })
        }

        /// A sell of the whole position of 10, from the origin given.
        fn selling(mut self, origin: Origin) -> Result<Self, GateError> {
            self.agent
                .positions
                .insert(self.proposed.instrument.clone(), Qty::parse("10")?);
            self.proposed.side = Side::Sell;
            self.proposed.origin = origin;
            Ok(self)
        }
    }

    fn id(text: &str) -> Result<AssetId, GateError> {
        AssetId::new(text).map_err(|_| GateError::InstrumentUnknown)
    }

    /// An opening buy of 10 × 100 in instrument `a`, in the universe, that every check allows.
    fn allowing() -> Result<Owned, GateError> {
        let usd = Usd::parse;
        let fraction = Fraction::parse;
        let config = GateConfig {
            price_floor: usd("5")?,
            liquidity_floor_usd: usd("1000000")?,
            crypto_liquidity_floor_usd: usd("1000000")?,
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
        };
        let mandate = ValidatedMandate::from_validated_parts(
            RiskLimits {
                max_position_usd: usd("1500")?,
                max_position_fraction: fraction("0.2")?,
                max_order_usd: usd("1000")?,
                max_gross_exposure_usd: usd("2000")?,
                max_orders_per_day: 50,
                reentry_cooldown_s: 3600,
                rebalance_band: fraction("0.05")?,
                breach_confirm_s: 60,
                drawdown_ladder: Vec::new(),
            },
            GoalState::Running,
            false,
            false,
        );
        let risk = RiskSnapshot {
            agent_equity: usd("10000")?,
            high_water_mark: usd("10000")?,
            day_start_equity: usd("10000")?,
            capital_base: usd("10000")?,
            inherited_loss: Usd::ZERO,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: Ratio::parse("1")?,
            agent_mode: AgentMode::Normal,
        };
        let a = id("a")?;
        let account = AccountSnapshot {
            account_type: AccountType::Margin,
            state: AccountState::Active,
            crypto_active: true,
            regime: DayTradeRegime::LegacyPdt,
            equity: usd("10000")?,
            prior_close_equity: usd("30000")?,
            model_buying_power: usd("100000")?,
            broker_buying_power: usd("100000")?,
            broker_non_marginable_buying_power: usd("100000")?,
            positions: BTreeMap::new(),
            market_values: BTreeMap::new(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        };
        let agent = AgentSnapshot {
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
        };
        let instrument = InstrumentSnapshot {
            instrument: a.clone(),
            asset_class: AssetClass::UsEquity,
            exchange: None,
            status_active: true,
            tradable: true,
            fractionable: false,
            ipo: false,
            ptp_no_exception: false,
            etp: EtpClass::Plain,
            etp_classified_at: None,
            prior_close: None,
            median_dollar_volume_20d: None,
            median_dollar_volume_30d: None,
            min_order_size: Qty::parse("1")?,
            halted: false,
            status_feed_current: true,
        };
        let market = MarketSnapshot {
            quote: None,
            last_trade: None,
            trailing_5m_volume: None,
            adv_20d: None,
        };
        let proposed = ProposedOrder {
            instrument: a.clone(),
            side: Side::Buy,
            qty: Qty::parse("10")?,
            limit_price: Price::parse("100")?,
            kind: ProposedKind::Plain,
            tif: TimeInForce::Day,
            extended_hours: false,
            origin: Origin::OrderBuilder,
            owner_confirmed_bid: None,
            client_order_id: ClientOrderId(1),
            fee_reservation: Usd::ZERO,
        };
        Ok(Owned {
            config,
            mandate,
            risk,
            account,
            agent,
            instrument,
            market,
            universe: WorkingUniverse::Known {
                instruments: [a].into(),
                pinned: true,
            },
            proposed,
        })
    }

    fn outside_the_universe(mut o: Owned) -> Owned {
        o.universe = WorkingUniverse::Known {
            instruments: BTreeSet::new(),
            pinned: true,
        };
        o
    }

    /// The journal's check list carries §9.1's eight ids in §9.1's order, spelled out here rather
    /// than read from the crate's own `ORDER`, which is the thing under test.
    #[test]
    fn the_checks_are_listed_in_section_9_1_order() -> Result<(), GateError> {
        let d = allowing()?.decide()?;
        let ids: Vec<Check> = d
            .checks
            .iter()
            .map(|c| match c {
                CheckOutcome::Passed(id)
                | CheckOutcome::Failed(id, _)
                | CheckOutcome::NotReached(id) => *id,
            })
            .collect();
        assert_eq!(
            (d.verdict, ids),
            (
                Verdict::Allow,
                vec![
                    Check::AccountAndMode,
                    Check::UniverseAndLimits,
                    Check::SessionAndHalt,
                    Check::OrderConstraints,
                    Check::MarkAndCollar,
                    Check::ConductControls,
                    Check::BuyingPowerAndExposure,
                    Check::DayTradeBudget,
                ]
            ),
            "§9.1: account and mode, universe and limits, session and halt, order constraints, \
             mark and collar, conduct, buying power and exposure, day-trade budget"
        );
        Ok(())
    }

    /// Two checks fail at once and the earlier one reports: check 1 before check 2.
    #[test]
    fn the_mode_reports_before_the_universe() -> Result<(), GateError> {
        let mut o = outside_the_universe(allowing()?);
        o.agent.mode = AgentMode::ExitsOnly;
        let d = o.decide()?;
        assert_eq!(
            (d.verdict, d.reason),
            (Verdict::Deny, Some(ReasonCode::AgentExitsOnly)),
            "exits_only (check 1) and an instrument outside the universe (check 2) both fail"
        );
        Ok(())
    }

    /// Two checks fail at once and the earlier one reports: check 2 before check 4.
    #[test]
    fn the_universe_reports_before_an_unknown_order() -> Result<(), GateError> {
        let mut o = outside_the_universe(allowing()?);
        o.account
            .unknown_orders
            .insert(o.proposed.instrument.clone());
        let d = o.decide()?;
        assert_eq!(
            (d.verdict, d.reason),
            (Verdict::Deny, Some(ReasonCode::NotInWorkingUniverse)),
            "the universe (check 2) and an Unknown order (check 4, rule 9) both fail"
        );
        Ok(())
    }

    /// The mode read is the stricter of the two snapshots, so neither softens the other (MI-6):
    /// each arm puts `paused` in one snapshot only.
    #[test]
    fn the_stricter_of_the_two_modes_holds() -> Result<(), GateError> {
        let mut risk_paused = allowing()?.selling(Origin::RiskEngine)?;
        risk_paused.risk.agent_mode = AgentMode::Paused;
        let mut agent_paused = allowing()?.selling(Origin::RiskEngine)?;
        agent_paused.agent.mode = AgentMode::Paused;
        let held = |d: Decision| (d.verdict, d.reason);
        assert_eq!(
            (held(risk_paused.decide()?), held(agent_paused.decide()?)),
            (
                (Verdict::Hold, Some(ReasonCode::AgentPaused)),
                (Verdict::Hold, Some(ReasonCode::AgentPaused))
            ),
            "a plain risk exit is held by paused whichever snapshot carries it"
        );
        Ok(())
    }

    #[test]
    fn exits_only_denies_an_opening() -> Result<(), GateError> {
        let mut o = allowing()?;
        o.agent.mode = AgentMode::ExitsOnly;
        let d = o.decide()?;
        assert_eq!(
            (d.verdict, d.reason),
            (Verdict::Deny, Some(ReasonCode::AgentExitsOnly)),
            "exits_only allows risk-reducing and protective orders only (trading spec §7.4)"
        );
        Ok(())
    }
}
