//! DEC-655: a sensitive data API demands the permission its context was authorized for (identity
//! spec ID-8, E9-8). Each context comes from the real [`authorize`] over this file's own store;
//! every expected value is the fixture's, never read back from the context.

use std::collections::BTreeSet;
use std::marker::PhantomData;

use mandate_time::UtcNanos;

use super::matrix::session;
use super::rows::ROWS;
use super::{ByMember, O1, O2, W1, W3};
use crate::Permission as P;
use crate::demand::{Permitted, ReadRecords, RequiredPermission};
use crate::{
    Membership, MembershipState, OrgId, Principal, PrincipalId, PrincipalKind, Refusal, Role,
    Scope, SessionKind, Tenant, TenantContext, WorkspaceId, authorize,
};

/// W1's admin: §4.2 grants it both View agents and Read records.
const ADMIN: PrincipalId = PrincipalId(0x31);
/// W1's viewer: View agents only.
const VIEWER: PrincipalId = PrincipalId(0x32);
/// W1's operator and approver, a client of the operator's in W1, and a platform operator in a
/// break-glass window for W1.
const OPERATOR: PrincipalId = PrincipalId(0x34);
const APPROVER: PrincipalId = PrincipalId(0x35);
const CLIENT: PrincipalId = PrincipalId(0x36);
const PLATFORM: PrincipalId = PrincipalId(0x37);
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
        member(OPERATOR, W1, O1, Role::Operator),
        member(APPROVER, W1, O1, Role::Approver),
    ]
}

/// The workspace context `authorize` grants `who` for `p` in `workspace` of `org`, if any.
fn granted(who: &Principal, org: OrgId, workspace: WorkspaceId, p: P) -> Option<TenantContext> {
    let (store, scope) = (memberships(), Scope::Workspace { org, workspace });
    let full = session(SessionKind::Full, Vec::new());
    match authorize(&ByMember(&store), who, &full, scope, p, UtcNanos::EPOCH) {
        Ok(crate::Authorized::Workspace { tenant, .. }) => Some(tenant),
        _ => None,
    }
}

/// As [`granted`], where anything but a workspace context is a fixture error.
fn context(who: &Principal, org: OrgId, workspace: WorkspaceId, p: P) -> TenantContext {
    granted(who, org, workspace, p).unwrap_or_else(|| panic!("fixture: {who:?} holds {p:?}"))
}

fn service() -> Principal {
    Principal::ServiceAccount {
        id: SERVICE,
        workspaces: BTreeSet::from([W3]),
    }
}

/// Every workspace row of §4.2 but Read records, written from the spec. Out of scope, as they
/// yield an `OrgContext`, a `PrincipalContext`, or nothing, and so no `require`: Kill switch org
/// scope, Re-enable a halted scope (inactive), one's own passkey, notification addresses, and
/// memberships, Leave, Org policy, SSO, Create or archive a workspace, Org memberships, Service
/// accounts, Billing, and Transfer org ownership. The SA column holds only Read records.
#[rustfmt::skip]
const OTHER_ROWS: [P; 30] = [
    P::ViewAgents, P::OwnerRequest, P::DryRun, P::ChatThread, P::DraftVersion,
    P::ConfirmReducingOrNeutral, P::ConfirmRiskIncreasing, P::Deploy, P::Pause, P::HoldNewOpenings,
    P::LiftClientHold, P::ResumeOrStop, P::KillSwitchAgent, P::KillSwitchConnectionOrWorkspace,
    P::KillSwitchPrivileges, P::OwnerExit, P::Acknowledge, P::Approve, P::Skip,
    P::RemoveOrNarrowDelegation, P::BrokerConnection, P::AcceptDisclosure,
    P::WorkspacePolicyTighten, P::WorkspacePolicyLoosen, P::WorkspaceMembers, P::WorkspaceRoles,
    P::ConnectClient, P::RevokeClient, P::ApproveBreakGlass, P::BreakGlassOperational,
];

/// A principal of each column reaching W1 (WA, Op, Ap, Vi, a client of the operator's, and a
/// platform operator in a break-glass window); between them they hold every row above.
#[rustfmt::skip]
fn columns() -> [Principal; 6] {
    [Principal::User { id: ADMIN }, Principal::User { id: OPERATOR },
     Principal::User { id: APPROVER }, Principal::User { id: VIEWER },
     Principal::Client { id: CLIENT, on_behalf_of: OPERATOR, workspace: W1 },
     Principal::PlatformOperator { id: PLATFORM, window: Some(W1) }]
}

/// DEC-655 items 2 and 3: every workspace context `authorize` grants any column in W1, for any
/// row, is refused `forbidden` unless it is for Read records, which yields a witness; the rows
/// reached are exactly [`OTHER_ROWS`] and Read records, so no row goes unexercised.
#[test]
fn a_context_for_another_permission_cannot_read_records() {
    let seen: Vec<_> = ROWS
        .iter()
        .flat_map(|(_, p)| columns().map(|who| (*p, granted(&who, O1, W1, *p))))
        .filter_map(|(p, c)| c.map(|c| (p, c.require::<ReadRecords>().err())))
        .collect();
    let refused = |p: P| (p, (p != P::ReadRecords).then_some(Refusal::Forbidden));
    assert_eq!(
        seen,
        seen.iter().map(|(p, _)| refused(*p)).collect::<Vec<_>>()
    );
    let reached: BTreeSet<_> = seen.iter().map(|(p, _)| *p).collect();
    let listed = OTHER_ROWS.into_iter().chain([P::ReadRecords]).collect();
    assert_eq!(reached, listed, "the rows exercised are the rows listed");
}

/// DEC-655 items 1 and 2: the witness is a `Tenant` for the very context `require` was called on:
/// its workspace, organization, principal, and kind are the fixture's, across two workspaces of two
/// organizations and two principal kinds, and a context for another permission yields none.
#[test]
fn require_returns_the_context_it_was_called_on() {
    let contexts = [
        context(&Principal::User { id: ADMIN }, O1, W1, P::ReadRecords),
        context(&service(), O2, W3, P::ReadRecords),
        context(&Principal::User { id: VIEWER }, O1, W1, P::ViewAgents),
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
    let reading = context(&Principal::User { id: ADMIN }, O1, W1, P::ReadRecords);
    let witness: Permitted<'_, ReadRecords> = Permitted {
        context: &reading,
        demanded: PhantomData,
    };
    assert!(std::ptr::eq(witness.context, &reading));
    assert_eq!(ReadRecords::PERMISSION, P::ReadRecords);
}
