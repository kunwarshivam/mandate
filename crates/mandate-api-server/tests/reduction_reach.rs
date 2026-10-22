//! What a reduction-only session may act on (workspace API spec §4.10 route 2 item 6; identity
//! spec §4.2's rows for pause and the kill switch; E9-1 S2b-4a): the live agents when the member
//! may pause or kill switch one, the established connections when the member may kill switch one,
//! and empty lists, never a refusal, when the reads fail.

mod common;

use std::collections::BTreeSet;

use common::control::{Rows, T0, activation, member};
use common::{at, hosted};
use mandate_api_server::members::membership_records;
use mandate_api_server::opening::{Reach, reach};
use mandate_identity::{LookupFailed, PrincipalId, Role};
use serde_json::json;

const ALICE: PrincipalId = PrincipalId(7);
const STEP_UP: &str = r#"{"assertion_id":"01JAA3P4A000000000000000S1","authenticated_at":"2026-10-19T15:00:05.250000000Z","method":"passkey"}"#;

fn step_up() -> serde_json::Value {
    serde_json::from_str(STEP_UP).expect("JSON")
}

fn deployed(rows: &mut Rows, secs: i64, agent: &str, connection: &str) {
    rows.edited("AgentDeployed", secs, |e| {
        e["schema_version"] = json!(2);
        e["payload"] = json!({
            "agent_id": agent, "mandate_version": format!("sha256:{}", "ef".repeat(32)),
            "record_ref": format!("sha256:{}", "ab".repeat(32)), "connection_id": connection,
        });
    });
}

fn connection(rows: &mut Rows, kind: &str, version: u64, secs: i64, id: &str) {
    rows.edited(kind, secs, |e| {
        e["schema_version"] = json!(version);
        e["payload"] = json!({
            "connection_id": id, "broker": "alpaca", "environment": "paper",
            "scopes": ["account:write", "trading"], "account_ref": "01JB3K6F0C9R2V7N4M8Q1T5W60",
            "user": "usr_1", "step_up": step_up(), "margin_attestation": null,
            "reason": "owner", "occasion": "connect", "check": null,
            "existing_connection_id": null,
        });
    });
}

/// The hosted workspace with three connections (established, still connecting, revoked) and three
/// agents (two live, one stopped), and ALICE holding `roles`.
fn workspace(roles: &[Role]) -> Rows {
    let mut rows = Rows::opened();
    rows.hosted(
        "MemberActivated",
        T0 - 1_000,
        activation(
            ALICE,
            &BTreeSet::from_iter(roles.iter().copied()),
            T0 - 1_000,
        ),
    );
    connection(&mut rows, "ConnectionRequested", 1, T0 - 900, "conn_live");
    connection(&mut rows, "ConnectionEstablished", 2, T0 - 890, "conn_live");
    connection(
        &mut rows,
        "ConnectionRequested",
        1,
        T0 - 880,
        "conn_pending",
    );
    connection(&mut rows, "ConnectionRequested", 1, T0 - 870, "conn_gone");
    connection(&mut rows, "ConnectionEstablished", 2, T0 - 860, "conn_gone");
    connection(&mut rows, "ConnectionRevoked", 2, T0 - 850, "conn_gone");
    deployed(&mut rows, T0 - 800, "agt_one", "conn_live");
    deployed(&mut rows, T0 - 790, "agt_stopped", "conn_live");
    deployed(&mut rows, T0 - 780, "agt_two", "conn_live");
    rows.hosted("AgentStopped", T0 - 700, json!({"agent_id": "agt_stopped"}));
    rows
}

fn reach_of(rows: &Rows, member: PrincipalId) -> Reach {
    let records = membership_records(hosted().workspace, &rows.rows).map_err(|_| LookupFailed);
    reach(hosted(), &rows.rows, records, member, at(T0))
}

fn ids(list: &[&str]) -> Vec<String> {
    list.iter().map(|id| (*id).to_owned()).collect()
}

#[test]
fn an_operator_reaches_every_live_agent_and_established_connection() {
    for role in [Role::Operator, Role::WorkspaceAdmin] {
        assert_eq!(
            reach_of(&workspace(&[role]), ALICE),
            Reach {
                agent_ids: ids(&["agt_one", "agt_two"]),
                connection_ids: ids(&["conn_live"]),
            },
            "{role:?}: no stopped agent, no connection still connecting or revoked"
        );
    }
}

#[test]
fn an_approver_may_pause_every_live_agent_and_kill_switch_nothing() {
    assert_eq!(
        reach_of(&workspace(&[Role::Approver]), ALICE),
        Reach {
            agent_ids: ids(&["agt_one", "agt_two"]),
            connection_ids: Vec::new(),
        }
    );
}

#[test]
fn without_a_role_that_reaches_or_a_membership_or_a_read_the_lists_are_empty() {
    assert_eq!(
        reach_of(&workspace(&[Role::Viewer]), ALICE),
        Reach::default(),
        "a viewer may do neither"
    );
    assert_eq!(
        reach_of(&workspace(&[Role::Operator]), PrincipalId(99)),
        Reach::default(),
        "no membership"
    );
    let mut deactivated = workspace(&[Role::Operator]);
    deactivated.hosted("MemberDeactivated", T0 - 10, member(ALICE));
    assert_eq!(
        reach_of(&deactivated, ALICE),
        Reach::default(),
        "deactivated"
    );
    let rows = workspace(&[Role::Operator]);
    assert_eq!(
        reach(hosted(), &rows.rows, Err(LookupFailed), ALICE, at(T0)),
        Reach::default(),
        "memberships unread: empty, never a refusal"
    );
    let records = membership_records(hosted().workspace, &rows.rows).map_err(|_| LookupFailed);
    assert_eq!(
        reach(hosted(), &[], records, ALICE, at(T0)),
        Reach::default(),
        "the control stream unread"
    );
}
