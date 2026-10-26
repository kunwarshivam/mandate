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
