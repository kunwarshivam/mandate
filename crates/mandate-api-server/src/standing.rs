//! Route 2's standing (§4.10 route 2 item 2; identity spec §6.4 route 2), as the interim of
//! [DEC-2319] item 2 reads it, in force until the journaled deprovision signal the founder chose
//! lands (backlog E9-13). The session store holds the process's only [`Standings`] and is its only
//! writer.
//!
//! The server sees no deprovision signal yet ([DEC-2319] item 1), so it keeps, in memory, only
//! each member's last successful sign-in on this process. Route 2 is refused to a member with no
//! sign-in kept, which after a restart is every member until they sign in again, and to a member
//! named by a membership record committed at or after that sign-in. Route 2's judge runs both
//! checks for every request, against a decoy principal when the passkey is unknown (§4.10 item 4).
//!
//! [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md

use std::collections::BTreeMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use mandate_authn::SubjectStanding;
use mandate_identity::{MembershipEvent, MembershipRecord, PrincipalId};
use mandate_time::UtcNanos;

/// Each member's last successful sign-in this process has seen.
#[derive(Debug, Default)]
pub struct Standings {
    signed_in: Mutex<BTreeMap<PrincipalId, UtcNanos>>,
}

impl Standings {
    /// None seen yet: every member needs a sign-in before route 2 serves them.
    pub fn new() -> Self {
        Self::default()
    }

    /// Keeps `member`'s successful sign-in at `at`. A sign-in earlier than the one kept moves
    /// nothing back.
    pub fn signed_in(&self, member: PrincipalId, at: UtcNanos) {
        self.held()
            .entry(member)
            .and_modify(|kept| *kept = (*kept).max(at))
            .or_insert(at);
    }

    /// The sign-ins, recovered if a panic poisoned the lock: they are plain data, and refusing on a
    /// poisoned lock would refuse every route-2 opening (`AGENTS.md` rule 13).
    fn held(&self) -> MutexGuard<'_, BTreeMap<PrincipalId, UtcNanos>> {
        self.signed_in
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// `member`'s standing: the last sign-in kept, and no deprovision, which has no source yet
    /// ([DEC-2319] item 1).
    ///
    /// [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
    pub fn standing(&self, member: PrincipalId) -> SubjectStanding {
        SubjectStanding {
            last_sign_in: self.held().get(&member).copied(),
            last_deprovision: None,
        }
    }
}

/// Whether [DEC-2319] item 2's interim refuses route 2 to `member`: `standing` holds no sign-in, or
/// a record among `records` names `member` and was committed at or after the last sign-in. Every
/// record is read whatever the answer, so the work does not depend on where a match is.
///
/// [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
pub fn interim_refuses(
    standing: SubjectStanding,
    records: &[MembershipRecord],
    member: PrincipalId,
) -> bool {
    let named_since = records
        .iter()
        .filter(|record| {
            named(record.event()) == Some(member)
                && standing
                    .last_sign_in
                    .is_some_and(|signed_in| record.event_time() >= signed_in)
        })
        .count();
    standing.last_sign_in.is_none() || named_since > 0
}

/// The member a membership record names; `None` for an invitation's, which names none yet.
fn named(event: &MembershipEvent) -> Option<PrincipalId> {
    match event {
        MembershipEvent::Invited { .. } | MembershipEvent::InvitationRevoked { .. } => None,
        MembershipEvent::Activated { member, .. }
        | MembershipEvent::RoleChanged { member, .. }
        | MembershipEvent::Deactivated { member }
        | MembershipEvent::Reactivated { member, .. }
        | MembershipEvent::Removed { member } => Some(*member),
    }
}
