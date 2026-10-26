//! X1b-3: the fixture paper broker that fills (`FixturePaper::filling`), through `mandate-alpaca`'s
//! client and then under a whole executor process. A limit order fills whole at its limit when it
//! is placed; a bracket's two legs rest under its filled entry; OCO protection and stops rest; each
//! fill is a `FILL` activity naming only the broker's order id, as Alpaca's does (DEC-2353), and
//! moves the position and the cash by its notional (paper's fees are simulated, so none is
//! charged). The process then journals the order, books the fill, keeps the bracket's legs as the
//! position's protection, and reconciles clean.

mod support;

use mandate_accounting::{AssetClass, InstrumentId, Side};
use mandate_canon::Value;
use mandate_executor::{
    ActivityCursor, AgentId, BracketLegs, BrokerOutcome, BrokerRequest, ClientOrderId, ExitTier,
    FillConfigRefs, InstrumentSnapshot, OcoLegs, OrderType, Purpose, SubmitOrder, TimeInForce,
};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::paper::{CASH, FixturePaper};
use mandate_executord::quotes::NoQuotes;
use mandate_executord::{Broker, ExecutorProcess, Wiring};
use mandate_journal::Environment;
use mandate_num::{Price, Qty, ShareIncrement, SignedQty, Usd};
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

/// A limit opening fills whole at its limit when it is placed. The submission's answer is the order
/// as Alpaca accepts it, `new` and unfilled; a later lookup reads it `filled`, one `FILL` activity
/// names the broker's order id and no client order id (Alpaca's shape, DEC-2353),
/// the cursor it returns reads nothing new, the position is the fill, and the cash falls by its
/// notional with no fee accrued (paper's fees are simulated).
#[test]
#[ignore = "pending E21-30"]
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
#[ignore = "pending E21-30"]
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
#[ignore = "pending E21-30"]
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

/// Protection never fills here, since the fixture has no price: an OCO's two orders and a lone
/// stop rest `new`, unfilled, and cancel.
#[test]
#[ignore = "pending E21-30"]
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

/// A filled order is not cancelable, as Alpaca refuses it.
#[test]
#[ignore = "pending E21-30"]
fn a_filled_order_does_not_cancel() -> Result<(), Error> {
    let mut broker = broker()?;
    let id = "md-01JINTENT0000000000000000A";
    submit(&mut broker, order(id, Side::Buy, "10", "150")?)?;
    let cancel = BrokerRequest::Cancel {
        client_order_id: ClientOrderId::parse(id)?,
    };
    assert!(!matches!(
        broker.call(&cancel),
        Ok(BrokerOutcome::CancelAccepted { .. })
    ));
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
    })
}

fn action(payload: &Value) -> Option<&str> {
    payload.get("action").and_then(Value::as_str)
}

/// X1b-3 end to end: an agent's opening of 10 AAPL at 150, handed to a process over the filling
/// fixture, is journaled and sent as a bracket, fills whole, is booked as the agent's fill, keeps
/// the bracket's two legs as the position's protection, and the account reconciles clean: the
/// broker's cash is the model's, and no agent is held.
#[test]
#[ignore = "pending E21-30"]
fn an_opening_fills_is_booked_protected_and_reconciles_clean() -> Result<(), Error> {
    let journal: Journal = deployed()?;
    propose(&journal, AGENT, OPEN, "buy", "10", "150", "open")?;
    let mut process = ExecutorProcess::start(
        journal.clone(),
        AlpacaBroker::new(FixturePaper::filling(at(NOON)?))?,
        wiring()?,
        &process_config(Environment::Paper),
        at(NOON)?,
    );
    for _ in 0..4 {
        process.step(at(NOON)?);
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
    let runs = journal.payloads("ReconciliationRun");
    let last = runs.last().ok_or("a reconciliation ran")?;
    assert_eq!(
        last.get("result").and_then(Value::as_str),
        Some("clean"),
        "{runs:?}"
    );
    let snapshots = journal.payloads("AccountSnapshotRecorded");
    assert!(
        snapshots
            .iter()
            .all(|s| s.get("cash_in_band") != Some(&Value::Bool(false))),
        "{snapshots:?}"
    );
    assert!(
        journal
            .payloads("AgentModeApplied")
            .iter()
            .all(|p| p.get("to").and_then(Value::as_str) == Some("normal")),
        "no agent is held"
    );
    let _ = (SignedQty::ZERO, AgentId(AGENT.to_owned()));
    Ok(())
}
