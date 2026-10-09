//! The Robinhood connector against the simulated server over loopback (E7-6, C1 tests part 2):
//! `Submit` is review then place with the derived `ref_id` on the recorded account; a re-send
//! after a restart is deduplicated; a lost or garbled place answer is `Unknown` and never placed
//! again (LT-6); the place answer's states read as connections spec §6.2 says; and no call leaves
//! the allowlist (LT-8). Part 3: an alert refuses an opening and never a protective order (LT-5),
//! `Cancel` goes by the broker's `order_id`, and what the profile does not offer is not sent. These tests live here, not in
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
    BracketLegs, BrokerConnector, BrokerOutcome, BrokerRequest, ClientOrderId, ConnectorError,
    EventId, IntentId, OcoLegs, OrderType as Kind, Purpose, SubmitOrder, TimeInForce as Tif,
};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_rh_sim::{Event, Fault, Garble, MarketHours, OrderType, Session, Side, SimServer};
use mandate_rh_sim::{State, TimeInForce, Variant};
use mandate_robinhood::{RobinhoodConnector, Tools};
use serde_json::Value;
use sha2::{Digest, Sha256};

const REVIEW: &str = "review_equity_order";
const PLACE: &str = "place_equity_order";
const CANCEL: &str = "cancel_equity_order";

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

/// A `gtc` protective stop-limit sell of 1 SPY, stop 480, limit 475.
fn stop(intent: &str) -> SubmitOrder {
    let (stop_price, limit_price) = (Some(price("480")), Some(price("475")));
    let (order_type, tif, purpose) = (Kind::StopLimit, Tif::Gtc, Purpose::Protective);
    SubmitOrder {
        side: Way::Sell,
        qty: qty("1"),
        stop_price,
        limit_price,
        order_type,
        tif,
        purpose,
        ..buy(intent)
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

#[test]
fn an_alert_refuses_an_opening_before_the_place() {
    let server = server();
    server
        .drive(|sim| sim.apply(Event::Halt("SPY".to_owned())))
        .unwrap()
        .unwrap();
    let outcome = run(&mut connector(&server), &submit(buy("01JHALT"))).unwrap();
    let BrokerOutcome::Rejected(reject) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        reject.client_order_id.as_deref(),
        Some(key("01JHALT").as_str())
    );
    assert_eq!(calls(&server), [REVIEW]);
}

/// Rule 13 and DEC-860 item 6: a protective stop-limit draws on the reserved budget and is placed
/// even under an alert; the simulator then refuses it, which a place reads as `Unknown`.
#[test]
fn a_protective_stop_limit_is_risk_reducing_and_no_alert_holds_it() {
    let server = server();
    let (mut c, probe) = probed(&server);
    let BrokerOutcome::Submitted(entry) = run(&mut c, &submit(buy("01JENTRY"))).unwrap() else {
        panic!()
    };
    server
        .drive(|sim| sim.fill(&entry.broker_order_id, qty("2"), price("499")))
        .unwrap()
        .unwrap();
    let placed = run(&mut c, &submit(stop("01JSTOP1"))).unwrap();
    assert!(matches!(placed, BrokerOutcome::Submitted(_)), "{placed:?}");
    let resting = &server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap()[0];
    assert_eq!(
        (resting.side, resting.order_type, resting.time_in_force),
        (Side::Sell, OrderType::StopLimit, TimeInForce::Gtc)
    );
    assert_eq!(
        (resting.stop_price, resting.limit_price),
        (Some(price("480")), Some(price("475")))
    );
    server
        .drive(|sim| sim.apply(Event::Halt("SPY".to_owned())))
        .unwrap()
        .unwrap();
    let held = run(&mut c, &submit(stop("01JSTOP2")));
    assert!(matches!(held, Err(ConnectorError::Unknown(_))), "{held:?}");
    assert_eq!(
        calls(&server),
        [REVIEW, PLACE, REVIEW, PLACE, REVIEW, PLACE]
    );
    let stops = &classes(&probe)[2..];
    assert!(
        stops
            .iter()
            .all(|(class, _)| *class == CallClass::RiskReducing),
        "{stops:?}"
    );
}

#[test]
fn a_cancel_goes_by_the_order_id_and_is_refused_once_terminal() {
    let server = server();
    let (mut c, probe) = probed(&server);
    let cancel = |intent: &str| BrokerRequest::Cancel {
        client_order_id: key(intent),
    };
    let unknown = run(&mut c, &cancel("01JNEVER"));
    let refused = ConnectorError::NotSent {
        code: "no_order_id",
    };
    assert_eq!(
        unknown,
        Err(refused),
        "DEC-860 item 7: escalated, never dropped"
    );
    assert!(
        server.calls().unwrap().is_empty(),
        "never cancelled by guess"
    );
    run(&mut c, &submit(buy("01JKEEP"))).unwrap();
    let BrokerOutcome::Submitted(gone) = run(&mut c, &submit(buy("01JGONE"))).unwrap() else {
        panic!()
    };
    let outcome = run(&mut c, &cancel("01JKEEP")).unwrap();
    let id = key("01JKEEP").as_str().to_owned();
    assert_eq!(
        outcome,
        BrokerOutcome::CancelAccepted {
            client_order_id: id
        }
    );
    assert_eq!(
        classes(&probe).last(),
        Some(&(CallClass::RiskReducing, CANCEL))
    );
    server
        .drive(|sim| sim.fill(&gone.broker_order_id, qty("2"), price("499")))
        .unwrap()
        .unwrap();
    let BrokerOutcome::Rejected(reject) = run(&mut c, &cancel("01JGONE")).unwrap() else {
        panic!()
    };
    assert_eq!(
        reject.client_order_id.as_deref(),
        Some(key("01JGONE").as_str())
    );
    let states: Vec<State> = server
        .drive(|sim| sim.orders(AGENTIC))
        .unwrap()
        .unwrap()
        .iter()
        .map(|o| o.state)
        .collect();
    assert_eq!(states, [State::Filled, State::Cancelled]);
    assert_eq!(
        calls(&server),
        [REVIEW, PLACE, REVIEW, PLACE, CANCEL, CANCEL]
    );
}

#[test]
fn what_the_profile_does_not_offer_is_not_sent() {
    let server = server();
    let mut c = connector(&server);
    let legs = OcoLegs {
        take_profit: price("520"),
        stop: price("480"),
        qty: qty("2"),
    };
    let bracket = Some(BracketLegs {
        take_profit: price("520"),
        stop: price("480"),
    });
    let refused = [
        submit(SubmitOrder {
            bracket,
            ..buy("01JBRKT")
        }),
        submit(SubmitOrder {
            oco: Some(legs),
            ..buy("01JOCO")
        }),
        submit(SubmitOrder {
            extended_hours: true,
            ..buy("01JEXT")
        }),
        submit(SubmitOrder {
            tif: Tif::Ioc,
            ..buy("01JIOC")
        }),
    ];
    for request in &refused {
        let outcome = run(&mut c, request);
        assert!(
            matches!(outcome, Err(ConnectorError::NotSent { .. })),
            "{request:?}: {outcome:?}"
        );
    }
    assert!(server.calls().unwrap().is_empty());
}
