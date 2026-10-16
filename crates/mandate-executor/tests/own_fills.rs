//! The own-fill reconciliation (DEC-955): an order this account stream journaled as submitted,
//! which the broker filled and the stream never booked, gets its fill records from the broker's
//! own word, and nothing else is drafted. The E2 state is the first case: the entry journaled
//! `filled`, its fill at the broker, no `FillApplied` or `LateFillApplied` on the stream.
//!
//! Each expectation is computed here from the case's own inputs, never from the crate's
//! predicates (`AGENTS.md`, independent oracles).

mod common;

use std::collections::BTreeSet;

use common::{
    ACCOUNT_STREAM, FixedInstruments, FixedMandate, Shell, TestIds, broker_fill, broker_order,
    config, event, handoff, instrument, opening, ports, signed_qty, stream_opened, text,
    with_clock,
};
use mandate_accounting::Side;
use mandate_canon::Value;
use mandate_executor::{
    BrokerFill, BrokerOrder, BrokerOutcome, BrokerUpdate, ClientOrderId, Effect, ExecutorError,
    FoldedEvent, Input, OrderState, Ports, RiskClock, Seq, WriterEpoch, book_own_fills,
    owed_orders,
};
use proptest::prelude::*;

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const CPHC: &str = FixedInstruments::THIN_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const BROKER_ID: &str = "68099755-c7e6-40ea-8ba2-5dc644393428";
const AT: i64 = 100;

/// A stream opened and a process started on it.
fn fresh(ports: &Ports<'_>) -> Shell {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    shell.restart_ready(ports)
}

/// One opening of `quantity` AAPL at 150, submitted and acknowledged as `BROKER_ID`, answering
/// its client order id.
fn accepted(shell: &mut Shell, ports: &Ports<'_>, quantity: &str) -> String {
    let ran = shell.run(
        handoff(INTENT, common::AGENT, opening(AAPL, quantity, "150")),
        ports,
    );
    let id = ran
        .submissions()
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .expect("the opening is sent");
    shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(broker_order(
            BROKER_ID,
            Some(&id),
            AAPL,
            Side::Buy,
            quantity,
            "0",
            "new",
        )))),
        ports,
    );
    id
}

/// The E2 state: the entry acknowledged, then read back `filled` whole, and its fill never
/// ingested.
fn filled_unbooked(ports: &Ports<'_>, quantity: &str) -> (Shell, String) {
    let mut shell = fresh(ports);
    let id = accepted(&mut shell, ports, quantity);
    shell.run(
        Input::BrokerUpdate(BrokerUpdate::Order(filled(&id, quantity))),
        ports,
    );
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.state),
        Some(OrderState::Filled),
        "the entry reads filled"
    );
    assert!(
        shell.state.fills().is_empty(),
        "and no fill of it is booked: the E2 state"
    );
    (shell, id)
}

/// The broker's answer to the lookup of `id`: filled `quantity` of its own `quantity`.
fn filled(id: &str, quantity: &str) -> BrokerOrder {
    broker_order(
        BROKER_ID,
        Some(id),
        AAPL,
        Side::Buy,
        quantity,
        quantity,
        "filled",
    )
}

/// One `FILL` activity as Alpaca reports it: its own id, the order's broker id, no client order
/// id.
fn activity(fill_id: &str, quantity: &str) -> BrokerFill {
    BrokerFill {
        broker_order_id: Some(BROKER_ID.to_owned()),
        ..broker_fill(fill_id, None, quantity, "150")
    }
}

fn key(id: &str) -> ClientOrderId {
    ClientOrderId::parse(id).expect("an id this platform derived")
}

/// The new owner's epoch: one past the process that wrote the stream.
fn next_epoch(shell: &Shell) -> WriterEpoch {
    WriterEpoch(shell.epoch.0.saturating_add(1))
}

fn book(
    shell: &Shell,
    orders: &[BrokerOrder],
    fills: &[BrokerFill],
    ports: &Ports<'_>,
) -> Result<Vec<Effect>, ExecutorError> {
    book_own_fills(
        &shell.state,
        next_epoch(shell),
        RiskClock::from_secs(AT),
        orders,
        fills,
        ports,
    )
}

/// The fill records among `effects`: `(event type, fill id, client order id, quantity)`.
fn fill_records(effects: &[Effect]) -> Vec<(String, String, String, String)> {
    let member = |payload: &Value, name: &str| {
        payload
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Journal(draft) if draft.event_type.ends_with("FillApplied") => Some((
                draft.event_type.clone(),
                member(&draft.payload, "fill_id"),
                member(&draft.payload, "client_order_id"),
                member(&draft.payload, "qty_gross"),
            )),
            _ => None,
        })
        .collect()
}

/// Commits every draft among `effects` at the stream's next `seq`, as the journal would.
fn commit(shell: &mut Shell, effects: &[Effect]) {
    for effect in effects {
        let Effect::Journal(draft) = effect else {
            panic!("the own-fill reconciliation drafts journal records only: {effect:?}");
        };
        let seq = shell.head().0.saturating_add(1);
        shell
            .fold_one(&FoldedEvent {
                stream: ACCOUNT_STREAM.to_owned(),
                seq: Seq(seq),
                event_id: draft.event_id.clone(),
                event_type: draft.event_type.clone(),
                causation_id: draft.causation_id.clone(),
                payload: draft.payload.clone(),
            })
            .unwrap_or_else(|e| panic!("{} folds back: {e}", draft.event_type));
    }
}

/// The E2 case: the entry is owed, its one fill is booked as a `LateFillApplied` (trading spec
/// §5.7: a fill of an order already terminal) at the broker's price, nothing but journal records
/// is drafted, and once committed the position is held and nothing more is owed or booked.
#[test]
fn the_e2_entry_filled_at_the_broker_and_never_booked_books_one_late_fill() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (mut shell, id) = filled_unbooked(&ports, "1");
    assert_eq!(
        owed_orders(&shell.state),
        vec![key(&id)],
        "the entry is owed"
    );

    let effects = book(&shell, &[filled(&id, "1")], &[activity("f-1", "1")], &ports)
        .unwrap_or_else(|e| panic!("books: {e}"));

    assert!(
        effects
            .iter()
            .all(|effect| matches!(effect, Effect::Journal(_))),
        "journal records only, never a broker request: {effects:?}"
    );
    assert_eq!(
        fill_records(&effects),
        vec![(
            "LateFillApplied".to_owned(),
            "f-1".to_owned(),
            id.clone(),
            "1".to_owned()
        )]
    );
    let price = effects.iter().find_map(|effect| match effect {
        Effect::Journal(draft) if draft.event_type == "LateFillApplied" => {
            draft.payload.get("price").and_then(Value::as_str)
        }
        _ => None,
    });
    assert_eq!(price, Some("150"), "the broker's fill price");

    commit(&mut shell, &effects);
    assert_eq!(
        shell.state.positions().get(&instrument(AAPL)).copied(),
        Some(signed_qty("1")),
        "the share is held"
    );
    assert!(owed_orders(&shell.state).is_empty(), "nothing more is owed");
    let again = book(&shell, &[filled(&id, "1")], &[activity("f-1", "1")], &ports)
        .unwrap_or_else(|e| panic!("a second pass answers: {e}"));
    assert!(again.is_empty(), "a booked fill is never booked twice");
}

/// An order the journal holds as working, filled at the broker, books an ordinary `FillApplied`
/// and moves to `filled` by the fill (§5.7).
#[test]
fn a_working_order_filled_at_the_broker_books_its_fill_and_its_state() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, "2");

    let effects = book(&shell, &[filled(&id, "2")], &[activity("f-1", "2")], &ports)
        .unwrap_or_else(|e| panic!("books: {e}"));

    let kinds: Vec<String> = fill_records(&effects).into_iter().map(|r| r.0).collect();
    assert_eq!(kinds, ["FillApplied"]);
    commit(&mut shell, &effects);
    assert_eq!(
        shell.state.order(&key(&id)).map(|order| order.state),
        Some(OrderState::Filled)
    );
}

/// An order the broker has not filled owes nothing, and a fill of another broker order (one the
/// journal does not own) is never booked: nothing is adopted (DEC-955 item 2).
#[test]
fn nothing_the_journal_does_not_own_is_booked() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let mut shell = fresh(&ports);
    let id = accepted(&mut shell, &ports, "2");
    let resting = broker_order(BROKER_ID, Some(&id), AAPL, Side::Buy, "2", "0", "new");
    let foreign = BrokerFill {
        broker_order_id: Some("someone-else".to_owned()),
        ..broker_fill("f-other", None, "2", "150")
    };

    let effects = book(&shell, &[resting], &[foreign.clone()], &ports)
        .unwrap_or_else(|e| panic!("answers: {e}"));
    assert!(effects.is_empty(), "nothing filled, nothing booked");

    let (shell, id) = filled_unbooked(&ports, "1");
    let effects = book(
        &shell,
        &[filled(&id, "1")],
        &[foreign, activity("f-1", "1")],
        &ports,
    )
    .unwrap_or_else(|e| panic!("books: {e}"));
    let booked: Vec<String> = fill_records(&effects).into_iter().map(|r| r.1).collect();
    assert_eq!(booked, ["f-1"], "only the owned order's fill");
}

/// Every disagreement between the broker's word and the journal stops the pass with nothing
/// drafted, naming the order's opaque id and a reason code (DEC-955 item 4, rule 3).
#[test]
fn a_disagreement_drafts_nothing_and_names_its_reason() {
    let ids = TestIds;
    let mandates = FixedMandate::covering(&[AAPL, CPHC]);
    let instruments = FixedInstruments;
    let config = config();
    let ports = ports(&ids, &mandates, &instruments, &config);
    let (shell, id) = filled_unbooked(&ports, "1");
    let other_instrument = BrokerFill {
        instrument: instrument(CPHC),
        ..activity("f-1", "1")
    };
    let other_side = BrokerFill {
        side: Side::Sell,
        ..activity("f-1", "1")
    };
    let named_elsewhere = BrokerFill {
        client_order_id: Some("md-00000100000000000005000009".to_owned()),
        ..activity("f-1", "1")
    };
    let sold = BrokerOrder {
        side: Side::Sell,
        ..filled(&id, "1")
    };
    let overfilled = broker_order(BROKER_ID, Some(&id), AAPL, Side::Buy, "1", "2", "filled");
    let unknown = filled("md-00000100000000000005000009", "1");
    let cases: Vec<(&str, Vec<BrokerOrder>, Vec<BrokerFill>)> = vec![
        ("fills_short", vec![filled(&id, "1")], vec![]),
        (
            "fills_exceed",
            vec![filled(&id, "1")],
            vec![activity("f-1", "1"), activity("f-2", "1")],
        ),
        ("not_ours", vec![unknown], vec![activity("f-1", "1")]),
        (
            "order_twice",
            vec![filled(&id, "1"), filled(&id, "1")],
            vec![activity("f-1", "1")],
        ),
        ("order_differs", vec![sold], vec![activity("f-1", "1")]),
        ("overfilled", vec![overfilled], vec![activity("f-1", "1")]),
        (
            "fill_differs",
            vec![filled(&id, "1")],
            vec![other_instrument],
        ),
        ("fill_differs", vec![filled(&id, "1")], vec![other_side]),
        (
            "fill_names_another_order",
            vec![filled(&id, "1")],
            vec![named_elsewhere],
        ),
    ];
    for (reason, orders, fills) in cases {
        let refused = book(&shell, &orders, &fills, &ports);
        match refused {
            Err(ExecutorError::OwnFillMismatch {
                reason: found,
                client_order_id,
            }) => {
                assert_eq!(found, reason, "{reason}");
                assert!(client_order_id.starts_with("md-"), "{reason}: an opaque id");
            }
            other => panic!("{reason}: {other:?}"),
        }
    }
    let mut booked = shell;
    let effects = book(
        &booked,
        &[filled(&id, "1")],
        &[activity("f-1", "1")],
        &ports,
    )
    .unwrap_or_else(|e| panic!("books: {e}"));
    commit(&mut booked, &effects);
    let less = broker_order(BROKER_ID, Some(&id), AAPL, Side::Buy, "1", "0", "canceled");
    match book(&booked, &[less], &[], &ports) {
        Err(error @ ExecutorError::OwnFillMismatch { .. }) => {
            assert_eq!(error.code(), "own_fill_mismatch");
            assert!(error.to_string().contains("broker_filled_less"), "{error}");
        }
        other => panic!("the broker filled less than the stream booked: {other:?}"),
    }
}

/// The parts one entry's fills split into, which of them the stream already booked, how many
/// fills of other orders the activity list carries, and a rotation of that list.
fn scenario() -> impl Strategy<Value = (Vec<u8>, Vec<bool>, usize, usize)> {
    prop::collection::vec(1u8..=4, 1..=4).prop_flat_map(|parts| {
        let n = parts.len();
        (
            Just(parts),
            prop::collection::vec(any::<bool>(), n),
            0usize..=3,
            0usize..=8,
        )
    })
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Whichever of an entry's fill records are missing, the pass books exactly those, each
    /// once, whatever else the activity list holds and in whatever order; the position then
    /// equals what the broker filled; a second pass books nothing; and a replay of the journal
    /// alone folds to the same positions and fills as the live state.
    #[test]
    fn exactly_the_missing_fill_records_are_booked(
        (parts, booked_before, foreign, rotation) in scenario()
    ) {
        let ids = TestIds;
        let mandates = FixedMandate::covering(&[AAPL]);
        let instruments = FixedInstruments;
        let config = config();
        let ports = ports(&ids, &mandates, &instruments, &config);
        let total: u32 = parts.iter().map(|part| u32::from(*part)).sum();
        let total = total.to_string();
        let (mut shell, id) = filled_unbooked(&ports, &total);
        let names: Vec<String> = (0..parts.len()).map(|n| format!("f-{n}")).collect();
        for ((name, part), booked) in names.iter().zip(&parts).zip(&booked_before) {
            if *booked {
                let seq = shell.head().0.saturating_add(1);
                shell
                    .fold_one(&event(
                        ACCOUNT_STREAM,
                        seq,
                        "LateFillApplied",
                        with_clock(
                            &[
                                ("fill_id", text(name)),
                                ("client_order_id", text(&id)),
                                ("instrument_id", text(AAPL)),
                                ("side", text("buy")),
                                ("qty_gross", text(&part.to_string())),
                                ("price", text("150")),
                            ],
                            50,
                        ),
                    ))
                    .unwrap_or_else(|e| panic!("a booked part folds: {e}"));
            }
        }
        let expected: BTreeSet<String> = names
            .iter()
            .zip(&booked_before)
            .filter(|(_, booked)| !**booked)
            .map(|(name, _)| name.clone())
            .collect();
        let mut activities: Vec<BrokerFill> = names
            .iter()
            .zip(&parts)
            .map(|(name, part)| activity(name, &part.to_string()))
            .collect();
        activities.extend((0..foreign).map(|n| BrokerFill {
            broker_order_id: Some(format!("other-{n}")),
            ..broker_fill(&format!("x-{n}"), None, "1", "150")
        }));
        let len = activities.len();
        activities.rotate_left(rotation % len);
        let owed = owed_orders(&shell.state);
        prop_assert_eq!(owed.is_empty(), expected.is_empty());

        let effects = book(&shell, &[filled(&id, &total)], &activities, &ports)
            .unwrap_or_else(|e| panic!("books: {e}"));
        let drafted: Vec<String> = fill_records(&effects).into_iter().map(|r| r.1).collect();
        let unique: BTreeSet<String> = drafted.iter().cloned().collect();
        prop_assert_eq!(drafted.len(), unique.len(), "each fill once");
        prop_assert_eq!(&unique, &expected);

        commit(&mut shell, &effects);
        prop_assert_eq!(
            shell.state.positions().get(&instrument(AAPL)).copied(),
            Some(signed_qty(&total))
        );
        prop_assert!(owed_orders(&shell.state).is_empty());
        let again = book(&shell, &[filled(&id, &total)], &activities, &ports)
            .unwrap_or_else(|e| panic!("a second pass answers: {e}"));
        prop_assert!(fill_records(&again).is_empty(), "nothing is booked twice");
        let replayed = shell.replay().unwrap_or_else(|e| panic!("replays: {e}"));
        prop_assert_eq!(replayed.positions(), shell.state.positions());
        prop_assert_eq!(replayed.fills(), shell.state.fills());
    }
}
