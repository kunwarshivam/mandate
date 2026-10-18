//! The membership lookup over the control stream (§3.11 item 8, DEC-978 item 8), directly.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::hosted;
use mandate_api_server::members::ControlLookup;
use mandate_identity::{
    LookupFailed, Membership, MembershipEvent, MembershipLookup, MembershipQuery, MembershipRecord,
    MembershipState, OrgId, PrincipalId, Role, Scope, WorkspaceId,
};
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;

const T0: i64 = 1_791_000_000;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn alice() -> PrincipalId {
    PrincipalId(7)
}

fn founding(roles: &[Role]) -> MembershipRecord {
    MembershipRecord::new(
        Seal::grant(),
        2,
        at(T0 - 100),
        MembershipEvent::Activated {
            member: alice(),
            invitation: None,
            roles: roles.iter().copied().collect(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0 - 100),
        },
    )
}

fn scope() -> Scope {
    Scope::Workspace {
        org: hosted().org,
        workspace: hosted().workspace,
    }
}

#[test]
fn a_member_reads_as_its_state_and_effective_roles() {
    let lookup = ControlLookup::new(
        hosted(),
        Ok(vec![founding(&[Role::Operator, Role::Approver])]),
        at(T0),
    );
    let expected = Membership::new(
        Seal::grant(),
        alice(),
        scope(),
        MembershipState::Active,
        BTreeMap::from([(Role::Operator, at(T0)), (Role::Approver, at(T0))]),
    );
    assert_eq!(lookup.snapshot(alice()), vec![expected.clone()]);
    assert_eq!(
        lookup.memberships(&MembershipQuery {
            member: Some(alice()),
            scope: scope()
        }),
        Ok(vec![expected])
    );
    assert_eq!(lookup.snapshot(PrincipalId(8)), vec![]);
    let elsewhere = Scope::Workspace {
        org: hosted().org,
        workspace: WorkspaceId(1),
    };
    assert_eq!(
        lookup.memberships(&MembershipQuery {
            member: Some(alice()),
            scope: elsewhere
        }),
        Ok(vec![])
    );
}

#[test]
fn every_member_or_an_unreadable_store_fails_the_lookup() {
    let lookup = ControlLookup::new(hosted(), Ok(vec![founding(&[Role::Operator])]), at(T0));
    assert_eq!(
        lookup.memberships(&MembershipQuery {
            member: None,
            scope: scope()
        }),
        Err(LookupFailed)
    );
    let failed = ControlLookup::new(hosted(), Err(LookupFailed), at(T0));
    assert_eq!(failed.snapshot(alice()), vec![]);
    assert_eq!(failed.workspaces(hosted().org), Err(LookupFailed));
    assert_eq!(
        failed.workspace_org(hosted().workspace),
        Ok(Some(hosted().org)),
        "the hosted workspace's organization is a setting, not a store read"
    );
}

#[test]
fn only_the_hosted_workspace_belongs_to_the_hosted_organization() {
    let lookup = ControlLookup::new(hosted(), Ok(vec![]), at(T0));
    assert_eq!(
        lookup.workspaces(hosted().org),
        Ok(BTreeSet::from([hosted().workspace]))
    );
    assert_eq!(lookup.workspaces(OrgId(5)), Ok(BTreeSet::new()));
    assert_eq!(
        lookup.workspace_org(hosted().workspace),
        Ok(Some(hosted().org))
    );
    assert_eq!(lookup.workspace_org(WorkspaceId(1)), Ok(None));
}

#[test]
fn a_role_still_cooling_off_is_not_effective() {
    let cooling = MembershipRecord::new(
        Seal::grant(),
        2,
        at(T0 - 100),
        MembershipEvent::Activated {
            member: alice(),
            invitation: None,
            roles: [Role::Operator].into_iter().collect(),
            independent_approval_required: false,
            cool_off_ends_at: at(T0 + 100),
        },
    );
    let lookup = ControlLookup::new(hosted(), Ok(vec![cooling]), at(T0));
    let snapshot = lookup.snapshot(alice());
    assert_eq!(snapshot.len(), 1);
    assert_eq!(
        snapshot[0],
        Membership::new(
            Seal::grant(),
            alice(),
            scope(),
            MembershipState::CoolingOff,
            BTreeMap::new()
        )
    );
}
