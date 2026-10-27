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

#[test]
fn the_probe_loop_fails_only_a_refused_key_a_stopped_worker_or_a_stall() {
    use mandate_datad::iex::progress::{Judged, Progress, STALL_S};
    let t = |secs: i64| UtcNanos::from_parts(1_800_000_000 + secs, 0).unwrap();
    let progress = Progress::default();
    assert_eq!(progress.judged(t(0)), Judged::Starting);
    assert_eq!(
        progress.judged(t(10_000)),
        Judged::Starting,
        "no pass yet is not ready, not failed"
    );
    progress.passed(t(0));
    assert_eq!(
        progress.judged(t(1)),
        Judged::Started,
        "the start is counted once"
    );
    assert_eq!(progress.judged(t(1)), Judged::Passing);
    assert_eq!(STALL_S, 600);
    assert_eq!(
        progress.judged(t(600)),
        Judged::Passing,
        "a long paced fetch is not a stall"
    );
    assert_eq!(progress.judged(t(601)), Judged::Failed);
    progress.passed(t(601));
    assert_eq!(progress.judged(t(602)), Judged::Passing);
    progress.stopped();
    assert_eq!(progress.judged(t(602)), Judged::Failed);
}

#[test]
fn a_refused_key_on_the_minute_stops_the_process_and_minute_generations_are_pruned_once_a_minute() {
    let cell = Cell::new("minute-key");
    let fake = Fake {
        bars: Vec::new(),
        status: Some(401),
        asked: RefCell::new(0),
    };
    let root = cell.0.join("datasets").join(GENERATIONS_ROOT);
    fs::create_dir_all(root.join("minutes").join("2026-10-27T1300")).unwrap();
    let mut pass = cell.pass();
    let early = at("2026-10-27T14:30:10Z");
    let tick = pass.step(
        &fake,
        &calendar(),
        &Registered::Instruments(Vec::new()),
        early,
    );
    assert!(!tick.stop, "no instrument, nothing asked");
    assert!(
        !root.join("minutes").join("2026-10-27T1300").exists(),
        "pruned at the first pass"
    );
    fs::create_dir_all(root.join("minutes").join("2026-10-27T1301")).unwrap();
    pass.step(
        &fake,
        &calendar(),
        &Registered::Instruments(Vec::new()),
        at("2026-10-27T14:30:40Z"),
    );
    assert!(
        root.join("minutes").join("2026-10-27T1301").exists(),
        "not pruned again within the minute"
    );
    pass.step(
        &fake,
        &calendar(),
        &Registered::Instruments(Vec::new()),
        at("2026-10-27T14:31:05Z"),
    );
    assert!(
        !root.join("minutes").join("2026-10-27T1301").exists(),
        "pruned in the next minute"
    );

    let fresh = Cell::new("minute-key-2");
    let mut writer_pass = fresh.pass();
    let morning = at("2026-10-27T14:30:10Z");
    let mut daily_done = Fake {
        bars: Vec::new(),
        status: Some(503),
        asked: RefCell::new(0),
    };
    let first = writer_pass.step(&daily_done, &calendar(), &secret(), morning);
    assert!(!first.stop, "{:?}", first.log);
    daily_done.status = Some(401);
    let second = writer_pass.step(
        &daily_done,
        &calendar(),
        &secret(),
        at("2026-10-27T14:31:05Z"),
    );
    assert!(second.stop, "{:?}", second.log);
    assert_eq!(
        second.log,
        ["minute_not_written: fetch"],
        "the daily request waits RETRY_S, the minute is asked and refused"
    );
}

#[test]
fn registrations_are_read_from_after_the_last_row_and_objects_once() {
    use mandate_datad::iex::registrations::Registrations;
    use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, TrustedStart};
    struct Counting(RefCell<BTreeMap<String, Vec<u8>>>, RefCell<usize>);
    impl ArtifactSource for Counting {
        fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
            *self.1.borrow_mut() += 1;
            self.0
                .borrow()
                .get(&reference.to_string())
                .cloned()
                .ok_or(ArtifactError::Missing)
        }
    }
    let object = br#"{"asset_class":"us_equity","instrument_id":"a","symbol":"QQQ"}"#;
    let reference = ArtifactRef::of(object);
    let store = Counting(
        RefCell::new(BTreeMap::from([(reference.to_string(), object.to_vec())])),
        RefCell::new(0),
    );
    let row = |seq: u64, hash: u8| StoredEvent {
        stream_id: "ctl:ws".to_owned(),
        seq,
        event_id: format!("e{seq}"),
        event_type: "ConfigSnapshotRegistered".to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-27T12:00:00Z".to_owned(),
        prev_hash: mandate_canon::Digest::ZERO,
        hash: mandate_canon::Digest::of(&[hash]),
        body: format!(
            r#"{{"payload":{{"kind":"instrument_snapshot","content_hash":"{reference}"}}}}"#
        )
        .into_bytes(),
    };
    let mut kept = Registrations::default();
    assert_eq!(kept.next_start(), TrustedStart::GENESIS);
    kept.extend(vec![row(1, 1), row(2, 2)]);
    assert_eq!(
        kept.next_start(),
        TrustedStart {
            from_seq: 3,
            prev_hash: mandate_canon::Digest::of(&[2])
        }
    );
    let first = kept.registered(&store);
    assert!(
        matches!(&first, Registered::Instruments(list) if list.len() == 1),
        "{first:?}"
    );
    assert_eq!(
        *store.1.borrow(),
        1,
        "the object is read once for both rows naming it"
    );
    store.0.borrow_mut().clear();
    assert_eq!(kept.registered(&store), first, "and kept after");
    assert_eq!(*store.1.borrow(), 1);
}

mod worker_loop {
    use std::cell::{Cell as Counter, RefCell};
    use std::sync::atomic::{AtomicBool, Ordering};

    use mandate_datad::iex::progress::{Judged, Progress};
    use mandate_datad::iex::worker::{ControlRows, Parts, REFRESH_S, run};
    use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, TrustedStart};
    use mandate_time::UtcNanos;

    use super::{Cell, Fake, calendar};

    /// A control stream of no rows that records where each read started.
    #[derive(Default)]
    struct Rows(RefCell<Vec<TrustedStart>>);

    impl ControlRows for Rows {
        fn rows_from(&self, start: TrustedStart) -> Result<Vec<StoredEvent>, &'static str> {
            self.0.borrow_mut().push(start);
            Ok(Vec::new())
        }
    }

    struct NoObjects;

    impl ArtifactSource for NoObjects {
        fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
            Err(ArtifactError::Missing)
        }
    }

    /// Runs the worker over `fake`, the clock one second further at each pause, until `turns`
    /// pauses have passed; the reads it made, its log, and the progress it left.
    fn turns(cell: &Cell, fake: &Fake, turns: u32) -> (Vec<TrustedStart>, Vec<String>, Progress) {
        let rows = Rows::default();
        let calendar = calendar();
        let clock = Counter::new(1_793_115_000_i64);
        let paused = Counter::new(0u32);
        let terminate = AtomicBool::new(false);
        let progress = Progress::default();
        let mut log = Vec::new();
        run(
            Parts {
                source: fake,
                control: &rows,
                artifacts: &NoObjects,
                calendar: &calendar,
                pass: cell.pass(),
            },
            &terminate,
            &progress,
            &|| UtcNanos::from_parts(clock.get(), 0).unwrap(),
            &|| {
                clock.set(clock.get() + 1);
                paused.set(paused.get() + 1);
                if paused.get() >= turns {
                    terminate.store(true, Ordering::SeqCst);
                }
            },
            &mut |line| log.push(line.to_owned()),
        );
        let reads = rows.0.into_inner();
        (reads, log, progress)
    }

    #[test]
    fn the_worker_reads_every_minute_passes_each_turn_and_marks_its_end() {
        let cell = Cell::new("worker");
        let fake = Fake {
            bars: Vec::new(),
            status: None,
            asked: RefCell::new(0),
        };
        let (reads, log, progress) = turns(&cell, &fake, 125);
        assert_eq!(REFRESH_S, 60);
        assert_eq!(
            reads.len(),
            3,
            "at the start, a minute on and two minutes on"
        );
        assert!(
            reads.iter().all(|start| *start == TrustedStart::GENESIS),
            "no rows, so each read starts at the stream's start"
        );
        assert!(log.is_empty(), "{log:?}");
        assert_eq!(
            progress.judged(UtcNanos::from_parts(1_793_115_200, 0).unwrap()),
            Judged::Failed,
            "a worker that has left its loop is stopped, so the probe loop never waits on it"
        );
    }

    #[test]
    fn a_refused_key_ends_the_worker_at_once() {
        let cell = Cell::new("worker-key");
        let fake = Fake {
            bars: Vec::new(),
            status: Some(401),
            asked: RefCell::new(0),
        };
        let calendar = calendar();
        let terminate = AtomicBool::new(false);
        let progress = Progress::default();
        let mut log = Vec::new();
        let paused = Counter::new(0u32);
        let snapshot = br#"{"asset_class":"us_equity","instrument_id":"a","symbol":"QQQ"}"#;
        struct One(Vec<u8>);
        impl ArtifactSource for One {
            fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
                Ok(self.0.clone())
            }
        }
        struct Registering(String);
        impl ControlRows for Registering {
            fn rows_from(&self, start: TrustedStart) -> Result<Vec<StoredEvent>, &'static str> {
                if start != TrustedStart::GENESIS {
                    return Ok(Vec::new());
                }
                Ok(vec![StoredEvent {
                    stream_id: "ctl:ws".to_owned(),
                    seq: 1,
                    event_id: "e1".to_owned(),
                    event_type: "ConfigSnapshotRegistered".to_owned(),
                    schema_version: 1,
                    environment: "paper".to_owned(),
                    recorded_at: "2026-10-27T12:00:00Z".to_owned(),
                    prev_hash: mandate_canon::Digest::ZERO,
                    hash: mandate_canon::Digest::ZERO,
                    body: format!(
                        r#"{{"payload":{{"kind":"instrument_snapshot","content_hash":"{}"}}}}"#,
                        self.0
                    )
                    .into_bytes(),
                }])
            }
        }
        run(
            Parts {
                source: &fake,
                control: &Registering(ArtifactRef::of(snapshot).to_string()),
                artifacts: &One(snapshot.to_vec()),
                calendar: &calendar,
                pass: cell.pass(),
            },
            &terminate,
            &progress,
            &|| super::at("2026-10-27T14:30:10Z"),
            &|| paused.set(paused.get() + 1),
            &mut |line| log.push(line.to_owned()),
        );
        assert_eq!(
            paused.get(),
            0,
            "the worker left at its first pass, before any pause"
        );
        assert_eq!(log, ["daily_not_written: fetch"]);
        assert_eq!(
            progress.judged(super::at("2026-10-27T14:30:11Z")),
            Judged::Failed
        );
    }

    #[test]
    fn a_worker_that_panics_is_marked_stopped() {
        let cell = Cell::new("worker-panic");
        let fake = Fake {
            bars: Vec::new(),
            status: None,
            asked: RefCell::new(0),
        };
        let rows = Rows::default();
        let calendar = calendar();
        let terminate = AtomicBool::new(false);
        let progress = Progress::default();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run(
                Parts {
                    source: &fake,
                    control: &rows,
                    artifacts: &NoObjects,
                    calendar: &calendar,
                    pass: cell.pass(),
                },
                &terminate,
                &progress,
                &|| super::at("2026-10-27T14:30:10Z"),
                &|| panic!("the pause fails"),
                &mut |_| {},
            );
        }));
        assert!(unwound.is_err());
        assert_eq!(
            progress.judged(super::at("2026-10-27T14:30:11Z")),
            Judged::Failed,
            "a panic before any pass still fails the process rather than leaving it starting"
        );
    }
}
