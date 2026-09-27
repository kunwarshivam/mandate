//! [`evaluate`]: §9.1's eight checks in order, stopping at the first that fails.
//!
//! Each check is one function returning `Ok(None)` when it passes and `Ok(Some(stop))` when it
//! decides, so the loop in [`evaluate`] is the only place the order lives.
//!
//! **A partial gate fails closed for adding risk** (DEC-129 item 29). While a check, or part of
//! one, is still owed by a later PR or story, an opening or increasing order the implemented checks
//! would allow is refused with [`GateError::Unimplemented`], naming the story that completes the
//! first missing check; a denial or hold from an implemented check still reports first. A reducing
//! purpose passes a check that does not exist yet (`AGENTS.md` rule 13: the broker is the
//! backstop), exactly as it will pass most of them once they do.

use mandate_num::Qty;

use crate::{
    AccountState, AgentMode, AssetClass, Check, CheckOutcome, Computed, Decision, GateError,
    GateInput, InstrumentRestriction, Origin, Purpose, ReasonCode, Side, Verdict, WorkingUniverse,
    limits,
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

/// The story that completes a check for an opening or an increase, while any part of it is owed.
/// Check 1 is whole here; check 2 still lacks the floor (E6-7), check 4 its rules 1, 2 and 4 to 8,
/// check 6 its conduct controls (E6-8) and check 7 buying power (E6-6), and checks 3, 5 and 8 are
/// not written yet.
fn owed_for_openings(check: Check) -> Option<&'static str> {
    match check {
        Check::AccountAndMode => None,
        Check::UniverseAndLimits => Some("E6-7"),
        Check::SessionAndHalt
        | Check::OrderConstraints
        | Check::BuyingPowerAndExposure
        | Check::DayTradeBudget => Some("E6-6"),
        Check::MarkAndCollar | Check::ConductControls => Some("E6-8"),
    }
}

/// §9.1: the first failing check decides, and every check after it is listed as not reached. A
/// check owed for an opening is listed as not reached too: the gate did not look.
pub(crate) fn evaluate(input: &GateInput<'_>) -> Result<Decision, GateError> {
    let held = input
        .agent
        .positions
        .get(&input.proposed.instrument)
        .copied()
        .unwrap_or(Qty::ZERO);
    let p = &input.proposed;
    let purpose = assign_purpose(p.origin, p.side, p.qty, held);
    let opening = matches!(purpose, Purpose::Open | Purpose::Increase);
    let mut computed = Computed::default();
    let mut checks = Vec::with_capacity(ORDER.len());
    let mut stop: Option<Stop> = None;
    let mut owed: Option<&'static str> = None;
    for check in ORDER {
        if stop.is_some() {
            checks.push(CheckOutcome::NotReached(check));
            continue;
        }
        match run(check, input, purpose, opening, &mut computed)? {
            Some((verdict, reason)) => {
                checks.push(CheckOutcome::Failed(check, reason));
                stop = Some((verdict, reason));
            }
            None => match owed_for_openings(check).filter(|_| opening) {
                Some(story) => {
                    checks.push(CheckOutcome::NotReached(check));
                    owed = owed.or(Some(story));
                }
                None => checks.push(CheckOutcome::Passed(check)),
            },
        }
    }
    let (verdict, reason) = match (stop, owed) {
        (Some((verdict, reason)), _) => (verdict, Some(reason)),
        (None, Some(story)) => return Err(GateError::Unimplemented("evaluate", story)),
        (None, None) => (Verdict::Allow, None),
    };
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
    opening: bool,
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    match check {
        Check::AccountAndMode => Ok(account_and_mode(input, purpose, opening)),
        Check::UniverseAndLimits if opening => match working_universe(input, computed)? {
            None => limits::position_order_and_cooldown(input, computed),
            stop => Ok(stop),
        },
        Check::OrderConstraints => Ok(order_constraints(input, opening)),
        Check::ConductControls if opening => Ok(limits::orders_per_day(input, computed)),
        Check::BuyingPowerAndExposure if opening => limits::gross_exposure(input, computed),
        _ => Ok(None),
    }
}

/// Check 1: the account (§7.3), then the agent's mode (§7.4). A blocked account is the broker arm
/// of MI-1's list, so it denies every purpose; a `closing_only` account and an inactive crypto
/// account deny an opening only. Then the mode rule of the brief's "exemptions, in one place", on
/// the stricter of the two modes the gate is handed, so neither snapshot can soften the other
/// (MI-6).
fn account_and_mode(input: &GateInput<'_>, purpose: Purpose, opening: bool) -> Option<Stop> {
    let account = input.account;
    if account.state == AccountState::Blocked {
        return Some((Verdict::Deny, ReasonCode::AccountTradingBlocked));
    }
    if opening && account.state == AccountState::ClosingOnly {
        return Some((Verdict::Deny, ReasonCode::AccountRestricted));
    }
    if opening && input.instrument.asset_class == AssetClass::Crypto && !account.crypto_active {
        return Some((Verdict::Deny, ReasonCode::CryptoAccountInactive));
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

/// Check 4, the rules this story owns: rule 3 (every sell above the position, a protective leg's
/// included, is typed an opening by [`assign_purpose`], so it is the only sell that reaches here as
/// one), then rule 9, which denies an opening and holds a reduction under one code (DEC-129 item
/// 22).
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

/// §9.1's purpose table, with the two rows v1's long-only book decides (DEC-129 item 30). Every
/// buy adds risk, a protective leg's included, since v1 holds no short for a buy to protect
/// (DEC-32). Every sell above the agent's position would open a short, so it is typed an opening
/// whatever its origin — a protective leg's too, which §5.3 checks against the position — and
/// check 4 denies it `would_cross_zero`: no reducing purpose is ever what a zero crossing is
/// denied under, which keeps MI-1 true as stated. Typed an opening, it meets check 1 before check
/// 4, so under `paused` or `stopped` it is held (and under `exits_only` denied
/// `agent_exits_only`) rather than denied `would_cross_zero`: §9.1's order applied as written, and
/// deliberate.
pub(crate) fn assign_purpose(origin: Origin, side: Side, qty: Qty, position: Qty) -> Purpose {
    match (origin, side) {
        (_, Side::Buy) if position.is_zero() => Purpose::Open,
        (_, Side::Buy) => Purpose::Increase,
        (_, Side::Sell) if qty > position => Purpose::Open,
        (Origin::ProtectiveLeg, Side::Sell) => Purpose::Protective,
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
/// live test passed (the #157 reviews): the eight check ids in §9.1's order, the order in which two
/// failing checks report, the stricter of the two mode snapshots, `exits_only` and an inactive
/// crypto account denying an opening, a protective leg above the position crossing zero, the
/// partial gate failing closed, a value exactly at each limit passing, and the account's own 1×
/// bound.
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

    /// An opening buy of 10 × 100 in instrument `a`, in the universe, that every check this PR
    /// implements allows.
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
    /// than read from the crate's own `ORDER`, which is the thing under test. A risk exit, because
    /// it passes every check this PR has, where an opening is refused until the gate is whole.
    #[test]
    fn the_checks_are_listed_in_section_9_1_order() -> Result<(), GateError> {
        let d = allowing()?.selling(Origin::RiskEngine)?.decide()?;
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
    /// every arm leaves `AgentSnapshot::mode` at `normal` and puts the stricter mode in the risk
    /// snapshot only, and the last puts `paused` in the agent snapshot only.
    #[test]
    fn the_stricter_of_the_two_modes_decides() -> Result<(), GateError> {
        let outcome = |o: Owned| o.decide().map(|d| (d.verdict, d.reason));
        let mut risk_paused = allowing()?.selling(Origin::RiskEngine)?;
        risk_paused.risk.agent_mode = AgentMode::Paused;
        let mut risk_stopped = allowing()?.selling(Origin::RiskEngine)?;
        risk_stopped.risk.agent_mode = AgentMode::Stopped;
        let mut risk_exits_only = allowing()?;
        risk_exits_only.risk.agent_mode = AgentMode::ExitsOnly;
        let mut agent_paused = allowing()?.selling(Origin::RiskEngine)?;
        agent_paused.agent.mode = AgentMode::Paused;
        assert_eq!(
            [
                outcome(risk_paused)?,
                outcome(risk_stopped)?,
                outcome(risk_exits_only)?,
                outcome(agent_paused)?,
            ],
            [
                (Verdict::Hold, Some(ReasonCode::AgentPaused)),
                (Verdict::Hold, Some(ReasonCode::AgentStopped)),
                (Verdict::Deny, Some(ReasonCode::AgentExitsOnly)),
                (Verdict::Hold, Some(ReasonCode::AgentPaused)),
            ],
            "paused holds a plain risk exit, stopped holds every order, and exits_only denies an \
             opening, whichever snapshot carries the mode"
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

    /// DEC-129 item 29: an opening every implemented check allows is refused, naming the story
    /// that completes the first check still owed (check 2's eligibility floor, E6-7).
    #[test]
    fn an_opening_the_partial_gate_would_allow_is_refused() -> Result<(), GateError> {
        let refused = allowing()?.decide();
        assert!(
            matches!(refused, Err(GateError::Unimplemented("evaluate", "E6-7"))),
            "a partial gate fails closed for adding risk, not open: {refused:?}"
        );
        Ok(())
    }

    /// §7.3: an inactive crypto account denies a crypto opening and leaves a crypto exit alone.
    #[test]
    fn an_inactive_crypto_account_denies_a_crypto_opening() -> Result<(), GateError> {
        let mut opening = allowing()?;
        opening.instrument.asset_class = AssetClass::Crypto;
        opening.account.crypto_active = false;
        let mut exit = allowing()?.selling(Origin::RiskEngine)?;
        exit.instrument.asset_class = AssetClass::Crypto;
        exit.account.crypto_active = false;
        let (d, e) = (opening.decide()?, exit.decide()?);
        assert_eq!(
            ((d.verdict, d.reason), (e.verdict, e.reason)),
            (
                (Verdict::Deny, Some(ReasonCode::CryptoAccountInactive)),
                (Verdict::Allow, None)
            ),
            "the gate requires crypto_status ACTIVE to open crypto; an exit is never denied by it"
        );
        Ok(())
    }

    /// §5.3 rule 3 for a protective leg: a stop that sells 11 against a position of 10 would open a
    /// short if it triggered, so it is typed an opening and denied `would_cross_zero`; and a
    /// protective "buy" adds risk in a long-only book, so it is an opening too (DEC-129 item 30).
    /// 11 × 90 = 990 sits inside every §5.3 limit, so check 2 passes it and check 4 is what denies.
    #[test]
    fn a_protective_leg_above_the_position_crosses_zero() -> Result<(), GateError> {
        let mut o = allowing()?.selling(Origin::ProtectiveLeg)?;
        o.proposed.qty = Qty::parse("11")?;
        o.proposed.limit_price = Price::parse("90")?;
        let d = o.decide()?;
        let buy = assign_purpose(
            Origin::ProtectiveLeg,
            Side::Buy,
            Qty::parse("1")?,
            Qty::ZERO,
        );
        assert_eq!(
            (d.verdict, d.reason, d.purpose, buy),
            (
                Verdict::Deny,
                Some(ReasonCode::WouldCrossZero),
                Purpose::Open,
                Purpose::Open
            ),
            "a protective leg is checked against the position; only a sell within it protects"
        );
        Ok(())
    }

    /// DEC-129 item 15: an order of exactly `max_order_usd` (10 × 100 = 1000), an instrument total
    /// of exactly the cap (500 held + 1000 = 1500), an agent gross of exactly its limit (500 + 500
    /// elsewhere + 1000 = 2000) and an account gross of exactly its equity all pass every limit.
    /// The opening is still refused, as item 29 requires while the floor is owed, and a limit that
    /// compared with `>=` would deny it instead.
    #[test]
    fn a_value_exactly_at_every_limit_passes_it() -> Result<(), GateError> {
        let mut o = allowing()?;
        let b = id("b")?;
        let held = Usd::parse("500")?;
        o.agent.market_values = [(o.proposed.instrument.clone(), held), (b.clone(), held)].into();
        o.agent
            .positions
            .insert(o.proposed.instrument.clone(), Qty::parse("5")?);
        o.account.market_values = o.agent.market_values.clone();
        o.account.equity = Usd::parse("2000")?;
        let refused = o.decide();
        assert!(
            matches!(refused, Err(GateError::Unimplemented("evaluate", "E6-7"))),
            "every comparison is `>`, so a value exactly at a limit passes it: {refused:?}"
        );
        Ok(())
    }

    /// §9.3: the account at 1× binds before the agent's own limit. Another agent's 1 on an account
    /// of equity 1000 puts the account at 1001, while this agent's 1000 is half its own limit.
    #[test]
    fn the_account_at_one_times_binds_before_the_agent_limit() -> Result<(), GateError> {
        let mut o = allowing()?;
        o.account.market_values = [(id("c")?, Usd::parse("1")?)].into();
        o.account.equity = Usd::parse("1000")?;
        let d = o.decide()?;
        assert_eq!(
            (d.verdict, d.reason, d.computed.gross),
            (Verdict::Deny, Some(ReasonCode::GrossExposureLimit), None),
            "the account bound denies before the agent's gross is ever computed"
        );
        Ok(())
    }
}
