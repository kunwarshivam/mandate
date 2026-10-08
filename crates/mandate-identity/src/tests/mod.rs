//! The crate's tests (E9-2). They sit in the crate because `Session`, `Membership`, and
//! `MembershipLookup` are sealed to it (DEC-642), so the test doubles below are `cfg(test)` only.

mod matrix;
mod rows;
mod wire;

use crate::{
    LookupFailed, Membership, MembershipLookup, MembershipQuery, OrgId, Permission, PrincipalId,
    PrincipalKind, SessionRef, Tenant, TenantContext, WorkspaceId, sealed,
};

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
}

/// A careless store that returns every membership whatever the query, so the step must pick the
/// member too.
pub(crate) struct Everything<'a>(pub(crate) &'a [Membership]);

impl sealed::Sealed for Everything<'_> {}

impl MembershipLookup for Everything<'_> {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Ok(self.0.to_vec())
    }
}

/// A store that cannot answer.
pub(crate) struct Failing;

impl sealed::Sealed for Failing {}

impl MembershipLookup for Failing {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
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
