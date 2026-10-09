//! The paper adapter's one run (E7-19 slice 5, E1a, DEC-846): SPY deployed as the shell's tests
//! deploy it, a real store and bars, a scripted broker, and no network (ES-19, rule 8).

#[path = "../../mandate-shell/tests/common/mod.rs"]
mod common;

use common::{FEE, RULES, SNAPSHOT, SPY, Stream, json, model, spy_mandate};
use mandate_alpaca::{BarsRequest, DataTransport, HttpRequest, Method, Pause, QuoteRequest};
use mandate_alpaca::{Response, TradingTransport, TransportError};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{DecStr, Value, to_canonical};
use mandate_journal::{ArtifactRef, ArtifactStore, get_artifact};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, Bar, DatasetId, Feed, Kind, Records, Symbol};
use mandate_modelhost::Signal;
use mandate_paper::{Args, Outcome, PaperError, Ports, run};
use mandate_shell::control::ControlRecord;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

/// Tuesday 13:00 in New York: Monday 2026-09-28 is the last completed session.
const NOW: &str = "2026-09-29T17:00:00Z";
const ALPACA: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../mandate-shell/tests/fixtures/"
);

type Answer = Result<Response, TransportError>;

/// A paper broker whose answers are `mandate-shell`'s recorded AAPL ones moved to SPY and to
/// `NOW`'s day. It accepts a submission and keeps every `POST /v2/orders` body it is handed.
#[derive(Clone, Default)]
struct Broker(Rc<RefCell<Vec<Value>>>);

fn file(name: &str) -> Vec<u8> {
    let text = fs::read_to_string(format!("{ALPACA}tracer/alpaca/{name}")).unwrap();
    let text = text.replace(common::AAPL, SPY).replace("AAPL", "SPY");
    let text = text.replace("NASDAQ", "ARCA");
    text.replace("2026-09-28T16:5", "2026-09-29T16:5")
        .into_bytes()
}

async fn answer(status: u16, body: Vec<u8>) -> Answer {
    Ok(Response { status, body })
}

impl TradingTransport for Broker {
    fn send(&self, request: &HttpRequest) -> impl Future<Output = Answer> {
        let path = request.path_and_query();
        let lists = ["/v2/positions", "/v2/orders?", "/v2/account/activities?"];
        let (status, body) = match (request.method(), path) {
            (Method::Get, "/v2/account") => (200, file("account.json")),
            (Method::Get, "/v2/assets/SPY") => (200, file("asset-aapl.json")),
            (Method::Post, "/v2/orders") => {
                let sent = json(request.body().unwrap());
                let id = sent.get("client_order_id").and_then(Value::as_str).unwrap();
                let order = String::from_utf8(file("order.json")).unwrap();
                let order = order.replace("md-e144b97773a6f87c1978cc2831", id);
                self.0.borrow_mut().push(sent);
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
        answer(200, file("quote-aapl.json"))
    }

    fn send_bars(&self, _: &BarsRequest) -> impl Future<Output = Answer> {
        answer(200, file("bars-aapl.json"))
    }
}

#[derive(Clone, Copy)]
struct Clock;

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        UtcNanos::parse_rfc3339(NOW).unwrap()
    }

    async fn pause(&self, _: Duration) {}
}

struct Fake {
    records: Vec<ControlRecord>,
    broker: Broker,
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
        Clock
    }
}

/// SPY deployed on the control stream, its objects stored, and `closes` its daily bars, one a
/// session through Monday 2026-09-28: rising a dollar a session to 255.2 is the host's `Long`.
struct Scene {
    root: PathBuf,
    args: Args,
    ports: Fake,
    closes: Vec<(Date, String)>,
}

impl Scene {
    fn new(name: &str, closes: impl Iterator<Item = u32>) -> Scene {
        let closes: Vec<u32> = closes.collect();
        let root =
            std::env::temp_dir().join(format!("mandate-paper-{name}-{}", std::process::id()));
        fs::remove_dir_all(&root).ok();
        let snapshot = SNAPSHOT.replace("2026-09-21", "2026-09-25");
        let stream = Stream::of(&spy_mandate(&model()), &snapshot, RULES, FEE, &model());
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
        let spy = Symbol::parse("SPY").unwrap();
        let dataset = DatasetId::new(AssetClass::UsEquity, Feed::Iex, kind, spy).unwrap();
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
        let ports = Fake {
            records: stream.records,
            broker: Broker::default(),
            connects: 0,
        };
        Scene {
            root,
            args,
            ports,
            closes,
        }
    }

    fn run(&mut self) -> Result<Outcome, PaperError> {
        run(&self.args, &[], &mut self.ports)
    }

    fn posts(&self) -> Vec<Value> {
        self.ports.broker.0.borrow().clone()
    }

    /// The observation's data as DEC-846 item 3 shapes it, computed here from the bars written.
    fn closes_bytes(&self) -> Vec<u8> {
        let row = |(day, close): &(Date, String)| format!(r#"["{day}","{close}"]"#);
        let rows: Vec<String> = self.closes.iter().map(row).collect();
        let rows = rows.join(",");
        let text = format!(r#"{{"closes":[{rows}],"instrument_id":"{SPY}"}}"#);
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
/// pinned asset, through the cycle. A dry run sends nothing and reports the order it would place.
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
