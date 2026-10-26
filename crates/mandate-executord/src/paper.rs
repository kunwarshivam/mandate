//! The fixture paper broker (DEC-2007 item 1): Alpaca's paper trading host answered in process,
//! below `mandate-alpaca`'s client, with the bodies that host sends, so every rule the client holds
//! applies to it unchanged. It names no host, holds no credential, and opens no socket: the
//! executor's broker in the dev and CI cells is this fixture, never a broker host (E21-30).
//!
//! What it answers, and nothing more:
//! - `GET /v2/account`: an active paper account with the fixture's cash, less what its fills
//!   bought and plus what they sold, and no account number;
//! - `GET /v2/positions`: the net of its fills per symbol, at the buys' average price;
//! - `GET /v2/account/activities`: its fills as `FILL` activities, oldest first, after the
//!   `page_token` the client sends;
//! - `POST /v2/orders`: an order accepted, or `422` for a `client_order_id` it already holds, as
//!   Alpaca answers; a bracket or an OCO with its two legs; a sell of more than the position
//!   less what open sells hold (an OCO's or a bracket's legs once), none held included, `403`
//!   with code 40310000, refused before anything changes, since there are no short sales
//!   (`AGENTS.md` rule 12);
//! - `GET /v2/assets/{symbol}`: the asset record of each of [`ASSETS`], active, tradable and
//!   fractionable, with no attribute, so neither an IPO nor a PTP; `404` for any other symbol;
//! - `GET /v2/orders` (open orders, legs nested), `GET /v2/orders/{id}` (the read-back of a
//!   cancel by the broker's id), `GET /v2/orders:by_client_order_id` (`404` for
//!   one it never took), and `DELETE /v2/orders/{id}` (`204`, the order and its group then
//!   `canceled`; `422` for one no longer open).
//!
//! [`FixturePaper::new`] fills nothing: every order rests. [`FixturePaper::filling`] fills a limit
//! order whole at its limit when it is placed, a simple order or a bracket's entry, buy or sell;
//! the bracket's legs then rest. The submission's answer is the order as Alpaca accepts it, before
//! the fill, which later reads report, as a marketable order's would. An OCO, a stop and a market
//! order rest, since the fixture has no price. The serving process with no paper credential runs
//! over [`FixturePaper::filling`], so the cell's trade loop fills (X1b-3). A fill names the broker's order id and no client order id, as Alpaca's activity does
//! (DEC-2353), and Alpaca paper does not simulate regulatory fees (trading spec §11), so a fill
//! moves the cash by its notional alone.
//!
//! Every other request is `404`. Ids are a counter in UUID form, so a run is deterministic.
//!
//! On the data host it answers `GET /v2/stocks/{symbol}/quotes/latest?feed=iex` for each of
//! [`ASSETS`] with that asset's fixed bid and ask, stamped by the clock [`FixturePaper::quoting`]
//! gives it, so the quote is as fresh as the reader's own clock says; without one, and for any
//! other symbol or read, it answers `404`, which reads as no quote. It is the venue, so it knows
//! none of its assets is halted ([`FixturePaper::halted`]).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mandate_alpaca::{
    BarsRequest, DataTransport, HttpRequest, Method, QuoteRequest, Response, TradingTransport,
    TransportError,
};
use mandate_canon::{Key, Object, Value, parse, to_canonical};
use mandate_time::UtcNanos;

/// The fixture account's cash, equity and buying power: a paper account of 100,000 dollars.
pub const CASH: &str = "100000";

/// The fixture's assets: symbol, listing exchange as Alpaca writes it, and the latest quote's bid
/// and ask. The cell's deployments trade SPY and QQQ.
pub const ASSETS: [FixtureAsset; 2] = [
    FixtureAsset {
        symbol: "SPY",
        exchange: "ARCA",
        bid: "580.11",
        ask: "580.13",
    },
    FixtureAsset {
        symbol: "QQQ",
        exchange: "NASDAQ",
        bid: "500.21",
        ask: "500.23",
    },
];

/// One of the fixture's [`ASSETS`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureAsset {
    pub symbol: &'static str,
    pub exchange: &'static str,
    pub bid: &'static str,
    pub ask: &'static str,
}

/// The listed asset `symbol`, if the fixture lists it.
fn listed(symbol: &str) -> Option<(usize, FixtureAsset)> {
    ASSETS
        .iter()
        .copied()
        .enumerate()
        .find(|(_, asset)| asset.symbol == symbol)
}

/// The clock a quote is stamped by: the reader's own, so the fixture never ages its quote.
#[derive(Clone)]
struct QuoteClock(Rc<dyn Fn() -> UtcNanos>);

impl std::fmt::Debug for QuoteClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("QuoteClock")
    }
}

/// The fixture's decimal scale: amounts are whole multiples of 10⁻⁹, as `mandate-num`'s are.
const SCALE: i128 = 1_000_000_000;

#[derive(Debug, Clone)]
struct Order {
    id: String,
    client_order_id: String,
    symbol: String,
    side: String,
    order_type: String,
    order_class: String,
    time_in_force: String,
    qty: String,
    limit_price: Option<String>,
    stop_price: Option<String>,
    status: String,
    filled_qty: String,
    filled_avg_price: Option<String>,
    /// The broker id of the bracket's or OCO's first order, for each of its legs.
    parent: Option<String>,
}

/// One fill, as its `FILL` activity reports it.
#[derive(Debug, Clone)]
struct Fill {
    id: String,
    order_id: String,
    symbol: String,
    side: String,
    qty: String,
    price: String,
}

/// A symbol's net quantity and the cost of what is held, both at [`SCALE`].
#[derive(Debug, Clone, Copy, Default)]
struct Holding {
    qty: i128,
    cost: i128,
}

#[derive(Debug)]
struct Books {
    orders: Vec<Order>,
    fills: Vec<Fill>,
    holdings: BTreeMap<String, Holding>,
    cash: i128,
    sequence: u64,
    served: Vec<String>,
}

impl Default for Books {
    fn default() -> Self {
        Self {
            orders: Vec::new(),
            fills: Vec::new(),
            holdings: BTreeMap::new(),
            cash: scaled(CASH).unwrap_or_default(),
            sequence: 0,
            served: Vec::new(),
        }
    }
}

impl Books {
    /// What a new sell of `symbol` may take, as Alpaca's `qty_available`: the holding less what
    /// open sells already hold, an OCO's or a bracket's legs once per group, since only one of
    /// them can fill. `None` when the sum does not fit.
    fn available(&self, symbol: &str) -> Option<i128> {
        let mut held: BTreeMap<&str, i128> = BTreeMap::new();
        for order in &self.orders {
            if order.symbol == symbol && order.side == "sell" && order.status == "new" {
                let group = order.parent.as_deref().unwrap_or(&order.id);
                let qty = scaled(&order.qty)?;
                let slot = held.entry(group).or_default();
                *slot = (*slot).max(qty);
            }
        }
        let holding = self.holdings.get(symbol).map_or(0, |holding| holding.qty);
        held.values()
            .try_fold(holding, |left, qty| left.checked_sub(*qty))
    }

    fn next_id(&mut self) -> String {
        self.sequence = self.sequence.saturating_add(1);
        uuid(self.sequence)
    }

    /// The order and every leg of it, the order first.
    fn group(&self, id: &str) -> Vec<Order> {
        self.orders
            .iter()
            .filter(|o| o.id == id || o.parent.as_deref() == Some(id))
            .cloned()
            .collect()
    }

    /// Fills `index`'s order whole at its limit: its status and fill, the `FILL` activity naming
    /// its broker id, the holding, and the cash by the notional. Its legs, held until now, rest.
    fn fill(&mut self, index: usize) -> Option<()> {
        let order = self.orders.get(index)?.clone();
        let price = order.limit_price.clone()?;
        let qty = scaled(&order.qty)?;
        let notional = qty.checked_mul(scaled(&price)?)?.checked_div(SCALE)?;
        let holding = self.holdings.entry(order.symbol.clone()).or_default();
        if order.side == "buy" {
            holding.qty = holding.qty.checked_add(qty)?;
            holding.cost = holding.cost.checked_add(notional)?;
            self.cash = self.cash.checked_sub(notional)?;
        } else {
            let average = average(*holding)?;
            holding.cost = holding
                .cost
                .checked_sub(average.checked_mul(qty)?.checked_div(SCALE)?)?;
            holding.qty = holding.qty.checked_sub(qty)?;
            self.cash = self.cash.checked_add(notional)?;
        }
        if holding.qty == 0 {
            self.holdings.remove(&order.symbol);
        }
        let activity = self.next_id();
        self.fills.push(Fill {
            id: format!("{:017}::{activity}", self.sequence),
            order_id: order.id.clone(),
            symbol: order.symbol,
            side: order.side,
            qty: order.qty.clone(),
            price: price.clone(),
        });
        let filled = self.orders.get_mut(index)?;
        filled.status = "filled".to_owned();
        filled.filled_qty = filled.qty.clone();
        filled.filled_avg_price = Some(price);
        for leg in &mut self.orders {
            if leg.parent.as_deref() == Some(order.id.as_str()) {
                leg.status = "new".to_owned();
            }
        }
        Some(())
    }
}

/// The fixture broker. Clones share one book, as a broker shared by every request is.
#[derive(Debug, Clone)]
pub struct FixturePaper {
    books: Rc<RefCell<Books>>,
    at: String,
    fills: bool,
    quotes: Option<QuoteClock>,
}

impl FixturePaper {
    /// A broker with an empty book whose records are stamped `at`, and whose orders all rest.
    pub fn new(at: UtcNanos) -> Self {
        Self {
            books: Rc::default(),
            at: at.to_string(),
            fills: false,
            quotes: None,
        }
    }

    /// The broker quoting each of [`ASSETS`] on the data host, every quote stamped by `clock` when
    /// it is read.
    #[must_use]
    pub fn quoting(self, clock: impl Fn() -> UtcNanos + 'static) -> Self {
        Self {
            quotes: Some(QuoteClock(Rc::new(clock))),
            ..self
        }
    }

    /// Whether `symbol` is halted: never, for one of [`ASSETS`], since the fixture is the venue
    /// and halts nothing; unread, `None`, for any other symbol.
    pub fn halted(&self, symbol: &str) -> Option<bool> {
        listed(symbol).map(|_| false)
    }

    /// A broker with an empty book whose records are stamped `at`, that fills each limit order
    /// whole at its limit when it is placed (X1b-3): the cell's fixture once its orders fill.
    pub fn filling(at: UtcNanos) -> Self {
        Self {
            fills: true,
            ..Self::new(at)
        }
    }

    /// Every trading-host request it answered, as `METHOD /path?query`, oldest first: what a test
    /// reads to see a fact asked of the broker.
    pub fn served(&self) -> Vec<String> {
        self.books.borrow().served.clone()
    }

    fn answer(&self, request: &HttpRequest) -> Response {
        let path = request.path_and_query();
        self.books.borrow_mut().served.push(format!(
            "{} {path}",
            format!("{:?}", request.method()).to_uppercase()
        ));
        let (route, query) = path.split_once('?').unwrap_or((path, ""));
        let param = |name: &str| {
            query
                .split('&')
                .find_map(|pair| pair.strip_prefix(name)?.strip_prefix('='))
                .unwrap_or_default()
        };
        match (request.method(), route) {
            (Method::Get, "/v2/account") => ok(self.account()),
            (Method::Get, "/v2/positions") => ok(self.positions()),
            (Method::Get, "/v2/account/activities") => ok(self.activities(param("page_token"))),
            (Method::Get, "/v2/orders") => {
                let books = self.books.borrow();
                ok(Value::Array(
                    books
                        .orders
                        .iter()
                        .filter(|o| o.parent.is_none())
                        .map(|o| books.group(&o.id))
                        .filter(|group| group.iter().any(open))
                        .map(|group| nested(&group, &self.at))
                        .collect(),
                ))
            }
            (Method::Get, "/v2/orders:by_client_order_id") => {
                let wanted = param("client_order_id");
                let books = self.books.borrow();
                match books.orders.iter().find(|o| o.client_order_id == wanted) {
                    Some(order) => ok(nested(&books.group(&order.id), &self.at)),
                    None => refusal(404, 40_410_000, "order not found"),
                }
            }
            (Method::Post, "/v2/orders") => self.submit(request.body()),
            (Method::Get, route) if route.starts_with("/v2/assets/") => {
                match route.strip_prefix("/v2/assets/").and_then(listed) {
                    Some((index, asset)) => ok(asset_json(index, asset)),
                    None => refusal(404, 40_410_000, "asset not found"),
                }
            }
            (Method::Get, route) => {
                let books = self.books.borrow();
                match route
                    .strip_prefix("/v2/orders/")
                    .and_then(|wanted| books.orders.iter().find(|o| o.id == wanted))
                {
                    Some(order) => ok(nested(&books.group(&order.id), &self.at)),
                    None => refusal(404, 40_410_000, "not found"),
                }
            }
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
        let leg_price = |member: &str, price: &str| {
            fields
                .get(member)
                .and_then(|leg| leg.get(price))
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
        if side == "sell" {
            let wanted = text("qty").as_deref().and_then(scaled);
            if wanted.is_none_or(|wanted| Some(wanted) > books.available(&symbol)) {
                return refusal(403, 40_310_000, "insufficient qty available for order");
            }
        }
        let class = text("order_class").unwrap_or_else(|| "simple".to_owned());
        let take_profit = leg_price("take_profit", "limit_price");
        let stop_loss = leg_price("stop_loss", "stop_price");
        let id = books.next_id();
        let base = Order {
            id: id.clone(),
            client_order_id,
            symbol,
            side,
            order_type,
            order_class: class.clone(),
            time_in_force: text("time_in_force").unwrap_or_else(|| "day".to_owned()),
            qty: text("qty").unwrap_or_else(|| "0".to_owned()),
            limit_price: text("limit_price"),
            stop_price: text("stop_price"),
            status: "new".to_owned(),
            filled_qty: "0".to_owned(),
            filled_avg_price: None,
            parent: None,
        };
        let closing = if base.side == "buy" { "sell" } else { "buy" };
        let mut legs = Vec::new();
        let first = match class.as_str() {
            "oco" => {
                legs.push(Order {
                    id: books.next_id(),
                    client_order_id: books.next_id(),
                    order_type: "stop".to_owned(),
                    limit_price: None,
                    stop_price: stop_loss,
                    parent: Some(id.clone()),
                    ..base.clone()
                });
                Order {
                    order_type: "limit".to_owned(),
                    limit_price: take_profit,
                    ..base
                }
            }
            "bracket" => {
                for (order_type, limit_price, stop_price) in
                    [("limit", take_profit, None), ("stop", None, stop_loss)]
                {
                    legs.push(Order {
                        id: books.next_id(),
                        client_order_id: books.next_id(),
                        side: closing.to_owned(),
                        order_type: order_type.to_owned(),
                        limit_price,
                        stop_price,
                        status: "held".to_owned(),
                        parent: Some(id.clone()),
                        ..base.clone()
                    });
                }
                base
            }
            _ => base,
        };
        let fills = self.fills
            && class != "oco"
            && first.order_type == "limit"
            && first.limit_price.is_some();
        books.orders.push(first);
        let index = books.orders.len().saturating_sub(1);
        books.orders.extend(legs);
        let accepted = nested(&books.group(&id), &self.at);
        if fills && books.fill(index).is_none() {
            return refusal(422, 40_010_000, "request body is not valid");
        }
        ok(accepted)
    }

    /// Cancels the open order `id` and, as Alpaca does for a bracket or an OCO, every open order
    /// of its group: the order's legs, or its parent and the parent's other legs.
    fn cancel(&self, id: &str) -> Response {
        let mut books = self.books.borrow_mut();
        let Some(order) = books.orders.iter().find(|o| o.id == id && open(o)).cloned() else {
            return refusal(422, 42_210_000, "order is not cancelable");
        };
        let root = order.parent.unwrap_or(order.id);
        for member in &mut books.orders {
            if (member.id == root || member.parent.as_deref() == Some(root.as_str()))
                && open(member)
            {
                member.status = "canceled".to_owned();
            }
        }
        Response {
            status: 204,
            body: Vec::new(),
        }
    }

    /// The account object, with the members `mandate-alpaca` reads. `id` and `account_number` are
    /// fixed placeholders: the fixture account has no number, and the client drops both on
    /// reading. Equity is the cash and the cost of what is held; buying power is the cash.
    fn account(&self) -> Value {
        let books = self.books.borrow();
        let held = books
            .holdings
            .values()
            .try_fold(0_i128, |sum, holding| sum.checked_add(holding.cost))
            .unwrap_or_default();
        let cash = decimal(books.cash);
        let equity = decimal(books.cash.saturating_add(held));
        object(vec![
            ("account_blocked", Value::Bool(false)),
            ("account_number", s("fixture")),
            ("accrued_fees", s("0")),
            ("buying_power", s(&cash)),
            ("cash", s(&cash)),
            ("crypto_status", s("ACTIVE")),
            ("currency", s("USD")),
            ("equity", s(&equity)),
            ("id", s("fixture")),
            ("last_equity", s(CASH)),
            ("maintenance_margin", s("0")),
            ("multiplier", s("1")),
            ("non_marginable_buying_power", s(&cash)),
            ("status", s("ACTIVE")),
            ("trade_suspended_by_user", Value::Bool(false)),
            ("trading_blocked", Value::Bool(false)),
        ])
    }

    fn positions(&self) -> Value {
        let books = self.books.borrow();
        Value::Array(
            books
                .holdings
                .iter()
                .map(|(symbol, holding)| {
                    object(vec![
                        ("asset_class", s("us_equity")),
                        (
                            "avg_entry_price",
                            s(&decimal(average(*holding).unwrap_or_default())),
                        ),
                        ("qty", s(&decimal(holding.qty))),
                        ("side", s(if holding.qty < 0 { "short" } else { "long" })),
                        ("symbol", s(symbol)),
                    ])
                })
                .collect(),
        )
    }

    /// The fills after the one `page_token` names, oldest first; all of them for an empty token.
    fn activities(&self, page_token: &str) -> Value {
        let books = self.books.borrow();
        let after = books
            .fills
            .iter()
            .position(|fill| fill.id == page_token)
            .map_or(0, |at| at.saturating_add(1));
        Value::Array(
            books
                .fills
                .iter()
                .skip(after)
                .map(|fill| {
                    object(vec![
                        ("activity_type", s("FILL")),
                        ("cum_qty", s(&fill.qty)),
                        ("id", s(&fill.id)),
                        ("leaves_qty", s("0")),
                        ("order_id", s(&fill.order_id)),
                        ("price", s(&fill.price)),
                        ("qty", s(&fill.qty)),
                        ("side", s(&fill.side)),
                        ("symbol", s(&fill.symbol)),
                        ("transaction_time", s(&self.at)),
                        ("type", s("fill")),
                    ])
                })
                .collect(),
        )
    }
}

impl TradingTransport for FixturePaper {
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        Ok(self.answer(request))
    }
}

impl DataTransport for FixturePaper {
    async fn send(&self, request: &QuoteRequest) -> Result<Response, TransportError> {
        Ok(self.quote(request.path_and_query()))
    }

    async fn send_bars(&self, _request: &BarsRequest) -> Result<Response, TransportError> {
        Ok(refusal(404, 40_410_000, "not found"))
    }
}

impl FixturePaper {
    /// The latest IEX quote of one of [`ASSETS`], stamped by the quote clock; `404` without a
    /// clock, for another symbol, or for any other read.
    fn quote(&self, path_and_query: &str) -> Response {
        let asset = path_and_query
            .strip_prefix("/v2/stocks/")
            .and_then(|rest| rest.strip_suffix("/quotes/latest?feed=iex"))
            .and_then(listed);
        match (asset, &self.quotes) {
            (Some((_, asset)), Some(QuoteClock(clock))) => {
                let quote = format!(
                    r#"{{"quote":{{"ap":{},"as":100,"ax":"V","bp":{},"bs":100,"bx":"V","c":["R"],"t":"{}","z":"B"}},"symbol":"{}"}}"#,
                    asset.ask,
                    asset.bid,
                    clock(),
                    asset.symbol
                );
                Response {
                    status: 200,
                    body: quote.into_bytes(),
                }
            }
            _ => refusal(404, 40_410_000, "not found"),
        }
    }
}

/// One of [`ASSETS`] as Alpaca's asset record: a US equity, active, tradable and fractionable,
/// with an empty `attributes` list, so neither an IPO nor a PTP. Its id is the asset's place in
/// the list in UUID form, apart from the order ids' counter by its high digit.
fn asset_json(index: usize, asset: FixtureAsset) -> Value {
    let id = format!(
        "a0000000-0000-4000-8000-{:012}",
        u64::try_from(index).unwrap_or(u64::MAX)
    );
    object(vec![
        ("attributes", Value::Array(Vec::new())),
        ("class", s("us_equity")),
        ("easy_to_borrow", Value::Bool(true)),
        ("exchange", s(asset.exchange)),
        ("fractionable", Value::Bool(true)),
        ("id", s(&id)),
        ("maintenance_margin_requirement", s("30")),
        ("marginable", Value::Bool(true)),
        ("name", s(asset.symbol)),
        ("shortable", Value::Bool(true)),
        ("status", s("active")),
        ("symbol", s(asset.symbol)),
        ("tradable", Value::Bool(true)),
    ])
}

/// Whether an order may still work: accepted and resting, or a leg held until its entry fills.
fn open(order: &Order) -> bool {
    matches!(order.status.as_str(), "new" | "held")
}

/// The average price of what `holding` holds, at [`SCALE`].
fn average(holding: Holding) -> Option<i128> {
    holding.cost.checked_mul(SCALE)?.checked_div(holding.qty)
}

/// `text`, a decimal of at most nine places, as a whole number of 10⁻⁹.
fn scaled(text: &str) -> Option<i128> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, places) = digits.split_once('.').unwrap_or((digits, ""));
    if whole.is_empty() || places.len() > 9 {
        return None;
    }
    let padded = format!("{places:0<9}");
    let value = whole
        .parse::<i128>()
        .ok()?
        .checked_mul(SCALE)?
        .checked_add(padded.parse::<i128>().ok()?)?;
    if negative {
        value.checked_neg()
    } else {
        Some(value)
    }
}

/// A whole number of 10⁻⁹ as canonical decimal text: no trailing zeros and no bare point.
fn decimal(value: i128) -> String {
    let sign = if value < 0 { "-" } else { "" };
    let magnitude = value.unsigned_abs();
    let unit = SCALE.unsigned_abs();
    let whole = magnitude.checked_div(unit).unwrap_or_default();
    let places = format!("{:09}", magnitude.checked_rem(unit).unwrap_or_default());
    let places = places.trim_end_matches('0');
    if places.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{places}")
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

/// `group`'s first order, with the rest nested as its `legs`, as `nested=true` lists it.
fn nested(group: &[Order], at: &str) -> Value {
    let Some((first, legs)) = group.split_first() else {
        return Value::Null;
    };
    let legs = if legs.is_empty() {
        Value::Null
    } else {
        Value::Array(
            legs.iter()
                .map(|leg| order_json(leg, Value::Null, at))
                .collect(),
        )
    };
    order_json(first, legs, at)
}

fn order_json(order: &Order, legs: Value, at: &str) -> Value {
    object(vec![
        ("asset_class", s("us_equity")),
        ("client_order_id", s(&order.client_order_id)),
        ("created_at", s(at)),
        ("extended_hours", Value::Bool(false)),
        (
            "filled_avg_price",
            optional(order.filled_avg_price.as_ref()),
        ),
        ("filled_qty", s(&order.filled_qty)),
        ("id", s(&order.id)),
        ("legs", legs),
        ("limit_price", optional(order.limit_price.as_ref())),
        ("order_class", s(&order.order_class)),
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

#[cfg(test)]
mod tests {
    use mandate_alpaca::{HttpRequest, Method, Response};
    use mandate_canon::{Value, parse};
    use mandate_time::UtcNanos;

    use super::{FixturePaper, decimal, scaled};

    type Error = Box<dyn std::error::Error>;

    fn answer(
        paper: &FixturePaper,
        method: Method,
        path: &str,
        body: Option<&str>,
    ) -> Result<Value, Error> {
        let request = HttpRequest::new(method, path, body.map(str::to_owned))?;
        Ok(parse(&paper.answer(&request).body)?)
    }

    fn text<'a>(value: &'a Value, member: &str) -> Option<&'a str> {
        value.get(member).and_then(Value::as_str)
    }

    fn open(paper: &FixturePaper) -> Result<Vec<Value>, Error> {
        let listed = answer(
            paper,
            Method::Get,
            "/v2/orders?status=open&limit=500&direction=asc&nested=true",
            None,
        )?;
        Ok(listed.as_array().ok_or("an array")?.to_vec())
    }

    fn legs(parent: &Value) -> Result<Vec<Value>, Error> {
        Ok(parent
            .get("legs")
            .and_then(Value::as_array)
            .ok_or("legs")?
            .to_vec())
    }

    const BUY: &str = r#"{"client_order_id":"md-01JINTENT0000000000000000A","limit_price":"150","order_class":"simple","qty":"10","side":"buy","symbol":"AAPL","time_in_force":"day","type":"limit"}"#;
    const OCO: &str = r#"{"client_order_id":"md-01JINTENT0000000000000000A-p1","order_class":"oco","qty":"10","side":"sell","stop_loss":{"stop_price":"147"},"symbol":"AAPL","take_profit":{"limit_price":"157.5"},"time_in_force":"gtc","type":"limit"}"#;
    /// [`OCO`] with a top-level `limit_price` and a `type` of its own, which neither leg takes.
    const OCO_WITH_TOP_LEVEL_PRICE: &str = r#"{"client_order_id":"md-01JINTENT0000000000000000A-p1","limit_price":"150","order_class":"oco","qty":"10","side":"sell","stop_loss":{"stop_price":"147"},"symbol":"AAPL","take_profit":{"limit_price":"157.5"},"time_in_force":"gtc","type":"stop_limit"}"#;
    const BRACKET: &str = r#"{"client_order_id":"md-01JINTENT0000000000000000B","order_class":"bracket","qty":"10","side":"buy","stop_loss":{"stop_price":"147"},"symbol":"AAPL","take_profit":{"limit_price":"157.5"},"time_in_force":"day","type":"market"}"#;

    /// An OCO is listed as Alpaca lists it: its take-profit limit first, under the client order id
    /// it was sent with, and its stop nested under it as a leg with ids of its own.
    #[test]
    fn an_oco_is_its_limit_with_its_stop_nested() -> Result<(), Error> {
        for oco in [OCO, OCO_WITH_TOP_LEVEL_PRICE] {
            an_oco_nests(oco)?;
        }
        Ok(())
    }

    /// [`an_oco_is_its_limit_with_its_stop_nested`] for one request `oco`: its legs come from
    /// `take_profit` and `stop_loss` alone, whatever its own `type` and `limit_price` say.
    fn an_oco_nests(oco: &str) -> Result<(), Error> {
        let paper = FixturePaper::filling(UtcNanos::parse("2026-09-21T15:00:00.000000000Z")?);
        answer(&paper, Method::Post, "/v2/orders", Some(BUY))?;
        answer(&paper, Method::Post, "/v2/orders", Some(oco))?;
        let listed = open(&paper)?;
        let [parent] = listed.as_slice() else {
            return Err(format!("one OCO open: {listed:?}").into());
        };
        assert_eq!(
            text(parent, "client_order_id"),
            Some("md-01JINTENT0000000000000000A-p1")
        );
        assert_eq!(text(parent, "order_class"), Some("oco"));
        assert_eq!(text(parent, "type"), Some("limit"));
        assert_eq!(text(parent, "limit_price"), Some("157.5"));
        assert_eq!(parent.get("stop_price"), Some(&Value::Null));
        let legs = legs(parent)?;
        let [stop] = legs.as_slice() else {
            return Err(format!("one stop leg: {legs:?}").into());
        };
        assert_eq!(text(stop, "type"), Some("stop"));
        assert_eq!(text(stop, "stop_price"), Some("147"));
        assert_eq!(stop.get("limit_price"), Some(&Value::Null));
        assert_eq!(text(stop, "status"), Some("new"));
        assert_ne!(text(stop, "id"), text(parent, "id"));
        assert_ne!(
            text(stop, "client_order_id"),
            text(parent, "client_order_id")
        );
        Ok(())
    }

    /// A bracket whose entry has not filled holds its two legs: a sell limit at the take-profit
    /// and a sell stop at the stop, each `held`, each with ids of its own.
    #[test]
    fn an_unfilled_brackets_legs_are_held() -> Result<(), Error> {
        let paper = FixturePaper::filling(UtcNanos::parse("2026-09-21T15:00:00.000000000Z")?);
        answer(&paper, Method::Post, "/v2/orders", Some(BRACKET))?;
        let listed = open(&paper)?;
        let [parent] = listed.as_slice() else {
            return Err(format!("one bracket open: {listed:?}").into());
        };
        assert_eq!(
            text(parent, "status"),
            Some("new"),
            "a market entry rests here"
        );
        let legs = legs(parent)?;
        let [take, stop] = legs.as_slice() else {
            return Err(format!("two legs: {legs:?}").into());
        };
        assert_eq!(
            (text(take, "type"), text(take, "limit_price")),
            (Some("limit"), Some("157.5"))
        );
        assert_eq!(
            (text(stop, "type"), text(stop, "stop_price")),
            (Some("stop"), Some("147"))
        );
        for leg in [take, stop] {
            assert_eq!(
                (text(leg, "side"), text(leg, "status")),
                (Some("sell"), Some("held"))
            );
            assert_ne!(text(leg, "id"), text(parent, "id"));
            assert_ne!(
                text(leg, "client_order_id"),
                text(parent, "client_order_id")
            );
        }
        assert_ne!(text(take, "id"), text(stop, "id"));
        assert_ne!(text(take, "client_order_id"), text(stop, "client_order_id"));
        Ok(())
    }

    /// A read of an order by the id it gave answers that order with its legs nested, and a read
    /// by an id it never gave is `404`, with an order on the book. No other path reaches it:
    /// `HttpRequest` exists only for the table's endpoints.
    #[test]
    fn an_order_it_never_gave_is_not_found() -> Result<(), Error> {
        let paper = FixturePaper::filling(UtcNanos::parse("2026-09-21T15:00:00.000000000Z")?);
        answer(&paper, Method::Post, "/v2/orders", Some(BRACKET))?;
        let listed = open(&paper)?;
        let [parent] = listed.as_slice() else {
            return Err(format!("one bracket open: {listed:?}").into());
        };
        let id = text(parent, "id").ok_or("an id")?;
        let read = |path: &str| -> Result<Response, Error> {
            Ok(paper.answer(&HttpRequest::new(Method::Get, path, None)?))
        };
        let found = read(&format!("/v2/orders/{id}"))?;
        assert_eq!(found.status, 200);
        let order: Value = parse(&found.body)?;
        assert_eq!(text(&order, "id"), Some(id));
        assert_eq!(legs(&order)?.len(), 2, "its legs nested: {order:?}");
        assert_eq!(
            read("/v2/orders/00000000-0000-4000-8000-000000000009")?.status,
            404,
            "an id it never gave, beside one it did"
        );
        Ok(())
    }

    /// A quoting fixture prints its clock by name only: the closure has no text of its own, and
    /// the fixture's `Debug` names what it holds.
    #[test]
    fn a_quoting_fixture_prints_its_clock_by_name() {
        let quoting = FixturePaper::new(UtcNanos::EPOCH).quoting(|| UtcNanos::EPOCH);
        let printed = format!("{quoting:?}");
        assert!(printed.contains("quotes: Some(QuoteClock)"), "{printed}");
        assert!(format!("{:?}", FixturePaper::new(UtcNanos::EPOCH)).contains("quotes: None"));
    }

    /// Decimal text to the fixture's scale and back: nine places at most, a whole part always,
    /// and a sign only below zero.
    #[test]
    fn decimals_read_and_write_at_nine_places() {
        assert_eq!(scaled("1.123456789"), Some(1_123_456_789));
        assert_eq!(scaled("1.1234567891"), None);
        assert_eq!(scaled(".5"), None);
        assert_eq!(scaled("-1.5"), Some(-1_500_000_000));
        assert_eq!(decimal(0), "0");
        assert_eq!(decimal(-1_500_000_000), "-1.5");
        assert_eq!(decimal(1_500_000_000), "1.5");
    }
}
