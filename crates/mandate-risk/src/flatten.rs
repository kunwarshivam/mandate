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
///
/// The computed floor `bid × (1 − max_exit_offset)` is rounded **up** at the 9 places a price
/// holds, where `ref.py` keeps the exact product: conservative, since a rounded sell floor never
/// admits a price below the exact one, and unreachable in practice, since an equity bid sits on the
/// 0.01 or 0.0001 tick and §5.6's tiers fix the offset at 0.03 or 0.05, so the product fits 9 places.
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use mandate_num::{Fraction, Price, Qty};

    use super::*;
    use crate::{AgentId, AgentPosition, AssetId};

    /// Only the owner's kill switch sells to a floor outside the regular session. An automated
    /// flatten defers the equity sell to the open even when a confirmed bid is present (mandate
    /// §5.5), so the floor path's scope is the initiator and not the bid.
    #[test]
    fn an_automated_flatten_defers_an_equity_sell_even_with_a_confirmed_bid()
    -> Result<(), GateError> {
        let orders = BTreeMap::new();
        let positions = vec![AgentPosition {
            agent: AgentId(1),
            instrument: AssetId::new("a").map_err(|_| GateError::InstrumentUnknown)?,
            asset_class: AssetClass::UsEquity,
            qty: Qty::parse("10")?,
        }];
        let broker = BTreeMap::new();
        let plan = agent_flatten(&FlattenInput {
            agent: AgentId(1),
            open_orders: &orders,
            agent_positions: &positions,
            broker_positions: &broker,
            session: Session::AfterHours,
            initiator: FlattenInitiator::RiskLimit,
            owner_confirmed_bid: Some(Price::parse("100")?),
            max_exit_offset: Fraction::parse("0.03")?,
            owner_floor_price: None,
        })?;
        assert_eq!(
            (plan.sells.len(), plan.deferred_sells.len()),
            (0, 1),
            "an automated flatten waits for the regular session whatever bid is present"
        );
        Ok(())
    }
}
