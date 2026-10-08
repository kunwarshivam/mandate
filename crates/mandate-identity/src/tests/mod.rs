//! The crate's tests (E9-2). They sit in the crate because `Session`, `Membership`, and
//! `MembershipLookup` are sealed to it (DEC-642), so the test doubles below are `cfg(test)` only.

mod matrix;
mod rows;
mod wire;

use std::collections::BTreeSet;

use crate::{
    LookupFailed, Membership, MembershipLookup, MembershipQuery, OrgContext, OrgId, Permission,
    PrincipalContext, PrincipalId, PrincipalKind, Scope, SessionRef, Tenant, TenantContext,
    WorkspaceId, sealed,
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

impl sealed::Sealed for ByMember<'_> {}

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

impl sealed::Sealed for Everything<'_> {}

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

impl sealed::Sealed for Failing {}

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

/// The control for the `OrgContext` and `PrincipalContext` `compile_fail` doctests: the same
/// literals compile inside the crate.
#[test]
fn the_org_and_principal_doctests_literals_build_inside_the_crate() {
    let org = OrgContext {
        org: OrgId(1),
        principal: PrincipalId(3),
        kind: PrincipalKind::User,
        session: SessionRef(4),
        permission: Permission::KillSwitchOrg,
        membership_unverified: false,
        workspaces: Some(BTreeSet::new()),
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
            false,
            Some(&BTreeSet::new())
        )
    );
    let own = PrincipalContext {
        principal: PrincipalId(3),
        kind: PrincipalKind::User,
        session: SessionRef(4),
        permission: Permission::OwnPasskey,
        scope: Scope::Principal,
        membership_unverified: false,
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
            false
        )
    );
}
