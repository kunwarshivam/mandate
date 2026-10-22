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
        vec![UNAPPLIED],
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
    tick(&mut m, 400);
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2],
        "a successful end answers only the switches committed before its start, so one more start"
    );
    m.orchestrator().fake.succeed(&ex(ACCOUNT));
    for s in 401..800 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2],
        "and once that start ends nothing starts it again"
    );
    clean(&mut m);
}

const AGENTS: [(&str, &str); 3] = [
    ("agent_0", ACCOUNT),
    ("agent_1", ACCOUNT),
    ("agent_2", OTHER),
];
const ACCOUNTS: [&str; 2] = [ACCOUNT, OTHER];
const UNAPPLIED: &str = "kill_switch_unapplied";

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
    /// The executor of an account no agent wants crashes before it ends.
    Crash(usize),
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
        4 => (0..ACCOUNTS.len()).prop_map(Op::Crash),
        3 => prop_oneof![0_i64..4, 0_i64..40, 0_i64..700].prop_map(Op::Advance),
        1 => Just(Op::Restart),
    ]
}

/// The oracle's own view, from the operations it generated alone.
#[derive(Default)]
struct Oracle {
    deployed: BTreeSet<usize>,
    stopped: BTreeSet<usize>,
    /// The accounts whose executor has an instance that may run, by the oracle's own count of the
    /// starts, stops, ends, and crashes it saw.
    live: BTreeSet<&'static str>,
    /// Per account, the switches that covered its executor since its last start: only a start
    /// after them answers them, never a successful end (journal spec §9.21, v0.76).
    pending: BTreeMap<&'static str, BTreeSet<String>>,
    /// The accounts whose executor, no agent wanting it, crashed and has not been started since.
    crashed: BTreeSet<&'static str>,
    /// The accounts whose executor's crash loop is journaled: only a switch starts it again.
    looped: BTreeSet<&'static str>,
    /// The accounts whose running generations began with a kill switch's start no agent wanted:
    /// its restarts carry the switch, and its crash loop leaves the switch unapplied.
    unapplied_if_looped: BTreeSet<&'static str>,
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

    fn pending(&self, account: &str) -> bool {
        self.pending.get(account).is_some_and(|p| !p.is_empty())
    }
}

/// One executor record of the control stream: its account, its type, its cause, and its causation.
struct Executed {
    account: String,
    kind: &'static str,
    cause: String,
    causation: Option<String>,
}

/// Every executor start, stop, and crash loop journaled after the first `from` rows.
fn executor_records(journal: &Journal, from: usize) -> Vec<Executed> {
    journal
        .rows()
        .into_iter()
        .skip(from)
        .filter_map(|row| {
            let kind = match row.event_type.as_str() {
                "ProcessStartIssued" => "start",
                "ProcessStopIssued" => "stop",
                "ProcessCrashLooped" => "looped",
                _ => return None,
            };
            let body = mandate_canon::parse(&row.body).ok()?;
            let payload = body.get("payload")?;
            let text = |member: &str| {
                payload
                    .get(member)
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned()
            };
            (text("process") == "executor").then(|| Executed {
                account: text("subject"),
                kind,
                cause: text("cause"),
                causation: body
                    .get("causation_id")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
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
    let mut alerted = 0;
    let mut nth = 0_u32;
    for op in ops {
        let mut switches: Vec<(String, Vec<&'static str>)> = Vec::new();
        match op {
            Op::Deploy(i) => {
                let (agent, account) = AGENTS[*i];
                if oracle.deployed.insert(*i) {
                    journal.services(&deployed(agent, account), at(now));
                    oracle.crashed.remove(account);
                    oracle.looped.remove(account);
                    oracle.unapplied_if_looped.remove(account);
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
                    oracle.unapplied_if_looped.remove(account);
                }
            }
            Op::Crash(j) => {
                let account = ACCOUNTS[*j];
                if !oracle.wanted(account) && oracle.live.remove(account) {
                    m.orchestrator().fake.crash(&ex(account));
                    oracle.crashed.insert(account);
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
            .filter(|a| !oracle.wanted(a) && !oracle.live.contains(a) && oracle.pending(a))
            .collect();
        block_on(m.tick(at(now))).map_err(|h| TestCaseError::fail(format!("{h:?}")))?;
        let records = executor_records(&journal, seen);
        seen = journal.rows().len();
        let mut unapplied = 0;
        for account in ACCOUNTS {
            let mine: Vec<&Executed> = records.iter().filter(|r| r.account == account).collect();
            let started: Vec<&&Executed> = mine.iter().filter(|r| r.kind == "start").collect();
            let looped = mine.iter().any(|r| r.kind == "looped");
            if !oracle.wanted(account) {
                for start in &started {
                    let answers = start.cause == "kill_switch"
                        && start.causation.as_ref().is_some_and(|c| {
                            oracle.pending.get(account).is_some_and(|p| p.contains(c))
                        });
                    let restarts = start.cause == "restart"
                        && oracle.crashed.contains(account)
                        && !oracle.looped.contains(account)
                        && !oracle.pending(account);
                    prop_assert!(
                        answers || restarts,
                        "{}: a start no agent wants answers a switch that covered it since its last start, or restarts a crash before its loop",
                        account
                    );
                }
                if waiting.contains(account) {
                    prop_assert_eq!(
                        started.len(),
                        1,
                        "{}: exactly one start for the switches that found it down",
                        account
                    );
                } else {
                    prop_assert!(
                        started.len() <= usize::from(oracle.crashed.contains(account)),
                        "{}: no start without a switch, but a crash's restart",
                        account
                    );
                }
                if oracle.pending(account) || oracle.unapplied_if_looped.contains(account) {
                    prop_assert!(
                        mine.iter().all(|r| r.kind != "stop"),
                        "{}: a generation a switch keeps is never stopped by the manager",
                        account
                    );
                }
                if looped {
                    prop_assert!(
                        oracle.crashed.contains(account)
                            && oracle.unapplied_if_looped.contains(account),
                        "{}: a crash loop no agent wants follows only a kill switch's crashing start",
                        account
                    );
                    unapplied += 1;
                }
            }
            if looped {
                oracle.looped.insert(account);
                oracle.crashed.remove(account);
                oracle.unapplied_if_looped.remove(account);
            }
            if let Some(last) = started.last() {
                if !oracle.wanted(account) && last.cause == "kill_switch" {
                    oracle.unapplied_if_looped.insert(account);
                }
                oracle.pending.remove(account);
                oracle.crashed.remove(account);
                oracle.live.insert(account);
            }
            if mine.iter().any(|r| r.kind == "stop") {
                oracle.live.remove(account);
            }
        }
        let alerts: Vec<&str> = m
            .alerts()
            .0
            .iter()
            .skip(alerted)
            .map(|alert| alert.code)
            .collect();
        alerted += alerts.len();
        prop_assert_eq!(
            alerts,
            vec![UNAPPLIED; unapplied],
            "each crash loop that leaves a switch unapplied raises kill_switch_unapplied once, and nothing else alerts"
        );
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
