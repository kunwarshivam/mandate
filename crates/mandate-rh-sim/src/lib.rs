#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The simulated Robinhood broker (E7-25, [DEC-124]; [DEC-441] items 11 and 16): the equity order
//! rules of Robinhood's published tool contract
//! ([robinhood-contract.md](../../../docs/project/tasks/robinhood-contract.md)) as a pure,
//! deterministic core. No agent ever calls Robinhood, so every Robinhood test runs against this.
//!
//! [`Sim`] reads no clock, no file and no randomness. Everything the real broker would decide on
//! its own arrives as a scripted input: an [`Event`] (the session, quotes, halts, faults on the
//! next place, the end of the trading day), a fill ([`Sim::fill`]), or another broker-side state
//! change ([`Sim::advance`]). Prices and quantities arrive as decimal strings and are
//! read only by `mandate-num`, never through a float (ES-23); text that is not canonical decimal
//! text is refused, which is stricter than the contract states.
//!
//! Where the contract is silent the simulator takes the reading that adds no risk, and says so:
//!
//! - a `ref_id` is deduplicated per account, and a re-send returns the first order whatever it
//!   says, unless [`Event::RefuseChangedResend`] makes a changed re-send an error;
//! - an order record does not echo its `ref_id` unless [`Event::EchoRefId`] turns that on, so a
//!   recovery that matches on it cannot pass here and fail against the real broker;
//! - buying power and cash are scripted figures that orders and fills do not move;
//! - a position carries the average price its buys were filled at, rounded to a price's 9
//!   places, and a sell does not move it; `get_equity_positions` serves every row, a zero
//!   quantity included;
//! - a sell is refused when it is larger than the position less every working sell, because the
//!   agentic account cannot sell short and two working sells must not oversell it;
//! - `place` refuses on any alert `review` would raise;
//! - a fill comes only to a `confirmed` or `partially_filled` order;
//! - the end of the trading day cancels every working `gfd` order, `queued` ones included;
//! - every other state change follows one lifecycle table: `new` to `queued`, `confirmed`,
//!   `unconfirmed`, `cancelled`, `rejected` or `failed`; `queued` to the same but `queued`;
//!   `unconfirmed` to `confirmed`, `cancelled`, `rejected` or `failed`; `confirmed` to
//!   `cancelled`, `voided` or `failed`; `partially_filled` to `cancelled` or `voided`; and fills
//!   alone reach `partially_filled` and `filled`.
//!
//! [`SimServer`] serves the core over loopback MCP (S2), honest or as a hostile [`Variant`].
//!
//! [DEC-124]: ../../../docs/project/04-decision-log.md
//! [DEC-441]: ../../../docs/project/decisions/DEC-441.md

use std::collections::{BTreeMap, BTreeSet, VecDeque};

mod server;

pub use server::{CONTRACT, Garble, INJECTION, ServerError, SimServer, Variant};

use mandate_num::{Price, Qty, Rounding, ShareIncrement, Usd};

/// The ten values of `get_equity_orders`'s `state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum State {
    New,
    Queued,
    Confirmed,
    Unconfirmed,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
    Failed,
    Voided,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Side {
    Buy,
    Sell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderType {
    Market,
    Limit,
    StopMarket,
    StopLimit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeInForce {
    Gfd,
    Gtc,
}

/// `market_hours`: `regular_hours` fills in the regular session only, `extended_hours` also in
/// the pre-market and after-hours sessions, and `all_day_hours` also overnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketHours {
    Regular,
    Extended,
    AllDay,
}

/// The session the market is in, set by the test.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Session {
    Closed,
    Overnight,
    Extended,
    Regular,
}

/// A pre-trade alert `review_equity_order` returns; any alert also refuses the place.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Alert {
    BuyingPower,
    PatternDayTrading,
    Halt,
}

/// What the next place call does instead of answering normally: answer with one of the states a
/// fresh order may start in (`new`, `unconfirmed`, `rejected`, `failed`), or create the order and
/// lose the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    Answer(State),
    LoseAnswer,
}

/// A scripted input: the session moves (a queued order whose market hours admit the new session
/// is confirmed), a quote or a halt, a fault for the next place that creates an order, or the end
/// of the trading day, which cancels every working `gfd` order and closes the market. Two switches,
/// both off by default, change readings the contract leaves open: whether an order record echoes
/// its `ref_id`, and whether a re-send of a `ref_id` with a different body is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Session(Session),
    Quote(String, Price),
    Halt(String),
    Script(Fault),
    EndOfDay,
    EchoRefId(bool),
    RefuseChangedResend(bool),
}

/// A customer account. `pattern_day_trader` marks an account a day trade would restrict, so an
/// order against today's fills in its symbol draws the pattern day trading alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub number: String,
    pub agentic_allowed: bool,
    /// The account's settled cash, which `get_portfolio` serves (DEC-902 item 2). A scripted
    /// figure like `buying_power`: orders and fills do not move it.
    pub cash: Usd,
    pub buying_power: Usd,
    pub pattern_day_trader: bool,
}

/// `place_equity_order`'s parameters as text, as they arrive over MCP; `review_equity_order`
/// takes the same and ignores `ref_id`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OrderRequest {
    pub account_number: String,
    pub symbol: String,
    pub side: String,
    pub order_type: String,
    pub quantity: Option<String>,
    pub dollar_amount: Option<String>,
    pub limit_price: Option<String>,
    pub stop_price: Option<String>,
    pub time_in_force: Option<String>,
    pub market_hours: Option<String>,
    pub ref_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Execution {
    pub quantity: Qty,
    pub price: Price,
}

/// An order as `get_equity_orders` returns it. A `dollar_amount` order holds the fractional
/// quantity the amount buys at the quote when it was placed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Order {
    pub id: String,
    pub account_number: String,
    /// `None` unless [`Event::EchoRefId`] is on: the contract does not say that `orders[]`
    /// carries it.
    pub ref_id: Option<String>,
    pub symbol: String,
    pub side: Side,
    pub order_type: OrderType,
    pub quantity: Qty,
    pub limit_price: Option<Price>,
    pub stop_price: Option<Price>,
    pub time_in_force: TimeInForce,
    pub market_hours: MarketHours,
    pub state: State,
    pub filled_quantity: Qty,
    pub executions: Vec<Execution>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Review {
    pub quote: Option<Price>,
    pub alerts: Vec<Alert>,
}

/// One position row `get_equity_positions` serves (DEC-902 item 3): the account, the symbol,
/// the quantity held (signed decimal text on the wire), and the average price it was bought
/// at, which a sell does not move and zero quantity keeps. The connector's preflight reads
/// every row, a zero quantity included.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub account_number: String,
    pub symbol: String,
    pub quantity: Qty,
    pub average_buy_price: Price,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    #[error("two accounts share a number")]
    DuplicateAccount,
    #[error("no such account")]
    UnknownAccount,
    #[error("the account does not allow agentic trading")]
    NotAgentic,
    #[error("`{0}` is missing, unreadable, or not allowed with the other parameters")]
    Unreadable(&'static str),
    #[error("a fractional quantity or a dollar amount needs a market order in regular hours")]
    QuantityForm,
    #[error("outside regular hours only a limit order is allowed")]
    SessionNeedsLimit,
    #[error("no quote for the symbol")]
    NoQuote,
    #[error("refused by a pre-trade alert: {0:?}")]
    Alert(Alert),
    #[error("the sell is larger than the position")]
    InsufficientShares,
    #[error("no such order")]
    UnknownOrder,
    #[error("the order is already filled, cancelled, rejected, failed or voided")]
    Terminal,
    #[error("the order cannot move to that state")]
    IllegalTransition,
    #[error("the order is not working")]
    NotWorking,
    #[error("the session does not admit the order's market hours")]
    OutsideSession,
    #[error("a fill must be positive and at most the unfilled quantity")]
    Overfill,
    #[error("the fill price is through the order's limit")]
    ThroughLimit,
    #[error("the order was created, and its answer was lost")]
    AnswerLost,
    #[error("the ref_id was sent before with a different order")]
    ChangedResend,
}

/// One account's holding of a symbol: its quantity, and the average price it was bought at,
/// which a sell does not move and zero quantity keeps, so `get_equity_positions` can serve
/// every row the broker holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Held {
    quantity: Qty,
    average: Price,
}

/// The simulated broker: its accounts, orders, positions, and the scripted market.
#[derive(Debug, Clone)]
pub struct Sim {
    accounts: Vec<Account>,
    orders: Vec<Order>,
    session: Session,
    quotes: BTreeMap<String, Price>,
    halted: BTreeSet<String>,
    faults: VecDeque<Fault>,
    positions: BTreeMap<(String, String), Held>,
    traded_today: BTreeSet<(String, String, Side)>,
    requests: Vec<OrderRequest>,
    server_keys: u64,
    echo_ref_id: bool,
    refuse_changed_resend: bool,
}

impl Sim {
    /// A simulator over these accounts, in the closed session, with no quotes, halts or orders.
    pub fn new(accounts: Vec<Account>) -> Result<Self, SimError> {
        let numbers: BTreeSet<&str> = accounts.iter().map(|a| a.number.as_str()).collect();
        if numbers.len() != accounts.len() {
            return Err(SimError::DuplicateAccount);
        }
        Ok(Self {
            accounts,
            orders: Vec::new(),
            session: Session::Closed,
            quotes: BTreeMap::new(),
            halted: BTreeSet::new(),
            faults: VecDeque::new(),
            positions: BTreeMap::new(),
            traded_today: BTreeSet::new(),
            requests: Vec::new(),
            server_keys: 0,
            echo_ref_id: false,
            refuse_changed_resend: false,
        })
    }

    /// Applies one scripted input of the market or the broker.
    pub fn apply(&mut self, event: Event) -> Result<(), SimError> {
        match event {
            Event::Session(session) => {
                self.session = session;
                for order in &mut self.orders {
                    if order.state == State::Queued && admits(order.market_hours, session) {
                        order.state = State::Confirmed;
                    }
                }
            }
            Event::Quote(symbol, price) => {
                self.quotes.insert(symbol, price);
            }
            Event::Halt(symbol) => {
                self.halted.insert(symbol);
            }
            Event::Script(Fault::Answer(state)) if !is_initial(state) => {
                return Err(SimError::IllegalTransition);
            }
            Event::Script(fault) => self.faults.push_back(fault),
            Event::EndOfDay => {
                for order in &mut self.orders {
                    if order.time_in_force == TimeInForce::Gfd && !is_terminal(order.state) {
                        order.state = State::Cancelled;
                    }
                }
                self.traded_today.clear();
                self.session = Session::Closed;
            }
            Event::EchoRefId(on) => self.echo_ref_id = on,
            Event::RefuseChangedResend(on) => self.refuse_changed_resend = on,
        }
        Ok(())
    }

    /// `review_equity_order`: the place's checks and alerts, with nothing placed.
    pub fn review(&self, request: &OrderRequest) -> Result<Review, SimError> {
        let account = self.agentic(&request.account_number)?;
        let (_, alerts) = self.draft(account, request)?;
        let quote = self.quotes.get(&request.symbol).copied();
        Ok(Review { quote, alerts })
    }

    /// `place_equity_order`. A `ref_id` this account has used returns that first order unchanged.
    pub fn place(&mut self, request: &OrderRequest) -> Result<Order, SimError> {
        let account = self.agentic(&request.account_number)?;
        let ref_id = match &request.ref_id {
            Some(ref_id) if is_uuid(ref_id) => ref_id.clone(),
            Some(_) => return Err(SimError::Unreadable("ref_id")),
            None => format!("rh-sim-server-key-{}", self.server_keys),
        };
        let first = self.orders.iter().zip(&self.requests).find(|(o, _)| {
            o.account_number == account.number && o.ref_id.as_deref() == Some(ref_id.as_str())
        });
        if let Some((first, sent)) = first {
            if self.refuse_changed_resend && sent != request {
                return Err(SimError::ChangedResend);
            }
            return Ok(shown(first.clone(), self.echo_ref_id));
        }
        let (draft, alerts) = self.draft(account, request)?;
        if let Some(alert) = alerts.first() {
            return Err(SimError::Alert(*alert));
        }
        self.server_keys = self.server_keys.saturating_add(1);
        let fault = self.faults.pop_front();
        let state = match fault {
            Some(Fault::Answer(state)) => state,
            _ if admits(draft.market_hours, self.session) => State::Confirmed,
            _ => State::Queued,
        };
        let id = format!("rh-sim-{:06}", self.orders.len().saturating_add(1));
        let order = Order {
            id,
            ref_id: Some(ref_id),
            state,
            ..draft
        };
        self.orders.push(order.clone());
        self.requests.push(request.clone());
        match fault {
            Some(Fault::LoseAnswer) => Err(SimError::AnswerLost),
            _ => Ok(shown(order, self.echo_ref_id)),
        }
    }

    /// `cancel_equity_order`: refused for a terminal order or another account's.
    pub fn cancel(&mut self, account_number: &str, order_id: &str) -> Result<Order, SimError> {
        let echo = self.echo_ref_id;
        let order = self
            .orders
            .iter_mut()
            .find(|o| o.id == order_id && o.account_number == account_number)
            .ok_or(SimError::UnknownOrder)?;
        if is_terminal(order.state) {
            return Err(SimError::Terminal);
        }
        order.state = State::Cancelled;
        Ok(shown(order.clone(), echo))
    }

    /// A scripted execution of a working order in a session its market hours admit.
    pub fn fill(&mut self, order_id: &str, quantity: Qty, price: Price) -> Result<Order, SimError> {
        let session = self.session;
        let echo = self.echo_ref_id;
        let order = self
            .orders
            .iter_mut()
            .find(|o| o.id == order_id)
            .ok_or(SimError::UnknownOrder)?;
        if is_terminal(order.state) {
            return Err(SimError::Terminal);
        }
        if !matches!(order.state, State::Confirmed | State::PartiallyFilled) {
            return Err(SimError::NotWorking);
        }
        if !admits(order.market_hours, session) {
            return Err(SimError::OutsideSession);
        }
        let filled = order
            .filled_quantity
            .checked_add(quantity)
            .map_err(|_| SimError::Overfill)?;
        if quantity.is_zero() || filled > order.quantity {
            return Err(SimError::Overfill);
        }
        let through = match (order.side, order.limit_price) {
            (Side::Buy, Some(limit)) => price > limit,
            (Side::Sell, Some(limit)) => price < limit,
            (_, None) => false,
        };
        if through {
            return Err(SimError::ThroughLimit);
        }
        let key = (order.account_number.clone(), order.symbol.clone());
        let held = self.positions.get(&key).copied();
        let held_quantity = held.map_or(Qty::ZERO, |position| position.quantity);
        let position = match order.side {
            Side::Buy => held_quantity
                .checked_add(quantity)
                .map_err(|_| SimError::Overfill)?,
            Side::Sell => held_quantity
                .checked_sub(quantity)
                .map_err(|_| SimError::InsufficientShares)?,
        };
        let average = match held {
            None => price,
            Some(previous) if order.side == Side::Buy => reaverage(previous, quantity, price)?,
            Some(previous) => previous.average,
        };
        self.positions.insert(
            key.clone(),
            Held {
                quantity: position,
                average,
            },
        );
        self.traded_today.insert((key.0, key.1, order.side));
        order.filled_quantity = filled;
        order.executions.push(Execution { quantity, price });
        order.state = if filled == order.quantity {
            State::Filled
        } else {
            State::PartiallyFilled
        };
        Ok(shown(order.clone(), echo))
    }

    /// A scripted broker-side state change other than a fill.
    pub fn advance(&mut self, order_id: &str, to: State) -> Result<Order, SimError> {
        let echo = self.echo_ref_id;
        let order = self
            .orders
            .iter_mut()
            .find(|o| o.id == order_id)
            .ok_or(SimError::UnknownOrder)?;
        may_advance(order.state, to)?;
        order.state = to;
        Ok(shown(order.clone(), echo))
    }

    /// `get_equity_orders` for one account, newest first.
    pub fn orders(&self, account_number: &str) -> Result<Vec<Order>, SimError> {
        self.account(account_number)?;
        let mine = self
            .orders
            .iter()
            .filter(|o| o.account_number == account_number);
        Ok(mine
            .rev()
            .map(|o| shown(o.clone(), self.echo_ref_id))
            .collect())
    }

    /// `get_accounts`'s records: every account of the customer as it was configured, agentic
    /// flags included (the connector checks them before it reads anything else).
    pub fn accounts(&self) -> &[Account] {
        &self.accounts
    }

    /// `get_portfolio` (DEC-902 item 2): the account's cash and its buying power, both scripted
    /// figures that orders and fills do not move.
    pub fn portfolio(&self, account_number: &str) -> Result<(Usd, Usd), SimError> {
        let account = self.account(account_number)?;
        Ok((account.cash, account.buying_power))
    }

    /// `get_equity_positions` (DEC-902 item 3): every row the account holds, a zero quantity
    /// included, in symbol order. None is merged or dropped.
    pub fn equity_positions(&self, account_number: &str) -> Result<Vec<Position>, SimError> {
        self.account(account_number)?;
        Ok(self
            .positions
            .iter()
            .filter(|((account, _), _)| account == account_number)
            .map(|((account, symbol), held)| Position {
                account_number: account.clone(),
                symbol: symbol.clone(),
                quantity: held.quantity,
                average_buy_price: held.average,
            })
            .collect())
    }

    /// `get_equity_tradability` (DEC-902 item 6): the account may trade the symbol unless the
    /// market holds it halted.
    pub fn tradability(&self, account_number: &str, symbol: &str) -> Result<bool, SimError> {
        self.account(account_number)?;
        Ok(!self.halted.contains(symbol))
    }

    /// `get_equity_quotes`: each asked symbol's quote, locked at the price the market scripted
    /// (DEC-902 item 7 reads a locked quote as uncrossed), in the order asked. A symbol with
    /// no scripted quote refuses, rather than answering a guess.
    pub fn equity_quotes(&self, symbols: &[String]) -> Result<Vec<(String, Price)>, SimError> {
        symbols
            .iter()
            .map(|symbol| {
                let price = self.quotes.get(symbol).copied().ok_or(SimError::NoQuote)?;
                Ok((symbol.clone(), price))
            })
            .collect()
    }

    pub fn position(&self, account_number: &str, symbol: &str) -> Result<Qty, SimError> {
        self.account(account_number)?;
        let key = (account_number.to_owned(), symbol.to_owned());
        Ok(self
            .positions
            .get(&key)
            .map_or(Qty::ZERO, |held| held.quantity))
    }

    fn account(&self, number: &str) -> Result<&Account, SimError> {
        self.accounts
            .iter()
            .find(|a| a.number == number)
            .ok_or(SimError::UnknownAccount)
    }

    fn agentic(&self, number: &str) -> Result<Account, SimError> {
        let account = self.account(number)?;
        if !account.agentic_allowed {
            return Err(SimError::NotAgentic);
        }
        Ok(account.clone())
    }

    /// The contract's checks, in order: the order the request would place, not yet numbered,
    /// and the alerts it would draw.
    fn draft(&self, account: Account, r: &OrderRequest) -> Result<(Order, Vec<Alert>), SimError> {
        let side = match r.side.as_str() {
            "buy" => Side::Buy,
            "sell" => Side::Sell,
            _ => return Err(SimError::Unreadable("side")),
        };
        let order_type = match r.order_type.as_str() {
            "market" => OrderType::Market,
            "limit" => OrderType::Limit,
            "stop_market" => OrderType::StopMarket,
            "stop_limit" => OrderType::StopLimit,
            _ => return Err(SimError::Unreadable("type")),
        };
        let time_in_force = match r.time_in_force.as_deref() {
            None | Some("gfd") => TimeInForce::Gfd,
            Some("gtc") => TimeInForce::Gtc,
            Some(_) => return Err(SimError::Unreadable("time_in_force")),
        };
        let market_hours = match r.market_hours.as_deref() {
            None | Some("regular_hours") => MarketHours::Regular,
            Some("extended_hours") => MarketHours::Extended,
            Some("all_day_hours") => MarketHours::AllDay,
            Some(_) => return Err(SimError::Unreadable("market_hours")),
        };
        let limited = matches!(order_type, OrderType::Limit | OrderType::StopLimit);
        let stopped = matches!(order_type, OrderType::StopMarket | OrderType::StopLimit);
        let limit_price = price_field(r.limit_price.as_deref(), limited, "limit_price")?;
        let stop_price = price_field(r.stop_price.as_deref(), stopped, "stop_price")?;
        if market_hours != MarketHours::Regular && order_type != OrderType::Limit {
            return Err(SimError::SessionNeedsLimit);
        }
        let quote = self.quotes.get(&r.symbol).copied();
        let quantity = match (r.quantity.as_deref(), r.dollar_amount.as_deref()) {
            (Some(text), None) => {
                let quantity = Qty::parse(text).map_err(|_| SimError::Unreadable("quantity"))?;
                if quantity.is_zero() {
                    return Err(SimError::Unreadable("quantity"));
                }
                if order_type != OrderType::Market && quantity.to_string().contains('.') {
                    return Err(SimError::QuantityForm);
                }
                quantity
            }
            (None, Some(text)) => {
                if order_type != OrderType::Market {
                    return Err(SimError::QuantityForm);
                }
                let dollars =
                    Usd::parse(text).map_err(|_| SimError::Unreadable("dollar_amount"))?;
                let quote = quote.ok_or(SimError::NoQuote)?;
                let quantity = dollars
                    .shares_at(quote, ShareIncrement::Fractional)
                    .map_err(|_| SimError::Unreadable("dollar_amount"))?;
                if quantity.is_zero() {
                    return Err(SimError::Unreadable("dollar_amount"));
                }
                quantity
            }
            _ => return Err(SimError::Unreadable("quantity")),
        };
        let reference = match order_type {
            OrderType::Market => quote.ok_or(SimError::NoQuote)?,
            OrderType::StopMarket => stop_price.ok_or(SimError::Unreadable("stop_price"))?,
            OrderType::Limit | OrderType::StopLimit => {
                limit_price.ok_or(SimError::Unreadable("limit_price"))?
            }
        };
        let key = (account.number.clone(), r.symbol.clone());
        let held = self
            .positions
            .get(&key)
            .map_or(Qty::ZERO, |position| position.quantity);
        let working_sells = self
            .orders
            .iter()
            .filter(|o| o.account_number == account.number && o.symbol == r.symbol)
            .filter(|o| o.side == Side::Sell && !is_terminal(o.state))
            .try_fold(Qty::ZERO, |sum, o| {
                o.quantity
                    .checked_sub(o.filled_quantity)
                    .and_then(|left| sum.checked_add(left))
            })
            .map_err(|_| SimError::InsufficientShares)?;
        let unsold = held.checked_sub(working_sells).unwrap_or(Qty::ZERO);
        if side == Side::Sell && quantity > unsold {
            return Err(SimError::InsufficientShares);
        }
        let cost = quantity
            .notional(reference)
            .map_err(|_| SimError::Alert(Alert::BuyingPower))?;
        let opposite = match side {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        };
        let mut alerts = Vec::new();
        if side == Side::Buy && cost > account.buying_power {
            alerts.push(Alert::BuyingPower);
        }
        if account.pattern_day_trader && self.traded_today.contains(&(key.0, key.1, opposite)) {
            alerts.push(Alert::PatternDayTrading);
        }
        if self.halted.contains(&r.symbol) {
            alerts.push(Alert::Halt);
        }
        let order = Order {
            id: String::new(),
            account_number: account.number,
            ref_id: None,
            symbol: r.symbol.clone(),
            side,
            order_type,
            quantity,
            limit_price,
            stop_price,
            time_in_force,
            market_hours,
            state: State::New,
            filled_quantity: Qty::ZERO,
            executions: Vec::new(),
        };
        Ok((order, alerts))
    }
}

/// A price the order type needs (present, positive, canonical) or must not carry.
fn price_field(
    text: Option<&str>,
    needed: bool,
    field: &'static str,
) -> Result<Option<Price>, SimError> {
    match (text, needed) {
        (Some(text), true) => Price::parse(text)
            .map(Some)
            .map_err(|_| SimError::Unreadable(field)),
        (None, false) => Ok(None),
        _ => Err(SimError::Unreadable(field)),
    }
}

/// The volume-weighted average of a holding bought again (DEC-902 item 3): the exact quotient
/// `(held × old + added × at) ÷ (held + added)`, rounded to a price's 9 places, half to even,
/// because the decimal types offer no other exact division that reaches a [`Price`]. The
/// number passes through the report quotient with each term's numeric value: `Qty::notional`
/// at one, the price's own scale.
fn reaverage(held: Held, added: Qty, at: Price) -> Result<Price, SimError> {
    let unrecordable = |_| SimError::Unreadable("average_buy_price");
    let one = Price::parse("1").map_err(unrecordable)?;
    let cost = held
        .quantity
        .notional(held.average)
        .map_err(unrecordable)?
        .checked_add(added.notional(at).map_err(unrecordable)?)
        .map_err(unrecordable)?;
    let number = held
        .quantity
        .checked_add(added)
        .map_err(unrecordable)?
        .notional(one)
        .map_err(unrecordable)?;
    let average = cost
        .ratio_to(number, 9, Rounding::HalfEven)
        .map_err(unrecordable)?
        .to_string();
    Price::parse(&average).map_err(unrecordable)
}

/// Whether a session lets an order with these market hours trade.
fn admits(hours: MarketHours, session: Session) -> bool {
    match hours {
        MarketHours::Regular => session == Session::Regular,
        MarketHours::Extended => matches!(session, Session::Regular | Session::Extended),
        MarketHours::AllDay => session != Session::Closed,
    }
}

fn is_terminal(state: State) -> bool {
    matches!(
        state,
        State::Filled | State::Cancelled | State::Rejected | State::Failed | State::Voided
    )
}

/// The states a place may answer with when a fault scripts the answer.
fn is_initial(state: State) -> bool {
    matches!(
        state,
        State::New | State::Unconfirmed | State::Rejected | State::Failed
    )
}

/// The broker-side moves other than a fill; fills alone reach `partially_filled` and `filled`, and
/// a terminal order moves nowhere.
fn may_advance(from: State, to: State) -> Result<(), SimError> {
    let allowed = match from {
        State::New => matches!(
            to,
            State::Queued
                | State::Confirmed
                | State::Unconfirmed
                | State::Cancelled
                | State::Rejected
                | State::Failed
        ),
        State::Queued => matches!(
            to,
            State::Confirmed
                | State::Unconfirmed
                | State::Cancelled
                | State::Rejected
                | State::Failed
        ),
        State::Unconfirmed => {
            matches!(
                to,
                State::Confirmed | State::Cancelled | State::Rejected | State::Failed
            )
        }
        State::Confirmed => matches!(to, State::Cancelled | State::Voided | State::Failed),
        State::PartiallyFilled => matches!(to, State::Cancelled | State::Voided),
        State::Filled | State::Cancelled | State::Rejected | State::Failed | State::Voided => {
            return Err(SimError::Terminal);
        }
    };
    if allowed {
        Ok(())
    } else {
        Err(SimError::IllegalTransition)
    }
}

/// The order as an answer shows it: without its `ref_id` unless the echo switch is on.
fn shown(mut order: Order, echo_ref_id: bool) -> Order {
    if !echo_ref_id {
        order.ref_id = None;
    }
    order
}

/// A UUID in its 8-4-4-4-12 hexadecimal text form.
fn is_uuid(text: &str) -> bool {
    let groups: Vec<usize> = text.split('-').map(str::len).collect();
    groups == [8, 4, 4, 4, 12] && text.chars().all(|c| c == '-' || c.is_ascii_hexdigit())
}
