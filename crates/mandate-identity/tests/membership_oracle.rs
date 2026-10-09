//! E9-7 M1c (DEC-657): ID-7 and §5.1 over random membership histories against an oracle written
//! here, apart from the crate.

mod membership;

use std::collections::{BTreeMap, BTreeSet};

use mandate_identity::{
    InvitationId, MembershipEvent as Event, MembershipState, PrincipalId, Role,
};
use mandate_time::UtcNanos;
use membership::{
    B, DAY, EPOCH, FOUNDER, activated, cool, fold, founding, invited, reactivated, set, t,
};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};

const MEMBERS: [u128; 3] = [FOUNDER, B, 0xC];

fn secs(at: UtcNanos) -> i64 {
    at.secs() - EPOCH
}

type Roles = BTreeSet<Role>;

/// A seat as the oracle keeps it: live from the end of its activation's cool-off with each role's
/// own start, off with the roles it keeps, or gone.
#[derive(Clone, Debug)]
enum Seat {
    Live(i64, BTreeMap<Role, i64>),
    Off(BTreeSet<Role>),
    Gone,
}

/// The oracle: seats, invitations as (roles, expiry, still open), and whether a record misfit.
#[derive(Clone, Debug, Default)]
struct Oracle {
    seats: BTreeMap<u128, Seat>,
    invites: BTreeMap<u128, (BTreeSet<Role>, i64, bool)>,
    broken: bool,
}

fn live(at: i64, end: i64, roles: &BTreeSet<Role>) -> Seat {
    let start = |r: &Role| match r {
        Role::Approver | Role::Operator => end,
        _ => at,
    };
    Seat::Live(end, roles.iter().map(|r| (*r, start(r))).collect())
}

impl Oracle {
    fn step(&mut self, at: i64, event: &Event) {
        self.broken = self.broken || !self.fits(at, event);
    }

    fn fits(&mut self, at: i64, event: &Event) -> bool {
        match event {
            Event::Invited {
                invitation,
                roles,
                expires_at,
            } => {
                let invite = (roles.clone(), secs(*expires_at), true);
                self.invites.insert(invitation.0, invite).is_none()
            }
            Event::InvitationRevoked { invitation } => match self.invites.get_mut(&invitation.0) {
                Some((_, expiry, open)) if *open && at < *expiry => std::mem::replace(open, false),
                _ => false,
            },
            Event::Activated {
                member,
                invitation,
                roles,
                cool_off_ends_at,
                ..
            } => {
                let fresh = matches!(self.seats.get(&member.0), None | Some(Seat::Gone));
                let accepted = invitation.is_none_or(|i| match self.invites.get_mut(&i.0) {
                    Some((named, expiry, open)) if *open && at < *expiry && named == roles => {
                        std::mem::replace(open, false)
                    }
                    _ => false,
                });
                if fresh && accepted {
                    self.seats
                        .insert(member.0, live(at, secs(*cool_off_ends_at), roles));
                }
                fresh && accepted
            }
            Event::RoleChanged {
                member,
                added,
                removed,
                ..
            } => match self.seats.get_mut(&member.0) {
                Some(Seat::Live(_, held)) => {
                    let fits = removed.iter().all(|r| held.contains_key(r))
                        && added.keys().all(|r| !held.contains_key(r));
                    held.retain(|r, _| !removed.contains(r));
                    held.extend(added.iter().map(|(r, end)| (*r, secs(*end))));
                    fits
                }
                Some(Seat::Off(kept)) => {
                    let fits = added.is_empty() && removed.is_subset(kept);
                    kept.retain(|r| !removed.contains(r));
                    fits
                }
                _ => false,
            },
            Event::Deactivated { member } => match self.seats.get(&member.0) {
                Some(Seat::Live(_, held)) => {
                    let kept = held.keys().copied().collect();
                    self.seats.insert(member.0, Seat::Off(kept));
                    true
                }
                _ => false,
            },
            Event::Reactivated {
                member,
                roles,
                cool_off_ends_at,
                ..
            } => match self.seats.get_mut(&member.0) {
                Some(seat @ Seat::Off(_)) if !roles.is_empty() => {
                    let restores = matches!(seat, Seat::Off(kept) if kept == roles);
                    *seat = live(at, secs(*cool_off_ends_at), roles);
                    restores
                }
                _ => false,
            },
            Event::Removed { member } => match self.seats.get_mut(&member.0) {
                Some(seat @ Seat::Off(_)) => {
                    *seat = Seat::Gone;
                    true
                }
                _ => false,
            },
        }
    }

    fn count(&self, at: i64) -> u32 {
        let active = self
            .seats
            .values()
            .filter(|s| matches!(s, Seat::Live(from, _) if *from <= at));
        if self.broken {
            1
        } else {
            u32::try_from(active.count()).unwrap()
        }
    }

    /// A member's state, effective roles, and kept roles at `at`.
    fn reading(&self, member: u128, at: i64) -> (Option<MembershipState>, Roles, Roles) {
        let none = BTreeSet::new;
        match self.seats.get(&member) {
            None => (None, none(), none()),
            Some(Seat::Live(from, held)) => {
                let state = match *from <= at {
                    true => MembershipState::Active,
                    false => MembershipState::CoolingOff,
                };
                let effective = held.iter().filter(|(_, s)| **s <= at).map(|(r, _)| *r);
                (Some(state), effective.collect(), none())
            }
            Some(Seat::Off(kept)) => (Some(MembershipState::Deactivated), none(), kept.clone()),
            Some(Seat::Gone) => (Some(MembershipState::Removed), none(), none()),
        }
    }
}

/// One random op: kind, member, invitation, role mask, independence, whether it is fitted to the
/// oracle's state, and the minutes since the previous record.
type Op = (u8, usize, u128, u8, bool, bool, i64);

/// The record an op names. A fitted op (most of them) picks a transition the member's state allows,
/// so histories run long; an unfitted one is raw, so misfits and the latch are exercised too.
fn event(o: &Oracle, at: i64, &(kind, who, inv, mask, ind, fitted, _): &Op) -> Option<Event> {
    let member = MEMBERS[who];
    let seat = o.seats.get(&member);
    let held: BTreeSet<Role> = match seat {
        Some(Seat::Live(_, held)) => held.keys().copied().collect(),
        Some(Seat::Off(kept)) => kept.clone(),
        _ => BTreeSet::new(),
    };
    let open = o
        .invites
        .iter()
        .find(|(_, (_, expiry, open))| *open && at < *expiry);
    let kind = match (fitted, seat) {
        (false, _) => kind,
        (true, None | Some(Seat::Gone)) => {
            if open.is_some() {
                2
            } else {
                0
            }
        }
        (true, Some(Seat::Live(..))) => [3, 4, 0, 1][usize::from(kind % 4)],
        (true, Some(Seat::Off(_))) => [5, 6, 3, 5][usize::from(kind % 4)],
    };
    let (inv, named) = match open.filter(|_| fitted) {
        Some((i, (roles, ..))) => (*i, roles.clone()),
        None => (inv, set(mask)),
    };
    let fresh = o.invites.keys().max().map_or(1, |i| i + 1);
    Some(match kind {
        0 => invited(if fitted { fresh } else { inv }, set(mask), at),
        1 => Event::InvitationRevoked {
            invitation: InvitationId(inv),
        },
        2 => activated(member, Some(inv), named, ind, at),
        3 => {
            let (grant, removed) = match fitted {
                true => (&set(mask) - &held, &held - &set(mask)),
                false => (set(mask), BTreeSet::new()),
            };
            let off = fitted && matches!(seat, Some(Seat::Off(_)));
            let one = |r: &Role| (*r, cool(at, ind, &BTreeSet::from([*r])));
            let added: BTreeMap<Role, UtcNanos> = grant.iter().filter(|_| !off).map(one).collect();
            if added.is_empty() && removed.is_empty() {
                return None;
            }
            let (member, independent_approval_required) = (PrincipalId(member), ind);
            Event::RoleChanged {
                member,
                added,
                removed,
                independent_approval_required,
            }
        }
        4 => Event::Deactivated {
            member: PrincipalId(member),
        },
        5 => match if fitted { held } else { set(mask) } {
            roles if roles.is_empty() => return None,
            roles => reactivated(member, roles, ind, at),
        },
        _ => Event::Removed {
            member: PrincipalId(member),
        },
    })
}

/// ID-7 and §5.1 over random histories: at every probe, `workspace_users`, the unreadable latch,
/// and each member's state, effective roles, and kept roles equal the oracle's own fold, so a
/// member the oracle reads deactivated or removed is granted nothing and counts zero.
#[test]
#[ignore = "pending E9-7"]
fn the_fold_equals_an_independent_oracle_over_random_histories() {
    let fitted = proptest::bool::weighted(0.9);
    let op = (
        0..7u8,
        1..3usize,
        1..4u128,
        1..32u8,
        any::<bool>(),
        fitted,
        1..4_000i64,
    );
    let strategy = proptest::collection::vec(op, 1..32);
    let config = ProptestConfig {
        cases: 512,
        failure_persistence: None,
        ..Default::default()
    };
    let body = |ops: Vec<Op>| -> Result<(), TestCaseError> {
        let mut oracle = Oracle::default();
        let mut events = vec![founding()];
        oracle.step(0, &events[0].1);
        let (mut snapshots, mut at) = (vec![(0, oracle.clone())], 0);
        for op in ops {
            at += op.6 * 60;
            if let Some(e) = event(&oracle, at, &op) {
                oracle.step(at, &e);
                snapshots.push((at, oracle.clone()));
                events.push((at, e));
            }
        }
        let fold = fold(events);
        let probes = snapshots
            .iter()
            .flat_map(|(at, _)| [*at, at + DAY - 1, at + DAY]);
        for p in probes.chain([at + 8 * DAY]) {
            let o = &snapshots.iter().rev().find(|(at, _)| *at <= p).unwrap().1;
            prop_assert_eq!(fold.workspace_users(t(p)), o.count(p), "ID-7 at {}", p);
            prop_assert_eq!(fold.unreadable(t(p)), o.broken, "unreadable at {}", p);
            for m in MEMBERS.into_iter().filter(|_| !o.broken) {
                let (id, (state, effective, kept)) = (PrincipalId(m), o.reading(m, p));
                let got = (fold.state(id, t(p)), fold.effective_roles(id, t(p)));
                let got = (got.0, got.1, fold.kept_roles(id, t(p)));
                prop_assert_eq!(got, (state, effective, kept), "member {} at {}", m, p);
            }
        }
        Ok(())
    };
    if let Err(failure) = TestRunner::new(config).run(&strategy, body) {
        panic!("{failure}");
    }
}
