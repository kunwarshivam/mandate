//! E1b's bounded watch (FT-11, DEC-853 items 5 and 6, DEC-858): the bound and the poll interval
//! (part 1a), and the watch over SPY's one submission (part 1b), against a scripted broker and a
//! clock that moves only when the watch pauses (ES-19). Expected values are typed here, never
//! computed by the code under test.

mod common;

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use mandate_alpaca::TransportError;
use mandate_alpaca::{Exchange, HttpRequest, Method, Pause, Response, TradingTransport};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_executor::ExecutorConfig;
use mandate_journal::{ArtifactSource, StoredEvent};
use mandate_modelhost::{DailyCloses, Evaluation, Pin, evaluate};
use mandate_num::Price;
use mandate_runtime::{ModelOutput, Observation};
use mandate_shell::adapters::{Sources, production};
use mandate_shell::paper::load_contexts;
use mandate_shell::stages::Stages;
use mandate_shell::{Cause, Report, Setup, ShellError, Watch, close_window_bound};
use mandate_shell::{poll_interval, production_cycle, run_observed_watched};
use mandate_spec::validate::RegisteredModel;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

use common::{AAPL, FEE, RULES, SNAPSHOT, SPY, Stream, json, model, spy_facts, spy_mandate};

/// Tuesday 2026-09-29, a full session: 13:00 and 15:50 in New York.
const TUESDAY: &str = "2026-09-29T17:00:00Z";
const BOUND: &str = "2026-09-29T19:50:00Z";
/// Twenty seconds before the bound.
const LATE: &str = "2026-09-29T19:49:40Z";

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

static MANDATE_FILE: OnceLock<PathBuf> = OnceLock::new();

/// SPY's mandate with its order cap raised to the allocation, as `observed.rs` runs it.
fn mandate() -> String {
    spy_mandate(&model()).replace(r#""max_order_usd": "300""#, r#""max_order_usd": "1000""#)
}

fn stream() -> Stream {
    classified_on("2026-09-25")
}

/// The stream with SPY's snapshot classified on `day`, so it is fresh for a run a few days later.
fn classified_on(day: &str) -> Stream {
    let snapshot = SNAPSHOT.replace("2026-09-21", day);
    Stream::of(&mandate(), &snapshot, RULES, FEE, &model())
}

/// The scripted clock, in whole seconds, and every pause the watch asked for.
#[derive(Clone)]
struct Clock {
    secs: Rc<Cell<i64>>,
    pauses: Rc<RefCell<Vec<Duration>>>,
}

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        UtcNanos::from_parts(self.secs.get(), 0).unwrap()
    }

    async fn pause(&self, duration: Duration) {
        let taken = self.pauses.borrow().len();
        assert!(
            taken < PAUSE_BUDGET,
            "the watch paused more than its budget of {PAUSE_BUDGET} pauses: a loop that never ends"
        );
        self.pauses.borrow_mut().push(duration);
        let step = i64::try_from(duration.as_secs()).unwrap();
        self.secs.set(self.secs.get() + step);
    }
}

/// More pauses than any case here needs (the longest, the cap's, takes 124 from 15:49:40 to 16:00,
/// and as many from 12:49:40 to 13:00 on an early close),
/// so a watch that never ends fails at once rather than running until the harness times it out
/// (#1041 review).
const PAUSE_BUDGET: usize = 130;

/// How the scripted broker reads the entry back (DEC-858 items 4 and 5).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// `accepted`, then `canceled` once deleted.
    Rests,
    /// `filled` for its whole quantity at the first read.
    Whole,
    /// `accepted`, then `filled` once deleted: the fill wins the race with the bound's cancel.
    Races,
    /// `accepted`, then `pending_cancel` for ever once deleted: the cancel is never confirmed.
    Stuck,
    /// `accepted` at the first read, then `filled` for its whole quantity from the second.
    Second,
    /// `expired` at the first read.
    Expires,
    /// `rejected` at the first read.
    Rejected,
}

/// A paper broker that accepts every submission and reads the entry, the first, back as `fill`
/// says. It logs every request after the entry with the clock's second, and keeps every body
/// posted.
#[derive(Clone)]
struct Broker {
    clock: Rc<Cell<i64>>,
    fill: Fill,
    submitted: Rc<RefCell<Option<String>>>,
    deleted: Rc<Cell<bool>>,
    /// How many times the entry has been read back.
    reads: Rc<Cell<usize>>,
    log: Rc<RefCell<Vec<(Method, String, i64)>>>,
    posts: Rc<RefCell<Vec<Value>>>,
}

impl Broker {
    fn order(&self, sent: &Value, status: &str, filled: &str) -> Vec<u8> {
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tracer/alpaca");
        let order = fs::read_to_string(format!("{dir}/order.json")).unwrap();
        let field = |name: &str| sent.get(name).and_then(Value::as_str).unwrap().to_owned();
        let order = order.replace("md-e144b97773a6f87c1978cc2831", &field("client_order_id"));
        let order = order.replace(AAPL, SPY).replace(r#""AAPL""#, r#""SPY""#);
        let order = order.replace(r#""qty":"1""#, &format!(r#""qty":"{}""#, field("qty")));
        let order = order.replace(r#""side":"buy""#, &format!(r#""side":"{}""#, field("side")));
        let order = order.replace(
            r#""filled_qty":"0""#,
            &format!(r#""filled_qty":"{filled}""#),
        );
        order
            .replace(r#""status":"accepted""#, &format!(r#""status":"{status}""#))
            .into_bytes()
    }

    /// The entry as `fill` reads it now.
    fn entry(&self, entry: &Value) -> Vec<u8> {
        let qty = entry.get("qty").and_then(Value::as_str).unwrap().to_owned();
        self.reads.set(self.reads.get() + 1);
        let first = self.reads.get() == 1;
        let (status, filled) = match (self.fill, self.deleted.get()) {
            (Fill::Whole, _) | (Fill::Races, true) => ("filled", qty.as_str()),
            (Fill::Second, _) if !first => ("filled", qty.as_str()),
            (Fill::Expires, _) => ("expired", "0"),
            (Fill::Rejected, _) => ("rejected", "0"),
            (Fill::Rests, true) => ("canceled", "0"),
            (Fill::Stuck, true) => ("pending_cancel", "0"),
            (Fill::Rests | Fill::Races | Fill::Stuck | Fill::Second, _) => ("accepted", "0"),
        };
        self.order(entry, status, filled)
    }

    fn answer(&self, request: &HttpRequest) -> (u16, Vec<u8>) {
        let path = request.path_and_query();
        if self.submitted.borrow().is_some() {
            self.log
                .borrow_mut()
                .push((request.method(), path.to_owned(), self.clock.get()));
            if request.method() == Method::Delete {
                self.deleted.set(true);
                return (204, Vec::new());
            }
            let entry = self.posts.borrow()[0].clone();
            let id = entry
                .get("client_order_id")
                .and_then(Value::as_str)
                .unwrap();
            if path.starts_with("/v2/orders:by_client_order_id") && path.ends_with(id) {
                return (200, self.entry(&entry));
            }
        }
        let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/tracer/alpaca");
        let fixture = |name: &str| fs::read(format!("{dir}/{name}")).unwrap();
        if path.starts_with("/v2/orders:by_client_order_id") {
            (404, fixture("order-absent.json"))
        } else if path == "/v2/account" {
            (200, fixture("account.json"))
        } else if request.method() == Method::Post && path == "/v2/orders" {
            let sent = json(request.body().unwrap());
            let id = sent.get("client_order_id").and_then(Value::as_str).unwrap();
            self.submitted.borrow_mut().get_or_insert(id.to_owned());
            self.posts.borrow_mut().push(sent.clone());
            (200, self.order(&sent, "accepted", "0"))
        } else if ["/v2/positions", "/v2/orders?", "/v2/account/activities?"]
            .iter()
            .any(|prefix| path.starts_with(prefix))
        {
            (200, b"[]".to_vec())
        } else {
            panic!("the scripted broker has no answer for {path}")
        }
    }
}

impl TradingTransport for Broker {
    fn send(
        &self,
        request: &HttpRequest,
    ) -> impl Future<Output = Result<Response, TransportError>> {
        let (status, body) = self.answer(request);
        async move { Ok(Response { status, body }) }
    }
}

/// The host's `Long` on 25 rising closes to `now`, and the store holding them (DEC-846 item 3).
fn observed(now: &str) -> (Observation, ModelOutput, BTreeMap<Digest, Vec<u8>>) {
    let calendar = ExchangeCalendar::us_equities().unwrap();
    let last = calendar.last_completed_regular_session(at(now)).unwrap();
    let (mut days, mut day) = (Vec::new(), Date::parse("2026-08-03").unwrap());
    while Some(day) <= last {
        days.extend(calendar.is_trading_day(day).unwrap().then_some(day));
        day = day.next().unwrap();
    }
    let closes: Vec<(Date, Price)> = (631_u32..)
        .zip(&days[days.len() - 25..])
        .map(|(close, day)| (*day, Price::parse(&format!("{close}.2")).unwrap()))
        .collect();
    let rows: Vec<String> = closes
        .iter()
        .map(|(d, p)| format!(r#"["{d}","{p}"]"#))
        .collect();
    let bytes = format!(
        r#"{{"closes":[{}],"instrument_id":"{SPY}"}}"#,
        rows.join(",")
    );
    let document = mandate_spec::Mandate::parse(&json(&mandate())).unwrap();
    let model = document.behavior.signal_models[0].clone();
    let entry = RegisteredModel {
        version: model.version.clone(),
        content_hash: model.content_hash,
        params: ["fast_periods", "slow_periods"].map(str::to_owned).into(),
        admits_instruments: false,
    };
    let registry = BTreeMap::from([(model.id.clone(), entry)]);
    let id = mandate_accounting::InstrumentId::new(SPY).unwrap();
    let pin = Pin {
        model,
        instrument_id: id.clone(),
    };
    let daily = DailyCloses {
        instrument_id: id.clone(),
        closes,
    };
    let Ok(Evaluation::Long(output)) = evaluate(&pin, &registry, &calendar, &daily, at(now)) else {
        panic!("rising closes are the host's Long at {now}");
    };
    let observation = Observation {
        source: "alpaca_iex_daily_bars".to_owned(),
        instrument_id: Some(id),
        as_of: output.as_of,
        data_ref: Digest::of(bytes.as_bytes()),
    };
    let store = BTreeMap::from([(Digest::of(bytes.as_bytes()), bytes.into_bytes())]);
    (observation, *output, store)
}

/// One watched run at `now`, bounded at 15:50, at the reviewed five-second interval.
struct Watched {
    outcome: Result<Report, ShellError>,
    /// The run's stages, which only the free function hands back: the production door keeps its
    /// own (DEC-475).
    stages: Option<Stages>,
    broker: Broker,
    clock: Clock,
}

/// The two ways into the watch: the free function and the production door.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Door {
    Function,
    Cycle,
}

fn watched(now: &str, place: bool) -> Watched {
    watched_through(now, place, Door::Function, Fill::Rests)
}

fn watched_through(now: &str, place: bool, door: Door, fill: Fill) -> Watched {
    watched_to(now, BOUND, place, door, fill)
}

/// One watched run at `now`, bounded at `bound`.
fn watched_to(now: &str, bound: &str, place: bool, door: Door, fill: Fill) -> Watched {
    let day = Date::parse(&now[..10]).unwrap();
    let stream = match &now[..10] {
        "2026-12-24" => classified_on("2026-12-22"),
        _ => stream(),
    };
    let artifacts = stream.artifacts_on(day).unwrap();
    let deployed = artifacts.deployment("ws_spy".into(), "agent_spy".into(), "acct_spy".into());
    let deployed = deployed.unwrap();
    let agent = deployed.deployment().agent.clone();
    let facts = spy_facts(SPY, "SPY", Exchange::Arca, at(now));
    let contexts = load_contexts(&artifacts, &facts, at(now), &agent).unwrap();
    let file = MANDATE_FILE.get_or_init(|| {
        let path =
            std::env::temp_dir().join(format!("mandate-shell-e1b-{}.json", std::process::id()));
        fs::write(&path, to_canonical(&json(&mandate()))).unwrap();
        path
    });
    let secs = Rc::new(Cell::new(at(now).secs()));
    let clock = Clock {
        secs: Rc::clone(&secs),
        pauses: Rc::default(),
    };
    let broker = Broker {
        clock: secs,
        fill,
        submitted: Rc::default(),
        deleted: Rc::default(),
        reads: Rc::default(),
        log: Rc::default(),
        posts: Rc::default(),
    };
    let (observation, output, store) = observed(now);
    let sources = Sources {
        mandate: file.clone(),
        dataset: PathBuf::from("the-watched-cycle-reads-no-dataset"),
        journal: None,
        recorded_at: at(now),
        agent,
        workspace: "ws_spy".to_owned(),
        account_ref: "acct_spy".to_owned(),
        executor: Some(contexts.executor),
        run: Some(contexts.run),
        artifacts: Some(Arc::new(store) as Arc<dyn ArtifactSource + Send + Sync>),
        transport: broker.clone(),
    };
    let setup = Setup {
        deployment: deployed.deployment().clone(),
        account_ref: "acct_spy".to_owned(),
        now: at(now),
        place_one_order: place,
        new_cycle: false,
    };
    let watch = Watch {
        pause: clock.clone(),
        bound: at(bound),
        interval: Duration::from_secs(5),
    };
    let (outcome, stages) = match door {
        Door::Function => {
            let mut stages = production(sources);
            let outcome = run_observed_watched(&mut stages, &setup, observation, output, &watch);
            (outcome, Some(stages))
        }
        Door::Cycle => {
            let mut cycle = production_cycle(sources, setup);
            (
                cycle.run_observed_watched(observation, output, &watch),
                None,
            )
        }
    };
    Watched {
        outcome,
        stages,
        broker,
        clock,
    }
}

impl Watched {
    /// The second of each request after the submission that `keep` selects.
    fn at(&self, keep: fn(&Method, &str) -> bool) -> Vec<i64> {
        let log = self.broker.log.borrow();
        log.iter()
            .filter(|(m, path, _)| keep(m, path))
            .map(|(.., secs)| *secs)
            .collect()
    }

    /// The account stream's rows, each as its type and its body's text.
    fn account(&self) -> Vec<(String, String)> {
        let stages = self.stages.as_ref().unwrap();
        let rows: Vec<StoredEvent> = stages.journal.read("acct:ws_spy:acct_spy").unwrap();
        let text = |row: &StoredEvent| String::from_utf8_lossy(&row.body).into_owned();
        rows.iter()
            .map(|row| (row.event_type.clone(), text(row)))
            .collect()
    }

    /// Whether the account stream records the entry's `pending_cancel`.
    fn cancel_journaled(&self) -> bool {
        let id = self.broker.submitted.borrow().clone().unwrap();
        let stages = self.stages.as_ref().unwrap();
        let rows: Vec<StoredEvent> = stages.journal.read("acct:ws_spy:acct_spy").unwrap();
        rows.iter().any(|row| {
            let body = String::from_utf8_lossy(&row.body);
            row.event_type == "OrderStateChanged"
                && body.contains(&id)
                && body.contains(r#""state":"pending_cancel""#)
        })
    }
}

fn read(method: &Method, path: &str) -> bool {
    *method == Method::Get && path.starts_with("/v2/orders:by_client")
}

fn delete(method: &Method, _: &str) -> bool {
    *method == Method::Delete
}

fn gate() -> mandate_risk::GateConfig {
    let artifacts = stream()
        .artifacts_on(Date::parse("2026-09-29").unwrap())
        .unwrap();
    artifacts.production_configuration().unwrap().gate.clone()
}

/// DEC-853 item 5: the bound is the session's end less `close_window_minutes`: 15:50 New York on a
/// full day, 12:50 on the day after Thanksgiving (an early close at 13:00), and 15:45 with a
/// fifteen-minute window.
#[test]
fn the_bound_is_the_close_window_start_of_the_runs_session() {
    let gate = gate();
    assert_eq!(gate.close_window_minutes, 10, "the reviewed rule set");
    let mut wider = gate.clone();
    wider.close_window_minutes = 15;
    let cases = [
        (TUESDAY, &gate, BOUND),
        ("2026-11-27T16:00:00Z", &gate, "2026-11-27T17:50:00Z"),
        (TUESDAY, &wider, "2026-09-29T19:45:00Z"),
    ];
    for (now, gate, bound) in cases {
        let found = close_window_bound(at(now), gate).unwrap_or_else(|e| panic!("{now}: {e:?}"));
        assert_eq!(found, at(bound), "{now} {}", gate.close_window_minutes);
    }
}

/// DEC-853 item 5, FT-8: no bound outside a regular session or inside its close window: pre-market,
/// 15:50 itself, 15:55, and a Saturday are each refused as absent.
#[test]
fn there_is_no_bound_outside_a_session_before_its_close_window() {
    let gate = gate();
    for now in [
        "2026-09-29T12:00:00Z",
        BOUND,
        "2026-09-29T19:55:00Z",
        "2026-10-03T17:00:00Z",
    ] {
        let found = close_window_bound(at(now), &gate);
        assert!(
            matches!(found, Err(Cause::Absent { .. })),
            "{now}: {found:?}"
        );
    }
}

/// DEC-853 item 6: the least of `exit_step_s`, `unknown_absent_window_s / unknown_absent_lookups`
/// rounded down and at least 1, `bracket_partial_fill_timeout_s` and `max_unprotected_s`; each term
/// is made the least in turn. A zero member is refused rather than skipped (FT-8).
#[test]
fn the_poll_interval_is_the_least_of_its_four_terms() {
    let config = |[exit, window, lookups, bracket, unprotected]: [i64; 5]| ExecutorConfig {
        exit_step_s: exit,
        unknown_absent_window_s: window,
        unknown_absent_lookups: u32::try_from(lookups).unwrap(),
        bracket_partial_fill_timeout_s: bracket,
        max_unprotected_s: unprotected,
        ..ExecutorConfig::PROPOSED
    };
    let cases = [
        ([5, 15, 3, 60, 60], 5),
        ([3, 15, 3, 60, 60], 3),
        ([9, 14, 3, 60, 60], 4),
        ([5, 2, 3, 60, 60], 1),
        ([9, 15, 3, 2, 60], 2),
        ([9, 15, 3, 60, 1], 1),
    ];
    for (members, secs) in cases {
        let found = poll_interval(&config(members));
        let found = found.unwrap_or_else(|e| panic!("{members:?}: {e:?}"));
        assert_eq!(found, Duration::from_secs(secs), "{members:?}");
    }
    for members in [[0, 15, 3, 60, 60], [5, 15, 0, 60, 60]] {
        let found = poll_interval(&config(members));
        assert!(
            matches!(found, Err(Cause::Absent { .. })),
            "{members:?}: {found:?}"
        );
    }
}

/// DEC-858 item 2, FT-8 (#1023 review): a zero in any other member is refused, never skipped.
#[test]
fn a_zero_window_timeout_or_bound_is_refused() {
    let base = ExecutorConfig::PROPOSED;
    let zeroed = [
        ExecutorConfig {
            unknown_absent_window_s: 0,
            ..base
        },
        ExecutorConfig {
            bracket_partial_fill_timeout_s: 0,
            ..base
        },
        ExecutorConfig {
            max_unprotected_s: 0,
            ..base
        },
    ];
    for config in zeroed {
        let found = poll_interval(&config);
        assert!(
            matches!(found, Err(Cause::Absent { .. })),
            "{config:?}: {found:?}"
        );
    }
}

/// FT-11, DEC-853 item 5, rules 5 and 12: an entry still working at the bound is cancelled through
/// the executor at the first wake at the bound, never before: the account stream records its
/// `pending_cancel`, the one `DELETE` is at 15:50, the watch then reads the entry canceled and
/// ends, and nothing but reads and that cancel reaches the broker after the submission.
#[test]
fn a_working_entry_is_cancelled_at_the_bound_through_the_executor() {
    let run = watched(LATE, true);
    assert_eq!(run.outcome.as_ref().unwrap().submitted.len(), 1);
    let bound = at(BOUND).secs();
    assert_eq!(run.at(delete), [bound], "one cancel, at the bound");
    assert!(run.cancel_journaled(), "the executor journaled the cancel");
    let reads = run.at(read);
    assert_eq!(
        reads.first(),
        Some(&(bound - 15)),
        "the first wake is five seconds in"
    );
    assert_eq!(
        reads.last(),
        Some(&(bound + 5)),
        "the next wake reads it canceled: {reads:?}"
    );
    let other = run.at(|m, path| !read(m, path) && !delete(m, path));
    assert!(other.is_empty(), "only reads and the cancel: {other:?}");
}

/// The same watch through the paper adapter's one door, `ProductionCycle::run_observed_watched`
/// (FT-1, #1041 review): one submission, the one `DELETE` at the bound, the first read five
/// seconds in, the last read the wake after the cancel, and nothing else sent. The door keeps its
/// stages, so the journal is the free function's test's to read.
#[test]
fn the_production_door_watches_as_the_function_does() {
    let run = watched_through(LATE, true, Door::Cycle, Fill::Rests);
    assert_eq!(run.outcome.as_ref().unwrap().submitted.len(), 1);
    let bound = at(BOUND).secs();
    assert_eq!(run.at(delete), [bound], "one cancel, at the bound");
    let reads = run.at(read);
    assert_eq!(reads.first(), Some(&(bound - 15)), "the first wake");
    assert_eq!(
        reads.last(),
        Some(&(bound + 5)),
        "the wake after the cancel: {reads:?}"
    );
    let other = run.at(|m, path| !read(m, path) && !delete(m, path));
    assert!(other.is_empty(), "only reads and the cancel: {other:?}");
}

/// FT-11, DEC-853 item 1: a run that submitted nothing watches nothing: a dry run reports the
/// order it would place, pauses never and reads nothing back.
#[test]
fn a_dry_run_watches_nothing() {
    let run = watched(TUESDAY, false);
    let report = run.outcome.as_ref().unwrap();
    assert!(
        report.submitted.is_empty() && report.would_place.is_some(),
        "{report:?}"
    );
    assert!(run.clock.pauses.borrow().is_empty());
    assert!(run.broker.log.borrow().is_empty());
}

/// The `client_order_id` of the `index`th order posted.
fn posted(run: &Watched, index: usize) -> String {
    let posts = run.broker.posts.borrow();
    let id = posts[index].get("client_order_id").and_then(Value::as_str);
    id.unwrap().to_owned()
}

/// The second of each read of the entry, the first order posted, by its client order id.
fn entry_reads(run: &Watched) -> Vec<i64> {
    let entry = posted(run, 0);
    let log = run.broker.log.borrow();
    let of_entry = |(m, path, _): &&(Method, String, i64)| read(m, path) && path.ends_with(&entry);
    log.iter()
        .filter(of_entry)
        .map(|(.., secs)| *secs)
        .collect()
}

/// The event time of each `ProtectionChanged placed` the account stream records for `entry`.
fn placed_at(run: &Watched, entry: &str) -> Vec<String> {
    let placed = |(kind, body): &(String, String)| {
        kind == "ProtectionChanged" && body.contains(r#""action":"placed""#) && body.contains(entry)
    };
    let time = |(_, body): (String, String)| {
        let tail = &body[body.find(r#""event_time":""#).unwrap() + 14..];
        tail[..tail.find('"').unwrap()].to_owned()
    };
    run.account().into_iter().filter(placed).map(time).collect()
}

/// DEC-858 item 4, FT-11, trading spec §5.4, rule 13: an entry the broker reads back `filled` ends
/// the watch at that wake, its bracket's legs recorded `placed` at that wake's time, never held for
/// the watch's end; the entry is read once, nothing is sent after that second, and nothing is
/// posted or cancelled after the submission.
#[test]
fn a_filled_entry_ends_the_watch_with_its_legs_placed_at_once() {
    let run = watched_through(TUESDAY, true, Door::Function, Fill::Whole);
    assert_eq!(run.outcome.as_ref().unwrap().submitted.len(), 1);
    let wake = at(TUESDAY).secs() + 5;
    assert_eq!(
        entry_reads(&run),
        [wake],
        "one read of the entry, five seconds in"
    );
    let later = run.at(|_, _| true).into_iter().filter(|secs| *secs != wake);
    assert_eq!(later.count(), 0, "nothing is sent after the fill's wake");
    let sent = run.at(|m, _| *m != Method::Get);
    assert!(sent.is_empty(), "nothing posted or cancelled: {sent:?}");
    let entry = posted(&run, 0);
    let wake = "2026-09-29T17:00:05.000000000Z";
    assert_eq!(
        placed_at(&run, &entry),
        [wake],
        "the legs, placed once, at the wake"
    );
}

/// Rule 13, trading spec §5.4, DEC-858 items 3 and 4, DEC-877 item 2: an entry that fills as the
/// bound's cancel leaves is protected at once: the cancel's own read-back at 15:50 finds it
/// `filled`, the reconciliation that asks for is gathered rather than stepped answer by answer, its
/// legs are recorded `placed` at that wake, the next wake's read ends the run cleanly, and nothing
/// is posted after the entry.
#[test]
fn a_fill_that_beats_the_bounds_cancel_is_protected_at_once() {
    let run = watched_through(LATE, true, Door::Function, Fill::Races);
    assert_eq!(run.outcome.as_ref().unwrap().submitted.len(), 1);
    let bound = at(BOUND).secs();
    assert_eq!(run.at(delete), [bound], "one cancel, at the bound");
    assert_eq!(entry_reads(&run).last(), Some(&(bound + 5)), "read filled");
    let entry = posted(&run, 0);
    let wake = "2026-09-29T19:50:00.000000000Z";
    assert_eq!(
        placed_at(&run, &entry),
        [wake],
        "the legs, placed at the bound's wake"
    );
    let posts = run.at(|m, _| *m == Method::Post);
    assert!(
        posts.is_empty(),
        "nothing posted after the entry: {posts:?}"
    );
}

/// DEC-858 item 5, DEC-877 item 1, rules 3 and 6: past the bound the watch runs at most to the
/// regular session's end, 16:00. A cancel the broker never confirms ends the run failing closed at
/// the first wake at or past it, before any read there: the stop's code is `cancel_unconfirmed` and
/// its message names no order; the entry's last journaled state stays the executor's
/// `pending_cancel`, for the next start's reconciliation; the one `DELETE` at the bound is the only
/// request but GETs, and the entry is read at every wake to 15:59:55 (at the bound by the cancel's
/// own read-back) and never after.
#[test]
fn an_unconfirmed_cancel_fails_closed_at_the_sessions_end() {
    let run = watched_through(LATE, true, Door::Function, Fill::Stuck);
    let error = run.outcome.as_ref().err();
    assert_eq!(
        error.map(ShellError::code),
        Some("cancel_unconfirmed"),
        "{:?}",
        run.outcome
    );
    let entry = posted(&run, 0);
    assert!(
        !error.unwrap().to_string().contains(&entry),
        "no order is named"
    );
    let (bound, end) = (at(BOUND).secs(), at("2026-09-29T20:00:00Z").secs());
    assert_eq!(run.at(delete), [bound], "one cancel, at the bound");
    let mut reads = entry_reads(&run);
    reads.dedup();
    let every_wake: Vec<i64> = (bound - 15..end).step_by(5).collect();
    assert_eq!(
        reads, every_wake,
        "a read at every wake to 15:59:55, none at 16:00"
    );
    assert_eq!(
        run.clock.pauses.borrow().len(),
        124,
        "the last wake is 16:00"
    );
    let other = run.at(|m, path| *m != Method::Get && !delete(m, path));
    assert!(
        other.is_empty(),
        "nothing placed after the submission: {other:?}"
    );
    let states: Vec<String> = run
        .account()
        .into_iter()
        .filter(|(kind, body)| kind == "OrderStateChanged" && body.contains(&entry))
        .map(|(_, body)| body)
        .collect();
    let last = states.last().unwrap();
    assert!(last.contains(r#""state":"pending_cancel""#), "{last}");
}

/// DEC-858 item 5, DEC-877 item 1, rule 3: on a short day the cap is the early close, never 16:00.
/// On Thursday 2026-12-24 (an early close at 13:00 New York, 18:00 UTC), bounded at 12:50, a cancel
/// the broker never confirms ends the run failing closed at the first wake at or past 13:00, before
/// any read there: the entry is read at every wake to 12:59:55 and never after, the one `DELETE`
/// is at 12:50, and the stop's code is `cancel_unconfirmed`.
#[test]
fn on_an_early_close_the_cap_stops_at_the_early_close() {
    let (late, bound) = ("2026-12-24T17:49:40Z", "2026-12-24T17:50:00Z");
    let run = watched_to(late, bound, true, Door::Function, Fill::Stuck);
    let error = run.outcome.as_ref().err();
    assert_eq!(
        error.map(ShellError::code),
        Some("cancel_unconfirmed"),
        "{:?}",
        run.outcome
    );
    let (bound, end) = (at(bound).secs(), at("2026-12-24T18:00:00Z").secs());
    assert_eq!(run.at(delete), [bound], "one cancel, at the bound");
    let mut reads = entry_reads(&run);
    reads.dedup();
    let every_wake: Vec<i64> = (bound - 15..end).step_by(5).collect();
    assert_eq!(
        reads, every_wake,
        "a read at every wake to 12:59:55, none at 13:00"
    );
    assert_eq!(
        run.clock.pauses.borrow().len(),
        124,
        "the last wake is 13:00"
    );
    let other = run.at(|m, path| *m != Method::Get && !delete(m, path));
    assert!(
        other.is_empty(),
        "nothing placed after the submission: {other:?}"
    );
}

/// The `risk_clock` each account record for `entry` in `state` carries.
fn risk_clocks(run: &Watched, entry: &str, state: &str) -> Vec<String> {
    let state = format!(r#""state":"{state}""#);
    let of = |(kind, body): &(String, String)| {
        kind == "OrderStateChanged" && body.contains(entry) && body.contains(&state)
    };
    let clock = |(_, body): (String, String)| {
        let tail = &body[body.find(r#""risk_clock":""#).unwrap() + 14..];
        tail[..tail.find('"').unwrap()].to_owned()
    };
    run.account().into_iter().filter(of).map(clock).collect()
}

/// DEC-858 item 3: each wake reads the entry, hands the answer to the executor, then ticks it at the
/// clock's second, so the executor's clock follows the watch, never stays at the run's start. An
/// entry read `accepted` at 13:00:05 and `filled` at 13:00:10 is recorded `filled` at 13:00:10 with
/// the executor's clock at 13:00:05, the first wake's tick; the watch then ends at that wake.
#[test]
fn each_wake_ticks_the_executor_at_the_clocks_second() {
    let run = watched_through(TUESDAY, true, Door::Function, Fill::Second);
    assert_eq!(run.outcome.as_ref().unwrap().submitted.len(), 1);
    let start = at(TUESDAY).secs();
    assert_eq!(entry_reads(&run), [start + 5, start + 10], "two wakes");
    let entry = posted(&run, 0);
    assert_eq!(
        risk_clocks(&run, &entry, "filled"),
        ["2026-09-29T17:00:05.000000000Z"],
        "the fill carries the first wake's tick, not the run's start"
    );
}

/// DEC-858 item 4: `expired` and `rejected` are terminal, as `filled` and `canceled` are. An entry
/// read back either way at the first wake ends the watch there: one read of the entry, at 13:00:05,
/// nothing sent after that second, nothing posted or cancelled, and the run ends `Ok`.
#[test]
fn an_expired_or_rejected_entry_ends_the_watch_at_once() {
    for fill in [Fill::Expires, Fill::Rejected] {
        let run = watched_through(TUESDAY, true, Door::Function, fill);
        let status = if fill == Fill::Expires {
            "expired"
        } else {
            "rejected"
        };
        let report = run.outcome.as_ref();
        assert_eq!(
            report.map(|r| r.submitted.len()).ok(),
            Some(1),
            "{status}: {report:?}"
        );
        let wake = at(TUESDAY).secs() + 5;
        assert_eq!(
            entry_reads(&run),
            [wake],
            "{status}: one read, five seconds in"
        );
        let later = run.at(|_, _| true).into_iter().filter(|secs| *secs != wake);
        assert_eq!(
            later.count(),
            0,
            "{status}: nothing is sent after that wake"
        );
        let sent = run.at(|m, _| *m != Method::Get);
        assert!(
            sent.is_empty(),
            "{status}: nothing posted or cancelled: {sent:?}"
        );
        assert_eq!(run.clock.pauses.borrow().len(), 1, "{status}: one wake");
    }
}
