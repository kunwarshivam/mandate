//! The members a workspace's approver references can name, read from its control stream's §9.12
//! records (E7-31, [DEC-2384](../../../docs/project/decisions/DEC-2384.md)).
//!
//! A run resolves a mandate's `autonomy.approval.approvers` to user ids before admission check 3
//! compares a responder with them (mandate spec §6.4, V-024; DEC-953). [`Roster::read`] folds the
//! records with [`crate::MembershipFold`] inside this crate, so the run needs no seal: a roster is
//! not a [`crate::MembershipLookup`], builds no session or membership, and carries no
//! `workspace_users` count, so `authorize` and V-047 stay the seal's holders' alone (DEC-642 item
//! 7), and its fields can be public. A fold that is unreadable at the instant read gives no roster (`AGENTS.md` rule 3).

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::decode_ulid;
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;

use crate::{
    MembershipEvent, MembershipFold, MembershipRecord, MembershipState, PrincipalId, Role,
};

/// The members `active` at one instant, each with the roles in effect then, and the founder: the
/// member of the stream's first membership record when that record is the founding grant (an
/// activation with no invitation, identity spec §3.2), while that member is `active`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Roster {
    /// Each member `active` at the instant read, with its effective roles.
    pub active: BTreeMap<PrincipalId, BTreeSet<Role>>,
    /// The founder, while it is `active`.
    pub founder: Option<PrincipalId>,
}

/// Why the records give no roster.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RosterRefused {
    /// The fold is unreadable at the instant read, from the record at `seq` (DEC-657 item 4).
    Unreadable { seq: u64 },
}

impl Roster {
    /// The roster `records`, in `seq` order, give at `at`.
    ///
    /// # Errors
    /// [`RosterRefused::Unreadable`] when a record at or before `at` does not fit the state it
    /// finds, or is out of order.
    pub fn read(records: Vec<MembershipRecord>, at: UtcNanos) -> Result<Self, RosterRefused> {
        let founding = records.first().and_then(|record| match record.event() {
            MembershipEvent::Activated {
                member,
                invitation: None,
                ..
            } => Some(*member),
            _ => None,
        });
        let activated: BTreeSet<PrincipalId> = records
            .iter()
            .filter_map(|record| match record.event() {
                MembershipEvent::Activated { member, .. } => Some(*member),
                _ => None,
            })
            .collect();
        let fold = MembershipFold::new(Seal::grant(), records);
        if let Some(seq) = fold.refused(at) {
            return Err(RosterRefused::Unreadable { seq });
        }
        let active: BTreeMap<PrincipalId, BTreeSet<Role>> = activated
            .into_iter()
            .filter(|member| fold.state(*member, at) == Some(MembershipState::Active))
            .map(|member| (member, fold.effective_roles(member, at)))
            .collect();
        let founder = founding.filter(|member| active.contains_key(member));
        Ok(Self { active, founder })
    }

    /// The members `references` name among the active members holding the approver role in
    /// effect, the set admission check 3 and the API's grant check compare a responder with
    /// (DEC-2384 item 3, DEC-2130): `role:approver` names each, `role:workspace_admin` each also
    /// holding workspace admin, `role:owner` the founder, and `user:<id>` the one whose ULID `<id>`
    /// parses to, in either case (Crockford base32 is case-insensitive; the CLI writes lower case,
    /// DEC-527 item 3). Text that does not parse as a ULID, an alias letter or a space included,
    /// names no one.
    pub fn approvers(&self, references: &[&str]) -> BTreeSet<PrincipalId> {
        self.active
            .iter()
            .filter(|(_, roles)| roles.contains(&Role::Approver))
            .filter(|(member, roles)| {
                references
                    .iter()
                    .any(|reference| match reference.split_once(':') {
                        Some(("role", "approver")) => true,
                        Some(("role", "workspace_admin")) => roles.contains(&Role::WorkspaceAdmin),
                        Some(("role", "owner")) => self.founder == Some(**member),
                        Some(("user", user)) => same_principal(user, **member),
                        Some(_) | None => false,
                    })
            })
            .map(|(member, _)| *member)
            .collect()
    }
}

/// Whether `text`, upper-cased, is the canonical ULID text of `member` (DEC-2384 item 3).
fn same_principal(text: &str, member: PrincipalId) -> bool {
    decode_ulid(&text.to_ascii_uppercase()).is_ok_and(|id| id == member.0)
}
