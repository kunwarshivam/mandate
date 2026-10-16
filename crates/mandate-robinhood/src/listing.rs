//! The shared fallback's listing, mapped to Robinhood (the first live trade brief's C3; DEC-529
//! item 4, connections spec §6.2, [DEC-941](../../../docs/project/decisions/DEC-941.md)): a
//! `ListOrders` request is one `get_equity_orders` read of the agentic account with `symbol`,
//! `placed_agent` `agentic` and `created_at_gte`, and every record it answers is handed to the
//! executor, which adopts exactly one match or leaves the order `Unknown`. The connector matches
//! nothing itself and never places.

use mandate_executor::{
    BrokerOutcome, BrokerUnknown, ClientOrderId, ConnectorError, ListedOrder, OrderListing,
    OrderType, TimeInForce,
};
use mandate_time::UtcNanos;
use serde_json::{Map, Value};

use crate::preflight::order_record;
use crate::scope::AccountRead;
use crate::{RobinhoodConnector, Tools, ref_id};

/// A listing that could not be read: what became of the order is still unknown, so it stays
/// `Unknown` and blocks its instrument (trading spec §5.3 rule 9). Never a rejection, never an
/// absence.
const UNREAD: ConnectorError = ConnectorError::Unknown(BrokerUnknown::Ambiguous);

impl<T: Tools> RobinhoodConnector<T> {
    /// One read of the agentic account's orders for `listing` (DEC-941 items 1 and 3), through
    /// [`Self::read_sole_member`]: an answer holding anything beside `orders`, a next-page cursor
    /// included, is refused, because a first page alone could hide a second match. A refused or
    /// unreadable read, or one record that is not exactly an order record of a type and time in
    /// force the executor models, is [`UNREAD`], so the order stays `Unknown`.
    pub(crate) async fn list(
        &self,
        listing: &OrderListing,
    ) -> Result<BrokerOutcome, ConnectorError> {
        let since = UtcNanos::from_parts(listing.created_since.secs(), 0).map_err(|_| UNREAD)?;
        let text = |value: String| Value::String(value);
        let mut filters = Map::new();
        filters.insert(
            "symbol".to_owned(),
            text(listing.instrument.as_str().to_owned()),
        );
        filters.insert("placed_agent".to_owned(), text("agentic".to_owned()));
        filters.insert("created_at_gte".to_owned(), text(since.to_string()));
        let records = self
            .read_sole_member(AccountRead::Orders, &filters)
            .await
            .map_err(|_| UNREAD)?;
        let ours = ref_id(&listing.client_order_id).map_err(|_| UNREAD)?;
        let mut orders = Vec::with_capacity(records.len());
        for record in &records {
            orders.push(listed(record, &listing.client_order_id, &ours).ok_or(UNREAD)?);
        }
        Ok(BrokerOutcome::Listed {
            client_order_id: listing.client_order_id.as_str().to_owned(),
            orders,
        })
    }
}

/// One `orders[]` record as the executor matches it (DEC-941 item 4). A record carrying our
/// `ref_id` names our key; one carrying another `ref_id` names that text, which is never a key
/// of ours, so it is never adopted; one carrying none names nothing, and the executor's
/// look-alike rule decides (DEC-863).
fn listed(record: &Map<String, Value>, key: &ClientOrderId, ours: &str) -> Option<ListedOrder> {
    let mut order = order_record(record)?;
    let order_type = match record.get("type").and_then(Value::as_str)? {
        "limit" => OrderType::Limit,
        "market" => OrderType::Market,
        "stop_limit" => OrderType::StopLimit,
        _ => return None,
    };
    let tif = match record.get("time_in_force").and_then(Value::as_str)? {
        "gfd" => TimeInForce::Day,
        "gtc" => TimeInForce::Gtc,
        _ => return None,
    };
    order.client_order_id = match record.get("ref_id").and_then(Value::as_str) {
        Some(theirs) if theirs == ours => Some(key.as_str().to_owned()),
        Some(theirs) => Some(theirs.to_owned()),
        None => None,
    };
    Some(ListedOrder {
        order,
        order_type,
        tif,
    })
}
