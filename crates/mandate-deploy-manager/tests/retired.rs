//! DEC-2344 items 1 and 3 (E21-33, E6-5; journal spec §9.21, trading spec §5.5): an account's
//! executor that no agent wants any more is still reached by a kill switch that covers it. A
//! covering switch gives such an executor exactly one start, cause `kill_switch`, journaled first;
//! a switch the executor has not answered keeps it from being stopped by the account's last
//! retirement; and nothing else ever starts it. The executor itself ends such a generation once
//! nothing wants it and its switches are applied (DEC-2343), which the manager reads as
//! `Succeeded` and never restarts.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    Journal, Manager, accounts, at, block_on, connection_of, deployed, kill, manager, restart,
    stopped,
};
use mandate_canon::Value;
use mandate_deploy_manager::Slot;
use mandate_deploy_manager::fake::Call;
use proptest::collection::vec;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};

const ACCOUNT: &str = "01J8Z2ACCT00000000000000A1";
const OTHER: &str = "01J8Z2ACCT00000000000000A2";

fn ex(account: &str) -> Slot {
    Slot::executor(account)
}

fn tick(m: &mut Manager, secs: i64) {
    block_on(m.tick(at(secs))).unwrap();
}

fn starts(m: &mut Manager, slot: &Slot) -> Vec<u64> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::Start {
                slot: s,
                generation,
                ..
            } if s == slot => Some(*generation),
            _ => None,
        })
        .collect()
}

fn stops(m: &mut Manager, slot: &Slot) -> Vec<u64> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::Stop {
                slot: s,
                generation,
                ..
            } if s == slot => Some(*generation),
            _ => None,
        })
        .collect()
}

/// The `cause` and `causation_id` of every executor start journaled for `account`.
fn executor_starts(journal: &Journal, account: &str) -> Vec<(String, Option<String>)> {
    journal
        .rows()
        .into_iter()
        .filter(|row| row.event_type == "ProcessStartIssued")
        .filter_map(|row| {
            let body = mandate_canon::parse(&row.body).ok()?;
            let payload = body.get("payload")?;
            (payload.get("process").and_then(Value::as_str) == Some("executor")
                && payload.get("subject").and_then(Value::as_str) == Some(account))
            .then(|| {
                (
                    payload
                        .get("cause")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    body.get("causation_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                )
            })
        })
        .collect()
}

fn clean(m: &mut Manager) {
    let violations = &m.orchestrator().violations;
    assert!(violations.is_empty(), "{violations:?}");
}

/// `agent_a` deployed on [`ACCOUNT`], its two processes up, then retired: the manager has stopped
/// both, so the account's executor is wanted by nothing.
fn retired(journal: &Journal) -> Manager {
    let mut m = manager(
        journal,
        accounts(&[("agent_a", ACCOUNT), ("agent_b", OTHER)]),
    );
    journal.services(&journal.deployed("agent_a"), at(1));
    tick(&mut m, 1);
    journal.services(&stopped("agent_a", ACCOUNT), at(2));
    tick(&mut m, 2);
    tick(&mut m, 3);
    assert_eq!(starts(&mut m, &ex(ACCOUNT)), vec![1]);
    assert_eq!(
        stops(&mut m, &ex(ACCOUNT)),
        vec![1],
        "the last retirement stops it"
    );
    m
}

#[test]
#[ignore = "pending E21-33"]
fn a_covering_kill_switch_starts_an_executor_no_agent_wants_once_journaled_first() {
    for (scope, subject) in [
        ("agent", "agent_a".to_owned()),
        ("connection", connection_of(ACCOUNT)),
        ("workspace", "ws_01J8Z2".to_owned()),
    ] {
        let journal = Journal::new();
        let mut m = retired(&journal);
        let (id, draft) = kill(scope, &subject, 1);
        journal.services(&draft, at(10));
        tick(&mut m, 10);
        assert_eq!(
            starts(&mut m, &ex(ACCOUNT)),
            vec![1, 2],
            "{scope}: one start, at once"
        );
        assert_eq!(
            executor_starts(&journal, ACCOUNT).last(),
            Some(&("kill_switch".to_owned(), Some(id.clone()))),
            "{scope}: journaled first, caused by the switch"
        );
        for s in 11..400 {
            tick(&mut m, s);
        }
        assert_eq!(
            starts(&mut m, &ex(ACCOUNT)),
            vec![1, 2],
            "{scope}: the switch read again starts nothing more"
        );
        assert_eq!(
            stops(&mut m, &ex(ACCOUNT)),
            vec![1],
            "{scope}: nor is that start stopped"
        );
        clean(&mut m);
    }
}

#[test]
#[ignore = "pending E21-33"]
fn each_covering_kill_switch_gives_the_executor_no_agent_wants_one_start() {
    let journal = Journal::new();
    let mut m = retired(&journal);
    for (n, second) in [(1, 10), (2, 20), (3, 30)] {
        journal.services(
            &kill("connection", &connection_of(ACCOUNT), n).1,
            at(second),
        );
        tick(&mut m, second);
        m.orchestrator().fake.succeed(&ex(ACCOUNT));
        tick(&mut m, second + 1);
    }
    assert_eq!(starts(&mut m, &ex(ACCOUNT)), vec![1, 2, 3, 4]);
    assert_eq!(
        executor_starts(&journal, ACCOUNT)
            .iter()
            .filter(|(cause, _)| cause == "kill_switch")
            .count(),
        3
    );
    clean(&mut m);
}

#[test]
#[ignore = "pending E21-33"]
fn a_kill_switch_start_that_ended_on_its_own_is_never_restarted() {
    let journal = Journal::new();
    let mut m = retired(&journal);
    journal.services(&kill("agent", "agent_a", 1).1, at(10));
    tick(&mut m, 10);
    m.orchestrator().fake.succeed(&ex(ACCOUNT));
    for s in 11..1000 {
        tick(&mut m, s);
    }
    assert_eq!(starts(&mut m, &ex(ACCOUNT)), vec![1, 2]);
    clean(&mut m);
}

#[test]
#[ignore = "pending E21-33"]
fn a_kill_switch_start_that_crash_loops_before_it_ends_raises_kill_switch_unapplied() {
    let journal = Journal::new();
    let mut m = retired(&journal);
    journal.services(&kill("connection", &connection_of(ACCOUNT), 1).1, at(10));
    tick(&mut m, 10);
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2],
        "the switch's one start"
    );
    let mut second = 11;
    for _ in 0..8 {
        let before = starts(&mut m, &ex(ACCOUNT)).len();
        m.orchestrator().fake.crash(&ex(ACCOUNT));
        while second < 4000 && starts(&mut m, &ex(ACCOUNT)).len() == before {
            tick(&mut m, second);
            second += 1;
        }
    }
    assert!(
        starts(&mut m, &ex(ACCOUNT)).len() > 3,
        "a crash before it ends is restarted, so one crash loses no switch"
    );
    assert!(
        journal
            .rows()
            .iter()
            .any(|row| row.event_type == "ProcessCrashLooped"),
        "until the crash-loop bound"
    );
    let codes: Vec<&str> = m.alerts().0.iter().map(|alert| alert.code).collect();
    assert_eq!(
        codes,
        vec!["kill_switch_unapplied"],
        "an unapplied switch raises its own opaque alert, once, in place of crash_loop"
    );
    clean(&mut m);
}

#[test]
fn no_kill_switch_and_none_that_covers_another_account_starts_an_executor_no_agent_wants() {
    let journal = Journal::new();
    let mut m = retired(&journal);
    journal.services(&kill("agent", "agent_b", 1).1, at(10));
    journal.services(&kill("connection", &connection_of(OTHER), 2).1, at(10));
    for s in 10..400 {
        tick(&mut m, s);
    }
    assert_eq!(starts(&mut m, &ex(ACCOUNT)), vec![1]);
    clean(&mut m);
}

#[test]
#[ignore = "pending E21-33"]
fn the_last_retirement_does_not_stop_an_executor_with_a_switch_it_has_not_answered() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[("agent_a", ACCOUNT)]));
    journal.services(&journal.deployed("agent_a"), at(1));
    tick(&mut m, 1);
    journal.services(&kill("connection", &connection_of(ACCOUNT), 1).1, at(2));
    journal.services(&stopped("agent_a", ACCOUNT), at(2));
    for s in 2..400 {
        tick(&mut m, s);
    }
    assert!(
        stops(&mut m, &ex(ACCOUNT)).is_empty(),
        "the switch that came before the retirement keeps it up"
    );
    m.orchestrator().fake.succeed(&ex(ACCOUNT));
    for s in 400..800 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1],
        "and once it ends on its own nothing starts it again"
    );
    clean(&mut m);
}

const AGENTS: [(&str, &str); 3] = [
    ("agent_0", ACCOUNT),
    ("agent_1", ACCOUNT),
    ("agent_2", OTHER),
];
const ACCOUNTS: [&str; 2] = [ACCOUNT, OTHER];

#[derive(Debug, Clone)]
enum Op {
    Deploy(usize),
    Stop(usize),
    KillAgent(usize),
    KillConnection(usize),
    KillWorkspace,
    /// The executor of an account no agent wants ends on its own, as DEC-2343's executor does once
    /// its switches are applied.
    Succeed(usize),
    Advance(i64),
    Restart,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..AGENTS.len()).prop_map(Op::Deploy),
        2 => (0..AGENTS.len()).prop_map(Op::Stop),
        2 => (0..AGENTS.len()).prop_map(Op::KillAgent),
        2 => (0..ACCOUNTS.len()).prop_map(Op::KillConnection),
        1 => Just(Op::KillWorkspace),
        2 => (0..ACCOUNTS.len()).prop_map(Op::Succeed),
        2 => prop_oneof![0_i64..4, 0_i64..700].prop_map(Op::Advance),
        1 => Just(Op::Restart),
    ]
}

/// The oracle's own view, from the operations it generated alone.
#[derive(Default)]
struct Oracle {
    deployed: BTreeSet<usize>,
    stopped: BTreeSet<usize>,
    /// The accounts whose executor has an instance that may run, by the oracle's own count of the
    /// starts, stops, and ends it saw.
    live: BTreeSet<&'static str>,
    /// Per account, the switches that covered its executor since its last start.
    pending: BTreeMap<&'static str, BTreeSet<String>>,
}

impl Oracle {
    fn wanted(&self, account: &str) -> bool {
        self.deployed
            .iter()
            .any(|i| !self.stopped.contains(i) && AGENTS[*i].1 == account)
    }

    /// Whether the manager has ever had an executor for `account`, which only a deployment on it
    /// makes: a switch covers no executor that never existed.
    fn known(&self, account: &str) -> bool {
        self.deployed.iter().any(|i| AGENTS[*i].1 == account)
    }
}

/// Every executor start and stop journaled after the first `from` rows: its account, whether it is
/// a start, its cause, and its causation id.
fn executor_records(journal: &Journal, from: usize) -> Vec<(String, bool, String, Option<String>)> {
    journal
        .rows()
        .into_iter()
        .skip(from)
        .filter(|row| {
            matches!(
                row.event_type.as_str(),
                "ProcessStartIssued" | "ProcessStopIssued"
            )
        })
        .filter_map(|row| {
            let body = mandate_canon::parse(&row.body).ok()?;
            let payload = body.get("payload")?;
            (payload.get("process").and_then(Value::as_str) == Some("executor")).then(|| {
                (
                    payload
                        .get("subject")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    row.event_type == "ProcessStartIssued",
                    payload
                        .get("cause")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    body.get("causation_id")
                        .and_then(Value::as_str)
                        .map(str::to_owned),
                )
            })
        })
        .collect()
}

fn check(ops: &[Op]) -> Result<(), TestCaseError> {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&AGENTS));
    let mut oracle = Oracle::default();
    let mut now = 1_i64;
    let mut seen = journal.rows().len();
    let mut nth = 0_u32;
    for op in ops {
        let mut switches: Vec<(String, Vec<&'static str>)> = Vec::new();
        match op {
            Op::Deploy(i) => {
                let (agent, account) = AGENTS[*i];
                if oracle.deployed.insert(*i) {
                    journal.services(&deployed(agent, account), at(now));
                }
            }
            Op::Stop(i) => {
                let (agent, account) = AGENTS[*i];
                if oracle.deployed.contains(i) && oracle.stopped.insert(*i) {
                    journal.services(&stopped(agent, account), at(now));
                }
            }
            Op::KillAgent(i) => {
                nth += 1;
                let (id, draft) = kill("agent", AGENTS[*i].0, nth);
                journal.services(&draft, at(now));
                let covered = if oracle.deployed.contains(i) {
                    vec![AGENTS[*i].1]
                } else {
                    vec![]
                };
                switches.push((id, covered));
            }
            Op::KillConnection(j) => {
                nth += 1;
                let (id, draft) = kill("connection", &connection_of(ACCOUNTS[*j]), nth);
                journal.services(&draft, at(now));
                let covered = if oracle.known(ACCOUNTS[*j]) {
                    vec![ACCOUNTS[*j]]
                } else {
                    vec![]
                };
                switches.push((id, covered));
            }
            Op::KillWorkspace => {
                nth += 1;
                let (id, draft) = kill("workspace", "ws_01J8Z2", nth);
                journal.services(&draft, at(now));
                let covered = ACCOUNTS
                    .iter()
                    .copied()
                    .filter(|a| oracle.known(a))
                    .collect();
                switches.push((id, covered));
            }
            Op::Succeed(j) => {
                let account = ACCOUNTS[*j];
                if !oracle.wanted(account) && oracle.live.remove(account) {
                    m.orchestrator().fake.succeed(&ex(account));
                    oracle.pending.remove(account);
                }
            }
            Op::Advance(secs) => now += secs,
            Op::Restart => m = restart(m),
        }
        for (id, covered) in &switches {
            for account in covered {
                oracle
                    .pending
                    .entry(account)
                    .or_default()
                    .insert(id.clone());
            }
        }
        let waiting: BTreeSet<&'static str> = ACCOUNTS
            .iter()
            .copied()
            .filter(|a| {
                !oracle.wanted(a)
                    && !oracle.live.contains(a)
                    && oracle.pending.get(a).is_some_and(|p| !p.is_empty())
            })
            .collect();
        block_on(m.tick(at(now))).map_err(|h| TestCaseError::fail(format!("{h:?}")))?;
        let records = executor_records(&journal, seen);
        seen = journal.rows().len();
        for account in ACCOUNTS {
            let mine: Vec<_> = records.iter().filter(|r| r.0 == account).collect();
            let started: Vec<_> = mine.iter().filter(|r| r.1).collect();
            if !oracle.wanted(account) {
                for (_, _, cause, causation) in &started {
                    prop_assert_eq!(
                        cause.as_str(),
                        "kill_switch",
                        "{}: a start no agent wants answers a switch",
                        account
                    );
                    prop_assert!(
                        causation.as_ref().is_some_and(|c| oracle
                            .pending
                            .get(account)
                            .is_some_and(|p| p.contains(c))),
                        "{}: and its cause is a switch that covered it since its last start",
                        account
                    );
                }
                prop_assert_eq!(
                    started.len(),
                    usize::from(waiting.contains(account)),
                    "{}: exactly one start for the switches that found it down, none otherwise",
                    account
                );
                let pending = oracle.pending.get(account).is_some_and(|p| !p.is_empty());
                if pending || !started.is_empty() {
                    prop_assert!(
                        mine.iter().all(|r| r.1),
                        "{}: a switch not yet answered keeps it from being stopped",
                        account
                    );
                }
            }
            if !started.is_empty() {
                oracle.pending.remove(account);
                oracle.live.insert(account);
            }
            if mine.iter().any(|r| !r.1) {
                oracle.live.remove(account);
            }
        }
    }
    Ok(())
}

#[test]
#[ignore = "pending E21-33"]
fn an_executor_no_agent_wants_starts_once_per_covering_kill_switch_and_never_without_one() {
    let mut runner = TestRunner::new(Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    });
    let outcome = runner.run(&vec(op(), 1..60), |ops| check(&ops));
    assert!(outcome.is_ok(), "{outcome:?}");
}
