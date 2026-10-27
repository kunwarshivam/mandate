//! The fixture paper broker as the process sees it, through `mandate-alpaca`'s client (DEC-2007
//! item 1): an active account, nothing held, an order that rests, a duplicate id refused as Alpaca
//! refuses it, a lookup that finds or misses, and a cancel. It answers in process, so no request
//! leaves the test.

use mandate_accounting::{InstrumentId, Side};
use mandate_executor::{
    ActivityCursor, BrokerOutcome, BrokerRequest, ClientOrderId, OrderType, Purpose, SubmitOrder,
    TimeInForce,
};
use mandate_executord::Broker;
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::paper::{CASH, FixturePaper};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

type Error = Box<dyn std::error::Error>;

fn broker() -> Result<AlpacaBroker<FixturePaper>, Error> {
    Ok(AlpacaBroker::new(FixturePaper::new(
        UtcNanos::parse_rfc3339("2026-09-21T15:00:00Z")?,
    ))?)
}

fn opening(id: &str) -> Result<BrokerRequest, Error> {
    Ok(BrokerRequest::Submit(SubmitOrder {
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
    }))
}

#[test]
fn the_account_is_active_with_the_fixtures_cash_and_holds_nothing() -> Result<(), Error> {
    let mut broker = broker()?;
    let BrokerOutcome::Account(account) = broker.call(&BrokerRequest::GetAccount)? else {
        return Err("an account".into());
    };
    assert_eq!(account.status, "ACTIVE");
    assert_eq!(account.cash.to_string(), CASH);
    assert_eq!(account.buying_power.to_string(), CASH);
    assert!(!account.trading_blocked && !account.account_blocked);
    assert_eq!(
        broker.call(&BrokerRequest::ListPositions)?,
        BrokerOutcome::Positions(Vec::new())
    );
    assert_eq!(
        broker.call(&BrokerRequest::ListOpenOrders)?,
        BrokerOutcome::OpenOrders(Vec::new())
    );
    let since = ActivityCursor(String::new());
    assert!(matches!(
        broker.call(&BrokerRequest::ListActivities { since })?,
        BrokerOutcome::Activities { fills, .. } if fills.is_empty()
    ));
    Ok(())
}

#[test]
fn an_order_rests_is_found_by_its_id_and_cancels() -> Result<(), Error> {
    let mut broker = broker()?;
    let id = "md-01J1NTENT0000000000000000A";
    let BrokerOutcome::Submitted(order) = broker.call(&opening(id)?)? else {
        return Err("a submission".into());
    };
    assert_eq!(order.client_order_id.as_deref(), Some(id));
    assert_eq!(order.status, "new");
    assert_eq!(order.filled_qty, Qty::ZERO);
    assert!(matches!(
        broker.call(&opening(id)?)?,
        BrokerOutcome::DuplicateClientOrderId { client_order_id } if client_order_id == id
    ));
    let lookup = BrokerRequest::GetOrderByClientId(ClientOrderId::parse(id)?);
    assert!(matches!(broker.call(&lookup)?, BrokerOutcome::Order(found) if found.status == "new"));
    let BrokerOutcome::OpenOrders(open) = broker.call(&BrokerRequest::ListOpenOrders)? else {
        return Err("open orders".into());
    };
    assert_eq!(open.len(), 1);
    let cancel = BrokerRequest::Cancel {
        client_order_id: ClientOrderId::parse(id)?,
    };
    assert!(matches!(
        broker.call(&cancel)?,
        BrokerOutcome::CancelAccepted { .. }
    ));
    assert!(
        matches!(broker.call(&lookup)?, BrokerOutcome::Order(found) if found.status == "canceled")
    );
    assert_eq!(
        broker.call(&BrokerRequest::ListOpenOrders)?,
        BrokerOutcome::OpenOrders(Vec::new())
    );
    Ok(())
}

/// Each order gets its own broker id, in UUID form, and a cancel touches only the order it names;
/// a second cancel of a canceled order is refused, as Alpaca refuses it.
#[test]
fn a_cancel_touches_only_its_own_order() -> Result<(), Error> {
    let mut broker = broker()?;
    let first = "md-01J1NTENT0000000000000000A";
    let second = "md-01J1NTENT0000000000000000C";
    let BrokerOutcome::Submitted(a) = broker.call(&opening(first)?)? else {
        return Err("a submission".into());
    };
    let BrokerOutcome::Submitted(b) = broker.call(&opening(second)?)? else {
        return Err("a submission".into());
    };
    assert_ne!(a.broker_order_id, b.broker_order_id);
    for id in [&a.broker_order_id, &b.broker_order_id] {
        let groups: Vec<usize> = id.split('-').map(str::len).collect();
        assert_eq!(groups, [8, 4, 4, 4, 12], "{id}");
    }
    let cancel = BrokerRequest::Cancel {
        client_order_id: ClientOrderId::parse(first)?,
    };
    assert!(matches!(
        broker.call(&cancel)?,
        BrokerOutcome::CancelAccepted { .. }
    ));
    let BrokerOutcome::OpenOrders(open) = broker.call(&BrokerRequest::ListOpenOrders)? else {
        return Err("open orders".into());
    };
    let open: Vec<_> = open
        .iter()
        .filter_map(|o| o.client_order_id.as_deref())
        .collect();
    assert_eq!(open, [second]);
    assert!(!matches!(
        broker.call(&cancel),
        Ok(BrokerOutcome::CancelAccepted { .. })
    ));
    Ok(())
}

#[test]
fn an_order_it_never_took_is_absent() -> Result<(), Error> {
    let mut broker = broker()?;
    let id = "md-01J1NTENT0000000000000000B";
    let lookup = BrokerRequest::GetOrderByClientId(ClientOrderId::parse(id)?);
    assert!(matches!(
        broker.call(&lookup)?,
        BrokerOutcome::Absent { client_order_id } if client_order_id == id
    ));
    Ok(())
}

#[test]
fn its_profile_is_alpacas() -> Result<(), Error> {
    assert_eq!(broker()?.profile()?, mandate_alpaca::alpaca_profile()?);
    Ok(())
}
