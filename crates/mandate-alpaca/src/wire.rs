//! Alpaca's trading wire types and the parse into exact values.
//!
//! Every quantity, price, and amount arrives as **raw text** and is parsed by `mandate-num`
//! (ADR-0001 ES-23). A field the broker sends in exponent form, with more places than the
//! instrument's increment, or as a JSON number — which has already been through a float — is a
//! typed [`WireError`] with a stable code, never a rounded value.
//!
//! The wire types name every field this crate records. A field the broker adds later is not in a
//! wire type, so it is never recorded and cannot slip past the redaction pass
//! (task brief interpretation 24). A field the wire type names but this stream does not yet
//! interpret answers [`WireError::NotInterpreted`] with its owning story (DEC-85).

use mandate_accounting::{InstrumentId, Side};
use mandate_executor::{
    BrokerAccount, BrokerFill, BrokerOrder, BrokerPosition, BrokerReject, FillId, OrderState,
    OrderType, StatusMapping, SubmitOrder, TimeInForce,
};
use mandate_num::{Price, Qty, SignedQty, Usd};
use mandate_time::{Date, UtcNanos, new_york_date_and_hour};
use serde_json::{Map, Value};

use crate::error::WireError;

/// Alpaca's order object, as `/v2/orders` and `/v2/orders:by_client_order_id` return it.
///
/// There is no `account_number` or account `id` on this type because the order endpoints do not
/// return one; the account endpoint does, and [`WireAccount`] is where that is handled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireOrder {
    pub id: String,
    pub client_order_id: String,
    pub symbol: String,
    pub asset_class: String,
    pub side: String,
    pub order_type: String,
    pub time_in_force: String,
    pub qty: Option<String>,
    pub filled_qty: String,
    pub limit_price: Option<String>,
    pub stop_price: Option<String>,
    pub filled_avg_price: Option<String>,
    pub status: String,
    pub extended_hours: bool,
    pub created_at: String,
    pub replaced_by: Option<String>,
    pub legs: Vec<WireOrder>,
}

/// Alpaca's position object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WirePosition {
    pub symbol: String,
    pub asset_class: String,
    pub qty: String,
    pub avg_entry_price: String,
}

/// Alpaca's account object.
///
/// `account_number` and `id` are named here **so that the redaction pass can replace them**:
/// journal spec §6.4 keeps both in the personal-data vault and holds them by reference, so
/// neither ever reaches a draft, a log, or the artifact store (interpretation 24).
///
/// `Debug` is written by hand and prints neither of them, so a log line or a panic message that
/// formats the account cannot leak personal data either (`AGENTS.md` rule 7, journal §6.4).
#[derive(Clone, PartialEq, Eq)]
pub struct WireAccount {
    pub id: String,
    pub account_number: String,
    pub status: String,
    pub crypto_status: String,
    pub currency: String,
    pub trading_blocked: bool,
    pub account_blocked: bool,
    pub trade_suspended_by_user: bool,
    pub multiplier: String,
    pub equity: String,
    pub cash: String,
    pub buying_power: String,
    pub non_marginable_buying_power: String,
    pub accrued_fees: String,
}

impl core::fmt::Debug for WireAccount {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WireAccount")
            .field("status", &self.status)
            .field("crypto_status", &self.crypto_status)
            .field("currency", &self.currency)
            .field("trading_blocked", &self.trading_blocked)
            .field("account_blocked", &self.account_blocked)
            .field("trade_suspended_by_user", &self.trade_suspended_by_user)
            .field("multiplier", &self.multiplier)
            .field("equity", &self.equity)
            .field("cash", &self.cash)
            .field("buying_power", &self.buying_power)
            .field(
                "non_marginable_buying_power",
                &self.non_marginable_buying_power,
            )
            .field("accrued_fees", &self.accrued_fees)
            .finish_non_exhaustive()
    }
}

/// One row of `/v2/account/activities`, restricted to the `FILL` activities this stream ingests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireActivity {
    pub id: String,
    pub activity_type: String,
    pub symbol: String,
    pub side: String,
    pub qty: String,
    pub price: String,
    pub transaction_time: String,
    pub order_id: String,
    pub client_order_id: Option<String>,
}

/// Alpaca's error body for a rejected request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireRejectBody {
    pub code: Option<i64>,
    pub message: String,
}

/// Parses one order object from a response body.
pub fn order(body: &[u8]) -> Result<BrokerOrder, WireError> {
    order_from(&json(body)?)
}

/// Parses a list of open orders.
pub fn open_orders(body: &[u8]) -> Result<Vec<BrokerOrder>, WireError> {
    list(&json(body)?, "orders")?
        .iter()
        .map(order_from)
        .collect()
}

/// Parses the position list.
pub fn positions(body: &[u8]) -> Result<Vec<BrokerPosition>, WireError> {
    list(&json(body)?, "positions")?
        .iter()
        .map(position_from)
        .collect()
}

/// Parses the account object, dropping `id` and `account_number` before anything else: the type
/// this returns has nowhere to put either (journal spec §6.4).
pub fn account(body: &[u8]) -> Result<BrokerAccount, WireError> {
    let value = json(body)?;
    let fields = object(&value, "account")?;
    Ok(BrokerAccount {
        status: text(fields, "status")?.to_owned(),
        crypto_status: text(fields, "crypto_status")?.to_owned(),
        trading_blocked: flag(fields, "trading_blocked")?,
        account_blocked: flag(fields, "account_blocked")?,
        trade_suspended_by_user: flag(fields, "trade_suspended_by_user")?,
        multiplier: text(fields, "multiplier")?
            .parse()
            .map_err(|_| WireError::WrongType {
                field: "multiplier",
            })?,
        equity: usd(fields, "equity")?,
        cash: usd(fields, "cash")?,
        buying_power: usd(fields, "buying_power")?,
        non_marginable_buying_power: usd(fields, "non_marginable_buying_power")?,
        accrued_fees: usd(fields, "accrued_fees")?,
        last_equity: usd(fields, "last_equity")?,
        maintenance_margin: usd(fields, "maintenance_margin")?,
    })
}

/// Parses the `FILL` activities since the cursor.
///
/// Alpaca's `FILL` activity carries no fee: regulatory fees post as their own activities, and on
/// paper they are simulated in the shadow ledger (trading-domain spec §10), so a fill's `fees` is
/// zero here. Any other activity type is not interpreted by this stream (DEC-85).
pub fn activities(body: &[u8]) -> Result<Vec<BrokerFill>, WireError> {
    list(&json(body)?, "activities")?
        .iter()
        .map(fill_from)
        .collect()
}

/// Parses a rejection body into the signals trading-domain spec §7.3's restriction table reads.
pub fn reject(status: u16, body: &[u8]) -> Result<BrokerReject, WireError> {
    let value = json(body)?;
    let fields = object(&value, "reject")?;
    let code = match fields.get("code") {
        None | Some(Value::Null) => None,
        Some(Value::Number(code)) => Some(code.to_string()),
        Some(_) => return Err(WireError::WrongType { field: "code" }),
    };
    Ok(BrokerReject {
        client_order_id: None,
        http_status: status,
        code,
        message: text(fields, "message")?.to_owned(),
    })
}

/// Maps one raw Alpaca status through trading-domain spec §5.7's table.
///
/// The table is **total**: every value it names maps, and any other value is
/// [`WireError::UnknownStatus`], which the executor turns into a pause and an alert — the spec's
/// own last row — rather than a silent no-op.
pub fn status(raw: &str) -> Result<StatusMapping, WireError> {
    Ok(match raw {
        "new" | "accepted" | "pending_new" | "accepted_for_bidding" | "held" => {
            StatusMapping::Becomes(OrderState::Accepted)
        }
        "partially_filled" => StatusMapping::Becomes(OrderState::PartiallyFilled),
        "filled" => StatusMapping::Becomes(OrderState::Filled),
        "done_for_day" | "stopped" | "calculated" => StatusMapping::Unchanged,
        "pending_cancel" => StatusMapping::Becomes(OrderState::PendingCancel),
        "canceled" => StatusMapping::Becomes(OrderState::Canceled),
        "expired" => StatusMapping::Becomes(OrderState::Expired),
        "rejected" => StatusMapping::Becomes(OrderState::Rejected),
        "suspended" => StatusMapping::AcceptedFlaggedRestricted,
        "pending_replace" => StatusMapping::Becomes(OrderState::PendingReplace),
        "replaced" => StatusMapping::ReplacedPair,
        other => {
            return Err(WireError::UnknownStatus {
                status: other.to_owned(),
            });
        }
    })
}

/// The canonical JSON body of one submission, with **our** `client_order_id` on the wire.
///
/// The body is canonical so that a recorded `requests.txt` line pins exactly what was sent, and
/// so that two runs of the same submission produce byte-identical bytes (ES-21): keys are sorted
/// and every number is `mandate-num`'s canonical text.
///
/// The order class follows §5.2's capability matrix: a bracket or an OCO carries its two legs and
/// no `extended_hours` (the legs share the entry's TIF and take no extended hours), an OCO has no
/// top-level limit because its take-profit leg is the limit, and `extended_hours` is sent only on
/// a simple equity order, never on crypto, which trades around the clock. An OCO whose legs name
/// another quantity than the order, or that carries a top-level limit or stop, is refused rather than sent
/// with one of the two: §5.4's OCO is for the filled quantity, and a guess could over-sell.
pub fn submission_body(order: &SubmitOrder) -> Result<String, WireError> {
    let mut body = Map::new();
    let mut put = |key: &str, value: Value| body.insert(key.to_owned(), value);
    put(
        "client_order_id",
        Value::from(order.client_order_id.as_str()),
    );
    put("symbol", Value::from(order.instrument.as_str()));
    put("qty", Value::from(order.qty.to_string()));
    put("side", Value::from(side_text(order.side)));
    put("type", Value::from(order_type_text(order.order_type)));
    put("time_in_force", Value::from(tif_text(order.tif)));
    if let Some(limit) = order.limit_price {
        put("limit_price", Value::from(limit.to_string()));
    }
    if let Some(stop) = order.stop_price {
        put("stop_price", Value::from(stop.to_string()));
    }
    let legs = match (&order.bracket, &order.oco) {
        (Some(_), Some(_)) => {
            return Err(WireError::WrongType {
                field: "order_class",
            });
        }
        (Some(bracket), None) => Some(("bracket", bracket.take_profit, bracket.stop)),
        (None, Some(oco)) => {
            if oco.qty != order.qty || order.limit_price.is_some() || order.stop_price.is_some() {
                return Err(WireError::WrongType { field: "oco" });
            }
            Some(("oco", oco.take_profit, oco.stop))
        }
        (None, None) => None,
    };
    match legs {
        Some((class, take_profit, stop)) => {
            put("order_class", Value::from(class));
            put("take_profit", leg("limit_price", take_profit));
            put("stop_loss", leg("stop_price", stop));
        }
        None => {
            put("order_class", Value::from("simple"));
            if !is_crypto(&order.instrument) {
                put("extended_hours", Value::from(order.extended_hours));
            }
        }
    }
    serde_json::to_string(&Value::Object(body)).map_err(|_| WireError::NotJson)
}

/// Reads one decimal field as canonical text, refusing a JSON number, an exponent, and more
/// places than the field allows (ES-23).
///
/// The text comes back exactly as the broker sent it; [`canonical`] is the one place a trailing
/// zero is dropped on the way into `mandate-num`. Whether the rest is a well-formed decimal is
/// `mandate-num`'s to judge, once: its parser accepts only text that is its own canonical form, so
/// a stray sign, letter or second point cannot become a quantity, a price or an amount.
///
/// So the text this answers is **not** yet a number: `abc` and `1.2.3` come back `Ok`. It is
/// public for the ES-23 tests that pin the float and exponent refusals, and a caller that needs a
/// value parses the text with `mandate-num`, as every parser in this module does through
/// `number`.
pub fn decimal_text(raw: &serde_json::Value, field: &'static str) -> Result<String, WireError> {
    let text = match raw {
        Value::String(text) => text.as_str(),
        Value::Number(_) => return Err(WireError::FloatNumber { field }),
        _ => return Err(WireError::WrongType { field }),
    };
    if text.contains(['e', 'E']) {
        return Err(WireError::ExponentForm { field });
    }
    let places = text.split_once('.').map_or("", |(_, places)| places);
    if places.len() > MAX_PLACES {
        return Err(WireError::TooManyPlaces { field });
    }
    Ok(text.to_owned())
}

/// The most fractional digits a broker decimal may carry: the 9 places of a price or a quantity
/// (trading-domain spec §2.1).
pub(crate) const MAX_PLACES: usize = 9;

/// `mandate-num`'s canonical form of text [`decimal_text`] accepted: trailing fractional zeros,
/// and a point left with nothing after it, are dropped. Nothing else changes, so no value moves.
pub(crate) fn canonical(text: &str) -> String {
    match text.split_once('.') {
        Some((whole, places)) => match places.trim_end_matches('0') {
            "" => whole.to_owned(),
            kept => format!("{whole}.{kept}"),
        },
        None => text.to_owned(),
    }
}

pub(crate) fn json(body: &[u8]) -> Result<Value, WireError> {
    serde_json::from_slice(body).map_err(|_| WireError::NotJson)
}

pub(crate) fn object<'a>(
    value: &'a Value,
    field: &'static str,
) -> Result<&'a Map<String, Value>, WireError> {
    value.as_object().ok_or(WireError::WrongType { field })
}

fn list<'a>(value: &'a Value, field: &'static str) -> Result<&'a Vec<Value>, WireError> {
    value.as_array().ok_or(WireError::WrongType { field })
}

pub(crate) fn text<'a>(
    fields: &'a Map<String, Value>,
    field: &'static str,
) -> Result<&'a str, WireError> {
    fields
        .get(field)
        .ok_or(WireError::MissingField { field })?
        .as_str()
        .ok_or(WireError::WrongType { field })
}

/// A text field that may be absent or `null`.
fn optional_text<'a>(
    fields: &'a Map<String, Value>,
    field: &'static str,
) -> Result<Option<&'a str>, WireError> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text)),
        Some(_) => Err(WireError::WrongType { field }),
    }
}

pub(crate) fn flag(fields: &Map<String, Value>, field: &'static str) -> Result<bool, WireError> {
    fields
        .get(field)
        .ok_or(WireError::MissingField { field })?
        .as_bool()
        .ok_or(WireError::WrongType { field })
}

/// The canonical text of a decimal field that must be present.
pub(crate) fn number(
    fields: &Map<String, Value>,
    field: &'static str,
) -> Result<String, WireError> {
    let raw = fields.get(field).ok_or(WireError::MissingField { field })?;
    Ok(canonical(&decimal_text(raw, field)?))
}

pub(crate) fn qty(fields: &Map<String, Value>, field: &'static str) -> Result<Qty, WireError> {
    Ok(Qty::parse(&number(fields, field)?)?)
}

pub(crate) fn price(fields: &Map<String, Value>, field: &'static str) -> Result<Price, WireError> {
    Ok(Price::parse(&number(fields, field)?)?)
}

fn usd(fields: &Map<String, Value>, field: &'static str) -> Result<Usd, WireError> {
    Ok(Usd::parse(&number(fields, field)?)?)
}

/// A price that may be absent or `null`, as a market order's limit is.
fn optional_price(
    fields: &Map<String, Value>,
    field: &'static str,
) -> Result<Option<Price>, WireError> {
    match fields.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => price(fields, field).map(Some),
    }
}

/// A broker symbol. It reaches `/v2/positions/{symbol}` (the read, and the account-wide close), so
/// it is held to a symbol's alphabet: one or two `/`-separated segments (`AAPL`, `BRK.B`,
/// `BTC/USD`), each starting with a letter or a digit and holding only letters, digits and `.`. A
/// dot segment, a percent sign, a query, or an empty segment is not a symbol this crate can read
/// (#191 review, round 3).
pub(crate) fn instrument(fields: &Map<String, Value>) -> Result<InstrumentId, WireError> {
    let raw = text(fields, "symbol")?;
    let segments: Vec<&str> = raw.split('/').collect();
    let symbol_like = segments.len() <= 2
        && segments.iter().all(|segment| {
            segment
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_alphanumeric())
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.')
        });
    if !symbol_like {
        return Err(WireError::WrongType { field: "symbol" });
    }
    InstrumentId::new(raw).map_err(|_| WireError::WrongType { field: "symbol" })
}

/// An order's quantity. An order placed by notional amount (`qty: null`, possible on the owner's
/// own account) names no share quantity, so its filled quantity is the only quantity known: it is
/// read as that, and the order is ingested like any other rather than failing the page it sits on
/// (trading-domain spec §11's external-activity row, #191 review, round 1). An order that omits
/// `qty` altogether is still unreadable.
fn order_qty(fields: &Map<String, Value>, filled: Qty) -> Result<Qty, WireError> {
    match fields.get("qty") {
        Some(Value::Null) => Ok(filled),
        _ => qty(fields, "qty"),
    }
}

fn side(fields: &Map<String, Value>) -> Result<Side, WireError> {
    match text(fields, "side")? {
        "buy" => Ok(Side::Buy),
        "sell" => Ok(Side::Sell),
        _ => Err(WireError::WrongType { field: "side" }),
    }
}

/// The New York calendar date of a broker timestamp: the trading day an order was created on or
/// a fill was executed on (trading-domain spec §6.1).
fn new_york_date(fields: &Map<String, Value>, field: &'static str) -> Result<Date, WireError> {
    let at = UtcNanos::parse_rfc3339(text(fields, field)?)?;
    Ok(new_york_date_and_hour(at)?.0)
}

/// A broker order id: the entry's own, a leg's, or the one a replacement names. The cancel puts
/// each in a path (`/v2/orders/{id}`), and Alpaca's is a UUID, so an id outside `[A-Za-z0-9-]+`
/// is not one this crate can read: a dot, a percent sign or a slash never reaches path building,
/// behind the allowlist's own dot-segment rule (#174, #189).
pub(crate) fn broker_id(raw: &str, field: &'static str) -> Result<String, WireError> {
    checked_id(raw, field, b"")
}

/// A broker activity id, which becomes the activities cursor the next read sends as its
/// `page_token`. Alpaca's is a timestamp, `::`, and a UUID, so the colon is the one byte it adds
/// to a broker order id's alphabet; `&`, `=`, `%`, `.` and `/` stay refused, so no id can add a
/// query parameter or move the path.
fn activity_id(raw: &str) -> Result<String, WireError> {
    checked_id(raw, "id", b":")
}

fn checked_id(raw: &str, field: &'static str, extra: &[u8]) -> Result<String, WireError> {
    if raw.is_empty()
        || !raw
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || extra.contains(&b))
    {
        return Err(WireError::WrongType { field });
    }
    Ok(raw.to_owned())
}

fn order_from(value: &Value) -> Result<BrokerOrder, WireError> {
    let fields = object(value, "order")?;
    let legs = match fields.get("legs") {
        None | Some(Value::Null) => Vec::new(),
        Some(legs) => list(legs, "legs")?
            .iter()
            .map(order_from)
            .collect::<Result<_, WireError>>()?,
    };
    let filled_qty = qty(fields, "filled_qty")?;
    Ok(BrokerOrder {
        broker_order_id: broker_id(text(fields, "id")?, "id")?,
        client_order_id: optional_text(fields, "client_order_id")?.map(str::to_owned),
        instrument: instrument(fields)?,
        side: side(fields)?,
        qty: order_qty(fields, filled_qty)?,
        filled_qty,
        limit_price: optional_price(fields, "limit_price")?,
        stop_price: optional_price(fields, "stop_price")?,
        status: text(fields, "status")?.to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: optional_text(fields, "replaced_by")?
            .map(|raw| broker_id(raw, "replaced_by"))
            .transpose()?,
        legs,
        created_on: Some(new_york_date(fields, "created_at")?),
    })
}

fn position_from(value: &Value) -> Result<BrokerPosition, WireError> {
    let fields = object(value, "positions")?;
    Ok(BrokerPosition {
        instrument: instrument(fields)?,
        qty: SignedQty::parse(&number(fields, "qty")?)?,
        avg_entry_price: price(fields, "avg_entry_price")?,
    })
}

fn fill_from(value: &Value) -> Result<BrokerFill, WireError> {
    let fields = object(value, "activities")?;
    if text(fields, "activity_type")? != "FILL" {
        return Err(WireError::NotInterpreted {
            field: "activity_type",
            story: "E7-3",
        });
    }
    Ok(BrokerFill {
        fill_id: FillId(activity_id(text(fields, "id")?)?),
        client_order_id: optional_text(fields, "client_order_id")?.map(str::to_owned),
        instrument: instrument(fields)?,
        side: side(fields)?,
        qty: qty(fields, "qty")?,
        price: price(fields, "price")?,
        fees: Usd::ZERO,
        trade_date: new_york_date(fields, "transaction_time")?,
    })
}

fn leg(key: &str, price: Price) -> Value {
    let mut leg = Map::new();
    leg.insert(key.to_owned(), Value::from(price.to_string()));
    Value::Object(leg)
}

/// Alpaca names a crypto pair with a slash (`BTC/USD`) and an equity without one.
fn is_crypto(instrument: &InstrumentId) -> bool {
    instrument.as_str().contains('/')
}

fn side_text(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn order_type_text(order_type: OrderType) -> &'static str {
    match order_type {
        OrderType::Limit => "limit",
        OrderType::Market => "market",
        OrderType::StopLimit => "stop_limit",
    }
}

fn tif_text(tif: TimeInForce) -> &'static str {
    match tif {
        TimeInForce::Day => "day",
        TimeInForce::Gtc => "gtc",
        TimeInForce::Ioc => "ioc",
    }
}
