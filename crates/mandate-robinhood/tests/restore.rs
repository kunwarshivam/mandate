//! C3 tests part 2 (E7-6; journal spec §9.16, DEC-860 item 7, DEC-869 item 4, DEC-870): the
//! connector's `ClientOrderId` → `order_id` map rebuilt from the account stream at start. A key
//! with one distinct id cancels by it; no id, two ids, an in-doubt place, or version-1 records give
//! `NotSent` (`no_order_id`) with nothing called. Oracles: records typed from §9.16's member list
//! and a recording tool double; the property counts distinct ids its own way. The restart against
//! `mandate-rh-sim`, `a_cancel_after_a_restart_finds_its_order_id_from_the_journal`, is part 3.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::parse;
use mandate_executor::{
    BrokerConnector, BrokerOrder, BrokerOutcome, BrokerRequest, ClientOrderId, ConnectorError,
    EventId, FoldedEvent, IntentId, Seq,
};
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Price, Qty};
use mandate_robinhood::{RobinhoodConnector, Tools};
use proptest::test_runner::{Config, TestRunner};
use serde_json::{Value, json};

const ACCOUNT: &str = "5QR00001";
const CANCEL: &str = "cancel_equity_order";
const NO_ORDER_ID: ConnectorError = ConnectorError::NotSent {
    code: "no_order_id",
};

/// Every call the connector makes, answered for a cancel with the named order in `state`.
struct Recording {
    calls: Calls,
    state: &'static str,
}

impl Tools for Recording {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> Result<String, McpError> {
        self.calls
            .borrow_mut()
            .push((class, tool, arguments.clone()));
        let record = json!({"id": arguments["order_id"], "state": self.state, "quantity": "2",
            "filled_quantity": "0", "limit_price": "499", "stop_price": null});
        Ok(json!({ "structuredContent": record }).to_string())
    }
}

type Calls = Rc<RefCell<Vec<(CallClass, &'static str, Value)>>>;

fn restored(
    records: &[FoldedEvent],
    state: &'static str,
) -> (RobinhoodConnector<Recording>, Calls) {
    let calls = Calls::default();
    let tools = Recording {
        calls: Rc::clone(&calls),
        state,
    };
    let connector = RobinhoodConnector::restore(tools, ACCOUNT.to_owned(), records).unwrap();
    (connector, calls)
}

fn cancel(
    c: &mut RobinhoodConnector<Recording>,
    key: &ClientOrderId,
) -> Result<BrokerOutcome, ConnectorError> {
    let request = BrokerRequest::Cancel {
        client_order_id: key.clone(),
    };
    let mut future = pin!(c.call(&request));
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(outcome) => outcome,
        Poll::Pending => panic!("the connector awaited something other than the seam"),
    }
}

fn key(intent: &str) -> ClientOrderId {
    ClientOrderId::for_intent(&IntentId(EventId(intent.to_owned()))).unwrap()
}

fn successor(origin: &str) -> ClientOrderId {
    ClientOrderId::for_replacement(&EventId(origin.to_owned())).unwrap()
}

/// The account stream under construction, numbered in journal order.
#[derive(Default)]
struct Stream(Vec<FoldedEvent>);

impl Stream {
    fn push(&mut self, event_type: &str, payload: Value) {
        let seq = u64::try_from(self.0.len()).unwrap() + 1;
        self.0.push(FoldedEvent {
            stream: "account".to_owned(),
            seq: Seq(seq),
            event_id: EventId(format!("01JEVENT{seq:018}")),
            event_type: event_type.to_owned(),
            causation_id: None,
            payload: parse(payload.to_string().as_bytes()).unwrap(),
        });
    }

    /// An `OrderSubmitted` journaled before the place (`AGENTS.md` rule 5).
    fn submitted(&mut self, key: &ClientOrderId, symbol: &str, side: &str) {
        let payload = json!({"client_order_id": key.as_str(), "instrument_id": symbol,
            "side": side, "qty": "2", "type": "limit", "tif": "day", "limit_price": "499",
            "attempt": 1});
        self.push("OrderSubmitted", payload);
    }

    /// An `OrderStateChanged`: version 1's members, and at version 2 `broker_order_id`, whose
    /// outer `None` is a version-1 record and inner `None` a `null`.
    fn changed(
        &mut self,
        key: &ClientOrderId,
        state: &str,
        id: Option<Option<&str>>,
        links: Value,
    ) {
        let mut payload = json!({"client_order_id": key.as_str(), "state": state,
            "risk_clock": "2026-10-09T14:30:00Z", "attempted": null, "broker_status": null,
            "filled_qty": null, "reject_code": null, "replaces": null, "replaced_by": null,
            "replaced_by_broker_order_id": null, "lookup": null, "ignored": false,
            "cancel_requested": false, "cancel_confirmed": false, "cancel_overdue": false,
            "adopted": false, "ladder_step": false});
        for (member, value) in links.as_object().into_iter().flatten() {
            payload[member] = value.clone();
        }
        if let Some(id) = id {
            payload["broker_order_id"] = json!(id);
        }
        self.push("OrderStateChanged", payload);
    }
}

fn order(key: &ClientOrderId, id: &str, symbol: &str, side: Side) -> BrokerOrder {
    BrokerOrder {
        broker_order_id: id.to_owned(),
        client_order_id: Some(key.as_str().to_owned()),
        instrument: InstrumentId::new(symbol).unwrap(),
        side,
        qty: Qty::parse("2").unwrap(),
        filled_qty: Qty::ZERO,
        limit_price: Some(Price::parse("499").unwrap()),
        stop_price: None,
        status: "accepted".to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    }
}

fn cancelled_by(id: &str) -> (CallClass, &'static str, Value) {
    let arguments = json!({"account_number": ACCOUNT, "order_id": id});
    (CallClass::RiskReducing, CANCEL, arguments)
}

const NONE: Value = Value::Null;

#[test]
#[ignore = "pending E7-6"]
fn a_restored_key_cancels_by_its_one_journaled_id_with_its_own_instrument_and_side() {
    let (spy, qqq) = (key("01JSPY"), key("01JQQQ"));
    let mut s = Stream::default();
    s.submitted(&spy, "SPY", "buy");
    s.submitted(&qqq, "QQQ", "sell");
    s.changed(&spy, "submitted", Some(None), NONE);
    s.changed(&spy, "accepted", Some(Some("rh-spy")), NONE);
    s.changed(&qqq, "accepted", Some(Some("rh-qqq")), NONE);
    s.changed(&spy, "partially_filled", Some(None), NONE);
    s.changed(&spy, "accepted", Some(Some("rh-spy")), NONE);
    s.changed(&spy, "partially_filled", Some(None), NONE);
    let (mut c, calls) = restored(&s.0, "confirmed");
    let outcome = cancel(&mut c, &spy).unwrap();
    assert_eq!(
        outcome,
        BrokerOutcome::Order(order(&spy, "rh-spy", "SPY", Side::Buy))
    );
    let outcome = cancel(&mut c, &qqq).unwrap();
    assert_eq!(
        outcome,
        BrokerOutcome::Order(order(&qqq, "rh-qqq", "QQQ", Side::Sell))
    );
    assert_eq!(
        *calls.borrow(),
        [cancelled_by("rh-spy"), cancelled_by("rh-qqq")]
    );
}

/// A known order beside the one asked about, so a connector that guesses has something to guess.
fn beside_a_known_order(s: &mut Stream) {
    let known = key("01JKNOWN");
    s.submitted(&known, "SPY", "buy");
    s.changed(&known, "accepted", Some(Some("rh-known")), NONE);
}

fn refused_with_nothing_called(records: &[FoldedEvent], keys: &[&ClientOrderId]) {
    let (mut c, calls) = restored(records, "cancelled");
    for key in keys {
        assert_eq!(cancel(&mut c, key), Err(NO_ORDER_ID), "{}", key.as_str());
    }
    assert!(calls.borrow().is_empty(), "CN-7: {:?}", calls.borrow());
}

#[test]
#[ignore = "pending E7-6"]
fn a_key_whose_records_carry_no_id_is_not_sent_and_nothing_is_called() {
    let lone = key("01JLONE");
    let mut s = Stream::default();
    beside_a_known_order(&mut s);
    s.submitted(&lone, "SPY", "buy");
    s.changed(&lone, "submitted", Some(None), NONE);
    s.changed(&lone, "accepted", Some(None), NONE);
    refused_with_nothing_called(&s.0, &[&lone]);
}

#[test]
#[ignore = "pending E7-6"]
fn two_different_ids_under_one_key_are_not_sent_and_nothing_is_called() {
    let torn = key("01JTORN");
    let mut s = Stream::default();
    s.submitted(&torn, "SPY", "buy");
    s.changed(&torn, "accepted", Some(Some("rh-first")), NONE);
    s.changed(&torn, "accepted", Some(Some("rh-second")), NONE);
    s.changed(&torn, "accepted", Some(Some("rh-first")), NONE);
    s.changed(&torn, "partially_filled", Some(None), NONE);
    beside_a_known_order(&mut s);
    refused_with_nothing_called(&s.0, &[&torn]);
}

#[test]
#[ignore = "pending E7-6"]
fn an_in_doubt_place_is_not_sent_neither_placed_again_nor_guessed() {
    let (silent, lost) = (key("01JSILENT"), key("01JLOST"));
    let mut s = Stream::default();
    beside_a_known_order(&mut s);
    s.submitted(&silent, "SPY", "buy");
    s.submitted(&lost, "SPY", "buy");
    s.changed(&lost, "unknown", Some(None), NONE);
    refused_with_nothing_called(&s.0, &[&silent, &lost]);
}

/// DEC-870: a replacement successor's own record carries its own id, and its instrument and side
/// are those of the order it replaces, through every link of the chain.
#[test]
#[ignore = "pending E7-6"]
fn a_replacement_successor_is_found_by_its_own_id() {
    let (first, second, third) = (
        key("01JFIRST"),
        successor("01JREPLACE1"),
        successor("01JREPLACE2"),
    );
    let (other, orphan) = (key("01JOTHER"), successor("01JREPLACE3"));
    let mut s = Stream::default();
    s.submitted(&first, "QQQ", "sell");
    s.changed(&first, "accepted", Some(Some("rh-1")), NONE);
    let to_second = json!({"replaced_by": second.as_str(), "replaced_by_broker_order_id": "rh-2"});
    s.changed(&first, "replaced", Some(Some("rh-1")), to_second);
    s.changed(
        &second,
        "accepted",
        Some(Some("rh-2")),
        json!({"replaces": first.as_str()}),
    );
    let to_third = json!({"replaced_by": third.as_str(), "replaced_by_broker_order_id": "rh-3"});
    s.changed(&second, "replaced", Some(Some("rh-2")), to_third);
    s.changed(
        &third,
        "accepted",
        Some(Some("rh-3")),
        json!({"replaces": second.as_str()}),
    );
    s.submitted(&other, "SPY", "buy");
    let to_orphan = json!({"replaced_by": orphan.as_str(), "replaced_by_broker_order_id": null});
    s.changed(&other, "replaced", Some(Some("rh-other")), to_orphan);
    s.changed(
        &orphan,
        "accepted",
        Some(None),
        json!({"replaces": other.as_str()}),
    );
    let (mut c, calls) = restored(&s.0, "confirmed");
    for (key, id) in [(&third, "rh-3"), (&second, "rh-2"), (&first, "rh-1")] {
        let expected = order(key, id, "QQQ", Side::Sell);
        assert_eq!(cancel(&mut c, key), Ok(BrokerOutcome::Order(expected)));
    }
    assert_eq!(
        cancel(&mut c, &orphan),
        Err(NO_ORDER_ID),
        "null: unrecoverable"
    );
    let expected = [
        cancelled_by("rh-3"),
        cancelled_by("rh-2"),
        cancelled_by("rh-1"),
    ];
    assert_eq!(*calls.borrow(), expected);
}

#[test]
#[ignore = "pending E7-6"]
fn version_1_records_give_no_recoverable_id() {
    let (old, new) = (key("01JOLD"), successor("01JREPLACEV1"));
    let mut s = Stream::default();
    s.submitted(&old, "SPY", "buy");
    s.changed(&old, "accepted", None, NONE);
    let link = json!({"replaced_by": new.as_str(), "replaced_by_broker_order_id": "rh-new"});
    s.changed(&old, "replaced", None, link);
    s.changed(&new, "accepted", None, json!({"replaces": old.as_str()}));
    refused_with_nothing_called(&s.0, &[&old, &new]);
}

/// Random streams over four keys. The oracle counts each key's distinct non-null ids from the
/// generated choices, never from the records: one id cancels by it, none or two is `NotSent`.
#[test]
#[ignore = "pending E7-6"]
fn a_key_cancels_exactly_when_its_records_carry_one_distinct_id() {
    let keys: Vec<ClientOrderId> = (0..4).map(|n| key(&format!("01JPROP{n}"))).collect();
    let choice = (0..keys.len(), 0..6_usize);
    let mut runner = TestRunner::new(Config::with_cases(256));
    let verdict = runner.run(&proptest::collection::vec(choice, 0..14), |picks| {
        let mut s = Stream::default();
        let mut ids: BTreeMap<usize, BTreeSet<String>> = BTreeMap::new();
        for key in &keys {
            s.submitted(key, "SPY", "buy");
        }
        for (n, pick) in picks {
            let id = format!("rh-{pick}");
            match pick {
                0 => s.changed(&keys[n], "accepted", None, NONE),
                1 => s.changed(&keys[n], "partially_filled", Some(None), NONE),
                2 => s.push(
                    "ClockAdvanced",
                    json!({"risk_clock": "2026-10-09T14:31:00Z"}),
                ),
                _ => {
                    s.changed(&keys[n], "accepted", Some(Some(&id)), NONE);
                    ids.entry(n).or_default().insert(id);
                }
            }
        }
        let (mut c, calls) = restored(&s.0, "cancelled");
        let mut expected = Vec::new();
        for (n, key) in keys.iter().enumerate() {
            let one = ids
                .get(&n)
                .filter(|set| set.len() == 1)
                .and_then(|set| set.first());
            let outcome = cancel(&mut c, key);
            match one {
                Some(id) => {
                    let accepted = BrokerOutcome::CancelAccepted {
                        client_order_id: key.as_str().to_owned(),
                    };
                    proptest::prop_assert_eq!(outcome, Ok(accepted));
                    expected.push(cancelled_by(id));
                }
                None => proptest::prop_assert_eq!(outcome, Err(NO_ORDER_ID)),
            }
        }
        proptest::prop_assert_eq!(&*calls.borrow(), &expected);
        Ok(())
    });
    verdict.unwrap();
}
