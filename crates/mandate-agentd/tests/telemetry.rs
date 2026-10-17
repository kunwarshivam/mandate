//! OPS-11 (E21-3): the decision cycle is the same with telemetry off, with the exporter serving
//! while stalled scrapers hold it or a scraper reads it throughout, with every scrape refused, and
//! with the exporter unable to bind; and the counts a scrape shows are the host's calls by
//! outcome.

mod common;

use std::cell::RefCell;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::{self, JoinHandle};

use common::{Broker, Crash, FakeStore, PERIOD, PaperHost, paper, streams};
use mandate_agentd::{Host, Instrumented, Phase, Status, Stop, Supervisor};
use mandate_shell::Cause;
use mandate_shell::stages::JournalWriter;
use mandate_telemetry::{Metrics, ProcessType, Release, TelemetryError, serve};
use proptest::prelude::*;

/// What a run did: every stream's epoch and records, every order sent, every host call, and
/// where it stopped.
type Outcome = (
    Vec<(u64, Vec<String>)>,
    Vec<String>,
    Vec<(usize, &'static str)>,
    Phase,
);

#[derive(Debug, Clone, Copy)]
struct Script {
    ticks: u64,
    late: u64,
    crash: Crash,
    cycle_fails: bool,
    settle_fails: bool,
    drain: bool,
}

/// How telemetry is set up around the host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Telemetry {
    /// The bare host, no telemetry at all: the reference.
    Absent,
    Off,
    /// Served, with one silent scraper and one that sent half a request holding the exporter.
    Stalled,
    /// Recording, but the exporter's address was taken.
    Unbound,
    /// Served, with a scraper reading `/metrics` in a loop for the whole run.
    Scraped,
    /// Served off, so every scrape the looping scraper makes is refused `503`.
    Refusing,
}

/// What must stay alive while a run goes: held connections, and a looping scraper that stops and
/// is joined when this is dropped.
#[derive(Default)]
struct Held {
    _streams: Vec<TcpStream>,
    stop: Arc<AtomicBool>,
    scraper: Option<JoinHandle<()>>,
}

impl Drop for Held {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(scraper) = self.scraper.take() {
            scraper.join().unwrap();
        }
    }
}

fn scrape(addr: SocketAddr) -> String {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.write_all(b"GET /metrics HTTP/1.1\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response
}

/// Serves `metrics` and scrapes it in a loop until stopped, returning once the first scrape has
/// been answered `status`, so the run overlaps scrapes.
fn scraped(metrics: &Metrics, status: &'static str) -> Held {
    let addr = serve(loopback(), metrics.clone()).unwrap().addr();
    let stop = Arc::new(AtomicBool::new(false));
    let (first, answered) = mpsc::channel();
    let flag = Arc::clone(&stop);
    let scraper = thread::spawn(move || {
        while !flag.load(Ordering::SeqCst) {
            let response = scrape(addr);
            assert!(response.starts_with(status), "{response}");
            first.send(()).ok();
        }
    });
    answered.recv().unwrap();
    Held {
        _streams: Vec::new(),
        stop,
        scraper: Some(scraper),
    }
}

/// Runs `script` and returns what it did, with the metrics it recorded into.
fn run(script: Script, telemetry: Telemetry) -> (Outcome, Metrics, Held) {
    let store = FakeStore::default();
    let broker: Rc<RefCell<Broker>> = Rc::default();
    let log = Rc::default();
    let mut host = PaperHost::new(0, &store.journal, &broker, &log);
    host.crash = script.crash;
    host.cycle_fails = script.cycle_fails;
    host.settle_fails = script.settle_fails;
    let (metrics, held) = metrics_for(telemetry);
    let host = match telemetry {
        Telemetry::Absent => Hosted::Bare(host),
        _ => Hosted::Instrumented(Instrumented::new(host, metrics.clone())),
    };
    let mut supervisor = Supervisor::start(store.clone(), host, &paper(), Status::default(), 0);
    for n in 0..script.ticks {
        supervisor.tick(n * PERIOD + n % 2 * script.late);
    }
    if script.drain {
        supervisor.drain();
    }
    let journal = store.journal.borrow();
    let streams = streams()
        .iter()
        .map(|s| (journal.epoch(s), journal.rows(s)))
        .collect();
    let outcome = (
        streams,
        broker.borrow().sent.clone(),
        log.borrow().clone(),
        supervisor.phase(),
    );
    (outcome, metrics, held)
}

/// The bare host or the instrumented one, as one type the supervisor can hold.
enum Hosted {
    Bare(PaperHost),
    Instrumented(Instrumented<PaperHost>),
}

impl Host for Hosted {
    fn reconcile(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        match self {
            Self::Bare(host) => host.reconcile(journal),
            Self::Instrumented(host) => host.reconcile(journal),
        }
    }

    fn cycle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        match self {
            Self::Bare(host) => host.cycle(journal),
            Self::Instrumented(host) => host.cycle(journal),
        }
    }

    fn settle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), Cause> {
        match self {
            Self::Bare(host) => host.settle(journal),
            Self::Instrumented(host) => host.settle(journal),
        }
    }
}

fn live() -> Metrics {
    Metrics::new(ProcessType::AgentProcess, &Release::parse("0.0.0").unwrap()).unwrap()
}

fn loopback() -> SocketAddr {
    SocketAddr::from(([127, 0, 0, 1], 0))
}

/// The metrics for a set-up, and what must stay alive while it runs.
fn metrics_for(telemetry: Telemetry) -> (Metrics, Held) {
    match telemetry {
        Telemetry::Absent | Telemetry::Off => (Metrics::off(), Held::default()),
        Telemetry::Scraped => {
            let metrics = live();
            let held = scraped(&metrics, "HTTP/1.1 200 OK\r\n");
            (metrics, held)
        }
        Telemetry::Refusing => {
            let metrics = Metrics::off();
            let held = scraped(&metrics, "HTTP/1.1 503 Service Unavailable\r\n");
            (metrics, held)
        }
        Telemetry::Stalled => {
            let metrics = live();
            let exporter = serve(loopback(), metrics.clone()).unwrap();
            let silent = TcpStream::connect(exporter.addr()).unwrap();
            let mut half = TcpStream::connect(exporter.addr()).unwrap();
            half.write_all(b"GET /metr").unwrap();
            let held = Held {
                _streams: vec![silent, half],
                stop: Arc::default(),
                scraper: None,
            };
            (metrics, held)
        }
        Telemetry::Unbound => {
            let metrics = live();
            let taken = TcpListener::bind(loopback()).unwrap();
            let refused = serve(taken.local_addr().unwrap(), metrics.clone());
            assert_eq!(
                refused,
                Err(TelemetryError::Start(std::io::ErrorKind::AddrInUse))
            );
            (metrics, Held::default())
        }
    }
}

fn crash() -> impl Strategy<Value = Crash> {
    prop_oneof![
        Just(Crash::Never),
        Just(Crash::AfterJournal),
        Just(Crash::AfterSend)
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn the_decision_cycle_is_the_same_whatever_the_exporter_does(
        ticks in 0u64..12,
        late in 0u64..PERIOD,
        crash in crash(),
        cycle_fails in any::<bool>(),
        settle_fails in any::<bool>(),
        drain in any::<bool>(),
    ) {
        let script = Script { ticks, late, crash, cycle_fails, settle_fails, drain };
        let (reference, _, _) = run(script, Telemetry::Absent);
        for telemetry in [
            Telemetry::Off,
            Telemetry::Stalled,
            Telemetry::Unbound,
            Telemetry::Scraped,
            Telemetry::Refusing,
        ] {
            let (outcome, _, _held) = run(script, telemetry);
            prop_assert_eq!(&outcome, &reference, "{:?}", telemetry);
        }
    }
}

fn line(text: &str, metric: &str, code: &str) -> Option<u64> {
    let prefix = format!("{metric}{{process_type=\"agent_process\",reason_code=\"{code}\"}} ");
    text.lines()
        .find_map(|l| l.strip_prefix(&prefix))
        .and_then(|n| n.parse().ok())
}

#[test]
fn a_scrape_counts_the_hosts_calls_by_outcome() {
    let script = Script {
        ticks: 5,
        late: 0,
        crash: Crash::Never,
        cycle_fails: false,
        settle_fails: true,
        drain: true,
    };
    let (outcome, metrics, _held) = run(script, Telemetry::Stalled);
    assert_eq!(outcome.1.len(), 5);
    let text = metrics.encode().unwrap();
    let cycles = "mandate_process_cycles_total";
    assert_eq!(line(&text, cycles, "completed"), Some(5), "{text}");
    assert_eq!(line(&text, cycles, "failed"), None, "{text}");
    let reconciliations = "mandate_process_reconciliations_total";
    assert_eq!(line(&text, reconciliations, "completed"), Some(1), "{text}");
    assert_eq!(
        line(&text, "mandate_process_settles_total", "failed"),
        Some(1),
        "{text}"
    );
    assert_eq!(
        line(&text, "mandate_process_settles_total", "completed"),
        None,
        "{text}"
    );
}

#[test]
fn a_failed_cycle_is_counted_failed() {
    let script = Script {
        ticks: 3,
        late: 0,
        crash: Crash::Never,
        cycle_fails: true,
        settle_fails: false,
        drain: false,
    };
    let (outcome, metrics, _held) = run(script, Telemetry::Unbound);
    assert_eq!(outcome.3, Phase::Stopped(Stop::CycleFailed));
    let text = metrics.encode().unwrap();
    assert_eq!(
        line(&text, "mandate_process_cycles_total", "failed"),
        Some(1),
        "{text}"
    );
    assert_eq!(
        line(&text, "mandate_process_cycles_total", "completed"),
        None,
        "{text}"
    );
}
