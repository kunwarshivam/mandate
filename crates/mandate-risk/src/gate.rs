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
    GateInput, InstrumentRestriction, Origin, Pacing, ProposedKind, Purpose, ReasonCode, Side,
    Verdict, WorkingUniverse, floor, limits,
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

/// The story that completes a check, while any part of it is owed. Check 1 is whole here; check 2
/// is whole for a US equity but still owes §3.2 item 7's "USD pairs only" for crypto (E6-10);
/// check 3 its sessions and auction windows (E6-6; the halt is here),
/// check 4 its rules 1, 2 and 4 to 8, check 6 its conduct controls (E6-8) and check 7 buying power
/// (E6-6), and checks 5 and 8 are not written yet.
///
/// Check 2 takes the input because "USD pairs only" has no field to read: `InstrumentSnapshot`
/// carries no quote currency and `AssetId` is a UUID, so nothing distinguishes BTC/USD from
/// BTC/USDT. Calling check 2 whole for crypto would let a non-USD pair be opened the moment E6-6
/// and E6-8 land, so a crypto opening stays owed and is refused by the fail-closed rule until
/// E6-10 supplies the field (DEC-129 item 34). A crypto *exit* is unaffected: `first_owed` accrues
/// only for an opening.
fn owed(check: Check, input: &GateInput<'_>) -> Option<&'static str> {
    match check {
        Check::UniverseAndLimits if input.instrument.asset_class != AssetClass::UsEquity => {
            Some("E6-10")
        }
        Check::AccountAndMode | Check::UniverseAndLimits => None,
        Check::SessionAndHalt
        | Check::OrderConstraints
        | Check::BuyingPowerAndExposure
        | Check::DayTradeBudget => Some("E6-6"),
        Check::MarkAndCollar | Check::ConductControls => Some("E6-8"),
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
    let mut computed = Computed::default();
    let mut checks = Vec::with_capacity(ORDER.len());
    let mut stop: Option<Stop> = None;
    let mut first_owed: Option<&'static str> = None;
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
        presumed_halt_repricing(input)
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
    purpose: Purpose,
    opening: bool,
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
        Check::SessionAndHalt if opening => Ok(halt(input)),
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

/// Check 3's halt (§4.4): a halted or paused instrument takes no new opening order. Only an opening
/// reaches here, so a halt never denies an exit (MI-1); a presumed halt, a dropped status feed, is
/// not a halt and denies nothing here (DEC-129 item 24).
fn halt(input: &GateInput<'_>) -> Option<Stop> {
    input
        .instrument
        .halted
        .then_some((Verdict::Deny, ReasonCode::InstrumentHalted))
}

/// §4.4's "no market orders" applies under a halt, real or presumed: a halted instrument, or a
/// dropped status feed. The stale-quote arm of a presumed halt reads the mark's freshness, which is
/// check 5's (E6-8). The `halted` arm cannot change check 4's verdict on an opening, which check 3
/// has already denied; it stays because the same predicate re-prices an exit under a real halt
/// (DEC-129 item 31), and as defence in depth should check 3's halt ever be reordered.
fn market_orders_barred(input: &GateInput<'_>) -> bool {
    input.instrument.halted || !input.instrument.status_feed_current
}

/// §4.4 and §5.6 (DEC-129 item 28): an allowed market order under a halt, real or presumed, is sent
/// as a marketable limit at its proposed quantity and price, never as a market order. Only a
/// reduction gets here with a market kind under a halt, since checks 3 and 4 deny such an opening.
fn presumed_halt_repricing(input: &GateInput<'_>) -> Option<Pacing> {
    (market_orders_barred(input) && input.proposed.kind == ProposedKind::Market).then(|| Pacing {
        qty: input.proposed.qty,
        limit_price: input.proposed.limit_price,
        marketable_limit_required: true,
        applied: BTreeSet::new(),
    })
}

/// Check 4, the rules this story owns: rule 3 (every sell above the position, a protective leg's
/// included, is typed an opening by [`assign_purpose`], so it is the only sell that reaches here as
/// one), then rule 9, which denies an opening and holds a reduction under one code (DEC-129 item
/// 22), then §4.4's presumed halt: a market order to open or increase is denied
/// `market_order_not_allowed` (DEC-129 items 24 and 28). §5.1's wider rule, that every opening is a
/// limit order in any state of the feed, is the v1 order policy's, which E6-6 completes; until then
/// such an opening is refused by the fail-closed rule (DEC-129 item 29), never allowed. §5.3's "bracket protective legs are checked against position + entry quantity" belongs to
/// rules 4 to 6 (`sell_exceeds_available`, E6-6), not to rule 3.
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
    if opening && input.proposed.kind == ProposedKind::Market && market_orders_barred(input) {
        return Some((Verdict::Deny, ReasonCode::MarketOrderNotAllowed));
    }
    None
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

        /// `f` over this scenario's inputs, at the fixture's `now`.
        fn with_input<T>(&self, f: impl FnOnce(&GateInput<'_>) -> T) -> Result<T, GateError> {
            Ok(f(&GateInput {
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
                CheckOutcome::NotReached(Check::SessionAndHalt),
                CheckOutcome::NotReached(Check::OrderConstraints),
                CheckOutcome::NotReached(Check::MarkAndCollar),
                CheckOutcome::NotReached(Check::ConductControls),
                CheckOutcome::NotReached(Check::BuyingPowerAndExposure),
                CheckOutcome::NotReached(Check::DayTradeBudget),
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
    /// that completes the first check still owed (check 3's sessions, E6-6).
    #[test]
    fn an_opening_the_partial_gate_would_allow_is_refused() -> Result<(), GateError> {
        let refused = allowing()?.decide();
        assert!(
            matches!(refused, Err(GateError::Unimplemented("evaluate", "E6-6"))),
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
            matches!(refused, Err(GateError::Unimplemented("evaluate", "E6-6"))),
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
            matches!(a, Err(GateError::Unimplemented("evaluate", "E6-6")))
                && matches!(b, Err(GateError::Unimplemented("evaluate", "E6-6"))),
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
            [Err("E6-6"), Ok(ReasonCode::GrossExposureLimit)],
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
            (Err("E6-6"), Some(Usd::parse("200")?)),
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
            [Err("E6-6"), Ok(ReasonCode::GrossExposureLimit)],
            "900 of another agent's + 100 against the account's 1000"
        );
        Ok(())
    }

    /// Working cost is the agent's working *opening, non-protective* orders: a 100000 order in the
    /// instrument that is protective, or that is not an opening, changes nothing.
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
            [Err("E6-6"), Err("E6-6"), Ok(ReasonCode::ConcentrationLimit)],
            "protective, non-opening, and an opening order that does count"
        );
        Ok(())
    }

    /// What the halt table expects of one row: the verdict, the code, the check that failed, and
    /// the pacing, or the story a fail-closed opening is refused for.
    type HaltRow =
        Result<(Verdict, Option<ReasonCode>, Option<Check>, Option<Pacing>), &'static str>;

    /// The halt table's oracle, transcribed from the spec rather than from this module: the mode
    /// rule of `ref.py`'s `order_decision` (check 1), then `exits_only` denying an opening (§7.4),
    /// then §4.4's halt for an opening at check 3, then
    /// §4.4's "no market orders" under a presumed halt at check 4, then the fail-closed refusal of
    /// DEC-129 item 29; an exit is allowed, and re-priced as a marketable limit when it is a
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
            if market && !feed_current {
                return deny(ReasonCode::MarketOrderNotAllowed, Check::OrderConstraints);
            }
            return Err("E6-6");
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
    /// only an opening and only at check 3; that a market opening is denied at check 4 under a
    /// dropped feed and nowhere else, and a bracket or IOC opening never is; that a market exit
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
        i.prior_close = match r.price {
            0 => Some(Price::parse("100")?),
            1 => Some(Price::parse("5")?),
            2 => Some(Price::parse("4.999999999")?),
            _ => None,
        };
        i.median_dollar_volume_20d =
            figure(r.volume_20d, "90000000", "1000000", "999999.999999999")?;
        i.median_dollar_volume_30d =
            figure(r.volume_30d, "90000000", "2000000", "1999999.999999999")?;
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

    /// Every row of §3.2 against [`floor_oracle`]: 2 asset classes × 3 statuses × 3 exchanges × 3
    /// item-3 states × 4 prices × 4 20-day volumes × 3 ETP classes × 4 permissions × 4
    /// classification ages × 4 30-day volumes = 165888 rows. It pins the list order, `≥` at every
    /// floor, an absent figure failing, the ETP rule needing both the mandate's switch and the
    /// disclosure, the classification age failing closed only when strictly older, and which items
    /// bind which asset class.
    #[test]
    fn the_floor_matches_the_oracle_on_every_row() -> Result<(), GateError> {
        let mut o = allowing()?;
        let mut rows = 0_u32;
        let mut denied = BTreeSet::new();
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
        assert_eq!(
            (rows, denied.len()),
            (165_888, 6),
            "every row ran, and every floor code was reached"
        );
        Ok(())
    }

    /// §3.2's platform minimums bind whatever the organization set: price 1.00, and 1,000,000 for
    /// both the 20-day equity volume and the 30-day crypto volume.
    ///
    /// `GateConfig`'s fields are public and nothing in the repo validates them, so a config below a
    /// platform minimum is representable and the gate has to refuse it on its own. The main floor
    /// table cannot show this: its config sits at or above every minimum, so deleting the
    /// enforcement changes none of its 165888 rows. Review round 1 found exactly that — two
    /// substitutions that dropped the minimum survived the whole suite.
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
}
