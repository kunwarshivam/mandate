//! The Robinhood connector against the simulated server over loopback (E7-6, C1 tests part 2):
//! `Submit` is review then place with the derived `ref_id` on the recorded account; a re-send
//! after a restart is deduplicated; a lost or garbled place answer is `Unknown` and never placed
//! again (LT-6); the place answer's states read as connections spec §6.2 says; and no call leaves
//! the allowlist (LT-8). Part 3: an alert refuses an opening and never a protective order (LT-5),
//! `Cancel` goes by the broker's `order_id`, and what the profile does not offer is not sent. The
//! tests correction for #1097 pins the review, cancel and refusal paths no test above reaches,
//! with a rewrite of one tool's answer after the server has answered. These tests live here, not in
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
    ActivityCursor, BracketLegs, BrokerConnector, BrokerOutcome, BrokerRequest, ClientOrderId,
    ConnectorError, EventId, IntentId, OcoLegs, OrderListing, OrderOrigin, OrderType as Kind,
    Purpose, RiskClock, SubmitOrder, TimeInForce as Tif,
};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_rh_sim::{Event, Fault, Garble, MarketHours, OrderType, Session, Side, SimServer};
use mandate_rh_sim::{State, TimeInForce, Variant};
use mandate_robinhood::{RobinhoodConnector, Tools};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const REVIEW: &str = "review_equity_order";
const PLACE: &str = "place_equity_order";
const CANCEL: &str = "cancel_equity_order";

/// A rewrite of a tool's answer, given the JSON-RPC `result` the server sent.
type Bend = fn(Value) -> Result<String, McpError>;

/// What a test sets and reads beside the connector: a garble for the next place answer, never
/// the review before it, a bend of the next answer to one tool, and each call's budget class.
#[derive(Default)]
struct Probe {
    garble_place: Cell<Option<Garble>>,
    bend: Cell<Option<(&'static str, Bend)>>,
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
        let result = message.get("result").cloned().ok_or(McpError::Malformed)?;
        match self.probe.bend.take() {
            Some((bent, bend)) if bent == tool => bend(result),
            other => {
                self.probe.bend.set(other);
                Ok(result.to_string())
            }
        }
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

#[test]
#[ignore = "pending E7-6"]
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
#[ignore = "pending E7-6"]
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
#[ignore = "pending E7-6"]
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
#[ignore = "pending E7-6"]
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

/// The answer lost after the server acted on the call.
fn lose(_: Value) -> Result<String, McpError> {
    Err(McpError::Network)
}

/// A tool result with `isError`, as the server sends a refusal.
fn refuse(_: Value) -> Result<String, McpError> {
    let block = json!({"type": "text", "text": "refused"});
    Ok(json!({"content": [block], "isError": true}).to_string())
}

/// The review's own answer with its `alerts` array taken out.
fn without_alerts(mut result: Value) -> Result<String, McpError> {
    let content = result["structuredContent"].as_object_mut().unwrap();
    assert!(content.remove("alerts").is_some(), "the review had alerts");
    Ok(result.to_string())
}

/// Text that is not JSON.
fn not_json(_: Value) -> Result<String, McpError> {
    Ok("{\"isError\": fal".to_owned())
}

/// The server's own answer with `isError` the string `"false"`.
fn is_error_text(mut result: Value) -> Result<String, McpError> {
    result["isError"] = json!("false");
    Ok(result.to_string())
}

/// The server's own answer with `isError` the number 0.
fn is_error_zero(mut result: Value) -> Result<String, McpError> {
    result["isError"] = json!(0);
    Ok(result.to_string())
}

/// A long position of 2 SPY, bought and filled at 499 through the connector.
fn long(server: &SimServer, c: &mut Connector<'_>) {
    let outcome = run(c, &submit(buy("01JLONG"))).unwrap();
    let BrokerOutcome::Submitted(entry) = outcome else {
        panic!("{outcome:?}")
    };
    server
        .drive(|sim| sim.fill(&entry.broker_order_id, qty("2"), price("499")))
        .unwrap()
        .unwrap();
}

fn orders(server: &SimServer) -> Vec<mandate_rh_sim::Order> {
    server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap()
}

/// Trading spec §5.2, Robinhood: "market: whole, fractional, notional; every other type: whole
/// shares". DEC-860 item 7: an order the profile does not offer is `NotSent`, before any call.
#[test]
#[ignore = "pending E7-6"]
fn a_fractional_limit_or_stop_limit_is_not_sent_and_nothing_is_called() {
    let server = server();
    let (mut c, probe) = probed(&server);
    let fractional = [
        SubmitOrder {
            qty: qty("1.5"),
            ..buy("01JFRACL")
        },
        SubmitOrder {
            qty: qty("0.5"),
            ..stop("01JFRACS")
        },
    ];
    for order in fractional {
        let outcome = run(&mut c, &submit(order.clone()));
        assert!(
            matches!(outcome, Err(ConnectorError::NotSent { .. })),
            "{order:?}: {outcome:?}"
        );
    }
    assert!(server.calls().unwrap().is_empty());
    assert!(classes(&probe).is_empty());
}

/// Connections spec §6.2: "A pre-trade alert refuses an opening or an increase before the place."
/// The spec names no outcome for a review refused with `isError`, lost, or answered without an
/// `alerts` array, so what is pinned is the outcome that adds no risk (`AGENTS.md` rule 3):
/// nothing is placed, and the connector does not report an order. Which refusal it reports is
/// left open.
#[test]
#[ignore = "pending E7-6"]
fn an_openings_review_refused_lost_or_without_alerts_places_nothing() {
    let bends: [(&str, Bend); 3] = [
        ("isError", refuse),
        ("lost", lose),
        ("no alerts", without_alerts),
    ];
    for purpose in [Purpose::Open, Purpose::Increase] {
        for (name, bend) in bends {
            let server = server();
            let (mut c, probe) = probed(&server);
            probe.bend.set(Some((REVIEW, bend)));
            let order = SubmitOrder {
                purpose,
                ..buy("01JREVIEW")
            };
            let outcome = run(&mut c, &submit(order));
            assert!(
                !matches!(outcome, Ok(BrokerOutcome::Submitted(_))),
                "{purpose:?} {name}: {outcome:?}"
            );
            assert_eq!(calls(&server), [REVIEW], "{purpose:?} {name}");
            assert!(orders(&server).is_empty(), "{purpose:?} {name}");
        }
    }
}

/// `AGENTS.md` rule 13 and connections spec §6.2: "For a sell or a protective order the alert is
/// journaled and the order is placed anyway, so the broker accepts or rejects it." A review that
/// is lost or refused holds a risk-reducing order no more than an alert does.
#[test]
#[ignore = "pending E7-6"]
fn a_risk_reducing_order_is_placed_when_its_review_is_lost_or_refused() {
    let sell = |purpose: Purpose| SubmitOrder {
        side: Way::Sell,
        qty: qty("1"),
        purpose,
        ..buy("01JSELL")
    };
    let reducing = [
        (sell(Purpose::RiskExit), OrderType::Limit),
        (sell(Purpose::OwnerExit), OrderType::Limit),
        (sell(Purpose::DiscretionaryExit), OrderType::Limit),
        (sell(Purpose::Flatten), OrderType::Limit),
        (stop("01JSTOP"), OrderType::StopLimit),
    ];
    let bends: [(&str, Bend); 2] = [("lost", lose), ("isError", refuse)];
    for (order, kind) in reducing {
        for (name, bend) in bends {
            let server = server();
            let (mut c, probe) = probed(&server);
            long(&server, &mut c);
            probe.bend.set(Some((REVIEW, bend)));
            let outcome = run(&mut c, &submit(order.clone()));
            let case = format!("{:?} {name}", order.purpose);
            assert!(
                matches!(outcome, Ok(BrokerOutcome::Submitted(_))),
                "{case}: {outcome:?}"
            );
            assert_eq!(calls(&server), [REVIEW, PLACE, REVIEW, PLACE], "{case}");
            let sells: Vec<_> = orders(&server)
                .into_iter()
                .filter(|o| o.side == Side::Sell)
                .map(|o| (o.order_type, o.quantity))
                .collect();
            assert_eq!(sells, [(kind, qty("1"))], "{case}");
        }
    }
}

/// Trading spec §5.7: a cancel's outcome is known only from the broker ("PendingCancel -->
/// Canceled: confirmed"). A lost cancel answer is `Unknown` (`ConnectorError::Unknown`: "a
/// timeout, a dropped connection"); an answer that is not JSON is `Unreadable` ("the broker
/// answered and the connector could not read the answer"). Neither is `CancelAccepted`.
#[test]
#[ignore = "pending E7-6"]
fn a_lost_or_unreadable_cancel_answer_never_reads_as_cancelled() {
    let cancel = BrokerRequest::Cancel {
        client_order_id: key("01JOPEN"),
    };
    let server = server();
    let (mut c, probe) = probed(&server);
    run(&mut c, &submit(buy("01JOPEN"))).unwrap();
    probe.bend.set(Some((CANCEL, lose)));
    let lost = run(&mut c, &cancel);
    assert!(matches!(lost, Err(ConnectorError::Unknown(_))), "{lost:?}");
    assert_eq!(calls(&server), [REVIEW, PLACE, CANCEL]);
    let server = crate::server();
    let (mut c, probe) = probed(&server);
    run(&mut c, &submit(buy("01JOPEN"))).unwrap();
    probe.bend.set(Some((CANCEL, not_json)));
    let garbled = run(&mut c, &cancel);
    assert!(
        matches!(garbled, Err(ConnectorError::Unreadable { .. })),
        "{garbled:?}"
    );
    assert_eq!(calls(&server), [REVIEW, PLACE, CANCEL]);
}

/// The MCP tool result's `isError` is a boolean. One that is not is not an answer: to a place it
/// is `Unknown` (DEC-860 item 4: "Anything else to a place ... is `ConnectorError::Unknown`"),
/// and to a cancel `Unreadable`, never `CancelAccepted`.
#[test]
#[ignore = "pending E7-6"]
fn an_is_error_that_is_not_a_boolean_is_never_success() {
    let bends: [(&str, Bend); 2] = [("text", is_error_text), ("zero", is_error_zero)];
    for (name, bend) in bends {
        let server = server();
        let (mut c, probe) = probed(&server);
        probe.bend.set(Some((PLACE, bend)));
        let placed = run(&mut c, &submit(buy("01JPLACE")));
        assert!(
            matches!(placed, Err(ConnectorError::Unknown(_))),
            "{name}: {placed:?}"
        );
        assert_eq!(calls(&server), [REVIEW, PLACE], "{name}");
        let server = crate::server();
        let (mut c, probe) = probed(&server);
        run(&mut c, &submit(buy("01JOPEN"))).unwrap();
        probe.bend.set(Some((CANCEL, bend)));
        let cancel = BrokerRequest::Cancel {
            client_order_id: key("01JOPEN"),
        };
        let cancelled = run(&mut c, &cancel);
        assert!(
            matches!(cancelled, Err(ConnectorError::Unreadable { .. })),
            "{name}: {cancelled:?}"
        );
        assert_eq!(calls(&server), [REVIEW, PLACE, CANCEL], "{name}");
    }
}

/// Connections spec §6.2: `GetOrderByClientId` "Not offered (U-R2)", `ListActivities` "No feed in
/// the contract (U-R8)", `AcknowledgeReplace` "Not applicable"; the list and the reads are C2's
/// and C3's, so C1 does not implement them (`ConnectorError::NotSent`: "the connector does not
/// implement it yet"). `CancelAll` and `ClosePosition` take an `AccountWideScope` no test can
/// build. Each is `NotSent` with nothing called, even with an order on record.
#[test]
#[ignore = "pending E7-6"]
fn every_other_request_is_not_sent_and_nothing_is_called() {
    let server = server();
    let (mut c, probe) = probed(&server);
    run(&mut c, &submit(buy("01JOPEN"))).unwrap();
    let listing = OrderListing {
        client_order_id: key("01JOPEN"),
        instrument: InstrumentId::new("SPY").unwrap(),
        origin: OrderOrigin::Agentic,
        created_since: RiskClock::from_secs(0),
    };
    let others = [
        BrokerRequest::AcknowledgeReplace {
            replaced: key("01JOPEN"),
        },
        BrokerRequest::GetOrderByClientId(key("01JOPEN")),
        BrokerRequest::ListOrders(listing),
        BrokerRequest::ListOpenOrders,
        BrokerRequest::ListPositions,
        BrokerRequest::GetAccount,
        BrokerRequest::ListActivities {
            since: ActivityCursor("0".to_owned()),
        },
    ];
    for request in &others {
        let outcome = run(&mut c, request);
        assert!(
            matches!(outcome, Err(ConnectorError::NotSent { .. })),
            "{request:?}: {outcome:?}"
        );
    }
    assert_eq!(calls(&server), [REVIEW, PLACE]);
    assert_eq!(classes(&probe).len(), 2);
}

/// The server's own answer to a cancel, its order record rewritten to show `state` and
/// `filled_quantity`, in both the structured content and the text block that carries it.
fn showing(mut result: Value, state: &str, filled: &str) -> Result<String, McpError> {
    let record = result["structuredContent"].as_object_mut().unwrap();
    assert_eq!(
        record["state"], "cancelled",
        "the server cancelled the order"
    );
    record.insert("state".to_owned(), json!(state));
    record.insert("filled_quantity".to_owned(), json!(filled));
    let text = result["structuredContent"].to_string();
    result["content"][0]["text"] = json!(text);
    Ok(result.to_string())
}

/// Connections spec §6.2's reading of each state an order may show after a cancel request the
/// broker took but has not yet carried out, typed here: `new`, `queued`, `confirmed` and
/// `unconfirmed` working, `partially_filled` working with its fill, and `filled` filled. Each row
/// carries its own bend of the cancel answer, since a bend takes no state.
const NOT_CANCELLED: [(&str, &str, &str, Bend); 6] = [
    ("new", "0", "accepted", |r| showing(r, "new", "0")),
    ("queued", "0", "accepted", |r| showing(r, "queued", "0")),
    ("confirmed", "0", "accepted", |r| {
        showing(r, "confirmed", "0")
    }),
    ("unconfirmed", "0", "accepted", |r| {
        showing(r, "unconfirmed", "0")
    }),
    ("partially_filled", "1", "partially_filled", |r| {
        showing(r, "partially_filled", "1")
    }),
    ("filled", "2", "filled", |r| showing(r, "filled", "2")),
];

/// An opening of 2 SPY placed through the connector, then a cancel of it whose answer `bend`
/// rewrites; the cancel's outcome and the order's broker `order_id`.
fn cancel_answered(bend: Bend) -> (Result<BrokerOutcome, ConnectorError>, String) {
    let server = server();
    let (mut c, probe) = probed(&server);
    let BrokerOutcome::Submitted(open) = run(&mut c, &submit(buy("01JOPEN"))).unwrap() else {
        panic!()
    };
    probe.bend.set(Some((CANCEL, bend)));
    let cancel = BrokerRequest::Cancel {
        client_order_id: key("01JOPEN"),
    };
    let outcome = run(&mut c, &cancel);
    assert_eq!(calls(&server), [REVIEW, PLACE, CANCEL]);
    (outcome, open.broker_order_id)
}

/// DEC-867 items 1 and 2, trading spec §5.7 ("PendingCancel --> Canceled: confirmed"): a cancel
/// the broker took is `CancelAccepted` only when its answer shows the order `cancelled`. An answer
/// showing the order still working, part filled or filled is that order, read as §6.2 says, so the
/// executor keeps the cancel unconfirmed and holds the reservation and the exit behind it.
#[test]
#[ignore = "pending E7-6"]
fn a_cancel_answered_with_an_order_not_cancelled_is_that_order_never_cancel_accepted() {
    for (state, filled, status, bend) in NOT_CANCELLED {
        let (outcome, order_id) = cancel_answered(bend);
        let outcome = outcome.unwrap();
        assert!(
            !matches!(outcome, BrokerOutcome::CancelAccepted { .. }),
            "{state}: {outcome:?}"
        );
        let BrokerOutcome::Order(order) = outcome else {
            panic!("{state}: {outcome:?}")
        };
        assert_eq!(order.status, status, "{state}");
        assert_eq!(order.broker_order_id, order_id, "{state}");
        assert_eq!(
            order.client_order_id.as_deref(),
            Some(key("01JOPEN").as_str()),
            "{state}"
        );
        assert_eq!(
            (order.qty, order.filled_qty),
            (qty("2"), qty(filled)),
            "{state}"
        );
    }
}

/// DEC-867 item 2: the answer that shows the order `cancelled` is the broker's confirmation.
#[test]
#[ignore = "pending E7-6"]
fn a_cancel_answered_with_the_order_cancelled_is_cancel_accepted() {
    let (outcome, _) = cancel_answered(|r| showing(r, "cancelled", "0"));
    let id = key("01JOPEN").as_str().to_owned();
    assert_eq!(
        outcome,
        Ok(BrokerOutcome::CancelAccepted {
            client_order_id: id
        })
    );
}
