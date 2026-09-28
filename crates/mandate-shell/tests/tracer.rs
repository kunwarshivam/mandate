//! The tracer end to end, through the production adapters, over recorded Alpaca paper fixtures and
//! a bar dataset written by `mandate-marketdata`'s own writer (task brief, "The CI end-to-end
//! test"). No test here touches a network: the transport is scripted, and `AlpacaPaperHttp` is never
//! constructed (ADR-0001 ES-19).
//!
//! Every test is pending on E7-7: each production adapter is a stub today, so each run stops at the
//! first probe with `Cause::Unimplemented`, and each test fails by that refusal propagating
//! (DEC-110, DEC-137). The implementation PR deletes the markers and changes nothing else here.
//!
//! The fail-closed suite is not here. It lives in `src/stages/fail_closed.rs`, because its
//! permissive doubles must not be reachable from a build that ships (task brief item 5).

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mandate_alpaca::{HttpRequest, Method, Response, TradingTransport, TransportError};
use mandate_canon::{DecStr, Digest, Value};
use mandate_journal::{Environment, StoredEvent, TrustedStart, verify_events};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_runtime::{AgentId, ConnectionId, Deployment, Proposal, SignalInputs, WorkspaceId};
use mandate_shell::adapters::{AlpacaConnector, BuilderPlan, Sources, SpecMandate, production};
use mandate_shell::stages::{MandateSource, Sizing, Stages};
use mandate_shell::{Report, Setup, ShellError, run};
use mandate_time::{Date, UtcNanos};

const AGENT: &str = "tracer-aapl";
const WORKSPACE: &str = "tracer";
const ACCOUNT_REF: &str = "tracer-paper";
const NOW: &str = "2026-09-25T20:00:00.000000000Z";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tracer")
}

fn agent_stream() -> String {
    format!("agent:{WORKSPACE}:{AGENT}")
}

fn account_stream() -> String {
    format!("acct:{WORKSPACE}:{ACCOUNT_REF}")
}

/// A scratch directory under the system's temporary directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path = std::env::temp_dir().join(format!(
            "mandate-shell-tracer-{name}-{}",
            std::process::id()
        ));
        fs::remove_dir_all(&path).ok();
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn dec(text: &str) -> DecStr {
    DecStr::parse(text).unwrap()
}

/// Writes one daily bar per weekday, starting 2026-08-21, with these closes, through
/// `mandate-marketdata`'s own writer, so the tracer reads the real format.
fn bars(dir: &Path, closes: &[&str]) -> PathBuf {
    let dataset = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars("1Day".parse().unwrap()),
        Symbol::parse("AAPL").unwrap(),
    )
    .unwrap();
    let store = Store::new(dir);
    let mut day = Date::parse("2026-08-21").unwrap();
    for close in closes {
        while day.is_weekend() {
            day = day.next().unwrap();
        }
        let bar = Bar {
            start: UtcNanos::parse_rfc3339(&format!("{day}T04:00:00Z")).unwrap(),
            open: dec(close),
            high: dec(close),
            low: dec(close),
            close: dec(close),
            volume: dec("50000000"),
            vwap: dec(close),
            trade_count: 400_000,
        };
        store
            .put_day(&dataset, day, &Records::Bars(vec![bar]))
            .unwrap();
        day = day.next().unwrap();
    }
    store.dataset_dir(&dataset)
}

/// Twenty-five closes rising one dollar a day to the fixture's 255.20, so the five-day average
/// is above the twenty-day one at the last close: E4-2's baseline says `Long`.
fn rising() -> Vec<String> {
    (0..25).map(|i| format!("{}.20", 231 + i)).collect()
}

fn falling() -> Vec<String> {
    (0..25).map(|i| format!("{}.20", 279 - i)).collect()
}

/// What the scripted broker answers, by endpoint rather than by position, so the test pins what
/// the tracer asks for without pinning the order the executor asks it in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Broker {
    /// A fresh paper account: active, flat, nothing open; a submission is accepted.
    Fresh,
    /// The broker already holds one share of AAPL.
    HoldsAapl,
    /// The submission times out, and the order query finds nothing.
    TimeoutThenAbsent,
}

#[derive(Default)]
struct Seen {
    requests: Vec<(Method, String, Option<String>)>,
}

impl Seen {
    fn posts(&self) -> usize {
        self.requests
            .iter()
            .filter(|(method, path, _)| *method == Method::Post && path == "/v2/orders")
            .count()
    }

    fn order_queries(&self) -> usize {
        self.requests
            .iter()
            .filter(|(_, path, _)| path.starts_with("/v2/orders:by_client_order_id"))
            .count()
    }
}

/// The scripted transport. It counts every `POST /v2/orders` it is handed before `mandate-alpaca`
/// interprets anything, which is the submission oracle the shell cannot reach.
#[derive(Clone)]
struct Scripted {
    broker: Broker,
    seen: Rc<RefCell<Seen>>,
}

impl Scripted {
    fn new(broker: Broker) -> Scripted {
        Scripted {
            broker,
            seen: Rc::default(),
        }
    }

    fn answer(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let path = request.path_and_query();
        let file = |name: &str| fs::read(fixtures().join("alpaca").join(name)).unwrap();
        let ok = |body: Vec<u8>| Ok(Response { status: 200, body });
        if path == "/v2/account" {
            return ok(file("account.json"));
        }
        if path == "/v2/positions" {
            return match self.broker {
                Broker::HoldsAapl => ok(file("position-aapl.json")),
                Broker::Fresh | Broker::TimeoutThenAbsent => ok(b"[]".to_vec()),
            };
        }
        if path.starts_with("/v2/orders?") {
            return ok(b"[]".to_vec());
        }
        if path == "/v2/orders" && request.method() == Method::Post {
            if self.broker == Broker::TimeoutThenAbsent {
                return Err(TransportError::Timeout);
            }
            let sent: Value = mandate_canon::parse(request.body().unwrap().as_bytes()).unwrap();
            let id = sent.get("client_order_id").and_then(Value::as_str).unwrap();
            let template = String::from_utf8(file("order.json")).unwrap();
            let body = template.replace("md-e144b97773a6f87c1978cc2831", id);
            return ok(body.into_bytes());
        }
        if path.starts_with("/v2/orders:by_client_order_id") {
            return Ok(Response {
                status: 404,
                body: file("order-absent.json"),
            });
        }
        panic!("the scripted broker has no answer for {path}");
    }
}

impl TradingTransport for Scripted {
    fn send(
        &self,
        request: &HttpRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        self.seen.borrow_mut().requests.push((
            request.method(),
            request.path_and_query().to_owned(),
            request.body().map(str::to_owned),
        ));
        let answer = self.answer(request);
        async move { answer }
    }
}

fn setup(place_one_order: bool) -> Setup {
    Setup {
        deployment: Deployment {
            agent: AgentId(AGENT.to_owned()),
            connection: ConnectionId("conn_alpaca_paper_01".to_owned()),
            workspace: WorkspaceId(WORKSPACE.to_owned()),
        },
        account_ref: ACCOUNT_REF.to_owned(),
        now: UtcNanos::parse(NOW).unwrap(),
        place_one_order,
        new_cycle: false,
    }
}

fn stages(mandate: &str, dataset: PathBuf, transport: Scripted) -> Stages {
    production(Sources {
        mandate: fixtures().join(mandate),
        dataset,
        journal: None,
        agent: AgentId(AGENT.to_owned()),
        transport,
    })
}

fn committed(stages: &Stages, stream: &str) -> Vec<StoredEvent> {
    stages.journal.read(stream).unwrap()
}

fn of_type(rows: &[StoredEvent], event_type: &str) -> Vec<Value> {
    rows.iter()
        .map(|row| mandate_canon::parse(&row.body).unwrap())
        .filter(|body| body.get("event_type").and_then(Value::as_str) == Some(event_type))
        .collect()
}

fn payload_field(body: &Value, name: &str) -> Option<String> {
    body.get("payload")
        .and_then(|payload| payload.get(name))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// The run's refusal, or a failure naming what the run did instead.
fn refused(outcome: Result<Report, ShellError>) -> ShellError {
    match outcome {
        Err(error) => error,
        Ok(report) => panic!("the run placed {report:?}"),
    }
}

/// Asserts the refusal's code, printing the refusal itself, so a run that stopped somewhere else
/// shows where and why.
fn assert_refused(outcome: Result<Report, ShellError>, code: &str) {
    let error = refused(outcome);
    assert_eq!(error.code(), code, "{error}");
}

fn stream_bytes(stages: &Stages) -> Vec<u8> {
    let mut bytes = Vec::new();
    for stream in [agent_stream(), account_stream()] {
        for row in committed(stages, &stream) {
            bytes.extend_from_slice(&row.body);
        }
    }
    bytes
}

/// Step 1: the fixture mandate validates with no violation before anything else uses it.
#[test]
#[ignore = "pending E7-7"]
fn the_fixture_mandate_validates_with_no_violation() {
    let admitted = SpecMandate {
        path: fixtures().join("mandate.json"),
    }
    .admitted()
    .unwrap();
    assert_eq!(admitted.environment, Environment::Paper);
    assert_eq!(admitted.model.id, "quant.ma_crossover");
    assert_eq!(admitted.model.version, "1.0.0");
    assert_eq!(admitted.view.working_universe.len(), 1);
}

/// Step 5, by hand (`generate.py` asserts the same against `ref.py`): the cap is
/// min(1000, 1 × 1000) = 1000, one fresh output of conviction 1 and confidence 1 gives a buy
/// conviction of 1 and a target of 1000, the budget is min(1000, 300, 1000, 1000) = 300, and
/// trunc(300 / 255.20) = 1 share, whose 255.20 clears the 50 band (0.05 × 1000).
#[test]
#[ignore = "pending E7-7"]
fn the_fixture_sizes_to_exactly_one_share() {
    let admitted = SpecMandate {
        path: fixtures().join("mandate.json"),
    }
    .admitted()
    .unwrap();
    let now = mandate_runtime::RiskClock::from_secs(UtcNanos::parse(NOW).unwrap().secs());
    let instrument = admitted.view.working_universe.first().unwrap().clone();
    let output = mandate_shell::map::model_output(
        mandate_backtest::Signal::Long,
        &admitted.model,
        &instrument,
        now,
    )
    .unwrap();
    let inputs = SignalInputs {
        outputs: BTreeMap::from([(output.model.clone(), BTreeMap::from([(instrument, output)]))]),
        now,
    };
    let proposal: Proposal = BuilderPlan.size(&admitted.view, &inputs).unwrap().unwrap();
    assert_eq!(proposal.qty.to_string(), "1");
    assert_eq!(proposal.limit.to_string(), "255.2");
}

/// The whole path: one order, its intent journaled before it was sent, both streams verifying,
/// every draft paper, and no account number or credential anywhere in the record (TI-1, TI-7,
/// TI-8).
#[test]
#[ignore = "pending E7-7"]
fn happy() {
    let scratch = Scratch::new("happy");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    let report = run(&mut stages, &setup(true)).unwrap();
    assert_eq!(seen.borrow().posts(), 1);
    assert_eq!(report.submitted.len(), 1);

    let agent = committed(&stages, &agent_stream());
    let account = committed(&stages, &account_stream());
    let intents = of_type(&agent, "IntentProposed");
    assert_eq!(intents.len(), 1);
    let submitted = of_type(&account, "OrderSubmitted");
    assert_eq!(submitted.len(), 1);
    let client_order_id = payload_field(&submitted[0], "client_order_id").unwrap();
    assert_eq!(report.submitted, std::slice::from_ref(&client_order_id));
    let posted = seen
        .borrow()
        .requests
        .iter()
        .find_map(|(method, _, body)| (*method == Method::Post).then(|| body.clone().unwrap()));
    assert!(posted.unwrap().contains(&client_order_id));

    for rows in [&agent, &account] {
        verify_events(
            rows,
            TrustedStart::GENESIS,
            &BTreeMap::<Digest, Vec<u8>>::new(),
        )
        .unwrap();
        for row in rows.iter() {
            assert_eq!(row.environment, "paper");
        }
    }
    let written = String::from_utf8(stream_bytes(&stages)).unwrap();
    for secret in [
        "pii:account_number",
        "pii:account_id",
        "account_number",
        "PKSENTINEL",
    ] {
        assert!(!written.contains(secret), "{secret} reached the journal");
    }
}

/// TI-10: the same fixtures, mandate, and clock journal byte-identical drafts.
#[test]
#[ignore = "pending E7-7"]
fn happy_is_deterministic() {
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let mut journals = Vec::new();
    for name in ["deterministic-a", "deterministic-b"] {
        let scratch = Scratch::new(name);
        let dataset = bars(&scratch.0, &closes);
        let mut stages = stages("mandate.json", dataset, Scripted::new(Broker::Fresh));
        run(&mut stages, &setup(true)).unwrap();
        journals.push(stream_bytes(&stages));
    }
    assert!(!journals[0].is_empty());
    assert_eq!(journals[0], journals[1]);
}

/// A planning run sends nothing and reports the order it would place.
#[test]
#[ignore = "pending E7-7"]
fn a_planning_run_sends_nothing() {
    let scratch = Scratch::new("planning");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    let report = run(&mut stages, &setup(false)).unwrap();
    assert_eq!(seen.borrow().posts(), 0);
    assert_eq!(report.would_place.unwrap().qty.to_string(), "1");
}

/// TI-9, PB-6: the mandate classifies the opening ASK, and the tracer has no escalation: the
/// request is journaled and nothing is sent.
#[test]
#[ignore = "pending E7-7"]
fn autonomy_ask() {
    let scratch = Scratch::new("ask");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate-ask.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "autonomy_not_auto");
    assert_eq!(seen.borrow().posts(), 0);
    let agent = committed(&stages, &agent_stream());
    assert_eq!(of_type(&agent, "ApprovalRequested").len(), 1);
    assert_eq!(of_type(&agent, "IntentProposed").len(), 0);
}

/// PB-7: falling closes put the five-day average below the twenty-day one; flat opens nothing.
#[test]
#[ignore = "pending E7-7"]
fn signal_flat() {
    let scratch = Scratch::new("flat");
    let closes = falling();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "no_long_signal");
    assert_eq!(seen.borrow().posts(), 0);
}

/// PB-7: ten closes are fewer than the twenty the slow window needs, so the signal is undecided,
/// never a buy.
#[test]
#[ignore = "pending E7-7"]
fn signal_undecided() {
    let scratch = Scratch::new("undecided");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().skip(15).map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "no_long_signal");
    assert_eq!(seen.borrow().posts(), 0);
}

/// A mandate whose order cap is below one share: the builder holds, so nothing is proposed and
/// nothing is sent. A cap is a limit, which the shell never judges (PB-14).
#[test]
#[ignore = "pending E7-7"]
fn oversized_proposal() {
    let scratch = Scratch::new("oversized");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate-small-orders.json", dataset, transport);
    let report = run(&mut stages, &setup(true)).unwrap();
    assert!(report.submitted.is_empty());
    assert_eq!(seen.borrow().posts(), 0);
    assert_eq!(
        of_type(&committed(&stages, &agent_stream()), "IntentProposed").len(),
        0
    );
}

/// PB-15: one close ten times the others is coverage the market-data stage cannot trust.
#[test]
#[ignore = "pending E7-7"]
fn outlier_close() {
    let scratch = Scratch::new("outlier");
    let mut closes = rising();
    closes[24] = "2552.00".to_owned();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "market_data_untrusted");
    assert_eq!(seen.borrow().posts(), 0);
}

/// TI-6, PB-3: a second run over the same journal sends nothing further; across both runs there is
/// exactly one submission and one intent, and the broker's duplicate check is never needed.
#[test]
#[ignore = "pending E7-7"]
fn duplicate_after_restart() {
    let scratch = Scratch::new("restart");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    run(&mut stages, &setup(true)).unwrap();
    assert_refused(run(&mut stages, &setup(true)), "cycle_already_open");
    assert_eq!(seen.borrow().posts(), 1);
    assert_eq!(
        of_type(&committed(&stages, &agent_stream()), "IntentProposed").len(),
        1
    );
}

/// TI-12, PB-16: a fresh journal against a broker that already holds the position is a
/// reconciliation mismatch, never a clean start.
#[test]
#[ignore = "pending E7-7"]
fn fresh_journal_with_broker_position() {
    let scratch = Scratch::new("fresh-journal");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::HoldsAapl);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    assert_refused(run(&mut stages, &setup(true)), "reconciliation_mismatch");
    assert_eq!(seen.borrow().posts(), 0);
}

/// PB-11: the submission times out, the query finds nothing, and one absence never resubmits.
#[test]
#[ignore = "pending E7-7"]
fn broker_unknown_then_absent() {
    let scratch = Scratch::new("unknown");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let transport = Scripted::new(Broker::TimeoutThenAbsent);
    let seen = Rc::clone(&transport.seen);
    let mut stages = stages("mandate.json", dataset, transport);
    let report = run(&mut stages, &setup(true)).unwrap();
    assert_eq!(report.submitted.len(), 1);
    assert_eq!(seen.borrow().posts(), 1);
    assert!(seen.borrow().order_queries() >= 1);
}

/// PB-12: after a mismatch the agent stays paused and alerted, and nothing in the tracer lifts it:
/// its agent stream's last mode is `paused`.
#[test]
#[ignore = "pending E7-7"]
fn reconcile_mismatch_pauses() {
    let scratch = Scratch::new("mismatch");
    let closes = rising();
    let closes: Vec<&str> = closes.iter().map(String::as_str).collect();
    let dataset = bars(&scratch.0, &closes);
    let mut stages = stages("mandate.json", dataset, Scripted::new(Broker::HoldsAapl));
    let error = refused(run(&mut stages, &setup(true)));
    assert_eq!(error.code(), "reconciliation_mismatch", "{error}");
    let modes: Vec<String> = of_type(&committed(&stages, &agent_stream()), "AgentModeChanged")
        .iter()
        .filter_map(|body| payload_field(body, "to"))
        .collect();
    assert_eq!(modes.last().map(String::as_str), Some("paused"));
}

/// The connector type the manual run uses is the one CI drives, over any transport.
#[test]
fn the_connector_is_the_alpaca_client_over_the_given_transport() {
    let transport = Scripted::new(Broker::Fresh);
    let seen = Rc::clone(&transport.seen);
    let mut connector = AlpacaConnector { transport };
    let answer = mandate_shell::stages::Connector::call(
        &mut connector,
        &mandate_executor::BrokerRequest::GetAccount,
    );
    assert!(answer.is_ok(), "{answer:?}");
    assert_eq!(seen.borrow().requests.len(), 1);
}
