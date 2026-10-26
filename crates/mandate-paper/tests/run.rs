//! The paper adapter's one run (E7-19 slice 5, E1a, DEC-846): SPY deployed as the shell's tests
//! deploy it, a real store and bars, a scripted broker, and no network (ES-19, rule 8). One
//! test places its order through a real Postgres journal, the suite's PG class — it skips
//! unless `MANDATE_PG_URL` is set, as `tests/binary.rs`'s Postgres test does (DEC-109).

/// The shell's shared test fixtures (the deployment the `Scene` deploys), under `shell_common`
/// here so the journal trio below can include `mandate-journal`'s fixtures as `common`, the
/// name its conformance suite's own `super::common` reads. `tests/binary.rs` includes the same
/// trio at its crate root, with `common` free for the journal's.
#[path = "../../mandate-shell/tests/common/mod.rs"]
mod shell_common;

#[path = "../../mandate-journal/tests/common/mod.rs"]
mod common;
#[path = "../../mandate-journal/tests/conformance/mod.rs"]
#[allow(
    unused_imports,
    unused_macros,
    reason = "only `support`'s backend trait is used here; the suite runs in `mandate-journal-pg`"
)]
mod conformance;
#[path = "../../mandate-journal-pg/tests/support/mod.rs"]
mod support;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{BarsRequest, DataTransport, HttpRequest, Method, Pause, QuoteRequest};
use mandate_alpaca::{Response, TradingTransport, TransportError};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{DecStr, Value, to_canonical};
use mandate_domain::Environment;
use mandate_journal::{ArtifactRef, ArtifactStore, StoredEvent, StreamId, get_artifact};
use mandate_journal::{TrustedStart, verify_events};
use mandate_journal_pg::APP_ROLE;
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_modelhost::Signal;
use mandate_paper::{
    Args, Broker as Chosen, Connection, ConnectionRefusal, Dial, Evidence, Outcome, PaperError,
    Ports, Reconciled, lines, run, stderr_line,
};
use mandate_shell::control::{ConfigRefusal, ControlRecord, DeploymentRefusal, Origin};
use mandate_shell::paper::daily_closes;
use mandate_shell::{Cause, ShellError};
use mandate_time::{Date, ExchangeCalendar, UtcNanos};
use proptest::prelude::*;
use shell_common::{AAPL, FEE, RULES, SNAPSHOT, SPY, Stream, json, model, spy_facts, spy_mandate};
use std::cell::{Cell, RefCell};
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// A run clock, and the minute the recorded quote and minute bars are moved to so they are fresh.
type Moment = (&'static str, &'static str);
/// Tuesday 13:00 in New York: Monday 2026-09-28 is the last completed session.
const TUESDAY: Moment = ("2026-09-29T17:00:00Z", "2026-09-29T16:5");
/// Tuesday 15:50 in New York, the first instant of the closing window (trading spec §9.6).
const CLOSING: Moment = ("2026-09-29T19:50:00Z", "2026-09-29T19:4");
/// Tuesday 15:40 in New York, ten minutes before the close window opens at 15:50.
const LATE: Moment = ("2026-09-29T19:40:00Z", "2026-09-29T19:3");
/// Wednesday 13:00 in New York: Tuesday is the last completed session, so bars through Monday
/// are stale.
const WEDNESDAY: Moment = ("2026-09-30T17:00:00Z", "2026-09-30T16:5");

/// A deployed equity: its asset id, symbol, the snapshot's listing, and the broker's.
#[derive(Clone, Copy)]
struct Equity(&'static str, &'static str, &'static str, &'static str);
const SPY_ARCA: Equity = Equity(SPY, "SPY", "arca", "ARCA");
const AAPL_NASDAQ: Equity = Equity(AAPL, "AAPL", "nasdaq", "NASDAQ");
const ALPACA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../mandate-shell/tests/fixtures/"
);

type Answer = Result<Response, TransportError>;

/// A paper broker whose answers are `mandate-shell`'s recorded AAPL ones moved to `equity` and
/// to the run's minute. It accepts a submission and keeps every `POST /v2/orders` body it is
/// handed; `holds` makes it report one share already held, as after a first run. It reads the
/// entry back `filled` (DEC-858 item 6), or with `rests` `accepted` until deleted and then
/// `canceled`, and logs every request after the entry with the clock's second. With
/// `quote_offset_ns` its quote is stamped that many nanoseconds after the clock's instant at the
/// quote read, as a live feed stamps it, instead of the recorded minute, and the minute bars drop
/// their first, which a clock past the run's whole minute no longer asks for.
#[derive(Clone)]
struct Broker {
    equity: Equity,
    quote_offset_ns: Option<i128>,
    holds: bool,
    rests: bool,
    /// The entry's fill, at the entry's own limit price, in the account activities once the entry
    /// was sent, and the share it bought in the positions, named by the broker's ticker as the
    /// paper broker reported #1355's (E2, 2026-10-16). A read on from a cursor (`page_token`)
    /// finds nothing after that one fill, as Alpaca's does.
    fills: bool,
    /// The fill activity as Alpaca really sends it: the broker's `order_id` and no
    /// `client_order_id` (E2, 2026-10-16; DEC-955 item 3).
    alpaca_activity: bool,
    /// The open-orders read lists the filled bracket's take-profit at top level under a
    /// broker-named id, as the E2 account did after its fill (runbook §11).
    open_take_profit: bool,
    /// The ticker the `FILL` names, when it is not the deployed equity's own: a fill in an
    /// instrument the run never registered.
    fill_symbol: Option<&'static str>,
    minutes: &'static str,
    posts: Rc<RefCell<Vec<Value>>>,
    clock: Clock,
    deleted: Rc<Cell<bool>>,
    log: Rc<RefCell<Vec<(Method, String, i64)>>>,
}

impl Broker {
    fn file(&self, name: &str) -> Vec<u8> {
        let Equity(id, symbol, _, exchange) = self.equity;
        let text = fs::read_to_string(format!("{ALPACA}tracer/alpaca/{name}")).unwrap();
        let text = text.replace(AAPL, id).replace("AAPL", symbol);
        let text = text.replace("NASDAQ", exchange);
        text.replace("2026-09-28T16:5", self.minutes).into_bytes()
    }

    /// The order fixture as the entry `sent`, at `status` with `filled` shares filled.
    fn order(&self, sent: &Value, status: &str, filled: &str) -> Vec<u8> {
        let field = |name: &str| sent.get(name).and_then(Value::as_str).unwrap().to_owned();
        let order = String::from_utf8(self.file("order.json")).unwrap();
        let order = order.replace("md-e144b97773a6f87c1978cc2831", &field("client_order_id"));
        let order = order.replace(r#""qty":"1""#, &format!(r#""qty":"{}""#, field("qty")));
        let order = order.replace(
            r#""filled_qty":"0""#,
            &format!(r#""filled_qty":"{filled}""#),
        );
        let status = format!(r#""status":"{status}""#);
        order
            .replace(r#""status":"accepted""#, &status)
            .into_bytes()
    }

    /// The entry's read-back, when `path` reads the entry by its client order id.
    fn entry(&self, path: &str) -> Option<Vec<u8>> {
        let entry = self.posts.borrow().first()?.clone();
        let id = entry.get("client_order_id").and_then(Value::as_str)?;
        let lookup = path.starts_with("/v2/orders:by_client_order_id") && path.ends_with(id);
        let qty = entry.get("qty").and_then(Value::as_str)?.to_owned();
        let (status, filled) = match (self.rests, self.deleted.get()) {
            (false, _) => ("filled", qty.as_str()),
            (true, false) => ("accepted", "0"),
            (true, true) => ("canceled", "0"),
        };
        lookup.then(|| self.order(&entry, status, filled))
    }

    /// The account once the entry has filled: its cash less the one share the fill bought at the
    /// entry's limit, 255.2, as the paper broker settles cash (trading spec §11's cash row), so the
    /// watch's reconciliation after the fill compares a cash the fill really moved.
    fn filled_account(&self) -> Vec<u8> {
        let entry = self.posts.borrow().first().cloned().unwrap();
        let field = |name: &str| entry.get(name).and_then(Value::as_str).unwrap().to_owned();
        assert_eq!(
            (field("qty"), field("limit_price")),
            ("1".to_owned(), "255.2".to_owned()),
            "the cash below is the fixture entry's"
        );
        let account = String::from_utf8(self.file("account.json")).unwrap();
        account
            .replace(r#""cash":"1000000""#, r#""cash":"999744.8""#)
            .into_bytes()
    }

    /// The account activities once the entry has filled: one `FILL` of its whole quantity at its
    /// limit price, or none before the entry was sent.
    fn fill(&self) -> Vec<u8> {
        let Some(entry) = self.posts.borrow().first().cloned() else {
            return b"[]".to_vec();
        };
        let field = |name: &str| entry.get(name).and_then(Value::as_str).unwrap().to_owned();
        let (id, qty, price) = (field("client_order_id"), field("qty"), field("limit_price"));
        let symbol = self.fill_symbol.unwrap_or(self.equity.1);
        let at = format!("{}9:59.000000000Z", self.minutes);
        let client = if self.alpaca_activity {
            String::new()
        } else {
            format!(r#""client_order_id":"{id}","#)
        };
        format!(
            r#"[{{"activity_type":"FILL",{client}"cum_qty":"{qty}","id":"20260929170000000::68099755-c7e6-40ea-8ba2-5dc644393428","leaves_qty":"0","order_id":"e02fc2d2-0ff3-444f-a0ab-6253613302fe","price":"{price}","qty":"{qty}","side":"buy","symbol":"{symbol}","transaction_time":"{at}","type":"fill"}}]"#
        )
        .into_bytes()
    }
}

impl Broker {
    /// The open-orders read after the E2 fill: the bracket's take-profit alone, a sell limit at
    /// top level under the broker's own ids, its entry filled and its stop `held` and unlisted.
    fn take_profit(&self) -> Vec<u8> {
        let order = String::from_utf8(self.file("order.json")).unwrap();
        let order = order
            .replace("md-e144b97773a6f87c1978cc2831", TAKE_PROFIT)
            .replace("e02fc2d2-0ff3-444f-a0ab-6253613302fe", TAKE_PROFIT)
            .replace(r#""side":"buy""#, r#""side":"sell""#)
            .replace(r#""status":"accepted""#, r#""status":"new""#);
        format!("[{order}]").into_bytes()
    }
}

/// The E2 take-profit leg's broker id, which is also the client order id Alpaca named it with.
const TAKE_PROFIT: &str = "58a1eef5-7397-48ea-b0c0-bb1154cb1bda";

async fn answer(status: u16, body: Vec<u8>) -> Answer {
    Ok(Response { status, body })
}

impl TradingTransport for Broker {
    fn send(&self, request: &HttpRequest) -> impl Future<Output = Answer> {
        let path = request.path_and_query();
        if !self.posts.borrow().is_empty() {
            let at = self.clock.now().secs();
            self.log
                .borrow_mut()
                .push((request.method(), path.to_owned(), at));
        }
        let lists = ["/v2/positions", "/v2/orders?", "/v2/account/activities?"];
        let asset = format!("/v2/assets/{}", self.equity.1);
        let file = |name| self.file(name);
        let (status, body) = match (request.method(), path) {
            (Method::Get, "/v2/account") if self.fills && !self.posts.borrow().is_empty() => {
                (200, self.filled_account())
            }
            (Method::Get, "/v2/account") => (200, file("account.json")),
            (Method::Get, "/v2/positions") if self.holds => (200, file("position-aapl.json")),
            (Method::Get, "/v2/positions") if self.fills && !self.posts.borrow().is_empty() => {
                (200, file("position-aapl.json"))
            }
            (Method::Get, path) if path == asset => (200, file("asset-aapl.json")),
            (Method::Post, "/v2/orders") => {
                let sent = json(request.body().unwrap());
                let order = self.order(&sent, "accepted", "0");
                self.posts.borrow_mut().push(sent);
                (200, order)
            }
            (Method::Delete, _) => {
                self.deleted.set(true);
                (204, Vec::new())
            }
            (_, lookup) if self.entry(lookup).is_some() => (200, self.entry(lookup).unwrap()),
            (_, list) if list.starts_with("/v2/account/activities?") && self.fills => {
                if list.contains("page_token=") {
                    (200, b"[]".to_vec())
                } else {
                    (200, self.fill())
                }
            }
            (_, list) if list.starts_with("/v2/orders?") && self.open_take_profit => {
                (200, self.take_profit())
            }
            (_, lookup) if lookup.starts_with("/v2/orders:by_client_order_id") => {
                (404, file("order-absent.json"))
            }
            (_, list) if lists.iter().any(|prefix| list.starts_with(prefix)) => {
                (200, b"[]".to_vec())
            }
            (method, path) => panic!("the scripted broker has no answer for {method:?} {path}"),
        };
        answer(status, body)
    }
}

impl DataTransport for Broker {
    fn send(&self, _: &QuoteRequest) -> impl Future<Output = Answer> {
        let quote = String::from_utf8(self.file("quote-aapl.json")).unwrap();
        let recorded = format!(r#""t":"{}9:59.5Z""#, self.minutes);
        assert!(quote.contains(&recorded), "the recorded quote's stamp");
        let quote = match self.quote_offset_ns {
            None => quote,
            Some(offset) => {
                let stamp = shifted(self.clock.peek(), offset);
                quote.replace(&recorded, &format!(r#""t":"{stamp}""#))
            }
        };
        answer(200, quote.into_bytes())
    }

    fn send_bars(&self, _: &BarsRequest) -> impl Future<Output = Answer> {
        let bars = String::from_utf8(self.file("bars-aapl.json")).unwrap();
        let bars = match self.quote_offset_ns {
            None => bars,
            Some(_) => {
                let (head, rest) = bars.split_once(r#""bars":["#).unwrap();
                let (_, later) = rest.split_once("},").unwrap();
                format!(r#"{head}"bars":[{later}"#)
            }
        };
        answer(200, bars.into_bytes())
    }
}

/// The run's clock: it starts at a moment and moves on by each pause (DEC-858 item 6), and by
/// `step_ns` at every read, as a real clock moves between the run's reads (#1355).
#[derive(Clone)]
struct Clock {
    start: &'static str,
    waited: Rc<Cell<u64>>,
    reads: Rc<Cell<u64>>,
    step_ns: u64,
}

impl Clock {
    fn at(start: &'static str) -> Clock {
        Clock::advancing(start, 0)
    }

    fn advancing(start: &'static str, step_ns: u64) -> Clock {
        let (waited, reads) = (Rc::default(), Rc::default());
        Clock {
            start,
            waited,
            reads,
            step_ns,
        }
    }

    /// The instant the next read answers, without reading.
    fn peek(&self) -> UtcNanos {
        let start = UtcNanos::parse_rfc3339(self.start).unwrap();
        let waited = i128::from(self.waited.get()) * NANOS_PER_S;
        let ticked = i128::from(self.reads.get()) * i128::from(self.step_ns);
        shifted(start, waited + ticked)
    }
}

const NANOS_PER_S: i128 = 1_000_000_000;

/// `at` moved by `nanos`.
fn shifted(at: UtcNanos, nanos: i128) -> UtcNanos {
    let total = i128::from(at.secs()) * NANOS_PER_S + i128::from(at.nanos()) + nanos;
    let secs = i64::try_from(total.div_euclid(NANOS_PER_S)).unwrap();
    let nanos = u32::try_from(total.rem_euclid(NANOS_PER_S)).unwrap();
    UtcNanos::from_parts(secs, nanos).unwrap()
}

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        let read = self.peek();
        self.reads.set(self.reads.get() + 1);
        read
    }

    async fn pause(&self, duration: Duration) {
        assert!(
            self.waited.get() < 3_600,
            "the run watched for over an hour"
        );
        self.waited.set(self.waited.get() + duration.as_secs());
    }
}

struct Fake {
    records: Vec<ControlRecord>,
    broker: Broker,
    clock: Clock,
    connects: usize,
    connections: Vec<Connection>,
    hardens: usize,
}

impl Ports for Fake {
    type Session = Broker;
    type Pause = Clock;

    fn control_stream(&mut self, workspace: &str) -> Result<Vec<ControlRecord>, PaperError> {
        assert_eq!(workspace, "ws_paper");
        Ok(self.records.clone())
    }

    fn connect(
        &mut self,
        connection: &Connection,
        dial: Option<&Dial>,
    ) -> Result<Broker, PaperError> {
        assert!(dial.is_none(), "a paper run dials no Robinhood endpoint");
        self.connects += 1;
        self.connections.push(connection.clone());
        Ok(self.broker.clone())
    }

    fn harden(&mut self) -> Result<(), PaperError> {
        self.hardens += 1;
        Ok(())
    }

    fn pause(&self) -> Clock {
        self.clock.clone()
    }
}

/// An equity deployed on the control stream, its objects stored, and `closes` its daily bars, one
/// a session through Monday 2026-09-28: rising a dollar a session to 255.2 is the host's `Long`.
struct Scene {
    equity: Equity,
    root: PathBuf,
    args: Args,
    ports: Fake,
    closes: Vec<(Date, String)>,
}

impl Scene {
    fn new(name: &str, closes: impl Iterator<Item = u32>) -> Scene {
        Scene::of(name, SPY_ARCA, closes)
    }

    fn of(name: &str, equity: Equity, closes: impl Iterator<Item = u32>) -> Scene {
        Scene::deployed(name, equity, closes, |mandate| mandate)
    }

    /// [`Scene::new`] with the deployed mandate's routine rule asking for every opening.
    fn asking(name: &str, closes: impl Iterator<Item = u32>) -> Scene {
        Scene::deployed(name, SPY_ARCA, closes, |mandate| {
            mandate.replace(r#""then": "auto""#, r#""then": "ask""#)
        })
    }

    fn deployed(
        name: &str,
        equity: Equity,
        closes: impl Iterator<Item = u32>,
        edit: fn(String) -> String,
    ) -> Scene {
        let Equity(id, symbol, listing, _) = equity;
        let closes: Vec<u32> = closes.collect();
        let root =
            std::env::temp_dir().join(format!("mandate-paper-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        let quoted = format!("\"{symbol}\"");
        let mandate = edit(
            spy_mandate(&model())
                .replace(SPY, id)
                .replace("\"SPY\"", &quoted),
        );
        let snapshot = SNAPSHOT
            .replace("2026-09-21", "2026-09-25")
            .replace(SPY, id);
        let snapshot = snapshot
            .replace("\"SPY\"", &quoted)
            .replace("arca", listing);
        let stream = Stream::of(&mandate, &snapshot, RULES, FEE, &model());
        let mut store = FsArtifactStore::open(root.join("store")).unwrap();
        for bytes in stream.store.values() {
            store.put_artifact(bytes).unwrap();
        }
        let calendar = ExchangeCalendar::us_equities().unwrap();
        let (mut days, mut day) = (Vec::new(), Date::parse("2026-08-03").unwrap());
        while day <= Date::parse("2026-09-28").unwrap() {
            days.extend(calendar.is_trading_day(day).unwrap().then_some(day));
            day = day.next().unwrap();
        }
        let days = &days[days.len() - closes.len()..];
        let close = |(day, close): (&Date, u32)| (*day, format!("{close}.2"));
        let closes: Vec<_> = days.iter().zip(closes).map(close).collect();
        let kind = Kind::Bars("1Day".parse().unwrap());
        let symbol = Symbol::parse(symbol).unwrap();
        let dataset = DatasetId::new(AssetClass::UsEquity, Feed::Iex, kind, symbol).unwrap();
        let bars = Store::new(root.join("bars"));
        for (day, close) in &closes {
            let [open, high, low, close, vwap] = [close; 5].map(|c| DecStr::parse(c).unwrap());
            let start = UtcNanos::parse_rfc3339(&format!("{day}T04:00:00Z")).unwrap();
            let (volume, trade_count) = (DecStr::parse("50000000").unwrap(), 400_000);
            let bar = vec![Bar {
                start,
                open,
                high,
                low,
                close,
                volume,
                vwap,
                trade_count,
            }];
            bars.put_day(&dataset, *day, &Records::Bars(bar)).unwrap();
        }
        let args = Args {
            workspace: "ws_paper".to_owned(),
            agent: "agent_spy".to_owned(),
            account_ref: "acct_paper".to_owned(),
            journal: None,
            store: root.join("store"),
            bars: bars.dataset_dir(&dataset),
            place_one_order: true,
            reconcile_only: false,
            acknowledged: Environment::Paper,
        };
        let (holds, minutes, posts) = (false, TUESDAY.1, Rc::default());
        let clock = Clock::at(TUESDAY.0);
        let ports = Fake {
            records: stream.records,
            broker: Broker {
                equity,
                quote_offset_ns: None,
                holds,
                rests: false,
                fills: false,
                alpaca_activity: false,
                open_take_profit: false,
                fill_symbol: None,
                minutes,
                posts,
                clock: clock.clone(),
                deleted: Rc::default(),
                log: Rc::default(),
            },
            clock,
            connects: 0,
            connections: Vec::new(),
            hardens: 0,
        };
        Scene {
            equity,
            root,
            args,
            ports,
            closes,
        }
    }

    fn run(&mut self) -> Result<Outcome, PaperError> {
        run(&self.args, &[], &mut self.ports)
    }

    /// Moves the run to `moment`: its clock, and the broker's fresh quote and minute bars.
    fn at(&mut self, (now, minutes): Moment) {
        (self.ports.clock, self.ports.broker.minutes) = (Clock::at(now), minutes);
        self.ports.broker.clock = self.ports.clock.clone();
    }

    /// Moves the run's clock on by `step_ns` at every read, from the run's moment, and stamps the
    /// broker's quote `quote_offset_ns` after the clock's instant at the quote read.
    fn advancing(&mut self, step_ns: u64, quote_offset_ns: i128) {
        self.ports.clock = Clock::advancing(self.ports.clock.start, step_ns);
        self.ports.broker.clock = self.ports.clock.clone();
        self.ports.broker.quote_offset_ns = Some(quote_offset_ns);
    }

    fn posts(&self) -> Vec<Value> {
        self.ports.broker.posts.borrow().clone()
    }

    /// The observation's data as DEC-846 item 3 shapes it, computed here from the bars written.
    fn closes_bytes(&self) -> Vec<u8> {
        let row = |(day, close): &(Date, String)| format!(r#"["{day}","{close}"]"#);
        let rows: Vec<String> = self.closes.iter().map(row).collect();
        let rows = rows.join(",");
        let id = self.equity.0;
        let text = format!(r#"{{"closes":[{rows}],"instrument_id":"{id}"}}"#);
        to_canonical(&json(&text))
    }

    fn stored(&self, bytes: &[u8]) -> Option<Vec<u8>> {
        let store = FsArtifactStore::open(self.root.join("store")).unwrap();
        get_artifact(&store, &ArtifactRef::of(bytes)).ok()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.root).ok();
    }
}

/// FT-1 and FT-6 (journal spec §5.1, §11 check 6): a placing run reads the credentials once,
/// stores the closes the host read under their digest, and sends exactly one order, for the
/// pinned asset, through the cycle. A dry run sends nothing and reports the order it would place,
/// and hands the cycle no journal (DEC-846 item 6): its DSN names a port nobody listens on.
#[test]
fn a_placing_run_stores_the_closes_and_sends_one_order() {
    let mut scene = Scene::new("place", 231..256);
    let outcome = scene.run();
    let Ok(Outcome::Cycle(cycle)) = outcome else {
        panic!("{outcome:?}")
    };
    let report = &cycle.report;
    let posts = scene.posts();
    let counts = (report.submitted.len(), posts.len(), scene.ports.connects);
    assert_eq!(counts, (1, 1, 1), "one credential read, one order");
    assert_eq!(posts[0].get("symbol").and_then(Value::as_str), Some(SPY));
    let bytes = scene.closes_bytes();
    assert_eq!(scene.stored(&bytes), Some(bytes), "the closes are stored");
    let mut dry = Scene::new("dry", 231..256);
    dry.args.place_one_order = false;
    dry.args.journal = Some("postgres://paper@127.0.0.1:1/nobody_listens".to_owned());
    let outcome = dry.run();
    let Ok(Outcome::Cycle(cycle)) = outcome else {
        panic!("{outcome:?}")
    };
    let (report, evidence) = (&cycle.report, cycle.evidence.as_ref());
    assert_eq!((report.submitted.len(), dry.posts().len()), (0, 0));
    assert!(report.would_place.is_some(), "a dry run reports the order");
    assert_eq!(evidence, None, "a dry run names no exportable stream");
}

/// G1a (DEC-937 items 1 and 3): a paper run hands its ports exactly the connection the door
/// chose from the record and the mandate — the Alpaca paper connector over the run's own account
/// stream — and never hardens the process, which only a live run does (LT-14: the paper path
/// is unchanged). A paper deployment acknowledged with `--confirm-live` refuses before any
/// credential is read and sends nothing: each environment needs its own acknowledgement.
#[test]
fn a_paper_run_connects_the_chosen_paper_connection_and_its_own_acknowledgement() {
    let mut scene = Scene::new("chosen", 231..256);
    let outcome = scene.run();
    assert!(matches!(outcome, Ok(Outcome::Cycle(_))), "{outcome:?}");
    let chosen = Connection {
        broker: Chosen::Alpaca,
        environment: Environment::Paper,
        account_ref: "acct_paper".to_owned(),
    };
    assert_eq!(scene.ports.connections, vec![chosen]);
    assert_eq!(scene.ports.hardens, 0, "a paper run keeps its core dumps");
    let mut live = Scene::new("acknowledged-live", 231..256);
    live.args.acknowledged = Environment::Live;
    let refused = live.run();
    assert!(
        matches!(refused, Err(PaperError::PaperUnconfirmed)),
        "{refused:?}"
    );
    let seen = (live.ports.connects, live.ports.hardens, live.posts().len());
    assert_eq!(
        seen,
        (0, 0, 0),
        "refused before any credential, nothing sent"
    );
}

/// #1313's review Minor 2, DEC-914 item 6, the founder's run's step 6, journal spec §2: a placing
/// run that appended to its journal names the three §2 streams its export reads. The assertion
/// cannot live beside the dry half's `None` above: `Scene::of` builds every `Args` with
/// `journal: None`, so the placing half's `Some` needs the journal the run appended to, and that
/// journal is Postgres — the suite's PG class, skipped unless `MANDATE_PG_URL` is set (DEC-109,
/// `tests/binary.rs`'s idiom) and the run still fails through the whole cycle without it. Full
/// value equality, so an always-`None` regression and a hardcoded-names regression both fail. The
/// fakes still drive the run — the scripted records, broker and clock — only the journal is real:
/// the two streams the cycle appended to hold the run's own rows in the journal the DSN names, so
/// the kept evidence is earned by appends, not merely derived from the DSN.
#[test]
fn a_placing_run_that_appended_names_the_streams_its_export_reads() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let mut scene = Scene::new("pg-place", 231..256);
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    scene.args.journal = Some(format!("{url}{joiner}options={options}"));
    let outcome = scene.run();
    let Ok(Outcome::Cycle(cycle)) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        (cycle.report.submitted.len(), scene.posts().len()),
        (1, 1),
        "a placing run through the journal it kept places one order"
    );
    assert_eq!(
        cycle.evidence,
        Some(Evidence {
            streams: [
                "agent:ws_paper:agent_spy".to_owned(),
                "acct:ws_paper:acct_paper".to_owned(),
                "ctl:ws_paper".to_owned(),
            ]
        }),
        "a placing run that appended to its journal names the three §2 streams its export reads"
    );
    let journal = db.journal();
    for name in ["agent:ws_paper:agent_spy", "acct:ws_paper:acct_paper"] {
        let stream = StreamId::parse(name).unwrap();
        let rows = db.block_on(journal.rows(&stream)).unwrap();
        assert!(!rows.is_empty(), "{name} holds the rows the run appended");
    }
}

/// FT-4 (DEC-157 item 4): falling closes are the host's `Flat`, so the run stores nothing, hands
/// the cycle nothing and sends nothing.
#[test]
fn a_flat_model_stores_and_sends_nothing() {
    let mut scene = Scene::new("flat", (231..256).rev());
    let outcome = scene.run();
    let flat = matches!(outcome, Ok(Outcome::NoOutput(Signal::Flat)));
    assert!(flat, "{outcome:?}");
    let counts = (scene.posts().len(), scene.ports.connects);
    assert_eq!(counts, (0, 1), "the host runs after the preflight");
    assert_eq!(scene.stored(&scene.closes_bytes()), None);
}

/// ES-09 and DEC-139: a stop names what stopped the run, and no value; the crate's live test.
#[test]
fn a_stop_names_what_stopped_the_run() {
    let stub = PaperError::Unimplemented { story: "E7-19" }.to_string();
    assert_eq!(stub, "E7-19 has not been implemented yet");
    let store = PaperError::Store.to_string();
    assert_eq!(store, "the artifact store could not be opened or written");
}

/// Appends a control-stream record after the others.
fn push(records: &mut Vec<ControlRecord>, event_type: &str, payload: &str) {
    let seq = u64::try_from(records.len()).unwrap() + 1;
    let (event_type, payload) = (event_type.to_owned(), json(payload));
    records.push(ControlRecord {
        seq,
        event_time: None,
        event_type,
        payload,
        origin: Origin {
            stream_id: "ctl:ws_paper".to_owned(),
            environment: Environment::Paper,
        },
    });
}

/// FT-3, FT-10 (DEC-505 item 1, TI-5, rule 7): no credential is read before any of phase 1's or the
/// registrations' refusals: agent undeployed or stopped; document missing, corrupt or live — a live
/// deployment refusing through the door ahead of the fold, its connection record unrecorded
/// (`ConnectionRefusal::Unrecorded`, DEC-918 item 3; the shell's own `DeploymentRefusal::Live`
/// stays for the shell's fold behind the door); version
/// unconfirmed; a kind unregistered; a fee schedule not yet in effect; or a configured host.
#[test]
fn every_refusal_before_the_credentials_reads_none() {
    let host =
        [("ALPACA_API_BASE", "https://api.alpaca.markets")].map(|(k, v)| (k.into(), v.into()));
    let cases = "undeployed stopped missing corrupt live unconfirmed unregistered fee_date host";
    for case in cases.split(' ') {
        let mut scene = Scene::new(case, 231..256);
        let mut store = FsArtifactStore::open(&scene.args.store).unwrap();
        let records = &mut scene.ports.records;
        let deployed = records.iter().find(|r| r.event_type == "AgentDeployed");
        let version = deployed.and_then(|r| r.payload.get("mandate_version")?.as_str());
        let version = ArtifactRef::parse(version.unwrap()).unwrap();
        let live = spy_mandate(&model()).replace(r#""paper""#, r#""live""#);
        let live = store.put_artifact(&to_canonical(&json(&live))).unwrap();
        let deploy = format!(
            r#"{{"agent_id":"agent_spy","mandate_version":"{live}","record_ref":"sha256:{}"}}"#,
            "6".repeat(64)
        );
        let fee = store.put_artifact(FEE.replace("2026-01-01", "2026-12-01").as_bytes());
        let fee = format!(
            r#"{{"admits_instruments":null,"content_hash":"{}","kind":"fee_config","model_id":null,"model_version":null,"params":[]}}"#,
            fee.unwrap()
        );
        let rule_set = json(r#""rule_set""#);
        match case {
            "undeployed" => records.retain(|r| r.event_type != "AgentDeployed"),
            "stopped" => push(records, "AgentStopped", r#"{"agent_id":"agent_spy"}"#),
            "missing" => scene.args.store = scene.root.join("empty"),
            "corrupt" => {
                let object = store.object_path(&version);
                fs::remove_file(&object).unwrap();
                fs::write(&object, b"{}").unwrap();
            }
            "live" => push(records, "AgentDeployed", &deploy),
            "unconfirmed" => records.retain(|r| r.event_type != "MandateConfirmed"),
            "unregistered" => records.retain(|r| r.payload.get("kind") != Some(&rule_set)),
            "fee_date" => push(records, "ConfigSnapshotRegistered", &fee),
            _ => {}
        }
        let vars: &[(String, String)] = if case == "host" { &host } else { &[] };
        let outcome = run(&scene.args, vars, &mut scene.ports);
        let refused = match &outcome {
            Err(PaperError::Deployment(refusal)) => match refusal {
                DeploymentRefusal::NotDeployed => "undeployed",
                DeploymentRefusal::Stopped => "stopped",
                DeploymentRefusal::DocumentMissing => "missing",
                DeploymentRefusal::DocumentCorrupt => "corrupt",
                DeploymentRefusal::Violations(_) => "unconfirmed",
                _ => "another deployment refusal",
            },
            Err(PaperError::Connection(ConnectionRefusal::Unrecorded)) => "live",
            Err(PaperError::Configuration(ConfigRefusal::Unregistered { kind: "rule_set" })) => {
                "unregistered"
            }
            Err(PaperError::Configuration(ConfigRefusal::FeeNotYetEffective)) => "fee_date",
            Err(PaperError::Shell(ShellError::NonPaperHost { .. })) => "host",
            _ => "something else",
        };
        let seen = (refused, scene.ports.connects, scene.posts().len());
        assert_eq!(seen, (case, 0, 0), "{case}: {outcome:?}");
    }
}

/// FT-2 (DEC-475 item 3): the same code runs two deployments on different equities that differ
/// only in the control stream, the store, the bars and the broker, and each sends its own order
/// and stores its own closes.
#[test]
fn two_equities_differ_only_in_their_journaled_inputs() {
    for equity in [SPY_ARCA, AAPL_NASDAQ] {
        let Equity(id, symbol, _, _) = equity;
        let mut scene = Scene::of(symbol, equity, 231..256);
        let outcome = scene.run();
        let Ok(Outcome::Cycle(cycle)) = outcome else {
            panic!("{symbol}: {outcome:?}")
        };
        let report = &cycle.report;
        let posts = scene.posts();
        assert_eq!((report.submitted.len(), posts.len()), (1, 1), "{symbol}");
        assert_eq!(posts[0].get("symbol").and_then(Value::as_str), Some(id));
        let bytes = scene.closes_bytes();
        assert_eq!(scene.stored(&bytes), Some(bytes), "{symbol}");
    }
}

/// The shell's refusal `outcome` carries, as its cause's message, or what else it was.
fn absent(outcome: &Result<Outcome, PaperError>) -> String {
    match outcome {
        Err(PaperError::Shell(ShellError::Refused { cause, .. })) => cause.to_string(),
        other => format!("not a shell refusal: {other:?}"),
    }
}

/// FT-7 (DEC-470 item 1): an account that already holds a position, as after a first run, is
/// refused with nothing sent, however the model reads.
#[test]
fn a_held_position_sends_nothing() {
    let mut scene = Scene::new("held", 231..256);
    scene.ports.broker.holds = true;
    let outcome = scene.run();
    let expected = "a paper account with no position and no open order";
    assert_eq!(absent(&outcome), expected, "{outcome:?}");
    assert!(scene.posts().is_empty());
}

/// FT-8 (`AGENTS.md` rule 3): bars that end before the last completed session are refused as
/// untrusted market data, with nothing stored or sent.
#[test]
fn stale_bars_store_and_send_nothing() {
    let mut scene = Scene::new("stale", 231..256);
    scene.at(WEDNESDAY);
    let outcome = scene.run();
    let code = match &outcome {
        Err(PaperError::Shell(error)) => error.code(),
        _ => "not a shell refusal",
    };
    assert_eq!(code, "market_data_untrusted", "{outcome:?}");
    assert!(scene.posts().is_empty());
    assert_eq!(scene.stored(&scene.closes_bytes()), None);
}

/// FT-9 (trading spec §9.6, DEC-509 item 4): from 15:50 New York no opening is placed, so a run
/// whose quote and minute bars are fresh is refused at the closing window with nothing sent.
#[test]
fn a_run_in_the_closing_window_sends_nothing() {
    let mut scene = Scene::new("closing", 231..256);
    scene.at(CLOSING);
    let outcome = scene.run();
    let expected = "a run clock before the close window";
    assert_eq!(absent(&outcome), expected, "{outcome:?}");
    assert!(scene.posts().is_empty());
}

/// DEC-846 items 2 and 3: `daily_closes` hands the host every stored close, oldest first, each
/// with its session, through the shell's trust check: stale bars, or another instrument's, are
/// untrusted.
#[test]
fn the_closes_are_the_full_trusted_span_in_order() {
    let scene = Scene::new("closes", 231..256);
    let (spy, aapl) = (
        InstrumentId::new("SPY").unwrap(),
        InstrumentId::new("AAPL").unwrap(),
    );
    let at = |(now, _): Moment| UtcNanos::parse_rfc3339(now).unwrap();
    let closes = daily_closes(&spy, &scene.args.bars, at(TUESDAY)).unwrap();
    let seen: Vec<(Date, String)> = closes.iter().map(|(d, p)| (*d, p.to_string())).collect();
    assert_eq!(
        (seen.len(), &seen),
        (25, &scene.closes),
        "all 25, oldest first"
    );
    for (symbol, moment) in [(&spy, WEDNESDAY), (&aapl, TUESDAY)] {
        let refused = daily_closes(symbol, &scene.args.bars, at(moment));
        assert!(
            matches!(refused, Err(Cause::Untrusted { .. })),
            "{symbol}: {refused:?}"
        );
    }
}

/// E1b part 2, FT-11, DEC-858 items 3 and 6, DEC-853 items 5 and 6: a placing run watches its
/// entry through the cycle at the registered rule set's interval (5 s) and cancels it through the
/// executor at the registered close window's start: from 15:40 the entry is read at every wake
/// from 15:40:05, the one `DELETE` is at 15:50:00, and the last read, at 15:50:05, reads it
/// canceled and ends the run.
#[test]
fn a_placing_run_watches_its_entry_to_the_registered_bound() {
    let mut scene = Scene::new("watch", 231..256);
    scene.at(LATE);
    scene.ports.broker.rests = true;
    let outcome = scene.run();
    let Ok(Outcome::Cycle(cycle)) = outcome else {
        panic!("{outcome:?}")
    };
    let report = &cycle.report;
    assert_eq!((report.submitted.len(), scene.posts().len()), (1, 1));
    let secs = |text| UtcNanos::parse_rfc3339(text).unwrap().secs();
    let log = scene.ports.broker.log.borrow().clone();
    let at = |keep: fn(&Method, &str) -> bool| -> Vec<i64> {
        log.iter()
            .filter(|(m, p, _)| keep(m, p))
            .map(|(.., s)| *s)
            .collect()
    };
    let deletes = at(|m, _| *m == Method::Delete);
    assert_eq!(
        deletes,
        [secs("2026-09-29T19:50:00Z")],
        "one cancel, at the bound"
    );
    let mut reads = at(|m, p| *m == Method::Get && p.starts_with("/v2/orders:by_client_order_id"));
    reads.dedup();
    let wakes = secs("2026-09-29T19:40:05Z")..=secs("2026-09-29T19:50:05Z");
    let wakes: Vec<i64> = wakes.step_by(5).collect();
    assert_eq!(
        reads, wakes,
        "reads at every five-second wake, and none after the cancel's"
    );
}

/// DEC-475 item 3 for the shipping adapter (FT-2): `run` selects no product value. No instrument,
/// model, deployment id, gate value or executor value is a constant in the adapter's assembly, and
/// the deployment input comes from the control stream and the artifact store. This is the
/// successor of the shell-side scan that read the deleted `mandate-tracer` binary's function.
#[test]
fn the_shipping_adapter_selects_no_product_value() {
    let source = include_str!("../src/lib.rs");
    let body = source
        .split_once("pub fn run<P: Ports>(")
        .and_then(|(_, after)| after.split_once("\n}\n\n/// The binary's ports"))
        .map(|(body, _)| body)
        .expect("the run function's body is bounded by its doc successor");
    for forbidden in [
        "\"AAPL\"",
        "\"SPY\"",
        "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415",
        "b28f4066-5c6d-479b-a2af-85dc1a8f16fb",
        "quant.ma",
        "conn_alpaca_paper_01",
        "tracer-aapl",
        "tracer-paper",
        "\"ws_",
        "\"agent_spy",
        "GateConfig",
        "ExecutorConfig",
    ] {
        assert!(
            !body.contains(forbidden),
            "the shipping adapter still selects {forbidden}"
        );
    }
    for required in [
        "let records = ports.control_stream(&args.workspace)?;",
        "confirmed_version(&records, &store, &facts)",
        "configuration(&records, &store, &pinned, day)",
        "Artifacts::from_registered(&confirmed, &config)",
    ] {
        assert!(
            body.contains(required),
            "the shipping adapter must take its inputs from the stream: {required}"
        );
    }
}

/// The quote's age bound the registered rule set holds (`iex_quote_max_age_s`).
const QUOTE_MAX_AGE_NS: i128 = 10 * NANOS_PER_S;
/// How far ahead of the host clock DEC-936 lets the preflight's IEX quote be stamped.
const QUOTE_SKEW_NS: i128 = 250_000_000;

/// The cause and stage `outcome` refused with, or what else it was.
fn refused_at(outcome: &Result<Outcome, PaperError>) -> String {
    match outcome {
        Err(PaperError::Shell(ShellError::Refused { stage, cause })) => {
            format!("{stage:?}: {cause}")
        }
        other => format!("not a shell refusal: {other:?}"),
    }
}

/// Whether `outcome` is a placing run that sent exactly one order.
fn placed_one(scene: &Scene, outcome: &Result<Outcome, PaperError>) -> bool {
    matches!(outcome, Ok(Outcome::Cycle(cycle)) if cycle.report.submitted.len() == 1)
        && scene.posts().len() == 1
}

/// #1355's defect, `AGENTS.md` rule 3 read with DEC-935: the clock moves between the run's
/// reads, so the asset record (stamped at its own read) and the quote (stamped at the quote
/// read) are both newer than the run's start. Each is judged at a clock read taken after it was
/// read, so the run places its one order; judging them at the start refused every real run with
/// "a current asset record".
#[test]
fn facts_stamped_after_the_run_start_are_judged_at_a_later_clock_read() {
    let mut scene = Scene::new("advancing", 231..256);
    scene.advancing(1_000_000, 0);
    let start = UtcNanos::parse_rfc3339(TUESDAY.0).unwrap();
    let outcome = scene.run();
    assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
    assert!(
        scene.ports.clock.peek() > start,
        "the clock moved during the run"
    );
}

/// DEC-935 item 2: no bound is widened. A clock that moves four seconds a read leaves the asset
/// record older than its ten-second bound at the judge's read, three reads after its own, so the
/// run still refuses it with nothing sent.
#[test]
fn an_asset_record_older_than_its_bound_at_the_judges_read_still_refuses() {
    let mut scene = Scene::new("stale-asset", 231..256);
    scene.advancing(4_000_000_000, 0);
    let outcome = scene.run();
    assert_eq!(refused_at(&outcome), "Validate: a current asset record");
    assert!(scene.posts().is_empty());
}

/// DEC-936 item 1 (amending DEC-935 item 2): a quote stamped more than 250 ms after the clock at
/// its read, by even a nanosecond, or older than its bound by a nanosecond there, is refused at
/// the preflight with nothing sent; one exactly at the bound is current.
#[test]
fn a_quote_past_the_skew_tolerance_or_its_bound_still_refuses() {
    let expected = "Reconcile: a current IEX quote";
    for (name, offset) in [
        ("future", QUOTE_SKEW_NS + 1),
        ("stale", -QUOTE_MAX_AGE_NS - 1),
    ] {
        let mut scene = Scene::new(&format!("quote-{name}"), 231..256);
        scene.advancing(0, offset);
        let outcome = scene.run();
        assert_eq!(refused_at(&outcome), expected, "{name}");
        assert!(scene.posts().is_empty(), "{name}");
    }
    let mut scene = Scene::new("quote-at-bound", 231..256);
    scene.advancing(0, -QUOTE_MAX_AGE_NS);
    let outcome = scene.run();
    assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
}

/// DEC-936 changes only the preflight's check: a quote within the tolerance passes it, and the
/// shell's judge, unchanged (DEC-935 item 2), still refuses a quote stamped after the run clock.
/// With a clock that does not move, #1355's 2 to 24 ms skews refuse there, with nothing sent.
#[test]
fn a_quote_within_the_tolerance_still_ahead_of_the_run_clock_refuses_at_the_judge() {
    for offset in [1, 2_000_000, 24_000_000, QUOTE_SKEW_NS] {
        let mut scene = Scene::new(&format!("quote-skew-{offset}"), 231..256);
        scene.advancing(0, offset);
        let outcome = scene.run();
        assert_eq!(
            refused_at(&outcome),
            "Validate: a current, uncrossed IEX quote",
            "{offset}"
        );
        assert!(scene.posts().is_empty(), "{offset}");
    }
}

/// #1355's case under DEC-936: a quote 24 ms ahead of the host clock at its read passes the
/// preflight, and once the clock has moved past its stamp by the run clock's read, the judge and
/// the binding gate hold it current, so the run places its one order.
#[test]
fn a_quote_24_ms_ahead_places_once_the_clock_has_passed_its_stamp() {
    let mut scene = Scene::new("quote-skew-placed", 231..256);
    scene.advancing(20_000_000, 24_000_000);
    let outcome = scene.run();
    assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, failure_persistence: None, ..ProptestConfig::default() })]

    /// DEC-935, over clock advances: however far the clock moves between reads, up to 20 ms a
    /// read, a quote up to 9 s old at its read and an asset record stamped at its own read are
    /// current at every later read, so the run places its one order.
    #[test]
    fn any_small_clock_advance_places_the_one_order(
        step_ns in 0_u64..=20_000_000,
        age_ns in 0_i128..=9 * NANOS_PER_S,
    ) {
        let mut scene = Scene::new(&format!("advance-{step_ns}-{age_ns}"), 231..256);
        scene.advancing(step_ns, -age_ns);
        let outcome = scene.run();
        prop_assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
    }

    /// DEC-936 item 1, over clock advances: a quote stamped more than 250 ms after the clock at
    /// its read, by a nanosecond to an hour, is refused whatever the clock's step, with nothing
    /// sent.
    #[test]
    fn a_future_quote_refuses_at_any_clock_advance(
        step_ns in 0_u64..=20_000_000,
        ahead_ns in QUOTE_SKEW_NS + 1..=3_600 * NANOS_PER_S,
    ) {
        let mut scene = Scene::new(&format!("future-{step_ns}-{ahead_ns}"), 231..256);
        scene.advancing(step_ns, ahead_ns);
        let outcome = scene.run();
        prop_assert_eq!(
            refused_at(&outcome),
            "Reconcile: a current IEX quote"
        );
        prop_assert!(scene.posts().is_empty());
    }
}

/// #1355's first defect (E2, 2026-10-16), `AGENTS.md` rule 5, journal spec §9 and DEC-938: once
/// the bracket entry reads `filled`, the reconciliation it asks for reads the broker's `FILL`,
/// which names the share by its ticker, and journals it as the run's own fill. With a clock that
/// moves at every read, as the founder's did, every append is accepted and the run ends having
/// placed its one order, with no external-activity alert: the first real fill was refused there
/// as `the journal answered Invalid`.
#[test]
fn a_filled_entry_journals_its_fill_and_the_run_ends() {
    let mut scene = Scene::new("filled", 231..256);
    scene.ports.broker.fills = true;
    scene.advancing(1_000_000, 0);
    let outcome = scene.run();
    assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
    let Ok(Outcome::Cycle(cycle)) = outcome else {
        unreachable!("placed_one matched a cycle")
    };
    assert!(
        !cycle.report.alerts.contains(&"external_activity"),
        "the run's own fill is never external activity: {:?}",
        cycle.report.alerts
    );
}

/// #1355's first defect through a real journal (the suite's PG class, DEC-109): the account
/// stream holds the entry's fill, priced as the broker filled it, for the client order id and
/// asset the run sent, and both streams the run appended to verify from genesis (journal spec
/// §11). The entry already read `filled` when its fill arrived, so the fill is the
/// `LateFillApplied` trading spec §5.7 journals for a fill of an order in a terminal state.
#[test]
fn a_filled_entry_journals_its_fill_price_and_both_streams_verify() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let mut scene = Scene::new("pg-filled", 231..256);
    scene.ports.broker.fills = true;
    scene.advancing(1_000_000, 0);
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    scene.args.journal = Some(format!("{url}{joiner}options={options}"));
    let outcome = scene.run();
    assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
    let sent = &scene.posts()[0];
    let field = |name: &str| sent.get(name).and_then(Value::as_str).unwrap().to_owned();
    let journal = db.journal();
    let account = StreamId::parse("acct:ws_paper:acct_paper").unwrap();
    let rows = db.block_on(journal.rows(&account)).unwrap();
    let fills: Vec<&StoredEvent> = rows
        .iter()
        .filter(|row| row.event_type.ends_with("FillApplied"))
        .collect();
    let types: Vec<&str> = fills.iter().map(|row| row.event_type.as_str()).collect();
    assert_eq!(
        types,
        ["LateFillApplied"],
        "one fill record for the one fill"
    );
    let body = json(std::str::from_utf8(&fills[0].body).unwrap());
    let payload = body.get("payload").unwrap();
    let member = |name: &str| payload.get(name).and_then(Value::as_str).map(str::to_owned);
    assert_eq!(
        member("price"),
        Some(field("limit_price")),
        "the fill price"
    );
    assert_eq!(member("client_order_id"), Some(field("client_order_id")));
    assert_eq!(
        member("instrument_id"),
        Some(field("symbol")),
        "the asset sent"
    );
    assert_eq!(member("qty_gross"), Some(field("qty")));
    let store = FsArtifactStore::open(&scene.args.store).unwrap();
    for name in ["agent:ws_paper:agent_spy", "acct:ws_paper:acct_paper"] {
        let stream = StreamId::parse(name).unwrap();
        let rows = db.block_on(journal.rows(&stream)).unwrap();
        let verified = verify_events(&rows, TrustedStart::GENESIS, &store);
        assert!(verified.is_ok(), "{name}: {verified:?}");
    }
}

/// #1369 review, DEC-938 item 1: a `FILL` naming a ticker the run never registered is external
/// activity. Since DEC-2817 the journal accepts its batch: the `FillApplied` names the run's fee
/// configuration and calendar and no instrument snapshot (journal spec §9.5 rule 300). The fill is
/// external activity all the same, so the reconciliation that found it does not pass and the run
/// stops there, and nothing further reaches the broker: no request follows the activity read that
/// found the fill.
#[test]
fn a_fill_in_an_unregistered_ticker_is_booked_and_stops_the_run_at_the_reconciliation() {
    let mut scene = Scene::new("unregistered-fill", 231..256);
    scene.ports.broker.fills = true;
    scene.ports.broker.fill_symbol = Some("QQQ");
    scene.advancing(1_000_000, 0);
    let outcome = scene.run();
    assert!(
        matches!(
            outcome,
            Err(PaperError::Shell(ShellError::ReconciliationMismatch))
        ),
        "{}",
        refused_at(&outcome)
    );
    assert_eq!(scene.posts().len(), 1, "the entry, and no other order");
    let log = scene.ports.broker.log.borrow();
    let found = log
        .iter()
        .rposition(|(_, path, _)| path.starts_with("/v2/account/activities?"))
        .expect("the reconciliation read the activities");
    assert_eq!(
        log.get(found + 1..).map(<[_]>::len),
        Some(0),
        "nothing is sent after the refused fill: {:?}",
        &log[found..]
    );
}

/// DEC-948 items 2 to 4 through a real journal (the suite's PG class, DEC-109): the executor
/// learns from its stream's envelope that it runs on paper, so the entry's fill books the
/// simulated fee the registered fee configuration defines (trading spec §10), naming that
/// configuration, for the client order id the run sent; and every account-stream record carries
/// the run clock's risk clock or later, never 1970's zero (mandate spec §5.2).
#[test]
fn a_paper_fill_books_its_simulated_fee_at_the_run_clock() {
    let Some(db) = support::TestDb::new() else {
        return;
    };
    let mut scene = Scene::new("pg-fee", 231..256);
    scene.ports.broker.fills = true;
    scene.advancing(1_000_000, 0);
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    scene.args.journal = Some(format!("{url}{joiner}options={options}"));
    let outcome = scene.run();
    assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
    let sent = scene.posts()[0].clone();
    let journal = db.journal();
    let account = StreamId::parse("acct:ws_paper:acct_paper").unwrap();
    let rows = db.block_on(journal.rows(&account)).unwrap();
    let bodies: Vec<(String, Value)> = rows
        .iter()
        .map(|row| {
            let body = json(std::str::from_utf8(&row.body).unwrap());
            (row.event_type.clone(), body)
        })
        .collect();
    let refs = |event_type: &str, name: &str| {
        bodies
            .iter()
            .find(|(t, _)| t == event_type)
            .and_then(|(_, body)| body.get("config_refs")?.get(name)?.as_str())
            .map(str::to_owned)
    };
    let fees: Vec<&Value> = bodies
        .iter()
        .filter(|(t, _)| t == "FeesCharged")
        .filter_map(|(_, body)| body.get("payload"))
        .collect();
    let [fee] = fees[..] else {
        panic!("one simulated fee for the one fill, not {}", fees.len());
    };
    let member = |name: &str| fee.get(name).cloned();
    assert_eq!(member("simulated"), Some(Value::Bool(true)));
    assert_eq!(
        member("client_order_id"),
        sent.get("client_order_id").cloned()
    );
    assert_eq!(member("family"), Some(Value::Str("equities".to_owned())));
    assert_eq!(
        member("charged"),
        Some(Value::Str("0".to_owned())),
        "a simulated fee is never charged by the broker"
    );
    assert!(
        refs("FeesCharged", "fee_config").is_some(),
        "the fee names its configuration"
    );
    assert_eq!(
        refs("FeesCharged", "fee_config"),
        refs("LateFillApplied", "fee_config"),
        "the configuration its fill was accounted under"
    );
    let start = UtcNanos::parse_rfc3339(TUESDAY.0).unwrap();
    let clocks: Vec<(&str, &str)> = bodies
        .iter()
        .filter_map(|(t, body)| {
            let clock = body.get("payload")?.get("risk_clock")?.as_str()?;
            Some((t.as_str(), clock))
        })
        .collect();
    assert!(!clocks.is_empty());
    for (event_type, clock) in clocks {
        let at = UtcNanos::parse(clock).unwrap();
        assert!(
            at.secs() >= start.secs(),
            "{event_type}'s risk clock {clock} is before the run clock {}",
            TUESDAY.0
        );
    }
    let store = FsArtifactStore::open(&scene.args.store).unwrap();
    let verified = verify_events(&rows, TrustedStart::GENESIS, &store);
    assert!(verified.is_ok(), "{verified:?}");
}

/// DEC-940, rule 3: a placing run whose cycle asks waits for the grant at the watch's interval,
/// and an ask no answer reaches denies at its own deadline with nothing sent: the scratch
/// journal no founder can answer on holds only the run's own ask. Each wake's fresh quote is
/// stamped half a second before its read, so every one is current (DEC-943 item 2).
#[test]
fn an_asked_opening_no_answer_reaches_denies_at_its_deadline() {
    let mut scene = Scene::asking("asked", 231..256);
    scene.advancing(0, -NANOS_PER_S / 2);
    let outcome = scene.run();
    assert!(
        matches!(
            outcome,
            Err(PaperError::Grant(mandate_paper::GrantRefusal::TimedOut))
        ),
        "{outcome:?}"
    );
    assert!(scene.posts().is_empty(), "nothing is sent");
    assert_eq!(
        scene.ports.clock.waited.get(),
        600,
        "the wait ran to the ask's own deadline, the mandate's 600 s timeout"
    );
}

/// DEC-943 item 2, rule 3: the wait reads a fresh IEX quote at every wake, through the
/// preflight's own check, and never relaxes its ten-second bound. A broker whose quote stays
/// stamped at its recorded instant, half a second before the run, answers current wakes until
/// the clock is more than ten seconds past that stamp; the first wake after it refuses there,
/// before the runtime is handed anything, with nothing sent.
#[test]
fn an_asked_openings_stale_fresh_quote_refuses_at_its_wake() {
    let mut scene = Scene::asking("asked-stale", 231..256);
    let outcome = scene.run();
    assert_eq!(
        refused_at(&outcome),
        "MarketData: a current, uncrossed IEX quote at the wake"
    );
    assert!(scene.posts().is_empty(), "nothing is sent");
    assert_eq!(
        scene.ports.clock.waited.get(),
        10,
        "the wakes at 0 and 5 s read a current quote; the one at 10 s reads it 10.5 s old"
    );
}

/// A dry run that asks never waits: it stops at the ask as before, nothing sent.
#[test]
fn a_dry_run_that_asks_stops_at_the_ask() {
    let mut scene = Scene::asking("asked-dry", 231..256);
    scene.args.place_one_order = false;
    let outcome = scene.run();
    assert!(
        matches!(&outcome, Err(PaperError::Shell(stop)) if stop.code() == "autonomy_not_auto"),
        "{outcome:?}"
    );
    assert!(scene.posts().is_empty());
    assert_eq!(scene.ports.clock.waited.get(), 0, "no wait");
}

/// DEC-940 item 4: an Alpaca session's own ask, the quote an asked opening's cross-check compares,
/// is the IEX ask its preflight read — through the test transport and through the binary's own
/// `Connected` session alike (its placeholder credentials build a client and send nothing).
#[test]
fn an_alpaca_session_answers_its_own_iex_ask() {
    let scene = Scene::new("broker-ask", 231..256);
    let facts = spy_facts(
        SPY,
        "SPY",
        mandate_alpaca::Exchange::Arca,
        UtcNanos::parse_rfc3339(TUESDAY.0).unwrap(),
    );
    let ask = Some(facts.broker.quote.ask);
    assert!(matches!(
        mandate_paper::Session::broker_ask(&scene.ports.broker, &facts.broker),
        mandate_paper::BrokerAsk::Quoted(quoted) if Some(quoted) == ask
    ));
    let credentials =
        mandate_alpaca::Credentials::from_lookup(|_| Some("placeholder".to_owned())).unwrap();
    let connected: mandate_paper::Connected = mandate_paper::Connected::Alpaca(
        mandate_alpaca::AlpacaPaperHttp::new(credentials).unwrap(),
    );
    assert!(matches!(
        mandate_paper::Session::broker_ask(&connected, &facts.broker),
        mandate_paper::BrokerAsk::Quoted(quoted) if Some(quoted) == ask
    ));
}

/// The journal DSN of `db`'s own schema, as the founder's runs name theirs.
fn dsn(db: &support::TestDb) -> String {
    let url = std::env::var(support::URL_VAR).unwrap();
    let joiner = if url.contains('?') { '&' } else { '?' };
    let options = format!("-c%20search_path%3D{}%20-c%20role%3D{APP_ROLE}", db.schema);
    format!("{url}{joiner}options={options}")
}

/// The account stream's rows in `db`.
fn account_rows(db: &support::TestDb) -> Vec<StoredEvent> {
    let account = StreamId::parse("acct:ws_paper:acct_paper").unwrap();
    db.block_on(db.journal().rows(&account)).unwrap()
}

/// The E2 state (runbook §11, DEC-955), made through a real journal (the suite's PG class,
/// DEC-109): one placing run whose entry the broker filled while reporting no fill activity, so
/// the account stream holds the entry `filled` and no `FillApplied` or `LateFillApplied`. The
/// broker then shows what E2's showed: the share held, the take-profit resting at top level, and
/// the fill in its activities as Alpaca sends it, by its `order_id` and with no client order id.
fn e2_state(name: &str) -> Option<(support::TestDb, Scene)> {
    let db = support::TestDb::new()?;
    let mut scene = Scene::new(name, 231..256);
    scene.advancing(1_000_000, 0);
    scene.args.journal = Some(dsn(&db));
    let outcome = scene.run();
    assert!(placed_one(&scene, &outcome), "{}", refused_at(&outcome));
    let rows = account_rows(&db);
    assert!(
        !rows
            .iter()
            .any(|row| row.event_type.ends_with("FillApplied")),
        "the E2 state: no fill booked"
    );
    assert!(
        rows.iter().any(|row| row.event_type == "OrderStateChanged"
            && String::from_utf8_lossy(&row.body).contains(r#""state":"filled""#)),
        "the E2 state: the entry journaled filled"
    );
    scene.ports.broker.fills = true;
    scene.ports.broker.alpaca_activity = true;
    scene.ports.broker.open_take_profit = true;
    Some((db, scene))
}

/// The requests the broker was sent from the `from`th logged one on, as `(method, path)`.
fn sent_since(scene: &Scene, from: usize) -> Vec<(mandate_alpaca::Method, String)> {
    let log = scene.ports.broker.log.borrow();
    log.iter()
        .skip(from)
        .map(|(method, path, _)| (*method, path.clone()))
        .collect()
}

/// The last row's `(seq, hash)` of `rows`.
fn head_of(rows: &[StoredEvent]) -> Option<(u64, String)> {
    rows.last().map(|row| (row.seq, format!("{:?}", row.hash)))
}

/// DEC-955, the E2 fix: a placing run on the E2 state books the entry's fill as the
/// `LateFillApplied` trading spec §5.7 names, at the broker's price, for the client order id and
/// asset it sent, before its preflight is judged; then the unchanged flat-account check (DEC-470
/// item 1) refuses at Validate with nothing sent. Every request the run made was a read, and the
/// streams verify from genesis. A third run is refused the same way and appends nothing: the
/// journal it replays already holds the fill, so the replay is identical.
#[test]
fn the_e2_state_books_its_fill_then_refuses_at_validate_with_nothing_sent() {
    let Some((db, mut scene)) = e2_state("e2-placing") else {
        return;
    };
    let logged = scene.ports.broker.log.borrow().len();
    let outcome = scene.run();
    assert_eq!(
        refused_at(&outcome),
        "Validate: a paper account with no position and no open order"
    );
    assert_eq!(scene.posts().len(), 1, "no second order");
    let sent = sent_since(&scene, logged);
    assert!(
        sent.iter()
            .all(|(method, _)| *method == mandate_alpaca::Method::Get),
        "reads only: {sent:?}"
    );
    let sent_entry = &scene.posts()[0];
    let field = |name: &str| {
        sent_entry
            .get(name)
            .and_then(Value::as_str)
            .unwrap()
            .to_owned()
    };
    let rows = account_rows(&db);
    let fills: Vec<&StoredEvent> = rows
        .iter()
        .filter(|row| row.event_type.ends_with("FillApplied"))
        .collect();
    let types: Vec<&str> = fills.iter().map(|row| row.event_type.as_str()).collect();
    assert_eq!(types, ["LateFillApplied"], "the one fill, booked once");
    let body = json(std::str::from_utf8(&fills[0].body).unwrap());
    let payload = body.get("payload").unwrap();
    let member = |name: &str| payload.get(name).and_then(Value::as_str).map(str::to_owned);
    assert_eq!(
        member("price"),
        Some(field("limit_price")),
        "the broker's price"
    );
    assert_eq!(member("client_order_id"), Some(field("client_order_id")));
    assert_eq!(
        member("instrument_id"),
        Some(field("symbol")),
        "the asset sent"
    );
    assert_eq!(member("qty_gross"), Some(field("qty")));
    assert!(
        !rows
            .iter()
            .any(|row| row.event_type == "ExternalActivityIngested"),
        "the take-profit at top level is never adopted"
    );
    let store = FsArtifactStore::open(&scene.args.store).unwrap();
    for name in ["agent:ws_paper:agent_spy", "acct:ws_paper:acct_paper"] {
        let stream = StreamId::parse(name).unwrap();
        let rows = db.block_on(db.journal().rows(&stream)).unwrap();
        let verified = verify_events(&rows, TrustedStart::GENESIS, &store);
        assert!(verified.is_ok(), "{name}: {verified:?}");
    }

    let before = head_of(&rows);
    let outcome = scene.run();
    assert_eq!(
        refused_at(&outcome),
        "Validate: a paper account with no position and no open order"
    );
    assert_eq!(
        head_of(&account_rows(&db)),
        before,
        "nothing more is appended"
    );
    assert_eq!(scene.posts().len(), 1);
}

/// DEC-955 item 6: `--reconcile-only` books the E2 fill, verifies that nothing more is owed, and
/// exits without a model, a market judgement or a cycle: it reads only, places nothing, and
/// prints the count alone. A second invocation books nothing.
#[test]
fn reconcile_only_books_the_e2_fill_and_places_nothing() {
    let Some((db, mut scene)) = e2_state("e2-reconcile") else {
        return;
    };
    scene.args.place_one_order = false;
    scene.args.reconcile_only = true;
    let logged = scene.ports.broker.log.borrow().len();
    let outcome = scene.run();
    assert!(
        matches!(&outcome, Ok(Outcome::Reconciled(Reconciled { fills: 1 }))),
        "{outcome:?}"
    );
    assert_eq!(
        lines(outcome.as_ref().unwrap()).unwrap(),
        ["booked 1 fill record; nothing sent"]
    );
    let sent = sent_since(&scene, logged);
    assert!(!sent.is_empty(), "the broker was asked");
    assert!(
        sent.iter()
            .all(|(method, path)| *method == mandate_alpaca::Method::Get
                && (path.starts_with("/v2/orders:by_client_order_id")
                    || path.starts_with("/v2/account/activities?"))),
        "only the owned order and the fills are read: {sent:?}"
    );
    assert_eq!(scene.posts().len(), 1);
    let fills = account_rows(&db)
        .iter()
        .filter(|row| row.event_type == "LateFillApplied")
        .count();
    assert_eq!(fills, 1);
    let outcome = scene.run();
    assert!(
        matches!(&outcome, Ok(Outcome::Reconciled(Reconciled { fills: 0 }))),
        "{outcome:?}"
    );
}

/// DEC-955 item 4, rule 3: a broker whose word disagrees with the journal — here the entry reads
/// filled and no fill activity is reported for it — stops the run at Reconcile before its
/// preflight, with nothing journaled or sent, and the stop names the order's opaque id and a
/// reason code only (rule 6).
#[test]
fn a_disagreeing_broker_stops_at_reconcile_with_nothing_journaled_or_sent() {
    let Some((db, mut scene)) = e2_state("e2-mismatch") else {
        return;
    };
    scene.ports.broker.fills = false;
    let before = head_of(&account_rows(&db));
    let outcome = scene.run();
    let refusal = refused_at(&outcome);
    assert!(refusal.starts_with("Reconcile: "), "{refusal}");
    assert!(refusal.contains("fills_short"), "{refusal}");
    let Err(error) = &outcome else {
        unreachable!("refused")
    };
    let line = stderr_line(error);
    let sent = &scene.posts()[0];
    let id = sent.get("client_order_id").and_then(Value::as_str).unwrap();
    let limit = sent.get("limit_price").and_then(Value::as_str).unwrap();
    assert!(line.contains(id) && !line.contains(limit), "{line}");
    assert_eq!(head_of(&account_rows(&db)), before, "nothing journaled");
    assert_eq!(scene.posts().len(), 1, "nothing sent");
}

/// A dry run keeps nothing (DEC-157 item 6): on the E2 state it books no fill and is refused at
/// Validate as before, so only a placing run or `--reconcile-only` appends.
#[test]
fn a_dry_run_on_the_e2_state_books_nothing() {
    let Some((db, mut scene)) = e2_state("e2-dry") else {
        return;
    };
    scene.args.place_one_order = false;
    let before = head_of(&account_rows(&db));
    let outcome = scene.run();
    assert_eq!(
        refused_at(&outcome),
        "Validate: a paper account with no position and no open order"
    );
    assert_eq!(head_of(&account_rows(&db)), before);
}

/// `--reconcile-only` books to the journal it names, so without one it refuses before anything
/// is read or connected.
#[test]
fn reconcile_only_without_a_journal_reads_nothing() {
    let mut scene = Scene::new("reconcile-no-journal", 231..256);
    scene.args.place_one_order = false;
    scene.args.reconcile_only = true;
    let outcome = scene.run();
    assert!(matches!(&outcome, Err(PaperError::Usage(_))), "{outcome:?}");
    assert_eq!(scene.ports.connects, 0);
}
