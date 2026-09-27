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
/// purpose is ever what a zero crossing is denied under, which keeps MI-1 true as stated.
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

/// The boundaries the reference cases and the tests PR's properties leave unpinned, each found by
/// a mutant that survived them: a value exactly at a limit passes (DEC-129 item 15), the account's
/// own 1× bound binds where the agent's does not, and `exits_only` denies an opening.
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

    fn id(text: &str) -> Result<AssetId, GateError> {
        AssetId::new(text).map_err(|_| GateError::InstrumentUnknown)
    }

    /// Order 10 × 100 = `max_order_usd`; the agent also holds 1000 elsewhere, so its gross is
    /// exactly `max_gross_exposure_usd`; the account's equity is 2000, so the account is exactly
    /// at 1× too. Every bound is met with equality, and `other_agent_mv` pushes only the account's.
    fn decide(mode: AgentMode, other_agent_mv: &str) -> Result<Decision, GateError> {
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
        let (a, b, c) = (id("a")?, id("b")?, id("c")?);
        let account = AccountSnapshot {
            account_type: AccountType::Margin,
            state: AccountState::Active,
            crypto_active: true,
            regime: DayTradeRegime::LegacyPdt,
            equity: usd("2000")?,
            prior_close_equity: usd("30000")?,
            model_buying_power: usd("100000")?,
            broker_buying_power: usd("100000")?,
            broker_non_marginable_buying_power: usd("100000")?,
            positions: BTreeMap::new(),
            market_values: [(b.clone(), usd("1000")?), (c, usd(other_agent_mv)?)].into(),
            working_orders: BTreeMap::new(),
            unknown_orders: BTreeSet::new(),
            related_account_resting: BTreeMap::new(),
        };
        let agent = AgentSnapshot {
            agent: AgentId(1),
            mode,
            instrument_restrictions: BTreeMap::new(),
            positions: BTreeMap::new(),
            market_values: [(b, usd("1000")?)].into(),
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
        evaluate(&GateInput {
            now: UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?,
            pass: GatePass::First,
            config: &config,
            mandate: &mandate,
            risk: &risk,
            account: &account,
            agent: &agent,
            instrument: &instrument,
            market: &market,
            conduct: &ConductState::default(),
            universe: &WorkingUniverse::Known {
                instruments: [a].into(),
                pinned: true,
            },
            proposed: &proposed,
        })
    }

    #[test]
    fn exits_only_denies_an_opening() -> Result<(), GateError> {
        let d = decide(AgentMode::ExitsOnly, "0")?;
        assert_eq!(
            (d.verdict, d.reason),
            (Verdict::Deny, Some(ReasonCode::AgentExitsOnly)),
            "exits_only allows risk-reducing and protective orders only (trading spec §7.4)"
        );
        Ok(())
    }
}
