//! The membership fold of journal spec §9.12 and identity spec §5 (E9-7, DEC-657): a workspace's
//! membership records fold to each member's state, its effective and kept roles, each invitation's
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

    /// Its `seq` on the control stream.
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Its envelope's `event_time`, the record's own instant (rule 106).
    pub fn event_time(&self) -> UtcNanos {
        self.event_time
    }

    /// What it says.
    pub fn event(&self) -> &MembershipEvent {
        &self.event
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

impl MembershipFold {
    /// The fold of `records`, given in `seq` order, for a holder of the seal.
    pub fn new(seal: Seal, records: Vec<MembershipRecord>) -> Self {
        let _: Seal = seal;
        Self { records }
    }

    /// Whether a record at or before `at` did not fit the state it found.
    pub fn unreadable(&self, at: UtcNanos) -> bool {
        self.refused(at).is_some()
    }

    /// The `seq` of the record at or before `at` that made the fold unreadable, if any.
    pub fn refused(&self, at: UtcNanos) -> Option<u64> {
        self.replay(at).refused
    }

    /// The member's §5.1 state at `at`, or `None` if no record activated it by then.
    pub fn state(&self, member: PrincipalId, at: UtcNanos) -> Option<MembershipState> {
        let replay = self.replay(at);
        let seat = replay.seats.get(&member)?;
        Some(match seat.standing {
            Standing::Live if at >= seat.until => MembershipState::Active,
            Standing::Live => MembershipState::CoolingOff,
            Standing::Deactivated => MembershipState::Deactivated,
            Standing::Removed => MembershipState::Removed,
        })
    }

    /// The member's roles whose cool-off ended at or before `at`, while it is `cooling_off` or
    /// `active`; empty in every other state.
    pub fn effective_roles(&self, member: PrincipalId, at: UtcNanos) -> BTreeSet<Role> {
        let replay = self.replay(at);
        let held = replay.seats.get(&member).map(|seat| &seat.roles);
        held.into_iter()
            .flatten()
            .filter(|(_, since)| **since <= at)
            .map(|(role, _)| *role)
            .collect()
    }

    /// The roles a `deactivated` member keeps for a reactivation at `at`, each it held when
    /// deactivated, effective or still cooling, less those removed since; empty in other states.
    pub fn kept_roles(&self, member: PrincipalId, at: UtcNanos) -> BTreeSet<Role> {
        let replay = self.replay(at);
        match replay.seats.get(&member) {
            Some(seat) if seat.standing == Standing::Deactivated => seat.kept.clone(),
            _ => BTreeSet::new(),
        }
    }

    /// The invitation's state at `at`, or `None` if it was not issued by then.
    pub fn invitation(&self, invitation: InvitationId, at: UtcNanos) -> Option<InvitationState> {
        let replay = self.replay(at);
        let invite = replay.invites.get(&invitation)?;
        Some(match invite.state {
            InvitationState::Invited if at >= invite.expires_at => InvitationState::Expired,
            state => state,
        })
    }

    /// `workspace_users` at `at` (§5.3, ID-7): the members `active`, so past their cool-off, at
    /// `at`, or 1 when the fold is unreadable at `at`.
    pub fn workspace_users(&self, at: UtcNanos) -> u32 {
        let replay = self.replay(at);
        if replay.refused.is_some() {
            return 1;
        }
        let active = replay
            .seats
            .values()
            .filter(|seat| seat.standing == Standing::Live && seat.until <= at)
            .count();
        u32::try_from(active).unwrap_or(u32::MAX)
    }

    /// The fold of the records whose `event_time` is at or before `at`, in `seq` order, each
    /// checked for order against the record held before it, folded at `at` or not (DEC-657 item
    /// 4). The first record that does not fit is refused and the rest fold on, as the reference
    /// fold does, so the latch holds.
    fn replay(&self, at: UtcNanos) -> Replay {
        let mut replay = Replay::default();
        let mut previous: Option<&MembershipRecord> = None;
        for record in &self.records {
            let ordered = previous.is_none_or(|before| follows(before, record));
            previous = Some(record);
            if record.event_time > at {
                continue;
            }
            let fits = ordered && replay.apply(record).is_some();
            if !fits && replay.refused.is_none() {
                replay.refused = Some(record.seq);
            }
        }
        replay
    }
}

/// Whether `record` may follow `before` (DEC-657 item 4): its `seq` is above, and its
/// `event_time` not before, `before`'s; no `seq` is above `u64::MAX`.
fn follows(before: &MembershipRecord, record: &MembershipRecord) -> bool {
    let seq_above = before
        .seq
        .checked_add(1)
        .is_some_and(|next| record.seq >= next);
    seq_above && record.event_time >= before.event_time
}

/// Where a membership stands in the fold: live (`cooling_off` until its cool-off ends, then
/// `active`), `deactivated`, or `removed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    Live,
    Deactivated,
    Removed,
}

/// A member as the fold keeps it: where it stands, the end of its activation's or reactivation's
/// cool-off, each held role with the instant it becomes effective, and, while deactivated, the
/// roles it keeps. A deactivation moves the held roles to the kept ones, so a member holds roles
/// only while it is live, and a removed one neither holds nor keeps any.
#[derive(Debug, Clone)]
struct Seat {
    standing: Standing,
    until: UtcNanos,
    roles: BTreeMap<Role, UtcNanos>,
    kept: BTreeSet<Role>,
}

impl Seat {
    /// A membership entering `cooling_off` at `at` until `end` with `roles`: operator and approver
    /// are effective from `end` (§8.3), every other role from `at`.
    fn started(roles: &BTreeSet<Role>, at: UtcNanos, end: UtcNanos) -> Self {
        let since = |role: &Role| match role {
            Role::Approver | Role::Operator => end,
            _ => at,
        };
        Self {
            standing: Standing::Live,
            until: end,
            roles: roles.iter().map(|role| (*role, since(role))).collect(),
            kept: BTreeSet::new(),
        }
    }
}

/// An issued invitation: its state (`invited`, `accepted`, or `revoked`; `expired` is read
/// against the instant), the roles it names, and its expiry.
#[derive(Debug, Clone)]
struct Invite {
    state: InvitationState,
    roles: BTreeSet<Role>,
    expires_at: UtcNanos,
}

impl Invite {
    /// Whether it can still be used or revoked at `at`: `invited`, and `at` before `expires_at`.
    fn open(&self, at: UtcNanos) -> bool {
        self.state == InvitationState::Invited && at < self.expires_at
    }
}

/// The fold's state after the records so far, and the first record it refused.
#[derive(Debug, Clone, Default)]
struct Replay {
    seats: BTreeMap<PrincipalId, Seat>,
    invites: BTreeMap<InvitationId, Invite>,
    refused: Option<u64>,
}

impl Replay {
    /// Folds `record` if it fits the state it finds, or returns `None` and changes nothing.
    fn apply(&mut self, record: &MembershipRecord) -> Option<()> {
        let at = record.event_time;
        match &record.event {
            MembershipEvent::Invited {
                invitation,
                roles,
                expires_at,
            } => {
                refuse_unless(!self.invites.contains_key(invitation))?;
                let invite = Invite {
                    state: InvitationState::Invited,
                    roles: roles.clone(),
                    expires_at: *expires_at,
                };
                self.invites.insert(*invitation, invite);
            }
            MembershipEvent::InvitationRevoked { invitation } => {
                let invite = self.invites.get_mut(invitation)?;
                refuse_unless(invite.open(at))?;
                invite.state = InvitationState::Revoked;
            }
            MembershipEvent::Activated {
                member,
                invitation,
                roles,
                cool_off_ends_at,
                ..
            } => {
                let fresh = self
                    .seats
                    .get(member)
                    .is_none_or(|seat| seat.standing == Standing::Removed);
                refuse_unless(fresh)?;
                if let Some(invitation) = invitation {
                    let invite = self.invites.get_mut(invitation)?;
                    refuse_unless(invite.open(at) && invite.roles == *roles)?;
                    invite.state = InvitationState::Accepted;
                }
                self.seats
                    .insert(*member, Seat::started(roles, at, *cool_off_ends_at));
            }
            MembershipEvent::RoleChanged {
                member,
                added,
                removed,
                ..
            } => {
                let seat = self.seats.get_mut(member)?;
                match seat.standing {
                    Standing::Live => {
                        refuse_unless(removed.iter().all(|role| seat.roles.contains_key(role)))?;
                        refuse_unless(added.keys().all(|role| !seat.roles.contains_key(role)))?;
                        seat.roles.retain(|role, _| !removed.contains(role));
                        seat.roles
                            .extend(added.iter().map(|(role, end)| (*role, *end)));
                    }
                    Standing::Deactivated => {
                        refuse_unless(added.is_empty() && removed.is_subset(&seat.kept))?;
                        seat.kept.retain(|role| !removed.contains(role));
                    }
                    Standing::Removed => return None,
                }
            }
            MembershipEvent::Deactivated { member } => {
                let seat = self.seats.get_mut(member)?;
                refuse_unless(seat.standing == Standing::Live)?;
                seat.kept = std::mem::take(&mut seat.roles).into_keys().collect();
                seat.standing = Standing::Deactivated;
            }
            MembershipEvent::Reactivated {
                member,
                roles,
                cool_off_ends_at,
                ..
            } => {
                let seat = self.seats.get_mut(member)?;
                refuse_unless(seat.standing == Standing::Deactivated)?;
                refuse_unless(!roles.is_empty() && seat.kept == *roles)?;
                *seat = Seat::started(roles, at, *cool_off_ends_at);
            }
            MembershipEvent::Removed { member } => {
                let seat = self.seats.get_mut(member)?;
                refuse_unless(seat.standing == Standing::Deactivated)?;
                seat.standing = Standing::Removed;
            }
        }
        Some(())
    }
}

/// `Some` when the record fits, so `?` stops a fold step that does not.
fn refuse_unless(fits: bool) -> Option<()> {
    fits.then_some(())
}

/// Why workspace services refuse to commit a membership record (§9.12's cross-record checks).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RecordRefusal {
    /// The record's `independent_approval_required` is not the workspace's effective policy at
    /// its `event_time` (DEC-648 item 7), so its cool-off (rule 103) was decided on a false input.
    #[error("the record's independent_approval_required is not the effective policy")]
    IndependenceMismatch,
    /// The record's `seq` is not above the stream's last membership record's, or its `event_time`
    /// is before that record's (DEC-659), so the fold would read it as out of order (DEC-657 item 4).
    #[error("the record is out of order with the stream's last membership record")]
    OutOfOrder,
}

/// The writer's check that a record's `independent_approval_required` equals `effective`, the
/// workspace's effective policy at the record's `event_time` (mandate spec §4.3), which the caller
/// reads from the policy fold (DEC-657 item 7). A record with no such member passes.
pub fn check_independence(record: &MembershipRecord, effective: bool) -> Result<(), RecordRefusal> {
    let recorded = match &record.event {
        MembershipEvent::Activated {
            independent_approval_required,
            ..
        }
        | MembershipEvent::RoleChanged {
            independent_approval_required,
            ..
        }
        | MembershipEvent::Reactivated {
            independent_approval_required,
            ..
        } => *independent_approval_required,
        MembershipEvent::Invited { .. }
        | MembershipEvent::InvitationRevoked { .. }
        | MembershipEvent::Deactivated { .. }
        | MembershipEvent::Removed { .. } => return Ok(()),
    };
    if recorded == effective {
        Ok(())
    } else {
        Err(RecordRefusal::IndependenceMismatch)
    }
}

/// The writer's check, before commit, that `next` follows `last`, the stream's last membership
/// record, if any (DEC-659): its `seq` is above `last`'s and its `event_time` is not before it, so
/// an equal `event_time` passes. The first membership record (`last` is `None`) passes, and every
/// record type is checked alike. A writer that calls it never commits a record the fold reads as
/// out of order (DEC-657 item 4), which stays the fold's backstop.
pub fn check_order(
    last: Option<&MembershipRecord>,
    next: &MembershipRecord,
) -> Result<(), RecordRefusal> {
    match last {
        Some(before) if !follows(before, next) => Err(RecordRefusal::OutOfOrder),
        Some(_) | None => Ok(()),
    }
}
