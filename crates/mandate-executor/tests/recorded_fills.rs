//! The fill a broker with no fill report implies (R0 R19, DEC-1151; connections spec U-R8,
//! DEC-529 items 3 and 4, trading spec §5.7 and §11): an entry the executor adopted as filled from
//! the broker's order record, on an account its ledger holds flat in the instrument, is booked
//! from that record's filled quantity at the broker position's average price, with an id derived
//! from the order and the quantity, so the reconciliation reads the position it explains instead
//! of pausing on it. A fill is never invented: with no record of a fill, a ledger that is not
//! flat, a second order with fills in the instrument, or a position that is not exactly the
//! filled quantity, nothing is derived, and the reconciliation still pauses the agent.
//!
//! Each expectation is computed here from the case's own inputs, never from the crate's
//! predicates (`AGENTS.md`, independent oracles).

mod common;

use common::{
    AGENT, FixedInstruments, FixedMandate, Shell, TestIds, broker_fill, broker_order, config, date,
    handoff, instrument, key as member, opening, ports, price, qty, risk_exit, signed_qty,
    snapshot, stream_opened, text,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerFill, BrokerOutcome, BrokerPosition, BrokerUpdate, ClientOrderId, FillId, Input,
    OrderState, Ports, ReconcileReason, RiskClock, recorded_fills,
};
use mandate_num::Usd;
use mandate_time::{Date, UtcNanos};
use proptest::prelude::*;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const SECOND: &str = "01JABCDEFGHJKMNPQRSTVWXYZ1";
const THIRD: &str = "01JABCDEFGHJKMNPQRSTVWXYZ2";
const BROKER_ID: &str = "rh-sim-000001";
/// Tuesday 2026-10-20 at 11:00 in New York, the rehearsal's session.
const TUESDAY: &str = "2026-10-20T15:00:00Z";
/// The broker position's average price, which differs from the order's limit (150) so the
/// oracle can tell which one the fill was booked at.
const AVERAGE: &str = "149.87";

/// A stream opened and a process started on it, its clock at [`TUESDAY`].
fn fresh(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    let now = UtcNanos::parse_rfc3339(TUESDAY)
        .expect("a timestamp")
        .secs();
    shell.run(Input::Tick(RiskClock::from_secs(now)), ports);
    shell
}

fn key(id: &str) -> ClientOrderId {
    ClientOrderId::parse(id).expect("an id this platform derived")
}

/// `AGENT`'s opening of `quantity` `name` at 150 as `intent`, sent and acknowledged as
/// `broker_id`; answers its client order id.
fn accepted(
    shell: &mut Shell,
    ports: &Ports<'_>,
    intent: &str,
    name: &str,
    quantity: &str,
    broker_id: &str,
) -> String {
    let ran = shell.run(
        handoff(intent, AGENT, opening(name, quantity, "150")),
        ports,
    );
    let id = ran
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the opening is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            broker_id,
            Some(&id),
            name,
            Side::Buy,
            quantity,
            "0",
            "accepted",
        )))),
        ports,
    );
    id
}

/// The broker's order record for `id`, `filled` of `quantity` in `status`, as a listing or a
/// lookup reads it: the executor adopts it and books no fill from it.
fn described(
    shell: &mut Shell,
    ports: &Ports<'_>,
    id: &str,
    name: &str,
    quantity: &str,
    filled: &str,
    status: &str,
) {
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(broker_order(
            BROKER_ID,
            Some(id),
            name,
            Side::Buy,
            quantity,
            filled,
            status,
        ))),
        ports,
    );
}

/// The R19 state: one share of AAPL bought, read back `filled` from the broker's record, no fill
/// of it booked, and the ledger flat.
fn adopted_filled(ports: &Ports<'_>, quantity: &str) -> (Shell, String) {
    let mut shell = fresh(ports);
    let id = accepted(&mut shell, ports, INTENT, AAPL, quantity, BROKER_ID);
    described(&mut shell, ports, &id, AAPL, quantity, quantity, "filled");
    let order = shell.state.order(&key(&id)).expect("the entry is folded");
    assert_eq!(
        (order.state, order.filled_qty),
        (OrderState::Filled, qty("0")),
        "adopted filled, its fill unbooked"
    );
    assert!(shell.state.fills().is_empty(), "no fill is booked");
    (shell, id)
}

fn held(name: &str, quantity: &str) -> BrokerPosition {
    BrokerPosition {
        instrument: instrument(name),
        qty: signed_qty(quantity),
        avg_entry_price: price(AVERAGE),
    }
}

/// The New York trade date of the order's `OrderSubmitted`, read from its §4.7 stamp, which is
/// [`TUESDAY`]'s: 11:00 in New York, the same date.
fn trade_date(shell: &Shell, id: &str) -> Date {
    let stamp = shell
        .account_journal
        .iter()
        .find(|event| {
            event.event_type == "OrderSubmitted"
                && event.payload.get("client_order_id").and_then(Value::as_str) == Some(id)
        })
        .and_then(|event| event.payload.get("risk_clock").and_then(Value::as_str))
        .expect("the order's OrderSubmitted carries its risk clock");
    assert_eq!(
        UtcNanos::parse(stamp).expect("a §4.7 timestamp"),
        UtcNanos::parse_rfc3339(TUESDAY).expect("a timestamp"),
        "submitted at the shell's clock"
    );
    date("2026-10-20")
}

/// The one fill the record and the position imply, written out field by field.
fn expected(shell: &Shell, id: &str, quantity: &str) -> BrokerFill {
    BrokerFill {
        fill_id: FillId(format!("record:{id}:{quantity}")),
        client_order_id: Some(id.to_owned()),
        broker_order_id: None,
        instrument: instrument(AAPL),
        side: Side::Buy,
        qty: qty(quantity),
        price: price(AVERAGE),
        fees: Usd::ZERO,
        trade_date: trade_date(shell, id),
    }
}

/// The reconciliation of a snapshot holding `positions` and `fills` at the shell's head: its
/// drafts' types, and whether it paused the agent on a position mismatch.
fn reconcile(
    shell: &mut Shell,
    ports: &Ports<'_>,
    positions: Vec<BrokerPosition>,
    fills: Vec<BrokerFill>,
) -> (Vec<String>, bool) {
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.positions = positions;
    taken.fills = fills;
    let ran = shell.run(Input::BrokerSnapshot(taken), ports);
    let types: Vec<String> = ran.draft_types().into_iter().map(str::to_owned).collect();
    let paused = ran
        .draft("BrokerPositionObserved")
        .is_some_and(|draft| draft.payload.get("mismatch") == Some(&Value::Bool(true)))
        && types.iter().any(|kind| kind == "AgentModeApplied");
    (types, paused)
}

#[test]
fn an_adopted_filled_entry_on_a_flat_ledger_is_booked_from_its_record() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, id) = adopted_filled(&ports, "1");
    let positions = vec![held(AAPL, "1")];

    let derived = recorded_fills(&shell.state, &positions);
    assert_eq!(derived, vec![expected(&shell, &id, "1")]);

    let (types, paused) = reconcile(&mut shell, &ports, positions.clone(), derived.clone());
    assert!(!paused, "the fill explains the position: {types:?}");
    assert!(
        types.iter().any(|kind| kind == "LateFillApplied"),
        "the entry was already terminal, so its fill is late (§5.7): {types:?}"
    );
    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("1")),
        "the ledger holds the broker's share"
    );
    assert!(
        recorded_fills(&shell.state, &positions).is_empty(),
        "once booked, nothing more is derived"
    );
    let (again, paused) = reconcile(&mut shell, &ports, positions, derived);
    assert!(!paused, "{again:?}");
    assert!(
        !again.iter().any(|kind| kind.ends_with("FillApplied")),
        "the same id is never booked twice: {again:?}"
    );
}

/// The control: with no fill derived, the same position pauses the agent, as before R19.
#[test]
fn without_a_derived_fill_the_same_position_still_pauses() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, _) = adopted_filled(&ports, "1");
    let (types, paused) = reconcile(&mut shell, &ports, vec![held(AAPL, "1")], Vec::new());
    assert!(
        paused,
        "a position the ledger cannot explain pauses: {types:?}"
    );
}

/// A working entry part filled at the broker books what its record reports, as an ordinary
/// `FillApplied`, and stays working for the rest.
#[test]
fn a_part_filled_working_entry_books_what_its_record_reports() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, INTENT, AAPL, "3", BROKER_ID);
    described(&mut shell, &ports, &id, AAPL, "3", "2", "partially_filled");
    let positions = vec![held(AAPL, "2")];
    let derived = recorded_fills(&shell.state, &positions);
    assert_eq!(derived, vec![expected(&shell, &id, "2")]);
    let (types, paused) = reconcile(&mut shell, &ports, positions, derived);
    assert!(!paused, "{types:?}");
    assert!(types.iter().any(|kind| kind == "FillApplied"), "{types:?}");
    let order = shell.state.order(&key(&id)).expect("folded");
    assert_eq!(
        (order.state, order.filled_qty),
        (OrderState::PartiallyFilled, qty("2"))
    );
}

/// No broker record of a fill means no fill: an accepted entry with a position beside it, a
/// filled entry with no position, a position that is not exactly the filled quantity, and a
/// position in another instrument each derive nothing.
#[test]
fn no_record_of_a_fill_or_a_position_that_differs_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);

    let mut working = fresh(&ports);
    accepted(&mut working, &ports, INTENT, AAPL, "1", BROKER_ID);
    assert!(
        recorded_fills(&working.state, &[held(AAPL, "1")]).is_empty(),
        "an order whose record reports no fill"
    );

    let (shell, _) = adopted_filled(&ports, "2");
    for (positions, why) in [
        (Vec::new(), "no position"),
        (vec![held(AAPL, "1")], "less than the record filled"),
        (vec![held(AAPL, "3")], "more than the record filled"),
        (vec![held(AAPL, "-2")], "short"),
        (vec![held(CPHC, "2")], "another instrument"),
        (
            vec![held(AAPL, "2"), held(AAPL, "2")],
            "two positions in the instrument",
        ),
    ] {
        assert!(
            recorded_fills(&shell.state, &positions).is_empty(),
            "{why}: nothing is derived"
        );
    }
}

/// The ledger must be flat in the instrument before the fill: a share already booked there, an
/// entry partly booked from a fill report, or a second order with a fill in the instrument leave
/// the average price unattributable, so nothing is derived.
#[test]
fn a_ledger_that_is_not_flat_or_a_second_fill_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);

    let mut earlier = fresh(&ports);
    let first = accepted(&mut earlier, &ports, SECOND, AAPL, "1", "b-0");
    earlier.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-0",
            Some(&first),
            "1",
            "150",
        ))),
        &ports,
    );
    assert_eq!(
        earlier.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("1")),
        "the earlier share is booked"
    );
    let id = accepted(&mut earlier, &ports, INTENT, AAPL, "1", BROKER_ID);
    described(&mut earlier, &ports, &id, AAPL, "1", "1", "filled");
    assert!(
        recorded_fills(&earlier.state, &[held(AAPL, "2")]).is_empty(),
        "a ledger already holding the instrument"
    );

    let mut partly = fresh(&ports);
    let id = accepted(&mut partly, &ports, INTENT, AAPL, "3", BROKER_ID);
    partly.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-1",
            Some(&id),
            "1",
            "150",
        ))),
        &ports,
    );
    described(&mut partly, &ports, &id, AAPL, "3", "3", "filled");
    assert!(
        recorded_fills(&partly.state, &[held(AAPL, "3")]).is_empty(),
        "an entry partly booked from a fill report"
    );

    let mut two = fresh(&ports);
    let one = accepted(&mut two, &ports, INTENT, AAPL, "1", BROKER_ID);
    let other = accepted(&mut two, &ports, SECOND, AAPL, "1", "b-2");
    described(&mut two, &ports, &one, AAPL, "1", "1", "filled");
    described(&mut two, &ports, &other, AAPL, "1", "1", "filled");
    assert!(
        recorded_fills(&two.state, &[held(AAPL, "2")]).is_empty(),
        "two orders with fills in the instrument"
    );
}

/// The account stream of `shell` folded afresh, each record's payload passed through `edit` with
/// its event type first: the same journal with one member changed, so one guard of
/// [`recorded_fills`] is the only reason it derives nothing.
fn replayed(shell: &Shell, edit: impl Fn(&str, &mut Value)) -> Shell {
    let mut replay = Shell::new(1);
    for event in &shell.account_journal {
        let mut event = event.clone();
        edit(&event.event_type, &mut event.payload);
        replay.fold_one(&event).expect("the edited journal folds");
    }
    replay
}

/// Sets `name` to `value` in a payload that is an object.
fn set(payload: &mut Value, name: &str, value: Value) {
    let Value::Object(members) = payload else {
        panic!("a payload is an object");
    };
    members.insert(member(name), value);
}

/// The control for every single-fault case below: the R19 journal, replayed unedited, derives the
/// one fill, so each edit alone is what refuses it.
#[test]
fn each_guard_alone_refuses_the_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = adopted_filled(&ports, "1");
    let positions = vec![held(AAPL, "1")];
    let control = replayed(&shell, |_, _| {});
    assert_eq!(
        recorded_fills(&control.state, &positions),
        vec![expected(&shell, &id, "1")],
        "the unedited journal derives the fill"
    );
    let cases: [(&str, &str, &str, Value, &str); 4] = [
        ("OrderSubmitted", "tif", "a GTC entry", text("gtc"), "1"),
        (
            "OrderRequestRecorded",
            "extended_hours",
            "an entry sent for extended hours",
            Value::Bool(true),
            "1",
        ),
        (
            "OrderRequestRecorded",
            "purpose",
            "a buy that adds no risk",
            text("risk_exit"),
            "1",
        ),
        (
            "OrderStateChanged",
            "filled_qty",
            "a record reporting more than the order's quantity, the position agreeing with it",
            text("2"),
            "2",
        ),
    ];
    for (event_type, name, why, value, position) in cases {
        let edited = replayed(&shell, |kind, payload| {
            if kind == event_type {
                set(payload, name, value.clone());
            }
        });
        assert!(
            recorded_fills(&edited.state, &[held(AAPL, position)]).is_empty(),
            "{why}: nothing is derived"
        );
    }
}

/// The position's average is read against the order's limit: at the limit the fill is derived,
/// a cent above it nothing is, since no fill of a buy limit order can be above its limit.
#[test]
fn an_average_above_the_orders_limit_derives_nothing() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, _) = adopted_filled(&ports, "1");
    let at = |average: &str| BrokerPosition {
        avg_entry_price: price(average),
        ..held(AAPL, "1")
    };
    let derived = recorded_fills(&shell.state, &[at("150")]);
    assert_eq!(
        derived.iter().map(|fill| fill.price).collect::<Vec<_>>(),
        [price("150")],
        "at the limit"
    );
    assert!(
        recorded_fills(&shell.state, &[at("150.01")]).is_empty(),
        "above the limit"
    );
}

/// A round trip booked earlier leaves the ledger's position at zero, not absent, and both its
/// orders booked all they reported: the next entry is still derived, so a flat ledger is read by
/// its quantity, never by whether the instrument was ever held.
#[test]
fn a_ledger_flat_after_a_round_trip_still_derives_the_next_entry() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let first = accepted(&mut shell, &ports, SECOND, AAPL, "1", "b-0");
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-0",
            Some(&first),
            "1",
            "150",
        ))),
        &ports,
    );
    let ran = shell.run(handoff(THIRD, AGENT, risk_exit(AAPL, "1", "150")), &ports);
    let exit = ran
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the exit is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            "b-1",
            Some(&exit),
            AAPL,
            Side::Sell,
            "1",
            "0",
            "accepted",
        )))),
        &ports,
    );
    let mut sold = broker_fill("f-1", Some(&exit), "1", "150");
    sold.side = Side::Sell;
    shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(sold)), &ports);
    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("0")),
        "the round trip is booked, its position zero"
    );
    let id = accepted(&mut shell, &ports, INTENT, AAPL, "1", BROKER_ID);
    described(&mut shell, &ports, &id, AAPL, "1", "1", "filled");
    let derived = recorded_fills(&shell.state, &[held(AAPL, "1")]);
    assert_eq!(
        derived
            .iter()
            .map(|fill| fill.fill_id.clone())
            .collect::<Vec<_>>(),
        [FillId(format!("record:{id}:1"))]
    );
}

/// The reviewer's case for the flat guard alone: the ledger holds a booked share, a second entry
/// reports one filled, and the broker holds one (the owner sold the first). Position and record
/// agree, yet the ledger is not flat, so the average may blend two buys and nothing is derived.
/// An unbooked fill in another instrument blocks nothing.
#[test]
fn the_flat_ledger_and_the_instrument_are_each_read_alone() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);

    let mut holding = fresh(&ports);
    let first = accepted(&mut holding, &ports, SECOND, AAPL, "1", "b-0");
    holding.run(
        Input::BrokerUpdate(BrokerUpdate::Fill(broker_fill(
            "f-0",
            Some(&first),
            "1",
            "150",
        ))),
        &ports,
    );
    let id = accepted(&mut holding, &ports, INTENT, AAPL, "1", BROKER_ID);
    described(&mut holding, &ports, &id, AAPL, "1", "1", "filled");
    assert!(
        recorded_fills(&holding.state, &[held(AAPL, "1")]).is_empty(),
        "a ledger holding the instrument"
    );

    let mut elsewhere = fresh(&ports);
    let other = accepted(&mut elsewhere, &ports, SECOND, CPHC, "1", "b-2");
    described(&mut elsewhere, &ports, &other, CPHC, "1", "1", "filled");
    let id = accepted(&mut elsewhere, &ports, INTENT, AAPL, "1", BROKER_ID);
    described(&mut elsewhere, &ports, &id, AAPL, "1", "1", "filled");
    let derived = recorded_fills(&elsewhere.state, &[held(AAPL, "1")]);
    assert_eq!(
        derived
            .iter()
            .map(|fill| fill.instrument.clone())
            .collect::<Vec<_>>(),
        [instrument(AAPL)],
        "the other instrument's unbooked fill, with no position given for it, blocks nothing"
    );
}

proptest! {
    /// Over a filled quantity and a position, a fill is derived exactly when the position is the
    /// filled quantity, and it is never more than the broker filled.
    #[test]
    fn a_fill_is_derived_exactly_when_the_position_is_the_filled_quantity(
        filled in 1u32..6,
        position in -2i32..8,
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let config = config();
        let ports = ports(&ids, &mandates, &instruments, &config);
        let mut shell = fresh(&ports);
        let id = accepted(&mut shell, &ports, INTENT, AAPL, "5", BROKER_ID);
        let status = if filled == 5 { "filled" } else { "partially_filled" };
        described(&mut shell, &ports, &id, AAPL, "5", &filled.to_string(), status);
        let derived = recorded_fills(&shell.state, &[held(AAPL, &position.to_string())]);
        let matches = i64::from(position) == i64::from(filled);
        prop_assert_eq!(derived.len(), usize::from(matches));
        for one in &derived {
            prop_assert_eq!(one.qty, qty(&filled.to_string()));
        }
    }
}
