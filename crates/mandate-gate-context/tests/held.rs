//! The agent's sub-ledger (DEC-2378 item 7): what the agent's own fills hold, which a flatten sells
//! with or without the facts an opening needs (`AGENTS.md` rule 13).

use std::collections::BTreeMap;

use mandate_canon::{Key, Value};
use mandate_gate_context::{Journaled, Row};
use mandate_num::Qty;
use mandate_time::UtcNanos;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const CLOCK: &str = "clock:ws1";
const AGENT: &str = "agent-1";
const ASSET: &str = "us_equity:AAPL";
const START: i64 = 1_790_000_000;

fn object(members: &[(&str, &str)]) -> Value {
    Value::Object(
        members
            .iter()
            .map(|(key, value)| (Key::new(key).unwrap(), Value::Str((*value).to_owned())))
            .collect(),
    )
}

fn on(state: &mut Journaled, event_type: &str, payload: &Value) -> bool {
    let row = Row {
        stream: ACCOUNT,
        event_type,
        payload,
    };
    state.fold(row).is_ok()
}

fn clock() -> String {
    UtcNanos::from_parts(START, 0).unwrap().to_string()
}

fn submitted(state: &mut Journaled, agent: &str, order: &str) {
    let clock = clock();
    let request = object(&[
        ("agent_id", agent),
        ("purpose", "open"),
        ("risk_clock", &clock),
    ]);
    assert!(on(state, "OrderRequestRecorded", &request));
    let order = object(&[
        ("client_order_id", order),
        ("instrument_id", ASSET),
        ("side", "buy"),
    ]);
    assert!(on(state, "OrderSubmitted", &order));
}

fn fill(id: &str, order: &str, side: &str, qty: &str) -> Value {
    let clock = clock();
    object(&[
        ("fill_id", id),
        ("client_order_id", order),
        ("instrument_id", ASSET),
        ("side", side),
        ("qty_gross", qty),
        ("price", "190"),
        ("risk_clock", &clock),
    ])
}

fn held(state: &Journaled) -> BTreeMap<String, Qty> {
    state.held().clone()
}

fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap()
}

#[test]
fn the_agents_own_fills_are_its_sub_ledger() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(&mut state, "FillApplied", &fill("f1", "o1", "buy", "3")));
    assert!(on(
        &mut state,
        "LateFillApplied",
        &fill("f2", "o1", "buy", "2")
    ));
    assert_eq!(held(&state), BTreeMap::from([(ASSET.to_owned(), qty("5"))]));
    submitted(&mut state, AGENT, "o2");
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f3", "o2", "sell", "1.5")
    ));
    assert_eq!(
        held(&state),
        BTreeMap::from([(ASSET.to_owned(), qty("3.5"))])
    );
}

#[test]
fn a_fill_seen_twice_counts_once() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(&mut state, "FillApplied", &fill("f1", "o1", "buy", "3")));
    assert!(on(
        &mut state,
        "LateFillApplied",
        &fill("f1", "o1", "buy", "3")
    ));
    assert_eq!(held(&state), BTreeMap::from([(ASSET.to_owned(), qty("3"))]));
}

#[test]
fn another_agents_orders_and_unnamed_fills_are_not_held() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, "agent-2", "o9");
    assert!(on(&mut state, "FillApplied", &fill("f1", "o9", "buy", "3")));
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f2", "o-unknown", "buy", "4")
    ));
    assert!(held(&state).is_empty());
}

#[test]
fn a_position_sold_out_is_no_longer_held() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(&mut state, "FillApplied", &fill("f1", "o1", "buy", "3")));
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f2", "o1", "sell", "3")
    ));
    assert!(held(&state).is_empty());
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f3", "o1", "sell", "1")
    ));
    assert!(held(&state).is_empty());
}

#[test]
fn an_order_submitted_with_no_request_before_it_is_not_the_agents() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    let order = object(&[("client_order_id", "o2"), ("instrument_id", ASSET)]);
    assert!(on(&mut state, "OrderSubmitted", &order));
    assert!(on(&mut state, "FillApplied", &fill("f1", "o2", "buy", "3")));
    assert!(held(&state).is_empty());
}

#[test]
fn an_unreadable_fill_or_order_row_is_refused() {
    let broken = [
        ("FillApplied", fill("f1", "o1", "hold", "3")),
        ("FillApplied", fill("f1", "o1", "buy", "-3")),
        ("FillApplied", object(&[("client_order_id", "o1")])),
        ("OrderSubmitted", object(&[("instrument_id", ASSET)])),
        ("OrderRequestRecorded", object(&[("purpose", "open")])),
    ];
    for (event_type, payload) in broken {
        let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
        if event_type == "FillApplied" {
            submitted(&mut state, AGENT, "o1");
        }
        assert!(
            !on(&mut state, event_type, &payload),
            "{event_type} {payload:?}"
        );
    }
}

/// Each of the three rows on another account's stream, the other two on this one, leaves the
/// fill unattributed.
#[test]
fn orders_and_fills_on_another_stream_are_not_the_agents() {
    let clock = clock();
    let rows = || {
        [
            (
                "OrderRequestRecorded",
                object(&[("agent_id", AGENT), ("risk_clock", &clock)]),
            ),
            ("OrderSubmitted", object(&[("client_order_id", "o1")])),
            ("FillApplied", fill("f1", "o1", "buy", "3")),
        ]
    };
    for elsewhere in 0..3 {
        let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
        for (index, (event_type, payload)) in rows().into_iter().enumerate() {
            let stream = if index == elsewhere {
                "acct:ws1:alpaca:other"
            } else {
                ACCOUNT
            };
            let row = Row {
                stream,
                event_type,
                payload: &payload,
            };
            assert!(state.fold(row).is_ok());
        }
        assert!(held(&state).is_empty(), "{elsewhere}");
    }
}
