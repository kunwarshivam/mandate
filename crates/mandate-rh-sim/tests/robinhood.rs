//! The Robinhood connector against the simulated server over loopback (E7-6, C1 tests part 2):
//! `Submit` is review then place with the derived `ref_id` on the recorded account; a re-send
//! after a restart is deduplicated; a lost or garbled place answer is `Unknown` and never placed
//! again (LT-6); the place answer's states read as connections spec §6.2 says; and no call leaves
//! the allowlist (LT-8). Part 3 adds alerts (LT-5), protection, `Cancel` and the `NotSent` cases. These tests live here, not in
//! `mandate-robinhood`, so no product crate depends on the simulator (first live trade brief).
//! Oracles: the simulator's own records and call log, `sha2`, and the spec's tables typed here.

mod common;

use std::cell::{Cell, RefCell};
use std::pin::pin;
use std::rc::Rc;
use std::task::{Context, Poll, Waker};

use common::wire::Wire;
use common::{AGENTIC, price, qty, sim};
use mandate_accounting::{InstrumentId, Side as Way};
use mandate_executor::{
    BrokerConnector, BrokerOutcome, BrokerRequest, ClientOrderId, ConnectorError, EventId,
    IntentId, OrderType as Kind, Purpose, SubmitOrder, TimeInForce as Tif,
};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_rh_sim::{Event, Fault, Garble, MarketHours, OrderType, Session, Side, SimServer};
use mandate_rh_sim::{State, TimeInForce, Variant};
use mandate_robinhood::{RobinhoodConnector, Tools};
use serde_json::Value;
use sha2::{Digest, Sha256};

const REVIEW: &str = "review_equity_order";
const PLACE: &str = "place_equity_order";

/// What a test sets and reads beside the connector: a garble for the next place answer, never
/// the review before it, and each call's budget class.
#[derive(Default)]
struct Probe {
    garble_place: Cell<Option<Garble>>,
    classes: RefCell<Vec<(CallClass, &'static str)>>,
}

/// The seam over the wire client.
struct Loopback<'s> {
    server: &'s SimServer,
    wire: RefCell<Wire>,
    probe: Rc<Probe>,
}

impl Tools for Loopback<'_> {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        args: &Value,
    ) -> Result<String, McpError> {
        self.probe.classes.borrow_mut().push((class, tool));
        if tool == PLACE
            && let Some(garble) = self.probe.garble_place.take()
        {
            self.server.garble_next(garble).unwrap();
        }
        let reply = self.wire.borrow_mut().call_raw(tool, args.clone());
        let message: Value = serde_json::from_str(&reply.ok_or(McpError::Network)?.body)
            .map_err(|_| McpError::Malformed)?;
        message
            .get("result")
            .map(Value::to_string)
            .ok_or(McpError::Malformed)
    }
}

type Connector<'s> = RobinhoodConnector<Loopback<'s>>;

fn probed(server: &SimServer) -> (Connector<'_>, Rc<Probe>) {
    let wire = RefCell::new(Wire::connect(&server.url().unwrap()));
    let probe = Rc::new(Probe::default());
    let tools = Loopback {
        server,
        wire,
        probe: Rc::clone(&probe),
    };
    (RobinhoodConnector::new(tools, AGENTIC.to_owned()), probe)
}

fn connector(server: &SimServer) -> Connector<'_> {
    probed(server).0
}

/// Every future the connector makes completes once the seam answers, which the wire does at once.
fn run(c: &mut Connector<'_>, r: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
    let mut future = pin!(c.call(r));
    match future
        .as_mut()
        .poll(&mut Context::from_waker(Waker::noop()))
    {
        Poll::Ready(outcome) => outcome,
        Poll::Pending => panic!("the connector awaited something other than the seam"),
    }
}

fn server() -> SimServer {
    let mut sim = sim().unwrap();
    sim.apply(Event::EchoRefId(true)).unwrap();
    SimServer::start(sim, Variant::Honest).unwrap()
}

fn key(intent: &str) -> ClientOrderId {
    ClientOrderId::for_intent(&IntentId(EventId(intent.to_owned()))).unwrap()
}

/// A `gfd` limit buy of 2 SPY at 499 for an opening.
fn buy(intent: &str) -> SubmitOrder {
    SubmitOrder {
        client_order_id: key(intent),
        instrument: InstrumentId::new("SPY").unwrap(),
        side: Way::Buy,
        qty: qty("2"),
        order_type: Kind::Limit,
        tif: Tif::Day,
        limit_price: Some(price("499")),
        stop_price: None,
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: Purpose::Open,
    }
}

fn submit(order: SubmitOrder) -> BrokerRequest {
    BrokerRequest::Submit(order)
}

/// RFC 9562 §5.8 over SHA-256: the first 16 bytes, version 8, variant `10`, lower case.
fn uuid_v8(text: &str) -> String {
    let mut b = Sha256::digest(text.as_bytes())[..16].to_vec();
    (b[6], b[8]) = ((b[6] & 0x0f) | 0x80, (b[8] & 0x3f) | 0x80);
    let hex: String = b.iter().map(|b| format!("{b:02x}")).collect();
    [
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..],
    ]
    .join("-")
}

/// The server's call log, which must be non-empty and inside the allowlist (LT-8).
fn calls(server: &SimServer) -> Vec<String> {
    let calls = server.calls().unwrap();
    assert!(
        calls.iter().all(|c| ALLOWLIST.contains(&c.as_str())),
        "{calls:?}"
    );
    calls
}

fn classes(probe: &Probe) -> Vec<(CallClass, &'static str)> {
    probe.classes.borrow().clone()
}

#[test]
#[ignore = "pending E7-6"]
fn a_submit_reviews_then_places_once_on_the_account_with_its_ref_id() {
    let server = server();
    let (mut c, probe) = probed(&server);
    let outcome = run(&mut c, &submit(buy("01JOPEN"))).unwrap();
    assert_eq!(calls(&server), [REVIEW, PLACE]);
    assert_eq!(
        classes(&probe),
        [(CallClass::Ordinary, REVIEW), (CallClass::Ordinary, PLACE)]
    );
    let orders = server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap();
    assert_eq!(orders.len(), 1);
    let o = &orders[0];
    assert_eq!(
        o.ref_id.as_deref(),
        Some(uuid_v8(key("01JOPEN").as_str()).as_str())
    );
    assert_eq!(
        (o.account_number.as_str(), o.symbol.as_str(), o.side),
        (AGENTIC, "SPY", Side::Buy)
    );
    assert_eq!(
        (o.order_type, o.quantity, o.limit_price),
        (OrderType::Limit, qty("2"), Some(price("499")))
    );
    assert_eq!(
        (o.time_in_force, o.market_hours),
        (TimeInForce::Gfd, MarketHours::Regular)
    );
    let BrokerOutcome::Submitted(order) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        (order.broker_order_id.as_str(), order.status.as_str()),
        (o.id.as_str(), "accepted")
    );
    assert_eq!(
        order.client_order_id.as_deref(),
        Some(key("01JOPEN").as_str())
    );
    assert_eq!(
        (order.qty, order.filled_qty, order.limit_price),
        (qty("2"), qty("0"), Some(price("499")))
    );
}

#[test]
#[ignore = "pending E7-6"]
fn a_resend_after_a_restart_carries_the_same_ref_id_and_the_broker_keeps_one_order() {
    let server = server();
    let first = run(&mut connector(&server), &submit(buy("01JOPEN"))).unwrap();
    let again = run(&mut connector(&server), &submit(buy("01JOPEN"))).unwrap();
    let (BrokerOutcome::Submitted(first), BrokerOutcome::Submitted(again)) = (&first, &again)
    else {
        panic!("{first:?} {again:?}")
    };
    assert_eq!(first.broker_order_id, again.broker_order_id);
    assert_eq!(
        server
            .drive(|sim| sim.orders(AGENTIC))
            .unwrap()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(calls(&server), [REVIEW, PLACE, REVIEW, PLACE]);
}

#[test]
#[ignore = "pending E7-6"]
fn a_lost_place_answer_is_unknown_and_never_placed_again() {
    let server = server();
    server
        .drive(|sim| sim.apply(Event::Script(Fault::LoseAnswer)))
        .unwrap()
        .unwrap();
    let mut c = connector(&server);
    for attempt in 0..2 {
        let outcome = run(&mut c, &submit(buy("01JLOST")));
        assert!(
            matches!(outcome, Err(ConnectorError::Unknown(_))),
            "{attempt}: {outcome:?}"
        );
    }
    assert_eq!(
        calls(&server),
        [REVIEW, PLACE],
        "the order in doubt is never sent again"
    );
    assert_eq!(
        server
            .drive(|sim| sim.orders(AGENTIC))
            .unwrap()
            .unwrap()
            .len(),
        1
    );
}

#[test]
#[ignore = "pending E7-6"]
fn a_garbled_place_answer_is_unknown_never_rejected_or_filled() {
    for garble in [
        Garble::UnknownState,
        Garble::MissingId,
        Garble::NumberQuantity,
        Garble::NotJson,
    ] {
        let server = server();
        let (mut c, probe) = probed(&server);
        probe.garble_place.set(Some(garble));
        let outcome = run(&mut c, &submit(buy("01JBENT")));
        assert!(
            matches!(outcome, Err(ConnectorError::Unknown(_))),
            "{garble:?}: {outcome:?}"
        );
        assert_eq!(calls(&server), [REVIEW, PLACE], "{garble:?}");
        assert_eq!(
            server
                .drive(|sim| sim.orders(AGENTIC))
                .unwrap()
                .unwrap()
                .len(),
            1
        );
    }
}

/// Connections spec §6.2 through the place answer: each state a fresh order may start in.
#[test]
#[ignore = "pending E7-6"]
fn each_state_a_place_answers_reads_as_the_spec_says() {
    let scripted = [
        (State::New, "accepted"),
        (State::Unconfirmed, "accepted"),
        (State::Rejected, "rejected"),
        (State::Failed, "rejected"),
    ];
    for (state, status) in scripted {
        let server = server();
        server
            .drive(|sim| sim.apply(Event::Script(Fault::Answer(state))))
            .unwrap()
            .unwrap();
        let outcome = run(&mut connector(&server), &submit(buy("01JSTATE"))).unwrap();
        let BrokerOutcome::Submitted(order) = outcome else {
            panic!("{state:?}: {outcome:?}")
        };
        assert_eq!(order.status, status, "{state:?}");
    }
    let server = server();
    server
        .drive(|sim| sim.apply(Event::Session(Session::Closed)))
        .unwrap()
        .unwrap();
    let outcome = run(&mut connector(&server), &submit(buy("01JQUEUE"))).unwrap();
    let BrokerOutcome::Submitted(order) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(order.status, "accepted", "queued");
    assert_eq!(
        server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap()[0].state,
        State::Queued
    );
}
