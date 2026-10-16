//! An order left `Unknown` by a lost place answer, recovered on the restart through the shell's
//! run path against the simulated Robinhood server (the first live trade brief's B2b, C3 and
//! LT-6; DEC-529 item 4, DEC-869, DEC-941). The first run places SPY through the shell's
//! Robinhood connector and loses the answer, so the journal holds the order `Unknown`. The
//! restart folds that journal: its `Input::Started` lists the agentic account's orders through
//! the connector by `symbol`, `placed_agent` `agentic` and `created_at_gte`, before the startup
//! reconciliation and before any new order, and adopts the one match, journaling
//! `OrderStateChanged` version 2 with the broker's `order_id`; with no match the order stays
//! `Unknown` and no opening in SPY is placed. Nothing is ever placed twice.
//!
//! The first run's startup reconciliation, and the second's where it must pass, read a flat
//! account from a scripted stand-in: the Robinhood reconciler refuses every account until its
//! account fields have a source (DEC-928 item 3, C4). Every order request goes to the shell's
//! Robinhood connector over loopback. Oracles: the simulator's own orders and call log, and the
//! account stream's own records.

#[path = "../../mandate-rh-sim/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-shell/tests/common/mod.rs"]
mod shell_common;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use common::wire::Wire;
use common::{AGENTIC, NOT_AGENTIC, account as holder, price};
use mandate_alpaca::Exchange;
use mandate_canon::{Digest, Value, to_canonical};
use mandate_executor::{
    BrokerAccount, BrokerOutcome, BrokerRequest, ConnectorError, EventId, FoldedEvent, Seq,
    SubmitOrder,
};
use mandate_journal::{ArtifactSource, StoredEvent};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_num::Price as Px;
use mandate_rh_sim::{Event, Fault, Session, Sim, SimServer, Variant};
use mandate_robinhood::{RobinhoodConnector, Tools, robinhood_profile};
use mandate_runtime::{ModelOutput, Observation};
use mandate_shell::adapters::{Sources, over};
use mandate_shell::paper::load_contexts;
use mandate_shell::robinhood::{
    RobinhoodBridge, RobinhoodReconciler, RobinhoodShellConnector, STATUS_UNPUBLISHED,
};
use mandate_shell::stages::{Connector, Stages};
use mandate_shell::{Cause, Report, Setup, ShellError, Stage, run_observed};
use mandate_spec::validate::RegisteredModel;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};
use shell_common::{FEE, RULES, SNAPSHOT, SPY, Stream, json, model, spy_facts, spy_mandate};
use tokio::runtime::Builder;

const REVIEW: &str = "review_equity_order";
const PLACE: &str = "place_equity_order";
const ORDERS: &str = "get_equity_orders";
const SOURCE: &str = "alpaca_iex_daily_bars";
/// Tuesday 13:00 in New York, the first run; the restart is a minute later.
const FIRST: &str = "2026-09-29T17:00:00Z";
const RESTART: &str = "2026-09-29T17:01:00Z";
const ACCOUNT_STREAM: &str = "acct:ws_spy:acct_spy";

/// The confirmed document as the file the run's mandate stage reads, written once per process so
/// parallel test threads never read it half written.
static MANDATE_FILE: OnceLock<PathBuf> = OnceLock::new();

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// SPY's mandate with its order cap raised to the allocation, so one share at 655.20 fits.
fn mandate() -> String {
    let mandate = spy_mandate(&model());
    mandate.replace(r#""max_order_usd": "300""#, r#""max_order_usd": "1000""#)
}

/// What becomes of the calls the first run makes from its place on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Line {
    /// Every call reaches the server.
    Up,
    /// The place reaches the server, whose answer is lost, and every call after it is lost too,
    /// so the first run cannot find out what became of the order.
    DownAfterPlace,
    /// The place never reaches the server; nothing else is lost.
    PlaceNeverArrives,
}

/// The seam over the wire client, owning its server so the run's stages hold it.
struct Loopback {
    wire: RefCell<Wire>,
    line: Line,
    placed: Cell<bool>,
}

impl Tools for Loopback {
    async fn call_tool(
        &self,
        _class: CallClass,
        tool: &'static str,
        args: &serde_json::Value,
    ) -> Result<String, McpError> {
        let lost = match self.line {
            Line::Up => false,
            Line::DownAfterPlace => self.placed.get(),
            Line::PlaceNeverArrives => tool == PLACE,
        };
        self.placed.set(self.placed.get() || tool == PLACE);
        if lost {
            return Err(McpError::Network);
        }
        let reply = self.wire.borrow_mut().call_raw(tool, args.clone());
        let message: serde_json::Value =
            serde_json::from_str(&reply.ok_or(McpError::Network)?.body)
                .map_err(|_| McpError::Malformed)?;
        let result = message.get("result").ok_or(McpError::Malformed)?;
        Ok(result.to_string())
    }
}

/// The run's Robinhood connector and reconciler over one bridge, restored from `records`.
fn bridge(
    server: &Rc<SimServer>,
    line: Line,
    records: &[FoldedEvent],
) -> Rc<RobinhoodBridge<Loopback>> {
    let tools = Loopback {
        wire: RefCell::new(Wire::connect(&server.url().unwrap())),
        line,
        placed: Cell::new(false),
    };
    let robinhood = RobinhoodConnector::restore(tools, AGENTIC.to_owned(), records).unwrap();
    let runtime = Builder::new_current_thread().build().unwrap();
    RobinhoodBridge::new(runtime, robinhood)
}

/// The Robinhood connector, with the startup reconciliation's four reads answered as a flat,
/// active account by a scripted stand-in for C4's reconciler (DEC-928 item 3). The builder
/// still plans a bracket whatever the profile (B3), so the stand-in hands the connector the
/// entry without its legs, as Robinhood's profile places it; the journaled order is unchanged.
struct FlatReads {
    robinhood: RobinhoodShellConnector<Loopback>,
    account: BrokerAccount,
}

impl Connector for FlatReads {
    fn profile(&self) -> Result<mandate_domain::CapabilityProfile, mandate_domain::ProfileError> {
        robinhood_profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        match request {
            BrokerRequest::ListOpenOrders => Ok(BrokerOutcome::OpenOrders(Vec::new())),
            BrokerRequest::ListPositions => Ok(BrokerOutcome::Positions(Vec::new())),
            BrokerRequest::GetAccount => Ok(BrokerOutcome::Account(self.account.clone())),
            BrokerRequest::ListActivities { since } => Ok(BrokerOutcome::Activities {
                fills: Vec::new(),
                cursor: since.clone(),
            }),
            BrokerRequest::Submit(order) => {
                let unbracketed = SubmitOrder {
                    bracket: None,
                    ..order.clone()
                };
                self.robinhood.call(&BrokerRequest::Submit(unbracketed))
            }
            other => self.robinhood.call(other),
        }
    }
}

fn flat(bridge: &Rc<RobinhoodBridge<Loopback>>) -> Box<dyn Connector> {
    let fixture = include_bytes!("../../mandate-shell/tests/fixtures/tracer/alpaca/account.json");
    Box::new(FlatReads {
        robinhood: RobinhoodShellConnector::new(Rc::clone(bridge)),
        account: mandate_alpaca::wire::account(fixture).unwrap(),
    })
}

/// The simulated broker at the first run's clock: one agentic account beside one that is not
/// (DEC-875 item 1), SPY quoted at 655.20 in the regular session under the instrument id the run
/// sends as its `symbol` (the connector sends the instrument id as it is), and, when `lose`, the answer to
/// the next place lost.
fn broker(lose: bool) -> Rc<SimServer> {
    let mut sim = Sim::new(vec![
        holder(AGENTIC, true, false),
        holder(NOT_AGENTIC, false, false),
    ])
    .unwrap();
    sim.apply(Event::Quote(SPY.to_owned(), price("655.2")))
        .unwrap();
    sim.apply(Event::Session(Session::Regular)).unwrap();
    sim.apply(Event::Clock(at(FIRST))).unwrap();
    if lose {
        sim.apply(Event::Script(Fault::LoseAnswer)).unwrap();
    }
    Rc::new(SimServer::start(sim, Variant::Honest).unwrap())
}

/// The host's observation and its `Long` on twenty-five sessions of rising closes ending on the
/// last session completed at `now`, with the closes' bytes the adapter stores.
fn observed(now: &str) -> (Observation, ModelOutput, Vec<u8>) {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let last = calendar.last_completed_regular_session(at(now)).unwrap();
    let mut days = Vec::new();
    let mut day = Date::parse("2026-08-03").unwrap();
    while Some(day) <= last {
        days.extend(calendar.is_trading_day(day).unwrap().then_some(day));
        day = day.next().unwrap();
    }
    let closes: Vec<(Date, Px)> = (630..)
        .zip(&days[days.len() - 25..])
        .map(|(close, day)| (*day, Px::parse(&format!("{close}.2")).unwrap()))
        .collect();
    let rows = closes.iter().map(|(d, p)| format!(r#"["{d}","{p}"]"#));
    let rows = rows.collect::<Vec<_>>().join(",");
    let bytes = format!(r#"{{"closes":[{rows}],"instrument_id":"{SPY}"}}"#);
    let document = mandate_spec::Mandate::parse(&json(&mandate())).unwrap();
    let pinned = document.behavior.signal_models[0].clone();
    let entry = RegisteredModel {
        version: pinned.version.clone(),
        content_hash: pinned.content_hash,
        params: ["fast_periods", "slow_periods"].map(str::to_owned).into(),
        admits_instruments: false,
    };
    let registry = BTreeMap::from([(pinned.id.clone(), entry)]);
    let instrument_id = mandate_accounting::InstrumentId::new(SPY).unwrap();
    let pin = Pin {
        model: pinned,
        instrument_id: instrument_id.clone(),
    };
    let closes = DailyCloses {
        instrument_id: instrument_id.clone(),
        closes,
    };
    let evaluated = evaluate(&pin, &registry, &calendar, &closes, at(now));
    let Ok(Evaluation::Long(output)) = evaluated else {
        panic!("rising closes give the host's Long at {now}: {evaluated:?}");
    };
    let observation = Observation {
        source: SOURCE.to_owned(),
        instrument_id: Some(instrument_id),
        as_of: output.as_of,
        data_ref: Digest::of(bytes.as_bytes()),
    };
    (observation, *output, bytes.into_bytes())
}

/// The production stages for SPY's run at `now` over `connector`, and the run's setup.
fn stages(now: &str, connector: Box<dyn Connector>, store: Vec<u8>) -> (Stages, Setup) {
    let stream = Stream::of(
        &mandate(),
        &SNAPSHOT.replace("2026-09-21", "2026-09-25"),
        RULES,
        FEE,
        &model(),
    );
    let day = Date::parse(&now[..10]).unwrap();
    let artifacts = stream.artifacts_on(day).unwrap();
    let deployed = artifacts
        .deployment("ws_spy".into(), "agent_spy".into(), "acct_spy".into())
        .unwrap();
    let agent = deployed.deployment().agent.clone();
    let facts = spy_facts(SPY, "SPY", Exchange::Arca, at(now));
    let contexts = load_contexts(&artifacts, &facts, at(now), &agent).unwrap();
    let file = MANDATE_FILE.get_or_init(|| {
        let path = std::env::temp_dir().join(format!(
            "mandate-shell-rh-sim-restart-{}.json",
            std::process::id()
        ));
        fs::write(&path, to_canonical(&json(&mandate()))).unwrap();
        path
    });
    let store = BTreeMap::from([(Digest::of(&store), store)]);
    let sources = Sources {
        mandate: file.clone(),
        journal: None,
        recorded_at: at(now),
        agent,
        workspace: "ws_spy".to_owned(),
        account_ref: "acct_spy".to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: Some(Arc::new(store) as Arc<dyn ArtifactSource + Send + Sync>),
        transport: connector,
    };
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: "acct_spy".to_owned(),
        now: at(now),
        place_one_order: true,
        new_cycle: true,
    };
    (over(sources), setup)
}

/// One run at `now` over `connector`, on `journal` when it is a restart.
fn run(
    now: &str,
    connector: Box<dyn Connector>,
    reconciler: Option<Box<dyn mandate_shell::stages::Reconciler>>,
    journal: Option<Box<dyn mandate_shell::stages::JournalWriter>>,
) -> (Result<Report, ShellError>, Stages) {
    let (observation, output, bytes) = observed(now);
    let (mut stages, setup) = stages(now, connector, bytes);
    if let Some(reconciler) = reconciler {
        stages.reconciler = reconciler;
    }
    if let Some(journal) = journal {
        stages.journal = journal;
    }
    let outcome = run_observed(&mut stages, &setup, observation, output);
    (outcome, stages)
}

fn account_rows(stages: &Stages) -> Vec<StoredEvent> {
    stages.journal.read(ACCOUNT_STREAM).unwrap()
}

/// The account stream's rows as the connector's restore reads them.
fn folded(rows: &[StoredEvent]) -> Vec<FoldedEvent> {
    rows.iter()
        .map(|row| {
            let body = mandate_canon::parse(&row.body).unwrap();
            FoldedEvent {
                stream: row.stream_id.clone(),
                seq: Seq(row.seq),
                event_id: EventId(row.event_id.clone()),
                event_type: row.event_type.clone(),
                causation_id: body
                    .get("causation_id")
                    .and_then(Value::as_str)
                    .map(|id| EventId(id.to_owned())),
                payload: body.get("payload").cloned().unwrap(),
            }
        })
        .collect()
}

fn payload(row: &StoredEvent) -> Value {
    let body = mandate_canon::parse(&row.body).unwrap();
    body.get("payload").cloned().unwrap()
}

fn member<'v>(payload: &'v Value, name: &str) -> Option<&'v str> {
    payload.get(name).and_then(Value::as_str)
}

/// Each `OrderStateChanged` on the account stream as its state, its `broker_order_id` and its
/// schema version, in journal order.
fn states(rows: &[StoredEvent]) -> Vec<(String, Option<String>, u64)> {
    rows.iter()
        .filter(|row| row.event_type == "OrderStateChanged")
        .map(|row| {
            let body = mandate_canon::parse(&row.body).unwrap();
            let version = body.get("schema_version").and_then(Value::as_int).unwrap();
            let payload = payload(row);
            let state = member(&payload, "state").unwrap().to_owned();
            let id = member(&payload, "broker_order_id").map(str::to_owned);
            (state, id, version)
        })
        .collect()
}

fn submitted(rows: &[StoredEvent]) -> Vec<String> {
    rows.iter()
        .filter(|row| row.event_type == "OrderSubmitted")
        .map(|row| member(&payload(row), "client_order_id").unwrap().to_owned())
        .collect()
}

/// The server's call log, inside the allowlist (LT-8).
fn calls(server: &SimServer) -> Vec<String> {
    let calls = server.calls().unwrap();
    assert!(
        calls.iter().all(|c| ALLOWLIST.contains(&c.as_str())),
        "{calls:?}"
    );
    calls
}

fn count(calls: &[String], tool: &str) -> usize {
    calls.iter().filter(|call| call.as_str() == tool).count()
}

/// The first run: SPY placed through the shell's Robinhood connector over `line`, the order left
/// `Unknown` on the account stream, and the stages holding the journal the restart folds.
fn first_run(server: &Rc<SimServer>, line: Line) -> Stages {
    let first = bridge(server, line, &[]);
    let (outcome, stages) = run(FIRST, flat(&first), None, None);
    let rows = account_rows(&stages);
    let [order] = submitted(&rows)
        .try_into()
        .unwrap_or_else(|all: Vec<String>| {
            panic!("one OrderSubmitted, before the place: {all:?} ({outcome:?})")
        });
    assert!(
        states(&rows)
            .last()
            .is_some_and(|(state, ..)| state == "unknown"),
        "{order}: {:?} ({outcome:?})",
        states(&rows)
    );
    let placed = rows.iter().find(|row| row.event_type == "OrderSubmitted");
    assert_eq!(
        placed
            .map(payload)
            .as_ref()
            .and_then(|p| member(p, "risk_clock")),
        Some("2026-09-29T17:00:00.000000000Z"),
        "the order carries the run's clock, which its listing's created_at_gte is derived from"
    );
    stages
}

/// LT-6, DEC-529 item 4 and DEC-869: the restart's `Started` lists the agentic account's orders
/// once, before its startup reconciliation reads anything, finds the one order the lost answer
/// left, and journals its adoption as `OrderStateChanged` version 2 carrying the broker's
/// `order_id`. The broker holds one order and saw one place: nothing was sent again.
#[test]
fn an_unknown_order_is_adopted_on_the_restart_and_never_placed_again() {
    let server = broker(true);
    let first = first_run(&server, Line::DownAfterPlace);
    let before = calls(&server);
    assert_eq!(count(&before, PLACE), 1, "{before:?}");
    let held = server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap();
    let [order] = held.as_slice() else {
        panic!("the lost answer's order is the broker's only one: {held:?}")
    };

    let records = folded(&account_rows(&first));
    let restart = bridge(&server, Line::Up, &records);
    let connector = Box::new(RobinhoodShellConnector::new(Rc::clone(&restart)));
    let reconciler = Box::new(RobinhoodReconciler::new(restart));
    let (outcome, stages) = run(RESTART, connector, Some(reconciler), Some(first.journal));

    let Err(ShellError::Refused {
        stage: Stage::Reconcile,
        cause: Cause::Absent { what },
    }) = &outcome
    else {
        panic!("the restart stops at its reconciliation, DEC-928 item 3: {outcome:?}")
    };
    assert_eq!(*what, STATUS_UNPUBLISHED);
    let rows = account_rows(&stages);
    let adopted = states(&rows);
    assert_eq!(
        adopted.last(),
        Some(&("accepted".to_owned(), Some(order.id.clone()), 2)),
        "{adopted:?}"
    );
    let after: Vec<String> = calls(&server).split_off(before.len());
    assert_eq!(
        after.first().map(String::as_str),
        Some("get_accounts"),
        "{after:?}"
    );
    assert_eq!(
        after.get(1).map(String::as_str),
        Some(ORDERS),
        "the listing comes before every read of the reconciliation: {after:?}"
    );
    assert_eq!(count(&after, PLACE) + count(&after, REVIEW), 0, "{after:?}");
    assert_eq!(
        server
            .drive(|sim| sim.orders(AGENTIC).unwrap().len())
            .unwrap(),
        1
    );
    assert_eq!(submitted(&rows).len(), 1, "no second OrderSubmitted");
}

/// DEC-529 item 4 and trading spec §5.3 rule 9: a place that never reached the broker lists
/// nothing on the restart, so the order stays `Unknown`; the restart reconciles, runs its cycle
/// on the same rising closes, and places no opening in SPY while the order is `Unknown`.
#[test]
fn an_unknown_order_with_no_match_stays_unknown_and_blocks_a_new_opening() {
    let server = broker(false);
    let first = first_run(&server, Line::PlaceNeverArrives);
    let before = calls(&server);
    assert_eq!(count(&before, PLACE), 0, "{before:?}");
    assert!(
        server
            .drive(|sim| sim.orders(AGENTIC).unwrap().is_empty())
            .unwrap()
    );

    let records = folded(&account_rows(&first));
    let restart = bridge(&server, Line::Up, &records);
    let (outcome, stages) = run(RESTART, flat(&restart), None, Some(first.journal));

    let rows = account_rows(&stages);
    assert!(
        states(&rows)
            .last()
            .is_some_and(|(state, ..)| state == "unknown"),
        "{:?} ({outcome:?})",
        states(&rows)
    );
    let after: Vec<String> = calls(&server).split_off(before.len());
    assert!(
        count(&after, ORDERS) >= 1,
        "the restart listed: {after:?} {outcome:?}"
    );
    assert_eq!(count(&after, PLACE) + count(&after, REVIEW), 0, "{after:?}");
    assert!(
        server
            .drive(|sim| sim.orders(AGENTIC).unwrap().is_empty())
            .unwrap()
    );
    assert_eq!(
        submitted(&rows).len(),
        1,
        "no new OrderSubmitted: {outcome:?}"
    );
    let gates: Vec<(String, String)> = rows
        .iter()
        .filter(|row| row.event_type == "GateDecided")
        .map(|row| {
            let gate = payload(row);
            let verdict = member(&gate, "verdict").unwrap_or_default().to_owned();
            (
                verdict,
                member(&gate, "reason_code").unwrap_or_default().to_owned(),
            )
        })
        .collect();
    let held = ("deny".to_owned(), "unknown_order_in_flight".to_owned());
    assert_eq!(
        gates.last(),
        Some(&held),
        "the restart's opening reached the gate, which denied it for the Unknown order: {gates:?}"
    );
    if let Ok(report) = &outcome {
        assert!(report.submitted.is_empty(), "{report:?}");
    }
}
