//! X1b-3: the fixture paper broker that fills (`FixturePaper::filling`), through `mandate-alpaca`'s
//! client and then under a whole executor process. A limit order fills whole at its limit when it
//! is placed; a bracket's two legs rest under its filled entry; OCO protection and stops rest; each
//! fill is a `FILL` activity naming only the broker's order id, as Alpaca's does (DEC-2353), and
//! moves the position and the cash by its notional (Alpaca paper does not simulate regulatory
//! fees, trading spec §11, so none is charged). The process then journals the order, books the fill, keeps the bracket's legs as the
//! position's protection, and reconciles clean.

mod support;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value;
use mandate_executor::{
    ActivityCursor, BracketLegs, BrokerOutcome, BrokerRequest, ClientOrderId, ExitTier,
    FillConfigRefs, InstrumentSnapshot, OcoLegs, OrderType, Purpose, SubmitOrder, TimeInForce,
};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::marks::NoMarks;
use mandate_executord::paper::{CASH, FixturePaper};
use mandate_executord::quotes::NoQuotes;
use mandate_executord::risk::Unopened;
use mandate_executord::{Broker, ExecutorProcess, Wiring};
use mandate_journal::Environment;
use mandate_num::{Price, Qty, ShareIncrement, Usd};
use support::{
    ACCOUNT_STREAM, AGENT, Allowing, Error, Ids, Instruments, Journal, Mandate, NOON,
    TwoPercentStop, at, config, deployed, fees, process_config, propose,
};

const OPEN: &str = "01JINTENT0000000000000000A";
const REF: &str = "sha256:6666666666666666666666666666666666666666666666666666666666666666";

fn broker() -> Result<AlpacaBroker<FixturePaper>, Error> {
    Ok(AlpacaBroker::new(FixturePaper::filling(at(NOON)?))?)
}

fn order(id: &str, side: Side, qty: &str, limit: &str) -> Result<SubmitOrder, Error> {
    Ok(SubmitOrder {
        client_order_id: ClientOrderId::parse(id)?,
        instrument: InstrumentId::new("AAPL")?,
        side,
        qty: Qty::parse(qty)?,
        order_type: OrderType::Limit,
        tif: TimeInForce::Day,
        limit_price: Some(Price::parse(limit)?),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: if side == Side::Buy {
            Purpose::Open
        } else {
            Purpose::RiskExit
        },
    })
}

fn submit(
    broker: &mut AlpacaBroker<FixturePaper>,
    order: SubmitOrder,
) -> Result<mandate_executor::BrokerOrder, Error> {
    match broker.call(&BrokerRequest::Submit(order))? {
        BrokerOutcome::Submitted(placed) => Ok(placed),
        other => Err(format!("a submission, not {other:?}").into()),
    }
}

fn fills(
    broker: &mut AlpacaBroker<FixturePaper>,
    since: &str,
) -> Result<(Vec<mandate_executor::BrokerFill>, ActivityCursor), Error> {
    let since = ActivityCursor(since.to_owned());
    match broker.call(&BrokerRequest::ListActivities { since })? {
        BrokerOutcome::Activities { fills, cursor } => Ok((fills, cursor)),
        other => Err(format!("activities, not {other:?}").into()),
    }
}

fn held(broker: &mut AlpacaBroker<FixturePaper>) -> Result<Vec<(String, String, String)>, Error> {
    match broker.call(&BrokerRequest::ListPositions)? {
        BrokerOutcome::Positions(positions) => Ok(positions
            .iter()
            .map(|p| {
                (
                    p.instrument.as_str().to_owned(),
                    p.qty.to_string(),
                    p.avg_entry_price.to_string(),
                )
            })
            .collect()),
        other => Err(format!("positions, not {other:?}").into()),
    }
}

fn cash(broker: &mut AlpacaBroker<FixturePaper>) -> Result<(String, String), Error> {
    match broker.call(&BrokerRequest::GetAccount)? {
        BrokerOutcome::Account(account) => {
            Ok((account.cash.to_string(), account.accrued_fees.to_string()))
        }
        other => Err(format!("an account, not {other:?}").into()),
    }
}

fn open_ids(broker: &mut AlpacaBroker<FixturePaper>) -> Result<Vec<String>, Error> {
    match broker.call(&BrokerRequest::ListOpenOrders)? {
        BrokerOutcome::OpenOrders(orders) => Ok(orders
            .into_iter()
            .map(|order| order.broker_order_id)
            .collect()),
        other => Err(format!("open orders, not {other:?}").into()),
    }
}

/// A limit opening fills whole at its limit when it is placed. The submission's answer is the order
/// as Alpaca accepts it, `new` and unfilled; a later lookup reads it `filled`, one `FILL` activity
/// names the broker's order id and no client order id (Alpaca's shape, DEC-2353),
/// the cursor it returns reads nothing new, the position is the fill, and the cash falls by its
/// notional with no fee accrued (Alpaca paper does not simulate regulatory fees, trading spec §11).
#[test]
fn a_limit_opening_fills_whole_at_its_limit() -> Result<(), Error> {
    let mut broker = broker()?;
    let placed = submit(
        &mut broker,
        order("md-01JINTENT0000000000000000A", Side::Buy, "10", "150")?,
    )?;
    assert_eq!(
        (placed.status.as_str(), placed.filled_qty),
        ("new", Qty::ZERO)
    );
    let lookup =
        BrokerRequest::GetOrderByClientId(ClientOrderId::parse("md-01JINTENT0000000000000000A")?);
    let BrokerOutcome::Order(found) = broker.call(&lookup)? else {
        return Err("the opening".into());
    };
    assert_eq!(found.status, "filled");
    assert_eq!(found.filled_qty, Qty::parse("10")?);
    let (booked, cursor) = fills(&mut broker, "")?;
    assert_eq!(booked.len(), 1);
    let fill = &booked[0];
    assert_eq!(
        fill.broker_order_id.as_deref(),
        Some(placed.broker_order_id.as_str())
    );
    assert_eq!(fill.client_order_id, None);
    assert_eq!(
        (fill.side, fill.qty, fill.price),
        (Side::Buy, Qty::parse("10")?, Price::parse("150")?)
    );
    assert!(
        fills(&mut broker, &cursor.0)?.0.is_empty(),
        "the cursor resumes after the fill"
    );
    assert_eq!(
        held(&mut broker)?,
        [("AAPL".to_owned(), "10".to_owned(), "150".to_owned())]
    );
    let spent = Usd::parse(CASH)?.checked_sub(Usd::parse("1500")?)?;
    assert_eq!(cash(&mut broker)?, (spent.to_string(), "0".to_owned()));
    Ok(())
}

/// A bracket's entry fills, and its two legs rest under it for the entry's quantity, as the
/// executor's reconciliation reads a resting bracket (`bracket_rests_under`): one stop at the
/// recorded stop and one sell limit at the take-profit, `new`, nested in the open orders' listing
/// and in the lookup of the entry by its own id.
#[test]
fn a_brackets_legs_rest_under_its_filled_entry() -> Result<(), Error> {
    let mut broker = broker()?;
    let id = "md-01JINTENT0000000000000000A";
    let mut entry = order(id, Side::Buy, "10", "150")?;
    entry.bracket = Some(BracketLegs {
        stop: Price::parse("147")?,
        take_profit: Price::parse("157.5")?,
    });
    submit(&mut broker, entry)?;
    let BrokerOutcome::OpenOrders(open) = broker.call(&BrokerRequest::ListOpenOrders)? else {
        return Err("open orders".into());
    };
    let lookup = BrokerRequest::GetOrderByClientId(ClientOrderId::parse(id)?);
    let BrokerOutcome::Order(found) = broker.call(&lookup)? else {
        return Err("the entry".into());
    };
    for parent in [open.first().ok_or("the entry is listed")?, &found] {
        assert_eq!(parent.client_order_id.as_deref(), Some(id));
        assert_eq!(parent.status, "filled");
        assert_eq!(parent.legs.len(), 2);
        let stop = parent
            .legs
            .iter()
            .find(|leg| leg.stop_price.is_some())
            .ok_or("a stop leg")?;
        let take = parent
            .legs
            .iter()
            .find(|leg| leg.stop_price.is_none())
            .ok_or("a take-profit leg")?;
        assert_eq!(stop.stop_price, Some(Price::parse("147")?));
        assert_eq!(take.limit_price, Some(Price::parse("157.5")?));
        for leg in [stop, take] {
            assert_eq!(
                (leg.side, leg.qty, leg.filled_qty),
                (Side::Sell, Qty::parse("10")?, Qty::ZERO)
            );
            assert_eq!(leg.status, "new");
        }
    }
    assert_eq!(fills(&mut broker, "")?.0.len(), 1, "only the entry fills");
    Ok(())
}

/// A sell limit fills at its limit too: an exit of 4 of the 10 leaves 6, at the buy's average
/// entry, and brings back its notional.
#[test]
fn a_sell_limit_fills_and_reduces_the_position() -> Result<(), Error> {
    let mut broker = broker()?;
    submit(
        &mut broker,
        order("md-01JINTENT0000000000000000A", Side::Buy, "10", "150")?,
    )?;
    let sold = submit(
        &mut broker,
        order("md-01JINTENT0000000000000000C", Side::Sell, "4", "155")?,
    )?;
    assert_eq!(sold.status, "new", "the answer is the order as accepted");
    assert_eq!(fills(&mut broker, "")?.0.len(), 2);
    assert_eq!(
        held(&mut broker)?,
        [("AAPL".to_owned(), "6".to_owned(), "150".to_owned())]
    );
    let left = Usd::parse(CASH)?
        .checked_sub(Usd::parse("1500")?)?
        .checked_add(Usd::parse("620")?)?;
    assert_eq!(cash(&mut broker)?.0, left.to_string());
    Ok(())
}

/// Protection never fills here, since the fixture has no price: an OCO's two orders rest `new`,
/// unfilled, and cancel; then a lone stop-limit over the same shares rests `new` too.
#[test]
fn protection_rests_unfilled() -> Result<(), Error> {
    let mut broker = broker()?;
    submit(
        &mut broker,
        order("md-01JINTENT0000000000000000A", Side::Buy, "10", "150")?,
    )?;
    let mut oco = order(
        "md-01JINTENT0000000000000000A-p1",
        Side::Sell,
        "10",
        "157.5",
    )?;
    oco.limit_price = None;
    oco.purpose = Purpose::Protective;
    oco.tif = TimeInForce::Gtc;
    oco.oco = Some(OcoLegs {
        take_profit: Price::parse("157.5")?,
        stop: Price::parse("147")?,
        qty: Qty::parse("10")?,
    });
    let placed = submit(&mut broker, oco)?;
    assert_eq!(
        (placed.status.as_str(), placed.filled_qty),
        ("new", Qty::ZERO)
    );
    let cancel = BrokerRequest::Cancel {
        client_order_id: ClientOrderId::parse("md-01JINTENT0000000000000000A-p1")?,
    };
    assert!(
        matches!(broker.call(&cancel)?, BrokerOutcome::CancelAccepted { .. }),
        "the OCO cancels"
    );
    let mut stop = order("md-01JINTENT0000000000000000A-p2", Side::Sell, "10", "146")?;
    stop.purpose = Purpose::Protective;
    stop.order_type = OrderType::StopLimit;
    stop.stop_price = Some(Price::parse("147")?);
    let resting = submit(&mut broker, stop)?;
    assert_eq!(resting.status, "new");
    assert_eq!(fills(&mut broker, "")?.0.len(), 1, "only the opening fills");
    assert_eq!(
        held(&mut broker)?,
        [("AAPL".to_owned(), "10".to_owned(), "150".to_owned())]
    );
    Ok(())
}

/// A filled order is not cancelable, as Alpaca refuses it: the cancel's read-back answers the
/// order itself, `filled`, never a confirmed cancel, and the position stays the fill.
#[test]
fn a_filled_order_does_not_cancel() -> Result<(), Error> {
    let mut broker = broker()?;
    let id = "md-01JINTENT0000000000000000A";
    submit(&mut broker, order(id, Side::Buy, "10", "150")?)?;
    let cancel = BrokerRequest::Cancel {
        client_order_id: ClientOrderId::parse(id)?,
    };
    match broker.call(&cancel)? {
        BrokerOutcome::Order(read) => assert_eq!(read.status, "filled"),
        other => return Err(format!("the filled order read back, not {other:?}").into()),
    }
    assert_eq!(
        held(&mut broker)?,
        [("AAPL".to_owned(), "10".to_owned(), "150".to_owned())]
    );
    assert_eq!(cash(&mut broker)?.0, "98500");
    Ok(())
}

/// A `client_order_id` the fixture already holds is refused as Alpaca refuses it, `422`, which the
/// client reads as the order already being there: one fill, one position.
#[test]
fn a_duplicate_client_order_id_is_refused() -> Result<(), Error> {
    let mut broker = broker()?;
    let id = "md-01JINTENT0000000000000000A";
    submit(&mut broker, order(id, Side::Buy, "10", "150")?)?;
    let again = broker.call(&BrokerRequest::Submit(order(id, Side::Buy, "10", "150")?))?;
    assert!(
        matches!(again, BrokerOutcome::DuplicateClientOrderId { .. }),
        "{again:?}"
    );
    assert_eq!(fills(&mut broker, "")?.0.len(), 1);
    assert_eq!(
        held(&mut broker)?,
        [("AAPL".to_owned(), "10".to_owned(), "150".to_owned())]
    );
    Ok(())
}

/// No short sales (`AGENTS.md` rule 12): a sell with no position, one larger than the position,
/// and one larger than what open sells leave available are each refused as Alpaca refuses them,
/// `403` with code 40310000, deciding before anything changes: no order is added, nothing fills,
/// and the book stays readable.
#[test]
fn a_sell_beyond_the_position_is_refused() -> Result<(), Error> {
    let mut broker = broker()?;
    let refused = |outcome: BrokerOutcome| match outcome {
        BrokerOutcome::Rejected(reject) => {
            reject.http_status == 403 && reject.code.as_deref() == Some("40310000")
        }
        _ => false,
    };
    let naked = broker.call(&BrokerRequest::Submit(order(
        "md-01JINTENT0000000000000000S",
        Side::Sell,
        "1",
        "150",
    )?))?;
    assert!(refused(naked.clone()), "{naked:?}");
    assert_eq!(
        open_ids(&mut broker)?,
        Vec::<String>::new(),
        "no order is added"
    );
    assert!(held(&mut broker)?.is_empty(), "no position is made");
    submit(
        &mut broker,
        order("md-01JINTENT0000000000000000A", Side::Buy, "10", "150")?,
    )?;
    let beyond = broker.call(&BrokerRequest::Submit(order(
        "md-01JINTENT0000000000000000B",
        Side::Sell,
        "11",
        "150",
    )?))?;
    assert!(refused(beyond.clone()), "{beyond:?}");
    assert_eq!(
        open_ids(&mut broker)?,
        Vec::<String>::new(),
        "no order is added"
    );
    let mut resting = order("md-01JINTENT0000000000000000C", Side::Sell, "6", "160")?;
    resting.order_type = OrderType::StopLimit;
    resting.stop_price = Some(Price::parse("161")?);
    let resting = submit(&mut broker, resting)?;
    let held_for_orders = broker.call(&BrokerRequest::Submit(order(
        "md-01JINTENT0000000000000000D",
        Side::Sell,
        "5",
        "150",
    )?))?;
    assert!(
        refused(held_for_orders.clone()),
        "6 of the 10 are held for the resting sell: {held_for_orders:?}"
    );
    assert_eq!(open_ids(&mut broker)?, [resting.broker_order_id]);
    assert_eq!(fills(&mut broker, "")?.0.len(), 1, "only the opening fills");
    assert_eq!(
        held(&mut broker)?,
        [("AAPL".to_owned(), "10".to_owned(), "150".to_owned())]
    );
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
        risk: Box::new(Unopened),
    })
}

fn action(payload: &Value) -> Option<&str> {
    payload.get("action").and_then(Value::as_str)
}

type Process = ExecutorProcess<Journal, AlpacaBroker<FixturePaper>>;

/// A process started at noon over `paper`, the agent deployed, and its opening of 10 AAPL at 150
/// proposed when `opening`.
fn running(paper: FixturePaper, opening: bool) -> Result<(Journal, Process), Error> {
    let journal: Journal = deployed()?;
    if opening {
        propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    }
    let process = ExecutorProcess::start(
        journal.clone(),
        AlpacaBroker::new(paper)?,
        wiring()?,
        &process_config(Environment::Paper),
        at(NOON)?,
    );
    Ok((journal, process))
}

/// One pass at `now`, after the scheduler's tick `id` at that time.
fn pass(journal: &Journal, process: &mut Process, id: &str, now: &str) -> Result<(), Error> {
    support::tick(journal, id, now)?;
    process.step(at(now)?);
    Ok(())
}

/// The `ReconciliationRun`s journaled after the first `FillApplied`, in order.
fn runs_after_the_fill(journal: &Journal) -> Result<Vec<Value>, Error> {
    let types = journal.types(ACCOUNT_STREAM);
    let fill = types
        .iter()
        .position(|kind| kind == "FillApplied")
        .ok_or("the fill is booked")?;
    let before = types[..fill]
        .iter()
        .filter(|kind| *kind == "ReconciliationRun")
        .count();
    Ok(journal
        .payloads("ReconciliationRun")
        .into_iter()
        .skip(before)
        .collect())
}

fn result(run: &Value) -> Option<&str> {
    run.get("result").and_then(Value::as_str)
}

/// X1b-3 end to end: an agent's opening of 10 AAPL at 150, handed to a process over the filling
/// fixture, is journaled and sent as a bracket, fills whole, is booked as the agent's fill, keeps
/// the bracket's two legs as the position's protection, and the account reconciles clean: the
/// broker's cash is the model's, and no agent is held. The passes run a minute apart, each after
/// a clock tick, so the process reads the broker again while its order works and books the fill
/// without a restart.
#[test]
fn an_opening_fills_is_booked_protected_and_reconciles_clean() -> Result<(), Error> {
    let (journal, mut process) = running(FixturePaper::filling(at(NOON)?), true)?;
    for minute in 0..10 {
        let now = format!("2026-09-21T15:{minute:02}:00Z");
        pass(&journal, &mut process, &format!("t{minute}"), &now)?;
    }
    let fill = journal
        .payloads("FillApplied")
        .into_iter()
        .find(|p| p.get("side").and_then(Value::as_str) == Some("buy"))
        .ok_or("the fill is booked")?;
    assert_eq!(
        fill.get("client_order_id").and_then(Value::as_str),
        Some(format!("md-{OPEN}").as_str())
    );
    assert_eq!(fill.get("qty_gross").and_then(Value::as_str), Some("10"));
    assert_eq!(fill.get("price").and_then(Value::as_str), Some("150"));
    let protection: Vec<_> = journal.payloads("ProtectionChanged");
    assert!(
        protection.iter().any(|p| action(p) == Some("placed")),
        "the bracket's legs are the position's protection: {:?}",
        journal.types(ACCOUNT_STREAM)
    );
    let runs = runs_after_the_fill(&journal)?;
    let (booking, later) = runs
        .split_first()
        .ok_or("a reconciliation booked the fill")?;
    assert_eq!(
        result(booking),
        Some("adopted"),
        "the run that books the fill adopts the broker's order state and its fill, and finds no \
         unexplained difference: {runs:?}"
    );
    assert!(
        !later.is_empty(),
        "the process reconciles again while its legs work"
    );
    assert!(
        later.iter().all(|run| result(run) == Some("clean")),
        "every later run is clean, none mismatch or adopted: {runs:?}"
    );
    let snapshots = journal.payloads("AccountSnapshotRecorded");
    assert!(
        snapshots
            .iter()
            .all(|s| s.get("cash_in_band") != Some(&Value::Bool(false))),
        "{snapshots:?}"
    );
    assert!(
        !snapshots.is_empty()
            && snapshots.iter().all(|s| {
                s.get("account_type") == Some(&Value::Str("margin".to_owned()))
                    && s.get("day_trading_regime")
                        == Some(&Value::Str("intraday_margin".to_owned()))
                    && s.get("last_equity").is_some()
            }),
        "the process journals its broker's declaration and figures on every snapshot (§9.35): \
         {snapshots:?}"
    );
    assert!(
        journal
            .payloads("AgentModeApplied")
            .iter()
            .all(|p| p.get("to").and_then(Value::as_str) == Some("normal")),
        "no agent is held"
    );
    Ok(())
}

/// The fill's own reconciliation (DEC-948 item 5): one fill, then exactly one reconciliation, ten
/// seconds on, which books it, adopting the broker's order state and its fill (§11) and finding
/// no unexplained difference, its snapshot's cash the model's cash. A snapshot that read its own
/// account report as an older base would count the fill's notional twice and mismatch.
#[test]
fn the_fills_one_reconciliation_books_it_and_its_cash_is_the_models() -> Result<(), Error> {
    let (journal, mut process) = running(FixturePaper::filling(at(NOON)?), true)?;
    pass(&journal, &mut process, "t0", NOON)?;
    pass(&journal, &mut process, "t1", "2026-09-21T15:00:10Z")?;
    let runs = runs_after_the_fill(&journal)?;
    assert_eq!(runs.len(), 1, "{:?}", journal.types(ACCOUNT_STREAM));
    assert_eq!(result(&runs[0]), Some("adopted"), "{runs:?}");
    let snapshot = journal
        .payloads("AccountSnapshotRecorded")
        .pop()
        .ok_or("the run records its snapshot")?;
    assert_eq!(snapshot.get("cash").and_then(Value::as_str), Some("98500"));
    assert_eq!(
        snapshot.get("model_cash").and_then(Value::as_str),
        Some("98500"),
        "{snapshot:?}"
    );
    Ok(())
}

/// The scheduled reconciliation's cadence (X1b-3): with an order working (the resting fixture),
/// a pass nine seconds after the start's reconciliation reads nothing, one at ten seconds
/// reconciles once, another at the same second reads nothing, and the next ten seconds on
/// reconciles once more.
#[test]
fn a_working_order_is_reconciled_every_ten_seconds() -> Result<(), Error> {
    let (journal, mut process) = running(FixturePaper::new(at(NOON)?), true)?;
    pass(&journal, &mut process, "t0", NOON)?;
    let started = journal.payloads("ReconciliationRun").len();
    assert!(
        journal
            .types(ACCOUNT_STREAM)
            .iter()
            .any(|kind| kind == "OrderSubmitted"),
        "the opening works"
    );
    let runs = || journal.payloads("ReconciliationRun").len();
    pass(&journal, &mut process, "t1", "2026-09-21T15:00:09Z")?;
    assert_eq!(runs(), started, "none before ten seconds");
    pass(&journal, &mut process, "t2", "2026-09-21T15:00:10Z")?;
    assert_eq!(runs(), started + 1, "one at ten seconds");
    pass(&journal, &mut process, "t3", "2026-09-21T15:00:10Z")?;
    assert_eq!(runs(), started + 1, "none again in the same second");
    pass(&journal, &mut process, "t4", "2026-09-21T15:00:19Z")?;
    assert_eq!(runs(), started + 1, "none before the next ten seconds");
    pass(&journal, &mut process, "t5", "2026-09-21T15:00:20Z")?;
    assert_eq!(runs(), started + 2, "one more at twenty seconds");
    Ok(())
}

/// With no order working, no scheduled reconciliation runs: only the start's.
#[test]
fn nothing_working_is_never_reconciled_on_schedule() -> Result<(), Error> {
    let (journal, mut process) = running(FixturePaper::new(at(NOON)?), false)?;
    pass(&journal, &mut process, "t0", NOON)?;
    let started = journal.payloads("ReconciliationRun").len();
    for (id, now) in [
        ("t1", "2026-09-21T15:00:10Z"),
        ("t2", "2026-09-21T15:00:20Z"),
        ("t3", "2026-09-21T15:01:00Z"),
    ] {
        pass(&journal, &mut process, id, now)?;
    }
    assert_eq!(journal.payloads("ReconciliationRun").len(), started);
    Ok(())
}

/// E21-30: the fixture fills a marketable limit at the touch, as a venue does, and leaves a limit
/// away from the touch resting. A buy of 2 SPY at 600 fills at the 580.13 ask; a sell of 1 at
/// 551.11, a cell run's stop-priced flatten, fills at the 580.11 bid, never at 551.11; a buy at the
/// ask itself and a sell at the bid itself fill there; a buy at 580.12, inside the spread, a buy at
/// 570 and a sell at 590 rest, open. Before, every limit filled at its own price, so that flatten booked a
/// loss of 29 a share no venue would have, and tripped a drawdown.
#[test]
fn a_marketable_limit_fills_at_the_touch_and_any_other_rests() -> Result<(), Error> {
    let mut broker = broker()?;
    let spy = |id: &str, side: Side, qty: &str, limit: &str| -> Result<SubmitOrder, Error> {
        let mut placed = order(id, side, qty, limit)?;
        placed.instrument = InstrumentId::new("SPY")?;
        Ok(placed)
    };
    for (id, side, qty, limit) in [
        ("md-01JINTENT0000000000000000A", Side::Buy, "2", "600"),
        ("md-01JINTENT0000000000000000B", Side::Sell, "1", "551.11"),
        ("md-01JINTENT0000000000000000C", Side::Buy, "1", "580.13"),
        ("md-01JINTENT0000000000000000D", Side::Sell, "1", "580.11"),
        ("md-01JINTENT0000000000000000E", Side::Buy, "1", "580.12"),
        ("md-01JINTENT0000000000000000F", Side::Buy, "1", "570"),
        ("md-01JINTENT0000000000000000G", Side::Sell, "1", "590"),
    ] {
        submit(&mut broker, spy(id, side, qty, limit)?)?;
    }
    assert_eq!(
        open_ids(&mut broker)?.len(),
        3,
        "the three away from the touch rest"
    );
    let (booked, _) = fills(&mut broker, "")?;
    assert_eq!(
        booked
            .iter()
            .map(|fill| (fill.side, fill.qty.to_string(), fill.price.to_string()))
            .collect::<Vec<_>>(),
        [
            (Side::Buy, "2".to_owned(), "580.13".to_owned()),
            (Side::Sell, "1".to_owned(), "580.11".to_owned()),
            (Side::Buy, "1".to_owned(), "580.13".to_owned()),
            (Side::Sell, "1".to_owned(), "580.11".to_owned()),
        ]
    );
    assert_eq!(
        held(&mut broker)?,
        [("SPY".to_owned(), "1".to_owned(), "580.13".to_owned())]
    );
    Ok(())
}

/// DEC-2813: the fixture takes one of its assets' ids as an order's `symbol`, as Alpaca does, and
/// names that asset by its ticker in the answer, the order read back, the position and the fill;
/// a symbol that is no asset's id stays as sent.
#[test]
fn an_asset_id_symbol_is_answered_by_its_ticker() -> Result<(), Error> {
    let mut broker = broker()?;
    let mut by_id = order("md-01JINTENT0000000000000000A", Side::Buy, "1", "580.13")?;
    by_id.instrument = InstrumentId::new("b28f4066-5c6d-479b-a2af-85dc1a8f16fb")?;
    let placed = submit(&mut broker, by_id)?;
    assert_eq!(placed.instrument.as_str(), "SPY");
    let (booked, _) = fills(&mut broker, "")?;
    assert_eq!(
        booked
            .iter()
            .map(|fill| fill.instrument.as_str())
            .collect::<Vec<_>>(),
        ["SPY"]
    );
    assert_eq!(
        held(&mut broker)?,
        [("SPY".to_owned(), "1".to_owned(), "580.13".to_owned())]
    );
    let mut unknown = order("md-01JINTENT0000000000000000B", Side::Buy, "1", "10")?;
    unknown.instrument = InstrumentId::new("AAPL")?;
    assert_eq!(submit(&mut broker, unknown)?.instrument.as_str(), "AAPL");
    Ok(())
}

/// The agent's bracket opening, filled on the filling fixture and reconciled ten passes a minute
/// apart, then handed `owed` and run six passes more: the journal and every order the process
/// adopted as `unknown` before `owed`.
fn reconciled_then(
    owed: &dyn Fn(&Journal) -> Result<(), Error>,
) -> Result<(Journal, usize), Error> {
    let (journal, mut process) = running(FixturePaper::filling(at(NOON)?), true)?;
    for minute in 0..10 {
        let now = format!("2026-09-21T15:{minute:02}:00Z");
        pass(&journal, &mut process, &format!("t{minute}"), &now)?;
    }
    let unknown = journal
        .payloads("OrderStateChanged")
        .iter()
        .filter(|payload| payload.get("state").and_then(Value::as_str) == Some("unknown"))
        .count();
    owed(&journal)?;
    for minute in 10..16 {
        let now = format!("2026-09-21T15:{minute:02}:00Z");
        pass(&journal, &mut process, &format!("t{minute}"), &now)?;
    }
    Ok((journal, unknown))
}

/// The sell of the agent's 10 shares filled, the account ends flat at the broker and in the
/// model, and no reconciliation found an unexplained difference: the one booking the sell adopts
/// its state and fill (§11), and nothing works after it. The fill is booked as a `FillApplied`, or
/// as a `LateFillApplied` when the order was already terminal when its fill was read, as a kill
/// switch's flatten is once its pending cancel reads it filled.
fn assert_ends_flat_and_explained(journal: &Journal) -> Result<(), Error> {
    let sold = |kind: &str| {
        journal.payloads(kind).iter().any(|fill| {
            fill.get("side").and_then(Value::as_str) == Some("sell")
                && fill.get("qty_gross").and_then(Value::as_str) == Some("10")
        })
    };
    assert!(
        sold("FillApplied") || sold("LateFillApplied"),
        "the sell fills"
    );
    let positions = journal.payloads("BrokerPositionObserved");
    let last = positions.last().ok_or("a position is observed")?;
    assert_eq!(
        (
            last.get("model_qty").and_then(Value::as_str),
            last.get("broker_qty").and_then(Value::as_str)
        ),
        (Some("0"), Some("0")),
        "the account ends flat: {last:?}"
    );
    let runs = journal.payloads("ReconciliationRun");
    assert!(
        runs.iter().all(|run| result(run) != Some("mismatch")),
        "no run finds an unexplained difference: {runs:?}"
    );
    Ok(())
}

/// The client order ids of the sells of 10 the process submitted.
fn sells_of_ten(journal: &Journal) -> Vec<String> {
    journal
        .payloads("OrderSubmitted")
        .iter()
        .filter(|payload| {
            payload.get("side").and_then(Value::as_str) == Some("sell")
                && payload.get("qty").and_then(Value::as_str) == Some("10")
        })
        .filter_map(|payload| {
            payload
                .get("client_order_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

/// The fixture lists a filled bracket's two legs under its entry on every read, so reconciliation
/// after reconciliation adopts neither leg as `unknown`, and a risk exit proposed after ten of
/// them is allowed, goes out and fills: nothing holds it on `unknown_order_in_flight`
/// (`AGENTS.md` rule 13), as the cell's trade loop relies on.
#[test]
fn a_filled_brackets_legs_never_read_unknown_and_a_risk_exit_goes_out() -> Result<(), Error> {
    let exit = "01JINTENT0000000000000000C";
    let (journal, unknown) = reconciled_then(&|journal| {
        propose(journal, AGENT, exit, "sell", "10", "149", "risk_exit")
    })?;
    assert_eq!(unknown, 0, "no leg is adopted unknown");
    assert_eq!(sells_of_ten(&journal), [format!("md-{exit}")]);
    assert!(
        journal
            .payloads("GateDecided")
            .iter()
            .all(|gate| gate.get("reason_code").and_then(Value::as_str)
                != Some("unknown_order_in_flight")),
        "nothing holds on an unknown order"
    );
    let exits: Vec<String> = journal
        .payloads("OrderStateChanged")
        .iter()
        .filter(|change| {
            change.get("client_order_id").and_then(Value::as_str) == Some(&format!("md-{exit}"))
        })
        .filter_map(|change| {
            change
                .get("state")
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(
        exits.last().map(String::as_str),
        Some("filled"),
        "the exit, which the fixture fills on placement and so lists in no open-orders read, may \
         pass through unknown, but its own run books its fill and it ends filled: {exits:?}"
    );
    assert_ends_flat_and_explained(&journal)?;
    Ok(())
}

/// And a kill switch applied after ten reconciliations flattens the agent's 10 shares: its sell
/// goes out and fills, the account ends flat, and no run finds an unexplained difference.
#[test]
fn a_filled_brackets_kill_switch_flattens_after_many_reconciliations() -> Result<(), Error> {
    let (journal, unknown) =
        reconciled_then(&|journal| support::kill(journal, AGENT, "01JKILL00000000000000000AA"))?;
    assert_eq!(unknown, 0, "no leg is adopted unknown");
    assert_eq!(sells_of_ten(&journal).len(), 1, "the switch sells the 10");
    assert!(
        journal
            .payloads("GateDecided")
            .iter()
            .all(|gate| gate.get("reason_code").and_then(Value::as_str)
                != Some("unknown_order_in_flight")),
        "nothing holds on an unknown order"
    );
    assert_ends_flat_and_explained(&journal)
}
