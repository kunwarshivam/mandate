//! The crate's tests (E9-2). They sit in the crate because `Session`, `Membership`, and
//! `MembershipLookup` are sealed to it (DEC-642), so the test doubles below are `cfg(test)` only.

mod matrix;
mod rows;
mod ulid;
mod wire;

use mandate_identity_seal::LookupSeal;

use crate::{
    LookupFailed, Membership, MembershipLookup, MembershipQuery, OrgId, Permission, PrincipalId,
    PrincipalKind, SessionRef, Tenant, TenantContext, WorkspaceId,
};

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
}

/// A careless store that returns every membership whatever the query, so the step must pick the
/// member too.
pub(crate) struct Everything<'a>(pub(crate) &'a [Membership]);

impl LookupSeal for Everything<'_> {}

impl MembershipLookup for Everything<'_> {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Ok(self.0.to_vec())
    }
}

/// A store that cannot answer.
pub(crate) struct Failing;

impl LookupSeal for Failing {}

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
