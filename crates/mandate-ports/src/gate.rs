//! The gate's dry run over the runtime's proposal (E6-3, E6-6 to E6-8).

use mandate_risk::{Decision, ProposedOrder};
use mandate_runtime::Proposal;

use crate::context::{AdvisoryGateContext, AdvisoryOrderFacts};
use crate::error::PortError;

const STORY: &str = "the trusted run context";

/// The order the gate evaluates: the proposal's instrument, side, quantity and limit, and every
/// other field from `facts`.
///
/// # Errors
/// [`PortError::Absent`] for an instrument the gate cannot name.
pub fn proposed_order(
    proposal: &Proposal,
    facts: &AdvisoryOrderFacts,
) -> Result<ProposedOrder, PortError> {
    let _ = (proposal, facts);
    Err(PortError::Absent { what: STORY })
}

/// [`mandate_risk::evaluate`] over `context` for `proposal`.
///
/// # Errors
/// As [`proposed_order`], and [`PortError::Gate`] for an order the gate refuses to evaluate.
pub fn evaluate(proposal: &Proposal, context: &AdvisoryGateContext) -> Result<Decision, PortError> {
    let _ = (proposal, context);
    Err(PortError::Absent { what: STORY })
}
