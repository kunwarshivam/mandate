//! The anti-fatigue bounds (EI-13, DEC-156 item 5): at most [`ASK_BUDGET_PER_RISK_DAY`] requests
//! per agent per America/New_York risk day, no re-ask of an instrument the owner skipped until the
//! next risk day or applied version, and none after a timeout for one `timeout_s`. Every bound only
//! removes asks, so none adds risk.

use mandate_time::{Date, UtcNanos, new_york_date_and_hour};

use crate::{ApprovalError, RiskClock};

pub const ASK_BUDGET_PER_RISK_DAY: u32 = 10;

/// What the ledger folds, from the agent stream, in journal order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AskEvent {
    Requested {
        instrument: String,
        at: RiskClock,
    },
    OwnerSkipped {
        instrument: String,
        at: RiskClock,
    },
    TimedOut {
        instrument: String,
        at: RiskClock,
        timeout_s: i64,
    },
    VersionApplied {
        at: RiskClock,
    },
}

/// One agent's folded asks. The events are kept as journaled; [`ask_permit`] reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AskLedger {
    pub events: Vec<AskEvent>,
}

/// `DecisionMade.ask_suppressed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Suppression {
    Budget,
    SkippedToday,
    RecentTimeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskPermit {
    Ask,
    Suppressed(Suppression),
}

/// Whether an ASK for `instrument` at `at` may be requested.
///
/// # Errors
/// [`ApprovalError::Unrepresentable`] for a clock outside `UtcNanos`'s range.
pub fn ask_permit(
    ledger: &AskLedger,
    instrument: &str,
    at: RiskClock,
) -> Result<AskPermit, ApprovalError> {
    let today = risk_day(at)?;
    let mut asked: u32 = 0;
    let mut skipped = false;
    let mut timed_out = false;
    for event in &ledger.events {
        match event {
            AskEvent::Requested { at: t, .. } => {
                if risk_day(*t)? == today {
                    asked = asked.saturating_add(1);
                }
            }
            AskEvent::OwnerSkipped {
                instrument: i,
                at: t,
            } => {
                if i == instrument && risk_day(*t)? == today {
                    skipped = true;
                }
            }
            AskEvent::VersionApplied { .. } => skipped = false,
            AskEvent::TimedOut {
                instrument: i,
                at: t,
                timeout_s,
            } => {
                if i == instrument && t.0 <= at.0 && at.0 < t.0.saturating_add(*timeout_s) {
                    timed_out = true;
                }
            }
        }
    }
    Ok(if asked >= ASK_BUDGET_PER_RISK_DAY {
        AskPermit::Suppressed(Suppression::Budget)
    } else if skipped {
        AskPermit::Suppressed(Suppression::SkippedToday)
    } else if timed_out {
        AskPermit::Suppressed(Suppression::RecentTimeout)
    } else {
        AskPermit::Ask
    })
}

/// The America/New_York calendar date an instant falls on: the risk day.
fn risk_day(at: RiskClock) -> Result<Date, ApprovalError> {
    let unrepresentable = |_| ApprovalError::Unrepresentable { what: "clock" };
    let instant = UtcNanos::from_parts(at.0, 0).map_err(unrepresentable)?;
    new_york_date_and_hour(instant)
        .map(|(date, _)| date)
        .map_err(unrepresentable)
}
