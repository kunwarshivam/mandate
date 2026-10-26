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
//! Every run reconciles through the Robinhood reconciler over the same bridge (C4, DEC-933),
//! reading the simulator's own account. Every order request goes to the shell's Robinhood
//! connector over loopback; the only stand-in drops an entry's bracket legs, which the builder
//! still plans whatever the profile (B3). Oracles: the simulator's own orders and call log, and
//! the account stream's own records.

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
use common::{AGENTIC, NOT_AGENTIC, account as holder, price, tickers};
use mandate_alpaca::Exchange;
use mandate_canon::{Digest, Value, to_canonical};
use mandate_executor::{
    AccountRef, AccountScope, BrokerOutcome, BrokerRequest, ConnectorError, EventId, FoldedEvent,
    Seq, SubmitOrder, WorkspaceId,
};
use mandate_journal::{ArtifactSource, StoredEvent};
use mandate_mcp::{ALLOWLIST, CallClass, McpError};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_num::Price as Px;
use mandate_rh_sim::{Event, Fault, Session, Sim, SimServer, Variant};
use mandate_robinhood::{RobinhoodConnector, Tools, robinhood_profile};
use mandate_runtime::{ModelOutput, Observation};
use mandate_shell::adapters::{CoreExecutor, Sources, over};
use mandate_shell::paper::load_contexts;
use mandate_shell::robinhood::{RobinhoodBridge, RobinhoodReconciler, RobinhoodShellConnector};
use mandate_shell::stages::{Connector, Stages};
use mandate_shell::symbols::BrokerSymbols;
use mandate_shell::{Report, Setup, ShellError, run_observed};
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
const AGENT_STREAM: &str = "agent:ws_spy:agent_spy";

/// The confirmed document as the file the run's mandate stage reads, written once per process so
/// parallel test threads never read it half written.
static MANDATE_FILE: OnceLock<PathBuf> = OnceLock::new();

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// SPY's mandate with its order cap raised to the allocation, so one share at 655.20 fits, and
/// the stop-limit offset V-008 asks for on a profile that protects equities with a stop-limit.
fn mandate() -> String {
    let mandate = spy_mandate(&model());
    mandate
        .replace(r#""max_order_usd": "300""#, r#""max_order_usd": "1000""#)
        .replace(
            r#""crypto_stop_limit_offset": null"#,
            r#""crypto_stop_limit_offset": "0.01""#,
        )
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
    let robinhood =
        RobinhoodConnector::restore(tools, AGENTIC.to_owned(), tickers(), records).unwrap();
    let runtime = Builder::new_current_thread().build().unwrap();
    RobinhoodBridge::new(runtime, robinhood)
}

/// The Robinhood connector, handing it each entry without its bracket legs: the builder still
/// plans a bracket whatever the profile (B3), and Robinhood's profile places the entry alone.
/// The journaled order is unchanged; every other request goes to the connector as it is.
struct Unbracketed {
    robinhood: RobinhoodShellConnector<Loopback>,
}

impl Connector for Unbracketed {
    fn profile(&self) -> Result<mandate_domain::CapabilityProfile, mandate_domain::ProfileError> {
        robinhood_profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        match request {
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

/// The simulated broker at the first run's clock: one agentic account beside one that is not
/// (DEC-875 item 1), SPY quoted at 655.20 in the regular session under its ticker, which the
/// connector sends for the run's asset id (DEC-956 item 1), and, when `lose`, the answer to the
/// next place lost.
fn broker(lose: bool) -> Rc<SimServer> {
    let mut sim = Sim::new(vec![
        holder(AGENTIC, true, false),
        holder(NOT_AGENTIC, false, false),
    ])
    .unwrap();
    sim.apply(Event::Quote("SPY".to_owned(), price("655.2")))
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

/// The production stages for SPY's run at `now` over `bridge`'s connector, with the Robinhood
/// reconciler over the same bridge in place of the executor's own (C4, DEC-933), and the run's
/// setup.
fn stages(now: &str, bridge: &Rc<RobinhoodBridge<Loopback>>, store: Vec<u8>) -> (Stages, Setup) {
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
    let contexts = load_contexts(
        &artifacts,
        &facts,
        at(now),
        &agent,
        mandate_domain::Environment::Paper,
    )
    .unwrap();
    let reconciled = load_contexts(
        &artifacts,
        &facts,
        at(now),
        &agent,
        mandate_domain::Environment::Paper,
    )
    .unwrap();
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
        content_store: None,
        symbols: BrokerSymbols::default(),
        transport: Box::new(Unbracketed {
            robinhood: RobinhoodShellConnector::new(Rc::clone(bridge)),
        }) as Box<dyn Connector>,
    };
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: "acct_spy".to_owned(),
        now: at(now),
        place_one_order: true,
        new_cycle: true,
        connection_environment: mandate_domain::Environment::Paper,
    };
    let mut stages = over(sources);
    let scope = AccountScope {
        account: AccountRef("acct_spy".to_owned()),
        workspace: WorkspaceId("ws_spy".to_owned()),
    };
    let (executor, reconciler) = CoreExecutor::pair(scope, Some(reconciled.executor));
    stages.executor = Box::new(executor);
    stages.reconciler = Box::new(RobinhoodReconciler::new(Rc::clone(bridge), reconciler));
    (stages, setup)
}

/// One run at `now` over `bridge`, on `journal` when it is a restart.
fn run(
    now: &str,
    bridge: &Rc<RobinhoodBridge<Loopback>>,
    journal: Option<Box<dyn mandate_shell::stages::JournalWriter>>,
) -> (Result<Report, ShellError>, Stages) {
    let (observation, output, bytes) = observed(now);
    let (mut stages, setup) = stages(now, bridge, bytes);
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
    let (outcome, stages) = run(FIRST, &first, None);
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
/// `order_id`. The broker holds one order and saw one place: nothing was sent again. The
/// reconciliation that follows reads the broker's working order, whose record carries no key of
/// ours (U-R2), and stops the restart before its cycle, fail closed: the order is marked
/// `Unknown` again and the executor drafts `ExternalActivityIngested` for the order it cannot match,
/// and the reconciliation's difference stops the restart (DEC-956 item 5). Matching it by its
/// journaled broker id is
/// DEC-956 item 2's proposal, for the founder. The run is refused on such an account anyway
/// (DEC-529 item 3, DEC-470 item 1).
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
    assert_eq!(
        order.symbol, "SPY",
        "Robinhood was sent the registered ticker, never the asset id (DEC-956 item 1)"
    );

    let records = folded(&account_rows(&first));
    let restart = bridge(&server, Line::Up, &records);
    let (outcome, stages) = run(RESTART, &restart, Some(first.journal));

    assert!(
        outcome.is_err(),
        "the broker's record carries no key of ours, so the reconciliation that follows the \
         adoption stops the restart before its cycle: {outcome:?}"
    );
    let rows = account_rows(&stages);
    let adopted = states(&rows);
    let adoption = ("accepted".to_owned(), Some(order.id.clone()), 2);
    assert_eq!(
        adopted.get(1),
        Some(&adoption),
        "the first run's `unknown`, then the restart's adoption: {adopted:?}"
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

/// DEC-529 item 4 and trading spec §5.3 rules 6 and 9: a place that never reached the broker lists
/// nothing on the restart, so the order stays `Unknown`; the restart reconciles, runs its cycle
/// on the same rising closes, and places no opening in SPY while the order is `Unknown`: the
/// runtime's dry run holds it, since the first opening still works (DEC-1150 item 5).
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
    let (outcome, stages) = run(RESTART, &restart, Some(first.journal));

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
    let decided: Vec<(String, String)> = stages
        .journal
        .read(AGENT_STREAM)
        .unwrap()
        .iter()
        .filter(|row| row.event_type == "DecisionMade")
        .map(|row| {
            let decision = payload(row);
            (
                member(&decision, "dry_run").unwrap_or_default().to_owned(),
                member(&decision, "reason_code")
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect();
    assert!(
        decided
            .iter()
            .all(|(_, reason)| reason != "working_order_limit"),
        "the restart's opening is held while the Unknown one works, before any decision \
         (trading spec §5.3 rule 6, DEC-2843): {decided:?}"
    );
    assert_eq!(
        gates.len(),
        1,
        "so it never reaches the gate, which decided only the first run's opening: {gates:?}"
    );
    if let Ok(report) = &outcome {
        assert!(report.submitted.is_empty(), "{report:?}");
    }
}

/// The close window's start on the first run's day, 15:50 in New York: the watch's bound.
const BOUND: &str = "2026-09-29T19:50:00Z";
/// The price the simulator fills the entry at, its quote.
const FILLED_AT: &str = "655.2";
/// More pauses than the watch below needs, so a watch that never ends fails at once.
const PAUSE_BUDGET: usize = 40;

/// The watch's clock, in whole seconds, which fills the entry at the broker on its first pause:
/// the broker's word on the fill is then its order record and its position alone (U-R8).
struct FillingClock {
    server: Rc<SimServer>,
    secs: Cell<i64>,
    pauses: Cell<usize>,
}

impl mandate_alpaca::Pause for FillingClock {
    fn now(&self) -> UtcNanos {
        UtcNanos::from_parts(self.secs.get(), 0).unwrap()
    }

    async fn pause(&self, duration: std::time::Duration) {
        let taken = self.pauses.get();
        assert!(taken < PAUSE_BUDGET, "the watch never ended");
        if taken == 0 {
            let [entry] = self
                .server
                .drive(|sim| sim.orders(AGENTIC))
                .unwrap()
                .unwrap()
                .try_into()
                .unwrap_or_else(|all: Vec<mandate_rh_sim::Order>| {
                    panic!("the entry alone rests at the first pause: {all:?}")
                });
            self.server
                .drive(|sim| sim.fill(&entry.id, entry.quantity, price(FILLED_AT)))
                .unwrap()
                .unwrap();
        }
        self.pauses.set(taken + 1);
        let step = i64::try_from(duration.as_secs()).unwrap();
        self.secs.set(self.secs.get() + step);
    }
}

/// R0's R19 and R20 (DEC-1151): the entry fills at the broker while the run watches it, and
/// Robinhood reports no fill. The watch adopts it `filled` from its order record, the fill is
/// booked from that record's filled quantity at the position's average price, under the id the
/// record derives, so the reconciliation finds the position explained and pauses nothing; and
/// the protective stop is sent and rests at the broker, since the one-order rule counts only an
/// order that adds risk (`AGENTS.md` rule 13).
#[test]
fn a_watched_entry_filled_at_the_broker_is_booked_from_its_record_and_protected() {
    let server = broker(false);
    let bridge = bridge(&server, Line::Up, &[]);
    let (observation, output, bytes) = observed(FIRST);
    let (mut stages, setup) = stages(FIRST, &bridge, bytes);
    let watch = mandate_shell::Watch {
        pause: FillingClock {
            server: Rc::clone(&server),
            secs: Cell::new(at(FIRST).secs()),
            pauses: Cell::new(0),
        },
        bound: at(BOUND),
        interval: std::time::Duration::from_secs(5),
    };
    let outcome =
        mandate_shell::run_observed_watched(&mut stages, &setup, observation, output, &watch);
    let rows = account_rows(&stages);
    let [entry] = submitted(&rows)
        .first()
        .cloned()
        .map(|entry| [entry])
        .unwrap_or_else(|| panic!("the entry is submitted: {outcome:?}"));
    let fills: Vec<(String, Value)> = rows
        .iter()
        .filter(|row| row.event_type.ends_with("FillApplied"))
        .map(|row| (row.event_type.clone(), payload(row)))
        .collect();
    let [(_, fill)] = fills.as_slice() else {
        panic!("one fill is booked: {fills:?} ({outcome:?})")
    };
    assert_eq!(
        ["fill_id", "client_order_id", "side", "qty_gross", "price"].map(|m| member(fill, m)),
        [
            Some(format!("record:{entry}:1").as_str()),
            Some(entry.as_str()),
            Some("buy"),
            Some("1"),
            Some(FILLED_AT),
        ],
        "the record's filled quantity at the position's average price"
    );
    let mismatched: Vec<Value> = rows
        .iter()
        .filter(|row| row.event_type == "BrokerPositionObserved")
        .map(payload)
        .filter(|observed| observed.get("mismatch") == Some(&Value::Bool(true)))
        .collect();
    assert!(mismatched.is_empty(), "{mismatched:?}");
    let held = server.drive(|sim| sim.orders(AGENTIC)).unwrap().unwrap();
    let stops: Vec<_> = held
        .iter()
        .filter(|order| order.order_type == mandate_rh_sim::OrderType::StopLimit)
        .map(|order| (order.side, order.quantity, order.time_in_force, order.state))
        .collect();
    assert_eq!(
        stops,
        [(
            mandate_rh_sim::Side::Sell,
            mandate_num::Qty::parse("1").unwrap(),
            mandate_rh_sim::TimeInForce::Gtc,
            mandate_rh_sim::State::Confirmed,
        )],
        "the protective stop rests at the broker: {outcome:?}"
    );
    let report = outcome.unwrap_or_else(|stop| panic!("the run ends cleanly: {stop}"));
    assert_eq!(report.submitted.len(), 2, "the entry, then its stop");
}
