//! The paper adapter's one run (E7-19 slice 5, E1a, DEC-846): SPY deployed as the shell's tests
//! deploy it, a real store and bars, a scripted broker, and no network (ES-19, rule 8).

#[path = "../../mandate-shell/tests/common/mod.rs"]
mod common;

use common::{AAPL, FEE, RULES, SNAPSHOT, SPY, Stream, json, model, spy_mandate};
use mandate_alpaca::{BarsRequest, DataTransport, HttpRequest, Method, Pause, QuoteRequest};
use mandate_alpaca::{Response, TradingTransport, TransportError};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{DecStr, Value, to_canonical};
use mandate_journal::{ArtifactRef, ArtifactStore, get_artifact};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_modelhost::Signal;
use mandate_paper::{Args, Outcome, PaperError, Ports, run};
use mandate_shell::ShellError;
use mandate_shell::control::{ConfigRefusal, ControlRecord, DeploymentRefusal};
use mandate_time::{Date, ExchangeCalendar, UtcNanos};
use std::cell::RefCell;
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
/// handed; `holds` makes it report one share already held, as after a first run.
#[derive(Clone)]
struct Broker {
    equity: Equity,
    holds: bool,
    minutes: &'static str,
    posts: Rc<RefCell<Vec<Value>>>,
}

impl Broker {
    fn file(&self, name: &str) -> Vec<u8> {
        let Equity(id, symbol, _, exchange) = self.equity;
        let text = fs::read_to_string(format!("{ALPACA}tracer/alpaca/{name}")).unwrap();
        let text = text.replace(AAPL, id).replace("AAPL", symbol);
        let text = text.replace("NASDAQ", exchange);
        text.replace("2026-09-28T16:5", self.minutes).into_bytes()
    }
}

async fn answer(status: u16, body: Vec<u8>) -> Answer {
    Ok(Response { status, body })
}

impl TradingTransport for Broker {
    fn send(&self, request: &HttpRequest) -> impl Future<Output = Answer> {
        let path = request.path_and_query();
        let lists = ["/v2/positions", "/v2/orders?", "/v2/account/activities?"];
        let asset = format!("/v2/assets/{}", self.equity.1);
        let file = |name| self.file(name);
        let (status, body) = match (request.method(), path) {
            (Method::Get, "/v2/account") => (200, file("account.json")),
            (Method::Get, "/v2/positions") if self.holds => (200, file("position-aapl.json")),
            (Method::Get, path) if path == asset => (200, file("asset-aapl.json")),
            (Method::Post, "/v2/orders") => {
                let sent = json(request.body().unwrap());
                let id = sent.get("client_order_id").and_then(Value::as_str).unwrap();
                let order = String::from_utf8(file("order.json")).unwrap();
                let order = order.replace("md-e144b97773a6f87c1978cc2831", id);
                self.posts.borrow_mut().push(sent);
                (200, order.into_bytes())
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
        answer(200, self.file("quote-aapl.json"))
    }

    fn send_bars(&self, _: &BarsRequest) -> impl Future<Output = Answer> {
        answer(200, self.file("bars-aapl.json"))
    }
}

#[derive(Clone, Copy)]
struct Clock(&'static str);

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        UtcNanos::parse_rfc3339(self.0).unwrap()
    }

    async fn pause(&self, _: Duration) {}
}

struct Fake {
    records: Vec<ControlRecord>,
    broker: Broker,
    clock: Clock,
    connects: usize,
}

impl Ports for Fake {
    type Transport = Broker;
    type Pause = Clock;

    fn control_stream(&mut self, workspace: &str) -> Result<Vec<ControlRecord>, PaperError> {
        assert_eq!(workspace, "ws_paper");
        Ok(self.records.clone())
    }

    fn connect(&mut self) -> Result<Broker, PaperError> {
        self.connects += 1;
        Ok(self.broker.clone())
    }

    fn pause(&self) -> Clock {
        self.clock
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
        let Equity(id, symbol, listing, _) = equity;
        let closes: Vec<u32> = closes.collect();
        let root =
            std::env::temp_dir().join(format!("mandate-paper-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        let quoted = format!("\"{symbol}\"");
        let mandate = spy_mandate(&model())
            .replace(SPY, id)
            .replace("\"SPY\"", &quoted);
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
        };
        let (holds, minutes, posts) = (false, TUESDAY.1, Rc::default());
        let ports = Fake {
            records: stream.records,
            broker: Broker {
                equity,
                holds,
                minutes,
                posts,
            },
            clock: Clock(TUESDAY.0),
            connects: 0,
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
        (self.ports.clock, self.ports.broker.minutes) = (Clock(now), minutes);
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
#[ignore = "pending E7-19"]
fn a_placing_run_stores_the_closes_and_sends_one_order() {
    let mut scene = Scene::new("place", 231..256);
    let outcome = scene.run();
    let Ok(Outcome::Cycle(report)) = outcome else {
        panic!("{outcome:?}")
    };
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
    let Ok(Outcome::Cycle(report)) = outcome else {
        panic!("{outcome:?}")
    };
    assert_eq!((report.submitted.len(), dry.posts().len()), (0, 0));
    assert!(report.would_place.is_some(), "a dry run reports the order");
}

/// FT-4 (DEC-157 item 4): falling closes are the host's `Flat`, so the run stores nothing, hands
/// the cycle nothing and sends nothing.
#[test]
#[ignore = "pending E7-19"]
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
        event_type,
        payload,
    });
}

/// FT-3, FT-10 (DEC-505 item 1, TI-5, rule 7): no credential is read before any of phase 1's or the
/// registrations' refusals: agent undeployed or stopped; document missing, corrupt or live; version
/// unconfirmed; a kind unregistered; a fee schedule not yet in effect; or a configured host.
#[test]
#[ignore = "pending E7-19"]
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
            "corrupt" => fs::write(store.object_path(&version), b"{}").unwrap(),
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
                DeploymentRefusal::Live => "live",
                DeploymentRefusal::Violations(_) => "unconfirmed",
                _ => "another deployment refusal",
            },
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
#[ignore = "pending E7-19"]
fn two_equities_differ_only_in_their_journaled_inputs() {
    for equity in [SPY_ARCA, AAPL_NASDAQ] {
        let Equity(id, symbol, _, _) = equity;
        let mut scene = Scene::of(symbol, equity, 231..256);
        let outcome = scene.run();
        let Ok(Outcome::Cycle(report)) = outcome else {
            panic!("{symbol}: {outcome:?}")
        };
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
#[ignore = "pending E7-19"]
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
#[ignore = "pending E7-19"]
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
#[ignore = "pending E7-19"]
fn a_run_in_the_closing_window_sends_nothing() {
    let mut scene = Scene::new("closing", 231..256);
    scene.at(CLOSING);
    let outcome = scene.run();
    let expected = "a run clock before the close window";
    assert_eq!(absent(&outcome), expected, "{outcome:?}");
    assert!(scene.posts().is_empty());
}
