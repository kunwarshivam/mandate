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
    Journal, Manager, accounts, at, block_on, connection_of, deployed, identity, kill, manager,
    restart, revoked_again, stopped,
};
use mandate_canon::Value;
use mandate_deploy_manager::fake::Call;
use mandate_deploy_manager::orchestrator::CRASH_LOOP;
use mandate_deploy_manager::{Cause, Record, Slot, StopCause, backoff};
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
    /// The accounts whose executor's crash loop is journaled: only a switch starts it again, and
    /// that start begins a new run, which may crash, restart, and loop again.
    looped: BTreeSet<&'static str>,
    /// The accounts whose running generations began with a kill switch's start no agent wanted:
    /// its restarts carry the switch, and its crash loop leaves the switch unapplied. A deployment
    /// does not end such a run; only its own successful end does (DEC-2344 item 3).
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

/// A bug seeded after a pass, as a manager with it would have journaled or raised, so each of the
/// oracle's checks is shown to catch one before it is trusted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Bug {
    None,
    /// A `kill_switch` start of an executor no agent wants, with no switch to answer.
    Unswitched,
    /// A second start of an executor no agent wants for the same switch, beside the one it asked for.
    Twice,
    /// A stop of an executor no agent wants whose switch is not answered.
    Stopped,
    /// A crash loop of an executor no agent wants that no kill switch started.
    Looped,
    /// A `crash_loop` alert where only `kill_switch_unapplied` may be raised.
    Alerted,
}

fn inject(journal: &Journal, record: &Record, now: i64) {
    journal.services(&record.draft(&identity(), "paper", at(now)), at(now));
}

/// Whether `bug` is to be seeded now, given the oracle's view before the pass, and seeds it.
fn seed(
    bug: Bug,
    m: &mut Manager,
    journal: &Journal,
    oracle: &Oracle,
    waiting: &BTreeSet<&str>,
    now: i64,
) -> bool {
    let idle = |a: &&str| {
        !oracle.wanted(a) && oracle.known(a) && !oracle.pending(a) && !oracle.live.contains(a)
    };
    let pick = match bug {
        Bug::None => None,
        Bug::Unswitched | Bug::Looped => ACCOUNTS.iter().copied().find(|a| idle(a)),
        Bug::Twice => ACCOUNTS.iter().copied().find(|a| waiting.contains(a)),
        Bug::Stopped => ACCOUNTS
            .iter()
            .copied()
            .find(|a| !oracle.wanted(a) && oracle.pending(a)),
        Bug::Alerted => ACCOUNTS.iter().copied().find(|a| oracle.known(a)),
    };
    let Some(account) = pick else {
        return false;
    };
    let slot = ex(account);
    let record = match bug {
        Bug::None => return false,
        Bug::Unswitched => Record::Start {
            slot,
            generation: 900,
            cause: Cause::KillSwitch,
            kill_switch: Some(kill("workspace", "ws_01J8Z2", 900).0),
        },
        Bug::Twice => Record::Start {
            slot,
            generation: 901,
            cause: Cause::KillSwitch,
            kill_switch: oracle.pending.get(account).and_then(|p| p.first().cloned()),
        },
        Bug::Stopped => Record::Stop {
            slot,
            generation: 1,
            cause: StopCause::AgentStopped,
        },
        Bug::Looped => Record::CrashLooped {
            slot,
            generation: 1,
            restarts: 5,
            alert: CRASH_LOOP,
        },
        Bug::Alerted => {
            m.alerts().0.push(CRASH_LOOP);
            return true;
        }
    };
    inject(journal, &record, now);
    true
}

fn check(ops: &[Op]) -> Result<(), TestCaseError> {
    check_seeded(ops, Bug::None)
}

fn check_seeded(ops: &[Op], bug: Bug) -> Result<(), TestCaseError> {
    let mut bug = bug;
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
                    oracle.looped.remove(account);
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
        let rearmed = if matches!(op, Op::Restart) {
            ACCOUNTS
                .iter()
                .filter(|a| oracle.looped.contains(*a) && !oracle.wanted(a))
                .count()
        } else {
            0
        };
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
        if seed(bug, &mut m, &journal, &oracle, &waiting, now) {
            bug = Bug::None;
        }
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
                    oracle.looped.remove(account);
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
            vec![UNAPPLIED; unapplied + rearmed],
            "each crash loop that leaves a switch unapplied raises kill_switch_unapplied once, again once by a restarted manager, and nothing else alerts"
        );
    }
    Ok(())
}

#[test]
fn an_executor_no_agent_wants_starts_once_per_covering_kill_switch_and_never_without_one() {
    let mut runner = TestRunner::new(Config {
        cases: 256,
        failure_persistence: None,
        ..Config::default()
    });
    let outcome = runner.run(&vec(op(), 1..60), |ops| check(&ops));
    assert!(outcome.is_ok(), "{outcome:?}");
}

/// Each check of the oracle fails on the bug seeded for it, before it is trusted.
#[test]
fn every_check_of_the_oracle_catches_its_seeded_bug() {
    let retired = [Op::Deploy(0), Op::Stop(0), Op::Advance(1)];
    let held = [
        Op::Deploy(0),
        Op::KillConnection(0),
        Op::Stop(0),
        Op::Advance(1),
    ];
    let waiting = [
        Op::Deploy(0),
        Op::Stop(0),
        Op::Advance(1),
        Op::KillConnection(0),
    ];
    let cases: [(Bug, &[Op], &str); 5] = [
        (Bug::Unswitched, &retired, "answers a switch"),
        (Bug::Twice, &waiting, "exactly one start"),
        (Bug::Stopped, &held, "never stopped by the manager"),
        (Bug::Looped, &retired, "follows only a kill switch"),
        (Bug::Alerted, &retired, "nothing else alerts"),
    ];
    for (bug, ops, message) in cases {
        let caught = check_seeded(ops, bug).expect_err(&format!("{bug:?} escaped"));
        assert!(caught.to_string().contains(message), "{bug:?}: {caught}");
        check(ops).unwrap();
    }
}

/// A connection revoked after its agent retired: a covering switch still starts the executor, as the
/// account of the connection its last generation ran as, since a revocation destroys the token, not
/// the account (DEC-2344 item 1, DEC-2350 item 3). One that cannot apply it, its login refused,
/// crash-loops and raises `kill_switch_unapplied`; one that applies it and ends is never restarted.
/// The executor's own exit once its switches are applied is lane LV's DEC-2347 item 3.2 test.
#[test]
fn a_revoked_connections_kill_switch_start_runs_as_its_account_and_is_unapplied_if_it_cannot_log_in()
 {
    for ends in [false, true] {
        let journal = Journal::new();
        let mut m = retired(&journal);
        journal.services(&revoked_again(&connection_of(ACCOUNT), 0), at(5));
        tick(&mut m, 5);
        journal.services(&kill("connection", &connection_of(ACCOUNT), 1).1, at(10));
        tick(&mut m, 10);
        let launched: Vec<Option<String>> = m
            .orchestrator()
            .fake
            .calls()
            .iter()
            .filter_map(|c| match c {
                Call::Start {
                    slot, connection, ..
                } if *slot == ex(ACCOUNT) => Some(connection.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            launched,
            vec![Some(connection_of(ACCOUNT)); 2],
            "ends {ends}"
        );
        let mut second = 11;
        if ends {
            m.orchestrator().fake.succeed(&ex(ACCOUNT));
        } else {
            for _ in 0..8 {
                let before = starts(&mut m, &ex(ACCOUNT)).len();
                m.orchestrator().fake.crash(&ex(ACCOUNT));
                while second < 4000 && starts(&mut m, &ex(ACCOUNT)).len() == before {
                    tick(&mut m, second);
                    second += 1;
                }
            }
        }
        for s in second..second + 800 {
            tick(&mut m, s);
        }
        let codes: Vec<&str> = m.alerts().0.iter().map(|alert| alert.code).collect();
        if ends {
            assert_eq!(starts(&mut m, &ex(ACCOUNT)), vec![1, 2]);
            assert!(codes.is_empty(), "{codes:?}");
        } else {
            assert_eq!(codes, vec![UNAPPLIED]);
        }
        clean(&mut m);
    }
}

/// A manager restarted after an executor's switch was left unapplied raises the alert again, once,
/// so a crash between the crash loop's record and its alert loses no alert.
#[test]
fn a_restarted_manager_raises_kill_switch_unapplied_again_once() {
    let journal = Journal::new();
    let mut m = retired(&journal);
    journal.services(&kill("agent", "agent_a", 1).1, at(10));
    tick(&mut m, 10);
    let mut second = 11;
    for _ in 0..8 {
        let before = starts(&mut m, &ex(ACCOUNT)).len();
        m.orchestrator().fake.crash(&ex(ACCOUNT));
        while second < 4000 && starts(&mut m, &ex(ACCOUNT)).len() == before {
            tick(&mut m, second);
            second += 1;
        }
    }
    let mut m = restart(m);
    for s in second..second + 50 {
        tick(&mut m, s);
    }
    let codes: Vec<&str> = m.alerts().0.iter().map(|alert| alert.code).collect();
    assert_eq!(codes, vec![UNAPPLIED, UNAPPLIED]);
}

/// An executor an agent still wants is restarted after any exit, a successful one included: only one
/// no agent wants ends by its own exit.
#[test]
fn a_wanted_executor_that_exits_with_success_is_restarted() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[("agent_a", ACCOUNT)]));
    journal.services(&journal.deployed("agent_a"), at(1));
    tick(&mut m, 1);
    m.orchestrator().fake.succeed(&ex(ACCOUNT));
    for s in 2..10 {
        tick(&mut m, s);
    }
    assert_eq!(starts(&mut m, &ex(ACCOUNT)), vec![1, 2]);
    assert_eq!(
        executor_starts(&journal, ACCOUNT)
            .last()
            .map(|s| s.0.as_str()),
        Some("restart")
    );
    clean(&mut m);
}

/// Crashes the executor of `account` after each start, from `second`, until its crash loop is
/// journaled, and returns the second after it.
fn crash_until_looped(m: &mut Manager, journal: &Journal, account: &str, mut second: i64) -> i64 {
    let loops = |journal: &Journal| {
        journal
            .rows()
            .iter()
            .filter(|row| row.event_type == "ProcessCrashLooped")
            .count()
    };
    let before_loops = loops(journal);
    while loops(journal) == before_loops && second < 8000 {
        let before = starts(m, &ex(account)).len();
        m.orchestrator().fake.crash(&ex(account));
        while second < 8000
            && starts(m, &ex(account)).len() == before
            && loops(journal) == before_loops
        {
            tick(m, second);
            second += 1;
        }
    }
    second
}

/// DEC-2344 item 1, and DEC-2351 item 2: a deployment on the account while a kill switch's
/// generation runs makes the executor wanted again, so a later crash loop of it is an ordinary one:
/// `crash_loop`, never `kill_switch_unapplied`, and a restarted manager agrees.
#[test]
fn a_redeploy_after_a_kill_switch_run_makes_a_later_crash_loop_an_ordinary_one() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", ACCOUNT),
            ("agent_c", ACCOUNT),
            ("agent_b", OTHER),
        ]),
    );
    journal.services(&journal.deployed("agent_a"), at(1));
    tick(&mut m, 1);
    journal.services(&stopped("agent_a", ACCOUNT), at(2));
    tick(&mut m, 2);
    tick(&mut m, 3);
    journal.services(&kill("connection", &connection_of(ACCOUNT), 1).1, at(10));
    tick(&mut m, 10);
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2],
        "the switch's one start"
    );
    journal.services(&journal.deployed("agent_c"), at(11));
    tick(&mut m, 11);
    let second = crash_until_looped(&mut m, &journal, ACCOUNT, 12);
    let codes: Vec<&str> = m.alerts().0.iter().map(|alert| alert.code).collect();
    assert_eq!(
        codes,
        vec![CRASH_LOOP.code],
        "an agent wants it again: an ordinary loop"
    );
    let mut m = restart(m);
    for s in second..second + 50 {
        tick(&mut m, s);
    }
    let codes: Vec<&str> = m.alerts().0.iter().map(|alert| alert.code).collect();
    assert_eq!(
        codes,
        vec![CRASH_LOOP.code, CRASH_LOOP.code],
        "and a restarted manager raises crash_loop again, never kill_switch_unapplied"
    );
}

/// DEC-2344 item 1 and rule 13: a switch that covers the executor after an unapplied loop starts a
/// new run, which is restarted after a crash like the first; its own loop is a second
/// `ProcessCrashLooped` and a second `kill_switch_unapplied`.
#[test]
fn a_covering_switch_after_an_unapplied_loop_starts_a_new_run_that_can_loop_again() {
    let journal = Journal::new();
    let mut m = retired(&journal);
    journal.services(&kill("connection", &connection_of(ACCOUNT), 1).1, at(10));
    tick(&mut m, 10);
    let second = crash_until_looped(&mut m, &journal, ACCOUNT, 11);
    let first_run = starts(&mut m, &ex(ACCOUNT)).len();
    journal.services(&kill("workspace", "ws_01J8Z2", 2).1, at(second));
    tick(&mut m, second);
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)).len(),
        first_run + 1,
        "the later switch's one start"
    );
    let causes: Vec<String> = executor_starts(&journal, ACCOUNT)
        .into_iter()
        .map(|s| s.0)
        .collect();
    assert_eq!(causes.last().map(String::as_str), Some("kill_switch"));
    crash_until_looped(&mut m, &journal, ACCOUNT, second + 1);
    assert!(
        starts(&mut m, &ex(ACCOUNT)).len() > first_run + 2,
        "the new run's crash is restarted"
    );
    let loops = journal
        .rows()
        .iter()
        .filter(|row| row.event_type == "ProcessCrashLooped")
        .count();
    assert_eq!(loops, 2, "each run's loop is journaled");
    let codes: Vec<&str> = m.alerts().0.iter().map(|alert| alert.code).collect();
    assert_eq!(
        codes,
        vec![UNAPPLIED, UNAPPLIED],
        "each unapplied switch is reported"
    );
    clean(&mut m);
}

/// DEC-2344 item 1: a crashed generation a switch started waits the usual back-off before its
/// restart, as a wanted one does.
#[test]
fn a_crashed_kill_switch_generation_waits_its_back_off_before_one_restart() {
    let journal = Journal::new();
    let mut m = retired(&journal);
    journal.services(&kill("connection", &connection_of(ACCOUNT), 1).1, at(10));
    tick(&mut m, 10);
    assert_eq!(starts(&mut m, &ex(ACCOUNT)), vec![1, 2]);
    m.orchestrator().fake.crash(&ex(ACCOUNT));
    let wait = i64::try_from(backoff(0).as_secs()).unwrap();
    for s in 20..20 + wait {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2],
        "no restart before its back-off, counted from the tick that saw the crash"
    );
    for s in 20 + wait..20 + wait + 3 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2, 3],
        "exactly one restart once it passes"
    );
    clean(&mut m);
}

/// DEC-2344 item 3 and rule 13: an agent deployed and retired while a kill switch's generation
/// runs does not take the hold away, so the account's last retirement does not stop that
/// generation; it ends by its own exit and is not restarted.
#[test]
fn a_deploy_and_retire_during_a_kill_switch_run_does_not_stop_it() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", ACCOUNT),
            ("agent_c", ACCOUNT),
            ("agent_b", OTHER),
        ]),
    );
    journal.services(&journal.deployed("agent_a"), at(1));
    tick(&mut m, 1);
    journal.services(&stopped("agent_a", ACCOUNT), at(2));
    tick(&mut m, 2);
    tick(&mut m, 3);
    journal.services(&kill("connection", &connection_of(ACCOUNT), 1).1, at(10));
    tick(&mut m, 10);
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2],
        "the switch's one start"
    );
    journal.services(&journal.deployed("agent_c"), at(11));
    tick(&mut m, 11);
    journal.services(&stopped("agent_c", ACCOUNT), at(12));
    for s in 12..40 {
        tick(&mut m, s);
    }
    assert_eq!(
        stops(&mut m, &ex(ACCOUNT)),
        vec![1],
        "the switch's generation is not stopped by the retirement"
    );
    m.orchestrator().fake.succeed(&ex(ACCOUNT));
    for s in 40..80 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &ex(ACCOUNT)),
        vec![1, 2],
        "its own end is not restarted"
    );
    clean(&mut m);
}
