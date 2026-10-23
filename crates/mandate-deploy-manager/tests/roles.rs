//! DEC-2350 (journal spec §9.21 and §9.33, v0.77): an OAuth connect's token exchange logs in with
//! the Kubernetes-auth roles the role provisioner makes for its connection, so the deployment
//! manager starts it only once it has read that request's `ConnectionRolesMade`. A refusal, a
//! record for another request, or none starts nothing, and the connect ends by its deadline; a
//! close read before the start leaves it unstarted; a restarted manager, folding from genesis,
//! starts it once. The pending tests fail on the manager's behaviour, which has no stub to stop at
//! (`xtask/behaviour-only`).

mod common;

use common::{
    Journal, Manager, accounts, at, block_on, manager, refused, request_id, requested, restart,
    roles_made, roles_made_for, roles_refused,
};
use mandate_canon::Value;
use mandate_deploy_manager::Slot;
use mandate_deploy_manager::fake::Call;

const CONNECTION: &str = "conn_01J8Z2ROLES";
const ACCOUNT: &str = "01J8Z2ACCT00000000000000R0";

fn exchange() -> Slot {
    Slot::token_exchange(CONNECTION)
}

fn tick(m: &mut Manager, secs: i64) {
    block_on(m.tick(at(secs))).unwrap();
}

/// The orchestrator's starts of the exchange.
fn launched(m: &mut Manager) -> usize {
    m.orchestrator()
        .fake
        .calls()
        .iter()
        .filter(|c| matches!(c, Call::Start { slot, .. } if *slot == exchange()))
        .count()
}

/// The manager's records for the exchange, as (event type, cause).
fn records(journal: &Journal) -> Vec<(String, String)> {
    journal
        .records()
        .into_iter()
        .filter(|(_, p)| p.get("process").and_then(Value::as_str) == Some("token_exchange"))
        .map(|(t, p)| {
            let cause = p.get("cause").and_then(Value::as_str).unwrap().to_owned();
            (t, cause)
        })
        .collect()
}

fn started() -> Vec<(String, String)> {
    vec![("ProcessStartIssued".to_owned(), "requested".to_owned())]
}

fn clean(m: &mut Manager) {
    let violations = &m.orchestrator().violations;
    assert!(violations.is_empty(), "{violations:?}");
}

/// A manager with no deployment and an OAuth request for [`CONNECTION`] committed at 0.
fn requesting() -> (Journal, Manager) {
    let journal = Journal::new();
    let m = manager(&journal, accounts(&[]));
    journal.services(&requested(CONNECTION, ACCOUNT, Some("oauth")), at(0));
    (journal, m)
}

#[test]
fn an_exchange_starts_only_once_its_requests_roles_are_made() {
    let (journal, mut m) = requesting();
    for secs in [1, 2, 30] {
        tick(&mut m, secs);
    }
    assert!(records(&journal).is_empty(), "nothing before the roles");
    assert_eq!(launched(&mut m), 0);
    journal.services(&roles_made(CONNECTION), at(31));
    tick(&mut m, 32);
    assert_eq!(records(&journal), started());
    assert_eq!(launched(&mut m), 1);
    tick(&mut m, 33);
    assert_eq!(
        records(&journal),
        started(),
        "a running exchange is left alone"
    );
    clean(&mut m);
}

#[test]
fn refused_or_absent_roles_start_nothing_and_the_connect_ends_at_its_deadline() {
    for refusal in [Some("vault_unavailable"), Some("role_refused"), None] {
        let (journal, mut m) = requesting();
        if let Some(reason) = refusal {
            journal.services(&roles_refused(CONNECTION, reason), at(1));
        }
        for secs in [2, 300, 599, 601] {
            tick(&mut m, secs);
        }
        journal.services(&refused(CONNECTION, "connect"), at(602));
        tick(&mut m, 603);
        assert!(records(&journal).is_empty(), "{refusal:?}");
        assert_eq!(launched(&mut m), 0, "{refusal:?}");
        clean(&mut m);
    }
}

#[test]
fn a_made_for_another_request_starts_nothing() {
    let (journal, mut m) = requesting();
    let other = request_id("conn_01J8Z2OTHER");
    journal.services(&roles_made_for(CONNECTION, &other), at(1));
    for secs in [2, 3, 300] {
        tick(&mut m, secs);
    }
    assert!(records(&journal).is_empty());
    assert_eq!(launched(&mut m), 0);
    clean(&mut m);
}

#[test]
fn a_close_after_the_made_and_before_the_start_leaves_it_unstarted() {
    for closer in [refused(CONNECTION, "connect"), common::revoked(CONNECTION)] {
        let (journal, mut m) = requesting();
        journal.services(&roles_made(CONNECTION), at(1));
        journal.services(&closer, at(1));
        for secs in [2, 3] {
            tick(&mut m, secs);
        }
        assert!(records(&journal).is_empty());
        assert_eq!(launched(&mut m), 0);
        clean(&mut m);
    }
}

#[test]
fn a_restarted_manager_folding_from_genesis_starts_the_exchange_once() {
    let (journal, mut m) = requesting();
    journal.services(&roles_made(CONNECTION), at(0));
    tick(&mut m, 1);
    assert_eq!(records(&journal), started());
    let mut m = restart(m);
    for secs in [2, 3] {
        tick(&mut m, secs);
    }
    assert_eq!(records(&journal), started());
    assert_eq!(launched(&mut m), 1);
    let fresh = Journal::new();
    let mut late = manager(&fresh, accounts(&[]));
    fresh.services(&requested(CONNECTION, ACCOUNT, Some("oauth")), at(0));
    fresh.services(&roles_made(CONNECTION), at(0));
    let mut late_restarted = {
        tick(&mut late, 1);
        restart(late)
    };
    tick(&mut late_restarted, 2);
    assert_eq!(records(&fresh), started());
    assert_eq!(launched(&mut late_restarted), 1);
    clean(&mut late_restarted);
}
