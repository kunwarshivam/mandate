//! The supervisor's start, schedule, fence, and drain (E21-2, DEC-968 items 1 to 5 and 8).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ACCOUNT, AGENT, Broker, FakeStore, PERIOD, PaperHost, paper};
use mandate_agentd::{Config, Host, IdleHost, Phase, Status, Stop, Supervisor};
use mandate_journal::Environment;
use mandate_shell::stages::JournalWriter;

type Log = Rc<RefCell<Vec<(usize, &'static str)>>>;

struct World {
    store: FakeStore,
    broker: Rc<RefCell<Broker>>,
    log: Log,
}

impl World {
    fn new() -> Self {
        Self {
            store: FakeStore::default(),
            broker: Rc::default(),
            log: Rc::default(),
        }
    }

    fn host(&self, id: usize) -> PaperHost {
        PaperHost::new(id, &self.store.journal, &self.broker, &self.log)
    }

    fn start(
        &self,
        host: PaperHost,
        config: &Config,
        now: u64,
    ) -> Supervisor<FakeStore, PaperHost> {
        Supervisor::start(self.store.clone(), host, config, Status::default(), now)
    }

    fn calls(&self) -> Vec<(usize, &'static str)> {
        self.log.borrow().clone()
    }

    fn sent(&self) -> Vec<String> {
        self.broker.borrow().sent.clone()
    }

    fn epoch(&self, stream: &str) -> u64 {
        self.store.journal.borrow().epoch(stream)
    }
}

#[test]
fn every_environment_but_paper_is_refused_before_a_lease_or_a_host_call() {
    for environment in [Environment::Live, Environment::Backtest] {
        let world = World::new();
        let config = Config {
            environment,
            ..paper()
        };
        let mut supervisor = world.start(world.host(0), &config, 0);
        supervisor.tick(0);
        supervisor.drain();
        assert_eq!(supervisor.phase(), Phase::Stopped(Stop::Refused));
        assert_eq!((world.epoch(AGENT), world.epoch(ACCOUNT)), (0, 0));
        assert!(world.calls().is_empty());
    }
}

#[test]
fn start_takes_every_stream_and_reconciles_before_the_first_cycle() {
    let world = World::new();
    let mut supervisor = world.start(world.host(0), &paper(), 50);
    assert_eq!(supervisor.phase(), Phase::Ready);
    assert_eq!((world.epoch(AGENT), world.epoch(ACCOUNT)), (1, 1));
    assert_eq!(world.calls(), vec![(0, "reconcile")]);
    supervisor.tick(50);
    assert_eq!(world.calls(), vec![(0, "reconcile"), (0, "cycle")]);
    assert_eq!(world.sent(), vec!["coid-1"]);
}

#[test]
fn a_failed_reconciliation_runs_no_cycle_and_is_not_ready() {
    let world = World::new();
    let status = Status::default();
    let mut supervisor = Supervisor::start(
        world.store.clone(),
        Refusing(world.host(0)),
        &paper(),
        status.clone(),
        0,
    );
    supervisor.tick(0);
    supervisor.tick(PERIOD);
    supervisor.drain();
    assert_eq!(status.phase(), Phase::Stopped(Stop::Refused));
    assert_eq!(world.calls(), vec![(0, "reconcile")]);
}

#[test]
fn a_store_that_will_not_hand_over_a_stream_is_refused_before_the_host() {
    let world = World::new();
    world.store.journal.borrow_mut().down = true;
    let mut supervisor = world.start(world.host(0), &paper(), 0);
    world.store.journal.borrow_mut().down = false;
    supervisor.tick(0);
    assert_eq!(supervisor.phase(), Phase::Stopped(Stop::Refused));
    assert!(world.calls().is_empty());
}

#[test]
fn cycles_start_once_per_period_from_the_start() {
    let world = World::new();
    let mut supervisor = world.start(world.host(0), &paper(), 100);
    for now in [
        99,
        100,
        100,
        100 + PERIOD - 1,
        100 + PERIOD,
        100 + 2 * PERIOD - 1,
    ] {
        supervisor.tick(now);
    }
    assert_eq!(world.sent(), vec!["coid-1", "coid-2"]);
    supervisor.tick(100 + 3 * PERIOD);
    assert_eq!(world.sent(), vec!["coid-1", "coid-2", "coid-3"]);
}

#[test]
fn a_late_tick_schedules_the_next_cycle_from_itself() {
    let world = World::new();
    let mut supervisor = world.start(world.host(0), &paper(), 0);
    supervisor.tick(5 * PERIOD);
    supervisor.tick(6 * PERIOD - 1);
    assert_eq!(world.sent().len(), 1);
    supervisor.tick(6 * PERIOD);
    assert_eq!(world.sent().len(), 2);
}

#[test]
fn a_period_that_overflows_never_runs_a_second_cycle() {
    let world = World::new();
    let config = Config {
        period_ns: u64::MAX,
        ..paper()
    };
    let mut supervisor = world.start(world.host(0), &config, 7);
    supervisor.tick(7);
    supervisor.tick(u64::MAX - 1);
    assert_eq!(world.sent().len(), 1);
    supervisor.tick(u64::MAX);
    assert_eq!(world.sent().len(), 2);
}

#[test]
fn a_process_whose_stream_was_taken_stops_fenced_without_calling_its_host() {
    for taken in [AGENT, ACCOUNT] {
        let world = World::new();
        let mut supervisor = world.start(world.host(0), &paper(), 0);
        world.store.clone().take_ownership(taken).ok();
        supervisor.tick(0);
        assert_eq!(supervisor.phase(), Phase::Stopped(Stop::Fenced));
        supervisor.drain();
        assert_eq!(supervisor.phase(), Phase::Stopped(Stop::Fenced));
        assert_eq!(world.calls(), vec![(0, "reconcile")]);
    }
}

#[test]
fn a_store_that_cannot_answer_counts_as_fenced() {
    let world = World::new();
    let mut supervisor = world.start(world.host(0), &paper(), 0);
    world.store.journal.borrow_mut().down = true;
    supervisor.tick(0);
    assert_eq!(supervisor.phase(), Phase::Stopped(Stop::Fenced));
    assert_eq!(world.calls(), vec![(0, "reconcile")]);
}

#[test]
fn a_cycle_that_fails_while_held_stops_the_process() {
    let world = World::new();
    let mut host = world.host(0);
    host.cycle_fails = true;
    let mut supervisor = world.start(host, &paper(), 0);
    supervisor.tick(0);
    assert_eq!(supervisor.phase(), Phase::Stopped(Stop::CycleFailed));
    supervisor.tick(PERIOD);
    assert_eq!(world.calls(), vec![(0, "reconcile"), (0, "cycle")]);
}

#[test]
fn a_cycle_fenced_mid_way_sends_nothing_and_stops_fenced() {
    let world = World::new();
    let mut host = world.host(0);
    let sibling = world.store.clone();
    host.before_append = Some(Box::new(move || {
        let mut sibling = sibling.clone();
        sibling.take_ownership(ACCOUNT).ok();
    }));
    let mut supervisor = world.start(host, &paper(), 0);
    supervisor.tick(0);
    assert_eq!(supervisor.phase(), Phase::Stopped(Stop::Fenced));
    assert!(world.sent().is_empty());
}

#[test]
fn drain_settles_while_draining_then_stops_and_runs_no_cycle() {
    let world = World::new();
    let status = Status::default();
    let mut host = world.host(0);
    host.status = Some(status.clone());
    let mut supervisor = Supervisor::start(world.store.clone(), host, &paper(), status.clone(), 0);
    supervisor.drain();
    assert_eq!(status.phase(), Phase::Stopped(Stop::Drained));
    supervisor.tick(0);
    supervisor.drain();
    assert_eq!(world.calls(), vec![(0, "reconcile"), (0, "settle")]);
    assert_eq!(status.phase(), Phase::Stopped(Stop::Drained));
}

#[test]
fn drain_after_a_stop_neither_settles_nor_changes_the_stop() {
    let world = World::new();
    let mut host = world.host(0);
    host.cycle_fails = true;
    let mut supervisor = world.start(host, &paper(), 0);
    supervisor.tick(0);
    supervisor.drain();
    assert_eq!(supervisor.phase(), Phase::Stopped(Stop::CycleFailed));
    assert_eq!(world.calls(), vec![(0, "reconcile"), (0, "cycle")]);
}

#[test]
fn every_stop_has_its_own_code_and_exit_status() {
    let stops = [
        (Stop::Drained, "drained", 0),
        (Stop::Refused, "refused", 2),
        (Stop::Fenced, "fenced", 3),
        (Stop::CycleFailed, "cycle_failed", 4),
        (Stop::DrainIncomplete, "drain_incomplete", 5),
    ];
    for (stop, code, status) in stops {
        assert_eq!((stop.code(), stop.exit_status()), (code, status));
    }
}

#[test]
fn a_new_status_is_starting() {
    assert_eq!(Status::default().phase(), Phase::Starting);
}

/// A host whose reconciliation finds something it cannot explain.
struct Refusing(PaperHost);

impl mandate_agentd::Host for Refusing {
    fn reconcile(&mut self, journal: &mut dyn JournalWriter) -> Result<(), mandate_shell::Cause> {
        self.0.reconcile(journal)?;
        Err(mandate_shell::Cause::Absent {
            what: "an unexplained difference",
        })
    }

    fn cycle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), mandate_shell::Cause> {
        self.0.cycle(journal)
    }

    fn settle(&mut self, journal: &mut dyn JournalWriter) -> Result<(), mandate_shell::Cause> {
        self.0.settle(journal)
    }
}

#[test]
fn the_idle_host_reads_every_stream_at_every_call_and_reports_a_read_it_cannot_make() {
    let world = World::new();
    let mut host = IdleHost {
        streams: common::streams(),
    };
    let mut store = world.store.clone();
    assert!(host.reconcile(&mut store).is_ok());
    assert!(host.cycle(&mut store).is_ok());
    assert!(host.settle(&mut store).is_ok());
    world.store.journal.borrow_mut().down = true;
    assert!(host.reconcile(&mut store).is_err());
    assert!(host.cycle(&mut store).is_err());
    assert!(host.settle(&mut store).is_err());
}

#[test]
fn a_drain_of_a_process_whose_stream_was_taken_stops_fenced_without_settling() {
    let world = World::new();
    let mut supervisor = world.start(world.host(0), &paper(), 0);
    world.store.clone().take_ownership(ACCOUNT).ok();
    supervisor.drain();
    assert_eq!(supervisor.phase(), Phase::Stopped(Stop::Fenced));
    assert_eq!(world.calls(), vec![(0, "reconcile")]);
}

#[test]
fn a_settle_that_fails_stops_the_drain_incomplete() {
    let world = World::new();
    let mut host = world.host(0);
    host.settle_fails = true;
    let mut supervisor = world.start(host, &paper(), 0);
    supervisor.drain();
    assert_eq!(supervisor.phase(), Phase::Stopped(Stop::DrainIncomplete));
    supervisor.tick(0);
    assert_eq!(world.calls(), vec![(0, "reconcile"), (0, "settle")]);
}
