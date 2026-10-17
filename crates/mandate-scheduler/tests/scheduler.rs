//! The scheduler process over the shared in-memory journal (E21-29, DEC-1010; infrastructure
//! design §3.4, OPS-14; journal spec §5.1, §5.4, §9.20): the only writer of `clock:`, fenced by
//! epoch; ticks that go on through a host clock out of tolerance or stepping back; measurements
//! journaled with their tolerance record; a retried batch never written twice.

use std::time::Duration;

use mandate_canon::{Value, parse};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_process::{Exit, State};
use mandate_scheduler::scheduler::exit;
use mandate_scheduler::store::MemoryStore;
use mandate_scheduler::{ClockStore, Config, Sample, Scheduler, TimeServers};
use mandate_time::{ExchangeCalendar, UtcNanos};

const WORKSPACE: &str = "ws_1";
/// 2026-10-16T13:00:00Z, a Friday, 09:00 in New York.
const FRIDAY_0900: i64 = 1_792_155_600;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 250_000_000).unwrap()
}

fn config() -> Config {
    Config {
        workspace: WORKSPACE.into(),
        environment: Environment::Paper,
        tolerance_ns: 50_000_000,
        liveness: Duration::from_secs(10),
    }
}

fn stream() -> StreamId {
    StreamId::parse("clock:ws_1").unwrap()
}

/// Servers that each answer with the given offset in nanoseconds, or none.
struct Fixed(Vec<i64>);

impl TimeServers for Fixed {
    fn ask(&mut self) -> Vec<Sample> {
        self.0
            .iter()
            .enumerate()
            .map(|(i, &offset_ns)| Sample {
                server: format!("time{i}.example"),
                stratum: 2,
                offset_ns,
                delay_ns: 1_000_000,
            })
            .collect()
    }
}

/// Steps a scheduler in a new monotonic second each time, as the binary's loop does.
trait StepNext {
    fn step_next(&mut self, host: UtcNanos, servers: &mut dyn TimeServers);
}

impl<S: ClockStore> StepNext for Scheduler<S> {
    fn step_next(&mut self, host: UtcNanos, servers: &mut dyn TimeServers) {
        SECOND.with(|second| {
            let next = second.get() + 1;
            second.set(next);
            self.step(host, next, servers);
        });
    }
}

thread_local! {
    static SECOND: std::cell::Cell<u64> = const { std::cell::Cell::new(0) };
}

fn start(store: &MemoryStore, now: i64) -> Scheduler<MemoryStore> {
    Scheduler::start(
        store.clone(),
        &config(),
        ExchangeCalendar::us_equities().unwrap(),
        at(now),
    )
}

fn rows(store: &MemoryStore) -> Vec<StoredEvent> {
    store.rows(&stream()).unwrap()
}

fn kinds(rows: &[StoredEvent]) -> Vec<String> {
    rows.iter().map(|r| r.event_type.clone()).collect()
}

fn payload(row: &StoredEvent) -> Value {
    parse(&row.body).unwrap().get("payload").cloned().unwrap()
}

fn member(row: &StoredEvent, name: &str) -> Value {
    payload(row).get(name).cloned().unwrap()
}

fn ticks(rows: &[StoredEvent]) -> Vec<String> {
    rows.iter()
        .filter(|r| r.event_type == "ClockAdvanced")
        .map(|r| member(r, "clock").as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_scheduler_opens_its_stream_ticks_and_measures_at_start() {
    let store = MemoryStore::default();
    let mut scheduler = start(&store, FRIDAY_0900 - 3_600);
    assert_eq!(scheduler.lifecycle().state(), State::Ready);
    scheduler.step_next(
        at(FRIDAY_0900 - 3_600),
        &mut Fixed(vec![2_000_000, -1_000_000, 3_000_000]),
    );
    let rows = rows(&store);
    assert_eq!(
        kinds(&rows),
        [
            "StreamOpened",
            "ClockAdvanced",
            "TradingDayStarted",
            "ClockOffsetRecorded"
        ],
        "a Friday's first tick starts its trading day"
    );
    assert_eq!(ticks(&rows), ["2026-10-16T12:00:00.000000000Z"]);
    let measured = &rows[3];
    assert_eq!(member(measured, "trigger").as_str(), Some("startup"));
    assert_eq!(member(measured, "offset").as_str(), Some("0.002"));
    assert_eq!(member(measured, "tolerance").as_str(), Some("0.05"));
    assert_eq!(member(measured, "within_tolerance"), Value::Bool(true));
}

#[test]
fn an_out_of_tolerance_clock_is_journaled_and_the_ticks_go_on() {
    let store = MemoryStore::default();
    let mut scheduler = start(&store, FRIDAY_0900 - 3_600);
    let mut hour_ahead = Fixed(vec![3_600_000_000_000, 3_600_000_000_000]);
    for second in 0..400 {
        scheduler.step_next(at(FRIDAY_0900 - 3_600 + second), &mut hour_ahead);
    }
    let rows = rows(&store);
    assert_eq!(ticks(&rows).len(), 400, "one tick per second, none held");
    let measured: Vec<&StoredEvent> = rows
        .iter()
        .filter(|r| r.event_type == "ClockOffsetRecorded")
        .collect();
    assert_eq!(measured.len(), 2, "at start and five minutes later");
    for m in measured {
        assert_eq!(member(m, "within_tolerance"), Value::Bool(false));
        let next = &rows[usize::try_from(m.seq).unwrap()];
        assert_eq!(next.event_type, "ClockToleranceExceeded");
        assert_eq!(member(next, "offset").as_str(), Some("3600"));
        let cause = parse(&next.body).unwrap().get("causation_id").cloned();
        assert_eq!(cause, Some(Value::Str(m.event_id.clone())));
    }
}

#[test]
fn a_measurement_no_server_answers_is_out_of_tolerance_with_no_offset() {
    let store = MemoryStore::default();
    let mut scheduler = start(&store, FRIDAY_0900);
    scheduler.step_next(at(FRIDAY_0900), &mut Fixed(Vec::new()));
    let rows = rows(&store);
    assert_eq!(
        kinds(&rows),
        [
            "StreamOpened",
            "ClockAdvanced",
            "TradingDayStarted",
            "ClockOffsetRecorded",
            "ClockToleranceExceeded"
        ]
    );
    assert_eq!(member(&rows[3], "offset"), Value::Null);
    assert_eq!(member(&rows[4], "offset"), Value::Null);
}

#[test]
fn a_host_clock_that_steps_back_never_holds_or_reverses_the_ticks() {
    let store = MemoryStore::default();
    let mut scheduler = start(&store, FRIDAY_0900);
    let good = &mut Fixed(vec![0]);
    scheduler.step_next(at(FRIDAY_0900), good);
    scheduler.step_next(at(FRIDAY_0900 - 3_600), good);
    scheduler.step_next(at(FRIDAY_0900 - 3_599), good);
    scheduler.step_next(at(FRIDAY_0900 + 10), good);
    assert_eq!(
        ticks(&rows(&store)),
        [
            "2026-10-16T13:00:00.000000000Z",
            "2026-10-16T13:00:01.000000000Z",
            "2026-10-16T13:00:02.000000000Z",
            "2026-10-16T13:00:10.000000000Z",
        ]
    );
}

#[test]
fn a_trading_day_starts_in_the_batch_of_its_first_tick() {
    let store = MemoryStore::default();
    let thursday_2359 = FRIDAY_0900 - 9 * 3_600 - 60;
    let mut scheduler = start(&store, thursday_2359);
    let good = &mut Fixed(vec![0]);
    for second in [0, 30, 60, 61] {
        scheduler.step_next(at(thursday_2359 + second), good);
    }
    let rows = rows(&store);
    let day = rows
        .iter()
        .rposition(|r| r.event_type == "TradingDayStarted")
        .unwrap();
    assert_eq!(rows[day - 1].event_type, "ClockAdvanced");
    assert_eq!(
        member(&rows[day - 1], "clock").as_str(),
        Some("2026-10-16T04:00:00.000000000Z")
    );
    assert_eq!(member(&rows[day], "date").as_str(), Some("2026-10-16"));
    assert_eq!(
        rows.iter()
            .filter(|r| r.event_type == "TradingDayStarted")
            .count(),
        2
    );
}

#[test]
fn a_second_scheduler_fences_the_first_before_it_writes_anything() {
    let store = MemoryStore::default();
    let mut first = start(&store, FRIDAY_0900);
    let good = &mut Fixed(vec![0]);
    first.step_next(at(FRIDAY_0900), good);
    let mut second = start(&store, FRIDAY_0900 + 1);
    second.step_next(at(FRIDAY_0900 + 1), good);
    let before = rows(&store);
    first.step_next(at(FRIDAY_0900 + 2), good);
    assert_eq!(exit(&first), Some(Exit::Fenced));
    assert_eq!(rows(&store), before, "the fenced scheduler wrote nothing");
    first.step_next(at(FRIDAY_0900 + 3), good);
    assert_eq!(rows(&store), before, "nor after it exited");
    second.step_next(at(FRIDAY_0900 + 2), good);
    assert_eq!(exit(&second), None);
    assert_eq!(
        kinds(&rows(&store))
            .iter()
            .filter(|k| *k == "StreamOpened")
            .count(),
        1,
        "a restart continues the stream"
    );
}

#[test]
fn a_restart_replays_and_continues_after_the_last_tick() {
    let store = MemoryStore::default();
    let mut first = start(&store, FRIDAY_0900);
    let good = &mut Fixed(vec![0]);
    for second in 0..5 {
        first.step_next(at(FRIDAY_0900 + second), good);
    }
    let mut restarted = start(&store, FRIDAY_0900 - 7_200);
    assert_eq!(
        restarted.last_tick(),
        Some(UtcNanos::from_parts(FRIDAY_0900 + 4, 0).unwrap())
    );
    restarted.step_next(at(FRIDAY_0900 - 7_200), good);
    let rows = rows(&store);
    assert_eq!(
        ticks(&rows).last().map(String::as_str),
        Some("2026-10-16T13:00:05.000000000Z")
    );
    assert_eq!(
        rows.iter()
            .filter(|r| member_str(r, "trigger") == Some("startup".into()))
            .count(),
        2,
        "a restarted scheduler measures at its own start"
    );
}

fn member_str(row: &StoredEvent, name: &str) -> Option<String> {
    payload(row)
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// A store that answers `Unavailable` to the first `failures` appends.
#[derive(Clone)]
struct Flaky {
    inner: MemoryStore,
    failures: std::rc::Rc<std::cell::Cell<u32>>,
}

impl ClockStore for Flaky {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.inner.take_ownership(stream)
    }
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.inner.rows(stream)
    }
    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let committed = self
            .inner
            .append(stream, expected_head, epoch, recorded_at, drafts);
        if self.failures.get() > 0 {
            self.failures.set(self.failures.get() - 1);
            return AppendOutcome::Unavailable;
        }
        committed
    }
}

#[test]
fn a_batch_whose_append_is_in_doubt_is_retried_as_the_same_bytes_and_written_once() {
    let inner = MemoryStore::default();
    let flaky = Flaky {
        inner: inner.clone(),
        failures: std::rc::Rc::new(std::cell::Cell::new(0)),
    };
    let mut scheduler = Scheduler::start(
        flaky.clone(),
        &config(),
        ExchangeCalendar::us_equities().unwrap(),
        at(FRIDAY_0900),
    );
    flaky.failures.set(2);
    let good = &mut Fixed(vec![0]);
    for second in 0..4 {
        scheduler.step_next(at(FRIDAY_0900 + second), good);
    }
    let rows = rows(&inner);
    let ticks = ticks(&rows);
    let mut unique = ticks.clone();
    unique.dedup();
    assert_eq!(ticks, unique, "no tick written twice");
    assert_eq!(
        ticks.first().map(String::as_str),
        Some("2026-10-16T13:00:00.000000000Z")
    );
    assert_eq!(exit(&scheduler), None);
}

#[test]
fn a_drain_writes_nothing_new_and_exits_drained() {
    let store = MemoryStore::default();
    let mut scheduler = start(&store, FRIDAY_0900);
    scheduler.step_next(at(FRIDAY_0900), &mut Fixed(vec![0]));
    let before = rows(&store);
    scheduler.drain(at(FRIDAY_0900 + 1));
    assert_eq!(exit(&scheduler), Some(Exit::Drained));
    scheduler.step_next(at(FRIDAY_0900 + 2), &mut Fixed(vec![0]));
    assert_eq!(rows(&store), before);
}

#[test]
fn a_scheduler_for_any_environment_but_paper_refuses_before_taking_the_stream() {
    let store = MemoryStore::default();
    for environment in [Environment::Live, Environment::Backtest] {
        let scheduler = Scheduler::start(
            store.clone(),
            &Config {
                environment,
                ..config()
            },
            ExchangeCalendar::us_equities().unwrap(),
            at(FRIDAY_0900),
        );
        assert_eq!(exit(&scheduler), Some(Exit::Refused));
    }
    assert!(rows(&store).is_empty());
    assert_eq!(store.0.borrow().head(&stream()).writer_epoch, 0);
}

#[test]
fn a_ready_scheduler_answers_live_and_ready_and_a_stalled_one_neither() {
    let store = MemoryStore::default();
    let scheduler = start(&store, FRIDAY_0900);
    let snapshot = scheduler.lifecycle().snapshot();
    let ask = |probe, secs| mandate_process::probe::answer(probe, snapshot, at(secs)).ok;
    assert!(ask(mandate_process::probe::Probe::Ready, FRIDAY_0900 + 5));
    assert!(!ask(mandate_process::probe::Probe::Live, FRIDAY_0900 + 11));
}

#[test]
fn a_loop_that_falls_behind_writes_one_tick_per_second_and_never_runs_ahead() {
    let store = MemoryStore::default();
    let mut scheduler = start(&store, FRIDAY_0900);
    let good = &mut Fixed(vec![0]);
    scheduler.step(at(FRIDAY_0900), 0, good);
    for _ in 0..20 {
        scheduler.step(at(FRIDAY_0900 + 20), 20, good);
    }
    scheduler.step(at(FRIDAY_0900 + 21), 21, good);
    assert_eq!(
        ticks(&rows(&store)),
        [
            "2026-10-16T13:00:00.000000000Z",
            "2026-10-16T13:00:20.000000000Z",
            "2026-10-16T13:00:21.000000000Z",
        ],
        "a stall of 20 seconds writes one tick at the host's second, not a burst"
    );
}
