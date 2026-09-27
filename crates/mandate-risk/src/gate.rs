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

use std::collections::BTreeSet;

use mandate_num::Qty;

use crate::{
    AccountState, AgentMode, AssetClass, Check, CheckOutcome, Computed, Decision, GateError,
    GateInput, InstrumentRestriction, Origin, Pacing, ProposedKind, Purpose, ReasonCode, Session,
    SessionAt, Side, Verdict, WorkingUniverse, account_rules, floor, limits, session,
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

/// The story that completes a check, while any part of it is owed: check 5 (marks and the collar)
/// and check 6's conduct controls are E6-8's, and check 2 still owes §3.2 item 7's "USD pairs
/// only" for crypto (E6-10). Checks 1, 3, 4, 7 and 8 are whole.
///
/// Check 2 takes the input because "USD pairs only" has no field to read: `InstrumentSnapshot`
/// carries no quote currency and `AssetId` is a UUID, so nothing distinguishes BTC/USD from
/// BTC/USDT. Calling check 2 whole for crypto would let a non-USD pair be opened the moment E6-8
/// lands, so a crypto opening stays owed and is refused by the fail-closed rule until E6-10
/// supplies the field (DEC-129 item 34). A crypto *exit* is unaffected: `first_owed` accrues only
/// for an opening.
fn owed(check: Check, input: &GateInput<'_>) -> Option<&'static str> {
    match check {
        Check::UniverseAndLimits if input.instrument.asset_class != AssetClass::UsEquity => {
            Some("E6-10")
        }
        Check::MarkAndCollar | Check::ConductControls => Some("E6-8"),
        Check::AccountAndMode
        | Check::UniverseAndLimits
        | Check::SessionAndHalt
        | Check::OrderConstraints
        | Check::BuyingPowerAndExposure
        | Check::DayTradeBudget => None,
    }
}

/// §9.1: the first failing check decides, and every check after it is listed as not reached. An
/// owed check is listed as not reached too, for every purpose: the gate did not look, and the
/// journal must not say it passed. Only an opening is refused for it (DEC-129 item 29). So
/// `pacing: None` on an allowed reduction means nothing was paced *because checks 5 and 6 were
/// not reached*, which `Decision::checks` records, not that the collar and the participation caps
/// found nothing to do.
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
    let at = session::derive(input.now, input.config, input.instrument.asset_class)?;
    let mut computed = Computed::default();
    let mut checks = Vec::with_capacity(ORDER.len());
    let mut stop: Option<Stop> = None;
    let mut first_owed: Option<&'static str> = None;
    for check in ORDER {
        if stop.is_some() {
            checks.push(CheckOutcome::NotReached(check));
            continue;
        }
        match run(check, input, (purpose, opening, &at), &mut computed)? {
            Some((verdict, reason)) => {
                checks.push(CheckOutcome::Failed(check, reason));
                stop = Some((verdict, reason));
            }
            None => match owed(check, input) {
                Some(story) => {
                    checks.push(CheckOutcome::NotReached(check));
                    if opening {
                        first_owed = first_owed.or(Some(story));
                    }
                }
                None => checks.push(CheckOutcome::Passed(check)),
            },
        }
    }
    let (verdict, reason) = match (stop, first_owed) {
        (Some((verdict, reason)), _) => (verdict, Some(reason)),
        (None, Some(story)) => return Err(GateError::Unimplemented("evaluate", story)),
        (None, None) => (Verdict::Allow, None),
    };
    let pacing = if verdict == Verdict::Allow {
        market_exit_repricing(input, &at)
    } else {
        None
    };
    Ok(Decision {
        verdict,
        reason,
        purpose,
        pacing,
        checks,
        computed,
    })
}

fn run(
    check: Check,
    input: &GateInput<'_>,
    (purpose, opening, at): (Purpose, bool, &SessionAt),
    computed: &mut Computed,
) -> Result<Option<Stop>, GateError> {
    match check {
        Check::AccountAndMode => Ok(account_and_mode(input, purpose, opening)),
        Check::UniverseAndLimits if opening => match working_universe(input, computed)? {
            None => match floor::eligibility(input)? {
                None => limits::position_order_and_cooldown(input, computed),
                stop => Ok(stop),
            },
            stop => Ok(stop),
        },
        Check::SessionAndHalt => {
            Ok(session::rules(input, purpose, at).or_else(|| halt(input).filter(|_| opening)))
        }
        Check::OrderConstraints => order_constraints(input, opening),
        Check::ConductControls if opening => Ok(limits::orders_per_day(input, computed)),
        Check::BuyingPowerAndExposure if opening => match account_rules::buying_power(input)? {
            None => limits::gross_exposure(input, computed),
            stop => Ok(stop),
        },
        Check::DayTradeBudget if opening => account_rules::day_trade_budget(input),
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
    match account.state {
        AccountState::Blocked => return Some((Verdict::Deny, ReasonCode::AccountTradingBlocked)),
        AccountState::ClosingOnly if opening => {
            return Some((Verdict::Deny, ReasonCode::AccountRestricted));
        }
        AccountState::ClosingOnly | AccountState::Active => {}
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

/// Check 3's halt (§4.4), after the session rules: a halted or paused instrument takes no new
/// opening order. It is applied to an opening only, so a halt never denies an exit (MI-1); a
/// presumed halt, a dropped status feed, is not a halt and denies nothing here (DEC-129 item 24).
fn halt(input: &GateInput<'_>) -> Option<Stop> {
    input
        .instrument
        .halted
        .then_some((Verdict::Deny, ReasonCode::InstrumentHalted))
}

/// §5.1: a market order is allowed only in the regular session outside the auction windows with
/// current status data, so it is barred under a halt, real or presumed (§4.4: a halted instrument
/// or a dropped status feed), for a US equity outside the regular session (§9.4: "exits in
/// extended hours as limit orders"), and in either auction window (§4.3: "exits in them use limit
/// orders, never market orders"; DEC-159). The stale-quote arm of a presumed halt reads the mark's
/// freshness, which is check 5's (E6-8).
fn market_orders_barred(input: &GateInput<'_>, at: &SessionAt) -> bool {
    let i = input.instrument;
    i.halted
        || !i.status_feed_current
        || at.opening_auction
        || at.close_window
        || (i.asset_class == AssetClass::UsEquity && at.session != Session::Regular)
}

/// §4.3, §4.4, §5.6 and §9.4 (DEC-129 items 28 and 31, DEC-159): an allowed market-order exit
/// where market orders are barred is sent as a marketable limit at its proposed quantity and price, never as a market
/// order, and never denied. Only a reduction is allowed with a market kind, since check 4 denies
/// every market opening.
fn market_exit_repricing(input: &GateInput<'_>, at: &SessionAt) -> Option<Pacing> {
    (market_orders_barred(input, at) && input.proposed.kind == ProposedKind::Market).then(|| {
        Pacing {
            qty: input.proposed.qty,
            limit_price: input.proposed.limit_price,
            marketable_limit_required: true,
            applied: BTreeSet::new(),
        }
    })
}

/// Check 4, §5.3's rules in list order. Rule 1 is unrepresentable (a proposal has no notional).
/// Rules 2 and 7 have no registered code, so an opening that breaks one is refused rather than
/// decided ([`account_rules::unregistered_rules`], DEC-129 item 27). Rule 3: every sell above the
/// position, a protective leg's included, is typed an opening by [`assign_purpose`], so it is the
/// only sell that reaches here as one. Rule 4 binds a reduction and rules 5, 6 and 8 an opening
/// ([`account_rules`]). Rule 9 denies an opening and holds a reduction under one code (DEC-129 item
/// 22). Then §5.1's order policy: every opening is a limit order, so a market opening is
/// `market_order_not_allowed` in any state of the feed (DEC-129 item 24).
fn order_constraints(input: &GateInput<'_>, opening: bool) -> Result<Option<Stop>, GateError> {
    if opening {
        account_rules::unregistered_rules(input)?;
        if input.proposed.side == Side::Sell {
            return Ok(Some((Verdict::Deny, ReasonCode::WouldCrossZero)));
        }
    }
    let rules_4_to_8 = if opening {
        account_rules::one_working_order(input)
    } else {
        account_rules::sell_available(input)?
    };
    if rules_4_to_8.is_some() {
        return Ok(rules_4_to_8);
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
        return Ok(Some((verdict, ReasonCode::UnknownOrderInFlight)));
    }
    Ok((opening && input.proposed.kind == ProposedKind::Market)
        .then_some((Verdict::Deny, ReasonCode::MarketOrderNotAllowed)))
}

/// §9.1's purpose table, with the two rows v1's long-only book decides (DEC-129 item 30). Every
/// buy adds risk, a protective leg's included, since v1 holds no short for a buy to protect
/// (DEC-32). Every sell above the agent's position would open a short, so it is typed an opening
/// whatever its origin — a protective leg's too — and check 4 denies it `would_cross_zero`, which
/// is §9.1's own rule ("a sell above the position is denied"): no reducing purpose is ever what a
/// zero crossing is
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
/// bound; for E6-9, every row of the halt and market-order table; and for E6-7, every row of the
/// eligibility floor and where it sits in check 2.
#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use mandate_num::{Fraction, Price, Ratio, Usd};
    use mandate_time::UtcNanos;

    use super::*;
    use crate::floor;
    use crate::spec_types::{GoalState, RiskLimits, RiskSnapshot, ValidatedMandate};
    use crate::{
        AccountSnapshot, AccountType, AgentId, AgentSnapshot, AssetClass, AssetId, ClientOrderId,
        ConductState, DayTradeLedger, DayTradeRegime, EtpClass, GateConfig, GatePass, GroupId,
        InstrumentSnapshot, MarketSnapshot, ProposedKind, ProposedOrder, TimeInForce, WorkingOrder,
    };

    /// Every input of one decision, owned, so a test changes only the field it is about.
    struct Owned {
        now: UtcNanos,
        pass: GatePass,
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
            self.with_input(evaluate)?
        }

        /// `f` over this scenario's inputs.
        fn with_input<T>(&self, f: impl FnOnce(&GateInput<'_>) -> T) -> Result<T, GateError> {
            Ok(f(&GateInput {
                now: self.now,
                pass: self.pass,
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
            }))
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

    /// An opening buy of 10 × 100 in instrument `a`, in the universe, of an instrument that passes
    /// the whole eligibility floor, that every check this PR implements allows.
    fn allowing() -> Result<Owned, GateError> {
        let usd = Usd::parse;
        let fraction = Fraction::parse;
        let config = GateConfig {
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
            exchange: Some(crate::Exchange::Nasdaq),
            status_active: true,
            tradable: true,
            fractionable: false,
            ipo: false,
            ptp_no_exception: false,
            etp: EtpClass::Plain,
            etp_classified_at: Some(UtcNanos::parse_rfc3339("2026-09-21T00:00:00Z")?),
            prior_close: Some(Price::parse("100")?),
            median_dollar_volume_20d: Some(usd("90000000")?),
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
            now: UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?,
            pass: GatePass::First,
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
        assert_eq!(
            d.checks,
            [
                CheckOutcome::Passed(Check::AccountAndMode),
                CheckOutcome::Passed(Check::UniverseAndLimits),
                CheckOutcome::Passed(Check::SessionAndHalt),
                CheckOutcome::Passed(Check::OrderConstraints),
                CheckOutcome::NotReached(Check::MarkAndCollar),
                CheckOutcome::NotReached(Check::ConductControls),
                CheckOutcome::Passed(Check::BuyingPowerAndExposure),
                CheckOutcome::Passed(Check::DayTradeBudget),
            ],
            "an owed check is journaled NotReached for a reduction too, never Passed: the gate did \
             not look"
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
    /// that completes the first check still owed (check 5's marks and collar, E6-8).
    #[test]
    fn an_opening_the_partial_gate_would_allow_is_refused() -> Result<(), GateError> {
        let refused = allowing()?.decide();
        assert!(
            matches!(refused, Err(GateError::Unimplemented("evaluate", "E6-8"))),
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
            "a sell above the position is denied (§9.1), a protective leg's too; only a sell \
             within it protects"
        );
        Ok(())
    }

    /// DEC-129 item 15: an order of exactly `max_order_usd` (10 × 100 = 1000), an instrument total
    /// of exactly the cap (500 held + 1000 = 1500), an agent gross of exactly its limit (500 + 500
    /// elsewhere + 1000 = 2000) and an account gross of exactly its equity all pass every limit.
    /// The opening is still refused, as item 29 requires while a check is owed, and a limit that
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
            matches!(refused, Err(GateError::Unimplemented("evaluate", "E6-8"))),
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

    /// Mandate §5.3's re-entry cooldown binds only an exit fill in the instrument's own group, and
    /// only strictly inside `reentry_cooldown_s`. An exit in an ungrouped other instrument 10 s ago
    /// denies nothing, and an exit in this instrument exactly 3600 s ago has run its course: both
    /// openings pass the cooldown and are refused only because a later check is owed (item 29).
    #[test]
    fn the_cooldown_binds_only_its_group_and_only_strictly_inside_it() -> Result<(), GateError> {
        let now = UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?;
        let mut other_group = allowing()?;
        other_group
            .agent
            .last_exit_fill_at
            .insert(id("z")?, UtcNanos::from_parts(now.secs() - 10, 0)?);
        let mut run_its_course = allowing()?;
        let mine = run_its_course.proposed.instrument.clone();
        run_its_course
            .agent
            .last_exit_fill_at
            .insert(mine, UtcNanos::from_parts(now.secs() - 3600, 0)?);
        let (a, b) = (other_group.decide(), run_its_course.decide());
        assert!(
            matches!(a, Err(GateError::Unimplemented("evaluate", "E6-8")))
                && matches!(b, Err(GateError::Unimplemented("evaluate", "E6-8"))),
            "neither is a reentry_cooldown denial: another group's exit {a:?}, 3600 s after {b:?}"
        );
        Ok(())
    }

    /// DEC-129 item 23: a `removed_instrument` restriction on an instrument still in the working
    /// universe denies an opening at check 2 as `not_in_working_universe`, and says nothing about
    /// another instrument.
    #[test]
    fn a_removed_instrument_is_the_working_universe_check() -> Result<(), GateError> {
        let mut o = allowing()?;
        o.agent.instrument_restrictions.insert(
            o.proposed.instrument.clone(),
            [InstrumentRestriction::RemovedInstrument].into(),
        );
        let d = o.decide()?;
        assert_eq!(
            (d.verdict, d.reason, d.checks.get(1)),
            (
                Verdict::Deny,
                Some(ReasonCode::NotInWorkingUniverse),
                Some(&CheckOutcome::Failed(
                    Check::UniverseAndLimits,
                    ReasonCode::NotInWorkingUniverse
                ))
            ),
            "the instrument is in the universe; the restriction alone denies, at check 2"
        );
        Ok(())
    }

    /// One opening of 1 × 100 in `a` after `edit`: `Ok(code)` for a denial, `Err(story)` for the
    /// refusal of the next owed check (item 29), which is what "passes every limit" looks like.
    fn limit_row(
        edit: impl FnOnce(&mut Owned) -> Result<(), GateError>,
    ) -> Result<Result<ReasonCode, &'static str>, GateError> {
        let mut o = allowing()?;
        o.proposed.qty = Qty::parse("1")?;
        edit(&mut o)?;
        match o.decide() {
            Ok(d) => Ok(d.reason.ok_or("allowed")),
            Err(GateError::Unimplemented(_, story)) => Ok(Err(story)),
            Err(e) => Err(e),
        }
    }

    /// The agent's gross limit is `min(max_gross_exposure_usd, agent equity)`, never the account's
    /// equity: with the agent at 1500 on an account of 1000000, 1400 held elsewhere plus 100 is
    /// exactly the limit and passes, and one nano-dollar more is denied.
    #[test]
    fn the_agent_gross_limit_is_on_agent_equity() -> Result<(), GateError> {
        let held_elsewhere = |mv: &'static str| {
            move |o: &mut Owned| -> Result<(), GateError> {
                o.risk.agent_equity = Usd::parse("1500")?;
                o.account.equity = Usd::parse("1000000")?;
                o.agent.market_values = [(id("b")?, Usd::parse(mv)?)].into();
                Ok(())
            }
        };
        assert_eq!(
            [
                limit_row(held_elsewhere("1400"))?,
                limit_row(held_elsewhere("1400.000000001"))?,
            ],
            [Err("E6-8"), Ok(ReasonCode::GrossExposureLimit)],
            "1400 + 100 against min(2000, agent equity 1500)"
        );
        Ok(())
    }

    /// The per-instrument cap scales the agent's equity: at 1000 the cap is min(1500, 0.2 × 1000)
    /// = 200, so 100 held plus 100 passes, one nano-dollar more is denied, and the denial reports
    /// that cap.
    #[test]
    fn the_fraction_cap_is_on_agent_equity() -> Result<(), GateError> {
        let held_here = |mv: &'static str| {
            move |o: &mut Owned| -> Result<(), GateError> {
                o.risk.agent_equity = Usd::parse("1000")?;
                o.account.equity = Usd::parse("1000000")?;
                o.agent.market_values = [(o.proposed.instrument.clone(), Usd::parse(mv)?)].into();
                o.agent
                    .positions
                    .insert(o.proposed.instrument.clone(), Qty::parse("1")?);
                Ok(())
            }
        };
        let mut over = allowing()?;
        over.proposed.qty = Qty::parse("1")?;
        held_here("100.000000001")(&mut over)?;
        assert_eq!(
            (limit_row(held_here("100"))?, over.decide()?.computed.cap,),
            (Err("E6-8"), Some(Usd::parse("200")?)),
            "100 + 100 is exactly the cap of 200; over it the denial reports cap 200"
        );
        assert_eq!(
            limit_row(held_here("100.000000001"))?,
            Ok(ReasonCode::ConcentrationLimit),
            "one nano-dollar over the cap"
        );
        Ok(())
    }

    /// The account's 1× bound counts every agent's working opening orders: another agent's 900
    /// plus this 100 is exactly an account equity of 1000 and passes; one nano-dollar more denies.
    #[test]
    fn the_account_bound_counts_other_agents_working_orders() -> Result<(), GateError> {
        let other_agent = |cost: &'static str| {
            move |o: &mut Owned| -> Result<(), GateError> {
                o.account.equity = Usd::parse("1000")?;
                o.account.working_orders.insert(
                    ClientOrderId(9),
                    WorkingOrder {
                        agent: AgentId(2),
                        instrument: id("c")?,
                        side: Side::Buy,
                        max_cost: Usd::parse(cost)?,
                        open_qty: Qty::parse("1")?,
                        protective: false,
                        opening: true,
                        submitted_on: mandate_time::Date::parse("2026-09-21")?,
                    },
                );
                Ok(())
            }
        };
        assert_eq!(
            [
                limit_row(other_agent("900"))?,
                limit_row(other_agent("900.000000001"))?,
            ],
            [Err("E6-8"), Ok(ReasonCode::GrossExposureLimit)],
            "900 of another agent's + 100 against the account's 1000"
        );
        Ok(())
    }

    /// Working cost is the agent's working *opening, non-protective* orders: a 100000 order in the
    /// instrument that is protective, or that is not an opening, passes check 2's cap and is met
    /// only by check 4's own rules, 8 and 6.
    #[test]
    fn working_cost_ignores_protective_and_non_opening_orders() -> Result<(), GateError> {
        let big_order = |protective: bool, opening: bool| {
            move |o: &mut Owned| -> Result<(), GateError> {
                o.account.working_orders.insert(
                    ClientOrderId(9),
                    WorkingOrder {
                        agent: AgentId(1),
                        instrument: o.proposed.instrument.clone(),
                        side: Side::Buy,
                        max_cost: Usd::parse("100000")?,
                        open_qty: Qty::parse("1")?,
                        protective,
                        opening,
                        submitted_on: mandate_time::Date::parse("2026-09-21")?,
                    },
                );
                Ok(())
            }
        };
        assert_eq!(
            [
                limit_row(big_order(true, true))?,
                limit_row(big_order(false, false))?,
                limit_row(big_order(false, true))?,
            ],
            [
                Ok(ReasonCode::AddBlockedByProtectiveOrder),
                Ok(ReasonCode::WorkingOrderLimit),
                Ok(ReasonCode::ConcentrationLimit)
            ],
            "protective and non-opening orders pass check 2's cap and meet check 4's rules 8 and \
             6; an opening order counts toward the cap"
        );
        Ok(())
    }

    /// What the halt table expects of one row: the verdict, the code, the check that failed, and
    /// the pacing, or the story a fail-closed opening is refused for.
    type HaltRow =
        Result<(Verdict, Option<ReasonCode>, Option<Check>, Option<Pacing>), &'static str>;

    /// The halt table's oracle, transcribed from the spec rather than from this module: the mode
    /// rule of `ref.py`'s `order_decision` (check 1), then `exits_only` denying an opening (§7.4),
    /// then §4.4's halt for an opening at check 3, then §5.1's limit-only openings at check 4, then
    /// the fail-closed refusal of DEC-129 item 29 for check 5 (E6-8); an exit is allowed, and re-priced as a marketable limit when it is a
    /// market order under a halt or a dropped status feed (§4.4, §5.6, DEC-129 items 28 and 31).
    /// Only the `Market` kind is a market order: a bracket, an IOC and a plain limit are limits.
    fn halt_oracle(
        side: Side,
        origin: Origin,
        mode: AgentMode,
        (halted, feed_current): (bool, bool),
        proposed: &crate::ProposedOrder,
    ) -> HaltRow {
        let market = proposed.kind == ProposedKind::Market;
        let exit = side == Side::Sell;
        let exempt = exit
            && matches!(
                origin,
                Origin::ProtectiveLeg | Origin::AutomatedKillSwitch | Origin::OwnerKillSwitch
            );
        let hold = |code| Ok((Verdict::Hold, Some(code), Some(Check::AccountAndMode), None));
        if mode == AgentMode::Stopped {
            return hold(ReasonCode::AgentStopped);
        }
        if mode == AgentMode::Paused && !exempt {
            return hold(ReasonCode::AgentPaused);
        }
        if !exit {
            let deny = |code, check| Ok((Verdict::Deny, Some(code), Some(check), None));
            if mode == AgentMode::ExitsOnly {
                return deny(ReasonCode::AgentExitsOnly, Check::AccountAndMode);
            }
            if halted {
                return deny(ReasonCode::InstrumentHalted, Check::SessionAndHalt);
            }
            if market {
                return deny(ReasonCode::MarketOrderNotAllowed, Check::OrderConstraints);
            }
            return Err("E6-8");
        }
        let repriced = market && (halted || !feed_current);
        let pacing = repriced.then(|| Pacing {
            qty: proposed.qty,
            limit_price: proposed.limit_price,
            marketable_limit_required: true,
            applied: BTreeSet::new(),
        });
        Ok((Verdict::Allow, None, None, pacing))
    }

    /// Every side, origin (10), `AgentMode` (all 4), halt, status feed and `ProposedKind` (all 4)
    /// against [`halt_oracle`]: 2 × 10 × 4 × 2 × 2 × 4 = 1280 rows. It pins that a halt denies
    /// only an opening and only at check 3; that a market opening is denied at check 4 whatever
    /// the feed, and a bracket or IOC opening never is; that a market exit
    /// under a real or presumed halt is allowed and re-priced at its own quantity and price, in
    /// every mode that lets it through, `exits_only` included; that a limit, bracket or IOC exit
    /// and a market exit with a current feed are not paced; and that a held exit carries no
    /// pacing.
    #[test]
    fn halts_and_market_orders_match_the_oracle_on_every_row() -> Result<(), GateError> {
        let origins = [
            Origin::OrderBuilder,
            Origin::GoalCompletion,
            Origin::RemovedInstrument,
            Origin::RiskEngine,
            Origin::TrimToTarget,
            Origin::StopWatchdog,
            Origin::AutomatedKillSwitch,
            Origin::OwnerClose,
            Origin::OwnerKillSwitch,
            Origin::ProtectiveLeg,
        ];
        let mut rows = 0_u32;
        for side in [Side::Buy, Side::Sell] {
            for origin in origins {
                for mode in [
                    AgentMode::Normal,
                    AgentMode::ExitsOnly,
                    AgentMode::Paused,
                    AgentMode::Stopped,
                ] {
                    for (halted, feed_current, kind) in (0..16_u8)
                        .map(|bits| -> Result<_, GateError> {
                            let kind = match bits >> 2 {
                                0 => ProposedKind::Plain,
                                1 => ProposedKind::Market,
                                2 => ProposedKind::Ioc,
                                _ => ProposedKind::Bracket {
                                    take_profit: Price::parse("110")?,
                                    stop: Price::parse("90")?,
                                },
                            };
                            Ok((bits & 1 == 1, bits & 2 == 2, kind))
                        })
                        .collect::<Result<Vec<_>, _>>()?
                    {
                        let mut o = match side {
                            Side::Buy => allowing()?,
                            Side::Sell => allowing()?.selling(origin)?,
                        };
                        o.proposed.origin = origin;
                        o.proposed.limit_price = Price::parse("99.5")?;
                        o.agent.mode = mode;
                        o.instrument.halted = halted;
                        o.instrument.status_feed_current = feed_current;
                        o.proposed.kind = kind.clone();
                        let expected =
                            halt_oracle(side, origin, mode, (halted, feed_current), &o.proposed);
                        let actual: HaltRow = match o.decide() {
                            Ok(d) => Ok((
                                d.verdict,
                                d.reason,
                                d.checks.iter().find_map(|c| match c {
                                    CheckOutcome::Failed(check, _) => Some(*check),
                                    CheckOutcome::Passed(_) | CheckOutcome::NotReached(_) => None,
                                }),
                                d.pacing,
                            )),
                            Err(GateError::Unimplemented("evaluate", story)) => Err(story),
                            Err(e) => return Err(e),
                        };
                        assert_eq!(
                            actual, expected,
                            "{side:?} from {origin:?} under {mode:?}, halted {halted}, feed \
                             current {feed_current}, {kind:?}"
                        );
                        rows = rows.saturating_add(1);
                    }
                }
            }
        }
        assert_eq!(rows, 1280, "every row of the table ran");
        Ok(())
    }

    /// One §3.2 row: every input the floor reads, each over the values that sit on, just past and
    /// well inside its bound, and absent.
    #[derive(Debug, Clone, Copy)]
    struct FloorRow {
        /// Whether the organization's floors are set **below** §3.2's platform minimums. The
        /// levels below are read against whichever floor binds, so a row means the same thing
        /// either way: level 1 sits exactly on the binding floor and level 2 one unit under it.
        weak_config: bool,
        crypto: bool,
        status: u8,
        exchange: u8,
        item3: u8,
        price: u8,
        volume_20d: u8,
        etp: EtpClass,
        permission: u8,
        classified: u8,
        volume_30d: u8,
    }

    /// 0 comfortably passes, 1 sits exactly on the floor (passes), 2 is one unit under it, 3 is
    /// absent.
    fn figure(level: u8, ok: &str, exact: &str, under: &str) -> Result<Option<Usd>, GateError> {
        Ok(match level {
            0 => Some(Usd::parse(ok)?),
            1 => Some(Usd::parse(exact)?),
            2 => Some(Usd::parse(under)?),
            _ => None,
        })
    }

    /// The floor's oracle, transcribed from §3.2's list and the three readings the PR raises:
    /// `status`/`tradable` report `not_in_working_universe`, `ptp_no_exception` reports
    /// `ipo_not_tradable`, the price and 20-day volume floors bind US equities and the 30-day one
    /// crypto; and item 6 fails closed on an unclassified ETP and on a stale or undated
    /// classification.
    fn floor_oracle(r: FloorRow) -> Option<ReasonCode> {
        let fails = |level: u8| level >= 2;
        if r.status != 0 {
            return Some(ReasonCode::NotInWorkingUniverse);
        }
        if !r.crypto && r.exchange != 0 {
            return Some(ReasonCode::IneligibleExchange);
        }
        if r.item3 != 0 {
            return Some(ReasonCode::IpoNotTradable);
        }
        if r.crypto {
            return fails(r.volume_30d).then_some(ReasonCode::BelowLiquidityFloor);
        }
        if fails(r.price) {
            return Some(ReasonCode::BelowPriceFloor);
        }
        if fails(r.volume_20d) {
            return Some(ReasonCode::BelowLiquidityFloor);
        }
        let permitted = r.permission == 0;
        let complex = matches!(r.etp, EtpClass::Complex | EtpClass::Unclassified);
        (fails(r.classified) || (complex && !permitted))
            .then_some(ReasonCode::LeveragedEtpNotEnabled)
    }

    /// The floor's verdict for a scenario built directly, rather than from a [`FloorRow`].
    fn floor(o: &Owned) -> Result<Option<ReasonCode>, GateError> {
        o.with_input(floor::eligibility)?.map(|stop| {
            stop.map(|(verdict, code)| {
                assert_eq!(verdict, Verdict::Deny, "the floor only ever denies");
                code
            })
        })
    }

    fn floor_of(o: &mut Owned, r: FloorRow) -> Result<Option<ReasonCode>, GateError> {
        let now = UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?;
        let i = &mut o.instrument;
        i.asset_class = if r.crypto {
            AssetClass::Crypto
        } else {
            AssetClass::UsEquity
        };
        i.status_active = r.status != 1;
        i.tradable = r.status != 2;
        i.exchange = match r.exchange {
            0 => Some(crate::Exchange::Nasdaq),
            1 => Some(crate::Exchange::Otc),
            _ => None,
        };
        i.ipo = r.item3 == 1;
        i.ptp_no_exception = r.item3 == 2;
        let (price_floor, equity_floor, crypto_floor) = if r.weak_config {
            ("0.5", "1", "1")
        } else {
            ("5", "1000000", "2000000")
        };
        o.config.price_floor = Usd::parse(price_floor)?;
        o.config.liquidity_floor_usd = Usd::parse(equity_floor)?;
        o.config.crypto_liquidity_floor_usd = Usd::parse(crypto_floor)?;
        let i = &mut o.instrument;
        i.prior_close = match (r.price, r.weak_config) {
            (0, _) => Some(Price::parse("100")?),
            (1, false) => Some(Price::parse("5")?),
            (2, false) => Some(Price::parse("4.999999999")?),
            (1, true) => Some(Price::parse("1")?),
            (2, true) => Some(Price::parse("0.999999999")?),
            _ => None,
        };
        i.median_dollar_volume_20d =
            figure(r.volume_20d, "90000000", "1000000", "999999.999999999")?;
        i.median_dollar_volume_30d = if r.weak_config {
            figure(r.volume_30d, "90000000", "1000000", "999999.999999999")?
        } else {
            figure(r.volume_30d, "90000000", "2000000", "1999999.999999999")?
        };
        i.etp = r.etp;
        let age = match r.classified {
            0 => Some(1),
            1 => Some(604_800),
            2 => Some(604_801),
            _ => None,
        };
        i.etp_classified_at = age
            .map(|secs| {
                let then = now
                    .secs()
                    .checked_sub(secs)
                    .ok_or(GateError::ConfigOutOfRange)?;
                Ok::<_, GateError>(UtcNanos::from_parts(then, 0)?)
            })
            .transpose()?;
        o.mandate = ValidatedMandate::from_validated_parts(
            o.mandate.risk().clone(),
            GoalState::Running,
            matches!(r.permission, 0 | 1),
            matches!(r.permission, 0 | 2),
        );
        o.with_input(floor::eligibility)?.map(|stop| {
            stop.map(|(verdict, code)| {
                assert_eq!(verdict, Verdict::Deny, "the floor only ever denies");
                code
            })
        })
    }

    /// Every row of §3.2 against [`floor_oracle`]: 2 configs (compliant, and below every platform
    /// minimum) × 2 asset classes × 3 statuses × 3 exchanges × 3 item-3 states × 4 prices × 4
    /// 20-day volumes × 3 ETP classes × 4 permissions × 4 classification ages × 4 30-day volumes =
    /// 331,776 rows. It pins the platform minimums binding over a weaker setting, the list order,
    /// `≥` at every floor, an absent figure failing, the ETP rule needing both the mandate's switch
    /// and the disclosure, the classification age failing closed only when strictly older, and
    /// which items bind which asset class.
    #[test]
    fn the_floor_matches_the_oracle_on_every_row() -> Result<(), GateError> {
        let mut o = allowing()?;
        let mut rows = 0_u32;
        let mut denied = BTreeSet::new();
        for weak_config in [false, true] {
            for crypto in [false, true] {
                for status in 0..3 {
                    for exchange in 0..3 {
                        for item3 in 0..3 {
                            for price in 0..4 {
                                for volume_20d in 0..4 {
                                    for etp in
                                        [EtpClass::Plain, EtpClass::Complex, EtpClass::Unclassified]
                                    {
                                        for permission in 0..4 {
                                            for classified in 0..4 {
                                                for volume_30d in 0..4 {
                                                    let r = FloorRow {
                                                        weak_config,
                                                        crypto,
                                                        status,
                                                        exchange,
                                                        item3,
                                                        price,
                                                        volume_20d,
                                                        etp,
                                                        permission,
                                                        classified,
                                                        volume_30d,
                                                    };
                                                    let got = floor_of(&mut o, r)?;
                                                    assert_eq!(got, floor_oracle(r), "{r:?}");
                                                    denied.extend(got);
                                                    rows = rows.saturating_add(1);
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            (rows, denied.len()),
            (331_776, 6),
            "every row ran under both a compliant and a below-minimum config, and every floor \
             code was reached"
        );
        Ok(())
    }

    /// A crypto opening stays owed at check 2 until E6-10 supplies the quote currency, while a
    /// crypto exit is untouched (DEC-129 item 34).
    ///
    /// §3.2 item 7 admits USD pairs only and nothing in `InstrumentSnapshot` says what a pair is
    /// quoted in, so calling check 2 whole for crypto would let a stablecoin pair open the moment
    /// E6-8 lands. The refusal names E6-10 rather than E6-8, which is what distinguishes
    /// this from the ordinary fail-closed refusal every opening gets today.
    #[test]
    fn a_crypto_opening_is_owed_to_e6_10_while_a_crypto_exit_is_not() -> Result<(), GateError> {
        let mut o = allowing()?;
        o.instrument.asset_class = AssetClass::Crypto;
        o.instrument.exchange = None;
        o.instrument.median_dollar_volume_30d = Some(Usd::parse("90000000")?);
        o.account.crypto_active = true;
        assert!(
            matches!(
                o.with_input(evaluate)?,
                Err(GateError::Unimplemented("evaluate", "E6-10"))
            ),
            "a crypto opening past the floor is owed E6-10's USD-pair check, not allowed"
        );

        let equity = allowing()?;
        assert!(
            matches!(
                equity.with_input(evaluate)?,
                Err(GateError::Unimplemented("evaluate", story)) if story != "E6-10"
            ),
            "a US equity is not owed E6-10: check 2 is whole for it"
        );

        o.agent
            .positions
            .insert(o.proposed.instrument.clone(), Qty::parse("10")?);
        o.proposed.side = Side::Sell;
        o.proposed.origin = Origin::RiskEngine;
        let exit = o.with_input(evaluate)??;
        assert_eq!(
            exit.verdict,
            Verdict::Allow,
            "a crypto exit is never owed: first_owed accrues only for an opening"
        );
        assert_eq!(
            exit.checks.get(..2),
            Some(
                &[
                    CheckOutcome::Passed(Check::AccountAndMode),
                    CheckOutcome::NotReached(Check::UniverseAndLimits),
                ][..]
            ),
            "E6-10 owes check 2 alone for crypto: check 1 is still run and recorded as passed, and \
             check 2 is recorded as not reached rather than passed"
        );
        Ok(())
    }

    /// §3.2's platform minimums bind whatever the organization set: price 1.00, and 1,000,000 for
    /// both the 20-day equity volume and the 30-day crypto volume.
    ///
    /// `GateConfig`'s fields are public and nothing in the repo validates them, so a config below a
    /// platform minimum is representable and the gate has to refuse it on its own. Review round 1
    /// found two substitutions that dropped the minimum surviving the whole suite, because the
    /// floor table's config then sat at or above every minimum. The table now also runs every row
    /// under a below-minimum config (331,776 rows) and catches each such plant by itself; this test
    /// is the second, readable pin on the reviewer's own probes.
    ///
    /// Each pair below is the reviewer's own probe and the value one unit above it: the weakened
    /// config would have admitted the first, and the platform minimum denies it, while the second
    /// shows the minimum is the floor rather than a blanket refusal.
    #[test]
    fn a_config_below_a_platform_minimum_does_not_weaken_the_floor() -> Result<(), GateError> {
        let weak = |o: &mut Owned| -> Result<(), GateError> {
            o.config.price_floor = Usd::parse("0.5")?;
            o.config.liquidity_floor_usd = Usd::parse("1")?;
            o.config.crypto_liquidity_floor_usd = Usd::parse("1")?;
            Ok(())
        };

        let mut o = allowing()?;
        weak(&mut o)?;
        o.instrument.prior_close = Some(Price::parse("0.6")?);
        assert_eq!(
            floor(&o)?,
            Some(ReasonCode::BelowPriceFloor),
            "0.6 clears a 0.5 setting but not the 1.00 platform minimum"
        );
        o.instrument.prior_close = Some(Price::parse("1")?);
        assert_eq!(
            floor(&o)?,
            None,
            "exactly at the 1.00 platform minimum passes, so the minimum is a floor not a ban"
        );

        let mut o = allowing()?;
        weak(&mut o)?;
        o.instrument.median_dollar_volume_20d = Some(Usd::parse("10")?);
        assert_eq!(
            floor(&o)?,
            Some(ReasonCode::BelowLiquidityFloor),
            "10 clears a 1 setting but not the 1000000 platform minimum"
        );
        o.instrument.median_dollar_volume_20d = Some(Usd::parse("1000000")?);
        assert_eq!(floor(&o)?, None, "exactly at the platform minimum passes");

        let mut o = allowing()?;
        weak(&mut o)?;
        o.instrument.asset_class = AssetClass::Crypto;
        o.instrument.median_dollar_volume_30d = Some(Usd::parse("10")?);
        assert_eq!(
            floor(&o)?,
            Some(ReasonCode::BelowLiquidityFloor),
            "the crypto floor has the same 1000000 minimum, and its own setting cannot lower it"
        );
        o.instrument.median_dollar_volume_30d = Some(Usd::parse("1000000")?);
        assert_eq!(floor(&o)?, None, "exactly at the platform minimum passes");
        Ok(())
    }

    /// The floor sits inside check 2, after the working universe and before concentration, and
    /// never touches an exit: outside the universe on an OTC listing reports the universe; an OTC
    /// listing over the cap reports the exchange; and a risk exit in an instrument failing every
    /// item is allowed.
    #[test]
    fn the_floor_runs_after_the_universe_before_concentration_and_never_on_an_exit()
    -> Result<(), GateError> {
        let otc = |mut o: Owned| {
            o.instrument.exchange = Some(crate::Exchange::Otc);
            o.instrument.status_active = false;
            o.instrument.prior_close = None;
            o
        };
        let outside = otc(outside_the_universe(allowing()?)).decide()?;
        let mut over_cap = otc(allowing()?);
        over_cap.instrument.status_active = true;
        over_cap.proposed.qty = Qty::parse("100")?;
        let over_cap = over_cap.decide()?;
        let exit = otc(allowing()?.selling(Origin::RiskEngine)?).decide()?;
        assert_eq!(
            [
                (
                    outside.verdict,
                    outside.reason,
                    outside.checks.get(1).cloned()
                ),
                (
                    over_cap.verdict,
                    over_cap.reason,
                    over_cap.checks.get(1).cloned()
                ),
                (exit.verdict, exit.reason, None),
            ],
            [
                (
                    Verdict::Deny,
                    Some(ReasonCode::NotInWorkingUniverse),
                    Some(CheckOutcome::Failed(
                        Check::UniverseAndLimits,
                        ReasonCode::NotInWorkingUniverse
                    ))
                ),
                (
                    Verdict::Deny,
                    Some(ReasonCode::IneligibleExchange),
                    Some(CheckOutcome::Failed(
                        Check::UniverseAndLimits,
                        ReasonCode::IneligibleExchange
                    ))
                ),
                (Verdict::Allow, None, None),
            ],
            "universe, then floor, then concentration; an exit is allowed regardless of the floor"
        );
        assert_eq!(
            over_cap.computed.cap, None,
            "the floor denied before concentration computed its cap"
        );
        Ok(())
    }

    /// One way to fail check 2: its name, how it changes the scenario, and the code it must report.
    type Breach = (&'static str, fn(&mut Owned), ReasonCode);

    /// Every way an instrument fails check 2 ahead of concentration, one breach each: the working
    /// universe, then §3.2's items in list order. The expected code is written out here rather
    /// than read from the floor, which is the thing under test.
    fn floor_breaches() -> [Breach; 15] {
        [
            (
                "outside the working universe",
                |o| {
                    o.universe = WorkingUniverse::Known {
                        instruments: BTreeSet::new(),
                        pinned: true,
                    }
                },
                ReasonCode::NotInWorkingUniverse,
            ),
            (
                "status not active",
                |o| o.instrument.status_active = false,
                ReasonCode::NotInWorkingUniverse,
            ),
            (
                "not tradable",
                |o| o.instrument.tradable = false,
                ReasonCode::NotInWorkingUniverse,
            ),
            (
                "an OTC listing",
                |o| o.instrument.exchange = Some(crate::Exchange::Otc),
                ReasonCode::IneligibleExchange,
            ),
            (
                "an equity with no exchange",
                |o| o.instrument.exchange = None,
                ReasonCode::IneligibleExchange,
            ),
            (
                "IPO day",
                |o| o.instrument.ipo = true,
                ReasonCode::IpoNotTradable,
            ),
            (
                "a PTP without the exception",
                |o| o.instrument.ptp_no_exception = true,
                ReasonCode::IpoNotTradable,
            ),
            (
                "a prior close under the price floor",
                |o| o.instrument.prior_close = Price::parse("4.99").ok(),
                ReasonCode::BelowPriceFloor,
            ),
            (
                "no prior close",
                |o| o.instrument.prior_close = None,
                ReasonCode::BelowPriceFloor,
            ),
            (
                "a 20-day volume under the liquidity floor",
                |o| o.instrument.median_dollar_volume_20d = Usd::parse("999999.99").ok(),
                ReasonCode::BelowLiquidityFloor,
            ),
            (
                "no 20-day volume",
                |o| o.instrument.median_dollar_volume_20d = None,
                ReasonCode::BelowLiquidityFloor,
            ),
            (
                "a complex ETP without the opt-in",
                |o| o.instrument.etp = EtpClass::Complex,
                ReasonCode::LeveragedEtpNotEnabled,
            ),
            (
                "an unclassified ETP",
                |o| o.instrument.etp = EtpClass::Unclassified,
                ReasonCode::LeveragedEtpNotEnabled,
            ),
            (
                "no classification date",
                |o| o.instrument.etp_classified_at = None,
                ReasonCode::LeveragedEtpNotEnabled,
            ),
            (
                "a crypto pair under the 30-day liquidity floor",
                |o| {
                    o.instrument.asset_class = AssetClass::Crypto;
                    o.instrument.exchange = None;
                    o.instrument.median_dollar_volume_30d = Usd::parse("1999999.99").ok();
                },
                ReasonCode::BelowLiquidityFloor,
            ),
        ]
    }

    /// §3.2's last paragraph and `AGENTS.md` rule 13, probed for every origin a sell within the
    /// position can come from, each typed by the brief's purpose table rather than by
    /// `assign_purpose`: no breach of the universe or the floor denies an exit, and check 2 is
    /// listed `Passed` for it, or `NotReached` for crypto, whose check 2 stays owed to E6-10 for
    /// every purpose until the quote currency is an input (DEC-129 items 29 and 34). Each breach
    /// first denies an opening with its own code at check 2, so an exit's allow is never an
    /// instrument that happens to pass the floor.
    #[test]
    fn no_floor_breach_denies_an_exit_from_any_origin() -> Result<(), GateError> {
        let exits = [
            (Origin::OrderBuilder, Purpose::DiscretionaryExit),
            (Origin::GoalCompletion, Purpose::DiscretionaryExit),
            (Origin::RemovedInstrument, Purpose::DiscretionaryExit),
            (Origin::RiskEngine, Purpose::RiskExit),
            (Origin::TrimToTarget, Purpose::RiskExit),
            (Origin::StopWatchdog, Purpose::RiskExit),
            (Origin::AutomatedKillSwitch, Purpose::RiskExit),
            (Origin::OwnerClose, Purpose::OwnerExit),
            (Origin::OwnerKillSwitch, Purpose::OwnerExit),
            (Origin::ProtectiveLeg, Purpose::Protective),
        ];
        for (breach, apply, code) in floor_breaches() {
            let mut opening = allowing()?;
            apply(&mut opening);
            let d = opening.decide()?;
            assert_eq!(
                (d.verdict, d.reason, d.checks.get(1).cloned()),
                (
                    Verdict::Deny,
                    Some(code),
                    Some(CheckOutcome::Failed(Check::UniverseAndLimits, code))
                ),
                "{breach}: an opening is denied at check 2"
            );
            for (origin, purpose) in exits {
                let mut exit = allowing()?.selling(origin)?;
                apply(&mut exit);
                let e = exit.decide()?;
                let listed = if exit.instrument.asset_class == AssetClass::Crypto {
                    CheckOutcome::NotReached(Check::UniverseAndLimits)
                } else {
                    CheckOutcome::Passed(Check::UniverseAndLimits)
                };
                assert_eq!(
                    (e.verdict, e.reason, e.purpose, e.checks.get(1).cloned()),
                    (Verdict::Allow, None, purpose, Some(listed)),
                    "{breach}: a sell from {origin:?} is allowed regardless of the floor"
                );
            }
        }
        Ok(())
    }

    /// §5.3 dates the cooldown from the *last* exit in the group: with exits in `a` 3000 s ago and
    /// in `b` 100 s ago, both in group 7, the denial reports `b`'s.
    #[test]
    fn the_cooldown_reports_the_last_exit_in_the_group() -> Result<(), GateError> {
        let now = UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?;
        let (a, b) = (id("a")?, id("b")?);
        let mut o = allowing()?;
        o.agent.instrument_groups = [(a.clone(), GroupId(7)), (b.clone(), GroupId(7))].into();
        let recent = UtcNanos::from_parts(now.secs() - 100, 0)?;
        o.agent.last_exit_fill_at = [
            (a, UtcNanos::from_parts(now.secs() - 3000, 0)?),
            (b.clone(), recent),
        ]
        .into();
        let d = o.decide()?;
        assert_eq!(
            (
                d.reason,
                d.computed.instrument,
                d.computed.last_exit_fill_at
            ),
            (Some(ReasonCode::ReentryCooldown), Some(b), Some(recent)),
            "the binding exit is the latest one in the group"
        );
        Ok(())
    }

    /// A decision as its verdict and code, or the story a refusal names.
    type Row = Result<(Verdict, Option<ReasonCode>), &'static str>;

    fn row(o: &Owned) -> Result<Row, GateError> {
        match o.decide() {
            Ok(d) => Ok(Ok((d.verdict, d.reason))),
            Err(GateError::Unimplemented(_, story)) => Ok(Err(story)),
            Err(e) => Err(e),
        }
    }

    fn at(text: &str) -> Result<UtcNanos, GateError> {
        Ok(UtcNanos::parse_rfc3339(text)?)
    }

    /// A working order in the proposed instrument `a`, submitted on the fixture's day.
    fn working(
        agent: u64,
        side: Side,
        protective: bool,
        opening: bool,
    ) -> Result<WorkingOrder, GateError> {
        Ok(WorkingOrder {
            agent: AgentId(agent),
            instrument: id("a")?,
            side,
            max_cost: Usd::ZERO,
            open_qty: Qty::parse("3")?,
            protective,
            opening,
            submitted_on: mandate_time::Date::parse("2026-09-21")?,
        })
    }

    /// Adds `order` to the account as client order 7, and to the agent's own orders when `mine`.
    fn with_order(mut o: Owned, order: WorkingOrder, mine: bool) -> Owned {
        o.account.working_orders.insert(ClientOrderId(7), order);
        if mine {
            o.agent.working_orders.insert(ClientOrderId(7));
        }
        o
    }

    /// §5.3 rules 5 and 6 are §9.6 conduct controls, which deny openings only (DEC-150 item 2):
    /// another working non-protective order in the instrument denies an opening
    /// `working_order_limit`, while a risk exit and a discretionary exit pass it at both passes.
    #[test]
    fn one_working_order_binds_an_opening_and_never_an_exit() -> Result<(), GateError> {
        let others = working(2, Side::Buy, false, true)?;
        let opening = with_order(allowing()?, others.clone(), false);
        let mut risk = with_order(
            allowing()?.selling(Origin::RiskEngine)?,
            others.clone(),
            false,
        );
        risk.pass = GatePass::BeforeSubmission;
        let mut discretionary = with_order(
            allowing()?.selling(Origin::OrderBuilder)?,
            others.clone(),
            false,
        );
        discretionary.pass = GatePass::BeforeSubmission;
        let mut sell_resting = others;
        sell_resting.side = Side::Sell;
        let one_side = with_order(allowing()?, sell_resting, true);
        assert_eq!(
            [
                row(&opening)?,
                row(&risk)?,
                row(&discretionary)?,
                row(&one_side)?
            ],
            [
                Ok((Verdict::Deny, Some(ReasonCode::WorkingOrderLimit))),
                Ok((Verdict::Allow, None)),
                Ok((Verdict::Allow, None)),
                Ok((Verdict::Deny, Some(ReasonCode::WorkingOrderLimit))),
            ],
            "another agent's buy denies an opening but no exit; the agent's own resting sell \
             denies its buy (one side at a time)"
        );
        Ok(())
    }

    /// §5.4 and §5.3 rule 8: a plain equity opening beside a resting protective order is
    /// `add_blocked_by_protective_order`, an IOC included; a bracket is its own tranche and passes,
    /// and crypto follows DEC-36's sequence, so it is not blocked either.
    #[test]
    fn a_protective_order_blocks_only_a_plain_equity_add() -> Result<(), GateError> {
        let protection = working(1, Side::Sell, true, false)?;
        let plain = with_order(allowing()?, protection.clone(), true);
        let mut ioc = with_order(allowing()?, protection.clone(), true);
        ioc.proposed.kind = ProposedKind::Ioc;
        let mut bracket = with_order(allowing()?, protection.clone(), true);
        bracket.proposed.kind = ProposedKind::Bracket {
            take_profit: Price::parse("110")?,
            stop: Price::parse("90")?,
        };
        let mut crypto = with_order(allowing()?, protection, true);
        crypto.instrument.asset_class = AssetClass::Crypto;
        crypto.instrument.exchange = None;
        crypto.instrument.median_dollar_volume_30d = Some(Usd::parse("90000000")?);
        assert_eq!(
            [row(&plain)?, row(&ioc)?, row(&bracket)?, row(&crypto)?],
            [
                Ok((Verdict::Deny, Some(ReasonCode::AddBlockedByProtectiveOrder))),
                Ok((Verdict::Deny, Some(ReasonCode::AddBlockedByProtectiveOrder))),
                Err("E6-8"),
                Err("E6-10"),
            ],
            "only a plain or IOC equity add is blocked by resting protection"
        );
        Ok(())
    }

    /// §5.3 rule 4: the sell plus the agent's own open sells in the instrument is at most the
    /// position of 10. The agent's own open sell of 3 counts; another agent's, the agent's own buy,
    /// and a sell in another instrument do not; 7 more fits and 8 does not.
    #[test]
    fn a_sell_counts_the_agents_own_open_sells() -> Result<(), GateError> {
        let sell = |qty: &str, order: WorkingOrder, mine: bool| -> Result<Row, GateError> {
            let mut o = with_order(allowing()?.selling(Origin::RiskEngine)?, order, mine);
            o.proposed.qty = Qty::parse(qty)?;
            row(&o)
        };
        let own_sell = working(1, Side::Sell, false, false)?;
        let mut elsewhere = own_sell.clone();
        elsewhere.instrument = id("b")?;
        let exceeds = Ok((Verdict::Deny, Some(ReasonCode::SellExceedsAvailable)));
        let allowed = Ok((Verdict::Allow, None));
        assert_eq!(
            [
                sell("7", own_sell.clone(), true)?,
                sell("8", own_sell.clone(), true)?,
                sell("8", working(2, Side::Sell, false, false)?, false)?,
                sell("10", working(1, Side::Buy, false, false)?, true)?,
                sell("8", elsewhere, true)?,
            ],
            [allowed, exceeds, allowed, allowed, allowed],
            "7 + 3 is the position exactly; 8 + 3 is over it; nothing else counts"
        );
        Ok(())
    }

    /// §5.3 rules 2 and 7 have no registered code, so an opening that breaks one is refused, never
    /// allowed, and an exit is never refused for them (DEC-129 item 27, DEC-150 item 4).
    #[test]
    fn the_unregistered_rules_refuse_an_opening_and_never_an_exit() -> Result<(), GateError> {
        let buying =
            |qty: &str, fractionable: bool, tif: TimeInForce| -> Result<Owned, GateError> {
                let mut o = allowing()?;
                o.proposed.qty = Qty::parse(qty)?;
                o.instrument.min_order_size = Qty::parse("0.5")?;
                o.instrument.fractionable = fractionable;
                o.proposed.tif = tif;
                Ok(o)
            };
        let mut bracket = buying("1.5", true, TimeInForce::Day)?;
        bracket.proposed.kind = ProposedKind::Bracket {
            take_profit: Price::parse("110")?,
            stop: Price::parse("90")?,
        };
        let mut small_exit = allowing()?.selling(Origin::OrderBuilder)?;
        small_exit.proposed.qty = Qty::parse("0.4")?;
        small_exit.instrument.min_order_size = Qty::parse("0.5")?;
        let mut crypto = buying("1.5", false, TimeInForce::Gtc)?;
        crypto.instrument.asset_class = AssetClass::Crypto;
        crypto.instrument.exchange = None;
        crypto.instrument.median_dollar_volume_30d = Some(Usd::parse("90000000")?);
        let item_27 = Err("DEC-129 item 27");
        assert_eq!(
            [
                row(&buying("0.4", true, TimeInForce::Day)?)?,
                row(&buying("0.5", true, TimeInForce::Day)?)?,
                row(&buying("1.5", false, TimeInForce::Day)?)?,
                row(&buying("1.5", true, TimeInForce::Gtc)?)?,
                row(&bracket)?,
                row(&buying("1.5", true, TimeInForce::Day)?)?,
                row(&buying("2", false, TimeInForce::Gtc)?)?,
                row(&small_exit)?,
                row(&crypto)?,
            ],
            [
                item_27,
                Err("E6-8"),
                item_27,
                item_27,
                item_27,
                Err("E6-8"),
                Err("E6-8"),
                Ok((Verdict::Allow, None)),
                Err("E6-10"),
            ],
            "below the minimum, fractional in a whole-share instrument, fractional GTC and a \
             fractional bracket are refused; the minimum itself, a fractional day order and whole \
             shares pass; a small exit is allowed; crypto has no share increment"
        );
        Ok(())
    }

    /// Check 7: `qty × limit + fee ≤ min(model, broker)`, with the broker's non-marginable figure
    /// for crypto and its buying power for an equity; a negative fee reservation counts as zero; an
    /// exit never meets buying power.
    #[test]
    fn buying_power_bounds_an_opening_only() -> Result<(), GateError> {
        let with = |model: &str, broker: &str, non_marginable: &str, fee: &str| {
            let (model, broker, non_marginable, fee) = (
                Usd::parse(model),
                Usd::parse(broker),
                Usd::parse(non_marginable),
                Usd::parse(fee),
            );
            move |o: &mut Owned| -> Result<(), GateError> {
                o.account.model_buying_power = model?;
                o.account.broker_buying_power = broker?;
                o.account.broker_non_marginable_buying_power = non_marginable?;
                o.proposed.fee_reservation = fee?;
                Ok(())
            }
        };
        let crypto = |o: &mut Owned| -> Result<(), GateError> {
            o.instrument.asset_class = AssetClass::Crypto;
            o.instrument.exchange = None;
            o.instrument.median_dollar_volume_30d = Some(Usd::parse("90000000")?);
            Ok(())
        };
        let mut exit = allowing()?.selling(Origin::OrderBuilder)?;
        with("0", "0", "0", "0")(&mut exit)?;
        let short = Ok(ReasonCode::InsufficientBuyingPower);
        assert_eq!(
            [
                limit_row(with("100", "100", "0", "0"))?,
                limit_row(with("99", "99", "0", "-5"))?,
                limit_row(with("100", "99.99", "1000", "0"))?,
                limit_row(with("99.99", "100", "1000", "0"))?,
                limit_row(|o| {
                    crypto(o)?;
                    with("1000", "1000", "99.99", "0")(o)
                })?,
                limit_row(|o| {
                    crypto(o)?;
                    with("1000", "99.99", "100", "0")(o)
                })?,
            ],
            [Err("E6-8"), short, short, short, short, Err("E6-10")],
            "1 × 100 at exactly 100 passes, a fee of -5 cannot bring 100 within 99, the lower figure \
             binds, and crypto reads the non-marginable figure rather than the marginable one"
        );
        assert_eq!(
            row(&exit)?,
            Ok((Verdict::Allow, None)),
            "buying power never denies an exit"
        );
        Ok(())
    }

    /// Check 8's `required` and where the budget applies: a margin account under `legacy_pdt`
    /// below the threshold, for a US-equity opening. Window count 2 leaves remaining 1, so one more
    /// component of `required` denies. A working same-day opening order elsewhere on the account
    /// counts, whoever placed it; a protective order, a closing order and yesterday's order do
    /// not.
    #[test]
    fn the_day_trade_budget_counts_the_account_and_binds_only_openings() -> Result<(), GateError> {
        let pdt = |edit: &dyn Fn(&mut Owned) -> Result<(), GateError>| -> Result<Row, GateError> {
            let mut o = allowing()?;
            o.account.prior_close_equity = Usd::parse("10000")?;
            o.agent.day_trades.window_count = 2;
            edit(&mut o)?;
            row(&o)
        };
        let elsewhere = |protective: bool, opening: bool, day: &'static str| {
            move |o: &mut Owned| -> Result<(), GateError> {
                let mut w = working(2, Side::Buy, protective, opening)?;
                w.instrument = id("b")?;
                w.submitted_on = mandate_time::Date::parse(day)?;
                o.account.working_orders.insert(ClientOrderId(8), w);
                Ok(())
            }
        };
        let budget = Ok((Verdict::Deny, Some(ReasonCode::LegacyPdtDayTradeBudget)));
        assert_eq!(
            [
                pdt(&|_| Ok(()))?,
                pdt(&elsewhere(false, true, "2026-09-21"))?,
                pdt(&|o| {
                    o.agent.day_trades.open_same_day_positions = [id("b")?].into();
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.sold_earlier_today = [id("a")?].into();
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.sold_earlier_today = [id("b")?].into();
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.window_count = 0;
                    o.agent.day_trades.flagged_pattern_day_trader = true;
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.window_count = 3;
                    Ok(())
                })?,
                pdt(&elsewhere(true, true, "2026-09-21"))?,
                pdt(&elsewhere(false, false, "2026-09-21"))?,
                pdt(&elsewhere(false, true, "2026-09-18"))?,
                pdt(&|o| {
                    o.agent.day_trades.window_count = 7;
                    o.account.prior_close_equity = Usd::parse("25000")?;
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.window_count = 7;
                    o.account.account_type = AccountType::Cash;
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.window_count = 7;
                    o.account.regime = crate::DayTradeRegime::IntradayMargin {
                        maintenance_excess: Usd::ZERO,
                    };
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.window_count = 7;
                    o.instrument.asset_class = AssetClass::Crypto;
                    o.instrument.exchange = None;
                    o.instrument.median_dollar_volume_30d = Some(Usd::parse("90000000")?);
                    Ok(())
                })?,
                pdt(&|o| {
                    o.agent.day_trades.window_count = 7;
                    o.agent.positions.insert(id("a")?, Qty::parse("10")?);
                    o.proposed.side = Side::Sell;
                    Ok(())
                })?,
            ],
            [
                Err("E6-8"),
                budget,
                budget,
                budget,
                Err("E6-8"),
                budget,
                budget,
                Err("E6-8"),
                Err("E6-8"),
                Err("E6-8"),
                Err("E6-8"),
                Err("E6-8"),
                Err("E6-8"),
                Err("E6-10"),
                Ok((Verdict::Allow, None)),
            ],
            "another agent's same-day opening elsewhere, an open same-day position and a sale of \
             this security earlier today each make required 2 (a sale of another does not); a \
             flagged account has remaining 0 and three day trades leave 0; protective, closing and \
             earlier orders do not count; at the threshold, in a cash account, under \
             intraday_margin and for crypto nothing is counted; an exit is never denied for the \
             count"
        );
        Ok(())
    }

    /// The session is derived from the committed calendar: the opening auction is the last two
    /// minutes of pre-market, the close window the last `close_window_minutes` of the regular
    /// session, a Saturday is `Overnight`, crypto is `Continuous`, and a date the calendar does not
    /// cover is `ConfigOutOfRange`, never a guess.
    #[test]
    fn the_session_comes_from_the_calendar() -> Result<(), GateError> {
        let config = allowing()?.config;
        let equity = |t: &str| crate::session_at(at(t)?, &config, AssetClass::UsEquity);
        let flags = |t: &str| -> Result<_, GateError> {
            let s = equity(t)?;
            Ok((s.session, s.opening_auction, s.close_window))
        };
        assert_eq!(
            [
                flags("2026-09-21T13:27:59Z")?,
                flags("2026-09-21T13:28:00Z")?,
                flags("2026-09-21T13:30:00Z")?,
                flags("2026-09-21T19:49:59Z")?,
                flags("2026-09-21T19:50:00Z")?,
                flags("2026-09-21T19:58:00Z")?,
                flags("2026-09-21T20:00:00Z")?,
                flags("2026-09-21T23:58:00Z")?,
                flags("2026-09-22T01:00:00Z")?,
                flags("2026-09-19T15:00:00Z")?,
            ],
            [
                (Session::PreMarket, false, false),
                (Session::PreMarket, true, false),
                (Session::Regular, false, false),
                (Session::Regular, false, false),
                (Session::Regular, false, true),
                (Session::Regular, false, true),
                (Session::AfterHours, false, false),
                (Session::AfterHours, false, false),
                (Session::Overnight, false, false),
                (Session::Overnight, false, false),
            ],
            "09:27:59 and 09:28 ET, the open, 15:49:59, 15:50 and 15:58 (the close window, never \
             the opening auction), the close, 19:58 (neither, though two minutes from a session's \
             end), 21:00 ET, a Saturday"
        );
        let regular = equity("2026-09-21T15:00:00Z")?;
        assert_eq!(
            (regular.start, regular.end),
            (at("2026-09-21T13:30:00Z")?, at("2026-09-21T20:00:00Z")?),
            "the regular session runs 09:30 to 16:00 ET"
        );
        let crypto = crate::session_at(at("2026-09-19T15:00:00Z")?, &config, AssetClass::Crypto)?;
        assert_eq!(
            crypto.session,
            Session::Continuous,
            "crypto trades continuously"
        );
        let mut no_window = config.clone();
        no_window.close_window_minutes = 0;
        let last = crate::session_at(
            at("2026-09-21T19:59:59Z")?,
            &no_window,
            AssetClass::UsEquity,
        )?;
        assert!(!last.close_window, "a zero-minute close window never opens");
        assert!(
            matches!(
                equity("2040-01-02T15:00:00Z"),
                Err(GateError::ConfigOutOfRange)
            ),
            "a date past the calendar is out of range"
        );
        Ok(())
    }

    /// Check 3 outside the regular session: every equity opening is denied, an extended-hours one
    /// in the regular session too; a risk exit and a protective order pass; a discretionary exit
    /// defers; an owner exit defers until the bid is confirmed; a market exit is re-priced as a
    /// marketable limit rather than denied, after hours and in the closing ten minutes alike
    /// (DEC-159), a limit exit is not paced, and a market opening in the window is
    /// `auction_window`.
    #[test]
    fn the_session_denies_openings_and_defers_only_exits() -> Result<(), GateError> {
        let after = |mut o: Owned| -> Result<Owned, GateError> {
            o.now = at("2026-09-21T21:00:00Z")?;
            Ok(o)
        };
        let mut extended = allowing()?;
        extended.proposed.extended_hours = true;
        let mut confirmed = after(allowing()?.selling(Origin::OwnerClose)?)?;
        confirmed.proposed.owner_confirmed_bid = Some(Price::parse("99")?);
        assert_eq!(
            [
                row(&after(allowing()?)?)?,
                row(&extended)?,
                row(&after(allowing()?.selling(Origin::RiskEngine)?)?)?,
                row(&after(allowing()?.selling(Origin::ProtectiveLeg)?)?)?,
                row(&after(allowing()?.selling(Origin::GoalCompletion)?)?)?,
                row(&after(allowing()?.selling(Origin::OwnerKillSwitch)?)?)?,
                row(&confirmed)?,
            ],
            [
                Ok((Verdict::Deny, Some(ReasonCode::SessionNotAllowed))),
                Ok((
                    Verdict::Deny,
                    Some(ReasonCode::ExtendedHoursOpeningNotAllowed)
                )),
                Ok((Verdict::Allow, None)),
                Ok((Verdict::Allow, None)),
                Ok((
                    Verdict::Defer,
                    Some(ReasonCode::DiscretionaryExitRegularSessionOnly)
                )),
                Ok((Verdict::Defer, Some(ReasonCode::OwnerConfirmationRequired))),
                Ok((Verdict::Allow, None)),
            ],
            "after hours: openings denied, risk and protective exits allowed, discretionary and \
             unconfirmed owner exits deferred"
        );
        let mut market = after(allowing()?.selling(Origin::RiskEngine)?)?;
        market.proposed.kind = ProposedKind::Market;
        let limit = after(allowing()?.selling(Origin::RiskEngine)?)?;
        let in_the_close = |origin: Origin, kind: ProposedKind| -> Result<Owned, GateError> {
            let mut o = allowing()?.selling(origin)?;
            o.now = at("2026-09-21T19:55:00Z")?;
            o.proposed.kind = kind;
            Ok(o)
        };
        let mut opening_in_the_close = allowing()?;
        opening_in_the_close.now = at("2026-09-21T19:55:00Z")?;
        opening_in_the_close.proposed.kind = ProposedKind::Market;
        let repriced = |o: Owned| -> Result<_, GateError> {
            let d = o.decide()?;
            Ok((d.verdict, d.pacing.map(|p| p.marketable_limit_required)))
        };
        assert_eq!(
            (
                repriced(market)?,
                repriced(limit)?,
                repriced(in_the_close(Origin::RiskEngine, ProposedKind::Market)?)?,
                repriced(in_the_close(Origin::OrderBuilder, ProposedKind::Market)?)?,
                repriced(in_the_close(Origin::OwnerClose, ProposedKind::Market)?)?,
                repriced(in_the_close(Origin::RiskEngine, ProposedKind::Plain)?)?,
                row(&opening_in_the_close)?,
            ),
            (
                (Verdict::Allow, Some(true)),
                (Verdict::Allow, None),
                (Verdict::Allow, Some(true)),
                (Verdict::Allow, Some(true)),
                (Verdict::Allow, Some(true)),
                (Verdict::Allow, None),
                Ok((Verdict::Deny, Some(ReasonCode::AuctionWindow))),
            ),
            "a market exit after hours or in the closing ten minutes is re-priced, never denied, \
             whatever its purpose (DEC-159); a limit exit is sent as is; a market opening in the \
             window is auction_window"
        );
        Ok(())
    }
}
