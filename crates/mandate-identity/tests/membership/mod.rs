//! What the membership fold's tests share (E9-7, DEC-657): instants, role masks, and records built
//! as journal spec §9.12 and rule 103 say a writer builds them.

use std::collections::BTreeSet;

use mandate_identity::{
    InvitationId, MembershipEvent as Event, MembershipFold, MembershipRecord, PrincipalId, Role,
};
use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;

pub const ROLES: [Role; 5] = [
    Role::Approver,
    Role::Auditor,
    Role::Operator,
    Role::Viewer,
    Role::WorkspaceAdmin,
];
pub const DAY: i64 = 86_400;
pub const EPOCH: i64 = 1_790_000_000;
pub const FOUNDER: u128 = 0xF;
pub const B: u128 = 0xB;

pub fn t(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(EPOCH + secs, 0).unwrap()
}

/// The roles a mask names, bit *i* for `ROLES[i]`.
pub fn set(mask: u8) -> BTreeSet<Role> {
    (0..5)
        .filter(|i| mask >> i & 1 == 1)
        .map(|i| ROLES[i])
        .collect()
}

/// Rule 103 as these tests read it: a day when independence is required and the grant adds
/// operator or approver to an existing workspace, and none otherwise.
pub fn cool(at: i64, independent: bool, roles: &BTreeSet<Role>) -> UtcNanos {
    let cooling = roles.contains(&Role::Operator) || roles.contains(&Role::Approver);
    t(if independent && cooling { at + DAY } else { at })
}

pub fn invited(invitation: u128, roles: BTreeSet<Role>, at: i64) -> Event {
    let (invitation, expires_at) = (InvitationId(invitation), t(at + 7 * DAY));
    Event::Invited {
        invitation,
        roles,
        expires_at,
    }
}

pub fn activated(
    member: u128,
    inv: Option<u128>,
    roles: BTreeSet<Role>,
    ind: bool,
    at: i64,
) -> Event {
    let (member, invitation, cool_off_ends_at) = (
        PrincipalId(member),
        inv.map(InvitationId),
        cool(at, ind && inv.is_some(), &roles),
    );
    let independent_approval_required = ind;
    Event::Activated {
        member,
        invitation,
        roles,
        independent_approval_required,
        cool_off_ends_at,
    }
}

pub fn reactivated(member: u128, roles: BTreeSet<Role>, ind: bool, at: i64) -> Event {
    let (member, cool_off_ends_at) = (PrincipalId(member), cool(at, ind, &roles));
    let independent_approval_required = ind;
    Event::Reactivated {
        member,
        roles,
        independent_approval_required,
        cool_off_ends_at,
    }
}

/// The founding grant at instant 0: approver, operator, and workspace admin, recording `ind`; it
/// creates the workspace, so it never cools off (rule 103).
pub fn founded(ind: bool) -> Event {
    activated(FOUNDER, None, set(0b10101), ind, 0)
}

pub fn founding() -> (i64, Event) {
    (0, founded(false))
}

/// The record at `seq`, committed at instant `at`, built with the seal as the store's adapter is.
pub fn record(seq: u64, at: i64, event: Event) -> MembershipRecord {
    MembershipRecord::new(Seal::grant(), seq, t(at), event)
}

/// The fold of records numbered as given, so a test can put them out of order.
pub fn sequenced(records: Vec<(u64, i64, Event)>) -> MembershipFold {
    let records = records.into_iter().map(|(seq, at, e)| record(seq, at, e));
    MembershipFold::new(Seal::grant(), records.collect())
}

/// The fold of records numbered from 1.
pub fn fold(events: Vec<(i64, Event)>) -> MembershipFold {
    sequenced(
        events
            .into_iter()
            .zip(1..)
            .map(|((at, e), seq)| (seq, at, e))
            .collect(),
    )
}
