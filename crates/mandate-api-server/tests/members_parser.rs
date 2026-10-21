//! The hosted control stream's rows as membership records (E21-55, DEC-2110): each row's envelope
//! is read, the workspace's `Member*` records map through `mandate_identity::control_entry`
//! (DEC-991 item 4), and the lookup folds them at its own instant (identity spec ID-3, §5; journal
//! spec §9.12; workspace API spec §3.11 item 8).

mod common;

use std::collections::BTreeMap;

use common::control::{
    OTHER_WORKSPACE, Rows, T0, activated, activation, change, hosted_stream, member, membership,
    other_stream, query, reactivation, record, roles, scope,
};
use common::{at, hosted};
use mandate_api_server::members::{ControlLookup, RowsRefused, control_parser, membership_records};
use mandate_identity::{
    LookupFailed, MembershipEvent, MembershipState, Permission, Principal, PrincipalId, Refusal,
    Role, Session, SessionKind, SessionRef, WorkspaceId, authorize,
};
use mandate_identity_seal::Seal;
use serde_json::json;

const ALICE: PrincipalId = PrincipalId(7);
const BOB: PrincipalId = PrincipalId(8);

#[test]
fn grants_revokes_and_role_changes_fold_in_stream_order() {
    let mut rows = Rows::opened();
    let owner = [Role::Operator, Role::WorkspaceAdmin];
    let added = BTreeMap::from([(Role::Approver, T0 - 40)]);
    let removed = roles(&[Role::WorkspaceAdmin]);
    let viewer = roles(&[Role::Viewer]);
    rows.hosted(
        "MemberActivated",
        T0 - 100,
        activation(ALICE, &roles(&owner), T0 - 50),
    );
    rows.hosted(
        "MemberRoleChanged",
        T0 - 90,
        change(ALICE, &added, &removed),
    );
    rows.hosted(
        "MemberActivated",
        T0 - 80,
        activation(BOB, &viewer, T0 - 80),
    );
    rows.hosted("MemberDeactivated", T0 - 70, member(BOB));
    rows.hosted(
        "MemberReactivated",
        T0 - 60,
        reactivation(BOB, &viewer, T0 - 30),
    );
    let changed = MembershipEvent::RoleChanged {
        member: ALICE,
        added: BTreeMap::from([(Role::Approver, at(T0 - 40))]),
        removed,
        independent_approval_required: false,
    };
    let reactivated = MembershipEvent::Reactivated {
        member: BOB,
        roles: viewer,
        independent_approval_required: false,
        cool_off_ends_at: at(T0 - 30),
    };
    assert_eq!(
        rows.parsed(),
        Ok(vec![
            activated(2, T0 - 100, ALICE, &owner, T0 - 50),
            record(3, T0 - 90, changed),
            activated(4, T0 - 80, BOB, &[Role::Viewer], T0 - 80),
            record(5, T0 - 70, MembershipEvent::Deactivated { member: BOB }),
            record(6, T0 - 60, reactivated),
        ]),
        "every Member* record, in seq order, and nothing else"
    );
    let lookup = rows.lookup(T0);
    let alice = membership(
        ALICE,
        MembershipState::Active,
        &[Role::Approver, Role::Operator],
        T0,
    );
    assert_eq!(
        query(&lookup, ALICE),
        Ok(vec![alice]),
        "workspace_admin removed, approver added"
    );
    let bob = membership(BOB, MembershipState::Active, &[Role::Viewer], T0);
    assert_eq!(
        query(&lookup, BOB),
        Ok(vec![bob]),
        "the reactivation follows the deactivation"
    );
}

#[test]
fn rows_of_other_workspaces_environments_and_types_are_ignored() {
    let mut rows = Rows::opened();
    let admin = roles(&[Role::WorkspaceAdmin]);
    rows.hosted(
        "MemberActivated",
        T0 - 100,
        activation(ALICE, &roles(&[Role::Viewer]), T0 - 100),
    );
    rows.push(
        &other_stream(),
        "paper",
        "MemberActivated",
        T0 - 95,
        activation(BOB, &admin, T0 - 95),
    );
    rows.push(
        &other_stream(),
        "paper",
        "MemberDeactivated",
        T0 - 94,
        member(ALICE),
    );
    rows.push(
        &hosted_stream(),
        "live",
        "PolicyChanged",
        T0 - 93,
        json!({"policy": "x"}),
    );
    rows.hosted("AgentDeployed", T0 - 92, json!({"agent_id": "a"}));
    assert_eq!(
        rows.parsed(),
        Ok(vec![activated(
            2,
            T0 - 100,
            ALICE,
            &[Role::Viewer],
            T0 - 100
        )]),
        "another workspace's grant and revoke, and the stream's other records, are no memberships"
    );
    let lookup = rows.lookup(T0);
    let alice = membership(ALICE, MembershipState::Active, &[Role::Viewer], T0);
    assert_eq!(query(&lookup, ALICE), Ok(vec![alice]));
    assert_eq!(
        query(&lookup, BOB),
        Ok(vec![]),
        "another workspace's member is none here"
    );
    assert_eq!(
        membership_records(hosted().workspace, &[]),
        Ok(vec![]),
        "an empty stream"
    );
    assert_eq!(Rows::opened().parsed(), Ok(vec![]), "a stream only opened");
}

/// A stream where alice is granted operator, then `last` appends the row at seq 3.
fn granted_then(last: impl FnOnce(&mut Rows)) -> Rows {
    let mut rows = Rows::opened();
    rows.hosted(
        "MemberActivated",
        T0 - 100,
        activation(ALICE, &roles(&[Role::Operator]), T0 - 100),
    );
    last(&mut rows);
    rows
}

#[test]
fn an_unreadable_membership_row_fails_the_whole_lookup_and_is_never_skipped() {
    let unread = granted_then(|rows| {
        rows.edited("MemberDeactivated", T0 - 50, |e| {
            e["payload"] = json!({"session_ref": null})
        });
    });
    assert_eq!(unread.parsed(), Err(RowsRefused::Unreadable { seq: 3 }));
    assert_eq!(
        query(&unread.failed(T0), ALICE),
        Err(LookupFailed),
        "never alice, still active"
    );
    for bad in [
        json!(["viewer", "approver"]),
        json!(["org_owner"]),
        json!("viewer"),
    ] {
        let unsorted = granted_then(|rows| {
            let mut payload = activation(BOB, &roles(&[Role::Viewer]), T0 - 50);
            payload["roles"] = bad;
            rows.hosted("MemberActivated", T0 - 50, payload);
        });
        assert_eq!(unsorted.parsed(), Err(RowsRefused::Unreadable { seq: 3 }));
    }
    let unknown_version = granted_then(|rows| {
        rows.edited("MemberDeactivated", T0 - 50, |e| {
            e["payload"] = member(ALICE);
            e["schema_version"] = json!(2);
        });
    });
    assert_eq!(
        unknown_version.parsed(),
        Err(RowsRefused::UnknownVersion { seq: 3 })
    );
    assert_eq!(query(&unknown_version.failed(T0), ALICE), Err(LookupFailed));
    let elsewhere = granted_then(|rows| {
        rows.push(
            &hosted_stream(),
            "live",
            "MemberDeactivated",
            T0 - 50,
            member(ALICE),
        );
    });
    assert_eq!(
        elsewhere.parsed(),
        Err(RowsRefused::OtherEnvironment { seq: 3 }),
        "a revoke in the workspace's own stream is never passed over"
    );
    assert_eq!(query(&elsewhere.failed(T0), ALICE), Err(LookupFailed));
    let undated = granted_then(|rows| {
        rows.edited("AgentDeployed", T0 - 50, |e| {
            e["event_time"] = json!("yesterday")
        });
    });
    assert_eq!(
        undated.parsed(),
        Err(RowsRefused::Unreadable { seq: 3 }),
        "every row is read"
    );
    let mut garbled = granted_then(|_| {});
    let mut row = garbled.rows.last().cloned().expect("a row");
    row.seq = 3;
    row.body = b"{\"event_type\":".to_vec();
    garbled.rows.push(row);
    assert_eq!(garbled.parsed(), Err(RowsRefused::Unreadable { seq: 3 }));
    let mut unopened = granted_then(|_| {});
    unopened.rows.remove(0);
    assert_eq!(
        unopened.parsed(),
        Err(RowsRefused::NotOpened),
        "the stream names no environment"
    );
    let mut first = Rows { rows: Vec::new() };
    first.hosted("AgentDeployed", T0 - 100, json!({"agent_id": "a"}));
    assert_eq!(
        first.parsed(),
        Err(RowsRefused::NotOpened),
        "seq 1 is no StreamOpened"
    );
    let mut late = Rows { rows: Vec::new() };
    late.push(
        &other_stream(),
        "paper",
        "StreamOpened",
        T0 - 100,
        json!({}),
    );
    late.hosted("StreamOpened", T0 - 100, json!({}));
    assert_eq!(
        late.parsed(),
        Err(RowsRefused::NotOpened),
        "a StreamOpened at seq 2 opens nothing"
    );
}

/// Whether `principal` may view the hosted workspace's agents through `lookup` at `now`.
fn authorized(lookup: &ControlLookup, principal: PrincipalId, now: i64) -> Result<(), Refusal> {
    let roles = lookup.snapshot(principal);
    let session = Session::new(Seal::grant(), SessionRef(99), SessionKind::Full, roles);
    let user = Principal::User { id: principal };
    authorize(
        lookup,
        &user,
        &session,
        scope(),
        Permission::ViewAgents,
        at(now),
    )
    .map(|_| ())
}

#[test]
fn a_revoked_member_is_refused() {
    let mut rows = Rows::opened();
    let viewer = roles(&[Role::Viewer]);
    rows.hosted(
        "MemberActivated",
        T0 - 100,
        activation(ALICE, &viewer, T0 - 100),
    );
    rows.hosted(
        "MemberActivated",
        T0 - 90,
        activation(BOB, &viewer, T0 - 90),
    );
    rows.hosted("MemberDeactivated", T0 - 80, member(ALICE));
    rows.hosted("MemberDeactivated", T0 - 70, member(BOB));
    rows.hosted("MemberRemoved", T0 - 60, member(BOB));
    let lookup = rows.lookup(T0);
    let alice = membership(ALICE, MembershipState::Deactivated, &[], T0);
    assert_eq!(query(&lookup, ALICE), Ok(vec![alice]));
    let bob = membership(BOB, MembershipState::Removed, &[], T0);
    assert_eq!(query(&lookup, BOB), Ok(vec![bob]));
    assert!(
        authorized(&lookup, ALICE, T0).is_err(),
        "a deactivated member reads nothing"
    );
    assert!(
        authorized(&lookup, BOB, T0).is_err(),
        "a removed member reads nothing"
    );
    let before = rows.lookup(T0 - 85);
    assert_eq!(
        authorized(&before, ALICE, T0 - 85),
        Ok(()),
        "before the revoke, alice reads"
    );
}

#[test]
fn the_time_base_is_the_lookups_now() {
    let mut rows = Rows::opened();
    let granted = [Role::Operator, Role::Viewer];
    rows.hosted(
        "MemberActivated",
        T0 - 100,
        activation(ALICE, &roles(&granted), T0 + 100),
    );
    rows.hosted("MemberDeactivated", T0 + 200, member(ALICE));
    let at_now = |now| query(&rows.lookup(now), ALICE);
    let cooling = membership(ALICE, MembershipState::CoolingOff, &[Role::Viewer], T0);
    assert_eq!(at_now(T0 - 101), Ok(vec![]), "not yet granted");
    assert_eq!(
        at_now(T0),
        Ok(vec![cooling]),
        "operator waits for its cool-off, not recorded_at"
    );
    let active = |now| membership(ALICE, MembershipState::Active, &granted, now);
    assert_eq!(at_now(T0 + 100), Ok(vec![active(T0 + 100)]));
    assert_eq!(
        at_now(T0 + 199),
        Ok(vec![active(T0 + 199)]),
        "a later revoke is not yet folded"
    );
    let revoked = membership(ALICE, MembershipState::Deactivated, &[], T0 + 200);
    assert_eq!(at_now(T0 + 200), Ok(vec![revoked]));
}

#[test]
fn a_membership_row_of_another_environment_fails_the_lookup_closed() {
    let viewer = roles(&[Role::Viewer]);
    let revoke = granted_then(|rows| {
        rows.push(
            &hosted_stream(),
            "live",
            "MemberDeactivated",
            T0 - 50,
            member(ALICE),
        );
    });
    let grant = granted_then(|rows| {
        let payload = activation(BOB, &viewer, T0 - 50);
        rows.push(
            &hosted_stream(),
            "live",
            "MemberActivated",
            T0 - 50,
            payload,
        );
    });
    for (rows, what) in [(revoke, "a revoke"), (grant, "a grant")] {
        let refused = Err(RowsRefused::OtherEnvironment { seq: 3 });
        assert_eq!(
            rows.parsed(),
            refused,
            "{what} in live on a paper stream (DEC-2110 item 5)"
        );
        assert_eq!(query(&rows.failed(T0), ALICE), Err(LookupFailed));
        assert_eq!(query(&rows.failed(T0), BOB), Err(LookupFailed));
    }
}

#[test]
fn the_deployments_parser_answers_every_refusal_as_a_failed_lookup() {
    let parse = control_parser(hosted().workspace);
    let granted = granted_then(|_| {});
    let records = vec![activated(2, T0 - 100, ALICE, &[Role::Operator], T0 - 100)];
    let answered = granted.parsed();
    assert_eq!(
        parse(&granted.rows),
        Ok(records),
        "membership_records answers {answered:?}"
    );
    let elsewhere = control_parser(WorkspaceId::from_ulid_text(OTHER_WORKSPACE).expect("a ULID"));
    assert_eq!(
        elsewhere(&granted.rows),
        Ok(vec![]),
        "another workspace's parser reads none"
    );
    let unreadable = granted_then(|rows| {
        rows.edited("MemberRemoved", T0 - 50, |e| e["payload"] = json!({}));
    });
    let live = granted_then(|rows| {
        rows.push(
            &hosted_stream(),
            "live",
            "MemberDeactivated",
            T0 - 50,
            member(ALICE),
        );
    });
    let mut unopened = granted_then(|_| {});
    unopened.rows.remove(0);
    for refused in [unreadable, live, unopened] {
        assert_eq!(parse(&refused.rows), Err(LookupFailed), "DEC-2110 item 7");
    }
}

#[test]
fn only_a_membership_record_of_the_workspace_must_be_at_version_1() {
    let passed_over = granted_then(|rows| {
        rows.edited("AgentDeployed", T0 - 60, |e| e["schema_version"] = json!(7));
        rows.edited("MemberDeactivated", T0 - 50, |e| {
            e["stream_id"] = json!(other_stream());
            e["payload"] = member(ALICE);
            e["schema_version"] = json!(2);
        });
    });
    assert_eq!(
        passed_over.parsed(),
        Ok(vec![activated(
            2,
            T0 - 100,
            ALICE,
            &[Role::Operator],
            T0 - 100
        )]),
        "a later version of another record, or of another workspace's revoke, says nothing here"
    );
    let live_unknown = granted_then(|rows| {
        rows.edited("MemberDeactivated", T0 - 50, |e| {
            e["environment"] = json!("live");
            e["payload"] = member(ALICE);
            e["schema_version"] = json!(2);
        });
    });
    assert!(
        live_unknown.parsed().is_err(),
        "it is still the workspace's own revoke"
    );
    assert_eq!(query(&live_unknown.failed(T0), ALICE), Err(LookupFailed));
}
