//! The broker's name for each instrument a run trades, beside the asset id the platform names it
//! by (DEC-938 item 1).
//!
//! The executor names an equity by its asset id, the mandate's `asset_id` (mandate spec §3), and
//! sends that id as the order's symbol, which Alpaca accepts. Alpaca's answers name the same
//! equity by its ticker: an order read back, a position, and a `FILL` activity all say `SPY`, and
//! a `FILL` carries no asset id at all. Read as they come, the run's own fill was another
//! instrument's, so the executor took it for external activity (#1355). [`BrokerSymbols`] puts the
//! asset id back on every instrument an answer names by a ticker the run registered, before the
//! executor reads it; a ticker the run did not register stays as the broker spelled it, so an
//! instrument the run does not trade is never taken for one it does. The shell's paper run and the
//! executor process both read their broker's answers through it (DEC-2812).

use std::collections::BTreeMap;

use mandate_accounting::InstrumentId;

use crate::{BrokerOrder, BrokerOutcome, ListedOrder};

/// Each registered ticker, mapped to the asset id the platform names that instrument by.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BrokerSymbols(BTreeMap<InstrumentId, InstrumentId>);

impl BrokerSymbols {
    /// The run's registered instruments, each as `(ticker, asset id)`. A ticker spelled as its own
    /// asset id maps nothing, so the empty map and that one read alike.
    pub fn registered(pairs: impl IntoIterator<Item = (InstrumentId, InstrumentId)>) -> Self {
        Self(
            pairs
                .into_iter()
                .filter(|(ticker, asset)| ticker != asset)
                .collect(),
        )
    }

    /// The platform's name for an instrument the broker named `named`.
    fn platform(&self, named: InstrumentId) -> InstrumentId {
        match self.0.get(&named) {
            Some(asset) => asset.clone(),
            None => named,
        }
    }

    fn order(&self, order: BrokerOrder) -> BrokerOrder {
        BrokerOrder {
            instrument: self.platform(order.instrument),
            legs: order.legs.into_iter().map(|leg| self.order(leg)).collect(),
            ..order
        }
    }

    /// `outcome` with every instrument it names by a registered ticker named by its asset id:
    /// orders and their legs, positions, fills, and listed orders. Every other answer names no
    /// instrument and passes unchanged.
    pub fn outcome(&self, outcome: BrokerOutcome) -> BrokerOutcome {
        match outcome {
            BrokerOutcome::Submitted(order) => BrokerOutcome::Submitted(self.order(order)),
            BrokerOutcome::Order(order) => BrokerOutcome::Order(self.order(order)),
            BrokerOutcome::OpenOrders(orders) => BrokerOutcome::OpenOrders(
                orders.into_iter().map(|order| self.order(order)).collect(),
            ),
            BrokerOutcome::Positions(positions) => BrokerOutcome::Positions(
                positions
                    .into_iter()
                    .map(|position| crate::BrokerPosition {
                        instrument: self.platform(position.instrument),
                        ..position
                    })
                    .collect(),
            ),
            BrokerOutcome::Activities { fills, cursor } => BrokerOutcome::Activities {
                fills: fills
                    .into_iter()
                    .map(|fill| crate::BrokerFill {
                        instrument: self.platform(fill.instrument),
                        ..fill
                    })
                    .collect(),
                cursor,
            },
            BrokerOutcome::Listed {
                client_order_id,
                orders,
            } => BrokerOutcome::Listed {
                client_order_id,
                orders: orders
                    .into_iter()
                    .map(|listed| ListedOrder {
                        order: self.order(listed.order),
                        ..listed
                    })
                    .collect(),
            },
            unnamed @ (BrokerOutcome::DuplicateClientOrderId { .. }
            | BrokerOutcome::Rejected(_)
            | BrokerOutcome::Absent { .. }
            | BrokerOutcome::Account(_)
            | BrokerOutcome::CancelAccepted { .. }
            | BrokerOutcome::AccountWideAccepted) => unnamed,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        ActivityCursor, BrokerFill, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerReject,
        FillId, ListedOrder, OrderType, TimeInForce,
    };
    use mandate_accounting::{InstrumentId, Side};
    use mandate_num::{Price, Qty, SignedQty, Usd};
    use mandate_time::Date;

    use super::BrokerSymbols;

    const ASSET: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";

    fn id(name: &str) -> Result<InstrumentId, String> {
        InstrumentId::new(name).map_err(|e| format!("{name}: {e}"))
    }

    fn spy() -> Result<BrokerSymbols, String> {
        Ok(BrokerSymbols::registered([(id("SPY")?, id(ASSET)?)]))
    }

    fn order(named: &str, legs: Vec<BrokerOrder>) -> Result<BrokerOrder, String> {
        Ok(BrokerOrder {
            broker_order_id: "b-1".to_owned(),
            client_order_id: None,
            instrument: id(named)?,
            side: Side::Buy,
            qty: Qty::parse("1").map_err(|e| e.to_string())?,
            filled_qty: Qty::ZERO,
            limit_price: None,
            stop_price: None,
            status: "filled".to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs,
            created_on: None,
        })
    }

    /// An entry named `named` with one leg named the same.
    fn bracket(named: &str) -> Result<BrokerOrder, String> {
        order(named, vec![order(named, Vec::new())?])
    }

    fn fill(named: &str) -> Result<BrokerFill, String> {
        Ok(BrokerFill {
            fill_id: FillId("f-1".to_owned()),
            client_order_id: Some("md-1".to_owned()),
            broker_order_id: None,
            instrument: id(named)?,
            side: Side::Buy,
            qty: Qty::parse("1").map_err(|e| e.to_string())?,
            price: Price::parse("763.75").map_err(|e| e.to_string())?,
            fees: Usd::ZERO,
            trade_date: Date::parse("2026-10-16").map_err(|e| e.to_string())?,
        })
    }

    fn position(named: &str) -> Result<BrokerPosition, String> {
        Ok(BrokerPosition {
            instrument: id(named)?,
            qty: SignedQty::parse("1").map_err(|e| e.to_string())?,
            avg_entry_price: Price::parse("763.75").map_err(|e| e.to_string())?,
        })
    }

    fn listed(order: BrokerOrder) -> ListedOrder {
        ListedOrder {
            order,
            order_type: OrderType::Limit,
            tif: TimeInForce::Gtc,
        }
    }

    /// #1355: Alpaca names the run's own fill, position and order, legs included, by the ticker,
    /// and the executor reads each by the asset id it sent; an unregistered ticker stays as the
    /// broker spelled it, and nothing registered maps nothing.
    #[test]
    fn a_registered_ticker_reads_as_its_asset_id_in_every_answer() -> Result<(), String> {
        let symbols = spy()?;
        let cursor = ActivityCursor("c-1".to_owned());
        let cases = [
            (
                BrokerOutcome::Submitted(bracket("SPY")?),
                BrokerOutcome::Submitted(bracket(ASSET)?),
            ),
            (
                BrokerOutcome::Order(bracket("SPY")?),
                BrokerOutcome::Order(bracket(ASSET)?),
            ),
            (
                BrokerOutcome::OpenOrders(vec![bracket("SPY")?, order("QQQ", Vec::new())?]),
                BrokerOutcome::OpenOrders(vec![bracket(ASSET)?, order("QQQ", Vec::new())?]),
            ),
            (
                BrokerOutcome::Positions(vec![position("SPY")?, position("QQQ")?]),
                BrokerOutcome::Positions(vec![position(ASSET)?, position("QQQ")?]),
            ),
            (
                BrokerOutcome::Activities {
                    fills: vec![fill("SPY")?, fill("QQQ")?],
                    cursor: cursor.clone(),
                },
                BrokerOutcome::Activities {
                    fills: vec![fill(ASSET)?, fill("QQQ")?],
                    cursor,
                },
            ),
            (
                BrokerOutcome::Listed {
                    client_order_id: "md-1".to_owned(),
                    orders: vec![listed(bracket("SPY")?)],
                },
                BrokerOutcome::Listed {
                    client_order_id: "md-1".to_owned(),
                    orders: vec![listed(bracket(ASSET)?)],
                },
            ),
        ];
        for (answered, read) in cases {
            assert_eq!(symbols.outcome(answered.clone()), read, "{answered:?}");
            assert_eq!(
                BrokerSymbols::default().outcome(answered.clone()),
                answered,
                "nothing registered maps nothing"
            );
        }
        Ok(())
    }

    /// An answer naming no instrument passes as the broker gave it.
    #[test]
    fn an_answer_naming_no_instrument_is_unchanged() -> Result<(), String> {
        for answered in [
            BrokerOutcome::DuplicateClientOrderId {
                client_order_id: "md-1".to_owned(),
            },
            BrokerOutcome::Rejected(BrokerReject {
                client_order_id: None,
                http_status: 403,
                code: None,
                message: "refused".to_owned(),
            }),
            BrokerOutcome::Absent {
                client_order_id: "md-1".to_owned(),
            },
            BrokerOutcome::CancelAccepted {
                client_order_id: "md-1".to_owned(),
            },
            BrokerOutcome::AccountWideAccepted,
        ] {
            assert_eq!(spy()?.outcome(answered.clone()), answered);
        }
        Ok(())
    }

    /// A ticker registered as its own asset id maps nothing.
    #[test]
    fn a_ticker_spelled_as_its_asset_id_registers_nothing() -> Result<(), String> {
        let same = BrokerSymbols::registered([(id(ASSET)?, id(ASSET)?)]);
        assert_eq!(same, BrokerSymbols::default());
        assert_ne!(spy()?, BrokerSymbols::default());
        Ok(())
    }
}
