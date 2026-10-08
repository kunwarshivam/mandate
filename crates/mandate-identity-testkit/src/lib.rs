//! Test support for `mandate-identity`'s sealed types (DEC-645), so other crates' tests can call
//! the real `authorize`.
//!
//! It sits at layer 11, above every product crate, so a crate can take it only as a
//! dev-dependency: a normal dependency must point to a lower layer (`cargo xtask layers`), as
//! `mandate-rh-sim` does (DEC-124). It is an allowed dependent of `mandate-identity-seal`.

use mandate_identity::{
    LookupFailed, Membership, MembershipLookup, MembershipQuery, MembershipState, PrincipalId,
    Role, Scope, Session, SessionKind, SessionRef,
};
use mandate_identity_seal::{LookupSeal, Seal};
use mandate_time::UtcNanos;

/// A membership of `member` in `scope`, in `state`, holding `roles`, each with the instant it
/// becomes effective (the end of its cool-off).
pub fn membership(
    member: PrincipalId,
    scope: Scope,
    state: MembershipState,
    roles: &[(Role, UtcNanos)],
) -> Membership {
    Membership::new(
        Seal::grant(),
        member,
        scope,
        state,
        roles.iter().copied().collect(),
    )
}

/// A session `reference` of `kind` whose roles snapshot is `snapshot`: the memberships, one per
/// workspace or organization, that reach their scopes.
pub fn session(reference: SessionRef, kind: SessionKind, snapshot: Vec<Membership>) -> Session {
    Session::new(Seal::grant(), reference, kind, snapshot)
}

/// An in-memory membership store that answers the query it is asked: the named member's
/// memberships in the scope, or every membership of the scope.
#[derive(Debug, Clone, Default)]
pub struct StaticLookup(pub Vec<Membership>);

impl LookupSeal for StaticLookup {}

impl MembershipLookup for StaticLookup {
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Ok(self
            .0
            .iter()
            .filter(|m| m.scope() == query.scope)
            .filter(|m| query.member.is_none_or(|member| m.member() == member))
            .cloned()
            .collect())
    }
}

/// A membership store that cannot answer: an outage.
#[derive(Debug, Clone, Copy, Default)]
pub struct FailingLookup;

impl LookupSeal for FailingLookup {}

impl MembershipLookup for FailingLookup {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Err(LookupFailed)
    }
}
