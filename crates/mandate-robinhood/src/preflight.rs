//! The preflight facts and the account snapshot (the first live trade brief's C4; DEC-470 item 1,
//! DEC-875 item 6, DEC-902): what the runner's preflight reads of the agentic account, mapped from
//! the answers of `get_portfolio`, `get_equity_positions`, `get_equity_orders`,
//! `get_equity_tradability` and `get_equity_quotes`. Every account read goes through
//! [`RobinhoodConnector::read`], so each one is scoped to the agentic account and checks it
//! afresh. A record of an assumed shape (DEC-875 item 4, DEC-902 items 1 to 7) with a field
//! missing, a field it does not list, a value of another type, or a number that is not canonical
//! decimal text refuses the whole read (`AGENTS.md` rule 3). Nothing here is defaulted.

use mandate_accounting::{InstrumentId, Side};
use mandate_executor::{BrokerOrder, BrokerPosition};
use mandate_mcp::CallClass;
use mandate_num::{Price, Qty, SignedQty, Usd};
use serde_json::{Map, Value};

use crate::scope::{AccountRead, structured};
use crate::{RobinhoodConnector, RobinhoodError, Tools};

const QUOTES: &str = "get_equity_quotes";

/// What DEC-470 item 1 checks of the agentic account, as the broker answered it: its cash, its
/// buying power, every position record (a zero quantity included), and every working order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountSnapshot {
    /// `get_portfolio`'s `cash`.
    pub cash: Usd,
    /// `get_portfolio`'s `buying_power`.
    pub buying_power: Usd,
    /// Every record of `get_equity_positions`, in the order the broker listed them.
    pub positions: Vec<BrokerPosition>,
    /// The records of `get_equity_orders` whose `state` is working (`new`, `queued`,
    /// `confirmed`, `unconfirmed`, `partially_filled`), in the order the broker listed them.
    pub open_orders: Vec<BrokerOrder>,
}

/// `get_equity_quotes`'s one record for the symbol (DEC-902 item 7): an uncrossed quote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Quote {
    pub symbol: InstrumentId,
    pub bid: Price,
    pub ask: Price,
}

/// The account snapshot, and the quote of the one instrument the agentic account may trade.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreflightFacts {
    pub account: AccountSnapshot,
    pub quote: Quote,
}

impl<T: Tools> RobinhoodConnector<T> {
    /// The agentic account's snapshot: `get_portfolio`, `get_equity_positions` and
    /// `get_equity_orders`, each through [`Self::read_sole_member`] with no filter but the
    /// recorded account, on the `Ordinary` budget. A refusal of any read is the snapshot's
    /// refusal, unchanged; a record that is not exactly its assumed shape is
    /// [`RobinhoodError::Unreadable`] with the read's code (DEC-902). No order, cancel or exit
    /// waits on it (`AGENTS.md` rule 13).
    pub async fn account_snapshot(&self) -> Result<AccountSnapshot, RobinhoodError> {
        let empty = Map::new();
        let portfolio = self
            .read_sole_member(AccountRead::Portfolio, &empty)
            .await?;
        let (cash, buying_power) = portfolio_record(&the_one(&portfolio, "portfolio")?)
            .ok_or(RobinhoodError::Unreadable { code: "portfolio" })?;
        let positions = self
            .read_sole_member(AccountRead::Positions, &empty)
            .await?;
        let positions = mapped(&positions, position_record, "positions")?;
        let orders = self.read_sole_member(AccountRead::Orders, &empty).await?;
        let mut open_orders = Vec::new();
        for record in &orders {
            let order =
                order_record(record).ok_or(RobinhoodError::Unreadable { code: "orders" })?;
            if matches!(order.status.as_str(), "accepted" | "partially_filled") {
                open_orders.push(order);
            }
        }
        Ok(AccountSnapshot {
            cash,
            buying_power,
            positions,
            open_orders,
        })
    }

    /// [`Self::account_snapshot`], then `get_equity_tradability` for `symbol` through
    /// [`Self::read_sole_member`], and `get_equity_quotes` for `symbol` alone, which names no
    /// account. A symbol the account may not trade is [`RobinhoodError::NotTradable`]; a quote
    /// that is not exactly one record for `symbol`, or is crossed, is
    /// [`RobinhoodError::Unreadable`] (DEC-902 items 6 and 7).
    pub async fn preflight_facts(
        &self,
        symbol: &InstrumentId,
    ) -> Result<PreflightFacts, RobinhoodError> {
        let account = self.account_snapshot().await?;
        let mut filters = Map::new();
        filters.insert(
            "symbol".to_owned(),
            Value::String(symbol.as_str().to_owned()),
        );
        let records = self
            .read_sole_member(AccountRead::Tradability, &filters)
            .await?;
        let tradable = tradability_record(&the_one(&records, "tradability")?, symbol).ok_or(
            RobinhoodError::Unreadable {
                code: "tradability",
            },
        )?;
        if !tradable {
            return Err(RobinhoodError::NotTradable);
        }
        let quote = self.quote(symbol).await?;
        Ok(PreflightFacts { account, quote })
    }

    /// `get_equity_quotes` for the one symbol (DEC-902 item 7): no account read, because the
    /// contract names no account for it and its answer is about none, so the account check does
    /// not run before it. Called on the `Ordinary` budget with exactly `{"symbols": [symbol]}`,
    /// and answered by one uncrossed record for the symbol; the answer's `structuredContent`
    /// holds its `quotes` member and nothing else.
    async fn quote(&self, symbol: &InstrumentId) -> Result<Quote, RobinhoodError> {
        let mut arguments = Map::new();
        arguments.insert(
            "symbols".to_owned(),
            Value::Array(vec![Value::String(symbol.as_str().to_owned())]),
        );
        let answer = self
            .tools
            .call_tool(CallClass::Ordinary, QUOTES, &Value::Object(arguments))
            .await;
        let unreadable = || RobinhoodError::Unreadable { code: "quotes" };
        let content = structured(answer).ok_or_else(unreadable)?;
        if content.len() != 1 {
            return Err(unreadable());
        }
        let quoted = match content.get("quotes") {
            Some(Value::Array(quoted)) => quoted,
            _ => return Err(unreadable()),
        };
        let [record] = quoted.as_slice() else {
            return Err(unreadable());
        };
        let Value::Object(record) = record else {
            return Err(unreadable());
        };
        quote_record(record, symbol).ok_or_else(unreadable)
    }
}

/// The read's one record, or its refusal when it answered none (its only record named another
/// account, which the scoped read drops): the figure the preflight maps is missing, so the
/// read is refused rather than defaulted (DEC-902 item 1, DEC-875 item 3).
fn the_one(
    records: &[Map<String, Value>],
    code: &'static str,
) -> Result<Map<String, Value>, RobinhoodError> {
    let [record] = records else {
        return Err(RobinhoodError::Unreadable { code });
    };
    Ok(record.clone())
}

/// Every record read by `reader`, or the read's refusal with `code` as soon as one of them is
/// not exactly its assumed shape.
fn mapped<T>(
    records: &[Map<String, Value>],
    reader: fn(&Map<String, Value>) -> Option<T>,
    code: &'static str,
) -> Result<Vec<T>, RobinhoodError> {
    let mut read = Vec::new();
    for record in records {
        read.push(reader(record).ok_or(RobinhoodError::Unreadable { code })?);
    }
    Ok(read)
}

/// A record's text at `field`; any other value is `None`, so the read refuses.
fn text<'a>(record: &'a Map<String, Value>, field: &str) -> Option<&'a str> {
    record.get(field).and_then(Value::as_str)
}

/// Whether the record holds exactly the fields its assumed shape lists: a field missing, or one
/// the shape does not list, makes the whole read unreadable (DEC-902 item 1).
fn listed(record: &Map<String, Value>, fields: &[&str]) -> bool {
    record.len() == fields.len() && fields.iter().all(|field| record.contains_key(*field))
}

/// The portfolio record (DEC-902 item 2): the account, its cash and its buying power, each
/// exact non-negative decimal text. No equity, margin or multiplier is read, so none is
/// invented.
fn portfolio_record(record: &Map<String, Value>) -> Option<(Usd, Usd)> {
    if !listed(record, &["account_number", "cash", "buying_power"]) {
        return None;
    }
    let cash = Usd::parse(text(record, "cash")?).ok()?;
    let buying_power = Usd::parse(text(record, "buying_power")?).ok()?;
    (cash >= Usd::ZERO && buying_power >= Usd::ZERO).then_some((cash, buying_power))
}

/// One position record (DEC-902 item 3): the account, the instrument, its signed quantity and
/// its average buy price, a positive price. A zero row is read like any other; none is merged
/// or dropped, so DEC-470 item 1 sees it.
fn position_record(record: &Map<String, Value>) -> Option<BrokerPosition> {
    if !listed(
        record,
        &["account_number", "symbol", "quantity", "average_buy_price"],
    ) {
        return None;
    }
    let instrument = InstrumentId::new(text(record, "symbol")?).ok()?;
    let qty = SignedQty::parse(text(record, "quantity")?).ok()?;
    let avg_entry_price = Price::parse(text(record, "average_buy_price")?).ok()?;
    Some(BrokerPosition {
        instrument,
        qty,
        avg_entry_price,
    })
}

/// The tradability record (DEC-902 item 6): the account, the symbol, byte-equal to the
/// instrument's, and the boolean. A `false` is the caller's [`RobinhoodError::NotTradable`].
fn tradability_record(record: &Map<String, Value>, symbol: &InstrumentId) -> Option<bool> {
    if !listed(record, &["account_number", "symbol", "tradable"]) {
        return None;
    }
    if text(record, "symbol")? != symbol.as_str() {
        return None;
    }
    record.get("tradable")?.as_bool()
}

/// The one quote record (DEC-902 item 7): the symbol, byte-equal to the instrument's, and its
/// bid and ask, uncrossed, so a locked quote is allowed. No time field is assumed, so none is
/// read.
fn quote_record(record: &Map<String, Value>, symbol: &InstrumentId) -> Option<Quote> {
    if !listed(record, &["symbol", "bid_price", "ask_price"]) {
        return None;
    }
    if text(record, "symbol")? != symbol.as_str() {
        return None;
    }
    let bid = Price::parse(text(record, "bid_price")?).ok()?;
    let ask = Price::parse(text(record, "ask_price")?).ok()?;
    (bid <= ask).then_some(Quote {
        symbol: symbol.clone(),
        bid,
        ask,
    })
}

/// One order record (DEC-902 item 4): exactly the keys `mandate-rh-sim` serves, with at most a
/// text `ref_id`; the side, type, time in force, market hours and state are the contract's
/// texts; the quantity, the filled quantity and each present price are decimal text, and a
/// missing one, unlike a JSON `null`, refuses the read; the broker's `id` is non-empty text.
/// Our `client_order_id` never appears in it (U-R2), so the executor never assumes the record
/// is ours. Anything the contract does not list refuses, rather than leaving an order
/// uncounted.
fn order_record(record: &Map<String, Value>) -> Option<BrokerOrder> {
    if !order_keys(record) {
        return None;
    }
    let id = text(record, "id")?;
    if id.is_empty() {
        return None;
    }
    let instrument = InstrumentId::new(text(record, "symbol")?).ok()?;
    let side = match text(record, "side")? {
        "buy" => Side::Buy,
        "sell" => Side::Sell,
        _ => return None,
    };
    match text(record, "type")? {
        "market" | "limit" | "stop_market" | "stop_limit" => {}
        _ => return None,
    }
    match text(record, "time_in_force")? {
        "gfd" | "gtc" => {}
        _ => return None,
    }
    match text(record, "market_hours")? {
        "regular_hours" | "extended_hours" | "all_day_hours" => {}
        _ => return None,
    }
    let qty = Qty::parse(text(record, "quantity")?).ok()?;
    let filled_qty = Qty::parse(text(record, "filled_quantity")?).ok()?;
    let limit_price = price_field(record, "limit_price")?;
    let stop_price = price_field(record, "stop_price")?;
    let status = crate::executor_status(text(record, "state")?)
        .ok()?
        .to_owned();
    Some(BrokerOrder {
        broker_order_id: id.to_owned(),
        client_order_id: None,
        instrument,
        side,
        qty,
        filled_qty,
        limit_price,
        stop_price,
        status,
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    })
}

/// Whether the order record holds every field its shape lists, no other, and at most the
/// optional text `ref_id` (DEC-902 item 4).
fn order_keys(record: &Map<String, Value>) -> bool {
    let required = [
        "id",
        "account_number",
        "symbol",
        "side",
        "type",
        "quantity",
        "limit_price",
        "stop_price",
        "time_in_force",
        "market_hours",
        "state",
        "filled_quantity",
    ];
    let listed = record.keys().filter(|key| key.as_str() != "ref_id").count();
    listed == required.len()
        && required.iter().all(|field| record.contains_key(*field))
        && record.get("ref_id").is_none_or(|value| value.is_string())
}

/// A price field: decimal text, or `None` for a JSON `null`. A missing key, a JSON number, or
/// any other value refuses the read; nothing is defaulted (DEC-902 item 1).
fn price_field(record: &Map<String, Value>, field: &'static str) -> Option<Option<Price>> {
    match record.get(field)? {
        Value::Null => Some(None),
        Value::String(price) => Price::parse(price).ok().map(Some),
        _ => None,
    }
}
