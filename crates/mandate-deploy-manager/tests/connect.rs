//! DEC-2335 and DEC-2336 (journal spec §9.21 v0.74, rule 257): when an OAuth connect's token
//! exchange exits successfully while its request is open, the deployment manager starts the
//! account's executor, journaled first at generation 1 with cause `exchanged`, and wants it until
//! the connection is refused or revoked, with or without an agent; it then stops it, journaled
//! first with cause `connection_closed`, unless a deployed agent of the account still wants it.
//! Every start and stop is checked against the records already committed, as the other manager
//! tests are. The pending tests fail on the manager's behaviour, which has no stub to stop at
//! (`xtask/behaviour-only`).

mod common;

use common::{
    Journal, Manager, Seed, accounts, at, block_on, connection_of, deployed, established, kill,
    manager, refused, requested, restart, revoked, roles_made, stopped,
};
use mandate_canon::Value;
use mandate_deploy_manager::Slot;
use mandate_deploy_manager::fake::Call;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const ACCOUNT: &str = "01J8Z2ACCT00000000000000C3";
const AGENTS: [&str; 2] = ["agent_c0", "agent_c1"];

/// The connect's connection: the one [`deployed`] names for [`ACCOUNT`].
fn connection() -> String {
    connection_of(ACCOUNT)
}

fn exchange() -> Slot {
    Slot::token_exchange(&connection())
}

fn executor() -> Slot {
    Slot::executor(ACCOUNT)
}

fn tick(m: &mut Manager, secs: i64) {
    block_on(m.tick(at(secs))).unwrap();
}

/// The executor's starts, by generation.
fn starts(m: &mut Manager) -> Vec<u64> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::Start {
                slot,
                generation,
                lifetime,
                ..
            } if *slot == executor() => {
                assert_eq!(*lifetime, None, "an executor runs until it is stopped");
                Some(*generation)
            }
            _ => None,
        })
        .collect()
}

fn stops(m: &mut Manager) -> Vec<u64> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::Stop {
                slot, generation, ..
            } if *slot == executor() => Some(*generation),
            _ => None,
        })
        .collect()
}

/// The executor's records, as (event type, generation, cause).
fn records(journal: &Journal) -> Vec<(String, i64, String)> {
    journal
        .records()
        .into_iter()
        .filter(|(_, p)| p.get("process").and_then(Value::as_str) == Some("executor"))
        .map(|(t, p)| {
            assert_eq!(p.get("subject").and_then(Value::as_str), Some(ACCOUNT));
            let generation = p.get("generation").and_then(Value::as_int).unwrap();
            let cause = p.get("cause").and_then(Value::as_str).unwrap().to_owned();
            (t, i64::try_from(generation).unwrap(), cause)
        })
        .collect()
}

fn record(event_type: &str, generation: i64, cause: &str) -> (String, i64, String) {
    (event_type.to_owned(), generation, cause.to_owned())
}

fn exchanged() -> (String, i64, String) {
    record("ProcessStartIssued", 1, "exchanged")
}

fn clean(m: &mut Manager) {
    let violations = &m.orchestrator().violations;
    assert!(violations.is_empty(), "{violations:?}");
}

/// A manager with no deployment, an OAuth request for the connect committed at 0, and its token
/// exchange started at 1.
fn exchanging() -> (Journal, Manager) {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    journal.services(&requested(&connection(), ACCOUNT, Some("oauth")), at(0));
    journal.services(&roles_made(&connection()), at(0));
    tick(&mut m, 1);
    assert_eq!(m.orchestrator().fake.live(&exchange()), 1);
    (journal, m)
}

/// [`exchanging`], then the exchange succeeds and the manager's pass at 2 reads it.
fn exchanged_at_2() -> (Journal, Manager) {
    let (journal, mut m) = exchanging();
    m.orchestrator().fake.succeed(&exchange());
    tick(&mut m, 2);
    (journal, m)
}

#[test]
fn an_exchange_that_succeeds_while_its_request_is_open_starts_the_accounts_executor_journaled_first()
 {
    let (journal, mut m) = exchanged_at_2();
    assert_eq!(records(&journal), [exchanged()]);
    assert_eq!(starts(&mut m), [1]);
    assert_eq!(m.orchestrator().fake.live(&executor()), 1);
    assert_eq!(
        m.orchestrator().fake.live(&exchange()),
        0,
        "an exchange that succeeded has exited"
    );
    tick(&mut m, 3);
    assert_eq!(starts(&mut m), [1], "a running executor is left alone");
    assert_eq!(records(&journal), [exchanged()]);
    clean(&mut m);
}

#[test]
fn an_exchange_that_fails_or_never_finishes_starts_no_executor() {
    let (journal, mut m) = exchanging();
    m.orchestrator().fake.crash(&exchange());
    tick(&mut m, 2);
    tick(&mut m, 700);
    journal.services(&refused(&connection(), "connect"), at(701));
    tick(&mut m, 702);
    let (journal_late, mut late) = exchanging();
    tick(&mut late, 700);
    journal_late.services(&refused(&connection(), "connect"), at(701));
    tick(&mut late, 702);
    for (journal, m) in [(&journal, &mut m), (&journal_late, &mut late)] {
        assert!(starts(m).is_empty() && records(journal).is_empty());
        clean(m);
    }
}

#[test]
fn a_success_read_after_its_request_closed_starts_nothing() {
    for closing in [
        refused(&connection(), "connect"),
        established(&connection(), ACCOUNT),
    ] {
        let (journal, mut m) = exchanging();
        m.orchestrator().fake.succeed(&exchange());
        journal.services(&closing, at(2));
        tick(&mut m, 3);
        tick(&mut m, 4);
        assert!(starts(&mut m).is_empty() && records(&journal).is_empty());
        clean(&mut m);
    }
}

#[test]
fn the_connects_executor_runs_on_through_establishment_with_no_agent() {
    let (journal, mut m) = exchanged_at_2();
    journal.services(&established(&connection(), ACCOUNT), at(3));
    tick(&mut m, 4);
    tick(&mut m, 5_000);
    assert_eq!(m.orchestrator().fake.live(&executor()), 1);
    assert!(stops(&mut m).is_empty());
    assert_eq!(records(&journal), [exchanged()]);
    clean(&mut m);
}

#[test]
fn a_refusal_or_a_revocation_stops_the_connects_executor_journaled_first_as_connection_closed() {
    let (journal, mut m) = exchanged_at_2();
    journal.services(&refused(&connection(), "connect"), at(3));
    tick(&mut m, 4);
    let (journal_revoked, mut revoked_m) = exchanged_at_2();
    journal_revoked.services(&established(&connection(), ACCOUNT), at(3));
    tick(&mut revoked_m, 4);
    journal_revoked.services(&revoked(&connection()), at(5));
    tick(&mut revoked_m, 6);
    for (journal, m) in [(&journal, &mut m), (&journal_revoked, &mut revoked_m)] {
        assert_eq!(
            records(journal),
            [
                exchanged(),
                record("ProcessStopIssued", 1, "connection_closed")
            ]
        );
        assert_eq!(stops(m), [1]);
        assert_eq!(m.orchestrator().fake.live(&executor()), 0);
        tick(m, 7);
        tick(m, 5_000);
        assert_eq!(
            starts(m),
            [1],
            "a closed connection's executor never starts again"
        );
        clean(m);
    }
}

#[test]
fn a_deployed_agent_keeps_the_executor_through_a_revocation_and_its_stop_is_agent_stopped() {
    let (journal, mut m) = exchanged_at_2();
    journal.services(&established(&connection(), ACCOUNT), at(3));
    journal.services(&deployed(AGENTS[0], ACCOUNT), at(4));
    tick(&mut m, 5);
    journal.services(&revoked(&connection()), at(6));
    tick(&mut m, 7);
    assert_eq!(m.orchestrator().fake.live(&executor()), 1);
    assert_eq!(
        starts(&mut m),
        [1],
        "the deployment shares the running executor"
    );
    journal.services(&stopped(AGENTS[0], ACCOUNT), at(8));
    tick(&mut m, 9);
    assert_eq!(
        records(&journal),
        [exchanged(), record("ProcessStopIssued", 1, "agent_stopped")]
    );
    assert_eq!(stops(&mut m), [1]);
    clean(&mut m);
}

#[test]
fn an_agent_stopping_while_the_connection_lives_leaves_the_executor_until_the_connection_closes() {
    let (journal, mut m) = exchanged_at_2();
    journal.services(&established(&connection(), ACCOUNT), at(3));
    journal.services(&deployed(AGENTS[0], ACCOUNT), at(4));
    tick(&mut m, 5);
    journal.services(&stopped(AGENTS[0], ACCOUNT), at(6));
    tick(&mut m, 7);
    assert_eq!(m.orchestrator().fake.live(&executor()), 1);
    assert!(stops(&mut m).is_empty());
    journal.services(&revoked(&connection()), at(8));
    tick(&mut m, 9);
    assert_eq!(
        records(&journal),
        [
            exchanged(),
            record("ProcessStopIssued", 1, "connection_closed")
        ]
    );
    assert_eq!(stops(&mut m), [1]);
    clean(&mut m);
}

#[test]
fn the_connects_executor_restarts_after_a_crash_and_answers_a_connection_kill_switch_at_once() {
    let (journal, mut m) = exchanged_at_2();
    m.orchestrator().fake.crash(&executor());
    tick(&mut m, 3);
    tick(&mut m, 5);
    m.orchestrator().fake.crash(&executor());
    let (id, switch) = kill("connection", &connection(), 0);
    journal.services(&switch, at(6));
    tick(&mut m, 6);
    assert_eq!(
        records(&journal),
        [
            exchanged(),
            record("ProcessStartIssued", 2, "restart"),
            record("ProcessStartIssued", 3, "kill_switch"),
        ],
        "a kill switch on the connection ({id}) reaches its executor before establishment"
    );
    assert_eq!(starts(&mut m), [1, 2, 3]);
    clean(&mut m);
}

#[test]
fn exchanged_is_journaled_once_across_the_managers_restart_and_the_pods_removal() {
    let (journal, m) = exchanged_at_2();
    let mut m = restart(m);
    m.orchestrator().fake.collect();
    tick(&mut m, 3);
    let mut m = restart(m);
    tick(&mut m, 4);
    assert_eq!(records(&journal), [exchanged()]);
    assert_eq!(starts(&mut m), [1]);
    assert_eq!(m.orchestrator().fake.live(&executor()), 1);
    assert!(
        m.orchestrator()
            .fake
            .instances()
            .iter()
            .all(|i| i.slot != exchange()),
        "the succeeded exchange is collected"
    );
    clean(&mut m);
}

#[test]
fn an_account_whose_executor_already_ran_journals_no_exchanged_start() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[(AGENTS[0], ACCOUNT)]));
    journal.services(&deployed(AGENTS[0], ACCOUNT), at(1));
    tick(&mut m, 2);
    journal.services(&stopped(AGENTS[0], ACCOUNT), at(3));
    tick(&mut m, 4);
    let again = Slot::token_exchange("conn_01J8Z2AGAIN");
    journal.services(
        &requested("conn_01J8Z2AGAIN", ACCOUNT, Some("oauth")),
        at(5),
    );
    journal.services(&roles_made("conn_01J8Z2AGAIN"), at(5));
    tick(&mut m, 6);
    m.orchestrator().fake.succeed(&again);
    tick(&mut m, 7);
    tick(&mut m, 8);
    assert_eq!(
        records(&journal),
        [
            record("ProcessStartIssued", 1, "deployed"),
            record("ProcessStopIssued", 1, "agent_stopped"),
        ],
        "rule 167 holds `exchanged` to generation 1, which this account's executor has used"
    );
    assert_eq!(starts(&mut m), [1]);
    clean(&mut m);
}

/// One step of the property's sequences.
#[derive(Debug, Clone)]
enum Op {
    /// The exchange exits successfully, if it is running.
    Succeed,
    /// The executor exits, if it is running.
    Crash,
    Establish,
    Refuse,
    Revoke,
    Deploy(usize),
    Stop(usize),
    /// The manager's pass, this many seconds after the last event.
    Tick(i64),
    Restart,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        2 => Just(Op::Succeed),
        1 => Just(Op::Crash),
        1 => Just(Op::Establish),
        1 => Just(Op::Refuse),
        1 => Just(Op::Revoke),
        1 => (0..AGENTS.len()).prop_map(Op::Deploy),
        1 => (0..AGENTS.len()).prop_map(Op::Stop),
        4 => (1_i64..400).prop_map(Op::Tick),
        1 => Just(Op::Restart),
    ]
}

/// The oracle: what the operations it generated say, never the manager's fold.
#[derive(Debug, Default)]
struct Oracle {
    /// A record closed the request: an establishment, a refusal or a revocation.
    request_closed: bool,
    established: bool,
    refused: bool,
    revoked: bool,
    /// The exchange's pod is running; it starts in the first pass, while the request is open.
    exchange_running: bool,
    exchange_succeeded: bool,
    /// A pass read the success while the request was open: the executor's first start.
    exchanged: bool,
    /// Agents deployed on the established, unrevoked connection, and not stopped since.
    live_agents: Vec<usize>,
    deployed: Vec<usize>,
    stopped: Vec<usize>,
    /// The last record that took a reason to want the executor away.
    last_reason: Option<&'static str>,
    crashes: usize,
}

impl Oracle {
    fn wants_executor(&self) -> bool {
        !self.live_agents.is_empty() || (self.exchanged && !self.refused && !self.revoked)
    }
}

fn run(ops: &[Op], seed: Seed) {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    m.orchestrator().seed = seed;
    journal.services(&requested(&connection(), ACCOUNT, Some("oauth")), at(0));
    journal.services(&roles_made(&connection()), at(0));
    let mut oracle = Oracle::default();
    let mut now = 1_i64;
    let mut seen = 0;
    let pass = |m: &mut Manager, oracle: &mut Oracle, now: i64, seen: &mut usize| {
        if !oracle.request_closed && !oracle.exchange_running && !oracle.exchange_succeeded {
            oracle.exchange_running = true;
        }
        if oracle.exchange_succeeded && !oracle.request_closed && !oracle.exchanged {
            oracle.exchanged = true;
        }
        tick(m, now);
        let all = records(&journal);
        for (t, _, cause) in &all[*seen..] {
            if t == "ProcessStopIssued" {
                assert_eq!(Some(cause.as_str()), oracle.last_reason, "{all:?}");
            }
            if cause == "exchanged" {
                assert!(oracle.exchanged, "{all:?}");
            }
        }
        *seen = all.len();
        assert!(
            oracle.wants_executor() || m.orchestrator().fake.live(&executor()) == 0,
            "an executor nothing wants is stopped in the pass: {all:?}"
        );
        if oracle.request_closed {
            oracle.exchange_running = false;
        }
    };
    pass(&mut m, &mut oracle, now, &mut seen);
    for op in ops {
        now += 1;
        match op {
            Op::Succeed if oracle.exchange_running && !oracle.exchange_succeeded => {
                m.orchestrator().fake.succeed(&exchange());
                oracle.exchange_succeeded = true;
            }
            Op::Crash if oracle.crashes < 2 && m.orchestrator().fake.live(&executor()) == 1 => {
                m.orchestrator().fake.crash(&executor());
                oracle.crashes += 1;
            }
            Op::Establish if !oracle.request_closed => {
                journal.services(&established(&connection(), ACCOUNT), at(now));
                oracle.request_closed = true;
                oracle.established = true;
            }
            Op::Refuse if !oracle.request_closed => {
                journal.services(&refused(&connection(), "connect"), at(now));
                oracle.request_closed = true;
                oracle.refused = true;
                if oracle.exchanged {
                    oracle.last_reason = Some("connection_closed");
                }
            }
            Op::Revoke if oracle.established && !oracle.revoked => {
                journal.services(&revoked(&connection()), at(now));
                oracle.revoked = true;
                if oracle.exchanged {
                    oracle.last_reason = Some("connection_closed");
                }
            }
            Op::Deploy(agent) if !oracle.deployed.contains(agent) => {
                journal.services(&deployed(AGENTS[*agent], ACCOUNT), at(now));
                oracle.deployed.push(*agent);
                if oracle.established && !oracle.revoked {
                    oracle.live_agents.push(*agent);
                }
            }
            Op::Stop(agent)
                if oracle.deployed.contains(agent) && !oracle.stopped.contains(agent) =>
            {
                journal.services(&stopped(AGENTS[*agent], ACCOUNT), at(now));
                oracle.stopped.push(*agent);
                if oracle.live_agents.contains(agent) {
                    oracle.live_agents.retain(|a| a != agent);
                    oracle.last_reason = Some("agent_stopped");
                }
            }
            Op::Tick(secs) => {
                now += secs;
                pass(&mut m, &mut oracle, now, &mut seen);
            }
            Op::Restart => m = restart(m),
            _ => {}
        }
    }
    for _ in 0..3 {
        now += 1_000;
        pass(&mut m, &mut oracle, now, &mut seen);
    }
    let wanted = usize::from(oracle.wants_executor());
    assert_eq!(
        m.orchestrator().fake.live(&executor()),
        wanted,
        "settled, the executor runs exactly while it is wanted"
    );
    let firsts = records(&journal)
        .iter()
        .filter(|(t, g, _)| t == "ProcessStartIssued" && *g == 1)
        .count();
    assert!(firsts <= 1, "one first start");
    clean(&mut m);
}

fn runner() -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases: 128,
        failure_persistence: None,
        ..ProptestConfig::default()
    })
}

fn sequences() -> impl Strategy<Value = Vec<Op>> {
    proptest::collection::vec(op(), 0..40)
}

#[test]
fn the_connects_executor_is_journaled_first_and_runs_exactly_while_wanted() {
    if let Err(failure) = runner().run(&sequences(), |ops| {
        run(&ops, Seed::None);
        Ok(())
    }) {
        panic!("{failure}");
    }
    for seed in [Seed::DropStarts, Seed::Unjournaled, Seed::SecondInstance] {
        let caught = runner().run(&sequences(), |ops| {
            run(&ops, seed);
            Ok(())
        });
        assert!(caught.is_err(), "the oracle misses {seed:?}");
    }
}
