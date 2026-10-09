//! The membership fold of journal spec §9.12 and identity spec §5 (E9-7, DEC-657): a workspace's
//! membership records fold to each member's state, its effective roles, each invitation's
//! state, and the `workspace_users` count V-047 reads (§5.3, ID-7).
//!
//! It reads [`MembershipRecord`]s, this crate's own minimal input, which the workspace store's
//! adapter maps the journal's records into; records of principals that hold no membership (clients,
//! agents, service accounts) never reach it. `mandate-journal` owns the records' schemas and rules
//! 96 to 106; this crate owns the state checks and the cross-record checks (DEC-648 item 7).

use std::collections::{BTreeMap, BTreeSet};

use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;

use crate::{MembershipState, PrincipalId, Role};

/// An invitation's opaque ULID (journal spec §9.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InvitationId(pub u128);

/// An invitation's state: §5.1's `invited`, `expired`, and `revoked`, and `accepted`, the fold's own
/// name for one an activation used (§9.12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[allow(
    missing_docs,
    reason = "each variant is the §9.12 state of the same name"
)]
pub enum InvitationState {
    Invited,
    Accepted,
    Expired,
    Revoked,
}

/// What one §9.12 record says: each variant is the `Member*` record of its name, with the payload
/// members the fold and the cross-record checks read. An activation's `invitation` is `None` for
/// the founding grant; a role change's `added` maps each granted role to its cool-off's end.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(
    missing_docs,
    reason = "each field is the §9.12 payload member of its name"
)]
pub enum MembershipEvent {
    Invited {
        invitation: InvitationId,
        roles: BTreeSet<Role>,
        expires_at: UtcNanos,
    },
    InvitationRevoked {
        invitation: InvitationId,
    },
    Activated {
        member: PrincipalId,
        invitation: Option<InvitationId>,
        roles: BTreeSet<Role>,
        independent_approval_required: bool,
        cool_off_ends_at: UtcNanos,
    },
    RoleChanged {
        member: PrincipalId,
        added: BTreeMap<Role, UtcNanos>,
        removed: BTreeSet<Role>,
        independent_approval_required: bool,
    },
    Deactivated {
        member: PrincipalId,
    },
    Reactivated {
        member: PrincipalId,
        roles: BTreeSet<Role>,
        independent_approval_required: bool,
        cool_off_ends_at: UtcNanos,
    },
    Removed {
        member: PrincipalId,
    },
}

/// One membership record of a workspace's control stream: its `seq`, its envelope's `event_time`
/// (rule 106 makes it the record's own instant), and what it says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipRecord {
    seq: u64,
    event_time: UtcNanos,
    event: MembershipEvent,
}

impl MembershipRecord {
    /// The record at `seq` on the control stream, committed at `event_time`, for the seal's holder.
    pub fn new(seal: Seal, seq: u64, event_time: UtcNanos, event: MembershipEvent) -> Self {
        let _: Seal = seal;
        Self {
            seq,
            event_time,
            event,
        }
    }
}

/// A workspace's membership records, folded. A reading at *t* folds, in `seq` order, only the
/// records whose `event_time` is at or before *t*, and reads each cool-off and expiry against *t*
/// (§9.12). A record that does not fit the state it finds, or is out of order (DEC-657 item 4),
/// makes it unreadable from that record's `event_time`; unreadable latches, and
/// [`Self::workspace_users`] then reads 1 (§5.3, rule 3). Only the seal's holders build one
/// (DEC-642 items 4 and 7), so a count is only ever what the fold read from the store's records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MembershipFold {
    records: Vec<MembershipRecord>,
}

#[expect(
    clippy::todo,
    reason = "a fold and its readings have no error to carry, so their stubs are todo!(), the \
              other form DEC-137 names"
)]
impl MembershipFold {
    /// The fold of `records`, given in `seq` order, for a holder of the seal.
    pub fn new(_seal: Seal, _records: Vec<MembershipRecord>) -> Self {
        todo!()
    }

    /// Whether a record at or before `at` did not fit the state it found.
    pub fn unreadable(&self, _at: UtcNanos) -> bool {
        todo!()
    }

    /// The `seq` of the record at or before `at` that made the fold unreadable, if any.
    pub fn refused(&self, _at: UtcNanos) -> Option<u64> {
        todo!()
    }

    /// The member's §5.1 state at `at`, or `None` if no record activated it by then.
    pub fn state(&self, _member: PrincipalId, _at: UtcNanos) -> Option<MembershipState> {
        todo!()
    }

    /// The member's roles whose cool-off ended at or before `at`, while it is `cooling_off` or
    /// `active`; empty in every other state.
    pub fn effective_roles(&self, _member: PrincipalId, _at: UtcNanos) -> BTreeSet<Role> {
        todo!()
    }

    /// The invitation's state at `at`, or `None` if it was not issued by then.
    pub fn invitation(&self, _invitation: InvitationId, _at: UtcNanos) -> Option<InvitationState> {
        todo!()
    }

    /// `workspace_users` at `at` (§5.3, ID-7): the members `active`, so past their cool-off, at
    /// `at`, or 1 when the fold is unreadable at `at`.
    pub fn workspace_users(&self, _at: UtcNanos) -> u32 {
        todo!()
    }
}
