//! The §5 fold's spine (DEC-167 item 5, slice R2): the agent sub-ledger over marks and fills, the
//! high-water mark, the ladder, breach confirmation with the hard trigger, the daily-loss trigger, the
//! lifetime floor, and the effective mode.
//!
//! What it does not fold yet is [`SpecError::Unimplemented`], never a silent answer: the risk day,
//! a held instrument's mark going stale, universe changes, and a `profit_stop` goal are slice R3's;
//! acknowledgments, allocation changes, floor loosening, goal completion, and retirement are R4's.

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::{AgentMode, AssetClass, MarketSession, Side};
use mandate_num::{Price, Qty, Ratio, Rounding, Usd};
use mandate_time::UtcNanos;

use super::limits::{Figures, Limits, Readings};
use super::{
    Confirmation, Input, KillScope, LiftReason, LimitKey, Opening, Outcome, Restriction, RiskEvent,
    SessionClock, Snapshot, Step, TriggerReason, add_seconds, hard_wait_s, rung_index, size_factor,
};
use crate::document::{Goal, LadderAction, LadderRung, LimitAction, Pointer};
use crate::validate::ValidatedMandate;
use crate::{SchemaDec, SpecError};

/// The places §5.2 reports the drawdown and the daily P&L fraction at, rounded half to even.
const REPORTED_RATIO_PLACES: u32 = 12;

/// Everything the fold carries between inputs. The clock is not here: it is the caller's calendar,
/// not state, so two folds of the same inputs are equal whichever clock each counted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Fold {
    limits: Limits,
    ladder: Vec<LadderRung>,
    daily_action: LimitAction,
    breach_confirm_s: u32,
    scale_lift_after_s: u32,
    asset_class: AssetClass,
    mark_max_age_s: u32,
    allocation: Usd,
    /// The sub-ledger's cash: sale proceeds less purchases, the opening position's cost included. E is
    /// A + cash + qty × mark, which is §5.1's A + realized − basis + market value with the basis split
    /// cancelled out: a reduction moves realized P&L and the basis by the same amount, so its rounding
    /// (trading spec §8.1) never reaches E.
    cash: Usd,
    qty: Qty,
    mark: Price,
    /// The peak of E since the state opened (§5.5). It starts at the allocation, which the schema makes
    /// positive, and only rises in this slice, so it is always positive.
    high_water: Usd,
    day_start: Usd,
    capital: Usd,
    inherited: Usd,
    net_contributed: Usd,
    at: UtcNanos,
    /// Session seconds a held instrument has gone without a sane mark (§5.2).
    mark_age_s: u64,
    scale: BTreeMap<u8, ScaleRung>,
    confirmations: BTreeMap<LimitKey, Confirmation>,
    /// Each limit whose 1.25x level a sane quote reached, and when: the start of its hard wait (§5.6).
    hard_since: BTreeMap<LimitKey, UtcNanos>,
    latched: BTreeSet<LimitKey>,
    restrictions: BTreeSet<Restriction>,
    mode: AgentMode,
}

/// A `scale_sizes` rung's timers (§5.5), in session seconds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ScaleRung {
    /// How long the rung has been active, while it is.
    active_s: Option<u64>,
    /// Time below the lift level since the rung last was not, credited by the state at each
    /// interval's start.
    lift_s: u64,
    /// Whether the last input left the rung active and below its lift level, which is what credits
    /// the next interval to `lift_s`.
    lifting: bool,
}

/// What an input did to the instrument's mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Read {
    /// A sane mark in a session that counts: the "sane quote" §5.6's hard trigger waits for.
    Quote,
    /// A mark in a session that counts which failed its checks.
    Failed,
    /// No mark, or one outside the regular session for an equity, which §5.2 ignores.
    Nothing,
}

/// The input's time, as each of §5's clocks counts it, and whether it was a sane quote.
#[derive(Debug, Clone, Copy)]
struct Moment {
    /// Wall-clock seconds: breach confirmation and the hard wait run on every second (§5.6).
    wall_s: u64,
    /// Regular-session seconds for an equity, every second for crypto: the lift delay, a rung's
    /// active time, and staleness (§5.2, §5.5).
    session_s: u64,
    quote: bool,
}

/// A limit that latches: its key, the action it journals, and the restriction it applies.
#[derive(Debug, Clone, Copy)]
struct Limit {
    key: LimitKey,
    action: LadderAction,
    restriction: Restriction,
}

/// How a confirmable limit triggered (§5.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Trigger {
    Confirmed,
    Hard,
}

impl Trigger {
    /// Plain confirmation journals no reason (§5.10).
    fn reason(self) -> Option<TriggerReason> {
        match self {
            Self::Confirmed => None,
            Self::Hard => Some(TriggerReason::HardTrigger),
        }
    }
}

impl Fold {
    /// E = H = E₀ = C = N = the allocation, L = the connection's loss carry, and mode `normal` (§5.2).
    pub(super) fn open(mandate: &ValidatedMandate, opening: &Opening) -> Result<Self, SpecError> {
        let document = mandate.mandate();
        match &document.goal {
            Goal::ProfitStop { profit_level, .. } => return confirm_profit_stop(profit_level),
            Goal::Continuous { .. } | Goal::Accumulate { .. } => {}
        }
        if opening.inherited_loss_usd.is_negative() {
            return Err(SpecError::InvalidInput {
                what: "a negative inherited loss, which would lower the lifetime floor",
            });
        }
        let risk = &document.risk;
        let allocation = Usd::parse(document.capital.allocation_usd.as_str()).map_err(|cause| {
            SpecError::OutOfRange {
                path: Pointer::new("/capital/allocation_usd"),
                cause,
            }
        })?;
        let scale = risk
            .drawdown_ladder
            .iter()
            .enumerate()
            .filter(|(_, rung)| rung.action == LadderAction::ScaleSizes)
            .map(|(index, _)| Ok((rung_index(index)?, ScaleRung::default())))
            .collect::<Result<BTreeMap<u8, ScaleRung>, SpecError>>()?;
        Ok(Self {
            limits: Limits::of(mandate)?,
            ladder: risk.drawdown_ladder.clone(),
            daily_action: risk.daily_loss_action,
            breach_confirm_s: risk.breach_confirm_s,
            scale_lift_after_s: risk.scale_lift_after_s,
            asset_class: opening.asset_class,
            mark_max_age_s: opening.mark_max_age_s,
            allocation,
            cash: opening.position_qty.notional(opening.avg_cost)?.negated(),
            qty: opening.position_qty,
            mark: opening.avg_cost,
            high_water: allocation,
            day_start: allocation,
            capital: allocation,
            inherited: opening.inherited_loss_usd,
            net_contributed: allocation,
            at: opening.at,
            mark_age_s: 0,
            scale,
            confirmations: BTreeMap::new(),
            hard_since: BTreeMap::new(),
            latched: BTreeSet::new(),
            restrictions: BTreeSet::new(),
            mode: AgentMode::Normal,
        })
    }

    /// One input in §5.2's order: settle time, apply the input, E then H, the rungs in ladder order,
    /// the daily loss, the floor, then the effective mode. The caller keeps the fold it had when this
    /// fails, so an error never leaves a half-applied step behind.
    pub(super) fn step(
        &mut self,
        clock: &dyn SessionClock,
        step: &Step,
    ) -> Result<Outcome, SpecError> {
        if step.at < self.at {
            return Err(SpecError::ClockWentBackwards);
        }
        let wall_s = elapsed_s(self.at, step.at)?;
        let session_s = match self.asset_class {
            AssetClass::Crypto => wall_s,
            AssetClass::UsEquity => clock.seconds_between(self.at, step.at)?,
        };
        self.at = step.at;
        let expired = self.age_mark(session_s)?;
        let read = self.apply(step)?;
        if self.goes_stale(expired, read) {
            return mark_goes_stale(&step.input);
        }
        let figures = self.settle_equity()?;
        let readings = self.limits.conditions(&figures)?;
        let moment = Moment {
            wall_s,
            session_s,
            quote: read == Read::Quote,
        };
        let mut journal = Vec::new();
        self.ladder(&readings, moment, &mut journal)?;
        let daily = Limit {
            key: LimitKey::MaxDailyLoss,
            action: self.daily_action.into(),
            restriction: Restriction::DailyLoss,
        };
        self.confirm(daily, &readings, moment, &mut journal)?;
        let floor = Limit {
            key: LimitKey::LifetimeFloor,
            action: LadderAction::FlattenAndPause,
            restriction: Restriction::LifetimeFloor,
        };
        self.confirm(floor, &readings, moment, &mut journal)?;
        self.apply_mode(&mut journal);
        Ok(Outcome {
            snapshot: self.snapshot()?,
            journal,
            pending: self.pending(),
            rejection: None,
        })
    }

    pub(super) fn snapshot(&self) -> Result<Snapshot, SpecError> {
        let equity = self.equity()?;
        let daily_pnl = equity.checked_sub(self.day_start)?;
        let active_rungs: BTreeMap<u8, u64> = self
            .scale
            .iter()
            .filter_map(|(index, rung)| rung.active_s.map(|active| (*index, active)))
            .collect();
        Ok(Snapshot {
            agent_equity: equity,
            high_water_mark: self.high_water,
            drawdown: reported(self.high_water.checked_sub(equity)?, self.high_water)?,
            day_start_equity: self.day_start,
            daily_pnl,
            daily_pnl_fraction: reported(daily_pnl, self.day_start)?,
            capital_base: self.capital,
            inherited_loss: self.inherited,
            size_factor: size_factor(&self.ladder, &active_rungs)?,
            latched: self.latched.clone(),
            active_rungs,
            restrictions: self.restrictions.clone(),
            agent_mode: self.mode,
            instrument_restrictions: BTreeSet::new(),
            net_contributed: self.net_contributed,
        })
    }

    fn equity(&self) -> Result<Usd, SpecError> {
        Ok(self
            .allocation
            .checked_add(self.cash)?
            .checked_add(self.qty.notional(self.mark)?)?)
    }

    /// E, then H = max(H, E) (§5.2), as the five figures the conditions read.
    fn settle_equity(&mut self) -> Result<Figures, SpecError> {
        let equity = self.equity()?;
        self.high_water = self.high_water.max(equity);
        Ok(Figures {
            equity,
            high_water: self.high_water,
            day_start: self.day_start,
            capital: self.capital,
            inherited: self.inherited,
        })
    }

    /// Advances a held instrument's mark age by the interval, before the input applies, and says
    /// whether it has reached the staleness limit (§5.2).
    fn age_mark(&mut self, session_s: u64) -> Result<bool, SpecError> {
        if self.qty.is_zero() {
            return Ok(false);
        }
        self.mark_age_s = add_seconds(self.mark_age_s, session_s)?;
        Ok(self.mark_age_s >= u64::from(self.mark_max_age_s))
    }

    /// Whether the step leaves a held instrument without a sane mark: one that failed its checks, or
    /// an age at the limit that no sane mark reset (§5.2). A flat book has nothing to go stale.
    fn goes_stale(&self, expired: bool, read: Read) -> bool {
        let held = !self.qty.is_zero();
        held && match read {
            Read::Quote => false,
            Read::Failed => true,
            Read::Nothing => expired,
        }
    }

    fn apply(&mut self, step: &Step) -> Result<Read, SpecError> {
        match &step.input {
            Input::Mark { bid, sane } => Ok(self.read_mark(*bid, *sane, step.session)),
            Input::Fill { side, qty, price } => {
                self.fill(*side, *qty, *price)?;
                Ok(Read::Nothing)
            }
            Input::Clock => Ok(Read::Nothing),
            Input::RiskDayStarted
            | Input::OwnerAcknowledged { .. }
            | Input::AllocationChange { .. }
            | Input::UniverseChanged { .. }
            | Input::FloorLoosened { .. }
            | Input::AgentStopped { .. }
            | Input::GoalComplete => fold_later(&step.input),
        }
    }

    /// For an equity only a regular-session mark counts; crypto counts every mark. A counted sane
    /// mark moves E and resets the mark's age (§5.2, trading spec §8.2).
    fn read_mark(&mut self, bid: Price, sane: bool, session: MarketSession) -> Read {
        let counts = self.asset_class == AssetClass::Crypto || session == MarketSession::Regular;
        if !counts {
            Read::Nothing
        } else if sane {
            self.mark = bid;
            self.mark_age_s = 0;
            Read::Quote
        } else {
            Read::Failed
        }
    }

    /// A fill moves the sub-ledger in any session (§5.2). A sale of more than the agent holds would be
    /// a short sale, which v1 never makes (AGENTS.md rule 12), so it is refused.
    fn fill(&mut self, side: Side, qty: Qty, price: Price) -> Result<(), SpecError> {
        let notional = qty.notional(price)?;
        match side {
            Side::Buy => {
                self.qty = self.qty.checked_add(qty)?;
                self.cash = self.cash.checked_sub(notional)?;
            }
            Side::Sell => {
                self.qty = self
                    .qty
                    .checked_sub(qty)
                    .map_err(|_| SpecError::InvalidInput {
                        what: "a sale of more than the agent holds, which would be a short sale",
                    })?;
                self.cash = self.cash.checked_add(notional)?;
            }
        }
        Ok(())
    }

    /// The rungs in ladder order, which V-010 makes ascending `at` (§5.2, §5.5).
    fn ladder(
        &mut self,
        readings: &Readings,
        moment: Moment,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        let actions: Vec<LadderAction> = self.ladder.iter().map(|rung| rung.action).collect();
        for (index, action) in actions.into_iter().enumerate() {
            let key = LimitKey::DrawdownRung(rung_index(index)?);
            match action {
                LadderAction::ScaleSizes => self.scale_rung(key, readings, moment, journal)?,
                LadderAction::ExitsOnly => {
                    let limit = Limit {
                        key,
                        action,
                        restriction: Restriction::DrawdownExitsOnly,
                    };
                    self.confirm(limit, readings, moment, journal)?;
                }
                LadderAction::FlattenAndPause => {
                    let limit = Limit {
                        key,
                        action,
                        restriction: Restriction::DrawdownFlatten,
                    };
                    self.confirm(limit, readings, moment, journal)?;
                }
            }
        }
        Ok(())
    }

    /// A `scale_sizes` rung triggers at once and lifts after `scale_lift_after_s` of session time
    /// continuously below its lift level (§5.5). It is the only kind of rung the lift level lifts.
    fn scale_rung(
        &mut self,
        key: LimitKey,
        readings: &Readings,
        moment: Moment,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        let LimitKey::DrawdownRung(index) = key else {
            return Err(SpecError::InvalidInput {
                what: "a scale rung that is not a drawdown rung",
            });
        };
        let (hit, _) = reading(readings, key)?;
        let below_lift =
            readings
                .below_lift
                .get(&index)
                .copied()
                .ok_or(SpecError::InvalidInput {
                    what: "a rung the conditions did not read",
                })?;
        let lift_after_s = u64::from(self.scale_lift_after_s);
        let rung = self.scale.get_mut(&index).ok_or(SpecError::InvalidInput {
            what: "a scale_sizes rung the state did not open with",
        })?;
        if rung.lifting {
            rung.lift_s = add_seconds(rung.lift_s, moment.session_s)?;
        }
        if let Some(active_s) = rung.active_s {
            rung.active_s = Some(add_seconds(active_s, moment.session_s)?);
        }
        if hit {
            if rung.active_s.is_none() {
                rung.active_s = Some(0);
                journal.push(RiskEvent::RiskLimitTriggered {
                    limit: key,
                    action: LadderAction::ScaleSizes,
                    reason: None,
                });
            }
        } else if rung.active_s.is_some() && below_lift && rung.lift_s >= lift_after_s {
            rung.active_s = None;
            journal.push(RiskEvent::RiskLimitLifted {
                limit: key,
                action: Some(LadderAction::ScaleSizes),
                reason: None,
            });
        }
        rung.lifting = rung.active_s.is_some() && below_lift;
        if !rung.lifting {
            rung.lift_s = 0;
        }
        Ok(())
    }

    /// A limit that confirms (§5.6) and then holds until its own lift path, none of which a mark is
    /// (§5.4, §5.7, §5.8): a latched limit is not evaluated again in this slice.
    fn confirm(
        &mut self,
        limit: Limit,
        readings: &Readings,
        moment: Moment,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        if self.latched.contains(&limit.key) {
            return Ok(());
        }
        let Some(trigger) =
            self.decide(limit.key, reading(readings, limit.key)?, moment, journal)?
        else {
            return Ok(());
        };
        self.confirmations.remove(&limit.key);
        self.latched.insert(limit.key);
        self.restrictions.insert(limit.restriction);
        journal.push(RiskEvent::RiskLimitTriggered {
            limit: limit.key,
            action: limit.action,
            reason: trigger.reason(),
        });
        if limit.action == LadderAction::FlattenAndPause {
            journal.push(RiskEvent::KillSwitchActivated {
                scope: KillScope::Agent,
                initiator: limit.key,
            });
        }
        Ok(())
    }

    /// Breach time, then the hard trigger (§5.6, DEC-63): a sane quote at the 1.25x level applies
    /// `hard_breach` at once, a second sane quote at it at least the hard wait later latches the limit,
    /// and a sane quote below it clears the restriction while breach time carries on.
    fn decide(
        &mut self,
        key: LimitKey,
        (hit, hard): (bool, bool),
        moment: Moment,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<Option<Trigger>, SpecError> {
        let confirmed = self.confirmations.entry(key).or_default().update(
            hit,
            moment.wall_s,
            self.breach_confirm_s,
        )?;
        if confirmed {
            self.clear_hard(key);
            return Ok(Some(Trigger::Confirmed));
        }
        if !moment.quote {
            return Ok(None);
        }
        if !hard {
            if self.clear_hard(key) {
                journal.push(RiskEvent::RiskLimitLifted {
                    limit: key,
                    action: None,
                    reason: Some(LiftReason::HardBreachCleared),
                });
            }
            return Ok(None);
        }
        let Some(since) = self.hard_since.get(&key).copied() else {
            self.hard_since.insert(key, self.at);
            self.restrictions.insert(Restriction::HardBreach);
            journal.push(RiskEvent::RiskLimitTriggered {
                limit: key,
                action: LadderAction::ExitsOnly,
                reason: Some(TriggerReason::HardBreachPending),
            });
            return Ok(None);
        };
        if elapsed_s(since, self.at)? < u64::from(hard_wait_s(self.breach_confirm_s)) {
            return Ok(None);
        }
        self.clear_hard(key);
        Ok(Some(Trigger::Hard))
    }

    /// Ends `key`'s hard wait, and lifts `hard_breach` once no limit is waiting. Whether it had one.
    fn clear_hard(&mut self, key: LimitKey) -> bool {
        let waiting = self.hard_since.remove(&key).is_some();
        if self.hard_since.is_empty() {
            self.restrictions.remove(&Restriction::HardBreach);
        }
        waiting
    }

    /// The strictest restriction's mode, journalled only when it changes (§5.9, MI-6).
    fn apply_mode(&mut self, journal: &mut Vec<RiskEvent>) {
        let mode = self
            .restrictions
            .iter()
            .map(|restriction| self.mode_of(*restriction))
            .max()
            .unwrap_or(AgentMode::Normal);
        if mode != self.mode {
            journal.push(RiskEvent::AgentModeApplied {
                from: self.mode,
                to: mode,
            });
            self.mode = mode;
        }
    }

    /// A `daily_loss` restriction asks for the mode of the action that set it: `flatten_and_pause`
    /// pauses (§5.4), which [`Restriction::mode`] alone cannot say. Every other restriction's mode is
    /// its own.
    fn mode_of(&self, restriction: Restriction) -> AgentMode {
        match (restriction, self.daily_action) {
            (Restriction::DailyLoss, LimitAction::FlattenAndPause) => AgentMode::Paused,
            _ => restriction.mode(),
        }
    }

    /// The limits with breach time accumulating (§5.6).
    fn pending(&self) -> BTreeSet<LimitKey> {
        self.confirmations
            .iter()
            .filter(|(_, confirmation)| confirmation.is_pending())
            .map(|(key, _)| *key)
            .collect()
    }
}

fn reading(readings: &Readings, key: LimitKey) -> Result<(bool, bool), SpecError> {
    readings
        .limits
        .get(&key)
        .copied()
        .ok_or(SpecError::InvalidInput {
            what: "a limit the conditions did not read",
        })
}

/// A reported ratio: `numerator ÷ denominator` rounded half to even at 12 places (§5.2).
fn reported(numerator: Usd, denominator: Usd) -> Result<Ratio, SpecError> {
    Ok(numerator.ratio_to(denominator, REPORTED_RATIO_PLACES, Rounding::HalfEven)?)
}

/// Whole seconds from `from` to `to`, the risk clock's unit (§5.2).
fn elapsed_s(from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
    to.secs()
        .checked_sub(from.secs())
        .and_then(|seconds| u64::try_from(seconds).ok())
        .ok_or(SpecError::ClockWentBackwards)
}

/// Slice R3: a `profit_stop` goal's confirmation (§3.1, §5.6), which runs at every step.
fn confirm_profit_stop(profit_level: &SchemaDec) -> Result<Fold, SpecError> {
    let _ = profit_level;
    Err(SpecError::Unimplemented)
}

/// Slice R3: `stale_mark`, set and cleared with its `InstrumentRestrictionChanged` (§5.2, §5.9).
fn mark_goes_stale(input: &Input) -> Result<Outcome, SpecError> {
    let _ = input;
    Err(SpecError::Unimplemented)
}

/// Slices R3 and R4: the risk day and universe changes (R3); acknowledgments, allocation changes,
/// floor loosening, retirement, and goal completion (R4).
fn fold_later(input: &Input) -> Result<Read, SpecError> {
    let _ = input;
    Err(SpecError::Unimplemented)
}
