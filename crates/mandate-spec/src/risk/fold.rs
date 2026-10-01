//! The §5 fold (DEC-167 item 5): the agent sub-ledger over marks and fills, the high-water mark, the
//! ladder, breach confirmation with the hard trigger, the daily loss over risk days, the lifetime
//! floor, a `profit_stop` goal, the instrument's staleness and universe restrictions, and the
//! effective mode (slices R2 and R3), and the owner's and the version's inputs: acknowledgments,
//! allocation changes, floor loosening, goal completion, and retirement (slice R4).

mod daily;
mod owner;

use std::collections::{BTreeMap, BTreeSet};

use mandate_domain::{AgentMode, AssetClass, AssetId, MarketSession, Side};
use mandate_num::{Price, Qty, Ratio, Rounding, Usd};
use mandate_time::UtcNanos;

use self::daily::{DailyLatch, Rollover};
use super::limits::{Figures, Limits, Readings};
use super::{
    Confirmation, GoalReason, Input, InstrumentRestriction, KillScope, LiftReason, LimitKey,
    Opening, Outcome, Rejection, RemovalReason, Restriction, RestrictionReason, RiskEvent,
    SessionClock, Snapshot, Step, ThenAction, TriggerReason, UniverseChange, add_seconds,
    hard_wait_s, rung_index, size_factor,
};
use crate::SpecError;
use crate::document::{LadderAction, LadderRung, LimitAction, OnComplete, Pointer};
use crate::validate::ValidatedMandate;

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
    daily_breach_min_s: u32,
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
    /// The daily loss's wait carries over the rollover, since the quote that armed it is still the
    /// last one the state saw.
    hard_since: BTreeMap<LimitKey, UtcNanos>,
    /// The latched rungs and the floor. The daily loss latches in `daily`, which also says when its
    /// lift may come (§5.4).
    latched: BTreeSet<LimitKey>,
    daily: Option<DailyLatch>,
    rollover: Option<Rollover>,
    restrictions: BTreeSet<Restriction>,
    instrument_restrictions: BTreeSet<InstrumentRestriction>,
    mode: AgentMode,
    /// The goal's `on_complete`, which a `profit_stop` does not have (§3.1).
    on_complete: Option<OnComplete>,
    /// Set by a completed goal's `disarm_ladder`: the rungs and the daily loss are no longer
    /// evaluated, and the floor still is (§3.1).
    disarmed: bool,
    /// Set by an acknowledgment of the ladder: until no scale rung is active, only the highest
    /// active one counts towards its lift, so sizes return one rung at a time (§5.8).
    stepping: bool,
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
        on_the_risk_clock(opening.at)?;
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
            daily_breach_min_s: risk.daily_breach_min_s,
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
            daily: None,
            rollover: None,
            restrictions: BTreeSet::new(),
            instrument_restrictions: BTreeSet::new(),
            mode: AgentMode::Normal,
            on_complete: document.goal.on_complete(),
            disarmed: false,
            stepping: false,
        })
    }

    /// One input, folded on a copy: the fold it returns and what the step produced, or an error and
    /// nothing, so a refused step can never leave a half-applied fold behind.
    pub(super) fn step(
        &self,
        clock: &dyn SessionClock,
        step: &Step,
    ) -> Result<(Self, Outcome), SpecError> {
        let mut next = self.clone();
        let outcome = next.fold(clock, step)?;
        Ok((next, outcome))
    }

    /// One input. An allocation change applies after time has been settled at its instant, with no
    /// time passing (§5.1): the settling is a clock tick at that instant, and its events come first.
    /// Settling unconditionally is the same as settling only when time has passed, since a tick at
    /// the previous input's instant changes nothing.
    fn fold(&mut self, clock: &dyn SessionClock, step: &Step) -> Result<Outcome, SpecError> {
        if !matches!(step.input, Input::AllocationChange { .. }) {
            return self.evaluate(clock, step);
        }
        let settle = Step {
            at: step.at,
            session: step.session,
            input: Input::Clock,
        };
        let mut journal = self.evaluate(clock, &settle)?.journal;
        let mut outcome = self.evaluate(clock, step)?;
        journal.append(&mut outcome.journal);
        outcome.journal = journal;
        Ok(outcome)
    }

    /// One input in §5.2's order: settle time, apply the input, E then H, the rungs in ladder order,
    /// the daily loss, the floor, the profit stop, the instrument's restrictions, then the effective
    /// mode.
    fn evaluate(&mut self, clock: &dyn SessionClock, step: &Step) -> Result<Outcome, SpecError> {
        on_the_risk_clock(step.at)?;
        if step.at < self.at {
            return Err(SpecError::ClockWentBackwards);
        }
        let wall_s = elapsed_s(self.at, step.at)?;
        let session_s = match self.asset_class {
            AssetClass::Crypto => wall_s,
            AssetClass::UsEquity => clock.seconds_between(self.at, step.at)?,
        };
        self.at = step.at;
        let before = self.instrument_restrictions.clone();
        let mut journal = Vec::new();
        self.age_mark(session_s)?;
        let (read, rejection) = self.apply(step, wall_s, &mut journal)?;
        let figures = self.settle_equity()?;
        let readings = self.limits.conditions(&figures)?;
        let moment = Moment {
            wall_s,
            session_s,
            quote: read == Read::Quote,
        };
        self.ladder(&readings, moment, &mut journal)?;
        self.daily_loss(&readings, figures.equity, moment, &mut journal)?;
        let floor = Limit {
            key: LimitKey::LifetimeFloor,
            action: LadderAction::FlattenAndPause,
            restriction: Restriction::LifetimeFloor,
        };
        self.confirm(floor, &readings, moment, &mut journal)?;
        self.profit_stop(readings.profit, moment, &mut journal)?;
        self.instrument_events(&before, &step.input, &mut journal);
        self.apply_mode(&mut journal);
        Ok(Outcome {
            snapshot: self.snapshot()?,
            journal,
            pending: self.pending(),
            rejection,
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
        let mut latched = self.latched.clone();
        if self.daily.is_some() {
            latched.insert(LimitKey::MaxDailyLoss);
        }
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
            latched,
            active_rungs,
            restrictions: self.restrictions.clone(),
            agent_mode: self.mode,
            instrument_restrictions: self.instrument_restrictions.clone(),
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
        self.high_water = self.high_water.max(self.equity()?);
        self.figures()
    }

    /// The five figures the conditions read, as the state holds them.
    fn figures(&self) -> Result<Figures, SpecError> {
        Ok(Figures {
            equity: self.equity()?,
            high_water: self.high_water,
            day_start: self.day_start,
            capital: self.capital,
            inherited: self.inherited,
        })
    }

    /// Advances a held instrument's mark age by the interval, before the input applies, and marks it
    /// `stale_mark` once the age reaches the staleness limit (§5.2).
    fn age_mark(&mut self, session_s: u64) -> Result<(), SpecError> {
        if self.qty.is_zero() {
            return Ok(());
        }
        self.mark_age_s = add_seconds(self.mark_age_s, session_s)?;
        if self.mark_age_s >= u64::from(self.mark_max_age_s) {
            self.instrument_restrictions
                .insert(InstrumentRestriction::StaleMark);
        }
        Ok(())
    }

    /// Applies the input: what it did to the mark, and the refusal of an owner's or a version's
    /// input, which leaves the state as it was (DEC-128 item 16).
    fn apply(
        &mut self,
        step: &Step,
        wall_s: u64,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(Read, Option<Rejection>), SpecError> {
        let refused = match &step.input {
            Input::Mark { bid, sane } => {
                return Ok((self.read_mark(*bid, *sane, step.session), None));
            }
            Input::Fill { side, qty, price } => {
                self.fill(*side, *qty, *price)?;
                None
            }
            Input::Clock => None,
            Input::RiskDayStarted => {
                self.start_day(wall_s, journal)?;
                None
            }
            Input::UniverseChanged {
                instrument,
                change,
                reason,
            } => {
                self.change_universe(instrument, *change, *reason, journal);
                None
            }
            Input::OwnerAcknowledged { restriction } => self.acknowledge(*restriction, journal)?,
            Input::AllocationChange { delta_usd } => self.allocate(*delta_usd, journal)?,
            Input::FloorLoosened {
                new_max_loss_from_allocation,
                confirmed_at,
                independent_approval,
            } => self.loosen_floor(
                new_max_loss_from_allocation,
                *confirmed_at,
                *independent_approval,
                journal,
            )?,
            Input::AgentStopped { reason } => {
                self.retire(*reason, journal)?;
                None
            }
            Input::GoalComplete => {
                self.complete_goal(journal)?;
                None
            }
        };
        Ok((Read::Nothing, refused))
    }

    /// For an equity only a regular-session mark counts; crypto counts every mark. A counted sane
    /// mark moves E, resets the mark's age, and clears `stale_mark`; a counted mark that fails its
    /// checks makes a held instrument `stale_mark` (§5.2, trading spec §8.2).
    fn read_mark(&mut self, bid: Price, sane: bool, session: MarketSession) -> Read {
        let counts = self.asset_class == AssetClass::Crypto || session == MarketSession::Regular;
        if !counts {
            Read::Nothing
        } else if sane {
            self.mark = bid;
            self.mark_age_s = 0;
            self.instrument_restrictions
                .remove(&InstrumentRestriction::StaleMark);
            Read::Quote
        } else {
            if !self.qty.is_zero() {
                self.instrument_restrictions
                    .insert(InstrumentRestriction::StaleMark);
            }
            Read::Failed
        }
    }

    /// A removal restricts the instrument and a re-admission lifts it (§5.9). The state holds one
    /// instrument's position, as [`Opening`] does, so a change applies to it whatever id it carries.
    fn change_universe(
        &mut self,
        instrument: &AssetId,
        change: UniverseChange,
        reason: RemovalReason,
        journal: &mut Vec<RiskEvent>,
    ) {
        match change {
            UniverseChange::Removed => self
                .instrument_restrictions
                .insert(InstrumentRestriction::RemovedInstrument),
            UniverseChange::Admitted => self
                .instrument_restrictions
                .remove(&InstrumentRestriction::RemovedInstrument),
        };
        journal.push(RiskEvent::UniverseChanged {
            instrument: instrument.clone(),
            change,
            reason,
        });
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
        if self.disarmed {
            return Ok(());
        }
        let head = self.stepping.then(|| self.highest_active());
        let actions: Vec<LadderAction> = self.ladder.iter().map(|rung| rung.action).collect();
        for (index, action) in actions.into_iter().enumerate() {
            let key = LimitKey::DrawdownRung(rung_index(index)?);
            match action {
                LadderAction::ScaleSizes => {
                    self.scale_rung(key, readings, moment, head, journal)?
                }
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
        self.queue_lifts(readings)
    }

    /// The highest active scale rung, which V-010's ascending `at` makes the highest index.
    fn highest_active(&self) -> Option<u8> {
        self.scale
            .iter()
            .rev()
            .find(|(_, rung)| rung.active_s.is_some())
            .map(|(index, _)| *index)
    }

    /// Which rungs count the next interval towards their lift: an active one below its lift level,
    /// and while sizes are stepping back after a reset only the highest active one (§5.8). A rung
    /// that does not count starts its delay again. Stepping ends once no scale rung is active.
    fn queue_lifts(&mut self, readings: &Readings) -> Result<(), SpecError> {
        let head = self.highest_active();
        self.stepping = self.stepping && head.is_some();
        let stepping = self.stepping;
        for (index, rung) in &mut self.scale {
            rung.lifting = rung.active_s.is_some()
                && below_lift(readings, *index)?
                && (!stepping || head == Some(*index));
            if !rung.lifting {
                rung.lift_s = 0;
            }
        }
        Ok(())
    }

    /// A `scale_sizes` rung triggers at once and lifts after `scale_lift_after_s` of session time
    /// continuously below its lift level (§5.5). It is the only kind of rung the lift level lifts.
    ///
    /// The delay credits no more session seconds than wall seconds passed, so a calendar that
    /// over-reports its session can never lift a rung early. Active time and staleness take the
    /// clock's count as it is, since an over-report only makes a trim or a stale mark come sooner.
    ///
    /// While sizes step back after a reset, `head` is the highest active rung, the only one that
    /// may lift (§5.8).
    fn scale_rung(
        &mut self,
        key: LimitKey,
        readings: &Readings,
        moment: Moment,
        head: Option<Option<u8>>,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        let LimitKey::DrawdownRung(index) = key else {
            return Err(SpecError::InvalidInput {
                what: "a scale rung that is not a drawdown rung",
            });
        };
        let (hit, _) = reading(readings, key)?;
        let below_lift = below_lift(readings, index)?;
        let lift_after_s = u64::from(self.scale_lift_after_s);
        let rung = self.scale.get_mut(&index).ok_or(SpecError::InvalidInput {
            what: "a scale_sizes rung the state did not open with",
        })?;
        if rung.lifting {
            rung.lift_s = add_seconds(rung.lift_s, moment.session_s.min(moment.wall_s))?;
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
        } else if rung.active_s.is_some()
            && below_lift
            && rung.lift_s >= lift_after_s
            && head.is_none_or(|highest| highest == Some(index))
        {
            rung.active_s = None;
            journal.push(RiskEvent::RiskLimitLifted {
                limit: key,
                action: Some(LadderAction::ScaleSizes),
                reason: None,
            });
        }
        Ok(())
    }

    /// A rung or the floor, which confirms (§5.6) and then holds until its own lift path, none of
    /// which a mark is (§5.7, §5.8): a latched one is not evaluated again until its own path lifts it.
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
        self.latched.insert(limit.key);
        self.latch(limit, trigger.reason(), journal);
        Ok(())
    }

    /// Applies a limit that has just triggered: its restriction, its event, and for a flatten the
    /// agent's kill switch (§5.4, §5.5, §5.7). Its breach time is spent.
    fn latch(&mut self, limit: Limit, reason: Option<TriggerReason>, journal: &mut Vec<RiskEvent>) {
        self.confirmations.remove(&limit.key);
        self.restrictions.insert(limit.restriction);
        journal.push(RiskEvent::RiskLimitTriggered {
            limit: limit.key,
            action: limit.action,
            reason,
        });
        if limit.action == LadderAction::FlattenAndPause {
            journal.push(RiskEvent::KillSwitchActivated {
                scope: KillScope::Agent,
                initiator: limit.key,
            });
        }
    }

    /// A `profit_stop` goal confirms by breach time with no hard trigger, until the goal completes
    /// (§3.1, §5.6). It is a goal and not a limit: it journals `GoalCompleted` with the one outcome
    /// §3.1 gives it, and restricts the agent to `goal_complete`.
    fn profit_stop(
        &mut self,
        reading: Option<bool>,
        moment: Moment,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        let Some(hit) = reading else {
            return Ok(());
        };
        if self.restrictions.contains(&Restriction::GoalComplete)
            || self.restrictions.contains(&Restriction::Retired)
        {
            return Ok(());
        }
        let key = LimitKey::ProfitStop;
        if self.confirmations.entry(key).or_default().update(
            hit,
            moment.wall_s,
            self.breach_confirm_s,
        )? {
            self.confirmations.remove(&key);
            self.restrictions.insert(Restriction::GoalComplete);
            journal.push(RiskEvent::GoalCompleted {
                reason: Some(GoalReason::ProfitStopReached),
                then: Some(ThenAction::DiscretionaryExitAllThenRetire),
                on_complete: None,
            });
        }
        Ok(())
    }

    /// A book the step leaves flat has nothing to go stale (§5.2). Then one event per instrument
    /// restriction the step changed, never one standing for another (§5.10): a removal or re-admission
    /// carries its universe change's reason, and `stale_mark` carries `no_sane_mark` when set and
    /// `sane_mark` when cleared.
    fn instrument_events(
        &mut self,
        before: &BTreeSet<InstrumentRestriction>,
        input: &Input,
        journal: &mut Vec<RiskEvent>,
    ) {
        if self.qty.is_zero() {
            self.instrument_restrictions
                .remove(&InstrumentRestriction::StaleMark);
        }
        for restriction in [
            InstrumentRestriction::RemovedInstrument,
            InstrumentRestriction::StaleMark,
        ] {
            let active = self.instrument_restrictions.contains(&restriction);
            if active == before.contains(&restriction) {
                continue;
            }
            let reason = match input {
                Input::UniverseChanged { reason, .. }
                    if restriction == InstrumentRestriction::RemovedInstrument =>
                {
                    RestrictionReason::Removal(*reason)
                }
                _ if active => RestrictionReason::NoSaneMark,
                _ => RestrictionReason::SaneMark,
            };
            journal.push(RiskEvent::InstrumentRestrictionChanged {
                restriction,
                reason,
                active,
            });
        }
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
    /// pauses (§5.4), which [`Restriction::mode`] alone cannot say, until the owner acknowledges it
    /// once flat. Every other restriction's mode is its own.
    fn mode_of(&self, restriction: Restriction) -> AgentMode {
        match (restriction, self.daily_action) {
            (Restriction::DailyLoss, LimitAction::FlattenAndPause)
                if !self.daily.is_some_and(|latch| latch.acknowledged) =>
            {
                AgentMode::Paused
            }
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

/// Whether a rung's drawdown is back past its lift level (§5.5).
fn below_lift(readings: &Readings, index: u8) -> Result<bool, SpecError> {
    readings
        .below_lift
        .get(&index)
        .copied()
        .ok_or(SpecError::InvalidInput {
            what: "a rung the conditions did not read",
        })
}

/// A reported ratio: `numerator ÷ denominator` rounded half to even at 12 places (§5.2).
fn reported(numerator: Usd, denominator: Usd) -> Result<Ratio, SpecError> {
    Ok(numerator.ratio_to(denominator, REPORTED_RATIO_PLACES, Rounding::HalfEven)?)
}

/// The risk clock counts whole seconds (§5.2). An instant between two of them is refused rather than
/// rounded, because either rounding could end a wait up to a second before §5.6 lets it end.
fn on_the_risk_clock(at: UtcNanos) -> Result<(), SpecError> {
    if at.nanos() == 0 {
        Ok(())
    } else {
        Err(SpecError::InvalidInput {
            what: "an instant between two seconds of the risk clock, which counts whole seconds",
        })
    }
}

/// Whole seconds from `from` to `to`, the risk clock's unit (§5.2).
fn elapsed_s(from: UtcNanos, to: UtcNanos) -> Result<u64, SpecError> {
    to.secs()
        .checked_sub(from.secs())
        .and_then(|seconds| u64::try_from(seconds).ok())
        .ok_or(SpecError::ClockWentBackwards)
}

#[cfg(test)]
mod tests;
