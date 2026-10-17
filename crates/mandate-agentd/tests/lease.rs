//! The fencing lease: two processes never both hold a deployment's streams, and a restart never
//! sends anything twice (E21-2, DEC-968 items 1 and 2).

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ACCOUNT, AGENT, Broker, Crash, FakeStore, PERIOD, PaperHost, paper, streams};
use mandate_agentd::{Epochs, FENCED, Lease, MemoryStore, Phase, Status, Supervisor};
use mandate_journal::{AppendOutcome, MemoryJournal};
use mandate_shell::stages::JournalWriter;
use mandate_time::UtcNanos;
use proptest::prelude::*;

fn memory() -> MemoryStore {
    MemoryStore::new(Rc::new(RefCell::new(MemoryJournal::new())), UtcNanos::EPOCH)
}

#[derive(Debug, Clone)]
enum LeaseOp {
    Acquire(usize),
    Ask(usize),
    Release(usize),
}

fn lease_op() -> impl Strategy<Value = LeaseOp> {
    prop_oneof![
        (0..3usize).prop_map(LeaseOp::Acquire),
        (0..3usize).prop_map(LeaseOp::Ask),
        (0..3usize).prop_map(LeaseOp::Release),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Over the real in-memory journal, with an oracle that only remembers who acquired last: at
    /// every step at most one instance holds the lease, the holder is the last to acquire, and only
    /// the holder's lease hands a cycle an epoch, the same one each time.
    #[test]
    fn two_instances_never_both_hold_the_lease(ops in prop::collection::vec(lease_op(), 1..40)) {
        let store = memory();
        let mut leases: [Option<Lease<MemoryStore>>; 3] = [None, None, None];
        let mut last_acquirer = None;
        for op in ops {
            match op {
                LeaseOp::Acquire(i) => {
                    leases[i] = Lease::acquire(store.clone(), &streams()).ok();
                    prop_assert!(leases[i].is_some());
                    last_acquirer = Some(i);
                }
                LeaseOp::Ask(i) => {
                    if let Some(lease) = leases[i].as_mut() {
                        let first = lease.take_ownership(ACCOUNT);
                        let again = lease.take_ownership(ACCOUNT);
                        prop_assert_eq!(first.is_ok(), last_acquirer == Some(i));
                        prop_assert_eq!(first.ok(), again.ok());
                    }
                }
                LeaseOp::Release(i) => {
                    leases[i].take().map(Lease::release);
                }
            }
            let holders: Vec<usize> = (0..3)
                .filter(|&i| leases[i].as_ref().is_some_and(Lease::held))
                .collect();
            prop_assert!(holders.len() <= 1);
            let expected: Vec<usize> = last_acquirer
                .filter(|&i| leases[i].is_some())
                .into_iter()
                .collect();
            prop_assert_eq!(holders, expected);
        }
    }
}

#[derive(Debug, Clone)]
enum ProcessOp {
    Start(Crash),
    Tick(usize),
    Drain(usize),
    Die(usize),
}

fn crash() -> impl Strategy<Value = Crash> {
    prop_oneof![
        Just(Crash::Never),
        Just(Crash::AfterJournal),
        Just(Crash::AfterSend)
    ]
}

fn process_op() -> impl Strategy<Value = ProcessOp> {
    prop_oneof![
        crash().prop_map(ProcessOp::Start),
        (0..4usize).prop_map(ProcessOp::Tick),
        (0..4usize).prop_map(ProcessOp::Drain),
        (0..4usize).prop_map(ProcessOp::Die),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig {
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// Processes start, cycle, crash at any point of a cycle, drain, and die, while older ones
    /// keep ticking. Nothing is ever sent twice, nothing is sent that was not journaled first,
    /// only the most recently started process runs cycles, and a final restart's reconciliation
    /// leaves every journaled submission answered and at the broker.
    #[test]
    fn a_crash_and_restart_never_sends_anything_twice(
        ops in prop::collection::vec(process_op(), 1..40),
    ) {
        let store = FakeStore::default();
        let broker: Rc<RefCell<Broker>> = Rc::default();
        let log = Rc::default();
        let mut processes: Vec<Option<Supervisor<FakeStore, PaperHost>>> = Vec::new();
        let mut now = 0;
        for op in ops {
            now += PERIOD;
            match op {
                ProcessOp::Start(crash) => {
                    let mut host = PaperHost::new(processes.len(), &store.journal, &broker, &log);
                    host.crash = crash;
                    let started =
                        Supervisor::start(store.clone(), host, &paper(), Status::default(), now);
                    processes.push(Some(started));
                }
                ProcessOp::Tick(i) => {
                    if let Some(Some(process)) = processes.get_mut(i) {
                        process.tick(now);
                    }
                }
                ProcessOp::Drain(i) => {
                    if let Some(Some(process)) = processes.get_mut(i) {
                        process.drain();
                    }
                }
                ProcessOp::Die(i) => {
                    if let Some(process) = processes.get_mut(i) {
                        *process = None;
                    }
                }
            }
            let sent = broker.borrow().sent.clone();
            let mut unique = sent.clone();
            unique.sort();
            unique.dedup();
            prop_assert_eq!(unique.len(), sent.len());
            let rows = store.journal.borrow().rows(ACCOUNT);
            for id in &sent {
                let submitted = format!("submitted:{id}");
                prop_assert!(rows.contains(&submitted));
            }
            let newest = processes.len().checked_sub(1);
            let calls: Vec<(usize, &str)> = log.borrow().clone();
            for (process, call) in calls {
                if call == "cycle" {
                    prop_assert_eq!(Some(process), newest);
                }
            }
            log.borrow_mut().clear();
        }
        let host = PaperHost::new(processes.len(), &store.journal, &broker, &log);
        let last = Supervisor::start(store.clone(), host, &paper(), Status::default(), now);
        prop_assert_eq!(last.phase(), Phase::Ready);
        let rows = store.journal.borrow().rows(ACCOUNT);
        for row in &rows {
            if let Some(id) = row.strip_prefix("submitted:") {
                let answered = format!("answered:{id}");
                prop_assert!(rows.contains(&answered));
                prop_assert!(broker.borrow().sent.contains(&id.to_owned()));
            }
        }
    }
}

#[test]
fn the_lease_hands_back_its_own_epoch_and_never_takes_a_newer_one() {
    let store = FakeStore::default();
    let mut lease = Lease::acquire(store.clone(), &streams()).ok();
    let lease = lease.as_mut().into_iter().next();
    let Some(lease) = lease else {
        panic!("a lease over a store that answers");
    };
    assert_eq!(lease.take_ownership(AGENT).ok(), Some(1));
    assert_eq!(lease.take_ownership(AGENT).ok(), Some(1));
    assert_eq!(store.writer_epoch(AGENT).ok(), Some(1));
    assert!(lease.take_ownership("ctl:ws_test").is_err());
}

#[test]
fn a_taken_stream_answers_fenced_through_the_lease() {
    let store = FakeStore::default();
    let Ok(mut lease) = Lease::acquire(store.clone(), &streams()) else {
        panic!("a lease over a store that answers");
    };
    store.clone().take_ownership(ACCOUNT).ok();
    assert!(!lease.held());
    assert!(matches!(lease.take_ownership(ACCOUNT), Err(e) if e.to_string() == FENCED.to_string()));
    assert_eq!(lease.take_ownership(AGENT).ok(), Some(1));
}

#[test]
fn an_append_goes_under_the_lease_epoch_whatever_the_caller_says() {
    let store = FakeStore::default();
    store.clone().take_ownership(ACCOUNT).ok();
    let Ok(mut lease) = Lease::acquire(store.clone(), &streams()) else {
        panic!("a lease over a store that answers");
    };
    let outcome = lease.append(ACCOUNT, 0, 99, &[b"submitted:x".to_vec()]);
    assert!(matches!(outcome, AppendOutcome::Committed(_)));
    assert_eq!(*store.appended_under.borrow(), vec![2]);
}

#[test]
fn an_append_to_a_stream_the_lease_does_not_hold_is_fenced_and_not_tried() {
    let store = FakeStore::default();
    let mut other = store.clone();
    other.take_ownership("ctl:ws_test").ok();
    other.take_ownership("ctl:ws_test").ok();
    let Ok(mut lease) = Lease::acquire(store.clone(), &streams()) else {
        panic!("a lease over a store that answers");
    };
    assert_eq!(
        lease.append("ctl:ws_test", 0, 2, &[b"x".to_vec()]),
        AppendOutcome::Fenced { current_epoch: 2 }
    );
    store.journal.borrow_mut().down = true;
    assert_eq!(
        lease.append("ctl:ws_test", 0, 2, &[b"x".to_vec()]),
        AppendOutcome::Fenced {
            current_epoch: u64::MAX
        }
    );
    assert!(store.appended_under.borrow().is_empty());
}

#[test]
fn the_lease_reads_through_to_its_store() {
    let store = FakeStore::default();
    let Ok(lease) = Lease::acquire(store.clone(), &streams()) else {
        panic!("a lease over a store that answers");
    };
    assert!(lease.read(AGENT).is_ok());
    store.journal.borrow_mut().down = true;
    assert!(lease.read(AGENT).is_err());
    assert!(!lease.held());
}

#[test]
fn no_lease_is_taken_from_a_store_that_will_not_answer() {
    let store = FakeStore::default();
    store.journal.borrow_mut().down = true;
    assert!(Lease::acquire(store, &streams()).is_err());
}

#[test]
fn the_memory_store_reads_epochs_and_streams_from_its_journal() {
    let mut store = memory();
    assert_eq!(store.writer_epoch(AGENT).ok(), Some(0));
    assert_eq!(store.take_ownership(AGENT).ok(), Some(1));
    assert_eq!(store.clone().take_ownership(AGENT).ok(), Some(2));
    assert_eq!(store.writer_epoch(AGENT).ok(), Some(2));
    assert_eq!(store.read(AGENT).ok(), Some(Vec::new()));
    assert!(matches!(
        store.append(AGENT, 0, 2, &[b"{}".to_vec()]),
        AppendOutcome::Invalid { .. }
    ));
}

#[test]
fn the_memory_store_refuses_a_stream_id_it_cannot_parse() {
    let mut store = memory();
    assert!(store.writer_epoch("nonsense").is_err());
    assert!(store.take_ownership("nonsense").is_err());
    assert!(store.read("nonsense").is_err());
    assert_eq!(
        store.append("nonsense", 0, 0, &[b"{}".to_vec()]),
        AppendOutcome::Unavailable
    );
}
