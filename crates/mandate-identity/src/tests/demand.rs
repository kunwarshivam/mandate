//! DEC-655: a sensitive data API demands the permission its context was authorized for (identity
//! spec ID-8, E9-8). Each context comes from the real [`authorize`] over this file's own store;
//! every expected value is the fixture's, never read back from the context.

use std::collections::BTreeSet;
use std::marker::PhantomData;

use mandate_time::UtcNanos;

use super::rows::ROWS;
use super::{ByMember, O1, O2, W1, W3};
use crate::demand::{Authorized, ReadRecords, RequiredPermission};
use crate::{
    Membership, MembershipState, OrgId, Permission, Principal, PrincipalId, PrincipalKind, Refusal,
    Role, Scope, Session, SessionKind, SessionRef, Tenant, TenantContext, WorkspaceId, authorize,
};

/// W1's admin: §4.2 grants it both View agents and Read records.
const ADMIN: PrincipalId = PrincipalId(0x31);
/// W1's viewer: View agents only.
const VIEWER: PrincipalId = PrincipalId(0x32);
/// A service account issued for W3, whose column grants Read records.
const SERVICE: PrincipalId = PrincipalId(0x33);

fn memberships() -> Vec<Membership> {
    let member = |member, workspace, org, role| Membership {
        member,
        scope: Scope::Workspace { org, workspace },
        state: MembershipState::Active,
        roles: [(role, UtcNanos::EPOCH)].into(),
    };
    vec![
        member(ADMIN, W1, O1, Role::WorkspaceAdmin),
        member(VIEWER, W1, O1, Role::Viewer),
    ]
}

/// The workspace context `authorize` grants `principal` for `permission` in `workspace` of `org`,
/// or `None` when it grants none.
fn granted(
    principal: &Principal,
    org: OrgId,
    workspace: WorkspaceId,
    permission: Permission,
) -> Option<TenantContext> {
    let store = memberships();
    let session = Session {
        reference: SessionRef(0x41),
        kind: SessionKind::Full,
        snapshot: Vec::new(),
    };
    let scope = Scope::Workspace { org, workspace };
    match authorize(
        &ByMember(&store),
        principal,
        &session,
        scope,
        permission,
        UtcNanos::EPOCH,
    ) {
        Ok(crate::Authorized::Workspace { tenant, .. }) => Some(tenant),
        _ => None,
    }
}

/// As [`granted`], where anything but a workspace context is a fixture error.
fn context(
    principal: &Principal,
    org: OrgId,
    workspace: WorkspaceId,
    permission: Permission,
) -> TenantContext {
    granted(principal, org, workspace, permission)
        .unwrap_or_else(|| panic!("fixture: {principal:?} holds {permission:?} in {workspace:?}"))
}

fn user(id: PrincipalId) -> Principal {
    Principal::User { id }
}

fn service() -> Principal {
    Principal::ServiceAccount {
        id: SERVICE,
        workspaces: BTreeSet::from([W3]),
    }
}

/// DEC-655 items 2 and 3: a context authorized for any other row of §4.2, whether its principal
/// could also read records or not, yields no Read records witness; it is refused `forbidden`. The
/// admin's Read records context yields one.
#[test]
#[ignore = "pending E9-8"]
fn a_context_for_another_permission_cannot_read_records() {
    let mut others: Vec<(Permission, TenantContext)> = ROWS
        .iter()
        .filter(|(_, p)| *p != Permission::ReadRecords)
        .filter_map(|(_, p)| granted(&user(ADMIN), O1, W1, *p).map(|c| (*p, c)))
        .collect();
    assert!(
        others.len() >= 10,
        "fixture: W1's admin holds {} other workspace rows",
        others.len()
    );
    let viewer = context(&user(VIEWER), O1, W1, Permission::ViewAgents);
    others.push((Permission::ViewAgents, viewer));
    let seen: Vec<_> = others
        .iter()
        .map(|(p, c)| (*p, c.require::<ReadRecords>().err()))
        .collect();
    let refused: Vec<_> = others
        .iter()
        .map(|(p, _)| (*p, Some(Refusal::Forbidden)))
        .collect();
    assert_eq!(seen, refused);
    let reading = context(&user(ADMIN), O1, W1, Permission::ReadRecords);
    assert_eq!(
        reading.require::<ReadRecords>().map(|_| ()),
        Ok(()),
        "a Read records context yields its witness"
    );
}

/// DEC-655 items 1 and 2: the witness is a `Tenant` for the very context `require` was called on:
/// its workspace, organization, principal, and kind are the fixture's, across two workspaces of two
/// organizations and two principal kinds, and a context for another permission yields none.
#[test]
#[ignore = "pending E9-8"]
fn require_returns_the_context_it_was_called_on() {
    let contexts = [
        context(&user(ADMIN), O1, W1, Permission::ReadRecords),
        context(&service(), O2, W3, Permission::ReadRecords),
        context(&user(VIEWER), O1, W1, Permission::ViewAgents),
    ];
    let seen: Vec<_> = contexts
        .iter()
        .map(|c| {
            c.require::<ReadRecords>()
                .map(|w| (w.workspace(), w.org(), w.principal(), w.kind()))
        })
        .collect();
    assert_eq!(
        seen,
        vec![
            Ok((W1, O1, ADMIN, PrincipalKind::User)),
            Ok((W3, O2, SERVICE, PrincipalKind::ServiceAccount)),
            Err(Refusal::Forbidden),
        ]
    );
}

/// The control for the witness's `compile_fail` doctests: the same literal compiles inside the
/// crate, so outside it fails on the private fields alone. The marker names Read records.
#[test]
fn the_witness_literal_builds_inside_the_crate() {
    let reading = context(&user(ADMIN), O1, W1, Permission::ReadRecords);
    let witness: Authorized<'_, ReadRecords> = Authorized {
        context: &reading,
        demanded: PhantomData,
    };
    assert!(std::ptr::eq(witness.context, &reading));
    assert_eq!(ReadRecords::PERMISSION, Permission::ReadRecords);
}
