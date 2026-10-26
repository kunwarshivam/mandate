//! The gate's dry run over the runtime's proposal (E6-3, E6-6 to E6-8).

use std::collections::BTreeSet;

use mandate_risk::{Advisory, AssetId, Decision, Deferred, GateError, GateInput, ProposedOrder};
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
    with_input(proposal, context, mandate_risk::evaluate)
}

/// [`mandate_risk::evaluate_deferring`] over `context` for `proposal`: the cell's runtime's
/// advisory pass, with `deferred` the checks whose facts nothing journals yet (DEC-2378 item 11).
/// With nothing deferred it answers as [`evaluate`]. Only that dry run may call it: `clippy.toml`
/// disallows it everywhere else, the executor's binding gate included.
///
/// # Errors
/// As [`evaluate`].
#[allow(
    clippy::disallowed_methods,
    reason = "the one port over the advisory pass, which only the cell's runtime's dry run calls (DEC-2378 item 11)"
)]
pub fn evaluate_deferring(
    proposal: &Proposal,
    context: &AdvisoryGateContext,
    deferred: &BTreeSet<Deferred>,
) -> Result<Advisory, PortError> {
    with_input(proposal, context, |input| {
        mandate_risk::evaluate_deferring(input, deferred)
    })
}

/// `gate` over the gate input `context` and `proposal` make.
fn with_input<T>(
    proposal: &Proposal,
    context: &AdvisoryGateContext,
    gate: impl FnOnce(&GateInput<'_>) -> Result<T, GateError>,
) -> Result<T, PortError> {
    let proposed = proposed_order(proposal, &context.order)?;
    Ok(gate(&GateInput {
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
