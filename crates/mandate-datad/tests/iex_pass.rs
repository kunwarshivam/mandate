//! DEC-2283, data-plane spec §3.8 and DP-17: one pass of the data process's IEX source writes the
//! generations due at its clock and puts their closes objects; it reports counts and codes only,
//! never a symbol or a price; a refused platform key stops the process; and the batched fetch
//! behind it is the client's. The oracles are the log lines and requests written out by hand.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use mandate_bars::generations::GENERATIONS_ROOT;
use mandate_canon::DecStr;
use mandate_datad::iex::generation::{BarsSource, Writer};
use mandate_datad::iex::instruments::{Instrument, Registered};
use mandate_datad::iex::pass::Pass;
use mandate_datad::iex::schedule::{Jitter, span};
use mandate_datad::iex::source::ClientSource;
use mandate_marketdata::batch::BarsBatch;
use mandate_marketdata::client::{
    Client, FetchError, Pause, RateHeaders, Response, Transport, TransportError,
};
use mandate_marketdata::model::{Bar, CorporateActions, DayRange, Symbol, TimeUnit, Timeframe};
use mandate_time::{ExchangeCalendar, UtcNanos, new_york_midnight};

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn calendar() -> ExchangeCalendar {
    ExchangeCalendar::us_equities().unwrap()
}

const NONE: Jitter = Jitter {
    daily_s: 0,
    minute_s: 0,
};

fn bar(start: UtcNanos, close: &str) -> Bar {
    let price = DecStr::parse(close).unwrap();
    Bar {
        start,
        open: price.clone(),
        high: price.clone(),
        low: price.clone(),
        close: price.clone(),
        volume: DecStr::parse("1000").unwrap(),
        vwap: price,
        trade_count: 1,
    }
}

/// Serves `bars` within the asked span, or a status.
struct Fake {
    bars: Vec<Bar>,
    status: Option<u16>,
    asked: RefCell<usize>,
}

impl BarsSource for Fake {
    fn bars(&self, batch: &BarsBatch) -> Result<BTreeMap<Symbol, Vec<Bar>>, FetchError> {
        *self.asked.borrow_mut() += 1;
        if let Some(status) = self.status {
            return Err(FetchError::Status { status });
        }
        Ok(batch
            .symbols()
            .iter()
            .map(|s| {
                let mine = self
                    .bars
                    .iter()
                    .filter(|b| b.start >= batch.start() && b.start < batch.end())
                    .cloned()
                    .collect();
                (s.clone(), mine)
            })
            .collect())
    }

    fn actions(
        &self,
        symbols: &BTreeSet<Symbol>,
        _: DayRange,
    ) -> Result<BTreeMap<Symbol, CorporateActions>, FetchError> {
        if let Some(status) = self.status {
            return Err(FetchError::Status { status });
        }
        Ok(symbols
            .iter()
            .map(|s| (s.clone(), CorporateActions::none(s.clone())))
            .collect())
    }
}

struct Cell(PathBuf);

impl Cell {
    fn new(name: &str) -> Self {
        let dir =
            std::env::temp_dir().join(format!("mandate-datad-pass-{name}-{}", std::process::id()));
        fs::remove_dir_all(&dir).ok();
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    fn pass(&self) -> Pass {
        let writer = Writer::new(
            self.0.join("datasets").join(GENERATIONS_ROOT),
            self.0.join("artifacts"),
        )
        .unwrap();
        Pass::new(writer, NONE)
    }
}

impl Drop for Cell {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).ok();
    }
}

fn secret() -> Registered {
    Registered::Instruments(vec![Instrument {
        asset_id: "asset-secret".to_owned(),
        symbol: Symbol::parse("SECRETCO").unwrap(),
    }])
}

/// DP-17: a pass that writes a daily generation, its closes object and a minute generation logs
/// counts, codes and the object's reference, and nothing that names the instrument or a price.
#[test]
fn a_pass_writes_what_is_due_and_logs_no_symbol_or_price() {
    let cell = Cell::new("logs");
    let session = mandate_time::Date::parse("2026-10-26").unwrap();
    let mut bars: Vec<Bar> = span(&calendar(), session)
        .unwrap()
        .iter()
        .enumerate()
        .map(|(n, d)| bar(new_york_midnight(*d).unwrap(), &format!("7{n:02}.25")))
        .collect();
    bars.push(bar(at("2026-10-27T14:28:00Z"), "733.75"));
    let fake = Fake {
        bars,
        status: None,
        asked: RefCell::new(0),
    };
    let mut pass = cell.pass();
    let tick = pass.step(&fake, &calendar(), &secret(), at("2026-10-27T14:30:10Z"));
    assert!(!tick.stop);
    let closes: Vec<&String> = tick
        .log
        .iter()
        .filter(|l| l.starts_with("closes_stored sha256:"))
        .collect();
    assert_eq!(closes.len(), 1, "{:?}", tick.log);
    let rest: Vec<&str> = tick
        .log
        .iter()
        .filter(|l| !l.starts_with("closes_stored"))
        .map(String::as_str)
        .collect();
    assert_eq!(rest, ["daily_written 1", "minute_written 1"]);
    for line in &tick.log {
        assert!(
            !line.contains("SECRETCO")
                && !line.contains("asset-secret")
                && !line.contains(".25")
                && !line.contains("733"),
            "{line}"
        );
    }
    let again = pass.step(&fake, &calendar(), &secret(), at("2026-10-27T14:30:20Z"));
    assert!(
        again.log.is_empty(),
        "nothing due is nothing logged: {:?}",
        again.log
    );
}

#[test]
fn a_refused_platform_key_stops_the_process_and_another_failure_does_not() {
    for (status, stops) in [(401, true), (403, true), (503, false)] {
        let cell = Cell::new(&format!("key-{status}"));
        let fake = Fake {
            bars: Vec::new(),
            status: Some(status),
            asked: RefCell::new(0),
        };
        let tick = cell
            .pass()
            .step(&fake, &calendar(), &secret(), at("2026-10-27T13:00:00Z"));
        assert_eq!(tick.stop, stops, "{status}");
        assert_eq!(tick.log, ["daily_not_written: fetch"], "{status}");
    }
}

#[test]
fn a_workspace_that_is_not_paper_or_unreadable_is_said_once_and_asks_nothing() {
    let cell = Cell::new("not-paper");
    let fake = Fake {
        bars: Vec::new(),
        status: None,
        asked: RefCell::new(0),
    };
    let mut pass = cell.pass();
    let now = at("2026-10-27T14:30:10Z");
    assert_eq!(
        pass.step(&fake, &calendar(), &Registered::NotPaper, now)
            .log,
        ["not_paper"]
    );
    assert!(
        pass.step(&fake, &calendar(), &Registered::NotPaper, now)
            .log
            .is_empty()
    );
    assert_eq!(
        pass.step(&fake, &calendar(), &Registered::Unreadable, now)
            .log,
        ["snapshots_unreadable"]
    );
    assert_eq!(*fake.asked.borrow(), 0);
}

/// A transport answering each request with the next scripted body, recording what was asked.
#[derive(Default)]
struct Scripted {
    bodies: RefCell<VecDeque<Vec<u8>>>,
    asked: RefCell<Vec<String>>,
}

impl Transport for Scripted {
    async fn get(&self, path_and_query: &str) -> Result<Response, TransportError> {
        self.asked.borrow_mut().push(path_and_query.to_owned());
        let body = self.bodies.borrow_mut().pop_front().unwrap();
        Ok(Response {
            status: 200,
            rate: RateHeaders::default(),
            body,
        })
    }
}

/// A clock that stands still and waits no time.
struct Still;

impl Pause for Still {
    fn now(&self) -> UtcNanos {
        at("2026-10-27T00:20:00Z")
    }

    async fn pause(&self, _: Duration) {}
}

#[test]
fn the_client_source_is_the_batched_fetch() {
    let transport = Scripted::default();
    transport.bodies.borrow_mut().extend([
        br#"{"bars":{"QQQ":[{"c":1,"h":1,"l":1,"n":1,"o":1,"t":"2026-10-26T04:00:00Z","v":1,"vw":1}]},"next_page_token":null}"#.to_vec(),
        br#"{"corporate_actions":{},"next_page_token":null}"#.to_vec(),
    ]);
    let source = ClientSource::new(Client::new(transport, Still)).unwrap();
    let symbols: BTreeSet<Symbol> = [Symbol::parse("QQQ").unwrap()].into_iter().collect();
    let batch = BarsBatch::new(
        symbols.clone(),
        Timeframe::new(1, TimeUnit::Day).unwrap(),
        at("2026-10-26T04:00:00Z"),
        at("2026-10-27T04:00:00Z"),
    )
    .unwrap();
    let bars = source.bars(&batch).unwrap();
    assert_eq!(bars[&Symbol::parse("QQQ").unwrap()].len(), 1);
    let range = DayRange::new(
        mandate_time::Date::parse("2026-10-26").unwrap(),
        mandate_time::Date::parse("2026-10-26").unwrap(),
    )
    .unwrap();
    assert!(source.actions(&symbols, range).unwrap()[&Symbol::parse("QQQ").unwrap()].is_empty());
}
