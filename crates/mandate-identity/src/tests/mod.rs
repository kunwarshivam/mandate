//! The crate's tests (E9-2). They sit in the crate because `Session`, `Membership`, and
//! `MembershipLookup` are sealed to it (DEC-642), so the test doubles below are `cfg(test)` only.

mod demand;
mod matrix;
mod roles;
mod rows;
mod wire;

use std::collections::BTreeSet;

use mandate_identity_seal::LookupSeal;

use crate::{
    LookupFailed, Membership, MembershipLookup, MembershipQuery, OrgContext, OrgId, Permission,
    PrincipalContext, PrincipalId, PrincipalKind, Scope, SessionRef, Tenant, TenantContext,
    WorkspaceId,
};

pub(crate) const O1: OrgId = OrgId(0x11);
pub(crate) const O2: OrgId = OrgId(0x12);
pub(crate) const W1: WorkspaceId = WorkspaceId(0x21);
pub(crate) const W2: WorkspaceId = WorkspaceId(0x22);
pub(crate) const W3: WorkspaceId = WorkspaceId(0x23);

/// The workspaces the tests' deployment hosts, each with the organization its record names: O1
/// holds W1 and W2, O2 holds W3.
pub(crate) const HOSTED: [(WorkspaceId, OrgId); 3] = [(W1, O1), (W2, O1), (W3, O2)];

/// The organization's workspaces, from [`HOSTED`].
pub(crate) fn hosted_in(org: OrgId) -> BTreeSet<WorkspaceId> {
    HOSTED
        .iter()
        .filter(|(_, o)| *o == org)
        .map(|(w, _)| *w)
        .collect()
}

/// The organization a workspace's record names, from [`HOSTED`].
pub(crate) fn record_of(workspace: WorkspaceId) -> Option<OrgId> {
    HOSTED
        .iter()
        .find(|(w, _)| *w == workspace)
        .map(|(_, o)| *o)
}

/// Whether the store holds the scope's pair; the principal's and an organization's scopes always
/// are.
pub(crate) fn paired(scope: Scope) -> bool {
    match scope {
        Scope::Workspace { org, workspace } => record_of(workspace) == Some(org),
        Scope::Principal | Scope::Org(_) => true,
    }
}

/// A store that answers the query it is asked: the named member's memberships in every scope (or
/// every membership for `None`), so the step must still pick the scope.
pub(crate) struct ByMember<'a>(pub(crate) &'a [Membership]);

impl LookupSeal for ByMember<'_> {}

impl MembershipLookup for ByMember<'_> {
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Ok(self
            .0
            .iter()
            .filter(|m| query.member.is_none_or(|member| m.member == member))
            .cloned()
            .collect())
    }

    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Ok(hosted_in(org))
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok(record_of(workspace))
    }
}

/// A careless store that returns every membership whatever the query, so the step must pick the
/// member too.
pub(crate) struct Everything<'a>(pub(crate) &'a [Membership]);

impl LookupSeal for Everything<'_> {}

impl MembershipLookup for Everything<'_> {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Ok(self.0.to_vec())
    }

    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Ok(hosted_in(org))
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok(record_of(workspace))
    }
}

/// A membership store that cannot answer, so neither memberships nor an organization's workspaces
/// can be read; each workspace's own record still can (identity spec §4.5, DEC-832 item 4).
pub(crate) struct Failing;

impl LookupSeal for Failing {}

impl MembershipLookup for Failing {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Err(LookupFailed)
    }

    fn workspaces(&self, _: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Err(LookupFailed)
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok(record_of(workspace))
    }
}

/// A store that cannot answer at all: neither memberships, nor an organization's workspaces, nor a
/// workspace's own record, so no pair can be checked (identity spec §4.5).
pub(crate) struct Unreadable;

impl LookupSeal for Unreadable {}

impl MembershipLookup for Unreadable {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Err(LookupFailed)
    }

    fn workspaces(&self, _: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Err(LookupFailed)
    }

    fn workspace_org(&self, _: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Err(LookupFailed)
    }
}

/// The control for the first `compile_fail` doctest: the same literal compiles inside the
/// crate, so outside it fails on the private fields alone (rustdoc does not check error
/// codes on stable).
#[test]
fn the_doctests_literal_builds_inside_the_crate() {
    let context = TenantContext {
        org: OrgId(1),
        workspace: WorkspaceId(2),
        principal: PrincipalId(3),
        kind: PrincipalKind::User,
        session: SessionRef(4),
        permission: Permission::ViewAgents,
        membership_unverified: true,
    };
    assert_eq!(
        (
            Tenant::org(&context),
            Tenant::workspace(&context),
            Tenant::principal(&context),
            Tenant::kind(&context),
        ),
        (
            OrgId(1),
            WorkspaceId(2),
            PrincipalId(3),
            PrincipalKind::User
        )
    );
    assert_eq!(
        (
            context.org(),
            context.workspace(),
            context.principal(),
            context.kind(),
            context.session(),
            context.permission(),
            context.membership_unverified()
        ),
        (
            OrgId(1),
            WorkspaceId(2),
            PrincipalId(3),
            PrincipalKind::User,
            SessionRef(4),
            Permission::ViewAgents,
            true
        )
    );
    let verified = TenantContext {
        membership_unverified: false,
        ..context
    };
    assert!(!verified.membership_unverified());
}

/// The sealed constructors build exactly the value their fields name.
#[test]
fn the_sealed_constructors_build_what_they_are_given() {
    use crate::{Membership, MembershipState, Role, Scope, Session, SessionKind};
    use mandate_identity_seal::Seal;
    use mandate_time::UtcNanos;
    use std::collections::BTreeMap;
    let scope = Scope::Org(OrgId(1));
    let from = UtcNanos::from_parts(1_790_000_000, 7).unwrap();
    let roles = BTreeMap::from([(Role::OrgOwner, UtcNanos::EPOCH), (Role::OrgAdmin, from)]);
    let built = Membership::new(
        Seal::grant(),
        PrincipalId(2),
        scope,
        MembershipState::CoolingOff,
        roles.clone(),
    );
    let literal = Membership {
        member: PrincipalId(2),
        scope,
        state: MembershipState::CoolingOff,
        roles,
    };
    assert_eq!((built.member(), built.scope()), (PrincipalId(2), scope));
    assert_eq!(built, literal);
    let session = Session::new(
        Seal::grant(),
        SessionRef(3),
        SessionKind::ReductionOnly,
        vec![literal.clone()],
    );
    let expected = Session {
        reference: SessionRef(3),
        kind: SessionKind::ReductionOnly,
        snapshot: vec![literal],
    };
    assert_eq!(session, expected);
}

/// The control for the `OrgContext` and `PrincipalContext` `compile_fail` doctests: the same
/// literals compile inside the crate. Each accessor is read on two literals with opposite values,
/// so no constant answer passes.
#[test]
fn the_org_and_principal_doctests_literals_build_inside_the_crate() {
    let org = OrgContext {
        org: OrgId(1),
        principal: PrincipalId(3),
        kind: PrincipalKind::User,
        session: SessionRef(4),
        permission: Permission::KillSwitchOrg,
        membership_unverified: true,
        workspaces: Some(BTreeSet::from([W1, W2])),
    };
    assert_eq!(
        (
            org.org(),
            org.principal(),
            org.kind(),
            org.session(),
            org.permission(),
            org.membership_unverified(),
            org.workspaces()
        ),
        (
            OrgId(1),
            PrincipalId(3),
            PrincipalKind::User,
            SessionRef(4),
            Permission::KillSwitchOrg,
            true,
            Some(&BTreeSet::from([W1, W2]))
        )
    );
    let outage = OrgContext {
        membership_unverified: false,
        workspaces: None,
        ..org
    };
    assert_eq!(
        (outage.membership_unverified(), outage.workspaces()),
        (false, None)
    );
    let empty = OrgContext {
        workspaces: Some(BTreeSet::new()),
        ..outage
    };
    assert_eq!(empty.workspaces(), Some(&BTreeSet::new()));
    let own = PrincipalContext {
        principal: PrincipalId(3),
        kind: PrincipalKind::User,
        session: SessionRef(4),
        permission: Permission::OwnPasskey,
        scope: Scope::Principal,
        membership_unverified: true,
    };
    assert_eq!(
        (
            own.principal(),
            own.kind(),
            own.session(),
            own.permission(),
            own.scope(),
            own.membership_unverified()
        ),
        (
            PrincipalId(3),
            PrincipalKind::User,
            SessionRef(4),
            Permission::OwnPasskey,
            Scope::Principal,
            true
        )
    );
    let verified = PrincipalContext {
        membership_unverified: false,
        ..own
    };
    assert!(!verified.membership_unverified());
}
