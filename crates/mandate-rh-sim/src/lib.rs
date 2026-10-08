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
//! Where the contract is silent the simulator takes the reading that adds no risk: a `ref_id` is
//! deduplicated per account and returns the first order whatever the re-send says; buying power
//! is a scripted figure that orders and fills do not move; and a sell larger than the position is
//! refused, because the agentic account cannot sell short.
//!
//! [DEC-124]: ../../../docs/project/04-decision-log.md
//! [DEC-441]: ../../../docs/project/decisions/DEC-441.md

use mandate_num::{Price, Qty, Usd};

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
/// of the trading day, which cancels every working `gfd` order and closes the market.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Session(Session),
    Quote(String, Price),
    Halt(String),
    Script(Fault),
    EndOfDay,
}

/// A customer account. `pattern_day_trader` marks an account a day trade would restrict, so an
/// order against today's fills in its symbol draws the pattern day trading alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub number: String,
    pub agentic_allowed: bool,
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
    pub ref_id: String,
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

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SimError {
    /// The body of every stub in the tests PR (DEC-77).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
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
}

/// The simulated broker: its accounts, orders, positions, and the scripted market.
#[derive(Debug, Clone)]
pub struct Sim {
    _stub: (),
}

impl Sim {
    /// A simulator over these accounts, in the closed session, with no quotes, halts or orders.
    pub fn new(_accounts: Vec<Account>) -> Result<Self, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    /// Applies one scripted input of the market or the broker.
    pub fn apply(&mut self, _event: Event) -> Result<(), SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    /// `review_equity_order`: the place's checks and alerts, with nothing placed.
    pub fn review(&self, _request: &OrderRequest) -> Result<Review, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    /// `place_equity_order`. A `ref_id` this account has used returns that first order unchanged.
    pub fn place(&mut self, _request: &OrderRequest) -> Result<Order, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    /// `cancel_equity_order`: refused for a terminal order or another account's.
    pub fn cancel(&mut self, _account_number: &str, _order_id: &str) -> Result<Order, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    /// A scripted execution of a working order in a session its market hours admit.
    pub fn fill(
        &mut self,
        _order_id: &str,
        _quantity: Qty,
        _price: Price,
    ) -> Result<Order, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    /// A scripted broker-side state change other than a fill.
    pub fn advance(&mut self, _order_id: &str, _to: State) -> Result<Order, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    /// `get_equity_orders` for one account, newest first.
    pub fn orders(&self, _account_number: &str) -> Result<Vec<Order>, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }

    pub fn position(&self, _account_number: &str, _symbol: &str) -> Result<Qty, SimError> {
        Err(SimError::Unimplemented { story: "E7-25" })
    }
}
