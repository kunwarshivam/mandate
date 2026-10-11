//! The preflight against the simulated server (E7-6 C4; DEC-875 item 6; DEC-902 items 1 to 7):
//! the connector reads the agentic account through the wire — portfolio, positions and orders
//! each behind a fresh `get_accounts` check, the one symbol's tradability through the same scoped
//! read, and the quote, which names no account — and the facts it maps survive the answer's ride
//! over MCP: cash and buying power as scripted, every position with its average, the working
//! orders with the side and filled quantity their records name, the locked quote at the price
//! the market scripted. A halted symbol is `NotTradable` before the quote is asked for. The core
//! itself: a fill sets the average its buys were filled at, rounded onto the price's grid, and
//! the reads refuse what the core does not hold, never answering a guess. Here as in
//! `robinhood.rs`, so no product crate depends on the simulator. Oracles: the fixture's scripted
//! figures, the sim's own records, the seam's call log, and the wire client's answers.

mod common;

use std::cell::RefCell;
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use common::wire::Wire;
use common::{AGENTIC, DAY_TRADER, NOT_AGENTIC, account, price, qty, sim};
use mandate_accounting::{InstrumentId, Side};
use mandate_executor::{
    BrokerConnector, BrokerOrder, BrokerOutcome, BrokerPosition, BrokerRequest, ClientOrderId,
    ConnectorError, EventId, IntentId, OrderType as Kind, Purpose, SubmitOrder, TimeInForce as Tif,
};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_num::{SignedQty, Usd};
use mandate_rh_sim::{Event, Position, Session, Sim, SimError, SimServer, Variant};
use mandate_robinhood::{AccountSnapshot, Quote, RobinhoodConnector, RobinhoodError, Tools};
use serde_json::{Value, json};

/// A call of the connector's, as the seam saw it.
type Call = (CallClass, &'static str, Value);
type Calls = Rc<RefCell<Vec<Call>>>;

/// The connector's seam over the wire client, recording every call.
struct Loopback {
    wire: RefCell<Wire>,
    calls: Calls,
}

impl Tools for Loopback {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> Result<String, McpError> {
        self.calls
            .borrow_mut()
            .push((class, tool, arguments.clone()));
        let reply = self.wire.borrow_mut().call_raw(tool, arguments.clone());
        let message: Value = serde_json::from_str(&reply.ok_or(McpError::Network)?.body)
            .map_err(|_| McpError::Malformed)?;
        Ok(message
            .get("result")
            .cloned()
            .ok_or(McpError::Malformed)?
            .to_string())
    }
}

/// The connector and the two ends of its calls: the seam's record and the server's.
fn connector(server: &SimServer) -> (RobinhoodConnector<Loopback>, Calls) {
    let calls = Calls::default();
    let tools = Loopback {
        wire: RefCell::new(Wire::connect(&server.url().unwrap())),
        calls: Rc::clone(&calls),
    };
    (RobinhoodConnector::new(tools, AGENTIC.to_owned()), calls)
}

/// The preflight's nine calls of the reads, with the arguments the connector must send.
fn preflight_calls() -> Vec<Call> {
    [
        (CallClass::Ordinary, "get_accounts", json!({})),
        (
            CallClass::Ordinary,
            "get_portfolio",
            json!({"account_number": AGENTIC}),
        ),
        (CallClass::Ordinary, "get_accounts", json!({})),
        (
            CallClass::Ordinary,
            "get_equity_positions",
            json!({"account_number": AGENTIC}),
        ),
        (CallClass::Ordinary, "get_accounts", json!({})),
        (
            CallClass::Ordinary,
            "get_equity_orders",
            json!({"account_number": AGENTIC}),
        ),
        (CallClass::Ordinary, "get_accounts", json!({})),
        (
            CallClass::Ordinary,
            "get_equity_tradability",
            json!({"account_number": AGENTIC, "symbol": "SPY"}),
        ),
        (
            CallClass::Ordinary,
            "get_equity_quotes",
            json!({"symbols": ["SPY"]}),
        ),
    ]
    .to_vec()
}

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol).unwrap()
}

fn key(intent: &str) -> ClientOrderId {
    ClientOrderId::for_intent(&IntentId(EventId(intent.to_owned()))).unwrap()
}

/// A `gfd` limit buy of `quantity` SPY at `limit`, an opening.
fn opening(intent: &str, quantity: &str, limit: &str) -> SubmitOrder {
    SubmitOrder {
        client_order_id: key(intent),
        instrument: id("SPY"),
        side: Side::Buy,
        qty: qty(quantity),
        order_type: Kind::Limit,
        tif: Tif::Day,
        limit_price: Some(price(limit)),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: Purpose::Open,
    }
}

fn run(
    connector: &mut RobinhoodConnector<Loopback>,
    request: &BrokerRequest,
) -> Result<BrokerOutcome, ConnectorError> {
    let mut future = pin!(connector.call(request));
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(outcome) => outcome,
        Poll::Pending => panic!("the connector awaited something other than the seam"),
    }
}

/// Every future the connector makes completes once the seam answers, which the wire does at once.
fn ready<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("the connector awaited something other than the seam"),
    }
}

/// A `gfd` limit buy of `quantity` SPY at `limit`, an opening, submitted and answered.
fn opened(
    connector: &mut RobinhoodConnector<Loopback>,
    intent: &str,
    quantity: &str,
    limit: &str,
) -> BrokerOrder {
    let request = BrokerRequest::Submit(opening(intent, quantity, limit));
    match run(connector, &request) {
        Ok(BrokerOutcome::Submitted(order)) => order,
        other => panic!("the opening {intent}: {other:?}"),
    }
}

fn refused(result: &Value) -> bool {
    result.get("isError").and_then(Value::as_bool) == Some(true)
}

/// The simulator the connector's preflight reads: exactly one agentic account beside one that is
/// not (DEC-875 item 1 refuses more than one agentic account, so the shared three-account
/// fixture would refuse every read), a SPY quote of 500, and the regular session.
fn read_sim() -> Result<Sim, SimError> {
    let mut core = Sim::new(vec![
        account(AGENTIC, true, false),
        account(NOT_AGENTIC, false, false),
    ])?;
    core.apply(Event::Quote("SPY".to_owned(), price("500")))?;
    core.apply(Event::Session(Session::Regular))?;
    Ok(core)
}

fn row(symbol: &str, quantity: &str, average_buy_price: &str) -> Position {
    Position {
        account_number: AGENTIC.to_owned(),
        symbol: symbol.to_owned(),
        quantity: qty(quantity),
        average_buy_price: price(average_buy_price),
    }
}

/// DEC-875 items 1 and 2 and DEC-902 items 1 to 8, end to end: one opening filled once, another
/// cancelled, and the preflight reading the whole account behind a fresh check each time.
#[test]
fn the_preflight_reads_the_whole_account_through_the_wire() {
    let mut core = read_sim().unwrap();
    core.apply(Event::EchoRefId(true)).unwrap();
    let server = SimServer::start(core, Variant::Honest).unwrap();
    let (mut connector, calls) = connector(&server);
    let first = opened(&mut connector, "01JOPEN", "2", "499");
    let second = opened(&mut connector, "01JMORE", "1", "400");
    server
        .drive(|core| core.fill(&first.broker_order_id, qty("1"), price("498")))
        .unwrap()
        .unwrap();
    let cancel = BrokerRequest::Cancel {
        client_order_id: key("01JMORE"),
    };
    run(&mut connector, &cancel).unwrap();
    let facts = ready(connector.preflight_facts(&id("SPY"))).unwrap();
    let expected = AccountSnapshot {
        cash: Usd::parse("750.25").unwrap(),
        buying_power: Usd::parse("10000").unwrap(),
        positions: vec![BrokerPosition {
            instrument: id("SPY"),
            qty: SignedQty::parse("1").unwrap(),
            avg_entry_price: price("498"),
        }],
        open_orders: vec![BrokerOrder {
            broker_order_id: first.broker_order_id.clone(),
            client_order_id: None,
            instrument: id("SPY"),
            side: Side::Buy,
            qty: qty("2"),
            filled_qty: qty("1"),
            limit_price: Some(price("499")),
            stop_price: None,
            status: "partially_filled".to_owned(),
            reject_code: None,
            replaced_by_broker_order_id: None,
            legs: Vec::new(),
            created_on: None,
        }],
    };
    assert_eq!(facts.account, expected);
    assert_eq!(
        facts.quote,
        Quote {
            symbol: id("SPY"),
            bid: price("500"),
            ask: price("500"),
        },
        "the quote is the scripted price, locked, and a locked quote is uncrossed"
    );
    let sent = calls.borrow();
    let placed: Vec<(CallClass, &str)> = sent
        .iter()
        .take(5)
        .map(|(class, tool, _)| (*class, *tool))
        .collect();
    assert_eq!(
        placed,
        [
            (CallClass::Ordinary, "review_equity_order"),
            (CallClass::Ordinary, "place_equity_order"),
            (CallClass::Ordinary, "review_equity_order"),
            (CallClass::Ordinary, "place_equity_order"),
            (CallClass::RiskReducing, "cancel_equity_order"),
        ]
    );
    assert_eq!(
        sent[5..],
        preflight_calls(),
        "each account read behind a fresh check, the quote on no account"
    );
    let seen = server.calls().unwrap();
    assert_eq!(
        seen,
        [
            "review_equity_order",
            "place_equity_order",
            "review_equity_order",
            "place_equity_order",
            "cancel_equity_order",
            "get_accounts",
            "get_portfolio",
            "get_accounts",
            "get_equity_positions",
            "get_accounts",
            "get_equity_orders",
            "get_accounts",
            "get_equity_tradability",
            "get_equity_quotes",
        ],
        "the call log, within the allowlist"
    );
    assert!(
        seen.iter().all(|call| ALLOWLIST.contains(&call.as_str())),
        "{seen:?}"
    );
    assert_eq!(second.broker_order_id, "rh-sim-000002");
}

/// DEC-902 item 6, end to end: a halted symbol answers `tradable: false`, and the preflight is
/// `NotTradable` after the four account reads, before any quote is asked for.
#[test]
fn a_halted_symbol_is_not_tradable_through_the_wire() {
    let mut core = read_sim().unwrap();
    core.apply(Event::Halt("SPY".to_owned())).unwrap();
    let server = SimServer::start(core, Variant::Honest).unwrap();
    let (connector, calls) = connector(&server);
    assert_eq!(
        ready(connector.preflight_facts(&id("SPY"))),
        Err(RobinhoodError::NotTradable)
    );
    let mut expected = preflight_calls();
    let quotes = expected.pop().unwrap();
    assert_eq!(
        calls.borrow().as_slice(),
        expected.as_slice(),
        "the quote was never asked for: {quotes:?} must not run"
    );
    assert_eq!(
        server.calls().unwrap(),
        [
            "get_accounts",
            "get_portfolio",
            "get_accounts",
            "get_equity_positions",
            "get_accounts",
            "get_equity_orders",
            "get_accounts",
            "get_equity_tradability",
        ]
    );
}

/// The five reads a tool call drives serve only what the core holds (DEC-875 item 4): an unknown
/// account, a symbol that is not text, and a symbol with no scripted quote each refuse, and the
/// account list itself answers with every account and its agentic flag.
#[test]
fn the_served_reads_refuse_what_the_core_does_not_hold() {
    let server = SimServer::start(sim().unwrap(), Variant::Honest).unwrap();
    let mut wire = Wire::connect(&server.url().unwrap());
    for (tool, arguments) in [
        ("get_portfolio", json!({"account_number": "nope"})),
        ("get_equity_positions", json!({"account_number": "nope"})),
        (
            "get_equity_tradability",
            json!({"account_number": "nope", "symbol": "SPY"}),
        ),
        (
            "get_equity_tradability",
            json!({"account_number": AGENTIC, "symbol": 5}),
        ),
        ("get_equity_quotes", json!({"symbols": ["QQQ"]})),
        ("get_equity_quotes", json!({"symbols": [5]})),
        ("get_equity_quotes", json!({})),
    ] {
        let answer = wire.call(tool, arguments.clone());
        assert!(refused(&answer), "{tool} on {arguments}: {answer}");
    }
    let accounts = wire.call("get_accounts", json!({}));
    let records = &accounts["structuredContent"]["accounts"];
    let listed: Vec<(&str, bool)> = records
        .as_array()
        .unwrap()
        .iter()
        .map(|record| {
            (
                record["account_number"].as_str().unwrap(),
                record["agentic_allowed"].as_bool().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        listed,
        [(AGENTIC, true), (NOT_AGENTIC, false), (DAY_TRADER, true)],
        "every account with its agentic flag"
    );
}

/// The core's own figures behind `get_portfolio` and `get_equity_positions` (DEC-902 items 2 and
/// 3): the scripted cash and buying power, each account's positions only, and the average price
/// the fills themselves set — weighted on a second buy, kept by a sell, kept on a row sold to
/// zero, which is still listed.
#[test]
fn fills_set_the_average_price_a_position_was_bought_at() {
    let mut core = sim().unwrap();
    let first = core.place(&common::limit("buy", "2", "500", 1)).unwrap();
    core.fill(&first.id, qty("2"), price("500")).unwrap();
    let second = core.place(&common::limit("buy", "1", "494", 2)).unwrap();
    core.fill(&second.id, qty("1"), price("494")).unwrap();
    assert_eq!(
        core.portfolio(AGENTIC).unwrap(),
        (Usd::parse("750.25").unwrap(), Usd::parse("10000").unwrap())
    );
    assert_eq!(core.position(AGENTIC, "SPY").unwrap(), qty("3"));
    assert_eq!(
        core.equity_positions(AGENTIC).unwrap(),
        vec![row("SPY", "3", "498")],
        "1000 + 494 over 3"
    );
    assert_eq!(core.equity_positions(NOT_AGENTIC).unwrap(), vec![]);
    let exit = core.place(&common::limit("sell", "3", "600", 3)).unwrap();
    core.fill(&exit.id, qty("3"), price("600")).unwrap();
    assert_eq!(
        core.equity_positions(AGENTIC).unwrap(),
        vec![row("SPY", "0", "498")],
        "the sold-out row is kept, with the average it had"
    );
}

/// The weighted average rounds onto the price grid, half to even (DEC-902 item 3): 2.000000005
/// over 2 rounds down to the even neighbour, never up to 1.000000003. Tradability turns on the
/// halt, and quotes refuse a symbol with none.
#[test]
fn a_rebought_average_rounds_to_the_grids_nine_places() {
    let mut core = sim().unwrap();
    assert!(core.tradability(AGENTIC, "SPY").unwrap());
    let first = core
        .place(&common::limit("buy", "1", "1.000000002", 1))
        .unwrap();
    core.fill(&first.id, qty("1"), price("1.000000002"))
        .unwrap();
    let second = core
        .place(&common::limit("buy", "1", "1.000000003", 2))
        .unwrap();
    core.fill(&second.id, qty("1"), price("1.000000003"))
        .unwrap();
    assert_eq!(
        core.equity_positions(AGENTIC).unwrap(),
        vec![row("SPY", "2", "1.000000002")]
    );
    core.apply(Event::Halt("SPY".to_owned())).unwrap();
    assert!(!core.tradability(AGENTIC, "SPY").unwrap());
    assert_eq!(
        core.equity_quotes(&["SPY".to_owned()]).unwrap(),
        vec![("SPY".to_owned(), price("500"))]
    );
    assert_eq!(
        core.equity_quotes(&["QQQ".to_owned()]).unwrap_err(),
        SimError::NoQuote
    );
    assert_eq!(
        core.accounts()
            .iter()
            .map(|account| (account.number.as_str(), account.agentic_allowed))
            .collect::<Vec<_>>(),
        [(AGENTIC, true), (NOT_AGENTIC, false), (DAY_TRADER, true)]
    );
}
