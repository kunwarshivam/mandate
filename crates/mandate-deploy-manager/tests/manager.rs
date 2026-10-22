//! E21-33 (DEC-1170; infrastructure design §3.4, §3.6): the deployment manager over the control
//! stream in a [`mandate_journal::MemoryJournal`], with the journal's own draft checks, and the
//! fake orchestrator. Every start and stop is checked, as it happens, against the records already
//! committed, and against the slot's other live instances.

mod common;

use common::{
    Captured, Checked, Journal, Manager, accounts, at, block_on, connection_of, deployed,
    deployed_v1, established, identity, kill, manager, owner_command, redeployed, restart, revoked,
    stopped,
};
use mandate_canon::Value;
use mandate_deploy_manager::fake::Call;
use mandate_deploy_manager::orchestrator::{CRASH_LOOP, DEPLOYMENT_UNCONNECTED, DEPLOYMENT_UNREAD};
use mandate_deploy_manager::{
    Cause, ControlJournal, Deployer, Halt, Identity, Record, Slot, backoff,
};
use mandate_journal::AppendOutcome;

fn rt(agent: &str) -> Slot {
    Slot::runtime(agent)
}

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

fn stops(m: &mut Manager) -> Vec<(Slot, u64)> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::Stop {
                slot, generation, ..
            } => Some((slot.clone(), *generation)),
            _ => None,
        })
        .collect()
}

fn causes(journal: &Journal) -> Vec<String> {
    journal
        .records()
        .iter()
        .filter(|(t, _)| t == "ProcessStartIssued")
        .map(|(_, p)| p.get("cause").and_then(Value::as_str).unwrap().to_owned())
        .collect()
}

fn clean(m: &mut Manager) {
    let violations = &m.orchestrator().violations;
    assert!(violations.is_empty(), "{violations:?}");
}

/// Deploys `agent` on `account` and brings both processes up.
fn deploy(journal: &Journal, m: &mut Manager, agent: &str, secs: i64) {
    journal.services(&journal.deployed(agent), at(secs));
    tick(m, secs);
}

#[test]
fn a_deployed_agent_gets_its_runtime_and_executor_each_journaled_first() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    assert_eq!(starts(&mut m, &rt("agent_a")), vec![1]);
    assert_eq!(starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")), vec![1]);
    assert_eq!(causes(&journal), vec!["deployed", "deployed"]);
    tick(&mut m, 2);
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1],
        "a running process is left alone"
    );
    clean(&mut m);
}

#[test]
fn nothing_starts_for_a_deployment_that_names_no_established_connection() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    tick(&mut m, 1);
    journal.services(&deployed_v1("agent_v1"), at(2));
    journal.services(
        &deployed("agent_elsewhere", "01J8Z2ACCT00000000000000Z9"),
        at(2),
    );
    tick(&mut m, 2);
    tick(&mut m, 3);
    assert!(
        m.orchestrator().fake.calls().is_empty(),
        "a version-1 deployment, or one on a connection the stream never established, starts nothing"
    );
    assert!(journal.records().is_empty());
    assert_eq!(
        m.alerts().0,
        vec![DEPLOYMENT_UNREAD, DEPLOYMENT_UNCONNECTED],
        "the version-1 deployment and the one rule 201 refuses are each reported once"
    );
}

#[test]
fn a_revocation_stops_nothing_but_a_deployment_after_it_starts_nothing() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", "01J8Z2ACCT00000000000000A1"),
            ("agent_b", "01J8Z2ACCT00000000000000A1"),
        ]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    journal.services(&revoked("conn_01J8Z2ACCT00000000000000A1"), at(2));
    journal.services(&journal.deployed("agent_b"), at(2));
    for s in 2..5 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1],
        "agent_a keeps running"
    );
    assert!(stops(&mut m).is_empty(), "a revocation stops nothing");
    assert!(
        starts(&mut m, &rt("agent_b")).is_empty(),
        "rule 201 refuses agent_b"
    );
    assert_eq!(
        m.alerts().0,
        vec![DEPLOYMENT_UNCONNECTED],
        "agent_b's refused deployment is reported once, agent_a's is not"
    );
    clean(&mut m);
}

/// DEC-2038: a version-2 record rule 201 refuses, of an agent already running or already stopped,
/// is no deployment the manager failed to run, so nothing reports it.
#[test]
fn a_refused_record_of_an_agent_running_or_stopped_is_not_reported() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", "01J8Z2ACCT00000000000000A1"),
            ("agent_b", "01J8Z2ACCT00000000000000A1"),
        ]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    deploy(&journal, &mut m, "agent_b", 2);
    journal.services(&stopped("agent_b", "01J8Z2ACCT00000000000000A1"), at(3));
    tick(&mut m, 3);
    journal.services(&revoked("conn_01J8Z2ACCT00000000000000A1"), at(4));
    journal.services(
        &redeployed("agent_a", "01J8Z2ACCT00000000000000A1", 2),
        at(4),
    );
    journal.services(
        &redeployed("agent_b", "01J8Z2ACCT00000000000000A1", 2),
        at(4),
    );
    for s in 4..7 {
        tick(&mut m, s);
    }
    assert!(m.alerts().0.is_empty(), "{:?}", m.alerts().0);
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1],
        "agent_a keeps running"
    );
    assert!(
        starts(&mut m, &rt("agent_b")).len() == 1,
        "agent_b is not started again"
    );
}

/// DEC-2038: a deployment rule 201 refuses is reported once per manager process, a later
/// establishment revives it no more than it starts it, and a stop leaves nothing to report.
#[test]
fn a_refused_deployment_is_reported_once_until_its_agent_is_stopped() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    tick(&mut m, 1);
    journal.services(
        &deployed("agent_elsewhere", "01J8Z2ACCT00000000000000Z9"),
        at(2),
    );
    for s in 2..5 {
        tick(&mut m, s);
    }
    journal.services(
        &established(
            "conn_01J8Z2ACCT00000000000000Z9",
            "01J8Z2ACCT00000000000000Z9",
        ),
        at(5),
    );
    tick(&mut m, 5);
    assert_eq!(m.alerts().0, vec![DEPLOYMENT_UNCONNECTED], "raised once");
    assert!(
        m.orchestrator().fake.calls().is_empty(),
        "a later establishment starts nothing"
    );
    let mut m = restart(m);
    tick(&mut m, 6);
    assert_eq!(
        m.alerts().0,
        vec![DEPLOYMENT_UNCONNECTED, DEPLOYMENT_UNCONNECTED],
        "a new manager process reports it again"
    );
    journal.services(
        &stopped("agent_elsewhere", "01J8Z2ACCT00000000000000Z9"),
        at(7),
    );
    tick(&mut m, 7);
    let mut m = restart(m);
    tick(&mut m, 8);
    assert_eq!(m.alerts().0.len(), 2, "once stopped, no process reports it");
    assert_eq!(
        (DEPLOYMENT_UNCONNECTED.code, DEPLOYMENT_UNCONNECTED.runbook),
        ("deployment_unconnected", "RB-14"),
        "a reason code and a runbook, nothing else"
    );
}

#[test]
fn an_unread_deployment_is_still_reported_when_a_journal_append_fails_in_its_pass() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    tick(&mut m, 0);
    journal.services(&deployed_v1("agent_b"), at(1));
    journal.services(&journal.deployed("agent_a"), at(1));
    *journal.failing_appends.borrow_mut() = 1;
    assert_eq!(block_on(m.tick(at(1))), Err(Halt::Journal));
    for s in 2..6 {
        tick(&mut m, s);
    }
    assert_eq!(
        m.alerts().0,
        vec![DEPLOYMENT_UNREAD],
        "raised once, though the pass that first saw it halted on an append"
    );
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1],
        "the start is retried, as the same generation"
    );
    clean(&mut m);
}

#[test]
fn a_version_1_record_of_an_agent_already_running_is_not_reported() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    journal.services(&deployed_v1("agent_a"), at(2));
    tick(&mut m, 2);
    assert!(
        m.alerts().0.is_empty(),
        "the agent runs from its version-2 deployment"
    );
    assert_eq!(starts(&mut m, &rt("agent_a")), vec![1]);
}

#[test]
fn a_stop_of_a_deployment_the_manager_never_read_is_final() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    journal.services(&deployed_v1("agent_a"), at(1));
    journal.services(&stopped("agent_a", "01J8Z2ACCT00000000000000A1"), at(1));
    journal.services(&journal.deployed("agent_a"), at(2));
    for s in 1..5 {
        tick(&mut m, s);
    }
    assert!(m.orchestrator().fake.calls().is_empty());
    assert!(
        m.alerts().0.is_empty(),
        "a stopped deployment is not reported"
    );
}

#[test]
fn every_record_the_manager_appends_is_one_of_its_three_and_none_carries_text() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator().fake.crash(&rt("agent_a"));
    tick(&mut m, 2);
    tick(&mut m, 4);
    journal.services(&stopped("agent_a", "01J8Z2ACCT00000000000000A1"), at(5));
    tick(&mut m, 5);
    let rows = journal.rows();
    let written: Vec<&str> = rows
        .iter()
        .filter(|r| r.body.windows(14).any(|w| w == b"deploy_manager"))
        .map(|r| r.event_type.as_str())
        .collect();
    assert!(!written.is_empty());
    assert!(
        written.iter().all(|t| matches!(
            *t,
            "ProcessStartIssued" | "ProcessStopIssued" | "ProcessCrashLooped"
        )),
        "{written:?}"
    );
}

#[test]
fn readiness_flaps_never_stop_or_restart_a_process() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    for second in 2..40 {
        m.orchestrator().fake.ready(&rt("agent_a"), second % 2 == 0);
        m.orchestrator()
            .fake
            .ready(&ex("01J8Z2ACCT00000000000000A1"), second % 3 == 0);
        tick(&mut m, second);
    }
    assert_eq!(
        m.orchestrator().fake.calls().len(),
        2,
        "only the two first starts"
    );
}

#[test]
fn a_crashed_process_restarts_after_its_backoff_as_the_next_generation() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator().fake.crash(&rt("agent_a"));
    tick(&mut m, 10);
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1],
        "not before the back-off"
    );
    tick(&mut m, 11);
    assert_eq!(starts(&mut m, &rt("agent_a")), vec![1, 2]);
    assert_eq!(causes(&journal).last().map(String::as_str), Some("restart"));
    m.orchestrator().fake.crash(&rt("agent_a"));
    tick(&mut m, 20);
    tick(&mut m, 21);
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1, 2],
        "the second restart waits two seconds"
    );
    tick(&mut m, 22);
    assert_eq!(starts(&mut m, &rt("agent_a")), vec![1, 2, 3]);
    clean(&mut m);
}

#[test]
fn the_backoff_doubles_and_is_capped() {
    assert_eq!(backoff(0).as_secs(), 1);
    assert_eq!(backoff(1).as_secs(), 2);
    assert_eq!(backoff(4).as_secs(), 16);
    assert_eq!(backoff(8).as_secs(), 256);
    assert_eq!(backoff(9).as_secs(), 300);
    assert_eq!(backoff(64).as_secs(), 300);
    assert_eq!(backoff(usize::MAX).as_secs(), 300);
}

/// Crashes `slot` and ticks until it is restarted or `limit` seconds pass; returns the second.
fn crash_and_wait(m: &mut Manager, slot: &Slot, from: i64, limit: i64) -> i64 {
    m.orchestrator().fake.crash(slot);
    let before = starts(m, slot).len();
    for second in from..from + limit {
        tick(m, second);
        if starts(m, slot).len() > before {
            return second;
        }
    }
    from + limit
}

#[test]
fn after_the_crash_loop_bound_the_process_is_left_down_with_one_opaque_alert() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let mut second = 2;
    for _ in 0..5 {
        second = crash_and_wait(&mut m, &rt("agent_a"), second, 60) + 1;
    }
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1, 2, 3, 4, 5, 6],
        "five restarts"
    );
    m.orchestrator().fake.crash(&rt("agent_a"));
    for s in second..second + 900 {
        tick(&mut m, s);
    }
    assert_eq!(starts(&mut m, &rt("agent_a")).len(), 6, "no sixth restart");
    let looped: Vec<_> = journal
        .records()
        .into_iter()
        .filter(|(t, _)| t == "ProcessCrashLooped")
        .collect();
    assert_eq!(looped.len(), 1, "the pause is journaled once");
    let payload = &looped[0].1;
    assert_eq!(payload.get("restarts").and_then(Value::as_int), Some(5));
    assert_eq!(
        payload.get("window_seconds").and_then(Value::as_int),
        Some(600)
    );
    assert_eq!(payload.get("generation").and_then(Value::as_int), Some(6));
    assert_eq!(m.alerts().0, vec![CRASH_LOOP]);
    assert_eq!(
        (CRASH_LOOP.code, CRASH_LOOP.runbook),
        ("crash_loop", "RB-14")
    );
    assert!(m.manager.paused(&rt("agent_a")));
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")),
        vec![1],
        "the executor is not touched"
    );
    clean(&mut m);
}

#[test]
fn restarts_spread_beyond_the_window_never_reach_the_bound() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let mut second = 2;
    for _ in 0..8 {
        second = crash_and_wait(&mut m, &rt("agent_a"), second, 60) + 200;
    }
    assert_eq!(starts(&mut m, &rt("agent_a")).len(), 9);
    assert!(m.alerts().0.is_empty());
}

#[test]
fn a_kill_switch_starts_a_process_that_is_down_at_once_without_backoff() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator()
        .fake
        .crash(&ex("01J8Z2ACCT00000000000000A1"));
    tick(&mut m, 2);
    let (id, draft) = kill("agent", "agent_a", 1);
    journal.services(&draft, at(2));
    tick(&mut m, 2);
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")),
        vec![1, 2],
        "the same second, no back-off"
    );
    let records = journal.records();
    let (_, start) = records.last().unwrap();
    assert_eq!(
        start.get("cause").and_then(Value::as_str),
        Some("kill_switch")
    );
    let row = journal.rows().into_iter().last().unwrap();
    let body = mandate_canon::parse(&row.body).unwrap();
    assert_eq!(
        body.get("causation_id").and_then(Value::as_str),
        Some(id.as_str())
    );
    clean(&mut m);
}

#[test]
fn a_kill_switch_never_stops_or_replaces_a_running_process() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", "01J8Z2ACCT00000000000000A1"),
            ("agent_b", "01J8Z2ACCT00000000000000A1"),
        ]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    deploy(&journal, &mut m, "agent_b", 2);
    for (n, (scope, subject)) in [
        ("agent", "agent_a"),
        ("connection", "conn_01J8Z2ACCT00000000000000A1"),
        ("workspace", "ws_01J8Z2"),
    ]
    .into_iter()
    .enumerate()
    {
        let (_, draft) = kill(scope, subject, u32::try_from(n).unwrap());
        journal.services(&draft, at(3));
        tick(&mut m, 3);
    }
    assert_eq!(
        m.orchestrator().fake.calls().len(),
        3,
        "only the three first starts"
    );
    assert!(stops(&mut m).is_empty());
}

#[test]
fn a_kill_switch_gives_a_crash_looped_process_one_start_and_no_more() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let mut second = 2;
    for _ in 0..5 {
        second = crash_and_wait(&mut m, &rt("agent_a"), second, 60) + 1;
    }
    m.orchestrator().fake.crash(&rt("agent_a"));
    tick(&mut m, second);
    assert!(m.manager.paused(&rt("agent_a")));
    let (_, draft) = kill("workspace", "ws_01J8Z2", 1);
    journal.services(&draft, at(second + 1));
    tick(&mut m, second + 1);
    assert_eq!(
        starts(&mut m, &rt("agent_a")).len(),
        7,
        "one start for the switch"
    );
    m.orchestrator().fake.crash(&rt("agent_a"));
    for s in second + 2..second + 400 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &rt("agent_a")).len(),
        7,
        "and no restart after it"
    );
    assert_eq!(m.alerts().0.len(), 1);
    clean(&mut m);
}

#[test]
fn a_kill_switch_that_arrived_while_a_process_ran_restarts_it_at_once_when_it_crashes() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let (_, draft) = kill("connection", "conn_01J8Z2ACCT00000000000000A1", 1);
    journal.services(&draft, at(2));
    tick(&mut m, 2);
    m.orchestrator()
        .fake
        .crash(&ex("01J8Z2ACCT00000000000000A1"));
    tick(&mut m, 3);
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")),
        vec![1, 2]
    );
    assert_eq!(
        causes(&journal).last().map(String::as_str),
        Some("kill_switch")
    );
}

#[test]
fn a_kill_switch_out_of_scope_or_unresolved_starts_nothing() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", "01J8Z2ACCT00000000000000A1"),
            ("agent_b", "01J8Z2ACCT00000000000000A2"),
        ]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    deploy(&journal, &mut m, "agent_b", 1);
    m.orchestrator().fake.crash(&rt("agent_b"));
    m.orchestrator()
        .fake
        .crash(&ex("01J8Z2ACCT00000000000000A2"));
    tick(&mut m, 2);
    let (_, by_agent) = kill("agent", "agent_a", 1);
    journal.services(&by_agent, at(2));
    let (_, unknown) = kill("connection", "conn_unknown", 2);
    journal.services(&unknown, at(2));
    tick(&mut m, 2);
    assert_eq!(starts(&mut m, &rt("agent_b")), vec![1]);
    assert_eq!(starts(&mut m, &ex("01J8Z2ACCT00000000000000A2")), vec![1]);
}

#[test]
fn a_stopped_agent_is_stopped_once_journaled_first_and_never_started_again() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    journal.services(&stopped("agent_a", "01J8Z2ACCT00000000000000A1"), at(2));
    tick(&mut m, 2);
    assert_eq!(
        stops(&mut m),
        vec![(rt("agent_a"), 1), (ex("01J8Z2ACCT00000000000000A1"), 1)]
    );
    let (_, draft) = kill("agent", "agent_a", 1);
    journal.services(&draft, at(3));
    journal.services(
        &redeployed("agent_a", "01J8Z2ACCT00000000000000A1", 1),
        at(3),
    );
    for s in 3..30 {
        tick(&mut m, s);
    }
    assert_eq!(starts(&mut m, &rt("agent_a")), vec![1], "stopped is final");
    assert_eq!(starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")), vec![1]);
    assert_eq!(stops(&mut m).len(), 2, "each stop once");
    clean(&mut m);
}

#[test]
fn an_executor_stops_only_with_the_last_agent_of_its_account() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", "01J8Z2ACCT00000000000000A1"),
            ("agent_b", "01J8Z2ACCT00000000000000A1"),
        ]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    deploy(&journal, &mut m, "agent_b", 1);
    journal.services(&stopped("agent_a", "01J8Z2ACCT00000000000000A1"), at(2));
    tick(&mut m, 2);
    assert_eq!(stops(&mut m), vec![(rt("agent_a"), 1)]);
    journal.services(&stopped("agent_b", "01J8Z2ACCT00000000000000A1"), at(3));
    tick(&mut m, 3);
    assert_eq!(
        stops(&mut m),
        vec![
            (rt("agent_a"), 1),
            (rt("agent_b"), 1),
            (ex("01J8Z2ACCT00000000000000A1"), 1)
        ]
    );
}

#[test]
fn a_lost_node_holds_the_restart_until_the_instance_is_gone() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator()
        .fake
        .lose_node(&ex("01J8Z2ACCT00000000000000A1"));
    for s in 2..100 {
        tick(&mut m, s);
    }
    let (_, draft) = kill("agent", "agent_a", 1);
    journal.services(&draft, at(100));
    tick(&mut m, 100);
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")),
        vec![1],
        "never beside an instance that may run"
    );
    m.orchestrator()
        .fake
        .vanish(&ex("01J8Z2ACCT00000000000000A1"));
    tick(&mut m, 101);
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")),
        vec![1, 2],
        "the switch's start, once it is gone"
    );
    clean(&mut m);
}

#[test]
fn a_stop_on_a_lost_node_is_journaled_once_and_repeated_until_the_instance_is_gone() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator().fake.lose_node(&rt("agent_a"));
    journal.services(&stopped("agent_a", "01J8Z2ACCT00000000000000A1"), at(2));
    tick(&mut m, 2);
    tick(&mut m, 3);
    let runtime_stops = stops(&mut m)
        .iter()
        .filter(|(s, _)| *s == rt("agent_a"))
        .count();
    assert_eq!(runtime_stops, 2);
    let records = journal.records();
    assert_eq!(
        records
            .iter()
            .filter(|(t, _)| t == "ProcessStopIssued")
            .count(),
        2,
        "one per process"
    );
}

#[test]
fn nothing_is_done_while_the_journal_is_unavailable() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    journal.services(&journal.deployed("agent_a"), at(1));
    *journal.unavailable.borrow_mut() = true;
    assert_eq!(block_on(m.tick(at(1))), Err(Halt::Journal));
    assert!(m.orchestrator().fake.calls().is_empty());
    *journal.unavailable.borrow_mut() = false;
    tick(&mut m, 2);
    assert_eq!(m.orchestrator().fake.calls().len(), 2);
}

#[test]
fn a_second_managers_draft_of_the_same_step_is_the_same_event_and_never_commits_twice() {
    let journal = Journal::new();
    let record = Record::Start {
        slot: rt("agent_a"),
        generation: 1,
        cause: Cause::Deployed,
        kill_switch: None,
    };
    let id = identity();
    assert_eq!(record.event_id(&id), record.event_id(&id));
    let other = Record::Start {
        slot: rt("agent_a"),
        generation: 2,
        cause: Cause::Restart,
        kill_switch: None,
    };
    assert_ne!(
        record.event_id(&id),
        other.event_id(&id),
        "one event per generation"
    );
    let mut port = journal.clone();
    let first = block_on(ControlJournal::append(
        &mut port,
        &record.draft(&id, "paper", at(1)),
        at(1),
    ))
    .unwrap();
    assert!(matches!(first, AppendOutcome::Committed(_)), "{first:?}");
    let again = block_on(ControlJournal::append(
        &mut port,
        &record.draft(&id, "paper", at(1)),
        at(1),
    ))
    .unwrap();
    assert!(
        matches!(again, AppendOutcome::AlreadyCommitted(_)),
        "{again:?}"
    );
    let later = block_on(ControlJournal::append(
        &mut port,
        &record.draft(&id, "paper", at(2)),
        at(2),
    ))
    .unwrap();
    assert!(
        matches!(later, AppendOutcome::IdempotencyConflict { .. }),
        "{later:?}"
    );
    assert_eq!(journal.records().len(), 1);
}

#[test]
fn a_replay_that_cannot_see_a_generation_counts_it_as_exited_and_journals_the_next() {
    let journal = Journal::new();
    let mut first = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut first, "agent_a", 1);
    let mut second = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    tick(&mut second, 2);
    assert_eq!(
        journal.records().len(),
        2,
        "the replay adds no record at once"
    );
    assert!(
        second.orchestrator().fake.calls().is_empty(),
        "it never re-creates a generation without a record"
    );
    tick(&mut second, 3);
    assert_eq!(starts(&mut second, &rt("agent_a")), vec![2]);
    assert_eq!(
        causes(&journal),
        vec!["deployed", "deployed", "restart", "restart"]
    );
    clean(&mut second);
}

#[test]
fn a_start_the_orchestrator_refused_is_retried_without_a_second_record() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    journal.services(&journal.deployed("agent_a"), at(1));
    m.orchestrator().fake.refuse_effects = true;
    tick(&mut m, 1);
    assert_eq!(journal.records().len(), 2);
    assert!(m.orchestrator().fake.instances().is_empty());
    m.orchestrator().fake.refuse_effects = false;
    tick(&mut m, 2);
    assert_eq!(journal.records().len(), 2, "no second record");
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1],
        "the same generation, asked again"
    );
    assert_eq!(m.orchestrator().fake.live(&rt("agent_a")), 1);
    tick(&mut m, 3);
    assert_eq!(starts(&mut m, &rt("agent_a")), vec![1]);
    clean(&mut m);
}

#[test]
fn a_journal_refusal_halts_the_pass_before_any_effect() {
    let journal = Journal::new();
    let mut m = Deployer::new(
        Identity {
            workspace: "ws_elsewhere".to_owned(),
            ..identity()
        },
        journal.clone(),
        Checked::new(journal.clone()),
        Captured::default(),
    );
    let account = "01J8Z2ACCT00000000000000A1";
    journal.services(&established(&connection_of(account), account), at(1));
    journal.services(&deployed("agent_a", account), at(1));
    let halted = block_on(m.tick(at(1)));
    assert!(matches!(halted, Err(Halt::Refused { .. })), "{halted:?}");
    assert!(m.orchestrator().fake.calls().is_empty());
}

#[test]
fn an_orchestrator_that_cannot_list_decides_nothing() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    journal.services(&journal.deployed("agent_a"), at(1));
    m.orchestrator().fake.unavailable = true;
    assert_eq!(block_on(m.tick(at(1))), Err(Halt::Orchestrator));
    assert!(journal.records().is_empty());
}

/// Crash-loops `slot` from `second`, returning the next second.
fn crash_loop_it(m: &mut Manager, slot: &Slot, mut second: i64) -> i64 {
    for _ in 0..5 {
        second = crash_and_wait(m, slot, second, 60) + 1;
    }
    m.orchestrator().fake.crash(slot);
    tick(m, second);
    assert!(m.manager.paused(slot));
    second + 1
}

#[test]
fn a_manager_restart_never_brings_back_a_crash_looped_process_and_raises_its_alert_again() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let second = crash_loop_it(&mut m, &rt("agent_a"), 2);
    assert!(
        !m.manager.paused(&ex("01J8Z2ACCT00000000000000A1")),
        "only the looping slot is paused"
    );
    m.orchestrator().fake.collect();
    let mut m = restart(m);
    for s in second..second + 900 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &rt("agent_a")).len(),
        6,
        "neither re-created nor restarted"
    );
    assert_eq!(
        m.alerts().0,
        vec![CRASH_LOOP, CRASH_LOOP],
        "raised again once after the restart"
    );
    assert_eq!(
        journal
            .records()
            .iter()
            .filter(|(t, _)| t == "ProcessCrashLooped")
            .count(),
        1
    );
    clean(&mut m);
}

#[test]
fn a_manager_restart_keeps_the_restarts_it_counted() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let mut second = 2;
    for _ in 0..4 {
        second = crash_and_wait(&mut m, &rt("agent_a"), second, 60) + 1;
    }
    let mut m = restart(m);
    second = crash_and_wait(&mut m, &rt("agent_a"), second, 60) + 1;
    assert_eq!(starts(&mut m, &rt("agent_a")).len(), 6, "the fifth restart");
    m.orchestrator().fake.crash(&rt("agent_a"));
    for s in second..second + 100 {
        tick(&mut m, s);
    }
    assert_eq!(
        starts(&mut m, &rt("agent_a")).len(),
        6,
        "the replay counted the first four"
    );
    assert!(m.manager.paused(&rt("agent_a")));
}

#[test]
fn a_manager_restart_after_a_kill_switch_start_continues_from_its_generation() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator()
        .fake
        .crash(&ex("01J8Z2ACCT00000000000000A1"));
    journal.services(&kill("agent", "agent_a", 1).1, at(2));
    tick(&mut m, 2);
    let mut m = restart(m);
    m.orchestrator()
        .fake
        .crash(&ex("01J8Z2ACCT00000000000000A1"));
    tick(&mut m, 3);
    tick(&mut m, 4);
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")),
        vec![1, 2, 3]
    );
    assert_eq!(
        causes(&journal),
        vec!["deployed", "deployed", "kill_switch", "restart"]
    );
}

#[test]
fn a_manager_restart_repeats_a_pending_stop_without_a_second_record() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator().fake.lose_node(&rt("agent_a"));
    journal.services(&stopped("agent_a", "01J8Z2ACCT00000000000000A1"), at(2));
    tick(&mut m, 2);
    let mut m = restart(m);
    tick(&mut m, 3);
    let records = journal.records();
    assert_eq!(
        records
            .iter()
            .filter(|(t, _)| t == "ProcessStopIssued")
            .count(),
        2
    );
    assert_eq!(
        stops(&mut m)
            .iter()
            .filter(|(s, _)| *s == rt("agent_a"))
            .count(),
        2
    );
}

#[test]
fn an_owner_command_other_than_the_kill_switch_starts_nothing_early() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator()
        .fake
        .crash(&ex("01J8Z2ACCT00000000000000A1"));
    tick(&mut m, 2);
    journal.services(&owner_command("pause", "agent", "agent_a", 1).1, at(2));
    tick(&mut m, 2);
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")),
        vec![1],
        "a pause waits out the back-off"
    );
}

#[test]
fn a_kill_switch_on_a_connection_the_stream_never_established_covers_nothing() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator()
        .fake
        .crash(&ex("01J8Z2ACCT00000000000000A1"));
    m.orchestrator().fake.crash(&rt("agent_a"));
    tick(&mut m, 2);
    journal.services(&kill("connection", "conn_elsewhere", 1).1, at(2));
    tick(&mut m, 2);
    assert_eq!(m.orchestrator().fake.calls().len(), 2);
}

#[test]
fn a_start_refused_after_a_crash_is_retried_as_the_same_generation() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    m.orchestrator().fake.crash(&rt("agent_a"));
    tick(&mut m, 2);
    m.orchestrator().fake.refuse_effects = true;
    tick(&mut m, 3);
    m.orchestrator().fake.refuse_effects = false;
    tick(&mut m, 4);
    assert_eq!(
        starts(&mut m, &rt("agent_a")),
        vec![1, 2],
        "generation 2 once, after the refusal"
    );
    assert_eq!(
        causes(&journal),
        vec!["deployed", "deployed", "restart"],
        "one record for it"
    );
    clean(&mut m);
}

#[test]
fn an_executor_pause_ends_when_every_agent_of_its_account_has_stopped() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", "01J8Z2ACCT00000000000000A1"),
            ("agent_b", "01J8Z2ACCT00000000000000A1"),
        ]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let second = crash_loop_it(&mut m, &ex("01J8Z2ACCT00000000000000A1"), 2);
    journal.services(
        &stopped("agent_a", "01J8Z2ACCT00000000000000A1"),
        at(second),
    );
    tick(&mut m, second);
    assert!(!m.manager.paused(&ex("01J8Z2ACCT00000000000000A1")));
    let mut m = restart(m);
    journal.services(&journal.deployed("agent_b"), at(second + 1));
    tick(&mut m, second + 1);
    tick(&mut m, second + 3);
    assert_eq!(
        starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")).len(),
        7,
        "the executor runs again for agent_b"
    );
    assert!(
        !m.manager.paused(&ex("01J8Z2ACCT00000000000000A1")),
        "and the replay agrees"
    );
}

#[test]
fn a_start_whose_agent_is_stopped_while_it_is_journaled_is_not_carried_out() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", "01J8Z2ACCT00000000000000A1")]),
    );
    tick(&mut m, 0);
    journal.services(&journal.deployed("agent_a"), at(1));
    journal
        .after_append
        .borrow_mut()
        .push(stopped("agent_a", "01J8Z2ACCT00000000000000A1"));
    tick(&mut m, 1);
    assert!(
        m.orchestrator().fake.calls().is_empty(),
        "{:?}",
        m.orchestrator().fake.calls()
    );
    tick(&mut m, 2);
    assert!(m.orchestrator().fake.calls().is_empty());
}

#[test]
fn a_second_agent_on_a_crash_looped_executors_account_leaves_it_paused() {
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[
            ("agent_a", "01J8Z2ACCT00000000000000A1"),
            ("agent_b", "01J8Z2ACCT00000000000000A1"),
        ]),
    );
    deploy(&journal, &mut m, "agent_a", 1);
    let second = crash_loop_it(&mut m, &ex("01J8Z2ACCT00000000000000A1"), 2);
    deploy(&journal, &mut m, "agent_b", second);
    for s in second + 1..second + 400 {
        tick(&mut m, s);
    }
    assert!(
        m.manager.paused(&ex("01J8Z2ACCT00000000000000A1")),
        "only every agent stopped ends it"
    );
    assert_eq!(starts(&mut m, &ex("01J8Z2ACCT00000000000000A1")).len(), 6);
}
