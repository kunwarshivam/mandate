//! Memberships from the workspace's control stream (§3.11 item 8, DEC-978 item 8): the one
//! [`MembershipLookup`] `authorize` reads, built per request from journal spec §9.12's records,
//! uncached (identity spec ID-3). A record that cannot be read, or a fold that cannot answer, is a
//! failed lookup, never an empty membership.

use std::collections::{BTreeMap, BTreeSet};

use mandate_identity::{
    LookupFailed, Membership, MembershipFold, MembershipLookup, MembershipQuery, MembershipRecord,
    OrgId, PrincipalId, Role, Scope, WorkspaceId,
};
use mandate_identity_seal::{LookupSeal, Seal};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

/// Turns the control stream's rows into its membership records, refusing a `Member*` record
/// that does not map (lane ID1's `mandate_identity::control_entry`).
pub type ParseMembers =
    dyn Fn(&[StoredEvent]) -> Result<Vec<MembershipRecord>, LookupFailed> + Send + Sync;

/// The parser until lane ID1's `control_entry` lands: it reads no record, so every lookup fails
/// and every read answers `membership_unavailable` (fail closed, `AGENTS.md` rule 3).
///
/// # Errors
/// Always [`LookupFailed`].
pub fn until_parser(rows: &[StoredEvent]) -> Result<Vec<MembershipRecord>, LookupFailed> {
    let _ = rows;
    Err(LookupFailed)
}

/// The one workspace this deployment hosts, and its organization (§3.11 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hosted {
    pub org: OrgId,
    pub workspace: WorkspaceId,
    /// Whether the deployment's configuration marks it one of the team's internal paper
    /// workspaces, checked against its control stream at start (DEC-1143).
    pub internal_paper: bool,
}

/// The hosted workspace's memberships at one instant.
pub struct ControlLookup {
    hosted: Hosted,
    now: UtcNanos,
    records: Result<Vec<MembershipRecord>, LookupFailed>,
}

impl ControlLookup {
    /// The lookup over `records`, read from the hosted workspace's control stream, at `now`.
    pub fn new(
        hosted: Hosted,
        records: Result<Vec<MembershipRecord>, LookupFailed>,
        now: UtcNanos,
    ) -> Self {
        Self {
            hosted,
            now,
            records,
        }
    }

    /// `member`'s memberships, for the session's roles snapshot; none when the lookup fails.
    pub fn snapshot(&self, member: PrincipalId) -> Vec<Membership> {
        self.memberships(&MembershipQuery {
            member: Some(member),
            scope: self.scope(),
        })
        .unwrap_or_default()
    }

    /// The hosted workspace's scope.
    fn scope(&self) -> Scope {
        Scope::Workspace {
            org: self.hosted.org,
            workspace: self.hosted.workspace,
        }
    }

    /// The control stream's fold, readable at `now`, or a failed lookup.
    fn fold(&self) -> Result<MembershipFold, LookupFailed> {
        let records = self.records.clone()?;
        let fold = MembershipFold::new(Seal::grant(), records);
        if fold.unreadable(self.now) {
            return Err(LookupFailed);
        }
        Ok(fold)
    }
}

impl LookupSeal for ControlLookup {}

impl MembershipLookup for ControlLookup {
    /// One member's membership of the hosted workspace, as the fold reads it at the lookup's
    /// instant: its state and the roles effective then. The query for every member of a scope
    /// (the last-owner count of a role change) is not served here and fails.
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        let member = query.member.ok_or(LookupFailed)?;
        let fold = self.fold()?;
        if query.scope != self.scope() {
            return Ok(Vec::new());
        }
        let Some(state) = fold.state(member, self.now) else {
            return Ok(Vec::new());
        };
        let roles: BTreeMap<Role, UtcNanos> = fold
            .effective_roles(member, self.now)
            .into_iter()
            .map(|role| (role, self.now))
            .collect();
        Ok(vec![Membership::new(
            Seal::grant(),
            member,
            query.scope,
            state,
            roles,
        )])
    }

    /// The hosted workspace, for its own organization; the store's read is the control stream's.
    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        self.records.as_ref().map_err(|_| LookupFailed)?;
        Ok(if org == self.hosted.org {
            BTreeSet::from([self.hosted.workspace])
        } else {
            BTreeSet::new()
        })
    }

    /// The hosted workspace's organization, from the deployment's settings; any other workspace
    /// is not hosted here.
    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok((workspace == self.hosted.workspace).then_some(self.hosted.org))
    }
}
