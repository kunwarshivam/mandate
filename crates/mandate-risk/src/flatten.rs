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

    /// A transcribed `ref.py` plan: the mode, the purpose, the deferred count, and each sell's
    /// pricing, floor and resting remainder.
    type RefPlan = (
        &'static str,
        &'static str,
        usize,
        Vec<(&'static str, Option<&'static str>, bool)>,
    );

    /// What `ref.py`'s `agent_flatten` plans for one equity or crypto position of 10, transcribed
    /// from its branches in `ref.py`'s own vocabulary and never computed through this crate: the
    /// mode, the purpose, the deferred count, and each sell's pricing, floor and resting remainder.
    /// The confirmed bid is 100 and the offset 0.03, so the computed floor is 97 by hand.
    fn ref_py_agent_flatten(
        session: &str,
        initiator: &str,
        asset_class: &str,
        confirmed: bool,
        explicit_floor: Option<&'static str>,
    ) -> RefPlan {
        let owner = initiator == "owner";
        let floor = if owner && confirmed {
            Some(explicit_floor.unwrap_or("97"))
        } else {
            None
        };
        let outside = session != "regular";
        let (sells, deferred) = if asset_class == "us_equity" && outside && !(owner && confirmed) {
            (Vec::new(), 1)
        } else if asset_class == "us_equity" && outside {
            (vec![("exit_price_ladder", floor, true)], 0)
        } else if outside {
            (vec![("exit_price_ladder", None, false)], 0)
        } else {
            (vec![("market_or_ladder", None, false)], 0)
        };
        (
            if owner { "stopped" } else { "paused" },
            if owner { "owner_exit" } else { "risk_exit" },
            deferred,
            sells,
        )
    }

    /// Every session × initiator × asset class × confirmed bid × explicit floor, 80 rows, each
    /// against the transcribed `ref.py` above, so no branch of the plan (the session test, the
    /// floor's owner-only scope, the explicit floor's precedence) goes unpinned.
    #[test]
    fn every_flatten_branch_matches_ref_py() -> Result<(), GateError> {
        let sessions = [
            (Session::Overnight, "overnight"),
            (Session::PreMarket, "pre_market"),
            (Session::Regular, "regular"),
            (Session::AfterHours, "after_hours"),
            (Session::Continuous, "continuous"),
        ];
        let initiators = [
            (FlattenInitiator::RiskLimit, "risk_limit"),
            (FlattenInitiator::Owner, "owner"),
        ];
        let classes = [
            (AssetClass::UsEquity, "us_equity"),
            (AssetClass::Crypto, "crypto"),
        ];
        let orders = BTreeMap::new();
        let broker = BTreeMap::new();
        let mut rows = 0;
        let mut mismatches = Vec::new();
        for (session, session_name) in sessions {
            for (initiator, initiator_name) in initiators {
                for (class, class_name) in classes {
                    for confirmed in [false, true] {
                        for explicit in [None, Some("98.5")] {
                            let positions = vec![AgentPosition {
                                agent: AgentId(1),
                                instrument: AssetId::new("a")
                                    .map_err(|_| GateError::InstrumentUnknown)?,
                                asset_class: class,
                                qty: Qty::parse("10")?,
                            }];
                            let plan = agent_flatten(&FlattenInput {
                                agent: AgentId(1),
                                open_orders: &orders,
                                agent_positions: &positions,
                                broker_positions: &broker,
                                session,
                                initiator,
                                owner_confirmed_bid: if confirmed {
                                    Some(Price::parse("100")?)
                                } else {
                                    None
                                },
                                max_exit_offset: Fraction::parse("0.03")?,
                                owner_floor_price: explicit.map(Price::parse).transpose()?,
                            })?;
                            let (mode, purpose, deferred, sells) = ref_py_agent_flatten(
                                session_name,
                                initiator_name,
                                class_name,
                                confirmed,
                                explicit,
                            );
                            let want_sells = sells
                                .into_iter()
                                .map(|(pricing, floor, rests)| {
                                    Ok((pricing, floor.map(Price::parse).transpose()?, rests))
                                })
                                .collect::<Result<Vec<_>, GateError>>()?;
                            let got = (
                                match plan.mode_applied_first {
                                    AgentMode::Stopped => "stopped",
                                    AgentMode::Paused => "paused",
                                    _ => "other",
                                },
                                match plan.purpose {
                                    Purpose::OwnerExit => "owner_exit",
                                    Purpose::RiskExit => "risk_exit",
                                    _ => "other",
                                },
                                plan.deferred_sells.len(),
                                plan.sells
                                    .iter()
                                    .map(|sell| {
                                        (
                                            match sell.pricing {
                                                FlattenPricing::MarketOrLadder => {
                                                    "market_or_ladder"
                                                }
                                                FlattenPricing::ExitPriceLadder => {
                                                    "exit_price_ladder"
                                                }
                                            },
                                            sell.floor_price,
                                            sell.rests_at_floor_then_waits_for_open,
                                        )
                                    })
                                    .collect::<Vec<_>>(),
                            );
                            let want = (mode, purpose, deferred, want_sells);
                            rows += 1;
                            if got != want {
                                mismatches.push(format!(
                                    "{session_name} {initiator_name} {class_name} \
                                     confirmed={confirmed} explicit={explicit:?}: \
                                     got {got:?}, ref.py {want:?}"
                                ));
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(
            (rows, mismatches),
            (80, Vec::<String>::new()),
            "every flatten row matches ref.py's agent_flatten"
        );
        Ok(())
    }
}
