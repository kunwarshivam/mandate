//! The executor process over the Robinhood connector (DEC-2386 items 2 to 5, DEC-2800, E21-30
//! X1b-3): `RobinhoodBroker` answers the profile Robinhood declares, sends `Submit`, `Cancel` and
//! `ListOrders` through the connector, and reads the agentic account in one read mapped by
//! `broker_account`; then a whole process over it opens, books the fill the broker's order record
//! and position imply, protects the position with the one GTC stop-limit the profile offers, and
//! reconciles clean with the cash in band, its activity cursor never moved.
//!
//! The broker is a scripted MCP seam answering in the shapes of Robinhood's published contract
//! ([robinhood-contract.md](../../../docs/project/tasks/robinhood-contract.md)): the test sets what
//! each read answers between passes, and a place answers the order it was asked for. No host, no
//! token and no simulator is reached, and nothing is recorded from a real account (DEC-529 item 6,
//! `AGENTS.md` rule 8; DEC-2800 item 1). Every request the seam is handed is checked against a
//! literal typed from the case's own inputs, so the echo never stands in for the oracle.

mod support;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value as Canon;
use mandate_executor::{
    ActivityCursor, BrokerOutcome, BrokerRequest, ClientOrderId, ConnectorError, ExitTier,
    FillConfigRefs, InstrumentSnapshot, OrderType, Purpose, SubmitOrder, TimeInForce,
};
use mandate_executord::follow::folded;
use mandate_executord::marks::NoMarks;
use mandate_executord::quotes::NoQuotes;
use mandate_executord::robinhood::RobinhoodBroker;
use mandate_executord::{Broker, ExecutorProcess, Wiring};
use mandate_journal::Environment;
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Price, Qty, ShareIncrement, SignedQty, Usd};
use mandate_robinhood::{Tickers, Tools, ref_id, robinhood_profile};
use serde_json::{Value, json};
use support::{
    ACCOUNT_STREAM, AGENT, Allowing, Error, Ids, Instruments, Journal, Mandate, NOON,
    TwoPercentStop, at, config, deployed, fees, process_config, propose,
};

/// The agentic account the connector is recorded on, typed by the founder in the real run and
/// made up here.
const ACCOUNT_NUMBER: &str = "5QR00001";
const OPEN: &str = "01JINTENT0000000000000000A";
const OPENING: &str = "md-01JINTENT0000000000000000A";
const REF: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";
/// The account's cash before the opening.
const CASH: &str = "100000";
/// The position's average after the opening of 10 at the limit 150 filled, below the limit so
/// the oracle can tell the record's average (DEC-1151) from the limit.
const AVERAGE: &str = "149.5";
/// The cash after 10 AAPL bought at [`AVERAGE`]: 100000 − 1495.
const SPENT: &str = "98505";
/// The protective stop `TwoPercentStop` gives an entry at 150: 2% below it.
const STOP: &str = "147";
/// The stop-limit's limit: the stop less the configuration's 0.5% offset, 147 × 0.995.
const STOP_LIMIT: &str = "146.265";
/// The cash after the stop sold the 10 at [`STOP_LIMIT`]: 98505 + 1462.65.
const SOLD: &str = "99967.65";

/// What each read of the account answers, set by the test between passes.
struct Book {
    cash: String,
    buying_power: String,
    positions: Vec<Value>,
    orders: Vec<Value>,
    /// How many places have been answered, which numbers each answered order.
    placed: u32,
}

/// Every call the seam was handed: its tool and arguments.
type Calls = Rc<RefCell<Vec<(&'static str, Value)>>>;

/// The scripted seam: the account's reads answer what the [`Book`] holds; a review clears; a
/// place answers the order it was asked for, accepted and unfilled, as the contract's record, and
/// the broker lists that record from then on, as Robinhood lists an order it has accepted; and
/// a listing (a `get_equity_orders` read filtered by `created_at_gte`) goes unanswered while
/// `listing_lost` is set.
#[derive(Clone)]
struct Scripted {
    book: Rc<RefCell<Book>>,
    calls: Calls,
    listing_lost: Rc<Cell<bool>>,
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
        let listing = arguments.get("created_at_gte").is_some();
        if tool == "get_equity_orders" && listing && self.listing_lost.get() {
            return Err(McpError::Network);
        }
        let mut book = self.book.borrow_mut();
        let content = match tool {
            "get_accounts" => json!({"accounts": [
                {"account_number": ACCOUNT_NUMBER, "agentic_allowed": true}]}),
            "get_portfolio" => json!({"account_number": ACCOUNT_NUMBER, "cash": book.cash,
                "buying_power": book.buying_power}),
            "get_equity_positions" => json!({"positions": book.positions}),
            "get_equity_orders" => json!({"orders": book.orders}),
            "review_equity_order" => json!({"alerts": []}),
            "place_equity_order" => {
                book.placed += 1;
                let record = answered(arguments, &format!("rh-{}", book.placed), "confirmed", "0");
                book.orders.push(record.clone());
                record
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

struct Seam {
    tools: Scripted,
    book: Rc<RefCell<Book>>,
    calls: Calls,
    listing_lost: Rc<Cell<bool>>,
}

fn seam(cash: &str) -> Seam {
    let book = Rc::new(RefCell::new(Book {
        cash: cash.to_owned(),
        buying_power: cash.to_owned(),
        positions: Vec::new(),
        orders: Vec::new(),
        placed: 0,
    }));
    let calls = Calls::default();
    let listing_lost = Rc::new(Cell::new(false));
    let tools = Scripted {
        book: Rc::clone(&book),
        calls: Rc::clone(&calls),
        listing_lost: Rc::clone(&listing_lost),
    };
    Seam {
        tools,
        book,
        calls,
        listing_lost,
    }
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

/// The arguments of every call to `tool` the seam was handed, in order.
fn sent(calls: &Calls, tool: &str) -> Vec<Value> {
    calls
        .borrow()
        .iter()
        .filter(|(name, _)| *name == tool)
        .map(|(_, arguments)| arguments.clone())
        .collect()
}

/// The place of the opening of 10 AAPL at 150 as the contract's arguments, typed here: a limit
/// buy in the regular session, `gfd`, on the recorded account, with no stop (`AGENTS.md` rule 12).
fn opening_place() -> Result<Value, Error> {
    Ok(
        json!({"account_number": ACCOUNT_NUMBER, "symbol": "AAPL", "side": "buy",
        "type": "limit", "quantity": "10", "limit_price": "150", "time_in_force": "gfd",
        "market_hours": "regular_hours",
        "ref_id": ref_id(&ClientOrderId::parse(OPENING)?)?}),
    )
}

/// The same request without its `ref_id`, as the review is sent it.
fn reviewed(place: &Value) -> Value {
    let mut review = place.clone();
    if let Value::Object(members) = &mut review {
        members.remove("ref_id");
    }
    review
}

/// DEC-2386 item 3: the process's broker declares Robinhood's own profile, the one the executor
/// reads its protection and its listing fallback from (DEC-531).
#[test]
#[ignore = "pending E21-30"]
fn the_broker_declares_robinhoods_profile() -> Result<(), Error> {
    let broker = broker(seam(CASH).tools)?;
    assert_eq!(broker.profile()?, robinhood_profile()?);
    Ok(())
}

/// A `Submit` goes through the connector: reviewed, then placed on the recorded account with the
/// order's derived `ref_id`, each request exactly the contract's arguments for the order, and
/// answered as the order the place answered.
#[test]
#[ignore = "pending E21-30"]
fn a_submit_is_reviewed_then_placed_through_the_connector() -> Result<(), Error> {
    let seam = seam(CASH);
    let mut broker = broker(seam.tools)?;
    let order = opening(OPENING)?;
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
        ("rh-1", Some(OPENING), order.qty, Qty::ZERO)
    );
    let place = opening_place()?;
    assert_eq!(sent(&seam.calls, "place_equity_order"), std::slice::from_ref(&place));
    assert_eq!(sent(&seam.calls, "review_equity_order"), [reviewed(&place)]);
    let order_calls: Vec<&str> = seam
        .calls
        .borrow()
        .iter()
        .map(|(tool, _)| *tool)
        .filter(|tool| tool.ends_with("equity_order"))
        .collect();
    assert_eq!(order_calls, ["review_equity_order", "place_equity_order"]);
    Ok(())
}

/// DEC-2386 item 2: the account is read once, one read each of the portfolio, the positions and
/// the orders, and mapped by the shell's own mapping (DEC-933 item 1): buying power above cash
/// reads as the cash, the equity is the cash, the account active at 1×.
#[test]
#[ignore = "pending E21-30"]
fn the_account_is_read_once_and_mapped_by_broker_account() -> Result<(), Error> {
    let seam = seam("1000.5");
    {
        let mut book = seam.book.borrow_mut();
        book.buying_power = "5000".to_owned();
        book.positions = vec![position("3", "85.12")];
    }
    let mut broker = broker(seam.tools)?;
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
    let cash = Usd::parse("1000.5")?;
    let account = &reads.account;
    assert_eq!(
        (
            account.cash,
            account.buying_power,
            account.non_marginable_buying_power,
            account.equity,
            account.multiplier,
            account.status.as_str()
        ),
        (cash, cash, cash, cash, 1, "ACTIVE")
    );
    for tool in ["get_portfolio", "get_equity_positions", "get_equity_orders"] {
        assert_eq!(sent(&seam.calls, tool).len(), 1, "one read of {tool}");
    }
    Ok(())
}

/// What Robinhood does not offer is never sent: the activity feed, the four Alpaca reads and the
/// lookup by client order id are `NotSent`, with nothing called.
#[test]
#[ignore = "pending E21-30"]
fn what_robinhood_does_not_offer_is_never_sent() -> Result<(), Error> {
    let seam = seam(CASH);
    let mut broker = broker(seam.tools)?;
    for request in [
        BrokerRequest::ListActivities {
            since: ActivityCursor(String::new()),
        },
        BrokerRequest::GetAccount,
        BrokerRequest::ListPositions,
        BrokerRequest::ListOpenOrders,
        BrokerRequest::GetOrderByClientId(ClientOrderId::parse(OPENING)?),
    ] {
        assert!(
            matches!(broker.call(&request), Err(ConnectorError::NotSent { .. })),
            "{request:?} is not sent"
        );
    }
    assert!(seam.calls.borrow().is_empty(), "nothing is called");
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

fn pass(journal: &Journal, process: &mut Process, id: &str, second: u32) -> Result<(), Error> {
    let now = format!("2026-09-21T15:{:02}:{:02}Z", second / 60, second % 60);
    support::tick(journal, id, &now)?;
    process.step(at(&now)?);
    Ok(())
}

fn text<'a>(payload: &'a Canon, member: &str) -> Option<&'a str> {
    payload.get(member).and_then(Canon::as_str)
}

/// The fills booked on the account stream, `FillApplied` and `LateFillApplied`, of `side`.
fn fills(journal: &Journal, side: &str) -> Vec<Canon> {
    journal
        .payloads("FillApplied")
        .into_iter()
        .chain(journal.payloads("LateFillApplied"))
        .filter(|p| text(p, "side") == Some(side))
        .collect()
}

/// The process started at noon, its agent's opening of 10 AAPL at 150 proposed and placed in the
/// first pass. Answers the journal, the process and the seam.
fn opened() -> Result<(Journal, Process, Seam), Error> {
    let journal: Journal = deployed()?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    let seam = seam(CASH);
    let mut process: Process = ExecutorProcess::start(
        journal.clone(),
        broker(seam.tools.clone())?,
        wiring()?,
        &process_config(Environment::Paper),
        at(NOON)?,
    );
    pass(&journal, &mut process, "t0", 0)?;
    assert_eq!(
        sent(&seam.calls, "place_equity_order"),
        [opening_place()?],
        "the opening is placed as the contract's limit buy"
    );
    Ok((journal, process, seam))
}

/// The broker's record `id` read `filled` of the order's whole quantity.
fn fill_record(seam: &Seam, id: &str) -> Result<(), Error> {
    let mut book = seam.book.borrow_mut();
    let record = book
        .orders
        .iter_mut()
        .find(|record| record.get("id").and_then(Value::as_str) == Some(id))
        .ok_or("the broker lists the order")?;
    let quantity = record.get("quantity").cloned().unwrap_or(Value::Null);
    if let Value::Object(members) = record {
        members.insert("state".to_owned(), json!("filled"));
        members.insert("filled_quantity".to_owned(), quantity);
    }
    Ok(())
}

/// The broker after the opening filled: the account holds the 10 at [`AVERAGE`], the cash is
/// [`SPENT`], and the opening's record reads it filled.
fn filled(seam: &Seam) -> Result<(), Error> {
    fill_record(seam, "rh-1")?;
    let mut book = seam.book.borrow_mut();
    book.cash = SPENT.to_owned();
    book.buying_power = SPENT.to_owned();
    book.positions = vec![position("10", AVERAGE)];
    Ok(())
}

/// After [`opened`] and [`filled`], passes ten seconds apart that book the fill and place the
/// protective stop, which the broker then lists as working.
fn protected_position() -> Result<(Journal, Process, Seam), Error> {
    let (journal, mut process, seam) = opened()?;
    filled(&seam)?;
    for (second, id) in [(10, "t1"), (20, "t2"), (30, "t3")] {
        pass(&journal, &mut process, id, second)?;
    }
    Ok((journal, process, seam))
}

/// Every place of a stop-limit the seam was handed, as its arguments.
fn stop_places(calls: &Calls) -> Vec<Value> {
    sent(calls, "place_equity_order")
        .into_iter()
        .filter(|arguments| arguments.get("type").and_then(Value::as_str) == Some("stop_limit"))
        .collect()
}

/// The `ReconciliationRun`s journaled from the one that booked `side`'s fill on, in order.
fn runs_from_the_fill(journal: &Journal, side: &str) -> Result<Vec<Canon>, Error> {
    let types = journal.types(ACCOUNT_STREAM);
    let fill_at = journal
        .rows_of(ACCOUNT_STREAM)
        .iter()
        .position(|row| {
            matches!(row.event_type.as_str(), "FillApplied" | "LateFillApplied")
                && folded(row).is_ok_and(|event| text(&event.payload, "side") == Some(side))
        })
        .ok_or("the fill is booked")?;
    let before = types[..fill_at]
        .iter()
        .filter(|kind| *kind == "ReconciliationRun")
        .count();
    Ok(journal
        .payloads("ReconciliationRun")
        .into_iter()
        .skip(before)
        .collect())
}

/// DEC-2386 item 4: no reconciliation moves the activity cursor, since Robinhood publishes none;
/// none mismatches; and the cash is in band in every snapshot, and checked in at least one.
fn reconciled_without_activity(journal: &Journal) {
    let runs = journal.payloads("ReconciliationRun");
    assert!(
        runs.iter()
            .all(|run| run.get("checkpoint") == Some(&Canon::Null)),
        "no cursor is ever recorded: {runs:?}"
    );
    assert!(
        runs.iter()
            .all(|run| text(run, "result") != Some("mismatch")),
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
        snapshots
            .iter()
            .any(|s| s.get("cash_in_band") == Some(&Canon::Bool(true))),
        "the cash is checked in band at least once: {snapshots:?}"
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
        "no agent's mode changes: nothing is held or paused"
    );
}

/// DEC-2386 item 5, end to end: an agent's opening of 10 AAPL at 150, handed to a process over the
/// Robinhood broker, is placed as the contract's limit buy; once the broker's order record reads it
/// filled and the account holds the 10 at 149.5, the process books exactly one fill of the
/// agent's, from the record and the position at their average (DEC-1151), protects the position
/// with one resting GTC stop-limit sell at the mandate's stop, the profile's only protection (no
/// OCO, DEC-966), and reconciles: the run that books the fill adopts it, and every later run is
/// clean.
#[test]
#[ignore = "pending E21-30"]
fn an_opening_fills_is_booked_protected_and_reconciles_clean() -> Result<(), Error> {
    let (journal, _process, seam) = protected_position()?;
    let bought = fills(&journal, "buy");
    let [fill] = bought.as_slice() else {
        return Err(format!("exactly one buy fill: {bought:?}").into());
    };
    assert_eq!(
        (
            text(fill, "fill_id"),
            text(fill, "client_order_id"),
            text(fill, "qty_gross"),
            text(fill, "price")
        ),
        (
            Some(format!("record:{OPENING}:10").as_str()),
            Some(OPENING),
            Some("10"),
            Some(AVERAGE)
        )
    );
    let stops = stop_places(&seam.calls);
    let [stop] = stops.as_slice() else {
        return Err(format!("one protective stop-limit is placed: {stops:?}").into());
    };
    let mut stop = stop.clone();
    let stop_ref = stop
        .as_object_mut()
        .and_then(|members| members.remove("ref_id"))
        .ok_or("the stop carries a ref_id")?;
    assert_ne!(
        Some(&stop_ref),
        opening_place()?.get("ref_id"),
        "the stop has its own ref_id"
    );
    assert_eq!(
        stop,
        json!({"account_number": ACCOUNT_NUMBER, "symbol": "AAPL", "side": "sell",
            "type": "stop_limit", "quantity": "10", "stop_price": STOP,
            "limit_price": STOP_LIMIT, "time_in_force": "gtc", "market_hours": "regular_hours"})
    );
    assert!(Price::parse(STOP_LIMIT)? <= Price::parse(STOP)?);
    assert!(
        journal
            .payloads("ProtectionChanged")
            .iter()
            .any(|p| text(p, "action") == Some("placed")),
        "the stop is the position's protection: {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    let runs = runs_from_the_fill(&journal, "buy")?;
    let (booking, later) = runs.split_first().ok_or("a run books the fill")?;
    assert_eq!(text(booking, "result"), Some("adopted"), "{runs:?}");
    assert!(!later.is_empty(), "the process reconciles again");
    assert!(
        later.iter().all(|run| text(run, "result") == Some("clean")),
        "{runs:?}"
    );
    reconciled_without_activity(&journal);
    Ok(())
}

/// DEC-2300 and DEC-2395 under the process: the resting stop sells the 10 at the broker, which then
/// lists the position as a row of quantity zero (DEC-902 item 3) and the stop's record filled. The
/// process lists the stop, books exactly one sell fill at the stop's limit, never its trigger, as
/// `limit:<id>:10`, and reconciles with no mismatch and no agent held: the ledger is flat, as the
/// broker is.
#[test]
#[ignore = "pending E21-30"]
fn the_resting_stop_sells_out_and_is_booked_at_its_limit() -> Result<(), Error> {
    let (journal, mut process, seam) = protected_position()?;
    assert_eq!(stop_places(&seam.calls).len(), 1, "one stop is placed");
    fill_record(&seam, "rh-2")?;
    {
        let mut book = seam.book.borrow_mut();
        book.cash = SOLD.to_owned();
        book.buying_power = SOLD.to_owned();
        book.positions = vec![position("0", AVERAGE)];
    }
    for (second, id) in [(40, "t4"), (50, "t5"), (60, "t6")] {
        pass(&journal, &mut process, id, second)?;
    }
    let sold = fills(&journal, "sell");
    let [fill] = sold.as_slice() else {
        return Err(format!("exactly one sell fill: {sold:?}").into());
    };
    assert_eq!(
        (text(fill, "qty_gross"), text(fill, "price")),
        (Some("10"), Some(STOP_LIMIT))
    );
    assert!(
        text(fill, "fill_id").is_some_and(|id| id.starts_with("limit:") && id.ends_with(":10")),
        "{fill:?}"
    );
    reconciled_without_activity(&journal);
    Ok(())
}

/// DEC-2800 item 2: a listing that goes unanswered holds. The opening filled at the broker, but
/// while its listing is lost nothing is adopted or booked, nothing mismatches and no agent is
/// held; once the listing answers, the fill is booked.
#[test]
#[ignore = "pending E21-30"]
fn an_unanswered_listing_holds_and_never_adopts() -> Result<(), Error> {
    let (journal, mut process, seam) = opened()?;
    filled(&seam)?;
    seam.listing_lost.set(true);
    pass(&journal, &mut process, "t1", 10)?;
    pass(&journal, &mut process, "t2", 20)?;
    assert!(
        fills(&journal, "buy").is_empty(),
        "nothing is booked: {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    assert!(
        journal
            .payloads("OrderStateChanged")
            .iter()
            .all(|p| text(p, "state") != Some("filled")),
        "the opening is not adopted as filled"
    );
    assert!(
        !journal
            .payloads("ReconciliationRun")
            .iter()
            .any(|run| text(run, "result") == Some("mismatch")),
        "no reconciliation reads the position unexplained"
    );
    assert!(
        journal.payloads("AgentModeApplied").is_empty(),
        "no agent is held"
    );
    seam.listing_lost.set(false);
    pass(&journal, &mut process, "t3", 30)?;
    pass(&journal, &mut process, "t4", 40)?;
    assert_eq!(
        fills(&journal, "buy").len(),
        1,
        "once answered, the fill is booked"
    );
    Ok(())
}

/// After a restart the broker is rebuilt from the account stream: the opening's key is never
/// placed again, so a second `Submit` of it sends nothing and answers `Unknown` (DEC-860 items 4
/// and 7).
#[test]
#[ignore = "pending E21-30"]
fn a_restored_broker_never_places_a_journaled_key_again() -> Result<(), Error> {
    let (journal, _process, _seam) = protected_position()?;
    let records = journal
        .rows_of(ACCOUNT_STREAM)
        .iter()
        .map(|row| folded(row).map_err(|_| "a readable row"))
        .collect::<Result<Vec<_>, _>>()?;
    let fresh = seam(SPENT);
    let mut restored =
        RobinhoodBroker::restore(fresh.tools, ACCOUNT_NUMBER.to_owned(), tickers()?, &records)?;
    let again = restored.call(&BrokerRequest::Submit(opening(OPENING)?));
    assert!(
        matches!(again, Err(ConnectorError::Unknown(_))),
        "{again:?}"
    );
    assert!(
        sent(&fresh.calls, "place_equity_order").is_empty(),
        "nothing is placed"
    );
    Ok(())
}
