//! Re-validation: the brief's checks 8 to 12, which decide `ApprovalRevalidated.result` (E8-3,
//! DEC-156 items 2 to 4).

use mandate_num::Price;

use crate::ApprovalError;
use crate::admit::Request;
use crate::content::BoundAction;
use crate::drift::within_band;

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
/// The act carries the bound order unchanged; nothing here re-prices or re-sizes it (EI-4).
///
/// # Errors
/// None: every input has an answer, and drift that cannot be computed is outside the band. The
/// `Result` is the stub API's shape.
pub fn revalidate(request: &Request, now: &Current) -> Result<Revalidation, ApprovalError> {
    let bound = &request.content.bound;
    let skip = if now.mandate_version != bound.mandate_version {
        Some(SkipReason::VersionChanged)
    } else if now.mode != ModeNow::Normal {
        Some(SkipReason::Mode)
    } else if now.instrument_restricted || !now.in_working_universe {
        Some(SkipReason::InstrumentRestricted)
    } else if let Some(reason) = reclassified(&now.classification, &bound.decided_by) {
        Some(reason)
    } else if let DryRun::Deny { reason } = &now.dry_run {
        Some(SkipReason::Gate {
            reason: reason.clone(),
        })
    } else if !within_band(
        bound.reference_mark.as_ref().map(|m| m.price),
        now.mark_now,
        bound.asset_class,
    )? {
        Some(SkipReason::Drift)
    } else {
        None
    };
    Ok(skip.map_or_else(
        || Revalidation::Act(GrantedOrder(bound.clone())),
        Revalidation::Skip,
    ))
}

/// Check 10: `deny` is never overridden, and an `ask` by another trigger is a question the owner
/// has not seen (DEC-156 item 4).
fn reclassified(now: &Classification, bound_trigger: &str) -> Option<SkipReason> {
    match now {
        Classification::Auto => None,
        Classification::Ask { decided_by } if decided_by == bound_trigger => None,
        Classification::Ask { .. } => Some(SkipReason::ReclassifiedOtherTrigger),
        Classification::Deny => Some(SkipReason::ReclassifiedDeny),
    }
}
