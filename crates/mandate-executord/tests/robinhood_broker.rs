//! The executor process over the Robinhood connector (DEC-2386 items 3 and 4, E21-30 X1b-3):
//! `RobinhoodBroker` answers the profile Robinhood declares, sends `Submit`, `Cancel` and
//! `ListOrders` through the connector, and reads the agentic account in one read mapped by
//! `broker_account`; then a whole process over it opens, books the fill the broker's order record
//! and position imply, protects the position with the one GTC stop-limit the profile offers, and
//! reconciles clean with the cash in band.
//!
//! The broker is a scripted MCP seam answering in the shapes of Robinhood's published contract
//! ([robinhood-contract.md](../../../docs/project/tasks/robinhood-contract.md)): the test sets what
//! each read answers between passes, and a place answers the order it was asked for. No host, no
//! token and no simulator is reached, and nothing is recorded from a real account (DEC-529 item 6,
//! `AGENTS.md` rule 8). Each expectation is typed from the case's own inputs.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value as Canon;
use mandate_executor::{
    BrokerOutcome, BrokerRequest, ClientOrderId, ExitTier, FillConfigRefs, InstrumentSnapshot,
    OrderType, Purpose, SubmitOrder, TimeInForce,
};
use mandate_executord::follow::folded;
use mandate_executord::marks::NoMarks;
use mandate_executord::quotes::NoQuotes;
use mandate_executord::robinhood::RobinhoodBroker;
use mandate_executord::{Broker, ExecutorProcess, Wiring};
use mandate_journal::Environment;
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Price, Qty, ShareIncrement, SignedQty, Usd};
use mandate_robinhood::{Tickers, Tools, robinhood_profile};
use serde_json::{Map, Value, json};
use support::{
    ACCOUNT_STREAM, AGENT, Allowing, Error, Ids, Instruments, Journal, Mandate, NOON,
    TwoPercentStop, at, config, deployed, fees, process_config, propose,
};

/// The agentic account the connector is recorded on, typed by the founder in the real run and
/// made up here.
const ACCOUNT_NUMBER: &str = "5QR00001";
const OPEN: &str = "01JINTENT0000000000000000A";
const REF: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
/// The account's cash before the opening.
const CASH: &str = "100000";
/// The cash after 10 AAPL bought at 150.
const SPENT: &str = "98500";

/// What each read of the account answers, set by the test between passes.
struct Book {
    cash: String,
    positions: Vec<Value>,
    orders: Vec<Value>,
    /// How many places have been answered, which numbers each answered order.
    placed: u32,
}

/// Every call the seam was handed: its tool and arguments.
type Calls = Rc<RefCell<Vec<(&'static str, Value)>>>;

/// The scripted seam: the account's reads answer what the [`Book`] holds; a review clears; a
/// place answers the order it was asked for, accepted and unfilled, as the contract's record.
#[derive(Clone)]
struct Scripted {
    book: Rc<RefCell<Book>>,
    calls: Calls,
}

impl Tools for Scripted {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> Result<String, McpError> {
        let _ = class;
        self.calls.borrow_mut().push((tool, arguments.clone()));
        let mut book = self.book.borrow_mut();
        let content = match tool {
            "get_accounts" => json!({"accounts": [
                {"account_number": ACCOUNT_NUMBER, "agentic_allowed": true}]}),
            "get_portfolio" => json!({"account_number": ACCOUNT_NUMBER, "cash": book.cash,
                "buying_power": book.cash}),
            "get_equity_positions" => json!({"positions": book.positions}),
            "get_equity_orders" => json!({"orders": book.orders}),
            "review_equity_order" => json!({"alerts": []}),
            "place_equity_order" => {
                book.placed += 1;
                answered(arguments, &format!("rh-{}", book.placed), "confirmed", "0")
            }
            _ => return Err(McpError::Network),
        };
        Ok(json!({"structuredContent": content, "isError": false}).to_string())
    }
}

/// The contract's order record for the order `arguments` asked for, as broker order `id` in
/// `state` with `filled` filled.
fn answered(arguments: &Value, id: &str, state: &str, filled: &str) -> Value {
    let asked = |name: &str| arguments.get(name).cloned().unwrap_or(Value::Null);
    json!({"id": id, "account_number": ACCOUNT_NUMBER, "symbol": asked("symbol"),
        "side": asked("side"), "type": asked("type"), "quantity": asked("quantity"),
        "limit_price": asked("limit_price"), "stop_price": asked("stop_price"),
        "time_in_force": asked("time_in_force"), "market_hours": "regular_hours",
        "state": state, "filled_quantity": filled})
}

fn seam(cash: &str) -> (Scripted, Rc<RefCell<Book>>, Calls) {
    let book = Rc::new(RefCell::new(Book {
        cash: cash.to_owned(),
        positions: Vec::new(),
        orders: Vec::new(),
        placed: 0,
    }));
    let calls = Calls::default();
    let tools = Scripted {
        book: Rc::clone(&book),
        calls: Rc::clone(&calls),
    };
    (tools, book, calls)
}

fn tickers() -> Result<Tickers, Error> {
    Ok(Tickers::registered([(
        InstrumentId::new("AAPL")?,
        "AAPL".to_owned(),
    )])?)
}

fn broker(tools: Scripted) -> Result<RobinhoodBroker<Scripted>, Error> {
    Ok(RobinhoodBroker::new(
        tools,
        ACCOUNT_NUMBER.to_owned(),
        tickers()?,
    )?)
}

fn position(quantity: &str, average: &str) -> Value {
    json!({"account_number": ACCOUNT_NUMBER, "symbol": "AAPL", "quantity": quantity,
        "average_buy_price": average})
}

fn opening(id: &str) -> Result<SubmitOrder, Error> {
    Ok(SubmitOrder {
        client_order_id: ClientOrderId::parse(id)?,
        instrument: InstrumentId::new("AAPL")?,
        side: Side::Buy,
        qty: Qty::parse("10")?,
        order_type: OrderType::Limit,
        tif: TimeInForce::Day,
        limit_price: Some(Price::parse("150")?),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: Purpose::Open,
    })
}

/// DEC-2386 item 3: the process's broker declares Robinhood's own profile, the one the executor
/// reads its protection and its listing fallback from (DEC-531).
#[test]
fn the_broker_declares_robinhoods_profile() -> Result<(), Error> {
    let (tools, _, _) = seam(CASH);
    let broker = broker(tools)?;
    assert_eq!(broker.profile()?, robinhood_profile()?);
    Ok(())
}

/// A `Submit` goes through the connector: reviewed, then placed on the recorded account with the
/// order's derived `ref_id`, and answered as the order the place answered.
#[test]
fn a_submit_is_reviewed_then_placed_through_the_connector() -> Result<(), Error> {
    let (tools, _, calls) = seam(CASH);
    let mut broker = broker(tools)?;
    let order = opening("md-01JINTENT0000000000000000A")?;
    let answer = broker.call(&BrokerRequest::Submit(order.clone()))?;
    let BrokerOutcome::Submitted(placed) = answer else {
        return Err(format!("a submission, not {answer:?}").into());
    };
    assert_eq!(
        (
            placed.broker_order_id.as_str(),
            placed.client_order_id.as_deref(),
            placed.qty,
            placed.filled_qty
        ),
        (
            "rh-1",
            Some("md-01JINTENT0000000000000000A"),
            order.qty,
            Qty::ZERO
        )
    );
    let sent: Vec<&str> = calls.borrow().iter().map(|(tool, _)| *tool).collect();
    assert_eq!(
        sent.iter()
            .filter(|tool| tool.ends_with("equity_order"))
            .copied()
            .collect::<Vec<_>>(),
        ["review_equity_order", "place_equity_order"]
    );
    let calls = calls.borrow();
    let place = calls
        .iter()
        .find(|(tool, _)| *tool == "place_equity_order")
        .map(|(_, arguments)| arguments)
        .ok_or("a place")?;
    assert_eq!(
        place.get("account_number").and_then(Value::as_str),
        Some(ACCOUNT_NUMBER)
    );
    assert_eq!(
        place.get("ref_id").and_then(Value::as_str),
        Some(mandate_robinhood::ref_id(&order.client_order_id)?.as_str())
    );
    Ok(())
}

/// DEC-2386 item 2: the account is read once and mapped by the shell's own mapping: the working
/// orders and positions as listed, and the account with every buying power at most the cash.
#[test]
fn the_account_is_read_once_and_mapped_by_broker_account() -> Result<(), Error> {
    let (tools, book, calls) = seam("1000.5");
    book.borrow_mut().positions = vec![position("3", "85.12")];
    let mut broker = broker(tools)?;
    let reads = broker.account_reads()?;
    assert_eq!(
        reads
            .positions
            .iter()
            .map(|p| (p.instrument.as_str().to_owned(), p.qty))
            .collect::<Vec<_>>(),
        [("AAPL".to_owned(), SignedQty::parse("3")?)]
    );
    assert!(reads.open_orders.is_empty());
    assert_eq!(
        (
            reads.account.cash,
            reads.account.buying_power,
            reads.account.multiplier
        ),
        (Usd::parse("1000.5")?, Usd::parse("1000.5")?, 1)
    );
    let reads_sent = calls
        .borrow()
        .iter()
        .filter(|(tool, _)| *tool == "get_portfolio")
        .count();
    assert_eq!(reads_sent, 1, "one read of the portfolio");
    Ok(())
}

/// What Robinhood does not offer is never sent: the activity feed and the four Alpaca reads are
/// `NotSent`, with nothing called.
#[test]
fn what_robinhood_does_not_offer_is_never_sent() -> Result<(), Error> {
    let (tools, _, calls) = seam(CASH);
    let mut broker = broker(tools)?;
    let activities = BrokerRequest::ListActivities {
        since: mandate_executor::ActivityCursor(String::new()),
    };
    for request in [
        activities,
        BrokerRequest::GetAccount,
        BrokerRequest::ListPositions,
    ] {
        assert!(
            matches!(
                broker.call(&request),
                Err(mandate_executor::ConnectorError::NotSent { .. })
            ),
            "{request:?} is not sent"
        );
    }
    assert!(calls.borrow().is_empty(), "nothing is called");
    Ok(())
}

/// Support's instruments, with the fill's configuration named, so the journal admits the fill
/// (DEC-938 item 2).
struct Referenced;

impl InstrumentSnapshot for Referenced {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        Instruments.asset_class(instrument)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        Instruments.increment(instrument)
    }

    fn exit_tier(&self, instrument: &InstrumentId) -> Option<ExitTier> {
        Instruments.exit_tier(instrument)
    }

    fn fill_refs(&self, instrument: &InstrumentId) -> Option<FillConfigRefs> {
        (instrument.as_str() == "AAPL").then(|| FillConfigRefs {
            fee_config: REF.to_owned(),
            trading_calendar: REF.to_owned(),
            settlement_calendar: REF.to_owned(),
            instrument_snapshot: REF.to_owned(),
        })
    }
}

fn wiring() -> Result<Wiring, Error> {
    Ok(Wiring {
        ids: Box::new(Ids),
        mandates: Box::new(Mandate),
        instruments: Box::new(Referenced),
        config: config(),
        fees: fees()?,
        gate: Box::new(Allowing),
        protections: Box::new(TwoPercentStop),
        quotes: Box::new(NoQuotes),
        marked: Box::new(NoMarks),
    })
}

type Process = ExecutorProcess<Journal, RobinhoodBroker<Scripted>>;

/// The journal, the process, the account's book and the seam's calls.
type Protected = (Journal, Process, Rc<RefCell<Book>>, Calls);

fn pass(journal: &Journal, process: &mut Process, id: &str, now: &str) -> Result<(), Error> {
    support::tick(journal, id, now)?;
    process.step(at(now)?);
    Ok(())
}

fn action(payload: &Canon) -> Option<&str> {
    payload.get("action").and_then(Canon::as_str)
}

fn result(run: &Canon) -> Option<&str> {
    run.get("result").and_then(Canon::as_str)
}

/// DEC-2386 item 5, end to end: an agent's opening of 10 AAPL at 150, handed to a process over the
/// Robinhood broker, is placed; once the broker's order record reads it filled and the account
/// holds the 10, the process books the agent's fill from the record and the position at their
/// average (DEC-1151), protects the position with one resting GTC stop-limit sell, the profile's
/// only protection (no OCO, DEC-966), and reconciles clean, the broker's cash the model's.
/// A process over the Robinhood broker after the agent's opening of 10 AAPL at 150 was placed,
/// the broker's record read it filled with the 10 held, and the passes a minute apart that book it
/// and place its protective stop, which the broker then lists as working. Answers the journal, the
/// process, the account's book and the seam's calls.
fn protected_position() -> Result<Protected, Error> {
    let journal: Journal = deployed()?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    let (tools, book, calls) = seam(CASH);
    let mut process: Process = ExecutorProcess::start(
        journal.clone(),
        broker(tools)?,
        wiring()?,
        &process_config(Environment::Paper),
        at(NOON)?,
    );
    pass(&journal, &mut process, "t0", NOON)?;
    let placed = calls
        .borrow()
        .iter()
        .find(|(tool, _)| *tool == "place_equity_order")
        .map(|(_, arguments)| arguments.clone())
        .ok_or("the opening is placed")?;
    {
        let mut account = book.borrow_mut();
        account.cash = SPENT.to_owned();
        account.positions = vec![position("10", "150")];
        account.orders = vec![answered(&placed, "rh-1", "filled", "10")];
    }
    for (second, id) in [(10, "t1"), (20, "t2"), (30, "t3")] {
        pass(
            &journal,
            &mut process,
            id,
            &format!("2026-09-21T15:00:{second}Z"),
        )?;
        let mut account = book.borrow_mut();
        let stops: Vec<Value> = stop_places(&calls)
            .iter()
            .map(|arguments| answered(arguments, "rh-2", "confirmed", "0"))
            .collect();
        account.orders.truncate(1);
        account.orders.extend(stops);
    }
    Ok((journal, process, book, calls))
}

/// Every place of a stop-limit the seam was handed, as its arguments.
fn stop_places(calls: &Calls) -> Vec<Value> {
    calls
        .borrow()
        .iter()
        .filter(|(tool, arguments)| {
            *tool == "place_equity_order"
                && arguments.get("type").and_then(Value::as_str) == Some("stop_limit")
        })
        .map(|(_, arguments)| arguments.clone())
        .collect()
}

#[test]
fn an_opening_fills_is_booked_protected_and_reconciles_clean() -> Result<(), Error> {
    let (journal, _process, _book, calls) = protected_position()?;
    let fill = journal
        .payloads("FillApplied")
        .into_iter()
        .chain(journal.payloads("LateFillApplied"))
        .find(|p| p.get("side").and_then(Canon::as_str) == Some("buy"))
        .ok_or("the agent's fill is booked")?;
    assert_eq!(
        fill.get("client_order_id").and_then(Canon::as_str),
        Some(format!("md-{OPEN}").as_str())
    );
    assert_eq!(fill.get("qty_gross").and_then(Canon::as_str), Some("10"));
    assert_eq!(fill.get("price").and_then(Canon::as_str), Some("150"));
    let stop: Vec<Map<String, Value>> = calls
        .borrow()
        .iter()
        .filter(|(tool, _)| *tool == "place_equity_order")
        .filter_map(|(_, arguments)| arguments.as_object().cloned())
        .filter(|arguments| arguments.get("type").and_then(Value::as_str) == Some("stop_limit"))
        .collect();
    let [stop] = stop.as_slice() else {
        return Err(format!("one protective stop-limit is placed: {stop:?}").into());
    };
    assert_eq!(
        (
            stop.get("side").and_then(Value::as_str),
            stop.get("quantity").and_then(Value::as_str),
            stop.get("time_in_force").and_then(Value::as_str)
        ),
        (Some("sell"), Some("10"), Some("gtc"))
    );
    assert!(
        journal
            .payloads("ProtectionChanged")
            .iter()
            .any(|p| action(p) == Some("placed")),
        "the stop is the position's protection: {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    let runs = journal.payloads("ReconciliationRun");
    let last = runs.last().ok_or("a reconciliation")?;
    assert_eq!(
        result(last),
        Some("clean"),
        "the last reconciliation runs clean: {runs:?}"
    );
    assert!(
        runs.iter().all(|run| result(run) != Some("mismatch")),
        "{runs:?}"
    );
    let snapshots = journal.payloads("AccountSnapshotRecorded");
    assert!(
        snapshots
            .iter()
            .all(|s| s.get("cash_in_band") != Some(&Canon::Bool(false))),
        "{snapshots:?}"
    );
    assert!(
        journal
            .payloads("AgentModeApplied")
            .iter()
            .all(|p| p.get("to").and_then(Canon::as_str) == Some("normal")),
        "no agent is held"
    );
    Ok(())
}

/// DEC-2300 and DEC-2395 under the process: the resting stop sells the 10 at the broker, which then
/// lists no position and the stop's record filled. The process lists the stop, books its fill at
/// the stop's limit price, never its trigger, as `limit:<id>:10`, and reconciles with no position
/// mismatch and no agent held: the ledger is flat, as the broker is.
#[test]
fn the_resting_stop_sells_out_and_is_booked_at_its_limit() -> Result<(), Error> {
    let (journal, mut process, book, calls) = protected_position()?;
    let [stop] = stop_places(&calls)
        .try_into()
        .map_err(|_| "one stop is placed")?;
    let limit = stop
        .get("limit_price")
        .and_then(Value::as_str)
        .ok_or("the stop's limit")?
        .to_owned();
    assert_ne!(
        Some(limit.as_str()),
        stop.get("stop_price").and_then(Value::as_str),
        "the limit is not the trigger"
    );
    let proceeds = Qty::parse("10")?.notional(Price::parse(&limit)?)?;
    {
        let mut account = book.borrow_mut();
        account.cash = Usd::parse(SPENT)?.checked_add(proceeds)?.to_string();
        account.positions = Vec::new();
        account.orders.truncate(1);
        account.orders.push(answered(&stop, "rh-2", "filled", "10"));
    }
    for (second, id) in [(40, "t4"), (50, "t5")] {
        pass(
            &journal,
            &mut process,
            id,
            &format!("2026-09-21T15:00:{second}Z"),
        )?;
    }
    let sold = journal
        .payloads("FillApplied")
        .into_iter()
        .chain(journal.payloads("LateFillApplied"))
        .find(|p| p.get("side").and_then(Canon::as_str) == Some("sell"))
        .ok_or("the stop's fill is booked")?;
    assert_eq!(
        sold.get("price").and_then(Canon::as_str),
        Some(limit.as_str())
    );
    assert_eq!(sold.get("qty_gross").and_then(Canon::as_str), Some("10"));
    assert!(
        sold.get("fill_id")
            .and_then(Canon::as_str)
            .is_some_and(|id| id.starts_with("limit:") && id.ends_with(":10")),
        "{sold:?}"
    );
    assert!(
        journal
            .payloads("BrokerPositionObserved")
            .iter()
            .all(|p| p.get("mismatch") != Some(&Canon::Bool(true))),
        "no position mismatch"
    );
    assert!(
        journal.payloads("AgentModeApplied").is_empty(),
        "no agent is held"
    );
    Ok(())
}

/// After a restart the broker is rebuilt from the account stream: the opening's key is never
/// placed again, so a second `Submit` of it sends nothing and answers `Unknown` (DEC-860 items 4
/// and 7).
#[test]
fn a_restored_broker_never_places_a_journaled_key_again() -> Result<(), Error> {
    let (journal, _process, _book, _calls) = protected_position()?;
    let records = journal
        .rows_of(ACCOUNT_STREAM)
        .iter()
        .map(|row| folded(row).map_err(|_| "a readable row"))
        .collect::<Result<Vec<_>, _>>()?;
    let (tools, _, calls) = seam(SPENT);
    let mut restored =
        RobinhoodBroker::restore(tools, ACCOUNT_NUMBER.to_owned(), tickers()?, &records)?;
    let again = restored.call(&BrokerRequest::Submit(opening(&format!("md-{OPEN}"))?));
    assert!(
        matches!(again, Err(mandate_executor::ConnectorError::Unknown(_))),
        "{again:?}"
    );
    assert!(
        calls
            .borrow()
            .iter()
            .all(|(tool, _)| *tool != "place_equity_order"),
        "nothing is placed"
    );
    Ok(())
}
