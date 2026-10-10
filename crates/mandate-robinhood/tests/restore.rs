//! C3 tests part 2 (E7-6; journal spec §9.16, DEC-860 item 7, DEC-869 item 4, DEC-870): the
//! connector's `ClientOrderId` → `order_id` map rebuilt from the account stream at start. A key
//! with one distinct id cancels by it; no id, two ids, an in-doubt place, or version-1 records give
//! `NotSent` (`no_order_id`) with nothing called. Oracles: records typed from §9.16's member list
//! and a recording tool double. A key the stream names anywhere is never placed again
//! (DEC-872). Part 3 adds the distinct-id property here, whose oracle counts ids from the
//! generated choices, and the restart against `mandate-rh-sim` in that crate's
//! `tests/robinhood.rs`. Part 4 (DEC-874, pending E7-6) pins that an odd `broker_order_id` and
//! the order in which the stream interleaves its keys' records never make a key placeable.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::pin::pin;
use std::rc::Rc;
use std::sync::mpsc;
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration;

use mandate_accounting::{InstrumentId, Side};
use mandate_canon::parse;
use mandate_executor::{
    BrokerConnector, BrokerOrder, BrokerOutcome, BrokerRequest, BrokerUnknown, ClientOrderId,
    ConnectorError, EventId, FoldedEvent, IntentId, OrderType, Purpose, Seq, SubmitOrder,
    TimeInForce,
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

/// Every call the connector makes, answered with an order in `state`: the named one for a cancel.
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
        let record = json!({"id": arguments.get("order_id").unwrap_or(&json!("rh-new")), "state": self.state, "quantity": "2",
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
    call(c, &request)
}

fn call(
    c: &mut RobinhoodConnector<Recording>,
    request: &BrokerRequest,
) -> Result<BrokerOutcome, ConnectorError> {
    let mut future = pin!(c.call(request));
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
    let other =
        json!({"client_order_id": spy.as_str(), "broker_order_id": "rh-other", "fill_id": "f1"});
    s.push("ExternalActivityIngested", other);
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

/// DEC-860 item 4 across a restart: a key the stream submitted, whether its place is in doubt
/// or answered, is never placed again; a `Submit` of it is `Unknown` with nothing called.
#[test]
fn a_key_the_journal_submitted_is_never_placed_again() {
    let (silent, lost, placed, fresh) = (key("01JA"), key("01JB"), key("01JC"), key("01JNEW"));
    let mut s = Stream::default();
    for key in [&silent, &lost, &placed] {
        s.submitted(key, "SPY", "sell");
    }
    s.changed(&lost, "unknown", Some(None), NONE);
    s.changed(&placed, "accepted", Some(Some("rh-c")), NONE);
    let (mut c, calls) = restored(&s.0, "confirmed");
    let submit = |key: &ClientOrderId| SubmitOrder {
        client_order_id: key.clone(),
        instrument: InstrumentId::new("SPY").unwrap(),
        side: Side::Sell,
        qty: Qty::parse("2").unwrap(),
        order_type: OrderType::StopLimit,
        tif: TimeInForce::Gtc,
        limit_price: Some(Price::parse("475").unwrap()),
        stop_price: Some(Price::parse("480").unwrap()),
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: Purpose::Protective,
    };
    let in_doubt = ConnectorError::Unknown(BrokerUnknown::Ambiguous);
    for key in [&silent, &lost, &placed] {
        let outcome = call(&mut c, &BrokerRequest::Submit(submit(key)));
        assert_eq!(outcome, Err(in_doubt), "{}", key.as_str());
    }
    assert!(calls.borrow().is_empty(), "{:?}", calls.borrow());
    let outcome = call(&mut c, &BrokerRequest::Submit(submit(&fresh)));
    assert!(
        matches!(outcome, Ok(BrokerOutcome::Submitted(_))),
        "{outcome:?}"
    );
    let tools: Vec<&str> = calls.borrow().iter().map(|(_, tool, _)| *tool).collect();
    assert_eq!(tools, ["review_equity_order", "place_equity_order"]);
}

/// Random streams over four keys. The oracle counts each key's distinct non-null ids from the
/// generated choices, never from the records: one id that no other key carries cancels by it;
/// none, two, or one shared with another key is `NotSent` (DEC-872 item 2).
#[test]
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
                .and_then(|set| set.first())
                .filter(|id| ids.values().filter(|set| set.contains(*id)).count() == 1);
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

/// A protective stop to `Submit` under `key`: risk-reducing, so nothing but the key refuses it.
fn protective_stop(key: &ClientOrderId) -> BrokerRequest {
    BrokerRequest::Submit(SubmitOrder {
        client_order_id: key.clone(),
        instrument: InstrumentId::new("SPY").unwrap(),
        side: Side::Sell,
        qty: Qty::parse("2").unwrap(),
        order_type: OrderType::StopLimit,
        tif: TimeInForce::Gtc,
        limit_price: Some(Price::parse("475").unwrap()),
        stop_price: Some(Price::parse("480").unwrap()),
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: Purpose::Protective,
    })
}

/// DEC-860 item 4, DEC-870 item 2 and DEC-872: each of `keys` is in doubt, so a `Submit` of it
/// is `Unknown` and its cancel `NotSent`, with nothing called and nothing guessed. A fresh key
/// is still reviewed and placed.
fn never_placed_again_nor_guessed(records: &[FoldedEvent], keys: &[&ClientOrderId]) {
    let (mut c, calls) = restored(records, "confirmed");
    let in_doubt = ConnectorError::Unknown(BrokerUnknown::Ambiguous);
    for key in keys {
        let outcome = call(&mut c, &protective_stop(key));
        assert_eq!(outcome, Err(in_doubt), "placed again: {}", key.as_str());
        assert_eq!(cancel(&mut c, key), Err(NO_ORDER_ID), "{}", key.as_str());
    }
    assert!(calls.borrow().is_empty(), "CN-7: {:?}", calls.borrow());
    let outcome = call(&mut c, &protective_stop(&key("01JFRESH")));
    assert!(
        matches!(outcome, Ok(BrokerOutcome::Submitted(_))),
        "{outcome:?}"
    );
    let tools: Vec<&str> = calls.borrow().iter().map(|(_, tool, _)| *tool).collect();
    assert_eq!(tools, ["review_equity_order", "place_equity_order"]);
}

/// DEC-872 item 1: an `OrderSubmitted` whose `side` or `instrument_id` is missing or cannot be
/// read still names its key, and so does a successor that `replaces` it, each with its own id.
#[test]
fn a_submitted_key_whose_record_cannot_be_read_is_never_placed_again() {
    let mut s = Stream::default();
    beside_a_known_order(&mut s);
    let unreadable = [
        ("01JNOSIDE", json!({"instrument_id": "SPY"})),
        (
            "01JBADSIDE",
            json!({"instrument_id": "SPY", "side": "short"}),
        ),
        ("01JNUMSIDE", json!({"instrument_id": "SPY", "side": 1})),
        ("01JNOINSTRUMENT", json!({"side": "sell"})),
        (
            "01JBADINSTRUMENT",
            json!({"instrument_id": "", "side": "sell"}),
        ),
        (
            "01JNUMINSTRUMENT",
            json!({"instrument_id": 7, "side": "sell"}),
        ),
    ];
    let mut keys = Vec::new();
    for (intent, members) in unreadable {
        let k = key(intent);
        let mut payload = json!({"client_order_id": k.as_str(), "qty": "2", "type": "limit",
            "tif": "day", "limit_price": "499", "attempt": 1});
        for (member, value) in members.as_object().into_iter().flatten() {
            payload[member] = value.clone();
        }
        s.push("OrderSubmitted", payload);
        let id = format!("rh-{intent}");
        s.changed(&k, "accepted", Some(Some(&id)), NONE);
        keys.push(k);
    }
    let heir = successor("01JREPLACEHEIR");
    let to_unreadable = json!({"replaces": keys[1].as_str()});
    s.changed(&heir, "accepted", Some(Some("rh-heir")), to_unreadable);
    keys.push(heir);
    never_placed_again_nor_guessed(&s.0, &keys.iter().collect::<Vec<_>>());
}

/// DEC-872 item 1: a key named only by an `OrderStateChanged` has no instrument or side to
/// cancel against, whatever id it carries.
#[test]
fn a_key_named_only_by_a_state_change_is_never_placed_again_nor_guessed() {
    let stray = key("01JSTRAY");
    let mut s = Stream::default();
    beside_a_known_order(&mut s);
    s.changed(&stray, "accepted", Some(Some("rh-stray")), NONE);
    never_placed_again_nor_guessed(&s.0, &[&stray]);
}

/// DEC-872 item 1: a successor whose `replaces` names a key the stream never submitted, so its
/// chain reaches no instrument or side; the missing key is named too.
#[test]
fn an_orphan_successor_is_never_placed_again_nor_guessed() {
    let (orphan, missing) = (successor("01JREPLACEORPHAN"), key("01JNEVERSUBMITTED"));
    let mut s = Stream::default();
    beside_a_known_order(&mut s);
    let to_missing = json!({"replaces": missing.as_str()});
    s.changed(&orphan, "accepted", Some(Some("rh-orphan")), to_missing);
    never_placed_again_nor_guessed(&s.0, &[&orphan, &missing]);
}

/// DEC-872 item 1: a successor named only by the old order's `replaced_by`, with no record of
/// its own.
#[test]
fn a_successor_named_only_by_replaced_by_is_never_placed_again() {
    let (old, new) = (key("01JOLDORDER"), successor("01JREPLACEUNSEEN"));
    let mut s = Stream::default();
    s.submitted(&old, "SPY", "buy");
    s.changed(&old, "accepted", Some(Some("rh-old")), NONE);
    let link = json!({"replaced_by": new.as_str(), "replaced_by_broker_order_id": "rh-new"});
    s.changed(&old, "replaced", Some(Some("rh-old")), link);
    never_placed_again_nor_guessed(&s.0, &[&new]);
}

/// DEC-872 item 2: one broker id under two keys is a corruption, and a cancel by it could cancel
/// an order the other key owns, so neither key cancels by it.
#[test]
fn two_keys_carrying_one_id_are_both_in_doubt() {
    let (first, second) = (key("01JSHAREA"), key("01JSHAREB"));
    let mut s = Stream::default();
    beside_a_known_order(&mut s);
    s.submitted(&first, "SPY", "buy");
    s.submitted(&second, "SPY", "buy");
    s.changed(&first, "accepted", Some(Some("rh-shared")), NONE);
    s.changed(&second, "accepted", Some(Some("rh-shared")), NONE);
    never_placed_again_nor_guessed(&s.0, &[&first, &second]);
}

/// DEC-872 item 3: two `OrderSubmitted` records under one key that disagree on instrument or
/// side leave it in doubt; two that agree exactly keep it placed by its one id.
#[test]
fn two_submits_of_one_key_that_disagree_are_in_doubt() {
    let (sides, symbols, same) = (key("01JTWOSIDES"), key("01JTWOSYMBOLS"), key("01JSAME"));
    let mut s = Stream::default();
    s.submitted(&sides, "SPY", "buy");
    s.submitted(&sides, "SPY", "sell");
    s.submitted(&symbols, "SPY", "buy");
    s.submitted(&symbols, "QQQ", "buy");
    s.submitted(&same, "QQQ", "sell");
    s.submitted(&same, "QQQ", "sell");
    for (key, id) in [
        (&sides, "rh-sides"),
        (&symbols, "rh-symbols"),
        (&same, "rh-same"),
    ] {
        s.changed(key, "accepted", Some(Some(id)), NONE);
    }
    never_placed_again_nor_guessed(&s.0, &[&sides, &symbols]);
    let (mut c, calls) = restored(&s.0, "confirmed");
    let expected = order(&same, "rh-same", "QQQ", Side::Sell);
    assert_eq!(cancel(&mut c, &same), Ok(BrokerOutcome::Order(expected)));
    assert_eq!(*calls.borrow(), [cancelled_by("rh-same")]);
}

/// DEC-872 item 3: a successor with its own `OrderSubmitted` keeps its own instrument and side,
/// never those of the order it `replaces`.
#[test]
fn a_successor_with_its_own_submit_keeps_its_own_instrument_and_side() {
    let (old, new) = (key("01JOLDSPY"), successor("01JREPLACEOWN"));
    let mut s = Stream::default();
    s.submitted(&old, "SPY", "buy");
    s.changed(&old, "accepted", Some(Some("rh-old")), NONE);
    s.submitted(&new, "QQQ", "sell");
    let link = json!({"replaced_by": new.as_str(), "replaced_by_broker_order_id": "rh-new"});
    s.changed(&old, "replaced", Some(Some("rh-old")), link);
    let back = json!({"replaces": old.as_str()});
    s.changed(&new, "accepted", Some(Some("rh-new")), back);
    never_placed_again_nor_guessed(&s.0, &[]);
    let (mut c, calls) = restored(&s.0, "confirmed");
    let expected = order(&new, "rh-new", "QQQ", Side::Sell);
    assert_eq!(cancel(&mut c, &new), Ok(BrokerOutcome::Order(expected)));
    assert_eq!(*calls.borrow(), [cancelled_by("rh-new")]);
}

/// DEC-874 item 1: a `broker_order_id` that is present and neither `null` nor non-empty text
/// (§9.16) leaves its key in doubt, whatever readable id its other records carry, so no cancel goes
/// out with `order_id: ""` or by an id the odd record may contradict. Each form is restored in a
/// stream of its own and checked on its own, and every form that leaves its key placeable is named
/// before the test fails. Beside each, a key with one unshared non-empty text id and its own
/// instrument and side still cancels by it (`AGENTS.md` rule 13).
#[test]
fn a_key_with_a_broker_order_id_that_is_not_non_empty_text_is_in_doubt() {
    let odd = [
        ("01JEMPTYID", json!(""), false),
        ("01JNUMBERID", json!(7), true),
        ("01JARRAYID", json!(["rh-array"]), true),
        ("01JOBJECTID", json!({"id": "rh-object"}), true),
        ("01JBOOLID", json!(true), true),
    ];
    let in_doubt = ConnectorError::Unknown(BrokerUnknown::Ambiguous);
    let known = key("01JKNOWN");
    let mut placeable = Vec::new();
    for (intent, value, with_a_readable_id) in odd {
        let k = key(intent);
        let mut s = Stream::default();
        beside_a_known_order(&mut s);
        s.submitted(&k, "SPY", "buy");
        if with_a_readable_id {
            let id = format!("rh-{intent}");
            s.changed(&k, "accepted", Some(Some(&id)), NONE);
        }
        s.changed(&k, "accepted", None, json!({ "broker_order_id": value }));
        let (mut c, calls) = restored(&s.0, "confirmed");
        let submit = call(&mut c, &protective_stop(&k));
        let cancelled = cancel(&mut c, &k);
        if submit != Err(in_doubt) || cancelled != Err(NO_ORDER_ID) || !calls.borrow().is_empty() {
            let called = calls.borrow().clone();
            placeable.push(format!(
                "{intent}: submit {submit:?}, cancel {cancelled:?}, calls {called:?}"
            ));
        }
        calls.borrow_mut().clear();
        let expected = order(&known, "rh-known", "SPY", Side::Buy);
        let outcome = cancel(&mut c, &known);
        assert_eq!(outcome, Ok(BrokerOutcome::Order(expected)), "{intent}");
        assert_eq!(*calls.borrow(), [cancelled_by("rh-known")], "{intent}");
    }
    assert!(
        placeable.is_empty(),
        "DEC-874 item 1: a key whose broker_order_id is neither null nor non-empty text must be in \
         doubt: {placeable:#?}"
    );
}

/// DEC-874 item 2: a successor's inherited instrument and side are those its origin has once the
/// whole stream is read, so an `OrderSubmitted` of the origin that disagrees, arriving after the
/// successor's record, leaves the successor in doubt too. One that agrees keeps it placed.
#[test]
fn a_late_disagreeing_submit_of_an_origin_leaves_its_successor_in_doubt() {
    let replaced = |s: &mut Stream, old: &ClientOrderId, new: &ClientOrderId, ids: [&str; 2]| {
        s.changed(old, "accepted", Some(Some(ids[0])), NONE);
        let link = json!({"replaced_by": new.as_str(), "replaced_by_broker_order_id": ids[1]});
        s.changed(old, "replaced", Some(Some(ids[0])), link);
        let back = json!({"replaces": old.as_str()});
        s.changed(new, "accepted", Some(Some(ids[1])), back);
    };
    let (old, new) = (key("01JLATEORIGIN"), successor("01JREPLACELATE"));
    let (kept, heir) = (key("01JLATEAGREES"), successor("01JREPLACEAGREES"));
    let mut s = Stream::default();
    beside_a_known_order(&mut s);
    s.submitted(&old, "SPY", "buy");
    replaced(&mut s, &old, &new, ["rh-old", "rh-new"]);
    s.submitted(&old, "SPY", "sell");
    s.submitted(&kept, "QQQ", "sell");
    replaced(&mut s, &kept, &heir, ["rh-kept", "rh-heir"]);
    s.submitted(&kept, "QQQ", "sell");
    never_placed_again_nor_guessed(&s.0, &[&new, &old]);
    let (mut c, calls) = restored(&s.0, "confirmed");
    let expected = order(&heir, "rh-heir", "QQQ", Side::Sell);
    assert_eq!(cancel(&mut c, &heir), Ok(BrokerOutcome::Order(expected)));
    assert_eq!(*calls.borrow(), [cancelled_by("rh-heir")]);
}

/// DEC-874 item 3: a key's own readable `OrderSubmitted` wins over the order it `replaces`
/// wherever it falls in the stream, and a successor of that key inherits it; an unreadable own
/// `OrderSubmitted` after the inheriting record still leaves the key in doubt, and so does a
/// `replaces` cycle with no `OrderSubmitted`, which the rebuild must still finish.
#[test]
fn a_successors_own_submit_wins_wherever_it_falls_in_the_stream() {
    let (old, new, next) = (
        key("01JREVERSEOLD"),
        successor("01JREPLACEREVERSE"),
        successor("01JREPLACENEXT"),
    );
    let (bad_old, bad_new) = (key("01JREVERSEBAD"), successor("01JREPLACEBAD"));
    let mut s = Stream::default();
    s.submitted(&old, "SPY", "buy");
    s.changed(&old, "accepted", Some(Some("rh-old")), NONE);
    s.changed(
        &new,
        "accepted",
        Some(Some("rh-new")),
        json!({"replaces": old.as_str()}),
    );
    s.changed(
        &next,
        "accepted",
        Some(Some("rh-next")),
        json!({"replaces": new.as_str()}),
    );
    s.submitted(&new, "QQQ", "sell");
    s.submitted(&bad_old, "SPY", "buy");
    s.changed(
        &bad_new,
        "accepted",
        Some(Some("rh-bad")),
        json!({"replaces": bad_old.as_str()}),
    );
    let unreadable = json!({"client_order_id": bad_new.as_str(), "instrument_id": "SPY",
        "side": "short", "qty": "2", "type": "limit", "tif": "day", "limit_price": "499",
        "attempt": 1});
    s.push("OrderSubmitted", unreadable);
    let (ring_a, ring_b) = (successor("01JREPLACERINGA"), successor("01JREPLACERINGB"));
    for (k, back, id) in [
        (&ring_a, &ring_b, "rh-ring-a"),
        (&ring_b, &ring_a, "rh-ring-b"),
    ] {
        let link = json!({"replaces": back.as_str()});
        s.changed(k, "accepted", Some(Some(id)), link);
    }
    rebuilt_within_seconds(&s.0);
    never_placed_again_nor_guessed(&s.0, &[&bad_new, &ring_a, &ring_b]);
    let (mut c, calls) = restored(&s.0, "confirmed");
    for (k, id, symbol, side) in [
        (&new, "rh-new", "QQQ", Side::Sell),
        (&next, "rh-next", "QQQ", Side::Sell),
        (&old, "rh-old", "SPY", Side::Buy),
    ] {
        let expected = order(k, id, symbol, side);
        assert_eq!(
            cancel(&mut c, k),
            Ok(BrokerOutcome::Order(expected)),
            "{id}"
        );
    }
    let expected = [
        cancelled_by("rh-new"),
        cancelled_by("rh-next"),
        cancelled_by("rh-old"),
    ];
    assert_eq!(*calls.borrow(), expected);
}

/// A tool double for a rebuild alone, which calls no tool, and which can cross to another thread.
struct NoTools;

impl Tools for NoTools {
    async fn call_tool(
        &self,
        _class: CallClass,
        tool: &'static str,
        _arguments: &Value,
    ) -> Result<String, McpError> {
        panic!("a rebuild calls no tool, but called {tool}")
    }
}

/// DEC-874 item 2: `restore` of `records` finishes within seconds, run on a thread of its own so a
/// rebuild that loops on a `replaces` cycle fails this assertion rather than hanging the test.
fn rebuilt_within_seconds(records: &[FoldedEvent]) {
    let records = records.to_vec();
    let (done, finished) = mpsc::channel();
    thread::spawn(move || {
        let rebuilt = RobinhoodConnector::restore(NoTools, ACCOUNT.to_owned(), &records).is_ok();
        done.send(rebuilt).unwrap();
    });
    assert_eq!(
        finished.recv_timeout(Duration::from_secs(5)),
        Ok(true),
        "DEC-874 item 2: the rebuild must finish on a `replaces` cycle and hold its keys in doubt"
    );
}

/// What one generated record of a key in the shuffle property is, from its choice: an
/// `OrderSubmitted` with one of four readable instrument-and-side pairs or an unreadable `side`,
/// or an `OrderStateChanged` with one of the `broker_order_id` forms.
const SUBMITS: [Option<(&str, &str)>; 5] = [
    Some(("SPY", "buy")),
    Some(("SPY", "sell")),
    Some(("QQQ", "buy")),
    Some(("QQQ", "sell")),
    None,
];

/// The `broker_order_id` of a generated `OrderStateChanged` of key `n`, by choice: absent
/// (version 1), `null`, the key's usual id (four choices in ten), a second id of its own, an id
/// every key may carry, the empty string, or a number.
fn generated_id(n: usize, choice: usize) -> Option<Value> {
    match choice {
        0 => None,
        1 => Some(Value::Null),
        2..=5 => Some(json!(format!("rh-{n}-a"))),
        6 => Some(json!(format!("rh-{n}-b"))),
        7 => Some(json!("rh-shared")),
        8 => Some(json!("")),
        _ => Some(json!(7)),
    }
}

/// DEC-872 and DEC-874 as a property: over five keys, three originals and two successors (the
/// second may replace the first), each key's records come in a generated order of their own and
/// the keys' records are interleaved at random. Which keys are placed, by which id and with which
/// instrument and side, never depends on the interleaving. The oracle reads only the generated
/// choices, never the records or their order: a key's instrument and side are its own agreeing
/// readable submits, or with none a successor's origin's; it is placed with one unshared non-empty
/// text id and no odd one.
#[test]
fn which_keys_are_placed_never_depends_on_how_the_stream_interleaves_its_keys() {
    let keys = [
        key("01JMIX0"),
        key("01JMIX1"),
        key("01JMIX2"),
        successor("01JREPLACEMIX3"),
        successor("01JREPLACEMIX4"),
    ];
    let own = proptest::collection::vec(0..15_usize, 0..5);
    let strategy = (
        proptest::collection::vec(own, keys.len()),
        (0..3_usize, 0..4_usize),
        proptest::collection::vec(0..keys.len(), 0..30),
    );
    let mut runner = TestRunner::new(Config::with_cases(256));
    let verdict = runner.run(&strategy, |(records, (p3, p4), picks)| {
        let parent = |n: usize| match n {
            3 => Some(p3),
            4 => Some(p4),
            _ => None,
        };
        let mut queues: Vec<Vec<FoldedEvent>> = Vec::new();
        for (n, choices) in records.iter().enumerate() {
            let mut own = Stream::default();
            for &choice in choices {
                if let Some(pair) = SUBMITS.get(choice) {
                    let (symbol, side) = pair.unwrap_or(("SPY", "short"));
                    own.submitted(&keys[n], symbol, side);
                    continue;
                }
                let mut links = json!({});
                if let Some(p) = parent(n) {
                    links["replaces"] = json!(keys[p].as_str());
                }
                if let Some(id) = generated_id(n, choice - SUBMITS.len()) {
                    links["broker_order_id"] = id;
                }
                own.changed(&keys[n], "accepted", None, links);
            }
            own.0.reverse();
            queues.push(own.0);
        }
        let mut stream = Vec::new();
        for pick in picks
            .into_iter()
            .chain((0..keys.len()).rev().cycle().take(100))
        {
            if let Some(record) = queues[pick].pop() {
                stream.push(record);
            }
        }
        proptest::prop_assert!(queues.iter().all(Vec::is_empty));

        let submits = |n: usize| records[n].iter().filter_map(|&c| SUBMITS.get(c).copied());
        let changes = |n: usize| {
            records[n]
                .iter()
                .filter(|&&c| c >= SUBMITS.len())
                .map(move |&c| c - SUBMITS.len())
        };
        let own_pair = |n: usize| -> Option<Option<(&str, &str)>> {
            let mut all = submits(n);
            let first = all.next()?;
            Some(first.filter(|_| all.all(|other| other == first)))
        };
        let mut pair: Vec<Option<(&str, &str)>> = Vec::new();
        for n in 0..keys.len() {
            let inherited = match parent(n) {
                Some(p) if changes(n).next().is_some() => pair[p],
                _ => None,
            };
            pair.push(own_pair(n).unwrap_or(inherited));
        }
        let ids = |n: usize| -> BTreeSet<String> {
            changes(n)
                .filter_map(|c| match c {
                    2..=5 => Some(format!("rh-{n}-a")),
                    6 => Some(format!("rh-{n}-b")),
                    7 => Some("rh-shared".to_owned()),
                    _ => None,
                })
                .collect()
        };
        let odd = |n: usize| changes(n).any(|c| c >= 8);

        let (mut c, calls) = restored(&stream, "confirmed");
        let mut expected = Vec::new();
        for (n, key) in keys.iter().enumerate() {
            let mine = ids(n);
            let one = mine
                .first()
                .filter(|_| mine.len() == 1 && !odd(n))
                .filter(|id| (0..keys.len()).all(|m| m == n || !ids(m).contains(*id)));
            let outcome = cancel(&mut c, key);
            match (one, pair[n]) {
                (Some(id), Some((symbol, side))) => {
                    let side = if side == "buy" { Side::Buy } else { Side::Sell };
                    let placed = order(key, id, symbol, side);
                    proptest::prop_assert_eq!(outcome, Ok(BrokerOutcome::Order(placed)));
                    expected.push(cancelled_by(id));
                }
                _ => proptest::prop_assert_eq!(outcome, Err(NO_ORDER_ID), "{}", key.as_str()),
            }
        }
        proptest::prop_assert_eq!(&*calls.borrow(), &expected);
        let in_doubt = ConnectorError::Unknown(BrokerUnknown::Ambiguous);
        for (n, key) in keys.iter().enumerate() {
            let named = !records[n].is_empty()
                || (3..keys.len()).any(|m| parent(m) == Some(n) && changes(m).next().is_some());
            if named {
                let outcome = call(&mut c, &protective_stop(key));
                proptest::prop_assert_eq!(outcome, Err(in_doubt), "{}", key.as_str());
            }
        }
        proptest::prop_assert_eq!(&*calls.borrow(), &expected);
        Ok(())
    });
    verdict.unwrap();
}
