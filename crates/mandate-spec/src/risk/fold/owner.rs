//! The inputs that come from the owner or from a version rather than the market (slice R4):
//! acknowledgments (§5.4, §5.8), allocation changes (§5.1), floor loosening (§5.7), goal completion
//! (§3.1), and retirement (§5.7).
//!
//! Each one either applies or is refused with a [`Rejection`]. A refused one changes nothing, and a
//! refused version journals its refusal (DEC-128 item 16). None of them is a path by which a limit
//! goes away except its own: an acknowledgment lifts the ladder, a loosening version lifts the floor,
//! and an allocation change neither triggers nor lifts anything (MI-2, MI-3).

use mandate_num::{Rounding, Usd, UsdExact};
use mandate_time::UtcNanos;

use super::{Fold, reading};
use crate::document::{LadderAction, LimitAction, OnComplete};
use crate::risk::limits::Figures;
use crate::risk::{
    ApplyResult, Latch, LiftReason, LimitKey, Rejection, Restriction, RiskEvent, StopReason,
    ThenAction, TriggerReason, decimal, risk_day,
};
use crate::{SchemaDec, SpecError};

/// The places §5.1 scales H, E₀, C, and L to, rounding up so no loss fraction falls.
const SCALED_PLACES: u32 = 12;

impl Fold {
    /// The owner's acknowledgment (§5.4, §5.8). The lifetime floor is never acknowledged.
    pub(super) fn acknowledge(
        &mut self,
        latch: Latch,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<Option<Rejection>, SpecError> {
        match latch {
            Latch::LifetimeFloor => Ok(Some(Rejection::NotAcknowledgeable)),
            Latch::DailyLoss => Ok(self.acknowledge_daily()),
            Latch::DrawdownLadder => self.acknowledge_ladder(journal),
        }
    }

    /// A latched `flatten_and_pause` daily loss, once the agent is flat, moves to `exits_only` and
    /// may lift at its time (§5.4). It journals nothing of its own: the mode change says it.
    fn acknowledge_daily(&mut self) -> Option<Rejection> {
        let flat = self.qty.is_zero();
        let flattening = self.daily_action == LimitAction::FlattenAndPause;
        let Some(latch) = self
            .daily
            .as_mut()
            .filter(|latch| flattening && !latch.acknowledged)
        else {
            return Some(Rejection::NothingToAcknowledge);
        };
        if !flat {
            return Some(Rejection::FlattenInProgress);
        }
        latch.acknowledged = true;
        None
    }

    /// Acknowledging the ladder (§5.8): refused while a drawdown flatten has not left the agent flat;
    /// otherwise H := E, the latched rungs lift, their confirmations and hard waits end, and every
    /// scale rung is active again, to step back one at a time from the highest.
    fn acknowledge_ladder(
        &mut self,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<Option<Rejection>, SpecError> {
        let rungs: Vec<LimitKey> = self
            .latched
            .iter()
            .filter(|key| matches!(key, LimitKey::DrawdownRung(_)))
            .copied()
            .collect();
        if rungs.is_empty() {
            return Ok(Some(Rejection::NothingToAcknowledge));
        }
        if self.restrictions.contains(&Restriction::DrawdownFlatten) && !self.qty.is_zero() {
            return Ok(Some(Rejection::FlattenInProgress));
        }
        let equity = self.equity()?;
        journal.push(RiskEvent::HighWaterMarkReset {
            from: self.high_water,
            to: equity,
        });
        self.high_water = equity;
        for key in rungs {
            self.latched.remove(&key);
            journal.push(RiskEvent::RiskLimitLifted {
                limit: key,
                action: None,
                reason: Some(LiftReason::OwnerAcknowledged),
            });
        }
        self.confirmations
            .retain(|key, _| !matches!(key, LimitKey::DrawdownRung(_)));
        let waiting: Vec<LimitKey> = self
            .hard_since
            .keys()
            .filter(|key| matches!(key, LimitKey::DrawdownRung(_)))
            .copied()
            .collect();
        for key in waiting {
            self.clear_hard(key);
            journal.push(RiskEvent::RiskLimitLifted {
                limit: key,
                action: None,
                reason: Some(LiftReason::HardBreachCleared),
            });
        }
        self.restrictions.remove(&Restriction::DrawdownExitsOnly);
        self.restrictions.remove(&Restriction::DrawdownFlatten);
        for (index, rung) in &mut self.scale {
            if rung.active_s.is_none() {
                rung.active_s = Some(0);
                journal.push(RiskEvent::RiskLimitTriggered {
                    limit: LimitKey::DrawdownRung(*index),
                    action: LadderAction::ScaleSizes,
                    reason: Some(TriggerReason::AfterReset),
                });
            }
            rung.lift_s = 0;
            rung.lifting = false;
        }
        self.stepping = true;
        Ok(None)
    }

    /// An allocation change of Δ (§5.1), after time has been settled. Refused if Δ > 0 while a limit
    /// is latched (MI-7), if E + Δ would not cover the agent's exposure or would not be positive, or
    /// if after scaling any limit condition, soft or at its 1.25x hard level, would be newly true.
    /// Otherwise A and N move by Δ, and H, E₀, C, and L are scaled by (E + Δ) ÷ E rounded up at 12
    /// places, so no drawdown or loss fraction falls (MI-2).
    ///
    /// # Errors
    /// `invalid_input` at an equity that is not positive, which §5.1's ratio cannot divide by.
    pub(super) fn allocate(
        &mut self,
        delta: Usd,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<Option<Rejection>, SpecError> {
        let before = self.figures()?;
        if !UsdExact::of(before.equity).is_positive()? {
            return Err(SpecError::InvalidInput {
                what: "an allocation change at an equity that is not positive, which §5.1 cannot scale",
            });
        }
        let equity = before.equity.checked_add(delta)?;
        let exposure = self.qty.notional(self.mark)?;
        let latched = !self.latched.is_empty() || self.daily.is_some();
        let refused = if UsdExact::of(delta).is_positive()? && latched {
            Some(Rejection::IncreaseBlockedWhileLatched)
        } else if !UsdExact::of(equity).is_positive()?
            || UsdExact::of(equity).is_below(UsdExact::of(exposure))?
        {
            Some(Rejection::EquityBelowExposure)
        } else {
            None
        };
        let scale = |figure: Usd| {
            UsdExact::of(figure)
                .checked_mul(UsdExact::of(equity))?
                .quotient(
                    UsdExact::of(before.equity),
                    SCALED_PLACES,
                    Rounding::Ceiling,
                )
        };
        let after = Figures {
            equity,
            high_water: scale(before.high_water)?,
            day_start: scale(before.day_start)?,
            capital: scale(before.capital)?,
            inherited: scale(before.inherited)?,
        };
        let refused = match refused {
            Some(rejection) => Some(rejection),
            None => self.newly_true(&before, &after)?,
        };
        if let Some(reason) = refused {
            journal.push(RiskEvent::MandateVersionApplied {
                result: ApplyResult::Rejected { reason },
            });
            return Ok(Some(reason));
        }
        self.allocation = self.allocation.checked_add(delta)?;
        self.net_contributed = self.net_contributed.checked_add(delta)?;
        self.high_water = after.high_water;
        self.day_start = after.day_start;
        self.capital = after.capital;
        self.inherited = after.inherited;
        journal.push(RiskEvent::MandateVersionApplied {
            result: ApplyResult::Applied {
                allocation_change: Some(delta),
                max_loss_from_allocation: None,
            },
        });
        Ok(None)
    }

    /// `would_trigger_limit` when a condition, soft or hard, holds after scaling and did not before
    /// (§5.1).
    fn newly_true(
        &self,
        before: &Figures,
        after: &Figures,
    ) -> Result<Option<Rejection>, SpecError> {
        let was = self.limits.conditions(before)?.limits;
        let would = self.limits.conditions(after)?.limits;
        for (key, (soft, hard)) in would {
            let (was_soft, was_hard) = was.get(&key).copied().ok_or(SpecError::InvalidInput {
                what: "a limit the conditions did not read",
            })?;
            if (soft && !was_soft) || (hard && !was_hard) {
                return Ok(Some(Rejection::WouldTriggerLimit));
            }
        }
        Ok(None)
    }

    /// A version raising `max_loss_from_allocation` to f′ (§5.7). Refused if it does not raise it; while
    /// the floor is latched, refused without independent approval until the first full risk day after
    /// the confirmation day has ended, and refused if E would not be strictly above the new floor.
    /// Applied, it lifts a latched floor, whose confirmation starts afresh.
    pub(super) fn loosen_floor(
        &mut self,
        fraction: &SchemaDec,
        confirmed_at: UtcNanos,
        independent_approval: bool,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<Option<Rejection>, SpecError> {
        let path = "/capital/max_loss_from_allocation";
        let current = decimal(&self.limits.max_loss_from_allocation, path)?;
        let latched = self.latched.contains(&LimitKey::LifetimeFloor);
        let mut loosened = self.limits.clone();
        loosened.max_loss_from_allocation = fraction.clone();
        let refused = if !current.is_below(decimal(fraction, path)?)? {
            Some(Rejection::NotLoosening)
        } else if latched && !independent_approval && self.at < first_full_day_ends(confirmed_at)? {
            Some(Rejection::WaitingPeriod)
        } else if latched
            && reading(
                &loosened.conditions(&self.figures()?)?,
                LimitKey::LifetimeFloor,
            )?
            .0
        {
            Some(Rejection::StillBelowNewFloor)
        } else {
            None
        };
        if let Some(reason) = refused {
            journal.push(RiskEvent::MandateVersionApplied {
                result: ApplyResult::Rejected { reason },
            });
            return Ok(Some(reason));
        }
        self.limits = loosened;
        journal.push(RiskEvent::MandateVersionApplied {
            result: ApplyResult::Applied {
                allocation_change: None,
                max_loss_from_allocation: Some(fraction.clone()),
            },
        });
        if latched {
            self.latched.remove(&LimitKey::LifetimeFloor);
            self.restrictions.remove(&Restriction::LifetimeFloor);
            self.confirmations.remove(&LimitKey::LifetimeFloor);
            journal.push(RiskEvent::RiskLimitLifted {
                limit: LimitKey::LifetimeFloor,
                action: None,
                reason: Some(LiftReason::VersionLoosened),
            });
        }
        Ok(None)
    }

    /// The goal completed, so its `on_complete` applies (§3.1): `hold_protected` and `disarm_ladder`
    /// restrict the agent to `goal_complete`, `disarm_ladder` also stops the ladder and the daily loss
    /// while the floor stays armed, and `release` hands the position to the owner and retires the
    /// agent as any retirement does, carrying its net dollar loss to the connection (§5.7, DEC-270),
    /// with E still valuing the position at the last mark. A `profit_stop` has no `on_complete`: its one outcome is a discretionary exit of every
    /// position and then retirement, under `goal_complete` (DEC-167 item 8).
    pub(super) fn complete_goal(&mut self, journal: &mut Vec<RiskEvent>) -> Result<(), SpecError> {
        let Some(on_complete) = self.on_complete else {
            journal.push(RiskEvent::GoalCompleted {
                reason: None,
                then: Some(ThenAction::DiscretionaryExitAllThenRetire),
                on_complete: None,
            });
            self.restrictions.insert(Restriction::GoalComplete);
            return Ok(());
        };
        journal.push(RiskEvent::GoalCompleted {
            reason: None,
            then: None,
            on_complete: Some(on_complete),
        });
        match on_complete {
            OnComplete::Release => {
                journal.push(RiskEvent::PositionReleased { qty: self.qty });
                self.retire(StopReason::GoalComplete, journal)?;
            }
            OnComplete::HoldProtected => {
                self.restrictions.insert(Restriction::GoalComplete);
            }
            OnComplete::DisarmLadder => {
                self.restrictions.insert(Restriction::GoalComplete);
                self.disarmed = true;
                self.rollover = None;
                self.confirmations
                    .retain(|key, _| *key == LimitKey::LifetimeFloor);
            }
        }
        Ok(())
    }

    /// The agent retires (§5.7): the connection carries its net dollar loss, max(0, N − E), which a
    /// withdrawal cannot shrink because it lowers N and E alike (MI-14).
    pub(super) fn retire(
        &mut self,
        reason: StopReason,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        let loss = self.net_contributed.checked_sub(self.equity()?)?;
        journal.push(RiskEvent::AgentStopped {
            reason,
            loss_carry_usd: if loss.is_negative() { Usd::ZERO } else { loss },
        });
        self.restrictions.insert(Restriction::Retired);
        Ok(())
    }
}

/// When a single user may loosen a latched floor: the end of the first full risk day after the one
/// the floor's confirmation fell in (§5.7).
fn first_full_day_ends(confirmed_at: UtcNanos) -> Result<UtcNanos, SpecError> {
    Ok(risk_day(risk_day(confirmed_at)?.ends_at)?.ends_at)
}
