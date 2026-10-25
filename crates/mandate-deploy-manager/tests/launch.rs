//! DEC-2060 item 9 (E21-33 K2b): the manager names an executor's connection on its every start and
//! stop, so the orchestrator runs it as that connection's own account and deletes the account with
//! it: the connection a deployment of the account names, or the connect whose exchange started it,
//! read from the journal alone, so a restarted manager names the same one. A runtime and a token
//! exchange name none.

mod common;

use common::{
    Journal, Manager, accounts, at, block_on, connection_of, deployed, established, manager,
    refused, requested, restart, revoked, roles_made, stopped,
};
use mandate_deploy_manager::fake::Call;
use mandate_deploy_manager::{Process, Slot};

const AGENT: &str = "agent_l0";
const DEPLOYED: &str = "01J8Z2ACCT00000000000000M1";
const CONNECTING: &str = "01J8Z2ACCT00000000000000M2";
const CONNECT: &str = "conn_01J8Z2LAUNCH";

fn tick(m: &mut Manager, secs: i64) {
    block_on(m.tick(at(secs))).unwrap();
}

/// Every start and stop, as (start or stop, slot, connection).
fn calls(m: &mut Manager) -> Vec<(&'static str, Slot, Option<String>)> {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .map(|c| match c {
            Call::Start {
                slot, connection, ..
            } => ("start", slot.clone(), connection.clone()),
            Call::Stop {
                slot, connection, ..
            } => ("stop", slot.clone(), connection.clone()),
        })
        .collect()
}

fn clean(m: &mut Manager) {
    let violations = &m.orchestrator().violations;
    assert!(violations.is_empty(), "{violations:?}");
}

#[test]
fn a_deployments_executor_starts_and_stops_naming_the_connection_it_is_deployed_on() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[(AGENT, DEPLOYED)]));
    journal.services(&deployed(AGENT, DEPLOYED), at(1));
    tick(&mut m, 2);
    m.orchestrator().fake.crash(&Slot::executor(DEPLOYED));
    let mut m = restart(m);
    tick(&mut m, 3);
    tick(&mut m, 10);
    journal.services(&stopped(AGENT, DEPLOYED), at(11));
    tick(&mut m, 12);
    let connection = Some(connection_of(DEPLOYED));
    let executor = Slot::executor(DEPLOYED);
    let runtime = Slot::runtime(AGENT);
    let mut seen = calls(&mut m);
    seen.sort();
    let mut want = vec![
        ("start", executor.clone(), connection.clone()),
        ("start", executor.clone(), connection.clone()),
        ("start", runtime.clone(), None),
        ("stop", executor, connection),
        ("stop", runtime, None),
    ];
    want.sort();
    assert_eq!(
        seen, want,
        "the restart after the manager's own restart names it too"
    );
    clean(&mut m);
}

#[test]
fn a_connects_executor_starts_and_stops_naming_its_connect() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    journal.services(&requested(CONNECT, CONNECTING, Some("oauth")), at(0));
    journal.services(&roles_made(CONNECT), at(0));
    tick(&mut m, 1);
    m.orchestrator()
        .fake
        .succeed(&Slot::token_exchange(CONNECT));
    tick(&mut m, 2);
    journal.services(&refused(CONNECT, "connect"), at(3));
    tick(&mut m, 4);
    let connection = Some(CONNECT.to_owned());
    let executor = Slot::executor(CONNECTING);
    let exchange = Slot::token_exchange(CONNECT);
    let mut seen = calls(&mut m);
    seen.sort();
    let mut want = vec![
        ("start", exchange.clone(), None),
        ("start", executor.clone(), connection.clone()),
        ("stop", exchange, None),
        ("stop", executor, connection),
    ];
    want.sort();
    assert_eq!(seen, want);
    clean(&mut m);
}

#[test]
fn each_executor_names_its_own_accounts_connection_beside_another_accounts_connect() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[(AGENT, DEPLOYED)]));
    journal.services(&requested(CONNECT, CONNECTING, Some("oauth")), at(0));
    journal.services(&roles_made(CONNECT), at(0));
    tick(&mut m, 1);
    m.orchestrator()
        .fake
        .succeed(&Slot::token_exchange(CONNECT));
    journal.services(&deployed(AGENT, DEPLOYED), at(2));
    tick(&mut m, 3);
    let starts: Vec<(Slot, Option<String>)> = calls(&mut m)
        .into_iter()
        .filter(|(kind, slot, _)| *kind == "start" && *slot == Slot::executor(DEPLOYED))
        .map(|(_, slot, connection)| (slot, connection))
        .collect();
    assert_eq!(
        starts,
        [(Slot::executor(DEPLOYED), Some(connection_of(DEPLOYED)))],
        "not the other account's connect"
    );
    clean(&mut m);
}

#[test]
fn two_connects_of_one_account_that_succeed_in_one_pass_start_its_executor_once() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    let other = "conn_01J8Z2LAUNCH2";
    journal.services(&requested(CONNECT, CONNECTING, Some("oauth")), at(0));
    journal.services(&roles_made(CONNECT), at(0));
    journal.services(&requested(other, CONNECTING, Some("oauth")), at(0));
    journal.services(&roles_made(other), at(0));
    tick(&mut m, 1);
    for connection in [CONNECT, other] {
        m.orchestrator()
            .fake
            .succeed(&Slot::token_exchange(connection));
    }
    tick(&mut m, 2);
    let starts = calls(&mut m)
        .into_iter()
        .filter(|(kind, slot, _)| *kind == "start" && *slot == Slot::executor(CONNECTING))
        .count();
    assert_eq!(
        starts, 1,
        "one journaled start, asked of the orchestrator once"
    );
    clean(&mut m);
}

/// A connection of [`DEPLOYED`]'s account that sorts before its current one, established and then
/// revoked before the agent is deployed on the current one.
const OLD: &str = "conn_00J8Z2OLD";

#[test]
fn a_deployments_executor_names_the_connection_the_deployment_names_not_a_revoked_one() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    journal.services(&established(OLD, DEPLOYED), at(0));
    journal.services(&revoked(OLD), at(0));
    journal.services(&established(&connection_of(DEPLOYED), DEPLOYED), at(0));
    journal.services(&deployed(AGENT, DEPLOYED), at(1));
    tick(&mut m, 2);
    journal.services(&stopped(AGENT, DEPLOYED), at(3));
    tick(&mut m, 4);
    let executor: Vec<(&str, Option<String>)> = calls(&mut m)
        .into_iter()
        .filter(|(_, slot, _)| *slot == Slot::executor(DEPLOYED))
        .map(|(kind, _, connection)| (kind, connection))
        .collect();
    let current = Some(connection_of(DEPLOYED));
    assert_eq!(
        executor,
        [("start", current.clone()), ("stop", current)],
        "the deployment's connection, never the revoked one"
    );
    clean(&mut m);
}

#[test]
fn a_restart_after_the_connect_ended_names_the_deployments_connection_across_the_managers_restart()
{
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    let current = connection_of(CONNECTING);
    journal.services(&requested(CONNECT, CONNECTING, Some("oauth")), at(0));
    journal.services(&roles_made(CONNECT), at(0));
    tick(&mut m, 1);
    m.orchestrator()
        .fake
        .succeed(&Slot::token_exchange(CONNECT));
    tick(&mut m, 2);
    journal.services(&established(&current, CONNECTING), at(3));
    journal.services(&deployed(AGENT, CONNECTING), at(3));
    tick(&mut m, 4);
    journal.services(&refused(CONNECT, "connect"), at(5));
    tick(&mut m, 6);
    m.orchestrator().fake.crash(&Slot::executor(CONNECTING));
    let mut m = restart(m);
    tick(&mut m, 7);
    tick(&mut m, 20);
    journal.services(&stopped(AGENT, CONNECTING), at(21));
    tick(&mut m, 22);
    let mut m = restart(m);
    tick(&mut m, 23);
    let executor: Vec<(&str, Option<String>)> = calls(&mut m)
        .into_iter()
        .filter(|(_, slot, _)| *slot == Slot::executor(CONNECTING))
        .map(|(kind, _, connection)| (kind, connection))
        .collect();
    assert_eq!(
        executor,
        [
            ("start", Some(CONNECT.to_owned())),
            ("start", Some(current.clone())),
            ("stop", Some(current)),
        ],
        "the connect's generation by its connect, the restart after it by the deployment's"
    );
    clean(&mut m);
}

#[test]
fn a_stop_names_the_connection_its_generation_started_with_once_nothing_names_one() {
    let journal = Journal::new();
    let mut m = manager(&journal, accounts(&[]));
    journal.services(&requested(CONNECT, CONNECTING, Some("oauth")), at(0));
    journal.services(&roles_made(CONNECT), at(0));
    tick(&mut m, 1);
    m.orchestrator()
        .fake
        .succeed(&Slot::token_exchange(CONNECT));
    tick(&mut m, 2);
    journal.services(&established(&connection_of(CONNECTING), CONNECTING), at(3));
    journal.services(&deployed(AGENT, CONNECTING), at(3));
    tick(&mut m, 4);
    journal.services(&stopped(AGENT, CONNECTING), at(5));
    journal.services(&refused(CONNECT, "connect"), at(5));
    let mut m = restart(m);
    tick(&mut m, 6);
    let executor: Vec<(&str, Option<String>)> = calls(&mut m)
        .into_iter()
        .filter(|(_, slot, _)| *slot == Slot::executor(CONNECTING))
        .map(|(kind, _, connection)| (kind, connection))
        .collect();
    assert_eq!(
        executor,
        [
            ("start", Some(CONNECT.to_owned())),
            ("stop", Some(CONNECT.to_owned())),
        ],
        "the account its start made is the one its stop deletes"
    );
    clean(&mut m);
}

#[test]
fn each_executor_names_its_own_accounts_connection_beside_other_accounts_deployments_and_connects()
{
    let first = "01J8Z2ACCT00000000000000P1";
    let second = "01J8Z2ACCT00000000000000Q1";
    let third = "01J8Z2ACCT00000000000000R1";
    let fourth = "01J8Z2ACCT00000000000000S1";
    let journal = Journal::new();
    let mut m = manager(
        &journal,
        accounts(&[("agent_a", first), ("agent_b", second)]),
    );
    journal.services(&requested("conn_01J8Z2X", third, Some("oauth")), at(0));
    journal.services(&roles_made("conn_01J8Z2X"), at(0));
    journal.services(&requested("conn_01J8Z2Y", fourth, Some("oauth")), at(0));
    journal.services(&roles_made("conn_01J8Z2Y"), at(0));
    tick(&mut m, 1);
    for connection in ["conn_01J8Z2X", "conn_01J8Z2Y"] {
        m.orchestrator()
            .fake
            .succeed(&Slot::token_exchange(connection));
    }
    journal.services(&deployed("agent_a", first), at(2));
    journal.services(&deployed("agent_b", second), at(2));
    tick(&mut m, 3);
    let mut starts: Vec<(Slot, Option<String>)> = calls(&mut m)
        .into_iter()
        .filter(|(kind, slot, _)| *kind == "start" && slot.process == Process::Executor)
        .map(|(_, slot, connection)| (slot, connection))
        .collect();
    starts.sort();
    let mut want = vec![
        (Slot::executor(first), Some(connection_of(first))),
        (Slot::executor(second), Some(connection_of(second))),
        (Slot::executor(third), Some("conn_01J8Z2X".to_owned())),
        (Slot::executor(fourth), Some("conn_01J8Z2Y".to_owned())),
    ];
    want.sort();
    assert_eq!(
        starts, want,
        "no executor takes another account's connection"
    );
    clean(&mut m);
}
