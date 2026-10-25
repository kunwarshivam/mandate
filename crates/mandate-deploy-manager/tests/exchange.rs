//! DEC-1175 (journal spec §9.21, rule 256): the deployment manager starts an OAuth connect's token
//! exchange once, journaled first with cause `requested`, ended by the request's deadline, and
//! stops it, journaled first with cause `request_closed`, when the request closes. Every start
//! and stop is checked against the records already committed, as the other manager tests are.

mod common;

use std::time::Duration;

use common::{
    Journal, Manager, accounts, at, block_on, established, kill, manager, refused, requested,
    restart, revoked, roles_made,
};
use mandate_canon::Value;
use mandate_deploy_manager::control::fact;
use mandate_deploy_manager::fake::Call;
use mandate_deploy_manager::{Process, Slot};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::TestRunner;

const CONNECTION: &str = "conn_01J8Z2OAUTH";
const ACCOUNT: &str = "01J8Z2ACCT00000000000000B7";

fn exchange() -> Slot {
    Slot::token_exchange(CONNECTION)
}

fn tick_at(m: &mut Manager, now: UtcNanos) {
    block_on(m.tick(now)).unwrap();
}

fn tick(m: &mut Manager, secs: i64) {
    tick_at(m, at(secs));
}

/// The exchange's starts, as (generation, lifetime).
fn starts(m: &mut Manager) -> Vec<(u64, Option<Duration>)> {
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
            } if *slot == exchange() => Some((*generation, *lifetime)),
            _ => None,
        })
        .collect()
}

/// The exchange's starts, as (generation, its request's expiry).
fn expiries(m: &mut Manager) -> Vec<(u64, Option<UtcNanos>)> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::Start {
                slot,
                generation,
                expires_at,
                ..
            } if *slot == exchange() => Some((*generation, *expires_at)),
            _ => None,
        })
        .collect()
}

/// The expiry the stored `ConnectionRequested` row gives its request, read from the row rather than
/// from the manager: its `recorded_at` plus 600 seconds (DEC-2627 item 1, DEC-2363).
fn recorded_expiry(journal: &Journal) -> UtcNanos {
    let row = journal
        .rows()
        .into_iter()
        .find(|r| r.event_type == "ConnectionRequested")
        .unwrap();
    let recorded = UtcNanos::parse(&row.recorded_at).unwrap();
    UtcNanos::from_parts(recorded.secs() + 600, recorded.nanos()).unwrap()
}

fn stops(m: &mut Manager) -> Vec<u64> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter_map(|c| match c {
            Call::Stop {
                slot, generation, ..
            } if *slot == exchange() => Some(*generation),
            _ => None,
        })
        .collect()
}

/// The exchange's records, as (event type, generation, cause).
fn records(journal: &Journal) -> Vec<(String, i64, String)> {
    journal
        .records()
        .into_iter()
        .filter(|(_, p)| p.get("process").and_then(Value::as_str) == Some("token_exchange"))
        .map(|(t, p)| {
            assert_eq!(p.get("subject").and_then(Value::as_str), Some(CONNECTION));
            let generation = p.get("generation").and_then(Value::as_int).unwrap();
            let cause = p.get("cause").and_then(Value::as_str).unwrap().to_owned();
            (t, i64::try_from(generation).unwrap(), cause)
        })
        .collect()
}

fn start_record() -> (String, i64, String) {
    ("ProcessStartIssued".to_owned(), 1, "requested".to_owned())
}

fn stop_record() -> (String, i64, String) {
    (
        "ProcessStopIssued".to_owned(),
        1,
        "request_closed".to_owned(),
    )
}

fn clean(m: &mut Manager) {
    let violations = &m.orchestrator().violations;
    assert!(violations.is_empty(), "{violations:?}");
}

/// A manager with no deployment, and an OAuth request for [`CONNECTION`] committed at `secs`.
fn requested_at(secs: i64) -> (Journal, Manager) {
    let journal = Journal::new();
    let m = manager(&journal, accounts(&[]));
    journal.services(&requested(CONNECTION, ACCOUNT, Some("oauth")), at(secs));
    journal.services(&roles_made(CONNECTION), at(secs));
    (journal, m)
}

#[test]
fn an_oauth_request_starts_one_exchange_journaled_first_and_ended_by_its_deadline() {
    let (journal, mut m) = requested_at(10);
    tick(&mut m, 12);
    assert_eq!(records(&journal), [start_record()]);
    assert_eq!(
        starts(&mut m),
        [(1, Some(Duration::from_secs(598)))],
        "the request's recorded_at plus 600 seconds, less the time already gone"
    );
    tick(&mut m, 13);
    assert_eq!(starts(&mut m).len(), 1, "a running exchange is left alone");
    tick(&mut m, 700);
    assert!(
        stops(&mut m).is_empty() && records(&journal) == [start_record()],
        "the deadline ends it on its own; the manager never stops an open request's exchange"
    );
    clean(&mut m);
}

/// DEC-2363: the exchange's start names its request's expiry, the stored row's `recorded_at` plus
/// 600 seconds, never a time read from the manager's clock, so a start asked again after the
/// orchestrator refused it names the same instant, and only its lifetime is shorter. A restarted
/// manager never asks for it again (rule 256), and folds the same expiry from the same row.
#[test]
fn every_start_of_an_exchange_names_its_requests_expiry_whatever_the_clock_reads() {
    let (journal, mut m) = requested_at(10);
    tick(&mut m, 12);
    assert_eq!(expiries(&mut m), [(1, Some(recorded_expiry(&journal)))]);
    assert_eq!(recorded_expiry(&journal), at(610));
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(
            &(0_i64..100_000, 0_i64..598, 1_i64..600, any::<bool>()),
            |(committed, first, gap, refused)| {
                let (journal, mut m) = requested_at(committed);
                let later = (first + gap).min(599);
                m.orchestrator().fake.refuse_effects = refused;
                tick(&mut m, committed + first);
                if refused {
                    m.orchestrator().fake.refuse_effects = false;
                    tick(&mut m, committed + later);
                }
                let ran = if refused { later } else { first };
                prop_assert_eq!(expiries(&mut m), vec![(1, Some(recorded_expiry(&journal)))]);
                prop_assert_eq!(
                    starts(&mut m),
                    vec![(1, Some(Duration::from_secs((600 - ran).unsigned_abs())))]
                );
                Ok(())
            },
        )
        .unwrap();
}

/// DEC-2363: a manager that folds the same rows again, as after its restart, holds the same expiry,
/// and its request's `event_time` is not where it comes from; no other process has one.
#[test]
fn a_refolded_manager_holds_the_same_expiry_and_no_other_process_has_one() {
    let (journal, _) = requested_at(10);
    let folded = || {
        let mut folding = mandate_deploy_manager::Manager::new();
        for row in journal.rows() {
            if let Some(fact) = fact(&row) {
                folding.fold(&fact);
            }
        }
        folding
    };
    let (first, again) = (folded(), folded());
    assert_eq!(
        first.expires_at(&exchange()),
        Some(recorded_expiry(&journal))
    );
    assert_eq!(again.expires_at(&exchange()), first.expires_at(&exchange()));
    assert_ne!(
        first.expires_at(&exchange()),
        Some(at(600)),
        "the row's event_time, 2026-10-17T15:00:00Z, plus 600 seconds"
    );
    for slot in [Slot::runtime("agent_a"), Slot::executor(ACCOUNT)] {
        assert_eq!(first.expires_at(&slot), None, "{slot:?}");
    }
    assert_eq!(
        first.expires_at(&Slot::token_exchange("conn_unrequested")),
        None
    );
}

#[test]
fn a_version_1_or_api_key_request_starts_nothing() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    journal.services(&requested("conn_v1", ACCOUNT, None), at(1));
    journal.services(&requested("conn_key", ACCOUNT, Some("api_key")), at(1));
    tick(&mut m, 2);
    assert!(m.orchestrator().fake.calls().is_empty());
    assert!(journal.records().is_empty());
}

#[test]
fn an_exchange_is_never_attempted_twice() {
    let (journal, mut m) = requested_at(0);
    tick(&mut m, 1);
    m.orchestrator().fake.crash(&exchange());
    tick(&mut m, 2);
    tick(&mut m, 400);
    let (id, switch) = kill("workspace", "ws_01J8Z2", 0);
    journal.services(&switch, at(401));
    tick(&mut m, 402);
    let mut m = restart(m);
    tick(&mut m, 403);
    m.orchestrator().fake.collect();
    tick(&mut m, 404);
    assert_eq!(
        starts(&mut m).len(),
        1,
        "not after it exits, a kill switch ({id}), a restart, or its pod's removal"
    );
    assert_eq!(records(&journal), [start_record()]);
    clean(&mut m);
}

#[test]
fn nothing_starts_with_less_than_a_second_of_its_request_left() {
    let (journal, mut m) = requested_at(0);
    tick_at(&mut m, UtcNanos::from_parts(at(599).secs(), 1).unwrap());
    tick(&mut m, 600);
    tick(&mut m, 900);
    assert!(starts(&mut m).is_empty());
    assert!(journal.records().is_empty());
    let (journal, mut m) = requested_at(0);
    tick(&mut m, 599);
    assert_eq!(starts(&mut m), [(1, Some(Duration::from_secs(1)))]);
    assert_eq!(records(&journal), [start_record()]);
}

#[test]
fn a_start_the_orchestrator_refused_is_asked_again_only_while_its_request_has_time_left() {
    let (journal, mut m) = requested_at(0);
    m.orchestrator().fake.refuse_effects = true;
    tick(&mut m, 1);
    m.orchestrator().fake.refuse_effects = false;
    tick(&mut m, 300);
    assert_eq!(starts(&mut m), [(1, Some(Duration::from_secs(300)))]);
    assert_eq!(
        records(&journal),
        [start_record()],
        "one record, asked for twice"
    );

    let (journal, mut m) = requested_at(0);
    m.orchestrator().fake.refuse_effects = true;
    tick(&mut m, 1);
    m.orchestrator().fake.refuse_effects = false;
    tick(&mut m, 600);
    assert!(starts(&mut m).is_empty(), "the deadline has passed");
    assert_eq!(records(&journal), [start_record()]);
    clean(&mut m);
}

#[test]
fn a_start_whose_answer_was_lost_is_not_asked_again_once_its_pod_is_listed() {
    let (journal, mut m) = requested_at(0);
    m.orchestrator().fake.lose_start_answers = true;
    tick(&mut m, 1);
    m.orchestrator().fake.lose_start_answers = false;
    tick(&mut m, 2);
    tick(&mut m, 3);
    assert_eq!(
        starts(&mut m).len(),
        1,
        "its pod is listed, so it was carried out"
    );
    assert_eq!(records(&journal), [start_record()]);
    clean(&mut m);
}

#[test]
fn a_closed_request_sweeps_the_account_of_an_exchange_whose_pod_never_ran() {
    let (journal, mut m) = requested_at(0);
    m.orchestrator().fake.refuse_effects = true;
    tick(&mut m, 1);
    journal.services(&revoked(CONNECTION), at(2));
    tick(&mut m, 3);
    m.orchestrator().fake.refuse_effects = false;
    tick(&mut m, 4);
    tick(&mut m, 5);
    assert_eq!(records(&journal), [start_record(), stop_record()]);
    assert_eq!(
        stops(&mut m),
        [1],
        "its account is deleted, though no pod was ever listed, and once is enough"
    );
    let mut m = restart(m);
    tick(&mut m, 6);
    tick(&mut m, 7);
    assert_eq!(
        stops(&mut m),
        [1, 1],
        "a restarted manager asks once more, as it cannot know the first stop landed"
    );
    assert_eq!(records(&journal), [start_record(), stop_record()]);
    clean(&mut m);
}

#[test]
fn each_record_that_closes_the_request_stops_its_exchange_journaled_first() {
    let closers = [
        ("established", established(CONNECTION, ACCOUNT)),
        ("refused at connect", refused(CONNECTION, "connect")),
        ("revoked", revoked(CONNECTION)),
    ];
    for (name, closer) in closers {
        let (journal, mut m) = requested_at(0);
        tick(&mut m, 1);
        journal.services(&closer, at(5));
        tick(&mut m, 6);
        tick(&mut m, 7);
        assert_eq!(records(&journal), [start_record(), stop_record()], "{name}");
        assert_eq!(stops(&mut m), [1], "{name}: stopped once");
        assert_eq!(m.orchestrator().fake.live(&exchange()), 0, "{name}");
        clean(&mut m);
    }
}

#[test]
fn an_exchange_that_already_exited_is_still_removed_when_its_request_closes() {
    let (journal, mut m) = requested_at(0);
    tick(&mut m, 1);
    m.orchestrator().fake.crash(&exchange());
    journal.services(&refused(CONNECTION, "connect"), at(5));
    tick(&mut m, 6);
    tick(&mut m, 7);
    assert_eq!(records(&journal), [start_record(), stop_record()]);
    assert_eq!(stops(&mut m), [1]);
    assert!(
        m.orchestrator().fake.instances().is_empty(),
        "its pod, and with it its account, is deleted"
    );
}

#[test]
fn a_stop_the_orchestrator_refused_is_asked_again_with_no_second_record() {
    let (journal, mut m) = requested_at(0);
    tick(&mut m, 1);
    journal.services(&revoked(CONNECTION), at(5));
    m.orchestrator().fake.refuse_effects = true;
    tick(&mut m, 6);
    m.orchestrator().fake.refuse_effects = false;
    let mut m = restart(m);
    tick(&mut m, 7);
    tick(&mut m, 8);
    assert_eq!(records(&journal), [start_record(), stop_record()]);
    assert_eq!(stops(&mut m), [1]);
    assert_eq!(m.orchestrator().fake.live(&exchange()), 0);
    clean(&mut m);
}

#[test]
fn a_request_closed_before_the_manager_reads_it_starts_nothing() {
    let (journal, mut m) = requested_at(0);
    journal.services(&refused(CONNECTION, "connect"), at(1));
    tick(&mut m, 2);
    assert!(m.orchestrator().fake.calls().is_empty());
    assert!(journal.records().is_empty());
}

#[test]
fn a_refused_reconnect_closes_no_request() {
    let (journal, mut m) = requested_at(0);
    tick(&mut m, 1);
    journal.services(&refused(CONNECTION, "reconnect"), at(2));
    tick(&mut m, 3);
    assert!(stops(&mut m).is_empty());
    assert_eq!(records(&journal), [start_record()]);
}

#[test]
fn no_kill_switch_reaches_an_exchange() {
    let (journal, mut m) = requested_at(0);
    tick(&mut m, 1);
    for (n, (scope, subject)) in [
        ("connection", CONNECTION),
        ("workspace", "ws_01J8Z2"),
        ("agent", CONNECTION),
    ]
    .into_iter()
    .enumerate()
    {
        let (_, switch) = kill(scope, subject, u32::try_from(n).unwrap());
        journal.services(&switch, at(2));
    }
    tick(&mut m, 3);
    assert_eq!(starts(&mut m).len(), 1);
    assert!(stops(&mut m).is_empty());
    assert!(
        !m.manager.covers(
            &mandate_deploy_manager::Scope::Connection(CONNECTION.to_owned()),
            &exchange()
        ),
        "it holds no order"
    );
    assert_eq!(exchange().process, Process::TokenExchange);
}

/// The property's connections.
const CONNECTIONS: [&str; 3] = [
    "conn_01J8Z2OAUTH0",
    "conn_01J8Z2OAUTH1",
    "conn_01J8Z2OAUTH2",
];

#[derive(Debug, Clone)]
enum Op {
    Request(usize),
    /// The provisioner makes the connection's roles, answering its request, once there is one.
    MakeRoles(usize),
    Establish(usize),
    RefuseConnect(usize),
    RefuseReconnect(usize),
    Revoke(usize),
    /// The connection's exchange exits, failed or done.
    Exit(usize),
    KillWorkspace,
    /// Starts and stops answer unavailable until the next `Heal`.
    Outage,
    Heal,
    Advance(i64),
    Restart,
    Collect,
}

fn op() -> impl Strategy<Value = Op> {
    let c = 0..CONNECTIONS.len();
    prop_oneof![
        3 => c.clone().prop_map(Op::Request),
        3 => c.clone().prop_map(Op::MakeRoles),
        1 => c.clone().prop_map(Op::Establish),
        1 => c.clone().prop_map(Op::RefuseConnect),
        1 => c.clone().prop_map(Op::RefuseReconnect),
        1 => c.clone().prop_map(Op::Revoke),
        2 => c.prop_map(Op::Exit),
        1 => Just(Op::KillWorkspace),
        1 => Just(Op::Outage),
        1 => Just(Op::Heal),
        4 => (0_i64..400).prop_map(Op::Advance),
        1 => Just(Op::Restart),
        1 => Just(Op::Collect),
    ]
}

/// What the oracle reads after a pass: the control stream's rows, the orchestrator's calls and
/// instances, and the pass's time and whether effects could run in it.
struct Observed {
    rows: Vec<mandate_journal::StoredEvent>,
    calls: Vec<Call>,
    instances: Vec<mandate_deploy_manager::Instance>,
    now: UtcNanos,
    effects: bool,
}

fn nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs()) * 1_000_000_000 + i128::from(at.nanos())
}

/// The oracle, from the rows alone, in journal order: each connection's OAuth request, the
/// provisioner's `ConnectionRolesMade` answering it, the first record closing it, and the
/// manager's records for its exchange, never the manager's fold. An exchange starts only once its
/// request's roles are made (journal spec §9.21, DEC-2350).
fn check(seen: &Observed) -> Result<(), String> {
    for connection in CONNECTIONS {
        let mut requested: Option<(u64, i128)> = None;
        let mut request_id: Option<String> = None;
        let mut made: Option<u64> = None;
        let mut closed: Option<u64> = None;
        let mut started = Vec::new();
        for row in &seen.rows {
            let body = mandate_canon::parse(&row.body).unwrap();
            let payload = body.get("payload").unwrap();
            let text = |m: &str| payload.get(m).and_then(Value::as_str);
            let recorded = nanos(UtcNanos::parse(&row.recorded_at).unwrap());
            let ours = text("connection_id") == Some(connection);
            match row.event_type.as_str() {
                "ConnectionRequested" if ours => {
                    requested = Some((row.seq, recorded));
                    request_id = Some(row.event_id.clone());
                }
                "ConnectionRolesMade"
                    if ours
                        && body.get("causation_id").and_then(Value::as_str)
                            == request_id.as_deref() =>
                {
                    made = made.or(Some(row.seq));
                }
                "ConnectionEstablished" | "ConnectionRevoked" if ours => {
                    closed = closed.or(Some(row.seq));
                }
                "ConnectionRefused" if ours && text("occasion") == Some("connect") => {
                    closed = closed.or(Some(row.seq));
                }
                "ProcessStartIssued" | "ProcessStopIssued"
                    if text("process") == Some("token_exchange")
                        && text("subject") == Some(connection) =>
                {
                    let Some((asked, at)) = requested else {
                        return Err(format!("{connection}: a record before its request"));
                    };
                    if payload.get("generation").and_then(Value::as_int) != Some(1) {
                        return Err(format!("{connection}: a generation other than 1"));
                    }
                    if row.event_type == "ProcessStartIssued" {
                        if closed.is_some() || row.seq < asked {
                            return Err(format!("{connection}: started outside its request"));
                        }
                        if made.is_none() {
                            return Err(format!("{connection}: started before its roles are made"));
                        }
                        if recorded > at + 599_000_000_000 {
                            return Err(format!("{connection}: started past its deadline"));
                        }
                        started.push(row.seq);
                    } else if closed.is_none() {
                        return Err(format!("{connection}: stopped while its request is open"));
                    }
                }
                _ => {}
            }
        }
        if started.len() > 1 {
            return Err(format!("{connection}: started twice"));
        }
        let slot = Slot::token_exchange(connection);
        let launched = seen
            .calls
            .iter()
            .filter(|c| matches!(c, Call::Start { slot: s, .. } if *s == slot))
            .count();
        if launched > 1 {
            return Err(format!("{connection}: launched twice"));
        }
        let listed = seen.instances.iter().any(|i| i.slot == slot);
        if seen.effects && closed.is_some() && listed {
            return Err(format!(
                "{connection}: left running after its request closed"
            ));
        }
        let swept = seen
            .calls
            .iter()
            .any(|c| matches!(c, Call::Stop { slot: s, .. } if *s == slot));
        if seen.effects && closed.is_some() && !started.is_empty() && !swept {
            return Err(format!(
                "{connection}: its account kept after its request closed"
            ));
        }
        if let Some((_, at)) = requested
            && seen.effects
            && closed.is_none()
            && made.is_some()
            && nanos(seen.now) <= at + 599_000_000_000
            && started.is_empty()
        {
            return Err(format!(
                "{connection}: not started while its request is open"
            ));
        }
    }
    Ok(())
}

fn run(ops: &[Op]) -> Result<(), String> {
    run_tampered(ops, |_| {})
}

/// The property, with `tamper` applied to what the oracle reads after each pass, so each of its
/// checks is shown to catch a bug before it is trusted.
fn run_tampered(ops: &[Op], tamper: impl Fn(&mut Observed)) -> Result<(), String> {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    let mut now = 1_i64;
    let mut kills = 0_u32;
    let mut sent = std::collections::BTreeSet::new();
    let mut outage = false;
    let once = |sent: &mut std::collections::BTreeSet<Vec<u8>>, draft: &[u8], now: i64| {
        if sent.insert(draft.to_vec()) {
            journal.services(draft, at(now));
        }
    };
    for op in ops {
        match op {
            Op::Request(i) => {
                let draft = requested(CONNECTIONS[*i], ACCOUNT, Some("oauth"));
                once(&mut sent, &draft, now);
            }
            Op::MakeRoles(i) => {
                let request = requested(CONNECTIONS[*i], ACCOUNT, Some("oauth"));
                if sent.contains(&request) {
                    once(&mut sent, &roles_made(CONNECTIONS[*i]), now);
                }
            }
            Op::Establish(i) => once(&mut sent, &established(CONNECTIONS[*i], ACCOUNT), now),
            Op::RefuseConnect(i) => once(&mut sent, &refused(CONNECTIONS[*i], "connect"), now),
            Op::RefuseReconnect(i) => {
                once(&mut sent, &refused(CONNECTIONS[*i], "reconnect"), now);
            }
            Op::Revoke(i) => once(&mut sent, &revoked(CONNECTIONS[*i]), now),
            Op::Exit(i) => m
                .orchestrator()
                .fake
                .crash(&Slot::token_exchange(CONNECTIONS[*i])),
            Op::KillWorkspace => {
                journal.services(&kill("workspace", "ws_01J8Z2", kills).1, at(now));
                kills += 1;
            }
            Op::Outage => outage = true,
            Op::Heal => outage = false,
            Op::Advance(secs) => now += secs,
            Op::Restart => m = restart(m),
            Op::Collect => m.orchestrator().fake.collect(),
        }
        m.orchestrator().fake.refuse_effects = outage;
        block_on(m.tick(at(now))).map_err(|h| format!("{h:?}"))?;
        let mut seen = Observed {
            rows: journal.rows(),
            calls: m.orchestrator().fake.calls().to_vec(),
            instances: m.orchestrator().fake.instances(),
            now: at(now),
            effects: !outage,
        };
        tamper(&mut seen);
        check(&seen)?;
        let violations = &m.orchestrator().violations;
        if !violations.is_empty() {
            return Err(format!("{violations:?}"));
        }
    }
    Ok(())
}

#[test]
fn an_exchange_runs_once_inside_its_request_and_never_outlives_its_closing() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    runner
        .run(&prop::collection::vec(op(), 1..60), |ops| {
            run(&ops).map_err(TestCaseError::fail)
        })
        .unwrap();
}

/// A bug seeded into what the oracle reads.
type Tamper = Box<dyn Fn(&mut Observed)>;

/// Each check of the oracle fails on the bug seeded for it, before it is trusted.
#[test]
fn every_check_of_the_exchange_oracle_catches_its_seeded_bug() {
    let opened = [Op::Request(0), Op::MakeRoles(0), Op::Advance(1)];
    let closed = [
        Op::Request(0),
        Op::MakeRoles(0),
        Op::Advance(1),
        Op::Revoke(0),
        Op::Advance(1),
    ];
    let start_again = |seen: &mut Observed| {
        let start = seen
            .rows
            .iter()
            .find(|r| r.event_type == "ProcessStartIssued")
            .cloned();
        seen.rows.extend(start);
    };
    let revoked = |seen: &Observed| {
        seen.rows
            .iter()
            .any(|r| r.event_type == "ConnectionRevoked")
    };
    let late = |seen: &mut Observed| {
        if let Some(i) = seen
            .rows
            .iter()
            .position(|r| r.event_type == "ProcessStartIssued")
        {
            seen.rows[i].recorded_at = at(700).to_string();
        }
    };
    let cases: [(&[Op], Tamper, &str); 9] = [
        (
            &opened,
            Box::new(|seen| seen.rows.retain(|r| r.event_type != "ConnectionRolesMade")),
            "started before its roles are made",
        ),
        (&opened, Box::new(start_again), "started twice"),
        (
            &closed,
            Box::new(move |seen| {
                if revoked(seen) {
                    start_again(seen);
                }
            }),
            "started outside its request",
        ),
        (&opened, Box::new(late), "past its deadline"),
        (
            &closed,
            Box::new(|seen| seen.calls.retain(|c| !matches!(c, Call::Stop { .. }))),
            "its account kept",
        ),
        (
            &opened,
            Box::new(|seen| {
                if let Some(again) = seen.calls.first().cloned() {
                    seen.calls.push(again);
                }
            }),
            "launched twice",
        ),
        (
            &closed,
            Box::new(move |seen| {
                if !revoked(seen) {
                    return;
                }
                seen.instances.push(mandate_deploy_manager::Instance {
                    slot: Slot::token_exchange(CONNECTIONS[0]),
                    generation: 1,
                    phase: mandate_deploy_manager::Phase::Exited,
                });
            }),
            "left running",
        ),
        (
            &opened,
            Box::new(|seen| seen.rows.retain(|r| r.event_type != "ProcessStartIssued")),
            "not started",
        ),
        (
            &closed,
            Box::new(|seen| {
                let revoked = seen
                    .rows
                    .iter()
                    .position(|r| r.event_type == "ConnectionRevoked");
                if let Some(i) = revoked {
                    seen.rows.remove(i);
                }
            }),
            "stopped while its request is open",
        ),
    ];
    for (ops, tamper, check) in cases {
        let caught = run_tampered(ops, tamper).expect_err(&format!("{check} escaped"));
        assert!(caught.contains(check), "{check}: {caught}");
    }
    run(&opened).unwrap();
    run(&closed).unwrap();
}
