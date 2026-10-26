//! The runtime's own rows for an owner's exit of one instrument, as `mandate-daemon`'s test writes
//! them to `fixtures/contracts/owner-exit-agent-stream.json`, read by this process's reader
//! (DEC-2387, DEC-2040). The two processes may not link each other (`xtask/layers.toml`), so the
//! file is their contract: a drift on either side fails one of the two tests. The expected
//! proposals come from the owner's command the daemon's test committed (a bid of 99 for 100, floor
//! 95, step-up authenticated ten seconds or 1,000 seconds before `NOW`, and the agent's 3 shares),
//! not from this reader.

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::{Value, parse};
use mandate_executor::{EventId, FoldedEvent, Purpose, RiskClock, Seq, TimeInForce};
use mandate_executord::follow::{Proposal, proposals};
use mandate_num::{Price, Qty};

const CONTRACT: &str = include_str!("../../../fixtures/contracts/owner-exit-agent-stream.json");

/// The daemon test's `NOW`, the second the owner committed each exit.
const NOW: i64 = 1_792_155_600;

fn rows_of(case: &Value) -> Vec<FoldedEvent> {
    let rows = match case.get("rows") {
        Some(Value::Array(rows)) => rows.clone(),
        _ => Vec::new(),
    };
    rows.iter()
        .zip(1u64..)
        .map(|(row, seq)| {
            let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
            FoldedEvent {
                stream: "agent:ws1:agent-a".to_owned(),
                seq: Seq(seq),
                event_id: EventId(text("event_id").unwrap_or_default()),
                event_type: text("event_type").unwrap_or_default(),
                causation_id: text("causation_id").map(EventId),
                payload: row.get("payload").cloned().unwrap_or(Value::Null),
            }
        })
        .collect()
}

fn case(name: &str) -> Vec<FoldedEvent> {
    let document = parse(CONTRACT.as_bytes()).unwrap();
    let Some(Value::Array(cases)) = document.get("cases") else {
        panic!("the contract lists its cases");
    };
    let found = cases
        .iter()
        .find(|case| case.get("name").and_then(Value::as_str) == Some(name))
        .unwrap_or_else(|| panic!("the contract holds {name}"));
    rows_of(found)
}

/// The one proposal the case's rows give, which must be the sale of the agent's 3 `spy` at the
/// market for the day, as an owner's exit, identified by its `IntentProposed`.
fn the_sale(name: &str) -> Proposal {
    let rows = case(name);
    let read = proposals(&rows).unwrap();
    assert_eq!(read.len(), 1, "{name}: {read:?}");
    let sale = read[0].clone();
    let leg = rows
        .iter()
        .find(|row| row.event_type == "IntentProposed")
        .unwrap();
    assert_eq!(sale.intent, leg.event_id, "{name}");
    assert_eq!(sale.instrument, InstrumentId::new("spy").unwrap(), "{name}");
    assert_eq!(sale.side, Side::Sell, "{name}");
    assert_eq!(sale.qty, Qty::parse("3").unwrap(), "{name}");
    assert_eq!(sale.tif, TimeInForce::Day, "{name}");
    assert_eq!(sale.purpose, Purpose::OwnerExit, "{name}");
    sale
}

#[test]
fn an_unconfirmed_owner_exit_goes_at_the_market_with_nothing_confirmed() {
    let sale = the_sale("unconfirmed");
    assert_eq!(sale.limit, None);
    assert_eq!(sale.confirmation, None);
}

/// DEC-2040 items 1, 4 and 7: valid step-up hands the confirmed bid, its size and the floor, from
/// the copy the leg is caused by, expiring 300 s after the step-up, and no limit.
#[test]
fn a_stepped_up_owner_exit_is_handed_its_confirmation() {
    let rows = case("stepped_up");
    let copy = rows
        .iter()
        .find(|row| row.event_type == "OwnerExitRequested")
        .unwrap()
        .event_id
        .clone();
    let sale = the_sale("stepped_up");
    assert_eq!(sale.limit, None);
    let handed = sale
        .confirmation
        .expect("valid step-up hands a confirmation");
    assert_eq!(handed.bid, Price::parse("99").unwrap());
    assert_eq!(handed.bid_size, Qty::parse("100").unwrap());
    assert_eq!(handed.floor, Price::parse("95").unwrap());
    assert_eq!(handed.expires_at, RiskClock::from_secs(NOW - 10 + 300));
    assert_eq!(handed.copied_from, copy);
}

/// Mandate spec §6.1: a confirmed bid with step-up already stale at commit unlocks nothing; the
/// exit still goes, at the floor the owner confirmed, and waits for the session.
#[test]
fn a_stale_owner_exit_goes_at_its_floor_with_no_confirmation() {
    let sale = the_sale("stale");
    assert_eq!(sale.limit, Some(Price::parse("95").unwrap()));
    assert_eq!(sale.confirmation, None);
}
