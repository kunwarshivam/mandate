//! The owner's position read (DEC-2136): the agent sub-ledger's §8.1 basis, the latest counted
//! mark, and the account's protection, each figure computed here by hand from the rows.

use mandate_accounting::Position;
use mandate_canon::{Key, Value};
use mandate_gate_context::{CountedSession, Positions, Protection, RiskMark, Row, Unreadable};
use mandate_num::{CostBasis, Price, Qty, SignedQty};
use mandate_time::UtcNanos;

const ACCOUNT: &str = "acct:ws1:alpaca:paper1";
const OTHER: &str = "acct:ws1:alpaca:other";
const AGENT: &str = "agent-1";
const ASSET: &str = "8ccae427-5dd0-45b3-b5fe-7c0c3a8ee5c4";
const ASSET_B: &str = "0d1d3a7b-58a5-4c3e-9ad7-2b51f3e6f0aa";
const START: i64 = 1_790_000_000;

trait Members {
    fn with(self, member: &str, value: Value) -> Self;
}

impl Members for Value {
    fn with(mut self, member: &str, value: Value) -> Self {
        if let Value::Object(members) = &mut self {
            members.insert(Key::new(member).unwrap(), value);
        }
        self
    }
}

fn object(members: &[(&str, Value)]) -> Value {
    members
        .iter()
        .fold(Value::Object(Default::default()), |object, (key, value)| {
            object.with(key, value.clone())
        })
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

fn list(items: &[&str]) -> Value {
    Value::Array(items.iter().map(|item| text(item)).collect())
}

fn instant(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn clock(secs: i64) -> Value {
    text(&instant(secs).to_string())
}

fn qty(value: &str) -> Qty {
    Qty::parse(value).unwrap()
}

fn price(value: &str) -> Price {
    Price::parse(value).unwrap()
}

fn position(q: &str, basis: &str) -> Position {
    Position::new(
        SignedQty::parse(q).unwrap(),
        CostBasis::parse(basis).unwrap(),
    )
    .unwrap()
}

fn request(agent: &str, purpose: &str) -> Value {
    object(&[
        ("agent_id", text(agent)),
        ("purpose", text(purpose)),
        ("order_class", Value::Null),
        ("risk_clock", clock(START)),
    ])
}

fn submitted(order: &str, instrument: &str, side: &str, q: &str) -> Value {
    object(&[
        ("client_order_id", text(order)),
        ("instrument_id", text(instrument)),
        ("side", text(side)),
        ("qty", text(q)),
        ("risk_clock", clock(START)),
    ])
}

fn fee(asset: &str) -> Value {
    object(&[
        ("kind", text("cat")),
        ("amount", text("0.01")),
        ("asset", text(asset)),
        ("status", text("accrued")),
    ])
}

fn fill(id: &str, order: &str, instrument: &str, side: &str, q: &str, p: &str) -> Value {
    object(&[
        ("fill_id", text(id)),
        ("client_order_id", text(order)),
        ("instrument_id", text(instrument)),
        ("side", text(side)),
        ("qty_gross", text(q)),
        ("price", text(p)),
        ("trade_date", text("2026-09-21")),
        ("risk_clock", clock(START)),
        ("fees", Value::Array(vec![fee("USD")])),
    ])
}

fn mark(instrument: &str, bid: Option<&str>, sane: Option<bool>, session: &str, at: i64) -> Value {
    let base = object(&[
        ("instrument_id", text(instrument)),
        ("price", text("150")),
        ("source", text("quote")),
        ("feed", text("iex")),
        ("risk_clock", clock(at)),
    ]);
    match sane {
        None => base,
        Some(sane) => base
            .with("bid", bid.map_or(Value::Null, text))
            .with("sane", Value::Bool(sane))
            .with("session", text(session)),
    }
}

fn protection(action: &str, at: i64) -> Value {
    object(&[
        ("instrument_id", text(ASSET)),
        ("action", text(action)),
        ("orders", Value::Array(Vec::new())),
        ("awaiting", Value::Array(Vec::new())),
        ("qty", Value::Null),
        ("stop", Value::Null),
        ("take_profit", Value::Null),
        ("bracket", Value::Null),
        ("uncovered", Value::Null),
        ("acknowledged", Value::Null),
        ("risk_clock", clock(at)),
    ])
}

fn state_changed(order: &str, state: &str) -> Value {
    object(&[
        ("client_order_id", text(order)),
        ("state", text(state)),
        ("ignored", Value::Bool(false)),
        ("risk_clock", clock(START)),
    ])
}

fn on(read: &mut Positions, event_type: &str, payload: &Value) {
    read.fold(Row {
        stream: ACCOUNT,
        event_type,
        payload,
    })
    .unwrap();
}

/// The agent's own order `order` in `instrument`, requested and submitted.
fn own_order(read: &mut Positions, order: &str, instrument: &str, side: &str, q: &str) {
    on(read, "OrderRequestRecorded", &request(AGENT, "open"));
    on(
        read,
        "OrderSubmitted",
        &submitted(order, instrument, side, q),
    );
}

/// The agent bought 10 at 512.25: Q 10, B 10 × 512.25 = 5122.5.
fn bought() -> Positions {
    let mut read = Positions::new(ACCOUNT, AGENT);
    own_order(&mut read, "mdt-1", ASSET, "buy", "10");
    on(
        &mut read,
        "FillApplied",
        &fill("f1", "mdt-1", ASSET, "buy", "10", "512.25"),
    );
    read
}

#[test]
fn an_opening_fill_adds_its_quantity_and_what_it_cost() {
    assert_eq!(bought().cost_basis(ASSET), Some(position("10", "5122.5")));
}

#[test]
fn a_reduction_removes_its_share_of_the_basis() {
    let mut read = bought();
    own_order(&mut read, "mdt-2", ASSET, "sell", "4");
    on(
        &mut read,
        "LateFillApplied",
        &fill("f2", "mdt-2", ASSET, "sell", "4", "520"),
    );
    assert_eq!(
        read.cost_basis(ASSET),
        Some(position("6", "3073.5")),
        "5122.5 less 5122.5 × 4 ÷ 10 = 2049"
    );
}

#[test]
fn a_reduction_rounds_the_removed_basis_half_even_at_twelve_places() {
    let mut read = Positions::new(ACCOUNT, AGENT);
    own_order(&mut read, "mdt-1", ASSET, "buy", "3");
    on(
        &mut read,
        "FillApplied",
        &fill("f1", "mdt-1", ASSET, "buy", "1", "0.5"),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f2", "mdt-1", ASSET, "buy", "2", "0.25"),
    );
    own_order(&mut read, "mdt-2", ASSET, "sell", "1");
    on(
        &mut read,
        "FillApplied",
        &fill("f3", "mdt-2", ASSET, "sell", "1", "9"),
    );
    assert_eq!(
        read.cost_basis(ASSET),
        Some(position("2", "0.666666666667")),
        "1 less round(1 ÷ 3, 12) = 0.333333333333"
    );
}

#[test]
fn a_close_leaves_a_flat_position_with_no_basis() {
    let mut read = bought();
    own_order(&mut read, "mdt-2", ASSET, "sell", "10");
    on(
        &mut read,
        "FillApplied",
        &fill("f2", "mdt-2", ASSET, "sell", "10", "500"),
    );
    assert_eq!(read.cost_basis(ASSET), Some(Position::FLAT));
}

#[test]
fn another_agents_fills_and_an_unrequested_order_are_not_the_agents() {
    let mut read = bought();
    on(
        &mut read,
        "OrderRequestRecorded",
        &request("agent-2", "open"),
    );
    on(
        &mut read,
        "OrderSubmitted",
        &submitted("mdt-9", ASSET, "buy", "5"),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f9", "mdt-9", ASSET, "buy", "5", "1"),
    );
    on(
        &mut read,
        "OrderSubmitted",
        &submitted("mdt-8", ASSET_B, "buy", "5"),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f8", "mdt-8", ASSET_B, "buy", "5", "1"),
    );
    assert_eq!(read.cost_basis(ASSET), Some(position("10", "5122.5")));
    assert_eq!(read.cost_basis(ASSET_B), None);
}

#[test]
fn a_fill_journaled_twice_counts_once() {
    let mut read = bought();
    on(
        &mut read,
        "FillApplied",
        &fill("f1", "mdt-1", ASSET, "buy", "10", "512.25"),
    );
    assert_eq!(read.cost_basis(ASSET), Some(position("10", "5122.5")));
}

#[test]
fn a_release_a_corporate_action_or_a_fee_in_the_asset_leaves_the_basis_unreported() {
    let mut released = bought();
    let release = object(&[
        ("agent_id", text(AGENT)),
        ("instrument_id", text(ASSET)),
        ("qty", text("10")),
    ]);
    on(&mut released, "PositionReleased", &release);
    assert_eq!(released.cost_basis(ASSET), None);

    let mut other = bought();
    on(
        &mut other,
        "PositionReleased",
        &release.clone().with("agent_id", text("agent-2")),
    );
    assert_eq!(other.cost_basis(ASSET), Some(position("10", "5122.5")));

    let mut acted = bought();
    let action = object(&[("instrument_id", text(ASSET)), ("kind", text("split"))]);
    on(&mut acted, "CorporateActionApplied", &action);
    assert_eq!(acted.cost_basis(ASSET), None);

    let mut charged = Positions::new(ACCOUNT, AGENT);
    own_order(&mut charged, "mdt-1", ASSET, "buy", "10");
    on(
        &mut charged,
        "FillApplied",
        &fill("f1", "mdt-1", ASSET, "buy", "10", "2")
            .with("fees", Value::Array(vec![fee("USD"), fee("BTC")])),
    );
    assert_eq!(charged.cost_basis(ASSET), None);
}

#[test]
fn a_row_on_another_stream_changes_nothing() {
    let mut read = bought();
    let payload = fill("f2", "mdt-1", ASSET, "buy", "10", "1");
    read.fold(Row {
        stream: OTHER,
        event_type: "FillApplied",
        payload: &payload,
    })
    .unwrap();
    assert_eq!(read.cost_basis(ASSET), Some(position("10", "5122.5")));
}

#[test]
fn a_fill_that_does_not_parse_is_refused_at_its_member() {
    for (member, bad) in [
        ("price", text("x")),
        ("qty_gross", text("-1")),
        ("side", text("short")),
        ("fees", Value::Null),
    ] {
        let mut read = bought();
        let payload = fill("f2", "mdt-1", ASSET, "buy", "1", "1").with(member, bad);
        let refused = read.fold(Row {
            stream: ACCOUNT,
            event_type: "FillApplied",
            payload: &payload,
        });
        assert_eq!(
            refused,
            Err(Unreadable {
                event_type: "FillApplied".to_owned(),
                member,
            })
        );
    }
}

#[test]
fn the_risk_mark_is_the_latest_counted_version_two_mark_as_journaled() {
    let mut read = Positions::new(ACCOUNT, AGENT);
    assert_eq!(read.risk_mark(ASSET), None);
    on(
        &mut read,
        "MarkUpdated",
        &mark(ASSET, Some("515.12"), Some(true), "regular", START),
    );
    let sane = RiskMark {
        bid: Some(price("515.12")),
        sane: true,
        session: CountedSession::Regular,
        at: instant(START),
    };
    assert_eq!(read.risk_mark(ASSET), Some(sane));
    on(
        &mut read,
        "MarkUpdated",
        &mark(ASSET, Some("600"), Some(true), "pre_market", START + 1),
    );
    on(
        &mut read,
        "MarkUpdated",
        &mark(ASSET, None, None, "regular", START + 2),
    );
    assert_eq!(
        read.risk_mark(ASSET),
        Some(sane),
        "an extended-hours mark and a version-1 mark move nothing"
    );
    on(
        &mut read,
        "MarkUpdated",
        &mark(ASSET, Some("1"), Some(false), "regular", START + 3),
    );
    assert_eq!(
        read.risk_mark(ASSET),
        Some(RiskMark {
            bid: None,
            sane: false,
            session: CountedSession::Regular,
            at: instant(START + 3),
        }),
        "a mark that is not sane is served as such, never priced"
    );
    on(
        &mut read,
        "MarkUpdated",
        &mark(ASSET_B, Some("2"), Some(true), "crypto", START + 4),
    );
    assert_eq!(
        read.risk_mark(ASSET_B).map(|mark| mark.session),
        Some(CountedSession::Crypto)
    );
}

#[test]
fn a_mark_that_does_not_parse_is_refused_at_its_member() {
    for (member, payload) in [
        (
            "session",
            mark(ASSET, Some("1"), Some(true), "lunch", START),
        ),
        ("bid", mark(ASSET, None, Some(true), "regular", START)),
        (
            "sane",
            mark(ASSET, Some("1"), Some(true), "regular", START).with("sane", text("yes")),
        ),
        (
            "risk_clock",
            mark(ASSET, Some("1"), Some(true), "regular", START).with("risk_clock", text("now")),
        ),
    ] {
        let mut read = Positions::new(ACCOUNT, AGENT);
        let refused = read.fold(Row {
            stream: ACCOUNT,
            event_type: "MarkUpdated",
            payload: &payload,
        });
        assert_eq!(
            refused,
            Err(Unreadable {
                event_type: "MarkUpdated".to_owned(),
                member,
            })
        );
    }
}

fn placed(orders: &[&str], covered: &str, stop: &str, take_profit: Option<&str>) -> Value {
    protection("placed", START)
        .with("orders", list(orders))
        .with("qty", text(covered))
        .with("stop", text(stop))
        .with("take_profit", take_profit.map_or(Value::Null, text))
}

#[test]
fn no_record_is_no_protection() {
    assert_eq!(bought().protection(ASSET), Protection::NONE);
}

#[test]
fn a_placement_rests_its_orders_and_covers_its_quantity_at_its_prices() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-1-p1", "mdt-1-p2"], "10", "497", Some("538")),
    );
    assert_eq!(
        read.protection(ASSET),
        Protection {
            orders: vec!["mdt-1-p1".to_owned(), "mdt-1-p2".to_owned()],
            covered_qty: qty("10"),
            stop: Some(price("497")),
            take_profit: Some(price("538")),
            unprotected_since: None,
        }
    );
    assert_eq!(read.protection(ASSET_B), Protection::NONE);
}

#[test]
fn a_cancel_takes_its_orders_and_the_quantity_it_journaled_off() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-1-p1"], "6", "497", None),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-1-p2"], "4", "490", None),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("cancelled", START)
            .with("orders", list(&["mdt-1-p1"]))
            .with("qty", text("6")),
    );
    assert_eq!(
        read.protection(ASSET),
        Protection {
            orders: vec!["mdt-1-p2".to_owned()],
            covered_qty: qty("4"),
            stop: Some(price("490")),
            take_profit: None,
            unprotected_since: None,
        },
        "the prices stay the latest placement's"
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("cancelled", START).with("orders", list(&["mdt-1-p2"])),
    );
    assert_eq!(read.protection(ASSET), Protection::NONE);
}

#[test]
fn a_protective_order_that_ends_leaves_the_protection_with_its_quantity() {
    let mut read = bought();
    on(
        &mut read,
        "OrderRequestRecorded",
        &request(AGENT, "protective"),
    );
    on(
        &mut read,
        "OrderSubmitted",
        &submitted("mdt-s1", ASSET, "sell", "6"),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-s1", "mdt-1-p2"], "10", "497", None),
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-s1", "filled"),
    );
    assert_eq!(
        read.protection(ASSET).covered_qty,
        qty("4"),
        "10 less the submitted 6"
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-1-p2", "accepted"),
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-1-p2", "canceled").with("ignored", Value::Bool(true)),
    );
    assert_eq!(read.protection(ASSET).orders, vec!["mdt-1-p2".to_owned()]);
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-1-p2", "expired"),
    );
    assert_eq!(read.protection(ASSET), Protection::NONE);
}

#[test]
fn an_unprotected_interval_is_open_from_its_start_until_it_ends() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_start", START + 5),
    );
    assert_eq!(
        read.protection(ASSET).unprotected_since,
        Some(instant(START + 5))
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("interval_limit", START + 65),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 66).with("uncovered", Value::Bool(true)),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 67).with("awaiting", list(&["mdt-1-p1"])),
    );
    assert_eq!(
        read.protection(ASSET).unprotected_since,
        Some(instant(START + 5)),
        "a bound, an uncovered end and an end awaiting a cancel end nothing"
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 70).with("acknowledged", Value::Bool(true)),
    );
    assert_eq!(read.protection(ASSET).unprotected_since, None);
    on(
        &mut read,
        "ProtectionChanged",
        &protection("passive_start", START + 80),
    );
    assert_eq!(
        read.protection(ASSET).unprotected_since,
        None,
        "a passive start opens no interval"
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_start", START + 90),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 95),
    );
    assert_eq!(read.protection(ASSET).unprotected_since, None);
}

#[test]
fn an_unknown_protection_action_is_refused() {
    let mut read = bought();
    let payload = protection("forgotten", START);
    assert_eq!(
        read.fold(Row {
            stream: ACCOUNT,
            event_type: "ProtectionChanged",
            payload: &payload,
        }),
        Err(Unreadable {
            event_type: "ProtectionChanged".to_owned(),
            member: "action",
        })
    );
}

fn filled(read: &Positions) -> Vec<(String, String)> {
    read.filled()
        .map(|(instrument, qty)| (instrument.to_owned(), qty.to_string()))
        .collect()
}

#[test]
fn a_leg_the_broker_created_for_the_agents_entry_is_the_agents() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-1-p1", "mdt-1-p2"], "10", "497", Some("538")),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f2", "mdt-1-p1", ASSET, "sell", "10", "497"),
    );
    assert_eq!(
        filled(&read),
        Vec::<(String, String)>::new(),
        "the stop sold all 10"
    );
    assert_eq!(read.cost_basis(ASSET), Some(Position::FLAT));
}

#[test]
fn a_leg_of_another_agents_entry_is_not_the_agents() {
    let mut read = bought();
    on(
        &mut read,
        "OrderRequestRecorded",
        &request("agent-2", "open"),
    );
    on(
        &mut read,
        "OrderSubmitted",
        &submitted("mdt-9", ASSET, "buy", "5"),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-9-p1"], "5", "490", None),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f9", "mdt-9-p1", ASSET, "sell", "5", "490"),
    );
    assert_eq!(filled(&read), vec![(ASSET.to_owned(), "10".to_owned())]);
    assert_eq!(read.cost_basis(ASSET), Some(position("10", "5122.5")));
}

#[test]
fn a_leg_that_names_no_known_entry_filling_where_the_agent_holds_is_refused() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["oco-1"], "10", "497", None),
    );
    let payload = fill("f2", "oco-1", ASSET, "sell", "10", "497");
    assert_eq!(
        read.fold(Row {
            stream: ACCOUNT,
            event_type: "FillApplied",
            payload: &payload,
        }),
        Err(Unreadable {
            event_type: "FillApplied".to_owned(),
            member: "client_order_id",
        }),
        "DEC-160's single holder may be this agent, which the read cannot tell"
    );
    let mut elsewhere = bought();
    on(
        &mut elsewhere,
        "ProtectionChanged",
        &placed(&["oco-1"], "5", "1", None).with("instrument_id", text(ASSET_B)),
    );
    on(
        &mut elsewhere,
        "FillApplied",
        &fill("f2", "oco-1", ASSET_B, "sell", "5", "1"),
    );
    assert_eq!(
        elsewhere.cost_basis(ASSET_B),
        None,
        "where it holds nothing"
    );
}

#[test]
fn a_replacement_of_the_agents_order_is_the_agents_and_inherits_what_was_left() {
    let mut read = Positions::new(ACCOUNT, AGENT);
    own_order(&mut read, "mdt-1", ASSET, "buy", "5");
    on(
        &mut read,
        "FillApplied",
        &fill("f1", "mdt-1", ASSET, "buy", "3", "10"),
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-1", "replaced").with("replaced_by", text("mdt-1b")),
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-1b", "accepted").with("replaces", text("mdt-1")),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f2", "mdt-1b", ASSET, "buy", "2", "11"),
    );
    assert_eq!(filled(&read), vec![(ASSET.to_owned(), "5".to_owned())]);
    assert_eq!(read.cost_basis(ASSET), Some(position("5", "52")), "30 + 22");
}

#[test]
fn a_change_of_an_order_the_journal_never_submitted_is_refused() {
    for (payload, member) in [
        (state_changed("mdt-x", "canceled"), "client_order_id"),
        (
            state_changed("mdt-y", "accepted").with("replaces", text("mdt-x")),
            "replaces",
        ),
    ] {
        let mut read = bought();
        assert_eq!(
            read.fold(Row {
                stream: ACCOUNT,
                event_type: "OrderStateChanged",
                payload: &payload,
            }),
            Err(Unreadable {
                event_type: "OrderStateChanged".to_owned(),
                member,
            })
        );
    }
}

#[test]
fn a_protective_order_named_twice_is_served_once_and_an_empty_id_is_refused() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-1-p1"], "6", "497", None),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-1-p1"], "4", "497", None),
    );
    let served = read.protection(ASSET);
    assert_eq!(served.orders, vec!["mdt-1-p1".to_owned()]);
    assert_eq!(served.covered_qty, qty("10"), "the executor's covered sum");
    let long = "x".repeat(201);
    for bad in ["", long.as_str()] {
        let payload = placed(&[bad], "1", "1", None);
        assert_eq!(
            read.fold(Row {
                stream: ACCOUNT,
                event_type: "ProtectionChanged",
                payload: &payload,
            }),
            Err(Unreadable {
                event_type: "ProtectionChanged".to_owned(),
                member: "orders",
            })
        );
    }
}

#[test]
fn a_release_of_nothing_leaves_the_basis_reported() {
    let mut read = bought();
    let release = object(&[
        ("agent_id", text(AGENT)),
        ("instrument_id", text(ASSET)),
        ("qty", text("0")),
    ]);
    on(&mut read, "PositionReleased", &release);
    assert_eq!(read.cost_basis(ASSET), Some(position("10", "5122.5")));
}

#[test]
fn a_counted_session_is_spelled_as_the_journal_spells_it() {
    assert_eq!(CountedSession::Regular.as_str(), "regular");
    assert_eq!(CountedSession::Crypto.as_str(), "crypto");
}

/// An order the executor ended as abandoned before submitting it is known, so a later change of
/// its state is read, not refused (as the executor's `order_abandoned` records it).
#[test]
fn an_abandoned_order_is_known() {
    let mut read = bought();
    let abandoned = object(&[
        ("client_order_id", text("mdt-7")),
        ("intent_id", text("i-7")),
    ]);
    on(&mut read, "OrderAbandoned", &abandoned);
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-7", "abandoned"),
    );
    assert_eq!(read.cost_basis(ASSET), Some(position("10", "5122.5")));
}

/// A version-1 submission with no companion names its own agent and, with no bracket, is a plain
/// entry, whose interval only its own stop's acknowledgment ends (DEC-966 item 2).
#[test]
fn a_legacy_submission_without_a_bracket_is_a_plain_entry() {
    let mut read = Positions::new(ACCOUNT, AGENT);
    on(
        &mut read,
        "OrderSubmitted",
        &submitted("mdt-1", ASSET, "buy", "10").with("agent_id", text(AGENT)),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f1", "mdt-1", ASSET, "buy", "10", "1"),
    );
    assert_eq!(read.cost_basis(ASSET), Some(position("10", "10")));
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_start", START + 1),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 2).with("uncovered", Value::Bool(true)),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_start", START + 3).with("bracket", text("mdt-1")),
    );
    assert_eq!(
        read.protection(ASSET).unprotected_since,
        Some(instant(START + 1)),
        "a plain entry's start ends no other interval, not even an uncovered one"
    );
}

/// Whether an entry is plain is its request's: a start naming a bracket entry ends an uncovered
/// interval, as any other start does; a start naming a plain entry ends none (DEC-966 item 2).
#[test]
fn only_a_start_naming_a_bracket_entry_ends_an_uncovered_interval() {
    for (order_class, ended) in [(Value::Null, false), (text("bracket"), true)] {
        let mut read = Positions::new(ACCOUNT, AGENT);
        on(
            &mut read,
            "OrderRequestRecorded",
            &request(AGENT, "open").with("order_class", order_class.clone()),
        );
        on(
            &mut read,
            "OrderSubmitted",
            &submitted("mdt-1", ASSET, "buy", "10"),
        );
        on(
            &mut read,
            "ProtectionChanged",
            &protection("unprotected_start", START + 1),
        );
        on(
            &mut read,
            "ProtectionChanged",
            &protection("unprotected_end", START + 2).with("uncovered", Value::Bool(true)),
        );
        on(
            &mut read,
            "ProtectionChanged",
            &protection("unprotected_start", START + 3).with("bracket", text("mdt-1")),
        );
        let since = if ended { START + 3 } else { START + 1 };
        assert_eq!(
            read.protection(ASSET).unprotected_since,
            Some(instant(since)),
            "order_class {order_class:?}"
        );
    }
}

/// An acknowledged end ends the interval of the entry whose stop it awaited, though it names no
/// bracket and still awaits that cancel itself.
#[test]
fn an_acknowledged_end_ends_the_interval_of_the_entry_it_awaited() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_start", START + 1).with("bracket", text("mdt-1")),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 2)
            .with("awaiting", list(&["mdt-1-p1"]))
            .with("bracket", text("mdt-1")),
    );
    assert_eq!(
        read.protection(ASSET).unprotected_since,
        Some(instant(START + 1))
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 3)
            .with("awaiting", list(&["mdt-1-p1"]))
            .with("acknowledged", Value::Bool(true)),
    );
    assert_eq!(read.protection(ASSET).unprotected_since, None);
}

/// The actions that move neither the resting orders nor an interval are read and change nothing.
#[test]
fn an_action_that_moves_no_protection_changes_nothing() {
    let mut read = bought();
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-1-p1"], "10", "497", None),
    );
    let before = read.protection(ASSET);
    for action in [
        "intended",
        "watchdog",
        "exit_unpriced",
        "ladder_floor",
        "expiry_unreplaceable",
        "rung_short",
    ] {
        on(
            &mut read,
            "ProtectionChanged",
            &protection(action, START + 5),
        );
        assert_eq!(read.protection(ASSET), before, "{action}");
    }
}

/// A replacement is a new order of the old one's kind for what the old one left unfilled, and so
/// is a replacement of it: what each covers leaves the protection with it.
#[test]
fn a_replacement_of_a_protective_order_covers_what_its_original_left() {
    let mut read = bought();
    on(
        &mut read,
        "OrderRequestRecorded",
        &request(AGENT, "protective"),
    );
    on(
        &mut read,
        "OrderSubmitted",
        &submitted("mdt-a", ASSET, "sell", "10"),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-c", "mdt-a"], "15", "497", None),
    );
    on(
        &mut read,
        "FillApplied",
        &fill("f-a", "mdt-a", ASSET, "sell", "4", "497"),
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-b", "accepted").with("replaces", text("mdt-a")),
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-e", "accepted").with("replaces", text("mdt-b")),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &placed(&["mdt-b", "mdt-e"], "12", "497", None),
    );
    assert_eq!(read.protection(ASSET).covered_qty, qty("27"));
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-b", "canceled"),
    );
    assert_eq!(
        read.protection(ASSET).covered_qty,
        qty("21"),
        "mdt-b covered the 6 mdt-a left"
    );
    on(
        &mut read,
        "OrderStateChanged",
        &state_changed("mdt-e", "canceled"),
    );
    assert_eq!(
        read.protection(ASSET).covered_qty,
        qty("15"),
        "mdt-e the 6 mdt-b left, none of it filled"
    );
}

/// A start ends only an open interval of its own instrument: never another instrument's, and
/// never one already ended, even one that ended uncovered.
#[test]
fn a_start_ends_only_an_open_interval_of_its_own_instrument() {
    let mut read = bought();
    let in_b = |action: &str, at: i64| protection(action, at).with("instrument_id", text(ASSET_B));
    on(
        &mut read,
        "ProtectionChanged",
        &in_b("unprotected_start", START + 1),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &in_b("unprotected_end", START + 2).with("uncovered", Value::Bool(true)),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_start", START + 3),
    );
    assert_eq!(
        read.protection(ASSET_B).unprotected_since,
        Some(instant(START + 1)),
        "A's start leaves B's interval open"
    );

    let mut read = bought();
    for (action, at, uncovered) in [
        ("unprotected_start", 1, false),
        ("unprotected_end", 2, true),
        ("unprotected_start", 3, false),
        ("unprotected_end", 4, true),
        ("unprotected_start", 5, false),
    ] {
        let mut payload = protection(action, START + at);
        if uncovered {
            payload = payload.with("uncovered", Value::Bool(true));
        }
        on(&mut read, "ProtectionChanged", &payload);
    }
    assert_eq!(
        read.protection(ASSET).unprotected_since,
        Some(instant(START + 5)),
        "the second start ended the first interval, the third the second"
    );
}

/// A plain entry `order` of the agent's, whose stop follows its fill (DEC-966 item 2).
fn plain_entry(read: &mut Positions, order: &str) {
    on(read, "OrderRequestRecorded", &request(AGENT, "open"));
    on(read, "OrderSubmitted", &submitted(order, ASSET, "buy", "1"));
}

/// A bracket entry `order` of the agent's.
fn bracket_entry(read: &mut Positions, order: &str) {
    on(
        read,
        "OrderRequestRecorded",
        &request(AGENT, "open").with("order_class", text("bracket")),
    );
    on(read, "OrderSubmitted", &submitted(order, ASSET, "buy", "1"));
}

fn starts(bracket: Option<&str>, instrument: &str, at: i64) -> Value {
    protection("unprotected_start", START + at)
        .with("instrument_id", text(instrument))
        .with("bracket", bracket.map_or(Value::Null, text))
}

fn open_since(read: &Positions, instrument: &str) -> Option<i64> {
    read.protection(instrument)
        .unprotected_since
        .map(|at| at.secs() - START)
}

/// An acknowledged end ends the first open interval of its own instrument among the entries it
/// awaited: never another instrument's, and never one already ended.
#[test]
fn an_acknowledged_end_ends_an_open_interval_of_its_own_instrument() {
    let mut read = bought();
    on(&mut read, "ProtectionChanged", &starts(None, ASSET_B, 1));
    on(&mut read, "ProtectionChanged", &starts(None, ASSET, 2));
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 3).with("acknowledged", Value::Bool(true)),
    );
    assert_eq!(open_since(&read, ASSET), None);
    assert_eq!(
        open_since(&read, ASSET_B),
        Some(1),
        "B's interval is not A's to end"
    );

    let mut read = bought();
    on(&mut read, "ProtectionChanged", &starts(None, ASSET, 1));
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 2),
    );
    on(&mut read, "ProtectionChanged", &starts(None, ASSET, 3));
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 4).with("acknowledged", Value::Bool(true)),
    );
    assert_eq!(
        open_since(&read, ASSET),
        None,
        "the open one ends, not the ended one again"
    );
}

/// An acknowledgment ends every plain entry's interval among the stops it awaited, in its own
/// instrument (DEC-966 item 7): the awaited stops of plain entries join one another, and a bracket
/// entry's interval, or a plain entry's in another instrument, stays open.
#[test]
fn an_acknowledgment_ends_the_interval_of_every_awaited_plain_entry_and_nothing_else() {
    let mut read = bought();
    plain_entry(&mut read, "mdt-p");
    plain_entry(&mut read, "mdt-q");
    bracket_entry(&mut read, "mdt-b");
    on(
        &mut read,
        "ProtectionChanged",
        &starts(Some("mdt-p"), ASSET, 1),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &starts(Some("mdt-q"), ASSET, 2),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &starts(Some("mdt-b"), ASSET, 3),
    );
    on(
        &mut read,
        "ProtectionChanged",
        &starts(Some("mdt-q"), ASSET_B, 4),
    );
    for (entry, at) in [("mdt-p", 5), ("mdt-q", 6)] {
        on(
            &mut read,
            "ProtectionChanged",
            &protection("unprotected_end", START + at)
                .with("bracket", text(entry))
                .with("awaiting", list(&[&format!("{entry}-p1")])),
        );
    }
    assert_eq!(open_since(&read, ASSET), Some(1));
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 7).with("acknowledged", Value::Bool(true)),
    );
    assert_eq!(
        open_since(&read, ASSET),
        Some(3),
        "both plain entries' intervals end, and the bracket entry's stays"
    );
    assert_eq!(
        open_since(&read, ASSET_B),
        Some(4),
        "another instrument's stays"
    );
}

/// An end that names a bracket ends that bracket's open interval, not another's.
#[test]
fn an_end_naming_a_bracket_ends_that_brackets_interval() {
    let mut read = bought();
    bracket_entry(&mut read, "mdt-b");
    on(
        &mut read,
        "ProtectionChanged",
        &starts(Some("mdt-b"), ASSET, 1),
    );
    on(&mut read, "ProtectionChanged", &starts(None, ASSET, 2));
    on(
        &mut read,
        "ProtectionChanged",
        &protection("unprotected_end", START + 3).with("bracket", text("mdt-b")),
    );
    assert_eq!(
        open_since(&read, ASSET),
        Some(2),
        "mdt-b's ended; the other stays"
    );
}
