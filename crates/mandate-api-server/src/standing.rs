//! Route 2's standing (§4.10 route 2 item 2; identity spec §6.4 route 2), as the interim of
//! [DEC-2319] item 2 reads it, which is Proposed and waits for the founder.
//!
//! The server sees no deprovision signal yet ([DEC-2319] item 1), so it keeps, in memory, only
//! each member's last successful sign-in on this process. Route 2 is refused to a member with no
//! sign-in kept, which after a restart is every member until they sign in again, and to a member
//! named by a membership record committed at or after that sign-in. Route 2's judge runs both
//! checks for every request, against a decoy principal when the passkey is unknown (§4.10 item 4).
//!
//! [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md

use std::collections::BTreeMap;
use std::sync::Mutex;

use mandate_authn::SubjectStanding;
use mandate_identity::{MembershipRecord, PrincipalId};
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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn signed_in(&self, member: PrincipalId, at: UtcNanos) {
        let _ = (member, at, &self.signed_in);
        todo!()
    }

    /// `member`'s standing: the last sign-in kept, and no deprovision, which has no source yet
    /// ([DEC-2319] item 1).
    ///
    /// [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub fn standing(&self, member: PrincipalId) -> SubjectStanding {
        let _ = member;
        todo!()
    }
}

/// Whether [DEC-2319] item 2's interim refuses route 2 to `member`: `standing` holds no sign-in, or
/// a record among `records` names `member` and was committed at or after the last sign-in. Every
/// record is read whatever the answer, so the work does not depend on where a match is.
///
/// [DEC-2319]: ../../../docs/project/decisions/DEC-2319.md
#[expect(
    clippy::todo,
    reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
)]
pub fn interim_refuses(
    standing: SubjectStanding,
    records: &[MembershipRecord],
    member: PrincipalId,
) -> bool {
    let _ = (standing, records, member);
    todo!()
}
