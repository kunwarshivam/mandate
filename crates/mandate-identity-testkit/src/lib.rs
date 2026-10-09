//! Test support for `mandate-identity`'s sealed types (DEC-645), so other crates' tests can call
//! the real `authorize`.
//!
//! It sits at layer 11, above every product crate, so a crate can take it only as a
//! dev-dependency: a normal dependency must point to a lower layer (`cargo xtask layers`), as
//! `mandate-rh-sim` does (DEC-124). It is an allowed dependent of `mandate-identity-seal`.

use std::collections::BTreeSet;

use mandate_identity::{
    LookupFailed, Membership, MembershipLookup, MembershipQuery, MembershipState, OrgId,
    PrincipalId, Role, Scope, Session, SessionKind, SessionRef, WorkspaceId,
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
/// memberships in the scope, or every membership of the scope. The workspaces it hosts, and the
/// organization each one's record names, are those its workspace memberships name: a workspace no
/// membership names is one this deployment does not host (`workspace_org` answers `None`, and no
/// organization's set holds it), so give a workspace at least one membership to host it.
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

    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Ok(hosted(&self.0)
            .filter(|(_, o)| *o == org)
            .map(|(w, _)| w)
            .collect())
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok(hosted(&self.0)
            .find(|(w, _)| *w == workspace)
            .map(|(_, o)| o))
    }
}

/// The (workspace, organization) pairs the memberships name.
fn hosted(memberships: &[Membership]) -> impl Iterator<Item = (WorkspaceId, OrgId)> + '_ {
    memberships.iter().filter_map(|m| match m.scope() {
        Scope::Workspace { org, workspace } => Some((workspace, org)),
        Scope::Principal | Scope::Org(_) => None,
    })
}

/// A membership store that cannot answer, an outage: neither memberships nor an organization's
/// workspaces can be read, while each workspace's own record still can (identity spec §4.5, DEC-832
/// item 4). It holds those records, each a workspace and the organization it names.
#[derive(Debug, Clone, Default)]
pub struct FailingLookup(pub Vec<(WorkspaceId, OrgId)>);

impl LookupSeal for FailingLookup {}

impl MembershipLookup for FailingLookup {
    fn memberships(&self, _: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        Err(LookupFailed)
    }

    fn workspaces(&self, _: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Err(LookupFailed)
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok(self
            .0
            .iter()
            .find(|(w, _)| *w == workspace)
            .map(|(_, o)| *o))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use mandate_identity::{
        LookupFailed, MembershipLookup, MembershipQuery, MembershipState, OrgId, PrincipalId,
        Scope, WorkspaceId,
    };

    use super::{FailingLookup, StaticLookup, membership};

    const O1: OrgId = OrgId(0x11);
    const O2: OrgId = OrgId(0x12);
    const W1: WorkspaceId = WorkspaceId(0x21);
    const W2: WorkspaceId = WorkspaceId(0x22);
    const U1: PrincipalId = PrincipalId(0x31);
    const U2: PrincipalId = PrincipalId(0x32);
    const WS1: Scope = Scope::Workspace {
        org: O1,
        workspace: W1,
    };

    /// The store answers only the scope and member asked, and hosts only the workspaces its
    /// memberships name, under the organization they name.
    #[test]
    fn static_lookup_answers_the_query_and_hosts_only_named_workspaces() {
        let active = MembershipState::Active;
        let mine = membership(U1, WS1, active, &[]);
        let theirs = membership(U2, WS1, active, &[]);
        let org = membership(U1, Scope::Org(O1), active, &[]);
        let lookup = StaticLookup(vec![mine.clone(), theirs.clone(), org.clone()]);
        let query = |member, scope| MembershipQuery { member, scope };
        assert_eq!(
            lookup.memberships(&query(Some(U1), WS1)),
            Ok(vec![mine.clone()])
        );
        assert_eq!(
            lookup.memberships(&query(None, WS1)),
            Ok(vec![mine, theirs])
        );
        assert_eq!(
            lookup.memberships(&query(Some(U1), Scope::Org(O1))),
            Ok(vec![org])
        );
        assert_eq!(
            lookup.memberships(&query(Some(U1), Scope::Org(O2))),
            Ok(vec![])
        );
        assert_eq!(lookup.workspaces(O1), Ok(BTreeSet::from([W1])));
        assert_eq!(lookup.workspaces(O2), Ok(BTreeSet::new()));
        assert_eq!(lookup.workspace_org(W1), Ok(Some(O1)));
        assert_eq!(lookup.workspace_org(W2), Ok(None));
    }

    /// The outage double fails every membership and set read, and still reads the records it holds.
    #[test]
    fn failing_lookup_reads_only_workspace_records() {
        let lookup = FailingLookup(vec![(W1, O1)]);
        let query = MembershipQuery {
            member: Some(U1),
            scope: WS1,
        };
        assert_eq!(lookup.memberships(&query), Err(LookupFailed));
        assert_eq!(lookup.workspaces(O1), Err(LookupFailed));
        assert_eq!(lookup.workspace_org(W1), Ok(Some(O1)));
        assert_eq!(lookup.workspace_org(W2), Ok(None));
    }
}
