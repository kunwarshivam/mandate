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

fn released(agent: &str, qty: &str) -> Value {
    object(&[
        ("agent_id", agent),
        ("instrument", ASSET),
        ("qty", qty),
        ("cause", "owner_release"),
    ])
}

/// R1's probe on #2432: the agent buys 10 and releases 4 to the owner, so a flatten sells 6 and
/// never the owner's released shares (DEC-2354 invariant 1, DEC-2360 item 4).
#[test]
fn shares_released_to_the_owner_leave_the_sub_ledger() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f1", "o1", "buy", "10")
    ));
    assert!(on(&mut state, "PositionReleased", &released(AGENT, "4")));
    assert_eq!(held(&state), BTreeMap::from([(ASSET.to_owned(), qty("6"))]));
    assert!(on(
        &mut state,
        "PositionReleased",
        &released("agent-2", "5")
    ));
    assert_eq!(held(&state), BTreeMap::from([(ASSET.to_owned(), qty("6"))]));
    assert!(on(&mut state, "PositionReleased", &released(AGENT, "7")));
    assert!(held(&state).is_empty());
}

/// The sub-ledger never exceeds what the account holds, as the executor's `sub_ledger` caps it:
/// an owner's own sell of the agent's shares outside the agent's orders lowers the account.
#[test]
fn the_sub_ledger_is_capped_at_what_the_account_holds() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f1", "o1", "buy", "10")
    ));
    submitted(&mut state, "agent-2", "o2");
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f2", "o2", "sell", "7")
    ));
    assert_eq!(held(&state), BTreeMap::from([(ASSET.to_owned(), qty("3"))]));
}

/// A crypto fee charged in the asset itself leaves the account's position, and so caps the
/// sub-ledger; an equities fee is cash and changes no quantity.
#[test]
fn an_in_kind_crypto_fee_lowers_what_the_account_holds() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(&mut state, "FillApplied", &fill("f1", "o1", "buy", "1")));
    let crypto = object(&[
        ("family", "crypto_asset"),
        ("instrument_id", ASSET),
        ("accrued", "0.0025"),
    ]);
    assert!(on(&mut state, "FeesCharged", &crypto));
    let equities = object(&[("family", "equities"), ("accrued", "0.02")]);
    assert!(on(&mut state, "FeesCharged", &equities));
    assert_eq!(
        held(&state),
        BTreeMap::from([(ASSET.to_owned(), qty("0.9975"))])
    );
}

/// The executor's fold does not interpret a corporate action yet, so neither does this one: a
/// split is refused, which holds every later opening, and the sub-ledger stays the fills' until
/// both folds read splits (DEC-2378 item 7).
#[test]
fn a_corporate_action_is_refused_and_holds_openings() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f1", "o1", "buy", "10")
    ));
    let split = object(&[("instrument_id", ASSET), ("kind", "split")]);
    assert!(!on(&mut state, "CorporateActionApplied", &split));
    assert_eq!(
        held(&state),
        BTreeMap::from([(ASSET.to_owned(), qty("10"))])
    );
}

#[test]
fn an_unreadable_release_or_fee_is_refused() {
    let cases = [
        ("PositionReleased", released(AGENT, "lots")),
        (
            "PositionReleased",
            object(&[("agent_id", AGENT), ("qty", "1")]),
        ),
        (
            "PositionReleased",
            object(&[("instrument", ASSET), ("qty", "1")]),
        ),
        ("FeesCharged", object(&[("accrued", "1")])),
        (
            "FeesCharged",
            object(&[("family", "crypto_asset"), ("accrued", "x")]),
        ),
        (
            "FeesCharged",
            object(&[("family", "crypto_asset"), ("accrued", "1")]),
        ),
    ];
    for (event_type, payload) in cases {
        let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
        assert!(
            !on(&mut state, event_type, &payload),
            "{event_type} {payload:?}"
        );
    }
}

/// A release, an in-kind fee and a corporate action on another account's stream are not this
/// account's.
#[test]
fn releases_fees_and_corporate_actions_on_another_stream_change_nothing() {
    let mut state = Journaled::new(ACCOUNT, CLOCK, AGENT);
    submitted(&mut state, AGENT, "o1");
    assert!(on(
        &mut state,
        "FillApplied",
        &fill("f1", "o1", "buy", "10")
    ));
    let crypto = object(&[
        ("family", "crypto_asset"),
        ("instrument_id", ASSET),
        ("accrued", "1"),
    ]);
    let split = object(&[("instrument_id", ASSET)]);
    for (event_type, payload) in [
        ("PositionReleased", released(AGENT, "4")),
        ("FeesCharged", crypto),
        ("CorporateActionApplied", split),
    ] {
        let row = Row {
            stream: "acct:ws1:alpaca:other",
            event_type,
            payload: &payload,
        };
        assert!(state.fold(row).is_ok(), "{event_type}");
    }
    assert_eq!(
        held(&state),
        BTreeMap::from([(ASSET.to_owned(), qty("10"))])
    );
}
