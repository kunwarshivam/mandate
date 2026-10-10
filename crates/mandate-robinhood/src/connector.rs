//! The connector over the tool seam (connections spec §6.2; DEC-860 items 4, 6 and 7).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::Value as Canon;
use mandate_domain::{
    AssetClass, CapabilityProfile, MarketSession, OrderType as CellType, ProfileError,
    ProtectionForm, QuantityForm,
};
use mandate_executor::{
    BrokerConnector, BrokerOrder, BrokerOutcome, BrokerReject, BrokerRequest, BrokerUnknown,
    ClientOrderId, ConnectorError, FoldedEvent, OrderType, SubmitOrder, TimeInForce,
};
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Fraction, Price, Qty, ShareIncrement};
use serde_json::{Map, Value};

const REVIEW: &str = "review_equity_order";
const PLACE: &str = "place_equity_order";
const CANCEL: &str = "cancel_equity_order";

/// A place or a cancel whose answer is lost or cannot be read: what became of it is unknown, and
/// recovery must find out (LT-6). Never a rejection.
const IN_DOUBT: ConnectorError = ConnectorError::Unknown(BrokerUnknown::Ambiguous);

/// An order the profile does not offer, refused before any call (DEC-860 item 7).
const NOT_OFFERED: ConnectorError = ConnectorError::NotSent {
    code: "not_offered",
};

/// One MCP `tools/call` (DEC-860 item 2), answered with the JSON-RPC `result` as sent. An `Err`
/// says nothing about whether the call reached the server.
pub trait Tools {
    fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> impl Future<Output = Result<String, McpError>>;
}

/// The connector for one agentic account, whose number the founder typed (CN-8). It has no
/// `Debug`: the account number is personal data (journal spec §6.4).
pub struct RobinhoodConnector<T> {
    pub(crate) tools: T,
    pub(crate) account_number: String,
    /// The order each place answered, by our key: its broker `order_id` is what a `Cancel` goes
    /// by, and its instrument and side are what a cancel's answer is read against. After a restart
    /// it is rebuilt from the journal by [`Self::restore`] (DEC-860 item 7).
    placed: BTreeMap<ClientOrderId, BrokerOrder>,
    /// The keys whose place went out without a readable answer. One is never placed again, and a
    /// second `Submit` of it is `Unknown` with nothing called (DEC-860 item 4).
    in_doubt: BTreeSet<ClientOrderId>,
}

impl<T: Tools> RobinhoodConnector<T> {
    /// Every request names `account_number`, and only it.
    pub fn new(tools: T, account_number: String) -> Self {
        Self {
            tools,
            account_number,
            placed: BTreeMap::new(),
            in_doubt: BTreeSet::new(),
        }
    }

    /// The connector after a restart: [`Self::new`] with its `ClientOrderId` → `order_id` map
    /// rebuilt from the account stream's `records`, in journal order (journal spec §9.16,
    /// DEC-860 item 7, DEC-870, DEC-872). A key maps to the one distinct `broker_order_id` its
    /// version-2 `OrderStateChanged` records carry, when no other key carries that id, with the
    /// instrument and side of its own readable `OrderSubmitted`, or else of the order it
    /// `replaces`. A key with no id, two different ids, an id another key carries, or no readable
    /// instrument and side (two of its `OrderSubmitted` that disagree included) maps to nothing,
    /// and so does a key whose records name two or more different `replaces` targets, whatever its
    /// own `OrderSubmitted` says, with every successor that reaches its instrument and side only
    /// through it; one target named twice is one link (DEC-876). Such a key's cancel is `NotSent`
    /// (`no_order_id`) with nothing called. Every key the stream names
    /// (an `OrderSubmitted` or `OrderStateChanged` key, or a `replaces` or `replaced_by` target)
    /// is never placed again: a `Submit` of it is `Unknown` (DEC-860 item 4, DEC-872).
    pub fn restore(
        tools: T,
        account_number: String,
        records: &[FoldedEvent],
    ) -> Result<Self, crate::RobinhoodError> {
        let mut connector = Self::new(tools, account_number);
        let mut submitted: BTreeMap<ClientOrderId, Option<(InstrumentId, Side)>> = BTreeMap::new();
        let mut ids: BTreeMap<ClientOrderId, BTreeSet<String>> = BTreeMap::new();
        let mut named: BTreeSet<ClientOrderId> = BTreeSet::new();
        let mut parents: BTreeMap<ClientOrderId, BTreeSet<ClientOrderId>> = BTreeMap::new();
        let mut odd: BTreeSet<ClientOrderId> = BTreeSet::new();
        for record in records {
            let text = |member: &str| record.payload.get(member).and_then(Canon::as_str);
            let Some(key) = text("client_order_id").and_then(|k| ClientOrderId::parse(k).ok())
            else {
                continue;
            };
            let link = |member: &str| text(member).and_then(|k| ClientOrderId::parse(k).ok());
            match record.event_type.as_str() {
                "OrderSubmitted" => {
                    named.insert(key.clone());
                    let side = match text("side") {
                        Some("buy") => Some(Side::Buy),
                        Some("sell") => Some(Side::Sell),
                        _ => None,
                    };
                    let instrument = text("instrument_id").and_then(|i| InstrumentId::new(i).ok());
                    let read = instrument.zip(side);
                    let own = submitted.entry(key).or_insert_with(|| read.clone());
                    if *own != read {
                        *own = None;
                    }
                }
                "OrderStateChanged" => {
                    named.insert(key.clone());
                    named.extend(link("replaced_by"));
                    let old = link("replaces");
                    named.extend(old.clone());
                    if let Some(old) = old {
                        parents.entry(key.clone()).or_default().insert(old);
                    }
                    let known = ids.entry(key.clone()).or_default();
                    match record.payload.get("broker_order_id") {
                        None | Some(Canon::Null) => {}
                        Some(Canon::Str(id)) if !id.is_empty() => {
                            known.insert(id.clone());
                        }
                        Some(_) => {
                            odd.insert(key);
                        }
                    }
                }
                _ => {}
            }
        }
        connector.in_doubt = named;
        for (key, origins) in &parents {
            if origins.len() > 1 {
                submitted.insert(key.clone(), None);
            }
        }
        for start in ids.keys() {
            let (mut at, mut path) = (start.clone(), BTreeSet::new());
            let found = loop {
                if let Some(read) = submitted.get(&at) {
                    break read.clone();
                }
                match parents.get(&at).and_then(BTreeSet::first) {
                    Some(parent) if path.insert(at.clone()) => at = parent.clone(),
                    _ => break None,
                }
            };
            for key in path {
                submitted.insert(key, found.clone());
            }
        }
        let (mut seen, mut shared) = (BTreeSet::new(), BTreeSet::new());
        for id in ids.values().flatten() {
            if !seen.insert(id) {
                shared.insert(id.clone());
            }
        }
        for (key, known) in ids {
            let mut only = known.iter();
            let (Some(id), None) = (only.next(), only.next()) else {
                continue;
            };
            if shared.contains(id) || odd.contains(&key) {
                continue;
            }
            let Some(Some((instrument, side))) = submitted.get(&key) else {
                continue;
            };
            let order = BrokerOrder {
                broker_order_id: id.clone(),
                client_order_id: Some(key.as_str().to_owned()),
                instrument: instrument.clone(),
                side: *side,
                qty: Qty::ZERO,
                filled_qty: Qty::ZERO,
                limit_price: None,
                stop_price: None,
                status: String::new(),
                reject_code: None,
                replaced_by_broker_order_id: None,
                legs: Vec::new(),
                created_on: None,
            };
            connector.in_doubt.remove(&key);
            connector.placed.insert(key, order);
        }
        Ok(connector)
    }

    /// Review, then place with the [`crate::ref_id`]. An opening or an increase draws on the
    /// `Ordinary` budget and is refused by a pre-trade alert before the place; every other
    /// purpose draws on `RiskReducing` and is placed whatever the review says, so the broker
    /// accepts or rejects it (`AGENTS.md` rule 13, DEC-860 item 6).
    async fn submit(&mut self, order: &SubmitOrder) -> Result<BrokerOutcome, ConnectorError> {
        let mut arguments = self.arguments(order).ok_or(NOT_OFFERED)?;
        let key = &order.client_order_id;
        if self.in_doubt.contains(key) || self.placed.contains_key(key) {
            return Err(IN_DOUBT);
        }
        let opening = order.purpose.adds_risk();
        let class = if opening {
            CallClass::Ordinary
        } else {
            CallClass::RiskReducing
        };
        let review = Value::Object(arguments.clone());
        let review = self.tools.call_tool(class, REVIEW, &review).await;
        if opening && let Some(refusal) = review_refusal(key, review)? {
            return Ok(BrokerOutcome::Rejected(refusal));
        }
        let ref_id = crate::ref_id(key).map_err(|_| ConnectorError::NotSent { code: "ref_id" })?;
        arguments.insert("ref_id".to_owned(), Value::String(ref_id));
        self.in_doubt.insert(key.clone());
        let answer = self
            .tools
            .call_tool(class, PLACE, &Value::Object(arguments))
            .await;
        let placed = placed(order, answer).ok_or(IN_DOUBT)?;
        self.in_doubt.remove(key);
        self.placed.insert(key.clone(), placed.clone());
        Ok(BrokerOutcome::Submitted(placed))
    }

    /// `cancel_equity_order` by the broker's `order_id`, on the `RiskReducing` budget. With no
    /// `order_id` recorded the cancel is `NotSent` (`no_order_id`) and nothing is guessed (CN-7).
    ///
    /// The answer is the order record. Only one whose `state` reads as cancelled (§6.2) is
    /// [`BrokerOutcome::CancelAccepted`], the broker's confirmation; any other state is that
    /// order, as [`BrokerOutcome::Order`], so the executor keeps the cancel unconfirmed and waits
    /// for the broker's own cancelled state (DEC-867 item 2; trading spec §5.7). A record that
    /// cannot be read is `Unreadable` (DEC-864).
    async fn cancel(&mut self, key: &ClientOrderId) -> Result<BrokerOutcome, ConnectorError> {
        let placed = self
            .placed
            .get(key)
            .ok_or(ConnectorError::NotSent {
                code: "no_order_id",
            })?
            .clone();
        let order_id = placed.broker_order_id.clone();
        let mut arguments = Map::new();
        arguments.insert(
            "account_number".to_owned(),
            Value::String(self.account_number.clone()),
        );
        arguments.insert("order_id".to_owned(), Value::String(order_id));
        let answer = self
            .tools
            .call_tool(CallClass::RiskReducing, CANCEL, &Value::Object(arguments))
            .await
            .map_err(|_| IN_DOUBT)?;
        let client_order_id = key.as_str().to_owned();
        match tool_result(&answer).ok_or(ConnectorError::Unreadable { code: "cancel" })? {
            ToolResult::Content(record) => {
                let unreadable = ConnectorError::Unreadable { code: "cancel" };
                let order = order_record(&record, &placed).ok_or(unreadable)?;
                if order.status == "canceled" {
                    Ok(BrokerOutcome::CancelAccepted { client_order_id })
                } else {
                    Ok(BrokerOutcome::Order(order))
                }
            }
            ToolResult::Refused => Ok(BrokerOutcome::Rejected(BrokerReject {
                client_order_id: Some(client_order_id),
                http_status: 0,
                code: Some("cancel_refused".to_owned()),
                message: "the broker refused the cancel".to_owned(),
            })),
        }
    }

    /// The review's and the place's arguments, or `None` for an order the profile does not offer:
    /// an order type and quantity form without a cell, a bracket or an OCO no cell lists,
    /// extended hours (no row), or a time in force the contract cannot spell (`ioc`).
    fn arguments(&self, order: &SubmitOrder) -> Option<Map<String, Value>> {
        let profile = crate::profile::robinhood().ok()?;
        if order.extended_hours || !offered(&profile, order) {
            return None;
        }
        let time_in_force = match order.tif {
            TimeInForce::Day => "gfd",
            TimeInForce::Gtc => "gtc",
            TimeInForce::Ioc => return None,
        };
        let (side, order_type) = wire_names(order);
        let text = |value: &str| Value::String(value.to_owned());
        let mut arguments = Map::new();
        arguments.insert("account_number".to_owned(), text(&self.account_number));
        arguments.insert("symbol".to_owned(), text(order.instrument.as_str()));
        arguments.insert("side".to_owned(), text(side));
        arguments.insert("type".to_owned(), text(order_type));
        arguments.insert("quantity".to_owned(), text(&order.qty.to_string()));
        arguments.insert("time_in_force".to_owned(), text(time_in_force));
        arguments.insert("market_hours".to_owned(), text("regular_hours"));
        if let Some(limit) = order.limit_price {
            arguments.insert("limit_price".to_owned(), text(&limit.to_string()));
        }
        if let Some(stop) = order.stop_price {
            arguments.insert("stop_price".to_owned(), text(&stop.to_string()));
        }
        Some(arguments)
    }
}

impl<T: Tools> BrokerConnector for RobinhoodConnector<T> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        crate::profile::robinhood()
    }

    /// `Submit` and `Cancel` (DEC-860); anything the profile does not offer is `NotSent`.
    async fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        match request {
            BrokerRequest::Submit(order) => self.submit(order).await,
            BrokerRequest::Cancel { client_order_id } => self.cancel(client_order_id).await,
            BrokerRequest::AcknowledgeReplace { .. }
            | BrokerRequest::GetOrderByClientId(_)
            | BrokerRequest::ListOrders(_)
            | BrokerRequest::ListOpenOrders
            | BrokerRequest::ListPositions
            | BrokerRequest::GetAccount
            | BrokerRequest::ListActivities { .. }
            | BrokerRequest::CancelAll(_)
            | BrokerRequest::ClosePosition(..) => Err(NOT_OFFERED),
        }
    }
}

/// Whether the profile's US equity regular-session row has a cell for the order's type and
/// quantity form, and that cell lists each protective form the order is sent as.
fn offered(profile: &CapabilityProfile, order: &SubmitOrder) -> bool {
    let order_type = match order.order_type {
        OrderType::Limit => CellType::Limit,
        OrderType::Market => CellType::Market,
        OrderType::StopLimit => CellType::StopLimit,
    };
    let whole = order
        .qty
        .portion(Fraction::ONE, ShareIncrement::Whole)
        .is_ok_and(|whole| whole == order.qty);
    let quantity_form = if whole {
        QuantityForm::Whole
    } else {
        QuantityForm::Fractional
    };
    let (equity, regular) = (AssetClass::UsEquity, MarketSession::Regular);
    let Ok(cell) = profile.cell(equity, regular, order_type, quantity_form) else {
        return false;
    };
    let sent_as = [
        (order.bracket.is_some(), ProtectionForm::Bracket),
        (order.oco.is_some(), ProtectionForm::Oco),
    ];
    sent_as
        .iter()
        .all(|(sent, form)| !sent || cell.protection_forms.contains(form))
}

/// The contract's spelling of the order's side and type.
fn wire_names(order: &SubmitOrder) -> (&'static str, &'static str) {
    let side = match order.side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    };
    let order_type = match order.order_type {
        OrderType::Limit => "limit",
        OrderType::Market => "market",
        OrderType::StopLimit => "stop_limit",
    };
    (side, order_type)
}

/// A tool result: its `structuredContent` when it carries no `isError` or `isError: false`, or
/// a refusal when `isError` is `true`. `None` for anything else.
pub(crate) enum ToolResult {
    Content(Map<String, Value>),
    Refused,
}

pub(crate) fn tool_result(text: &str) -> Option<ToolResult> {
    let Value::Object(mut result) = serde_json::from_str(text).ok()? else {
        return None;
    };
    match result.get("isError") {
        None | Some(Value::Bool(false)) => match result.remove("structuredContent") {
            Some(Value::Object(content)) => Some(ToolResult::Content(content)),
            _ => None,
        },
        Some(Value::Bool(true)) => Some(ToolResult::Refused),
        Some(_) => None,
    }
}

/// The refusal of an opening's review: an `isError` result or any pre-trade alert. A review that
/// is lost or cannot be read is `Unreadable`, and nothing is placed. The broker's text is never
/// carried (connections spec §6.2 rule 4).
fn review_refusal(
    key: &ClientOrderId,
    review: Result<String, McpError>,
) -> Result<Option<BrokerReject>, ConnectorError> {
    let unreadable = ConnectorError::Unreadable { code: "review" };
    let result = review.ok().and_then(|text| tool_result(&text));
    let clear = match result.ok_or(unreadable)? {
        ToolResult::Refused => false,
        ToolResult::Content(content) => content
            .get("alerts")
            .and_then(Value::as_array)
            .ok_or(unreadable)?
            .is_empty(),
    };
    Ok((!clear).then(|| BrokerReject {
        client_order_id: Some(key.as_str().to_owned()),
        http_status: 0,
        code: Some("pre_trade_alert".to_owned()),
        message: "refused at review, before the place".to_owned(),
    }))
}

/// The order a place answered, or `None` unless the answer is exactly an order record: no
/// `isError`, an `id`, a `state` of the contract's ten, and decimal-text numbers (DEC-860 item 4;
/// connections spec §6.2 rule 5). Side and instrument are the request's.
fn placed(order: &SubmitOrder, answer: Result<String, McpError>) -> Option<BrokerOrder> {
    let ToolResult::Content(record) = tool_result(&answer.ok()?)? else {
        return None;
    };
    let requested = BrokerOrder {
        broker_order_id: String::new(),
        client_order_id: Some(order.client_order_id.as_str().to_owned()),
        instrument: order.instrument.clone(),
        side: order.side,
        qty: order.qty,
        filled_qty: Qty::ZERO,
        limit_price: order.limit_price,
        stop_price: order.stop_price,
        status: String::new(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    };
    order_record(&record, &requested)
}

/// An order record read as an order: no `isError`, an `id`, a `state` of the contract's ten, and
/// decimal-text numbers (connections spec §6.2 rule 5). Our key, the instrument and the side are
/// `known`'s, which the record is about; `None` for anything else.
fn order_record(record: &Map<String, Value>, known: &BrokerOrder) -> Option<BrokerOrder> {
    let text = |field: &str| record.get(field).and_then(Value::as_str);
    let qty = |field: &str| Qty::parse(text(field)?).ok();
    let price = |field: &str| match record.get(field) {
        None | Some(Value::Null) => Some(None),
        Some(Value::String(price)) => Price::parse(price).ok().map(Some),
        Some(_) => None,
    };
    Some(BrokerOrder {
        broker_order_id: text("id")?.to_owned(),
        client_order_id: known.client_order_id.clone(),
        instrument: known.instrument.clone(),
        side: known.side,
        qty: qty("quantity")?,
        filled_qty: qty("filled_quantity")?,
        limit_price: price("limit_price")?,
        stop_price: price("stop_price")?,
        status: crate::executor_status(text("state")?).ok()?.to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    })
}
