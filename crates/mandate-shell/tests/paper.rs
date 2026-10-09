//! The shipping assembly end to end with no network: [`Artifacts::load`], the GET-only
//! [`preflight`], [`liquidity_facts`], [`load_contexts`], and `production`, in the binary's order,
//! over one scripted transport that answers both the trading and the data host from recorded
//! fixtures (DEC-466, DEC-470, DEC-471). `AlpacaPaperHttp` is never constructed and no credential is
//! read.
//!
//! What it proves: the preflight sends no `POST` and reads the trailing minute bars from the data
//! host rather than from a stored dataset; the run sends exactly one; the intent and the
//! submission are committed to the journal before that `POST` leaves; the order carries the live
//! quote's ask and the protection derived from it; and a restart over the same journal sends none,
//! whether the broker shows the order or not.

use std::cell::{Cell, RefCell};
use std::fs;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use mandate_alpaca::{
    BarsRequest, DataTransport, HttpRequest, Method, Pause, QuoteRequest, Response,
    TradingTransport, TransportError, alpaca_account_rules,
};
use mandate_canon::{DecStr, Value};
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_runtime::{AgentId, ConnectionId, Deployment, WorkspaceId};
use mandate_shell::adapters::{Sources, production};
use mandate_shell::paper::{Artifacts, PaperFacts, liquidity_facts, load_contexts, preflight};
use mandate_shell::stages::{JournalWriter, Stages};
use mandate_shell::{Cause, Report, Setup, ShellError, Stage, run};
use mandate_time::{Date, UtcNanos};

const NOW: &str = "2026-09-28T17:00:00Z";
const POST: &str = "POST /v2/orders";
const TEST_WORKSPACE: &str = "tracer";
const TEST_AGENT: &str = "tracer-aapl";
const TEST_CONNECTION: &str = "conn_alpaca_paper_01";
const TEST_ACCOUNT_REF: &str = "tracer-paper";
/// The one bars read the preflight may send at [`NOW`]: the five complete minutes 16:55 to 16:59.
const BARS: &str = "/v2/stocks/AAPL/bars?timeframe=1Min&start=2026-09-28T16:55:00Z&\
                    end=2026-09-28T16:59:00Z&limit=5&adjustment=raw&feed=iex&sort=asc";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tracer")
}

fn now() -> UtcNanos {
    UtcNanos::parse_rfc3339(NOW).unwrap()
}

/// A scratch directory under the system's temporary directory, removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path =
            std::env::temp_dir().join(format!("mandate-shell-paper-{name}-{}", std::process::id()));
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

fn bar(start: UtcNanos, close: &str, volume: &str) -> Bar {
    Bar {
        start,
        open: dec(close),
        high: dec(close),
        low: dec(close),
        close: dec(close),
        volume: dec(volume),
        vwap: dec(close),
        trade_count: 400_000,
    }
}

fn dataset(timeframe: &str) -> DatasetId {
    DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars(timeframe.parse().unwrap()),
        Symbol::parse("AAPL").unwrap(),
    )
    .unwrap()
}

/// Twenty-five daily closes rising a dollar a day to 255.20 on Friday 2026-09-25, the last session
/// completed at [`NOW`], so E4-2's baseline says `Long`; fifty million shares a day.
fn daily(root: &Path) -> PathBuf {
    let id = dataset("1Day");
    let store = Store::new(root);
    let mut day = Date::parse("2026-08-24").unwrap();
    for close in 231..256 {
        while day.is_weekend() {
            day = day.next().unwrap();
        }
        let start = UtcNanos::parse_rfc3339(&format!("{day}T04:00:00Z")).unwrap();
        let bars = vec![bar(start, &format!("{close}.20"), "50000000")];
        store.put_day(&id, day, &Records::Bars(bars)).unwrap();
        day = day.next().unwrap();
    }
    store.dataset_dir(&id)
}

/// What the scripted broker shows after the one submission.
#[derive(Clone, Copy, PartialEq, Eq)]
enum AfterPost {
    /// Nothing open: the order is not yet visible.
    Flat,
    /// The submitted order is open.
    Open,
}

/// What the scripted data host answers to the bars read.
#[derive(Clone, Copy, PartialEq, Eq)]
enum MinuteBars {
    /// The recorded five complete minutes.
    Recorded,
    /// One page with no bar: IEX printed none in the window.
    Empty,
}

/// One timeline every request and every journal batch is written to, in the order they happen.
#[derive(Default)]
struct Timeline {
    entries: Vec<String>,
    posted: Option<String>,
}

impl Timeline {
    fn posts(&self) -> usize {
        self.entries.iter().filter(|entry| *entry == POST).count()
    }

    fn first(&self, entry: &str) -> Option<usize> {
        self.entries.iter().position(|seen| seen == entry)
    }
}

/// The scripted transport for both hosts. It writes each request to the timeline before
/// `mandate-alpaca` interprets anything.
#[derive(Clone)]
struct Broker {
    after_post: AfterPost,
    minute_bars: MinuteBars,
    timeline: Rc<RefCell<Timeline>>,
}

impl Broker {
    fn file(name: &str) -> Vec<u8> {
        fs::read(fixtures().join("alpaca").join(name)).unwrap()
    }

    fn order_with(id: &str) -> String {
        String::from_utf8(Broker::file("order.json"))
            .unwrap()
            .replace("md-e144b97773a6f87c1978cc2831", id)
    }

    fn answer(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let path = request.path_and_query();
        let ok = |body: Vec<u8>| Ok(Response { status: 200, body });
        let posted = self.timeline.borrow().posted.clone();
        match (request.method(), path) {
            (Method::Get, "/v2/account") => ok(Broker::file("account.json")),
            (Method::Get, "/v2/positions") => ok(b"[]".to_vec()),
            (Method::Get, "/v2/assets/AAPL") => ok(Broker::file("asset-aapl.json")),
            (Method::Get, open) if open.starts_with("/v2/orders?") => {
                match (self.after_post, posted) {
                    (AfterPost::Open, Some(id)) => {
                        ok(format!("[{}]", Broker::order_with(&id)).into_bytes())
                    }
                    (AfterPost::Open | AfterPost::Flat, _) => ok(b"[]".to_vec()),
                }
            }
            (Method::Get, activities) if activities.starts_with("/v2/account/activities?") => {
                ok(b"[]".to_vec())
            }
            (Method::Get, lookup) if lookup.starts_with("/v2/orders:by_client_order_id") => {
                Ok(Response {
                    status: 404,
                    body: Broker::file("order-absent.json"),
                })
            }
            (Method::Post, "/v2/orders") => {
                let sent = mandate_canon::parse(request.body().unwrap().as_bytes()).unwrap();
                let id = sent
                    .get("client_order_id")
                    .and_then(Value::as_str)
                    .unwrap()
                    .to_owned();
                self.timeline.borrow_mut().posted = Some(id.clone());
                ok(Broker::order_with(&id).into_bytes())
            }
            (method, path) => panic!("the scripted broker has no answer for {method:?} {path}"),
        }
    }
}

impl TradingTransport for Broker {
    fn send(
        &self,
        request: &HttpRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        let method = request.method().as_str();
        let path = request.path_and_query();
        let entry = if request.method() == Method::Post && path == "/v2/orders" {
            POST.to_owned()
        } else {
            format!("{method} {path}")
        };
        self.timeline.borrow_mut().entries.push(entry);
        if let Some(body) = request.body() {
            self.timeline
                .borrow_mut()
                .entries
                .push(format!("BODY {body}"));
        }
        let answer = self.answer(request);
        async move { answer }
    }
}

impl DataTransport for Broker {
    fn send(
        &self,
        request: &QuoteRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        let path = request.path_and_query().to_owned();
        self.timeline
            .borrow_mut()
            .entries
            .push(format!("GET data {path}"));
        let answer = if path == "/v2/stocks/AAPL/quotes/latest?feed=iex" {
            Ok(Response {
                status: 200,
                body: Broker::file("quote-aapl.json"),
            })
        } else {
            panic!("the scripted data host has no answer for {path}")
        };
        async move { answer }
    }

    /// The recorded bars: five complete minutes, twenty thousand shares each.
    fn send_bars(
        &self,
        request: &BarsRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        let path = request.path_and_query().to_owned();
        self.timeline
            .borrow_mut()
            .entries
            .push(format!("GET data {path}"));
        let answer = if path == BARS {
            let body = match self.minute_bars {
                MinuteBars::Recorded => Broker::file("bars-aapl.json"),
                MinuteBars::Empty => {
                    br#"{"bars":[],"next_page_token":null,"symbol":"AAPL"}"#.to_vec()
                }
            };
            Ok(Response { status: 200, body })
        } else {
            panic!("the scripted data host has no answer for {path}")
        };
        async move { answer }
    }
}

/// A clock that stands still unless paused, so every age is judged at [`NOW`].
#[derive(Clone)]
struct Clock(Rc<Cell<UtcNanos>>);

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        self.0.get()
    }

    fn pause(&self, duration: Duration) -> impl Future<Output = ()> {
        let at = self.0.get();
        let secs = at.secs() + i64::try_from(duration.as_secs()).unwrap();
        self.0.set(UtcNanos::from_parts(secs, at.nanos()).unwrap());
        async {}
    }
}

struct Unreachable;

impl JournalWriter for Unreachable {
    fn take_ownership(&mut self, _stream: &str) -> Result<u64, Cause> {
        panic!("the placeholder journal is never used")
    }

    fn read(&self, _stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        panic!("the placeholder journal is never used")
    }

    fn append(&mut self, _: &str, _: u64, _: u64, _: &[Vec<u8>]) -> AppendOutcome {
        panic!("the placeholder journal is never used")
    }
}

/// Writes each committed batch's event types to the timeline once the journal has committed it.
struct Recording {
    inner: Box<dyn JournalWriter>,
    timeline: Rc<RefCell<Timeline>>,
}

impl JournalWriter for Recording {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        self.inner.take_ownership(stream)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        self.inner.read(stream)
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let outcome = self
            .inner
            .append(stream, expected_head, writer_epoch, drafts);
        if let AppendOutcome::Committed(rows) = &outcome {
            let types: Vec<&str> = rows.iter().map(|row| row.event_type.as_str()).collect();
            self.timeline
                .borrow_mut()
                .entries
                .push(format!("COMMITTED {}", types.join(",")));
        }
        outcome
    }
}

/// The binary's assembly, step for step, over `broker`, with the journal `journal` when a run is
/// restarted over an earlier one's.
fn assemble(
    broker: &Broker,
    daily: &Path,
    journal: Option<Box<dyn JournalWriter>>,
) -> Result<Stages, ShellError> {
    let mandate = fixtures().join("mandate.json");
    let artifacts = Artifacts::load(&mandate, &fixtures().join("config")).map_err(|cause| {
        ShellError::Refused {
            stage: Stage::Validate,
            cause,
        }
    })?;
    let clock = Clock(Rc::new(Cell::new(now())));
    let facts = preflight(&artifacts, broker.clone(), broker.clone(), clock).map_err(|cause| {
        ShellError::Refused {
            stage: Stage::Reconcile,
            cause,
        }
    })?;
    assert_eq!(
        facts.account_rules,
        alpaca_account_rules(),
        "the preflight carries the connector's declared account rules (DEC-840)"
    );
    let liquidity = liquidity_facts(
        artifacts.production_identity().symbol,
        daily,
        &facts.minute_bars,
        now(),
    )
    .map_err(|cause| ShellError::Refused {
        stage: Stage::MarketData,
        cause,
    })?;
    let agent = AgentId(TEST_AGENT.to_owned());
    let contexts = load_contexts(
        &artifacts,
        &PaperFacts {
            broker: facts,
            liquidity,
        },
        now(),
        &agent,
    )
    .map_err(|cause| ShellError::Refused {
        stage: Stage::Validate,
        cause,
    })?;
    let mut stages = production(Sources {
        mandate,
        dataset: daily.to_path_buf(),
        journal: None,
        recorded_at: now(),
        agent,
        workspace: TEST_WORKSPACE.to_owned(),
        account_ref: TEST_ACCOUNT_REF.to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: None,
        transport: broker.clone(),
    });
    let inner = match journal {
        Some(journal) => journal,
        None => std::mem::replace(&mut stages.journal, Box::new(Unreachable)),
    };
    stages.journal = Box::new(Recording {
        inner,
        timeline: Rc::clone(&broker.timeline),
    });
    Ok(stages)
}

fn setup() -> Setup {
    Setup {
        deployment: Deployment {
            agent: AgentId(TEST_AGENT.to_owned()),
            connection: ConnectionId(TEST_CONNECTION.to_owned()),
            workspace: WorkspaceId(TEST_WORKSPACE.to_owned()),
        },
        account_ref: TEST_ACCOUNT_REF.to_owned(),
        now: now(),
        place_one_order: true,
        new_cycle: false,
    }
}

fn refused(outcome: Result<Report, ShellError>) -> ShellError {
    match outcome {
        Err(error) => error,
        Ok(report) => panic!("the restart placed {report:?}"),
    }
}

/// The first run, which must place exactly one order; answers the stages for the restart.
fn first_run(broker: &Broker, daily: &Path) -> Stages {
    let mut stages = assemble(broker, daily, None).unwrap();
    let preflight: Vec<String> = broker.timeline.borrow().entries.clone();
    assert!(
        preflight.iter().all(|entry| entry.starts_with("GET ")),
        "the preflight reads with GETs only: {preflight:?}"
    );
    for read in [
        "GET /v2/account",
        "GET /v2/positions",
        "GET /v2/assets/AAPL",
        "GET data /v2/stocks/AAPL/quotes/latest?feed=iex",
        &format!("GET data {BARS}"),
    ] {
        assert!(preflight.iter().any(|entry| entry == read), "{read}");
    }
    assert!(
        preflight
            .iter()
            .any(|entry| entry.starts_with("GET /v2/orders?")),
        "the preflight reads the open orders"
    );

    let report = run(&mut stages, &setup()).unwrap();
    let timeline = broker.timeline.borrow();
    assert_eq!(timeline.posts(), 1, "{:?}", timeline.entries);
    assert_eq!(report.submitted.len(), 1);
    let post = timeline.first(POST).unwrap();
    let intent = timeline
        .entries
        .iter()
        .position(|entry| entry.starts_with("COMMITTED") && entry.contains("IntentProposed"))
        .expect("the intent is committed");
    let submission = timeline
        .entries
        .iter()
        .position(|entry| {
            entry.starts_with("COMMITTED") && entry.contains("OrderRequestRecorded,OrderSubmitted")
        })
        .expect("the request companion and the submission are committed together");
    assert!(
        intent < post && submission < post,
        "the intent and the submission are committed before the POST: {:?}",
        timeline.entries
    );
    let body = timeline
        .entries
        .get(post + 1)
        .and_then(|entry| entry.strip_prefix("BODY "))
        .unwrap();
    let sent = mandate_canon::parse(body.as_bytes()).unwrap();
    assert_eq!(
        sent.get("client_order_id").and_then(Value::as_str),
        report.submitted.first().map(String::as_str)
    );
    assert_eq!(
        sent.get("limit_price").and_then(Value::as_str),
        Some("255.2"),
        "the limit is the live IEX ask: {body}"
    );
    assert_eq!(
        sent.get("stop_loss")
            .and_then(|leg| leg.get("stop_price"))
            .and_then(Value::as_str),
        Some("242.44"),
        "the stop is 255.2 × 0.95: {body}"
    );
    assert_eq!(
        sent.get("take_profit")
            .and_then(|leg| leg.get("limit_price"))
            .and_then(Value::as_str),
        Some("280.72"),
        "the take-profit is 255.2 × 1.1: {body}"
    );
    drop(timeline);
    stages
}

#[test]
fn the_shipping_assembly_sends_one_post_after_the_journal_and_none_on_restart() {
    let scratch = Scratch::new("restart");
    let daily = daily(&scratch.0.join("daily"));
    let broker = Broker {
        after_post: AfterPost::Flat,
        minute_bars: MinuteBars::Recorded,
        timeline: Rc::default(),
    };
    let mut first = first_run(&broker, &daily);

    let journal = std::mem::replace(&mut first.journal, Box::new(Unreachable));
    let mut restarted = assemble(&broker, &daily, Some(journal)).unwrap();
    let error = refused(run(&mut restarted, &setup()));
    assert_eq!(error.code(), "cycle_already_open", "{error}");
    assert_eq!(
        broker.timeline.borrow().posts(),
        1,
        "the restart sends no POST"
    );
}

#[test]
fn a_restart_against_the_open_order_refuses_at_the_preflight_and_sends_nothing() {
    let scratch = Scratch::new("open-order");
    let daily = daily(&scratch.0.join("daily"));
    let broker = Broker {
        after_post: AfterPost::Open,
        minute_bars: MinuteBars::Recorded,
        timeline: Rc::default(),
    };
    let mut first = first_run(&broker, &daily);

    let journal = std::mem::replace(&mut first.journal, Box::new(Unreachable));
    let error = match assemble(&broker, &daily, Some(journal)) {
        Err(error) => error,
        Ok(_) => panic!("a broker holding an open order is not a clean first-order account"),
    };
    assert!(
        matches!(
            &error,
            ShellError::Refused {
                stage: Stage::Validate,
                cause: Cause::Absent {
                    what: "a paper account with no position and no open order"
                },
            }
        ),
        "{error}"
    );
    assert_eq!(broker.timeline.borrow().posts(), 1);
}

/// A window with no minute bar is no trailing volume the run can vouch for: the preflight refuses
/// before anything is assembled, and no default volume stands in for it (`AGENTS.md` rule 3).
#[test]
fn a_window_without_minute_bars_refuses_at_the_preflight_and_sends_nothing() {
    let scratch = Scratch::new("no-bars");
    let daily = daily(&scratch.0.join("daily"));
    let broker = Broker {
        after_post: AfterPost::Flat,
        minute_bars: MinuteBars::Empty,
        timeline: Rc::default(),
    };
    let error = match assemble(&broker, &daily, None) {
        Err(error) => error,
        Ok(_) => panic!("an empty bars answer is not a trailing volume"),
    };
    assert!(
        matches!(
            &error,
            ShellError::Refused {
                stage: Stage::Reconcile,
                cause: Cause::Absent {
                    what: "the trailing window's IEX minute bars"
                },
            }
        ),
        "{error}"
    );
    let timeline = broker.timeline.borrow();
    assert_eq!(timeline.posts(), 0);
    assert!(
        timeline
            .entries
            .iter()
            .all(|entry| entry.starts_with("GET ")),
        "{:?}",
        timeline.entries
    );
}
