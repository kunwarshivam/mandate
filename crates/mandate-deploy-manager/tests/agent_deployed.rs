//! Journal spec v0.54 §9.21's reading of a deployment's account (DEC-1171 item 4, DEC-1173; E21-33),
//! judged against the vectors' `agent_deployed` streams, whose `processes` `reference/journal/
//! deployed.py` computes with its own fold:
//! - a version-2 `AgentDeployed` runs on the `account_ref` of its connection's version-2
//!   `ConnectionEstablished` earlier on the stream, when that connection is not revoked since;
//! - a version-1 one, and a version-2 one rule 201 refuses, start nothing, and a later establishment
//!   revives neither;
//! - an `AgentStopped` is final, whether or not the manager read the deployment it stops;
//! - a revocation after the deployment stops nothing.
//!
//! Each stream's rows are folded through [`fact`] and [`Manager::fold`], and the first pass with
//! nothing running must journal a `deployed` start for exactly the vector's processes. Before
//! DEC-1171's code change the manager asked a resolver for every deployment, so no vector stream
//! could start anything without one. Every version-1 deployment of an agent not stopped raises one
//! opaque operator alert (DEC-1173 item 3), and so does every version-2 one the vector runs nothing
//! for, of an agent not stopped (DEC-2038); nothing else raises either.

use std::collections::BTreeSet;
use std::path::Path;

use mandate_canon::{Digest, Value, parse, to_canonical};
use mandate_deploy_manager::control::fact;
use mandate_deploy_manager::orchestrator::{DEPLOYMENT_UNCONNECTED, DEPLOYMENT_UNREAD};
use mandate_deploy_manager::{Cause, Decision, Effect, Manager, Process, Record};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

const AT: &str = "2026-10-17T16:00:00.000000000Z";

fn section() -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases/journal.json");
    let fixture = parse(&std::fs::read(path).unwrap()).unwrap();
    fixture.get("agent_deployed").cloned().unwrap()
}

fn list<'a>(value: &'a Value, name: &str) -> &'a [Value] {
    value
        .get(name)
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn text<'a>(value: &'a Value, name: &str) -> &'a str {
    value.get(name).and_then(Value::as_str).unwrap_or_default()
}

fn texts(value: &Value, name: &str) -> BTreeSet<String> {
    list(value, name)
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

/// The case's control-stream records as the manager reads them; an account stream's are not its.
fn rows(case: &Value) -> Vec<StoredEvent> {
    list(case, "records")
        .iter()
        .filter(|record| text(record, "stream_id").starts_with("ctl:"))
        .zip(1..)
        .map(|(record, seq)| StoredEvent {
            seq,
            event_id: text(record, "event_id").to_owned(),
            stream_id: text(record, "stream_id").to_owned(),
            event_type: text(record, "event_type").to_owned(),
            schema_version: record
                .get("schema_version")
                .and_then(Value::as_int)
                .unwrap(),
            environment: text(record, "environment").to_owned(),
            recorded_at: AT.to_owned(),
            prev_hash: Digest::ZERO,
            hash: Digest::ZERO,
            body: to_canonical(record),
        })
        .collect()
}

/// What the first pass over the folded stream asks for.
struct Pass {
    runtimes: BTreeSet<String>,
    executors: BTreeSet<String>,
    unread: usize,
    unconnected: usize,
}

/// The first pass over the folded stream: the slots it journals a `deployed` start for, by
/// process, and how many alerts of each kind it raises.
fn first_pass(case: &Value) -> Pass {
    let mut manager = Manager::new();
    for row in rows(case) {
        if let Some(fact) = fact(&row) {
            manager.fold(&fact);
        }
    }
    let now = UtcNanos::parse(AT).unwrap();
    let (mut runtimes, mut executors) = (BTreeSet::new(), BTreeSet::new());
    let (mut unread, mut unconnected) = (0, 0);
    for decision in manager.decide(&[], now) {
        match decision {
            Decision::Journal(Record::Start {
                slot,
                generation: 1,
                cause: Cause::Deployed,
                kill_switch: None,
            }) => {
                let started = match slot.process {
                    Process::Runtime => &mut runtimes,
                    Process::Executor => &mut executors,
                    Process::TokenExchange => panic!("a deployment starts no token exchange"),
                };
                assert!(started.insert(slot.subject), "one start per slot");
            }
            Decision::Retry(Effect::Alert(alert)) if alert == DEPLOYMENT_UNREAD => unread += 1,
            Decision::Retry(Effect::Alert(alert)) if alert == DEPLOYMENT_UNCONNECTED => {
                unconnected += 1;
            }
            other => panic!(
                "{}: a first pass asks only for starts: {other:?}",
                text(case, "name")
            ),
        }
    }
    Pass {
        runtimes,
        executors,
        unread,
        unconnected,
    }
}

/// The agents a version-2 deployment names that no `AgentStopped` names and the vector runs no
/// runtime for: the deployments rule 201 refuses, which the manager must report.
fn unconnected_v2(case: &Value, running: &BTreeSet<String>) -> usize {
    let records = list(case, "records");
    let stopped: BTreeSet<&str> = records
        .iter()
        .filter(|r| text(r, "event_type") == "AgentStopped")
        .map(|r| text(r.get("payload").unwrap(), "agent_id"))
        .collect();
    let refused: BTreeSet<&str> = records
        .iter()
        .filter(|r| text(r, "event_type") == "AgentDeployed")
        .filter(|r| r.get("schema_version").and_then(Value::as_int) == Some(2))
        .map(|r| text(r.get("payload").unwrap(), "agent_id"))
        .filter(|agent| !stopped.contains(agent) && !running.contains(*agent))
        .collect();
    refused.len()
}

/// The version-1 deployments of agents that no `AgentStopped` names and no later version-2
/// deployment the vector runs has read: the deployments a manager cannot run and must report.
fn unread_v1(case: &Value, running: &BTreeSet<String>) -> usize {
    let records = list(case, "records");
    let stopped: BTreeSet<&str> = records
        .iter()
        .filter(|r| text(r, "event_type") == "AgentStopped")
        .map(|r| text(r.get("payload").unwrap(), "agent_id"))
        .collect();
    let unread: BTreeSet<&str> = records
        .iter()
        .filter(|r| text(r, "event_type") == "AgentDeployed")
        .filter(|r| r.get("schema_version").and_then(Value::as_int) == Some(1))
        .map(|r| text(r.get("payload").unwrap(), "agent_id"))
        .filter(|agent| !stopped.contains(agent) && !running.contains(*agent))
        .collect();
    unread.len()
}

#[test]
fn every_agent_deployed_stream_starts_exactly_the_processes_its_vector_lists() {
    let section = section();
    let cases = list(&section, "streams");
    let mut failed = Vec::new();
    let (mut alerted, mut refused) = (0, 0);
    for case in cases {
        let want = case.get("processes").unwrap();
        let Pass {
            runtimes,
            executors,
            unread,
            unconnected,
        } = first_pass(case);
        let expected = (texts(want, "runtimes"), texts(want, "executors"));
        if (runtimes.clone(), executors.clone()) != expected {
            failed.push(format!(
                "{} ({}): expected {expected:?}, got {:?}",
                text(case, "name"),
                text(case, "clause"),
                (runtimes.clone(), executors)
            ));
        }
        let unread_expected = unread_v1(case, &runtimes);
        if unread != unread_expected {
            failed.push(format!(
                "{}: {unread} unread-deployment alerts, expected {unread_expected}",
                text(case, "name")
            ));
        }
        let unconnected_expected = unconnected_v2(case, &runtimes);
        if unconnected != unconnected_expected {
            failed.push(format!(
                "{}: {unconnected} unconnected-deployment alerts, expected {unconnected_expected}",
                text(case, "name")
            ));
        }
        alerted += unread;
        refused += unconnected;
    }
    assert!(failed.is_empty(), "{}", failed.join("\n"));
    assert!(
        cases.len() >= 17 && alerted >= 2 && refused >= 2,
        "{} streams, {alerted} unread and {refused} unconnected alerts; never fewer",
        cases.len()
    );
}

#[test]
fn an_unread_deployment_is_reported_once_and_never_again_in_later_passes() {
    let section = section();
    let case = list(&section, "streams")
        .iter()
        .find(|c| text(c, "name") == "version_1_deployment")
        .unwrap();
    let mut manager = Manager::new();
    for row in rows(case) {
        if let Some(fact) = fact(&row) {
            manager.fold(&fact);
        }
    }
    let now = UtcNanos::parse(AT).unwrap();
    assert_eq!(
        manager.decide(&[], now),
        vec![Decision::Retry(Effect::Alert(DEPLOYMENT_UNREAD))]
    );
    assert!(manager.decide(&[], now).is_empty(), "raised once");
    assert_eq!(
        (DEPLOYMENT_UNREAD.code, DEPLOYMENT_UNREAD.runbook),
        ("deployment_unread", "RB-14"),
        "a reason code and a runbook, nothing else"
    );
}
