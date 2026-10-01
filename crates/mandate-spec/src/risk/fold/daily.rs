//! The daily loss over risk days (§5.4): the rollover, a breach carried over it against the day it
//! began in, the renewal, and the lift.

use mandate_num::Usd;
use mandate_time::UtcNanos;

use super::{Fold, Limit, Moment, elapsed_s, reading};
use crate::SpecError;
use crate::document::LimitAction;
use crate::risk::limits::Readings;
use crate::risk::{Confirmation, LimitKey, Restriction, RiskEvent, TriggerReason};

/// A latched daily loss (§5.4).
///
/// A renewal writes a fresh latch, so it is unacknowledged whatever came before: only a
/// `flatten_and_pause` daily loss is acknowledged, and a renewed one pauses the agent again until the
/// owner acknowledges the new breach (DEC-167 item 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DailyLatch {
    /// When it latched or was last renewed, which `daily_breach_min_s` counts from.
    since: UtcNanos,
    /// Whether a risk day has started since then, which both the lift and a renewal wait for.
    day_started: bool,
    /// Whether the owner acknowledged a `flatten_and_pause` daily loss once flat, which moves its
    /// restriction to `exits_only` and lets it lift (§5.4).
    pub(super) acknowledged: bool,
}

impl DailyLatch {
    fn at(since: UtcNanos) -> Self {
        Self {
            since,
            day_started: false,
            acknowledged: false,
        }
    }
}

/// A daily breach still confirming at the rollover, which keeps confirming against the day it began
/// in (§5.4). It is not reported as `pending`: the new day starts with no confirmation of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Rollover {
    day_start: Usd,
    confirmation: Confirmation,
}

impl Fold {
    /// `RiskDayStarted`: E₀ becomes E (§5.4). A daily breach still confirming is carried over
    /// against the day it began in, and a latched daily loss has seen its new day. The daily loss's
    /// hard wait, if a quote armed one, carries on under the new day.
    ///
    /// A carried breach is decided within `breach_confirm_s`, at most 300 s. One that latches does so
    /// on a tick the executor journals, because the tick emits an event (§5.2); one that is dropped
    /// needs no tick, and the next input drops it. So a carried breach still undecided at the next
    /// rollover means the stream lost the tick that latched it, and the step is `invalid_input`
    /// rather than a guess at which day it belongs to.
    pub(super) fn start_day(
        &mut self,
        wall_s: u64,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        let equity = self.equity()?;
        if let Some(mut carried) = self.rollover.take() {
            let (hit, _) = self.limits.daily_loss(equity, carried.day_start)?;
            if carried
                .confirmation
                .update(hit, wall_s, self.breach_confirm_s)?
                || carried.confirmation.is_pending()
            {
                return Err(SpecError::InvalidInput {
                    what: "a daily breach carried over one rollover and still undecided at the next",
                });
            }
        }
        let today = self.confirmations.remove(&LimitKey::MaxDailyLoss);
        match self.daily.as_mut() {
            Some(latch) => latch.day_started = true,
            None => {
                self.rollover =
                    today
                        .filter(Confirmation::is_pending)
                        .map(|confirmation| Rollover {
                            day_start: self.day_start,
                            confirmation,
                        });
            }
        }
        self.day_start = equity;
        journal.push(RiskEvent::RiskDayStarted {
            day_start_equity: equity,
        });
        Ok(())
    }

    /// The daily loss, after the ladder and before the floor (§5.2): a breach carried over the
    /// rollover first, then the day's own trigger while unlatched, or, once a latched limit has seen
    /// a new risk day, its renewal or its lift (§5.4).
    ///
    /// A renewal is a breach of the new day's line confirmed by breach time, or at the 1.25x level on
    /// one sane quote: it only extends a latch that already holds, so it adds no restriction and
    /// fires no kill switch. The lift needs the new day and `daily_breach_min_s` since the breach, and
    /// for `flatten_and_pause` the owner's acknowledgment.
    ///
    /// A ladder disarmed by a completed goal disarms the daily loss with it (§3.1): nothing is
    /// evaluated, and a latch already held stays.
    pub(super) fn daily_loss(
        &mut self,
        readings: &Readings,
        equity: Usd,
        moment: Moment,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        if self.disarmed {
            return Ok(());
        }
        let limit = Limit {
            key: LimitKey::MaxDailyLoss,
            action: self.daily_action.into(),
            restriction: Restriction::DailyLoss,
        };
        self.resolve_rollover(limit, equity, moment, journal)?;
        let (hit, hard) = reading(readings, limit.key)?;
        let Some(latch) = self.daily else {
            if let Some(trigger) = self.decide(limit.key, (hit, hard), moment, journal)? {
                self.latch_daily(limit, trigger.reason(), journal);
            }
            return Ok(());
        };
        if !latch.day_started {
            return Ok(());
        }
        let renewed = self.confirmations.entry(limit.key).or_default().update(
            hit,
            moment.wall_s,
            self.breach_confirm_s,
        )? || (hard && moment.quote);
        if renewed {
            self.confirmations.remove(&limit.key);
            self.daily = Some(DailyLatch::at(self.at));
            journal.push(RiskEvent::RiskLimitTriggered {
                limit: limit.key,
                action: limit.action,
                reason: Some(TriggerReason::NewDayBreach),
            });
        } else if (self.daily_action == LimitAction::ExitsOnly || latch.acknowledged)
            && elapsed_s(latch.since, self.at)? >= u64::from(self.daily_breach_min_s)
        {
            self.daily = None;
            self.confirmations.remove(&limit.key);
            self.restrictions.remove(&limit.restriction);
            journal.push(RiskEvent::RiskLimitLifted {
                limit: limit.key,
                action: None,
                reason: None,
            });
        }
        Ok(())
    }

    /// A breach carried over the rollover keeps confirming against the previous day's E₀, on its soft
    /// level only: it latches with `resolved_at_rollover` if it confirms, and is dropped once it is no
    /// longer pending (§5.4). The hard trigger is the day's own, against the new E₀.
    fn resolve_rollover(
        &mut self,
        limit: Limit,
        equity: Usd,
        moment: Moment,
        journal: &mut Vec<RiskEvent>,
    ) -> Result<(), SpecError> {
        let Some(mut carried) = self.rollover.take() else {
            return Ok(());
        };
        let (hit, _) = self.limits.daily_loss(equity, carried.day_start)?;
        if carried
            .confirmation
            .update(hit, moment.wall_s, self.breach_confirm_s)?
        {
            self.latch_daily(limit, Some(TriggerReason::ResolvedAtRollover), journal);
        } else if carried.confirmation.is_pending() {
            self.rollover = Some(carried);
        }
        Ok(())
    }

    /// Latches the daily loss (§5.4). A breach carried over the rollover has nothing left to decide
    /// once the limit holds, and the day's hard wait ends with the latch, as every limit's does
    /// (§5.6).
    fn latch_daily(
        &mut self,
        limit: Limit,
        reason: Option<TriggerReason>,
        journal: &mut Vec<RiskEvent>,
    ) {
        self.daily = Some(DailyLatch::at(self.at));
        self.rollover = None;
        self.clear_hard(limit.key);
        self.latch(limit, reason, journal);
    }
}
