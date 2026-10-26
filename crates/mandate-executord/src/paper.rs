//! The fixture paper broker (DEC-2007 item 1): Alpaca's paper trading host answered in process,
//! below `mandate-alpaca`'s client, with the bodies that host sends, so every rule the client holds
//! applies to it unchanged. It names no host, holds no credential, and opens no socket: the
//! executor's broker in the dev and CI cells is this fixture, never a broker host (E21-30).
//!
//! What it answers, and nothing more:
//! - `GET /v2/account`: an active paper account with the fixture's cash, and no account number;
//! - `GET /v2/positions` and `GET /v2/account/activities`: what its orders filled, which is nothing
//!   in this slice, since no order fills here (fills come with the order slice, X1b);
//! - `POST /v2/orders`: an order accepted and resting, or `422` for a `client_order_id` it already
//!   holds, as Alpaca answers;
//! - `GET /v2/orders` (open orders, legs nested), `GET /v2/orders:by_client_order_id` (`404` for
//!   one it never took), and `DELETE /v2/orders/{id}` (`204`, the order then `canceled`).
//!
//! Every other request is `404`. Ids are a counter in UUID form, so a run is deterministic.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_alpaca::{HttpRequest, Method, Response, TradingTransport, TransportError};
use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_time::UtcNanos;

/// The fixture account's cash, equity and buying power: a paper account of 100,000 dollars.
pub const CASH: &str = "100000";

#[derive(Debug, Clone)]
struct Order {
    id: String,
    client_order_id: String,
    symbol: String,
    side: String,
    order_type: String,
    time_in_force: String,
    qty: String,
    limit_price: Option<String>,
    stop_price: Option<String>,
    status: String,
}

#[derive(Debug, Default)]
struct Books {
    orders: Vec<Order>,
    sequence: u64,
}

/// The fixture broker. Clones share one book, as a broker shared by every request is.
#[derive(Debug, Clone)]
pub struct FixturePaper {
    books: Rc<RefCell<Books>>,
    at: String,
}

impl FixturePaper {
    /// A broker with an empty book whose records are stamped `at`.
    pub fn new(at: UtcNanos) -> Self {
        Self {
            books: Rc::default(),
            at: at.to_string(),
        }
    }

    /// A broker with an empty book whose records are stamped `at`, and that fills: the cell's
    /// fixture once its orders fill (X1b-3). Until that slice lands it rests every order, as
    /// [`FixturePaper::new`] does, which is what its pending tests fail on.
    pub fn filling(at: UtcNanos) -> Self {
        Self::new(at)
    }

    fn answer(&self, request: &HttpRequest) -> Response {
        let path = request.path_and_query();
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        match (request.method(), route) {
            (Method::Get, "/v2/account") => ok(account()),
            (Method::Get, "/v2/positions" | "/v2/account/activities") => {
                ok(Value::Array(Vec::new()))
            }
            (Method::Get, "/v2/orders") => ok(Value::Array(
                self.books
                    .borrow()
                    .orders
                    .iter()
                    .filter(|o| o.status == "new")
                    .map(|o| order_json(o, &self.at))
                    .collect(),
            )),
            (Method::Get, "/v2/orders:by_client_order_id") => {
                let wanted = query
                    .split('&')
                    .find_map(|pair| pair.strip_prefix("client_order_id="))
                    .unwrap_or_default();
                let books = self.books.borrow();
                match books.orders.iter().find(|o| o.client_order_id == wanted) {
                    Some(order) => ok(order_json(order, &self.at)),
                    None => refusal(404, 40_410_000, "order not found"),
                }
            }
            (Method::Post, "/v2/orders") => self.submit(request.body()),
            (Method::Delete, route) => match route.strip_prefix("/v2/orders/") {
                Some(id) => self.cancel(id),
                None => refusal(404, 40_410_000, "not found"),
            },
            _ => refusal(404, 40_410_000, "not found"),
        }
    }

    fn submit(&self, body: Option<&str>) -> Response {
        let Some(fields) = body.and_then(|b| parse(b.as_bytes()).ok()) else {
            return refusal(422, 40_010_000, "request body is not valid");
        };
        let text = |member: &str| {
            fields
                .get(member)
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        let (Some(client_order_id), Some(symbol), Some(side), Some(order_type)) = (
            text("client_order_id"),
            text("symbol"),
            text("side"),
            text("type"),
        ) else {
            return refusal(422, 40_010_000, "request body is not valid");
        };
        let mut books = self.books.borrow_mut();
        if books
            .orders
            .iter()
            .any(|o| o.client_order_id == client_order_id)
        {
            return refusal(422, 40_010_001, "client_order_id must be unique");
        }
        books.sequence = books.sequence.saturating_add(1);
        let order = Order {
            id: uuid(books.sequence),
            client_order_id,
            symbol,
            side,
            order_type,
            time_in_force: text("time_in_force").unwrap_or_else(|| "day".to_owned()),
            qty: text("qty").unwrap_or_else(|| "0".to_owned()),
            limit_price: text("limit_price"),
            stop_price: text("stop_price"),
            status: "new".to_owned(),
        };
        let answer = order_json(&order, &self.at);
        books.orders.push(order);
        ok(answer)
    }

    fn cancel(&self, id: &str) -> Response {
        let mut books = self.books.borrow_mut();
        match books
            .orders
            .iter_mut()
            .find(|o| o.id == id && o.status == "new")
        {
            Some(order) => {
                order.status = "canceled".to_owned();
                Response {
                    status: 204,
                    body: Vec::new(),
                }
            }
            None => refusal(422, 42_210_000, "order is not cancelable"),
        }
    }
}

impl TradingTransport for FixturePaper {
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        Ok(self.answer(request))
    }
}

/// A broker order id in UUID form from the fixture's counter.
fn uuid(n: u64) -> String {
    format!("00000000-0000-4000-8000-{n:012}")
}

fn ok(value: Value) -> Response {
    Response {
        status: 200,
        body: to_canonical(&value),
    }
}

fn refusal(status: u16, code: u64, message: &str) -> Response {
    let code = mandate_canon::Int::new(code).map_or(Value::Null, Value::Int);
    Response {
        status,
        body: to_canonical(&object(vec![
            ("code", code),
            ("message", Value::Str(message.to_owned())),
        ])),
    }
}

fn object(members: Vec<(&str, Value)>) -> Value {
    let mut map = Object::new();
    for (name, value) in members {
        if let Ok(key) = Key::new(name) {
            map.insert(key, value);
        }
    }
    Value::Object(map)
}

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn optional(text: Option<&String>) -> Value {
    text.map_or(Value::Null, |t| s(t))
}

/// The account object, with the members `mandate-alpaca` reads. `id` and `account_number` are
/// fixed placeholders: the fixture account has no number, and the client drops both on reading.
fn account() -> Value {
    object(vec![
        ("account_blocked", Value::Bool(false)),
        ("account_number", s("fixture")),
        ("accrued_fees", s("0")),
        ("buying_power", s(CASH)),
        ("cash", s(CASH)),
        ("crypto_status", s("ACTIVE")),
        ("currency", s("USD")),
        ("equity", s(CASH)),
        ("id", s("fixture")),
        ("last_equity", s(CASH)),
        ("maintenance_margin", s("0")),
        ("multiplier", s("1")),
        ("non_marginable_buying_power", s(CASH)),
        ("status", s("ACTIVE")),
        ("trade_suspended_by_user", Value::Bool(false)),
        ("trading_blocked", Value::Bool(false)),
    ])
}

fn order_json(order: &Order, at: &str) -> Value {
    object(vec![
        ("asset_class", s("us_equity")),
        ("client_order_id", s(&order.client_order_id)),
        ("created_at", s(at)),
        ("extended_hours", Value::Bool(false)),
        ("filled_avg_price", Value::Null),
        ("filled_qty", s("0")),
        ("id", s(&order.id)),
        ("legs", Value::Null),
        ("limit_price", optional(order.limit_price.as_ref())),
        ("order_type", s(&order.order_type)),
        ("qty", s(&order.qty)),
        ("replaced_by", Value::Null),
        ("side", s(&order.side)),
        ("status", s(&order.status)),
        ("stop_price", optional(order.stop_price.as_ref())),
        ("symbol", s(&order.symbol)),
        ("time_in_force", s(&order.time_in_force)),
        ("type", s(&order.order_type)),
    ])
}
