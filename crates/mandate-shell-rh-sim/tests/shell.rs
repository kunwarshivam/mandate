//! The shell's Robinhood connector and reconciler against the simulated server over loopback
//! (E7-6, DEC-928): the shell's `Connector` reviews then places with the derived `ref_id`, a
//! re-send is deduplicated by the broker and a restored key is never sent again, a lost or
//! garbled place answer is `Unknown`, `Cancel` goes by the broker's `order_id`, nothing outside
//! the profile is sent, every call names the recorded agentic account, the profile is
//! Robinhood's, and the reconciler reads only the agentic account and then refuses (DEC-928
//! item 3). Oracles: the simulator's own orders and call log, and the requests the seam saw.

#[path = "../../mandate-rh-sim/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::wire::Wire;
use common::{AGENTIC, NOT_AGENTIC, account as holder, price, qty, sim};
use mandate_accounting::{InstrumentId, Side as Way};
use mandate_canon::parse;
use mandate_executor::{
    ActivityCursor, BrokerAccount, BrokerOutcome, BrokerRequest, BrokerSnapshot, ClientOrderId,
    ConnectorError, EventId, FoldedEvent, IntentId, OrderType as Kind, Purpose, ReconcileReason,
    Seq, SubmitOrder, TimeInForce as Tif,
};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_num::Usd;
use mandate_rh_sim::{Event, Fault, Garble, Session, Sim, SimServer, State, Variant};
use mandate_robinhood::{RobinhoodConnector, Tools, ref_id, robinhood_profile};
use mandate_shell::Cause;
use mandate_shell::robinhood::{
    ACCOUNT_UNREAD, RobinhoodBridge, RobinhoodReconciler, RobinhoodShellConnector,
    STATUS_UNPUBLISHED,
};
use mandate_shell::stages::{Connector, Reconciler};
use serde_json::{Value, json};
use tokio::runtime::{Builder, Runtime};

const REVIEW: &str = "review_equity_order";
const PLACE: &str = "place_equity_order";
const CANCEL: &str = "cancel_equity_order";

/// Every call the seam made: its budget class, its tool and its arguments.
type Seen = Rc<RefCell<Vec<(CallClass, &'static str, Value)>>>;

/// The seam over the wire client, with a garble for the next place answer, never the review.
struct Loopback<'s> {
    server: &'s SimServer,
    wire: RefCell<Wire>,
    garble_place: Rc<Cell<Option<Garble>>>,
    seen: Seen,
}

impl Tools for Loopback<'_> {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        args: &Value,
    ) -> Result<String, McpError> {
        self.seen.borrow_mut().push((class, tool, args.clone()));
        if tool == PLACE
            && let Some(garble) = self.garble_place.take()
        {
            self.server.garble_next(garble).unwrap();
        }
        let reply = self.wire.borrow_mut().call_raw(tool, args.clone());
        let message: Value = serde_json::from_str(&reply.ok_or(McpError::Network)?.body)
            .map_err(|_| McpError::Malformed)?;
        let result = message.get("result").ok_or(McpError::Malformed)?;
        Ok(result.to_string())
    }
}

struct Shell<'s> {
    connector: RobinhoodShellConnector<Loopback<'s>>,
    reconciler: RobinhoodReconciler<Loopback<'s>>,
    garble_place: Rc<Cell<Option<Garble>>>,
    seen: Seen,
}

fn runtime() -> Runtime {
    Builder::new_current_thread().build().unwrap()
}

/// The shell's pair over one bridge for `account`, restored from `records` when there are any.
fn shell_for<'s>(server: &'s SimServer, account: &str, records: &[FoldedEvent]) -> Shell<'s> {
    let (garble_place, seen) = (Rc::default(), Seen::default());
    let tools = Loopback {
        server,
        wire: RefCell::new(Wire::connect(&server.url().unwrap())),
        garble_place: Rc::clone(&garble_place),
        seen: Rc::clone(&seen),
    };
    let robinhood = RobinhoodConnector::restore(tools, account.to_owned(), records).unwrap();
    let bridge = RobinhoodBridge::new(runtime(), robinhood);
    Shell {
        connector: RobinhoodShellConnector::new(Rc::clone(&bridge)),
        reconciler: RobinhoodReconciler::new(bridge),
        garble_place,
        seen,
    }
}

fn shell(server: &SimServer) -> Shell<'_> {
    shell_for(server, AGENTIC, &[])
}

fn server() -> SimServer {
    let mut s = sim().unwrap();
    s.apply(Event::EchoRefId(true)).unwrap();
    SimServer::start(s, Variant::Honest).unwrap()
}

/// The server the reads run against: exactly one agentic account beside one that is not, since
/// DEC-875 item 1 refuses every read of a session that lists two agentic accounts.
fn read_server() -> SimServer {
    let mut s = Sim::new(vec![
        holder(AGENTIC, true, false),
        holder(NOT_AGENTIC, false, false),
    ])
    .unwrap();
    s.apply(Event::Quote("SPY".to_owned(), price("500")))
        .unwrap();
    s.apply(Event::Session(Session::Regular)).unwrap();
    SimServer::start(s, Variant::Honest).unwrap()
}

fn key(intent: &str) -> ClientOrderId {
    ClientOrderId::for_intent(&IntentId(EventId(intent.to_owned()))).unwrap()
}

/// A `gfd` limit buy of 2 SPY at 499 for an opening.
fn buy(intent: &str) -> BrokerRequest {
    BrokerRequest::Submit(SubmitOrder {
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
    })
}

/// The server's call log, which must stay inside the allowlist (LT-8).
fn calls(server: &SimServer) -> Vec<String> {
    let calls = server.calls().unwrap();
    assert!(
        calls.iter().all(|c| ALLOWLIST.contains(&c.as_str())),
        "{calls:?}"
    );
    calls
}

fn orders(server: &SimServer) -> Vec<mandate_rh_sim::Order> {
    server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap()
}

/// Every call that names an account names the recorded one (CN-8); `get_accounts` names none.
fn every_call_names_the_agentic_account(seen: &Seen) {
    for (_, tool, args) in seen.borrow().iter() {
        let expected = if *tool == "get_accounts" {
            None
        } else {
            Some(AGENTIC)
        };
        assert_eq!(args["account_number"].as_str(), expected, "{tool}: {args}");
    }
}

fn submitted(outcome: Result<BrokerOutcome, ConnectorError>) -> mandate_executor::BrokerOrder {
    match outcome {
        Ok(BrokerOutcome::Submitted(order)) => order,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_shell_connector_declares_robinhoods_profile() {
    let server = server();
    let shell = shell(&server);
    assert_eq!(shell.connector.profile(), robinhood_profile());
    assert!(calls(&server).is_empty(), "the profile calls nothing");
}

#[test]
fn a_submit_through_the_shell_reviews_then_places_with_the_ref_id() {
    let server = server();
    let mut shell = shell(&server);
    let order = submitted(shell.connector.call(&buy("01JOPEN")));
    assert_eq!(calls(&server), [REVIEW, PLACE]);
    let seen = shell.seen.borrow().clone();
    let expected = ref_id(&key("01JOPEN")).unwrap();
    assert_eq!(seen[1].2["ref_id"].as_str(), Some(expected.as_str()));
    assert_eq!(
        seen[0].2.get("ref_id"),
        None,
        "the review carries no ref_id"
    );
    assert_eq!(
        [seen[0].0, seen[1].0],
        [CallClass::Ordinary, CallClass::Ordinary]
    );
    every_call_names_the_agentic_account(&shell.seen);
    let placed = orders(&server);
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].ref_id.as_deref(), Some(expected.as_str()));
    assert_eq!(order.broker_order_id, placed[0].id);
    assert_eq!(
        order.client_order_id.as_deref(),
        Some(key("01JOPEN").as_str())
    );
    assert_eq!(order.status, "accepted");
}

#[test]
fn a_resend_by_a_fresh_shell_is_deduplicated_by_the_broker() {
    let server = server();
    let first = submitted(shell(&server).connector.call(&buy("01JOPEN")));
    let again = submitted(shell(&server).connector.call(&buy("01JOPEN")));
    assert_eq!(first.broker_order_id, again.broker_order_id);
    assert_eq!(orders(&server).len(), 1, "one ref_id, one order");
    assert_eq!(calls(&server), [REVIEW, PLACE, REVIEW, PLACE]);
}

/// What the executor journals for an order placed before a restart (journal spec §9.16).
fn journaled(key: &ClientOrderId, id: Option<&str>) -> Vec<FoldedEvent> {
    let submitted = json!({"client_order_id": key.as_str(), "instrument_id": "SPY",
        "side": "buy", "qty": "2", "type": "limit", "tif": "day", "limit_price": "499"});
    let changed = json!({"client_order_id": key.as_str(), "state": "accepted",
        "risk_clock": "2026-10-09T14:30:00Z", "attempted": null, "broker_status": "accepted",
        "filled_qty": null, "reject_code": null, "replaces": null, "replaced_by": null,
        "replaced_by_broker_order_id": null, "lookup": null, "ignored": false,
        "cancel_requested": false, "cancel_confirmed": false, "cancel_overdue": false,
        "adopted": false, "ladder_step": false, "broker_order_id": id});
    let event = |(n, (event_type, payload)): (u64, (&str, Value))| FoldedEvent {
        stream: "account".to_owned(),
        seq: Seq(n + 1),
        event_id: EventId(format!("01JEVENT{n:018}")),
        event_type: event_type.to_owned(),
        causation_id: None,
        payload: parse(payload.to_string().as_bytes()).unwrap(),
    };
    let records = [
        ("OrderSubmitted", submitted),
        ("OrderStateChanged", changed),
    ];
    (0..).zip(records).map(event).collect()
}

#[test]
fn after_a_restart_a_journaled_key_is_never_sent_again_and_cancels_by_its_id() {
    let server = server();
    let placed = submitted(shell(&server).connector.call(&buy("01JRESTART")));
    let records = journaled(&key("01JRESTART"), Some(&placed.broker_order_id));
    let mut restarted = shell_for(&server, AGENTIC, &records);
    let again = restarted.connector.call(&buy("01JRESTART"));
    assert!(
        matches!(again, Err(ConnectorError::Unknown(_))),
        "{again:?}"
    );
    assert_eq!(calls(&server), [REVIEW, PLACE], "nothing is called for it");
    let cancel = BrokerRequest::Cancel {
        client_order_id: key("01JRESTART"),
    };
    let accepted = BrokerOutcome::CancelAccepted {
        client_order_id: key("01JRESTART").as_str().to_owned(),
    };
    assert_eq!(restarted.connector.call(&cancel), Ok(accepted));
    let seen = restarted.seen.borrow().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!((seen[0].0, seen[0].1), (CallClass::RiskReducing, CANCEL));
    assert_eq!(
        seen[0].2,
        json!({"account_number": AGENTIC, "order_id": placed.broker_order_id})
    );
    assert_eq!(orders(&server)[0].state, State::Cancelled);
}

#[test]
fn a_lost_place_answer_is_unknown_and_never_placed_again() {
    let server = server();
    server
        .drive(|sim| sim.apply(Event::Script(Fault::LoseAnswer)))
        .unwrap()
        .unwrap();
    let mut shell = shell(&server);
    for attempt in 0..2 {
        let outcome = shell.connector.call(&buy("01JLOST"));
        assert!(
            matches!(outcome, Err(ConnectorError::Unknown(_))),
            "{attempt}: {outcome:?}"
        );
    }
    assert_eq!(
        calls(&server),
        [REVIEW, PLACE],
        "the order in doubt is never re-sent"
    );
    assert_eq!(orders(&server).len(), 1);
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
        let mut shell = shell(&server);
        shell.garble_place.set(Some(garble));
        let outcome = shell.connector.call(&buy("01JBENT"));
        assert!(
            matches!(outcome, Err(ConnectorError::Unknown(_))),
            "{garble:?}: {outcome:?}"
        );
        assert_eq!(calls(&server), [REVIEW, PLACE], "{garble:?}");
    }
}

#[test]
fn a_cancel_goes_by_the_order_id_and_one_with_no_id_is_not_sent() {
    let server = server();
    let mut shell = shell(&server);
    let placed = submitted(shell.connector.call(&buy("01JCANCEL")));
    let cancel = |intent: &str| BrokerRequest::Cancel {
        client_order_id: key(intent),
    };
    let accepted = BrokerOutcome::CancelAccepted {
        client_order_id: key("01JCANCEL").as_str().to_owned(),
    };
    assert_eq!(shell.connector.call(&cancel("01JCANCEL")), Ok(accepted));
    assert_eq!(
        shell.seen.borrow()[2].2["order_id"],
        json!(placed.broker_order_id)
    );
    let refused = ConnectorError::NotSent {
        code: "no_order_id",
    };
    assert_eq!(shell.connector.call(&cancel("01JNEVER")), Err(refused));
    assert_eq!(calls(&server), [REVIEW, PLACE, CANCEL]);
    assert_eq!(orders(&server)[0].state, State::Cancelled);
}

#[test]
fn nothing_outside_the_profile_is_sent() {
    let server = server();
    let mut shell = shell(&server);
    let not_offered = ConnectorError::NotSent {
        code: "not_offered",
    };
    for request in [
        BrokerRequest::ListOpenOrders,
        BrokerRequest::ListPositions,
        BrokerRequest::GetAccount,
        BrokerRequest::ListActivities {
            since: ActivityCursor(String::new()),
        },
    ] {
        assert_eq!(
            shell.connector.call(&request),
            Err(not_offered.clone()),
            "{request:?}"
        );
    }
    assert!(calls(&server).is_empty());
}

fn account() -> BrokerAccount {
    let usd = |text: &str| Usd::parse(text).unwrap();
    BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "ACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 1,
        equity: usd("750.25"),
        cash: usd("750.25"),
        buying_power: usd("750.25"),
        non_marginable_buying_power: usd("750.25"),
        accrued_fees: usd("0"),
        last_equity: usd("750.25"),
        maintenance_margin: usd("0"),
    }
}

fn snapshot_requests() -> [BrokerRequest; 4] {
    [
        BrokerRequest::ListOpenOrders,
        BrokerRequest::ListPositions,
        BrokerRequest::GetAccount,
        BrokerRequest::ListActivities {
            since: ActivityCursor(String::new()),
        },
    ]
}

fn absent(cause: Result<BrokerSnapshot, Cause>) -> &'static str {
    match cause {
        Err(Cause::Absent { what }) => what,
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_reconciler_reads_only_the_agentic_account_and_then_refuses() {
    let server = read_server();
    let mut shell = shell(&server);
    let Shell {
        connector,
        reconciler,
        seen,
        ..
    } = &mut shell;
    let snapshot = reconciler.snapshot(connector, &snapshot_requests());
    assert_eq!(absent(snapshot), STATUS_UNPUBLISHED);
    let reads = [
        "get_accounts",
        "get_portfolio",
        "get_accounts",
        "get_equity_positions",
        "get_accounts",
        "get_equity_orders",
    ];
    assert_eq!(calls(&server), reads);
    assert!(
        seen.borrow()
            .iter()
            .all(|(class, ..)| *class == CallClass::Ordinary),
        "reads never draw on the exits' budget"
    );
    every_call_names_the_agentic_account(seen);
    assert!(
        orders(&server).is_empty(),
        "a reconciliation places nothing"
    );
}

#[test]
fn an_account_that_is_not_agentic_is_never_read_and_its_snapshot_refuses() {
    let server = read_server();
    let mut shell = shell_for(&server, NOT_AGENTIC, &[]);
    let Shell {
        connector,
        reconciler,
        ..
    } = &mut shell;
    let snapshot = reconciler.snapshot(connector, &snapshot_requests());
    assert_eq!(absent(snapshot), ACCOUNT_UNREAD);
    assert_eq!(calls(&server), ["get_accounts"]);
}

#[test]
fn no_snapshot_is_reconciled_for_a_robinhood_account() {
    let server = server();
    let mut shell = shell(&server);
    let snapshot = BrokerSnapshot {
        open_orders: Vec::new(),
        positions: Vec::new(),
        account: account(),
        fills: Vec::new(),
        cursor: ActivityCursor(String::new()),
        reason: ReconcileReason::Startup,
        taken_at_head: Seq(0),
    };
    let refused = shell.reconciler.reconcile(&snapshot);
    assert!(
        matches!(refused, Err(Cause::Absent { what }) if what == STATUS_UNPUBLISHED),
        "{refused:?}"
    );
    assert!(calls(&server).is_empty());
}
