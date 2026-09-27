//! Re-validation: the brief's checks 8 to 12, which decide `ApprovalRevalidated.result` (E8-3,
//! DEC-156 items 2 to 4).

use mandate_num::Price;

use crate::ApprovalError;
use crate::admit::Request;
use crate::content::BoundAction;

/// The effective mode this step applies. Anything stricter than `normal` skips.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModeNow {
    Normal,
    ExitsOnly,
    Paused,
    Stopped,
}

/// `OrderPlan::classify` for the bound proposal in the current view, with its `DecidedBy` label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Classification {
    Auto,
    Ask { decided_by: String },
    Deny,
}

/// `GateDryRun::check` for the bound order on current state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DryRun {
    Allow,
    Deny { reason: String },
}

/// The values re-validation compares, which the runtime already has or gets through its ports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    pub mandate_version: String,
    pub mode: ModeNow,
    pub instrument_restricted: bool,
    pub in_working_universe: bool,
    pub classification: Classification,
    pub dry_run: DryRun,
    /// The latest folded mark of the bound instrument, if any.
    pub mark_now: Option<Price>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    VersionChanged,
    Mode,
    InstrumentRestricted,
    ReclassifiedDeny,
    ReclassifiedOtherTrigger,
    Gate { reason: String },
    Drift,
}

/// The only order a grant can produce: the bound one. Its fields are private and it is built only
/// from a [`BoundAction`], so a grant cannot re-price or re-size (EI-4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantedOrder(BoundAction);

impl GrantedOrder {
    /// The bound order, which is the whole of what the intent carries.
    pub fn order(&self) -> &BoundAction {
        &self.0
    }
}

/// `ApprovalRevalidated.result`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Revalidation {
    Act(GrantedOrder),
    Skip(SkipReason),
}

/// Checks 8 to 12 in order, for a grant [`crate::admit`] admitted in the same step.
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-3.
pub fn revalidate(request: &Request, now: &Current) -> Result<Revalidation, ApprovalError> {
    let _ = (request, now);
    Err(ApprovalError::Unimplemented { story: "E8-3" })
}
