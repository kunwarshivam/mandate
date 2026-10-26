//! The shell's Robinhood connector and reconciler against the simulated server over loopback
//! (E7-6, DEC-928): the shell's `Connector` reviews then places with the derived `ref_id`, a
//! re-send is deduplicated by the broker and a restored key is never sent again, a lost or
//! garbled place answer is `Unknown`, `Cancel` goes by the broker's `order_id`, nothing outside
//! the profile is sent, every call names the recorded agentic account, the profile is
//! Robinhood's, and the reconciler reads only the agentic account, maps its snapshot by the
//! least-risk reading and hands it to the executor's own reconciliation (DEC-933 item 1), and the
//! bridge's preflight reads the same account (DEC-933 item 2). Oracles: the simulator's own
//! orders and call log, the requests the seam saw, and the snapshot written out field by field.

#[path = "../../mandate-rh-sim/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;

use common::wire::Wire;
use common::{AGENTIC, NOT_AGENTIC, account as holder, price, qty, sim};
use mandate_accounting::{InstrumentId, Side as Way};
use mandate_canon::parse;
use mandate_executor::{
    AccountRef, AccountScope, ActivityCursor, BrokerAccount, BrokerOrder, BrokerOutcome,
    BrokerPosition, BrokerRequest, BrokerSnapshot, ClientOrderId, ConnectorError, EventId,
    FoldedEvent, IntentId, OrderType as Kind, Purpose, ReconcileReason, Seq, SubmitOrder,
    TimeInForce as Tif, WorkspaceId,
};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_num::{SignedQty, Usd};
use mandate_rh_sim::{Event, Fault, Garble, Session, Sim, SimServer, State, Variant};
use mandate_robinhood::{Quote, RobinhoodConnector, Tickers, Tools, ref_id, robinhood_profile};
use mandate_runtime::AgentId as RuntimeAgentId;
use mandate_shell::Cause;
use mandate_shell::adapters::{CoreExecutor, Sources, robinhood_stages};
use mandate_shell::robinhood::{
    ACCOUNT_UNREAD, QUOTE_UNREAD, RobinhoodBridge, RobinhoodReconciler, RobinhoodShellConnector,
};
use mandate_shell::stages::{Connector, Reconciler};
use mandate_shell::symbols::BrokerSymbols;
use mandate_time::UtcNanos;
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
    bridge: Rc<RobinhoodBridge<Loopback<'s>>>,
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
    let robinhood =
        RobinhoodConnector::restore(tools, account.to_owned(), tickers(), records).unwrap();
    let bridge = RobinhoodBridge::new(runtime(), robinhood);
    let scope = AccountScope {
        account: AccountRef("acct_rh".to_owned()),
        workspace: WorkspaceId("ws_rh".to_owned()),
    };
    let (_, executor) = CoreExecutor::pair(scope, None);
    Shell {
        bridge: Rc::clone(&bridge),
        connector: RobinhoodShellConnector::new(Rc::clone(&bridge)),
        reconciler: RobinhoodReconciler::new(bridge, executor),
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
    read_server_echoing(false)
}

/// [`read_server`], echoing each order's `ref_id` as the broker deduplicates by it when `echo`.
fn read_server_echoing(echo: bool) -> SimServer {
    let mut s = Sim::new(vec![
        holder(AGENTIC, true, false),
        holder(NOT_AGENTIC, false, false),
    ])
    .unwrap();
    s.apply(Event::Quote("SPY".to_owned(), price("500")))
        .unwrap();
    s.apply(Event::Session(Session::Regular)).unwrap();
    s.apply(Event::EchoRefId(echo)).unwrap();
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
            Err(not_offered),
            "{request:?}"
        );
    }
    assert!(calls(&server).is_empty());
}

/// The account the reconciler reads from the simulator's agentic account (DEC-933 item 1): its
/// cash 750.25 as reported, its buying power of 10,000 clamped to that cash, the status the
/// agentic check stands for, and every unpublished figure at the value that adds no buying power
/// and lifts no limit.
fn mapped_account() -> BrokerAccount {
    let usd = |text: &str| Usd::parse(text).unwrap();
    BrokerAccount {
        status: "ACTIVE".to_owned(),
        crypto_status: "INACTIVE".to_owned(),
        trading_blocked: false,
        account_blocked: false,
        trade_suspended_by_user: false,
        multiplier: 1,
        equity: usd("750.25"),
        cash: usd("750.25"),
        buying_power: usd("750.25"),
        non_marginable_buying_power: usd("750.25"),
        accrued_fees: Usd::ZERO,
        last_equity: Usd::ZERO,
        maintenance_margin: usd("750.25"),
    }
}

/// A fresh executor's startup snapshot over `open_orders`, `positions` and the mapped account:
/// no fill, the empty cursor, the empty stream's head.
fn startup(open_orders: Vec<BrokerOrder>, positions: Vec<BrokerPosition>) -> BrokerSnapshot {
    BrokerSnapshot {
        open_orders,
        positions,
        account: mapped_account(),
        fills: Vec::new(),
        cursor: ActivityCursor(String::new()),
        reason: ReconcileReason::Startup,
        declared: None,
        taken_at_head: Seq(0),
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

const ACCOUNT_READS: [&str; 6] = [
    "get_accounts",
    "get_portfolio",
    "get_accounts",
    "get_equity_positions",
    "get_accounts",
    "get_equity_orders",
];

#[test]
fn the_reconciler_reads_only_the_agentic_account_and_maps_its_snapshot() {
    let server = read_server();
    let mut shell = shell(&server);
    let Shell {
        connector,
        reconciler,
        seen,
        ..
    } = &mut shell;
    let snapshot = reconciler.snapshot(connector, &snapshot_requests());
    assert_eq!(snapshot.unwrap(), startup(Vec::new(), Vec::new()));
    assert_eq!(calls(&server), ACCOUNT_READS);
    assert!(
        seen.borrow()
            .iter()
            .all(|(class, ..)| *class == CallClass::Read),
        "reads are reads, and never draw on the exits' budget"
    );
    every_call_names_the_agentic_account(seen);
    assert!(
        orders(&server).is_empty(),
        "a reconciliation places nothing"
    );
}

/// The positions and working orders are the broker's as `mandate-robinhood` read them, a part
/// filled order included and named by the key it was placed under (DEC-1151 item 6), and the
/// account is mapped the same way whatever the account holds: the fill spent 498 of the cash, and
/// the buying power is clamped to what is left.
#[test]
fn the_reconcilers_snapshot_lists_what_the_account_holds() {
    let server = read_server_echoing(true);
    let mut shell = shell(&server);
    let order = submitted(shell.connector.call(&buy("01JOPEN")));
    server
        .drive(|core| core.fill(&order.broker_order_id, qty("1"), price("498")))
        .unwrap()
        .unwrap();
    let Shell {
        connector,
        reconciler,
        ..
    } = &mut shell;
    let snapshot = reconciler
        .snapshot(connector, &snapshot_requests())
        .unwrap();
    let position = BrokerPosition {
        instrument: InstrumentId::new("SPY").unwrap(),
        qty: SignedQty::parse("1").unwrap(),
        avg_entry_price: price("498"),
    };
    let working = BrokerOrder {
        broker_order_id: order.broker_order_id.clone(),
        client_order_id: Some(key("01JOPEN").as_str().to_owned()),
        instrument: InstrumentId::new("SPY").unwrap(),
        side: Way::Buy,
        qty: qty("2"),
        filled_qty: qty("1"),
        limit_price: Some(price("499")),
        stop_price: None,
        status: "partially_filled".to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    };
    let left = Usd::parse("252.25").unwrap();
    let mut expected = startup(vec![working], vec![position]);
    expected.account = BrokerAccount {
        equity: left,
        cash: left,
        buying_power: left,
        non_marginable_buying_power: left,
        maintenance_margin: left,
        ..mapped_account()
    };
    assert_eq!(snapshot, expected);
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

/// The snapshot goes to the executor's own reconciliation, which refuses here only because this
/// executor was given no trusted context; no broker call is made for it.
#[test]
fn the_snapshot_goes_to_the_executors_own_reconciliation() {
    let server = server();
    let mut shell = shell(&server);
    let refused = shell.reconciler.reconcile(&startup(Vec::new(), Vec::new()));
    assert!(
        matches!(refused, Err(Cause::Absent { what }) if what == "the trusted executor context"),
        "{refused:?}"
    );
    assert!(calls(&server).is_empty());
}

/// The bridge's preflight is `mandate-robinhood`'s own read, through the same session: the
/// account reads, the tradability read and the quote, nothing placed (DEC-933 item 2).
#[test]
fn the_bridges_preflight_reads_the_agentic_account_and_the_quote() {
    let server = read_server();
    let shell = shell(&server);
    let facts = shell
        .bridge
        .preflight_facts(&InstrumentId::new("SPY").unwrap())
        .unwrap();
    assert_eq!(facts.account.cash, Usd::parse("750.25").unwrap());
    assert_eq!(facts.account.buying_power, Usd::parse("10000").unwrap());
    assert!(facts.account.positions.is_empty() && facts.account.open_orders.is_empty());
    assert_eq!(
        facts.quote,
        Quote {
            symbol: InstrumentId::new("SPY").unwrap(),
            bid: price("500"),
            ask: price("500"),
        }
    );
    let mut reads = ACCOUNT_READS.to_vec();
    reads.extend([
        "get_accounts",
        "get_equity_tradability",
        "get_equity_quotes",
    ]);
    assert_eq!(calls(&server), reads);
    for (_, tool, args) in shell.seen.borrow().iter() {
        let expected = match *tool {
            "get_accounts" | "get_equity_quotes" => None,
            _ => Some(AGENTIC),
        };
        assert_eq!(args["account_number"].as_str(), expected, "{tool}: {args}");
    }
}

#[test]
fn the_bridges_preflight_of_an_account_that_is_not_agentic_refuses() {
    let server = read_server();
    let shell = shell_for(&server, NOT_AGENTIC, &[]);
    let refused = shell
        .bridge
        .preflight_facts(&InstrumentId::new("SPY").unwrap());
    assert!(
        matches!(refused, Err(Cause::Absent { what }) if what == ACCOUNT_UNREAD),
        "{refused:?}"
    );
    assert_eq!(calls(&server), ["get_accounts"]);
}

/// The live door's lapse evidence (DEC-947 item 1, DEC-933 item 3): the bridge reads the agentic
/// account's snapshot through the same session, the account reads alone, no quote and nothing
/// placed, and lists what the account holds, a filled position and a working order included.
#[test]
fn the_bridges_snapshot_reads_the_agentic_account_alone() {
    let server = read_server();
    let shell = shell(&server);
    let snapshot = shell.bridge.account_snapshot().unwrap();
    assert_eq!(snapshot.cash, Usd::parse("750.25").unwrap());
    assert!(snapshot.positions.is_empty() && snapshot.open_orders.is_empty());
    assert_eq!(calls(&server), ACCOUNT_READS);
    let held = read_server_echoing(true);
    let mut shell = shell_for(&held, AGENTIC, &[]);
    let order = submitted(shell.connector.call(&buy("01JHELD")));
    held.drive(|core| core.fill(&order.broker_order_id, qty("1"), price("498")))
        .unwrap()
        .unwrap();
    let snapshot = shell.bridge.account_snapshot().unwrap();
    assert_eq!(
        (snapshot.positions.len(), snapshot.open_orders.len()),
        (1, 1)
    );
}

/// A snapshot of an account that is not agentic refuses as every unread account does, after the
/// one listing read.
#[test]
fn the_bridges_snapshot_of_an_account_that_is_not_agentic_refuses() {
    let server = read_server();
    let shell = shell_for(&server, NOT_AGENTIC, &[]);
    let refused = shell.bridge.account_snapshot();
    assert!(
        matches!(refused, Err(Cause::Absent { what }) if what == ACCOUNT_UNREAD),
        "{refused:?}"
    );
    assert_eq!(calls(&server), ["get_accounts"]);
}

/// The production stages over one Robinhood session (DEC-947 item 4, DEC-928 item 2): the run's
/// connector is the Robinhood connector, so its profile is Robinhood's and each request is
/// placed through the session, and the startup reconciliation reads the agentic account through
/// that same session, never Alpaca's four requests.
#[test]
fn the_robinhood_stages_trade_and_reconcile_through_one_session() {
    let server: &'static SimServer = Box::leak(Box::new(read_server_echoing(true)));
    let (garble_place, seen) = (Rc::default(), Seen::default());
    let tools = Loopback {
        server,
        wire: RefCell::new(Wire::connect(&server.url().unwrap())),
        garble_place,
        seen,
    };
    let robinhood = RobinhoodConnector::new(tools, AGENTIC.to_owned(), tickers());
    let sources = Sources {
        mandate: PathBuf::from("unread-mandate.json"),
        journal: None,
        recorded_at: UtcNanos::from_parts(0, 0).unwrap(),
        agent: RuntimeAgentId("agent_rh".to_owned()),
        workspace: "ws_rh".to_owned(),
        account_ref: "acct_rh".to_owned(),
        executor: None,
        run: None,
        artifacts: None,
        content_store: None,
        symbols: BrokerSymbols::default(),
        transport: RobinhoodBridge::new(runtime(), robinhood),
    };
    let mut stages = robinhood_stages(sources);
    assert_eq!(stages.connector.profile(), robinhood_profile());
    let snapshot = stages
        .reconciler
        .snapshot(&mut *stages.connector, &snapshot_requests())
        .unwrap();
    assert_eq!(snapshot, startup(Vec::new(), Vec::new()));
    assert_eq!(calls(server), ACCOUNT_READS);
    let order = submitted(stages.connector.call(&buy("01JSTAGES")));
    assert_eq!(orders(server).len(), 1);
    assert_eq!(orders(server)[0].id, order.broker_order_id);
}

/// DEC-945 item 1: the bridge's quote read is `get_equity_quotes` alone, on the `Ordinary`
/// budget with exactly `{"symbols": [symbol]}` and no account, answered with the simulator's
/// locked quote; a second read answers the quote the broker holds then, not the first one.
#[test]
fn the_bridges_quote_is_the_brokers_latest_and_names_no_account() {
    let server = read_server();
    let shell = shell(&server);
    let spy = InstrumentId::new("SPY").unwrap();
    let quote = shell.bridge.equity_quote(&spy).unwrap();
    assert_eq!(
        quote,
        Quote {
            symbol: spy.clone(),
            bid: price("500"),
            ask: price("500"),
        }
    );
    assert_eq!(calls(&server), ["get_equity_quotes"]);
    assert_eq!(
        *shell.seen.borrow(),
        [(
            CallClass::Read,
            "get_equity_quotes",
            json!({"symbols": ["SPY"]})
        )]
    );
    server
        .drive(|sim| sim.apply(Event::Quote("SPY".to_owned(), price("501.25"))))
        .unwrap()
        .unwrap();
    let moved = shell.bridge.equity_quote(&spy).unwrap();
    assert_eq!((moved.bid, moved.ask), (price("501.25"), price("501.25")));
    assert_eq!(calls(&server), ["get_equity_quotes", "get_equity_quotes"]);
}

/// DEC-945 item 1: a symbol the broker quotes nothing for, and a garbled answer, each refuse the
/// read as [`QUOTE_UNREAD`]; nothing but the quote read is called.
#[test]
fn the_bridges_quote_refuses_a_missing_or_garbled_answer() {
    let server = read_server();
    let shell = shell(&server);
    let missing = shell
        .bridge
        .equity_quote(&InstrumentId::new("QQQ").unwrap());
    assert!(
        matches!(missing, Err(Cause::Absent { what }) if what == QUOTE_UNREAD),
        "{missing:?}"
    );
    server.garble_next(Garble::NotJson).unwrap();
    let garbled = shell
        .bridge
        .equity_quote(&InstrumentId::new("SPY").unwrap());
    assert!(
        matches!(garbled, Err(Cause::Absent { what }) if what == QUOTE_UNREAD),
        "{garbled:?}"
    );
    assert_eq!(calls(&server), ["get_equity_quotes", "get_equity_quotes"]);
}

/// SPY and QQQ registered as their own asset ids, as these tests name them (DEC-956 item 1).
fn tickers() -> Tickers {
    let pairs = ["SPY", "QQQ"].map(|s| (InstrumentId::new(s).unwrap(), s.to_owned()));
    Tickers::registered(pairs).unwrap()
}

/// DEC-956 item 1: a session opened before the run read its mandate holds no ticker, so the
/// bridge quotes nothing until the run registers the pinned instrument's `(asset id, symbol)`;
/// from then on the quote is read under that ticker.
#[test]
fn the_bridge_quotes_only_what_the_run_registered() {
    let server = read_server();
    let tools = Loopback {
        server: &server,
        wire: RefCell::new(Wire::connect(&server.url().unwrap())),
        garble_place: Rc::default(),
        seen: Seen::default(),
    };
    let robinhood = RobinhoodConnector::new(tools, AGENTIC.to_owned(), Tickers::default());
    let bridge = RobinhoodBridge::new(runtime(), robinhood);
    let spy = InstrumentId::new("SPY").unwrap();
    assert!(
        matches!(bridge.equity_quote(&spy), Err(Cause::Absent { what }) if what == QUOTE_UNREAD)
    );
    assert!(
        calls(&server).is_empty(),
        "nothing is asked before the registration"
    );
    bridge.register(tickers()).unwrap();
    let quote = bridge.equity_quote(&spy).unwrap();
    assert_eq!((quote.symbol, quote.ask), (spy, price("500")));
    assert_eq!(calls(&server), ["get_equity_quotes"]);
}

/// DEC-966 items 1 and 6: once the entry has filled at the broker, the protective stop the
/// executor sends on Robinhood's profile goes through the shell's connector as one `gtc`
/// `stop_limit` sell of the 2, stop 480 and limit 477.6, and rests: it does not fill while SPY
/// quotes above 480, and fills once SPY reaches it. A take-profit sell beside it is refused by
/// the broker, which has no OCO, so the two can never both sell the 2.
#[test]
fn the_protective_stop_rests_at_the_broker_and_fills_only_once_triggered() {
    let server = server();
    let mut shell = shell(&server);
    let entry = submitted(shell.connector.call(&buy("01JSTOPENTRY")));
    server
        .drive(|sim| sim.fill(&entry.broker_order_id, qty("2"), price("499")))
        .unwrap()
        .unwrap();
    let protective = |order_type, limit: &str, stop: Option<&str>, event: &str| SubmitOrder {
        client_order_id: ClientOrderId::for_protection(
            &key("01JSTOPENTRY"),
            &EventId(event.to_owned()),
        )
        .unwrap(),
        instrument: InstrumentId::new("SPY").unwrap(),
        side: Way::Sell,
        qty: qty("2"),
        order_type,
        tif: Tif::Gtc,
        limit_price: Some(price(limit)),
        stop_price: stop.map(price),
        bracket: None,
        oco: None,
        extended_hours: false,
        purpose: Purpose::Protective,
    };
    let stop = protective(Kind::StopLimit, "477.6", Some("480"), "01JSTOP");
    let rests = submitted(shell.connector.call(&BrokerRequest::Submit(stop)));
    let held = orders(&server)
        .into_iter()
        .find(|order| order.id == rests.broker_order_id)
        .unwrap();
    assert_eq!(
        (
            held.order_type,
            held.time_in_force,
            held.state,
            held.stop_price,
            held.limit_price,
            held.quantity,
        ),
        (
            mandate_rh_sim::OrderType::StopLimit,
            mandate_rh_sim::TimeInForce::Gtc,
            State::Confirmed,
            Some(price("480")),
            Some(price("477.6")),
            qty("2"),
        )
    );
    let take_profit = protective(Kind::Limit, "520", None, "01JTAKE");
    let refused = shell.connector.call(&BrokerRequest::Submit(take_profit));
    assert!(
        !matches!(refused, Ok(BrokerOutcome::Submitted(_))),
        "no OCO: a take-profit beside the whole-position stop is refused: {refused:?}"
    );
    assert_eq!(orders(&server).len(), 2, "the entry and the stop alone");
    let early = server
        .drive(|sim| sim.fill(&held.id, qty("2"), price("490")))
        .unwrap();
    assert_eq!(early, Err(mandate_rh_sim::SimError::NotTriggered));
    let filled = server
        .drive(|sim| {
            sim.apply(Event::Quote("SPY".to_owned(), price("480")))?;
            sim.fill(&held.id, qty("2"), price("478"))
        })
        .unwrap()
        .unwrap();
    assert_eq!(filled.state, State::Filled);
    let position = server.drive(|sim| sim.position(AGENTIC, "SPY")).unwrap();
    assert_eq!(position, Ok(qty("0")));
}

/// R0's R18 (DEC-1150 item 7): a broker that does not echo our `ref_id` lists our order with no
/// client order id, so the connector names the record by the broker's own id for it, the one its
/// place answered (journal spec §9.16: the connector reads that id, the executor's fold never
/// does). A connector that never placed the order, and so holds no id for it, names nothing.
#[test]
fn a_listing_names_our_order_by_the_broker_id_its_place_answered() {
    let server = read_server_echoing(false);
    let opened = UtcNanos::parse("2026-10-20T15:00:00.000000000Z").unwrap();
    server
        .drive(|sim| sim.apply(Event::Clock(opened)))
        .unwrap()
        .unwrap();
    let mut placing = shell(&server);
    let order = submitted(placing.connector.call(&buy("01JOPEN")));
    server
        .drive(|sim| sim.fill(&order.broker_order_id, qty("2"), price("499")))
        .unwrap()
        .unwrap();
    let listing = BrokerRequest::ListOrders(mandate_executor::OrderListing {
        client_order_id: key("01JOPEN"),
        instrument: InstrumentId::new("SPY").unwrap(),
        origin: mandate_executor::OrderOrigin::Agentic,
        created_since: mandate_executor::RiskClock::from_secs(opened.secs() - 300),
    });
    let named = |outcome: Result<BrokerOutcome, ConnectorError>| match outcome {
        Ok(BrokerOutcome::Listed { orders, .. }) => orders
            .into_iter()
            .map(|record| (record.order.client_order_id, record.order.status))
            .collect::<Vec<_>>(),
        other => panic!("a listing: {other:?}"),
    };
    let ours = Some(key("01JOPEN").as_str().to_owned());
    assert_eq!(
        named(placing.connector.call(&listing)),
        vec![(ours, "filled".to_owned())],
        "the one record is ours, by the order id the place answered"
    );
    let mut stranger = shell(&server);
    assert_eq!(
        named(stranger.connector.call(&listing)),
        vec![(None, "filled".to_owned())],
        "a connector that holds no id for the key names nothing"
    );
}

/// R0's R21 (DEC-1151 item 6): a broker that does not echo our `ref_id` lists our working order
/// with none, so the snapshot names it by the order id this connector's place answered, and the
/// reconciliation finds it ours rather than the broker's alone. A connector that never placed the
/// order, and so holds no id for it, names nothing.
#[test]
fn a_snapshot_names_our_working_order_by_the_broker_id_its_place_answered() {
    let server = read_server_echoing(false);
    let mut placing = shell(&server);
    submitted(placing.connector.call(&buy("01JOPEN")));
    let named = |shell: &mut Shell<'_>| {
        let Shell {
            connector,
            reconciler,
            ..
        } = shell;
        reconciler
            .snapshot(connector, &snapshot_requests())
            .unwrap()
            .open_orders
            .into_iter()
            .map(|order| order.client_order_id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        named(&mut placing),
        [Some(key("01JOPEN").as_str().to_owned())],
        "named by the id the place answered"
    );
    let mut stranger = shell(&server);
    assert_eq!(
        named(&mut stranger),
        [None],
        "a connector that holds no id names nothing"
    );
}
