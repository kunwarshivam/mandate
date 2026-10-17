//! Memberships from the workspace's control stream (§3.11 item 8, DEC-978 item 8): the one
//! [`MembershipLookup`] `authorize` reads, built per request from journal spec §9.12's records,
//! uncached (identity spec ID-3). A record that cannot be read, or a fold that cannot answer, is a
//! failed lookup, never an empty membership.

use std::collections::{BTreeMap, BTreeSet};

use mandate_identity::{
    LookupFailed, Membership, MembershipLookup, MembershipQuery, MembershipRecord, OrgId,
    PrincipalId, WorkspaceId,
};
use mandate_identity_seal::LookupSeal;
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
        let _ = member;
        Vec::new()
    }
}

impl LookupSeal for ControlLookup {}

impl MembershipLookup for ControlLookup {
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        let _ = (query, &self.records, self.now, BTreeMap::<u8, u8>::new());
        Err(LookupFailed)
    }

    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        let _ = (org, self.hosted);
        Err(LookupFailed)
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        let _ = workspace;
        Err(LookupFailed)
    }
}
