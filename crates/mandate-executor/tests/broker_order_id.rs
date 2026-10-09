//! Journal spec §9.16 (v0.34, DEC-869 item 3): the executor writes `OrderStateChanged` at
//! `schema_version` 2, and `broker_order_id` is non-null exactly on a record written from a broker
//! order record carrying the order's broker id (a place answer, a query answer, a pushed update,
//! a cancel answer that is the order, a reconciliation adoption), and `null` on every other: a
//! fill, a timeout, a cancel request, a lookup's `absent`, a confirmed cancel. A replacement's
//! new order is named by `replaced_by_broker_order_id`, which the successor's own record carries
//! as its `broker_order_id`. The fold never reads the member.
//!
//! The oracle is the id each test hands the broker record, and the spec's list of writers typed
//! here; the fold oracle replays the same stream with the member rewritten.

mod common;

use common::{
    AGENT, FixedInstruments, FixedMandate, Ran, Shell, TestIds, agent, broker_fill, broker_order,
    config, handoff, instrument, opening, ports, scope, snapshot, stream_opened,
};
use mandate_accounting::Side;
use mandate_canon::{Key, Value};
use mandate_executor::{
    BrokerOrder, BrokerOutcome, BrokerUnknown, BrokerUpdate, Command, ExecutorConfig,
    ExecutorState, Input, Ports, ReconcileReason, fold,
};
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

const AAPL: &str = FixedInstruments::LIQUID_EQUITY;
const INTENT: &str = "01JABCDEFGHJKMNPQRSTVWXYZ0";
const ID: &str = "rh-order-1";

struct Fixture(TestIds, FixedMandate, FixedInstruments, ExecutorConfig);

impl Fixture {
    fn new() -> Self {
        Self(
            TestIds,
            FixedMandate::covering(&[AAPL]),
            FixedInstruments,
            config(),
        )
    }

    fn ports(&self) -> Ports<'_> {
        ports(&self.0, &self.1, &self.2, &self.3)
    }
}

/// One `OrderStateChanged` as written: its order, state, schema version and `broker_order_id`.
type Written = (String, String, u64, Option<String>);

/// Every `OrderStateChanged` drafted in `ran`. A draft without the member is not version 2.
fn written(ran: &Ran) -> Vec<Written> {
    fn member<'a>(payload: &'a Value, name: &str) -> &'a str {
        payload
            .get(name)
            .and_then(Value::as_str)
            .unwrap_or_default()
    }
    ran.drafts
        .iter()
        .filter(|draft| draft.event_type == "OrderStateChanged")
        .map(|draft| {
            let id = match draft.payload.get("broker_order_id") {
                Some(Value::Null) => None,
                Some(Value::Str(id)) => Some(id.clone()),
                other => panic!("`broker_order_id` is text or null on every record: {other:?}"),
            };
            let order = member(&draft.payload, "client_order_id");
            let state = member(&draft.payload, "state");
            (order.to_owned(), state.to_owned(), draft.schema_version, id)
        })
        .collect()
}

fn row(order: &str, state: &str, id: Option<&str>) -> Written {
    (order.to_owned(), state.to_owned(), 2, id.map(str::to_owned))
}

fn order(id: &str, ours: &str, filled: &str, status: &str) -> BrokerOrder {
    broker_order(id, Some(ours), AAPL, Side::Buy, "10", filled, status)
}

fn answer(order: BrokerOrder) -> Input {
    Input::Broker(Ok(BrokerOutcome::Order(order)))
}

/// A ready shell with `AGENT`'s opening of 10 AAPL at 150 sent, and the order's id.
fn sent(ports: &Ports<'_>) -> (Shell, String) {
    let mut shell = Shell::new(1);
    shell.fold_one(&stream_opened()).expect("folds");
    let mut shell = shell.restart_ready(ports);
    let ran = shell.run(handoff(INTENT, AGENT, opening(AAPL, "10", "150")), ports);
    let id = ran.submissions()[0].client_order_id.as_str().to_owned();
    assert!(
        written(&ran).iter().all(|w| w.3.is_none()),
        "nothing answered yet"
    );
    (shell, id)
}

/// The same, with the place answered as broker order [`ID`], `new`.
fn placed(ports: &Ports<'_>) -> (Shell, String, Ran) {
    let (mut shell, id) = sent(ports);
    let ran = shell.run(
        Input::Broker(Ok(BrokerOutcome::Submitted(order(ID, &id, "0", "new")))),
        ports,
    );
    (shell, id, ran)
}

#[test]
#[ignore = "pending E7-6"]
fn a_place_answer_and_a_pushed_update_carry_the_id_and_a_fill_none() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, ran) = placed(&ports);
    assert_eq!(written(&ran), [row(&id, "accepted", Some(ID))]);
    let pushed = order(ID, &id, "4", "partially_filled");
    let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Order(pushed)), &ports);
    assert_eq!(written(&ran), [row(&id, "partially_filled", Some(ID))]);
    let fill = broker_fill("fill-1", Some(&id), "10", "150");
    let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(fill)), &ports);
    let ours: Vec<Written> = written(&ran).into_iter().filter(|w| w.0 == id).collect();
    assert_eq!(
        ours,
        [row(&id, "filled", None)],
        "a fill carries no order record"
    );
}

#[test]
#[ignore = "pending E7-6"]
fn a_timeout_an_absence_and_an_empty_id_carry_none_and_the_query_answer_the_id() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id) = sent(&ports);
    let ran = shell.run(Input::Broker(Err(BrokerUnknown::Timeout)), &ports);
    assert_eq!(written(&ran), [row(&id, "unknown", None)]);
    let absent = BrokerOutcome::Absent {
        client_order_id: id.clone(),
    };
    let ran = shell.run(Input::Broker(Ok(absent)), &ports);
    assert_eq!(written(&ran), [row(&id, "unknown", None)]);
    let ran = shell.run(answer(order("", &id, "0", "accepted")), &ports);
    let empty = "§9.2's `text`: an empty id is no id, written `null`";
    assert_eq!(written(&ran), [row(&id, "accepted", None)], "{empty}");
    let ran = shell.run(answer(order("rh-order-7", &id, "0", "accepted")), &ports);
    assert_eq!(written(&ran), [row(&id, "accepted", Some("rh-order-7"))]);
}

#[test]
#[ignore = "pending E7-6"]
fn a_cancel_request_carries_none_its_order_answer_the_id_and_its_confirmation_none() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = placed(&ports);
    let cancel = Command::CancelOpenings {
        agent: agent(AGENT),
        instrument: instrument(AAPL),
    };
    let ran = shell.run(Input::Command(cancel), &ports);
    assert_eq!(written(&ran), [row(&id, "pending_cancel", None)]);
    let ran = shell.run(answer(order(ID, &id, "0", "pending_cancel")), &ports);
    assert_eq!(written(&ran), [row(&id, "pending_cancel", Some(ID))]);
    let confirmed = BrokerOutcome::CancelAccepted {
        client_order_id: id.clone(),
    };
    let ran = shell.run(Input::Broker(Ok(confirmed)), &ports);
    assert_eq!(written(&ran), [row(&id, "canceled", None)]);
}

#[test]
#[ignore = "pending E7-6"]
fn a_reconciliation_adopts_with_the_open_orders_id_and_a_missing_order_with_none() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = placed(&ports);
    let mut taken = snapshot(shell.head().0, ReconcileReason::Startup);
    taken.open_orders = vec![order(ID, &id, "0", "pending_cancel")];
    let ran = shell.run(Input::BrokerSnapshot(taken), &ports);
    assert_eq!(written(&ran), [row(&id, "pending_cancel", Some(ID))]);
    let taken = snapshot(shell.head().0, ReconcileReason::Startup);
    let ran = shell.run(Input::BrokerSnapshot(taken), &ports);
    assert_eq!(written(&ran), [row(&id, "unknown", None)]);
}

/// §9.16 reads the member as "the broker's own id for the order named by `client_order_id`": the
/// replaced order's record carries its own id, and the successor's, written from the same record,
/// carries the new id that record gives it (`replaced_by_broker_order_id`), or `null` without one.
#[test]
#[ignore = "pending E7-6"]
fn each_order_of_a_replacement_carries_its_own_broker_id() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    for new_id in [Some("rh-order-2"), None] {
        let (mut shell, id, _) = placed(&ports);
        let mut replaced = order(ID, &id, "0", "replaced");
        replaced.replaced_by_broker_order_id = new_id.map(str::to_owned);
        let ran = shell.run(Input::BrokerUpdate(BrokerUpdate::Order(replaced)), &ports);
        let rows = written(&ran);
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0], row(&id, "replaced", Some(ID)));
        assert_ne!(rows[1].0, id, "the successor has its own client order id");
        assert_eq!(rows[1], row(&rows[1].0, "accepted", new_id));
    }
}

/// The fold does not read the member (§9.16): a stream replays to the same state whatever
/// `broker_order_id` each `OrderStateChanged` carries, and without it, as version 1 wrote it.
#[test]
fn the_fold_never_reads_the_brokers_order_id() {
    let fixture = Fixture::new();
    let ports = fixture.ports();
    let (mut shell, id, _) = placed(&ports);
    let fill = broker_fill("fill-1", Some(&id), "4", "150");
    shell.run(Input::BrokerUpdate(BrokerUpdate::Fill(fill)), &ports);
    let replay = |rewrite: &dyn Fn(usize) -> Option<Value>| {
        let mut state = ExecutorState::new(scope());
        for (n, event) in shell.account_journal.iter().enumerate() {
            let mut event = event.clone();
            if let (true, Value::Object(payload)) =
                (event.event_type == "OrderStateChanged", &mut event.payload)
            {
                let key = Key::new("broker_order_id").expect("a key");
                match rewrite(n) {
                    Some(value) => payload.insert(key, value),
                    None => payload.remove("broker_order_id"),
                };
            }
            fold(&mut state, &event).expect("the stream folds");
        }
        state
    };
    let without = replay(&|_| None);
    let changes = shell
        .account_journal
        .iter()
        .filter(|e| e.event_type == "OrderStateChanged")
        .count();
    assert!(changes >= 2, "the stream holds order state changes");
    let ids = prop::collection::vec(prop::option::of("[a-z0-9-]{1,12}"), 1..64);
    let mut runner = TestRunner::new(Config {
        cases: 32,
        failure_persistence: None,
        ..Config::default()
    });
    let outcome = runner.run(&ids, |ids| {
        let with = replay(&|n| {
            let id = ids.get(n % ids.len()).cloned().flatten();
            Some(id.map_or(Value::Null, Value::Str))
        });
        prop_assert_eq!(&with, &without);
        Ok(())
    });
    assert!(outcome.is_ok(), "{outcome:?}");
}
