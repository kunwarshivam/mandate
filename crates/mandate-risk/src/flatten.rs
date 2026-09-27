//! [`agent_flatten`]: the agent-scoped kill switch's plan (mandate §5.5, trading spec §5.5),
//! reproducing `ref.py`'s `agent_flatten`.

use mandate_num::Adverse;

use crate::{
    AgentMode, AssetClass, DeferredSell, FlattenInitiator, FlattenInput, FlattenPlan,
    FlattenPricing, FlattenSell, GateError, Purpose, Session,
};

/// The final mode first; then this agent's own client order ids, sorted, and never the
/// account-wide endpoints; then one sell of the sub-ledger quantity per non-flat position, priced
/// by the session alone (DEC-129 item 21). Outside the regular session an equity sell waits for
/// the open, unless the owner confirmed the bid, when it sells now to a floor.
pub(crate) fn agent_flatten(input: &FlattenInput<'_>) -> Result<FlattenPlan, GateError> {
    let owner = input.initiator == FlattenInitiator::Owner;
    let floor = match (owner, input.owner_confirmed_bid) {
        (true, Some(bid)) => Some(match input.owner_floor_price {
            Some(explicit) => explicit,
            None => bid.collar_bound(input.max_exit_offset, Adverse::Down)?,
        }),
        _ => None,
    };
    let regular = input.session == Session::Regular;
    let pricing = if regular {
        FlattenPricing::MarketOrLadder
    } else {
        FlattenPricing::ExitPriceLadder
    };
    let mut sells = Vec::new();
    let mut deferred_sells = Vec::new();
    for position in input.agent_positions {
        if position.agent != input.agent || position.qty.is_zero() {
            continue;
        }
        let waits_for_open = position.asset_class == AssetClass::UsEquity && !regular;
        match (waits_for_open, floor) {
            (true, None) => deferred_sells.push(DeferredSell {
                instrument: position.instrument.clone(),
                qty: position.qty,
            }),
            (true, Some(floor_price)) => sells.push(FlattenSell {
                instrument: position.instrument.clone(),
                qty: position.qty,
                pricing,
                floor_price: Some(floor_price),
                rests_at_floor_then_waits_for_open: true,
            }),
            (false, _) => sells.push(FlattenSell {
                instrument: position.instrument.clone(),
                qty: position.qty,
                pricing,
                floor_price: None,
                rests_at_floor_then_waits_for_open: false,
            }),
        }
    }
    Ok(FlattenPlan {
        mode_applied_first: if owner {
            AgentMode::Stopped
        } else {
            AgentMode::Paused
        },
        purpose: if owner {
            Purpose::OwnerExit
        } else {
            Purpose::RiskExit
        },
        cancel_client_order_ids: input
            .open_orders
            .iter()
            .filter(|(_, order)| order.agent == input.agent)
            .map(|(id, _)| *id)
            .collect(),
        cancel_all_endpoint: false,
        close_position_endpoint: false,
        sells,
        deferred_sells,
    })
}
