//! E21-33's property (DEC-1170, DEC-1171): across random sequences of deploys, stops (of read and
//! never-read deployments), connection revocations and re-establishments, kill switches, crashes, readiness flaps, lost nodes and clock jumps, through the
//! control stream in a
//! [`mandate_journal::MemoryJournal`] and the fake orchestrator:
//!
//! - no slot ever has two instances that may be running, and no start or stop comes before its
//!   record is committed (the checking orchestrator of the harness);
//! - nothing starts for a process the control stream does not want, and a stopped agent's runtime
//!   is never started again; a deployment on a connection revoked before it is not wanted, and a
//!   revocation after a deployment stops nothing (journal spec §9.24 rule 201);
//! - nothing stops a process the control stream still wants, whatever its readiness or a kill
//!   switch;
//! - in the pass after a kill switch, every process it covers that the control stream wants is up:
//!   the manager never delays it;
//! - once its crash loop is journaled, a process is started again only to answer a kill switch
//!   that covered it since its last start, until the control stream stops wanting it;
//! - within any crash-loop window a process is started at most seven times (a first start, five
//!   restarts, and one start answering kill switches from before the window), plus once per kill
//!   switch in the window that covers it;
//! - each deployment rule 201 refuses, of an agent not stopped, raises one opaque
//!   `deployment_unconnected` alert in each manager process that reads it, and nothing else raises
//!   one (DEC-2038);
//! - all of it across the manager's own restarts and the orchestrator's garbage collection.
//!
//! The oracle computes what is wanted, what a switch covers, and the counts from its own record of
//! the operations it generated, never from the manager's fold.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
    Journal, Manager, Seed, accounts, at, block_on, connection_of, deployed, established_again,
    kill, manager, restart, revoked_again, stopped,
};
use mandate_deploy_manager::Slot;
use mandate_deploy_manager::fake::Call;
use mandate_deploy_manager::orchestrator::DEPLOYMENT_UNCONNECTED;
use proptest::prelude::*;

const AGENTS: [(&str, &str); 3] = [
    ("agent_0", "01J8Z2ACCT00000000000000A1"),
    ("agent_1", "01J8Z2ACCT00000000000000A1"),
    ("agent_2", "01J8Z2ACCT00000000000000A2"),
];
const ACCOUNTS: [&str; 2] = ["01J8Z2ACCT00000000000000A1", "01J8Z2ACCT00000000000000A2"];
const WINDOW: i64 = 600;
/// The most starts of one process within a crash-loop window, before kill switches in the window:
/// the start before the window's first restart (a first start, a restart counted earlier, or a
/// generation a replay or the garbage collection made unlistable, which counts as exited), five
/// restarts, and one start answering kill switches that arrived before the window.
const WINDOW_STARTS: usize = 7;

#[derive(Debug, Clone)]
enum Op {
    Deploy(usize),
    Stop(usize),
    /// The connection of one account is revoked.
    Revoke(usize),
    /// A revoked connection is established again: deployments after it run, ones before it do not.
    Reestablish(usize),
    KillAgent(usize),
    KillConnection(usize),
    KillWorkspace,
    Crash(usize),
    Ready(usize, bool),
    LoseNode(usize),
    Vanish(usize),
    Advance(i64),
    /// The manager itself restarts, keeping nothing in memory.
    Restart,
    /// The orchestrator's garbage collection removes every exited instance.
    Collect,
}

fn slots() -> Vec<Slot> {
    let mut out: Vec<Slot> = AGENTS.iter().map(|(a, _)| Slot::runtime(a)).collect();
    out.extend(ACCOUNTS.iter().map(|a| Slot::executor(a)));
    out
}

fn op() -> impl Strategy<Value = Op> {
    let slot = 0..slots().len();
    prop_oneof![
        3 => (0..AGENTS.len()).prop_map(Op::Deploy),
        1 => (0..AGENTS.len()).prop_map(Op::Stop),
        1 => (0..ACCOUNTS.len()).prop_map(Op::Revoke),
        1 => (0..ACCOUNTS.len()).prop_map(Op::Reestablish),
        1 => (0..AGENTS.len()).prop_map(Op::KillAgent),
        1 => (0..ACCOUNTS.len()).prop_map(Op::KillConnection),
        1 => Just(Op::KillWorkspace),
        6 => slot.clone().prop_map(Op::Crash),
        3 => (slot.clone(), any::<bool>()).prop_map(|(s, r)| Op::Ready(s, r)),
        1 => slot.clone().prop_map(Op::LoseNode),
        2 => slot.prop_map(Op::Vanish),
        4 => prop_oneof![0_i64..4, 0_i64..40, 0_i64..700].prop_map(Op::Advance),
        1 => Just(Op::Restart),
        1 => Just(Op::Collect),
    ]
}

/// The oracle's own view of the control stream.
#[derive(Default)]
struct Oracle {
    /// The agents whose deployment was journaled, wanted or not.
    journaled: BTreeSet<usize>,
    /// The agents deployed while their connection was established: the deployments rule 201 admits.
    deployed: BTreeSet<usize>,
    /// The accounts whose connection is revoked.
    revoked: BTreeSet<&'static str>,
    /// Per account, how many revocations and establishments the stream holds beyond the first
    /// establishment, so each is its own event.
    revocations: BTreeMap<&'static str, u32>,
    establishments: BTreeMap<&'static str, u32>,
    stopped: BTreeSet<usize>,
    /// Per slot, the second of each kill switch that covered it.
    kills: BTreeMap<Slot, Vec<i64>>,
    /// Per slot, the second of each start call.
    starts: BTreeMap<Slot, Vec<i64>>,
    /// Per slot, the kill switches that covered it since its last start: each start may answer
    /// them all at once.
    unanswered: BTreeMap<Slot, usize>,
    /// The slots whose crash loop is journaled, until the control stream stops wanting them.
    looped: BTreeSet<Slot>,
    /// The refused deployments the current manager process has reported, and how many reports
    /// every process has raised in all.
    reported: BTreeSet<usize>,
    unconnected_alerts: usize,
}

impl Oracle {
    fn live_agents(&self) -> impl Iterator<Item = usize> + '_ {
        self.deployed
            .iter()
            .copied()
            .filter(|i| !self.stopped.contains(i))
    }

    fn wanted(&self, slot: &Slot) -> bool {
        self.live_agents().any(|i| {
            let (agent, account) = AGENTS[i];
            *slot == Slot::runtime(agent) || *slot == Slot::executor(account)
        })
    }

    /// The slots a switch covers: of each deployed agent it names, by itself or by its account,
    /// the runtime and the account's executor.
    fn covered(&self, names: impl Fn(usize) -> bool) -> Vec<Slot> {
        let mut out = BTreeSet::new();
        for i in self.deployed.iter().copied().filter(|i| names(*i)) {
            let (agent, account) = AGENTS[i];
            out.insert(Slot::runtime(agent));
            out.insert(Slot::executor(account));
        }
        out.into_iter().collect()
    }
}

fn run(ops: &[Op]) -> Result<(), TestCaseError> {
    run_seeded(ops, Seed::None)
}

fn run_seeded(ops: &[Op], seed: Seed) -> Result<(), TestCaseError> {
    let journal = Journal::new();
    let mut m: Manager = manager(&journal, accounts(&AGENTS));
    m.orchestrator().seed = seed;
    let mut oracle = Oracle::default();
    let mut now = 1_i64;
    let mut seen_calls = 0;
    let mut seen_records = 0;
    let mut kill_n = 0_u32;
    block_on(m.tick(at(now))).map_err(|h| TestCaseError::fail(format!("{h:?}")))?;
    for op in ops {
        let mut covered: Vec<Slot> = Vec::new();
        let all = slots();
        match op {
            Op::Deploy(i) => {
                let (agent, account) = AGENTS[*i];
                if oracle.journaled.insert(*i) {
                    journal.services(&deployed(agent, account), at(now));
                    if !oracle.revoked.contains(account) {
                        oracle.deployed.insert(*i);
                    }
                }
            }
            Op::Revoke(j) => {
                let account = ACCOUNTS[*j];
                if oracle.revoked.insert(account) {
                    let nth = oracle.revocations.entry(account).or_default();
                    journal.services(&revoked_again(&connection_of(account), *nth), at(now));
                    *nth += 1;
                }
            }
            Op::Reestablish(j) => {
                let account = ACCOUNTS[*j];
                if oracle.revoked.remove(account) {
                    let nth = oracle.establishments.entry(account).or_insert(1);
                    let draft = established_again(&connection_of(account), account, *nth);
                    journal.services(&draft, at(now));
                    *nth += 1;
                }
            }
            Op::Stop(i) => {
                if oracle.journaled.contains(i) && oracle.stopped.insert(*i) {
                    journal.services(&stopped(AGENTS[*i].0, AGENTS[*i].1), at(now));
                }
            }
            Op::KillAgent(i) => {
                kill_n += 1;
                journal.services(&kill("agent", AGENTS[*i].0, kill_n).1, at(now));
                covered = oracle.covered(|n| n == *i);
            }
            Op::KillConnection(j) => {
                kill_n += 1;
                let account = ACCOUNTS[*j];
                journal.services(
                    &kill("connection", &format!("conn_{account}"), kill_n).1,
                    at(now),
                );
                covered = oracle.covered(|n| AGENTS[n].1 == account);
            }
            Op::KillWorkspace => {
                kill_n += 1;
                journal.services(&kill("workspace", "ws_01J8Z2", kill_n).1, at(now));
                covered = oracle.covered(|_| true);
            }
            Op::Crash(k) => m.orchestrator().fake.crash(&all[*k]),
            Op::Ready(k, r) => m.orchestrator().fake.ready(&all[*k], *r),
            Op::LoseNode(k) => m.orchestrator().fake.lose_node(&all[*k]),
            Op::Vanish(k) => m.orchestrator().fake.vanish(&all[*k]),
            Op::Advance(secs) => now += secs,
            Op::Restart => {
                m = restart(m);
                oracle.reported.clear();
            }
            Op::Collect => m.orchestrator().fake.collect(),
        }
        for slot in &covered {
            oracle.kills.entry(slot.clone()).or_default().push(now);
            *oracle.unanswered.entry(slot.clone()).or_default() += 1;
        }
        let wanted_now: Vec<Slot> = all.iter().filter(|s| oracle.wanted(s)).cloned().collect();
        oracle.looped.retain(|s| wanted_now.contains(s));
        block_on(m.tick(at(now))).map_err(|h| TestCaseError::fail(format!("{op:?}: {h:?}")))?;

        let refused: Vec<usize> = oracle
            .journaled
            .iter()
            .copied()
            .filter(|i| !oracle.deployed.contains(i) && !oracle.stopped.contains(i))
            .collect();
        for i in refused {
            if oracle.reported.insert(i) {
                oracle.unconnected_alerts += 1;
            }
        }
        let raised = m
            .alerts()
            .0
            .iter()
            .filter(|alert| **alert == DEPLOYMENT_UNCONNECTED)
            .count();
        prop_assert_eq!(
            raised,
            oracle.unconnected_alerts,
            "{:?}: unconnected-deployment alerts",
            op
        );
        let violations = m.orchestrator().violations.clone();
        prop_assert!(violations.is_empty(), "{op:?}: {violations:?}");
        let calls = m.orchestrator().fake.calls().to_vec();
        for call in &calls[seen_calls..] {
            match call {
                Call::Start { slot, .. } => {
                    prop_assert!(oracle.wanted(slot), "{op:?}: started unwanted {slot:?}");
                    let answered = oracle.unanswered.insert(slot.clone(), 0).unwrap_or(0);
                    prop_assert!(
                        !oracle.looped.contains(slot) || answered > 0,
                        "{op:?}: restarted crash-looped {slot:?} with no kill switch"
                    );
                    oracle.starts.entry(slot.clone()).or_default().push(now);
                }
                Call::Stop { slot, .. } => {
                    prop_assert!(!oracle.wanted(slot), "{op:?}: stopped wanted {slot:?}");
                }
            }
        }
        seen_calls = calls.len();
        let records = journal.records();
        for (event_type, payload) in &records[seen_records..] {
            if event_type == "ProcessCrashLooped" {
                let process = payload
                    .get("process")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let subject = payload
                    .get("subject")
                    .and_then(|v| v.as_str())
                    .unwrap_or_default();
                let slot = all
                    .iter()
                    .find(|s| s.process.code() == process && s.subject == subject);
                if let Some(slot) = slot.filter(|s| oracle.wanted(s)) {
                    oracle.looped.insert(slot.clone());
                }
            }
        }
        seen_records = records.len();
        for slot in &all {
            prop_assert!(
                m.orchestrator().fake.live(slot) <= 1,
                "{op:?}: two live {slot:?}"
            );
        }
        for slot in &covered {
            if oracle.wanted(slot) {
                prop_assert!(
                    m.orchestrator().fake.live(slot) == 1,
                    "{op:?}: kill switch left {slot:?} down"
                );
            }
        }
        for slot in &all {
            let starts = oracle
                .starts
                .get(slot)
                .map_or(0, |s| s.iter().filter(|t| **t > now - WINDOW).count());
            let kills = oracle
                .kills
                .get(slot)
                .map_or(0, |k| k.iter().filter(|t| **t > now - WINDOW).count());
            prop_assert!(
                starts <= WINDOW_STARTS + kills,
                "{op:?}: {slot:?} started {starts} times in a window with {kills} kill switches"
            );
        }
    }
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn the_manager_keeps_one_writer_never_delays_a_kill_switch_and_never_restarts_a_stopped_agent(
        ops in prop::collection::vec(op(), 1..80)
    ) {
        run(&ops)?;
    }
}

#[test]
fn a_long_crash_loop_is_bounded_and_a_kill_switch_still_reaches_it() {
    let mut ops = vec![Op::Deploy(0)];
    for _ in 0..12 {
        ops.push(Op::Crash(0));
        ops.push(Op::Advance(1));
        ops.push(Op::Advance(40));
    }
    ops.push(Op::KillAgent(0));
    ops.push(Op::Crash(0));
    ops.push(Op::Advance(700));
    run(&ops).unwrap();
}

/// A deployment on a revoked connection is reported once per manager process, and a stop before a
/// restart leaves the new process nothing to report.
#[test]
fn a_refused_deployment_is_reported_once_per_process_until_it_is_stopped() {
    run(&[
        Op::Revoke(0),
        Op::Deploy(0),
        Op::Advance(5),
        Op::Restart,
        Op::Advance(5),
        Op::Stop(0),
        Op::Restart,
        Op::Advance(5),
    ])
    .unwrap();
}

/// Each check of the oracle fails on the bug seeded for it, before it is trusted.
#[test]
fn every_check_of_the_oracle_catches_its_seeded_bug() {
    let mut looping = vec![Op::Deploy(0)];
    for _ in 0..3 {
        looping.push(Op::Crash(0));
        looping.push(Op::Advance(5));
    }
    let killed = [Op::Deploy(2), Op::Crash(4), Op::KillWorkspace];
    let mut revived = vec![Op::Deploy(0)];
    for _ in 0..6 {
        revived.push(Op::Crash(0));
        revived.push(Op::Advance(40));
    }
    revived.push(Op::Deploy(2));
    let cases: [(Seed, &[Op], &str); 6] = [
        (Seed::DropStarts, &killed, "kill switch left"),
        (Seed::SecondInstance, &[Op::Deploy(0)], "two live"),
        (Seed::Unwanted, &[Op::Deploy(0)], "started unwanted"),
        (Seed::Repeated, &looping, "times in a window"),
        (Seed::Unjournaled, &[Op::Deploy(0)], "before its record"),
        (Seed::Revive, &revived, "restarted crash-looped"),
    ];
    for (seed, ops, check) in cases {
        let caught = run_seeded(ops, seed).expect_err(&format!("{seed:?} escaped"));
        assert!(caught.to_string().contains(check), "{seed:?}: {caught}");
    }
    run(&killed).unwrap();
    run(&looping).unwrap();
    run(&revived).unwrap();
}
