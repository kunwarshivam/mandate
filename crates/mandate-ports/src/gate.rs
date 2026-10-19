//! The gate's dry run over the runtime's proposal (E6-3, E6-6 to E6-8).

use mandate_risk::{AssetId, Decision, GateInput, ProposedOrder};
use mandate_runtime::Proposal;

use crate::context::{AdvisoryGateContext, AdvisoryOrderFacts};
use crate::error::PortError;

/// The order the gate evaluates: the proposal's instrument, side, quantity and limit, and every
/// other field from `facts`.
///
/// # Errors
/// [`PortError::Absent`] for an instrument the gate cannot name.
pub fn proposed_order(
    proposal: &Proposal,
    facts: &AdvisoryOrderFacts,
) -> Result<ProposedOrder, PortError> {
    Ok(ProposedOrder {
        instrument: AssetId::new(proposal.instrument.as_str()).map_err(|_| PortError::Absent {
            what: "the gate's proposed instrument id",
        })?,
        side: proposal.side,
        qty: proposal.qty,
        limit_price: proposal.limit,
        kind: facts.kind.clone(),
        tif: facts.tif,
        extended_hours: facts.extended_hours,
        origin: facts.origin,
        owner_confirmed_bid: facts.owner_confirmed_bid,
        client_order_id: facts.client_order_id,
        fee_reservation: facts.fee_reservation,
    })
}

/// [`mandate_risk::evaluate`] over `context` for `proposal`.
///
/// # Errors
/// As [`proposed_order`], and [`PortError::Gate`] for an order the gate refuses to evaluate.
pub fn evaluate(proposal: &Proposal, context: &AdvisoryGateContext) -> Result<Decision, PortError> {
    let proposed = proposed_order(proposal, &context.order)?;
    Ok(mandate_risk::evaluate(&GateInput {
        now: context.now,
        pass: context.pass,
        config: &context.config,
        mandate: &context.mandate,
        risk: &context.risk,
        account: &context.account,
        agent: &context.agent,
        instrument: &context.instrument,
        market: &context.market,
        conduct: &context.conduct,
        universe: &context.universe,
        proposed: &proposed,
    })?)
}
