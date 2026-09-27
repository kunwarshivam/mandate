//! The anti-fatigue bounds (EI-13, DEC-156 item 5): at most [`ASK_BUDGET_PER_RISK_DAY`] requests
//! per agent per America/New_York risk day, no re-ask of an instrument the owner skipped until the
//! next risk day or applied version, and none after a timeout for one `timeout_s`. Every bound only
//! removes asks, so none adds risk.

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
/// [`ApprovalError::Unimplemented`] until E8-2.
pub fn ask_permit(
    ledger: &AskLedger,
    instrument: &str,
    at: RiskClock,
) -> Result<AskPermit, ApprovalError> {
    let _ = (ledger, instrument, at);
    Err(ApprovalError::Unimplemented { story: "E8-2" })
}
