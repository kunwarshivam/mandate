//! E9-7 M1c (DEC-657): ID-7 and §5.1 over random membership histories against an oracle written
//! here, apart from the crate; and ID-3 (E9-11), the real `authorize` interleaved with those
//! histories against the same oracle and the §4.2 matrix parsed from the spec.

#[allow(dead_code, reason = "the parser's rows carry cells ID-3 does not read")]
#[path = "../src/tests/grammar.rs"]
mod grammar;
mod membership;
#[allow(
    dead_code,
    reason = "the spec's row map holds tables ID-3 does not read"
)]
#[path = "../src/tests/rows.rs"]
mod rows;

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

use grammar::{Row, matrix};
use mandate_identity::{
    Authorized, InvitationId, LookupFailed, Membership, MembershipEvent as Event, MembershipFold,
    MembershipLookup, MembershipQuery, MembershipState, OrgId, Permission, Principal, PrincipalId,
    Role, Scope, Session, SessionKind, SessionRef, StepUp, WorkspaceId, authorize,
};
use mandate_identity_seal::{LookupSeal, Seal};
use mandate_time::UtcNanos;
use membership::{
    B, DAY, EPOCH, FOUNDER, activated, cool, fold, founding, invited, reactivated, set, t,
};
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestRunner};
use rows::{ORG_ROLES, ROLE_COLUMNS};

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

/// The records `ops` name after the founding grant, folded by the crate, and the oracle after each
/// one with its instant, and the last record's instant.
fn history(ops: Vec<Op>) -> (MembershipFold, Vec<(i64, Oracle)>, i64) {
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
    (fold(events), snapshots, at)
}

fn op() -> impl Strategy<Value = Op> {
    let fitted = proptest::bool::weighted(0.9);
    (
        0..7u8,
        1..3usize,
        1..4u128,
        1..32u8,
        any::<bool>(),
        fitted,
        1..4_000i64,
    )
}

/// ID-7 and §5.1 over random histories: at every probe, `workspace_users`, the unreadable latch,
/// and each member's state, effective roles, and kept roles equal the oracle's own fold, so a
/// member the oracle reads deactivated or removed is granted nothing and counts zero.
#[test]
fn the_fold_equals_an_independent_oracle_over_random_histories() {
    let strategy = proptest::collection::vec(op(), 1..32);
    let config = ProptestConfig {
        cases: 512,
        failure_persistence: None,
        ..Default::default()
    };
    let body = |ops: Vec<Op>| -> Result<(), TestCaseError> {
        let (fold, snapshots, at) = history(ops);
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

const ORG: OrgId = OrgId(0x11);
const W: WorkspaceId = WorkspaceId(0x21);
const SCOPE: Scope = Scope::Workspace {
    org: ORG,
    workspace: W,
};

/// Who asks: the histories' members and a user that never holds a membership in W.
const ASKERS: [u128; 4] = [FOUNDER, B, 0xC, 0xD];

/// The system under test: the real `authorize` over an honest read of the fold, or a planted bug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Flaw {
    Honest,
    /// A membership read that memoizes the first membership it finds per member and keeps
    /// serving it after later records.
    CachedRead,
    /// A reach rule that admits `deactivated`, its kept roles with it.
    AdmitsDeactivated,
    /// Grants every request.
    AlwaysAllows,
}

/// The workspace store's membership read over the crate's fold, at the instant it serves: the
/// member's §5.1 state, with its effective roles, or a deactivated member's kept ones, which the
/// store holds for a reactivation; refused while the fold is unreadable.
struct FoldLookup<'a> {
    fold: &'a MembershipFold,
    at: UtcNanos,
    flaw: Flaw,
    served: &'a RefCell<BTreeMap<PrincipalId, Vec<Membership>>>,
}

impl FoldLookup<'_> {
    fn read(&self, member: PrincipalId) -> Vec<Membership> {
        let Some(state) = self.fold.state(member, self.at) else {
            return Vec::new();
        };
        let (state, roles) = match state {
            MembershipState::Deactivated => {
                let kept = self.fold.kept_roles(member, self.at);
                match self.flaw {
                    Flaw::AdmitsDeactivated => (MembershipState::Active, kept),
                    _ => (state, kept),
                }
            }
            _ => (state, self.fold.effective_roles(member, self.at)),
        };
        let roles = roles.into_iter().map(|r| (r, self.at)).collect();
        vec![Membership::new(Seal::grant(), member, SCOPE, state, roles)]
    }
}

impl LookupSeal for FoldLookup<'_> {}

impl MembershipLookup for FoldLookup<'_> {
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        let Some(member) = query.member else {
            return Err(LookupFailed);
        };
        if self.fold.unreadable(self.at) {
            return Err(LookupFailed);
        }
        let cached = self.served.borrow().get(&member).cloned();
        if let (Flaw::CachedRead, Some(first)) = (self.flaw, cached) {
            return Ok(first);
        }
        let read = self.read(member);
        if !read.is_empty() {
            self.served.borrow_mut().insert(member, read.clone());
        }
        Ok(read)
    }

    fn workspaces(&self, _: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Ok(BTreeSet::from([W]))
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok((workspace == W).then_some(ORG))
    }
}

/// The oracle's answer: whether the asker's seat reaches W by the oracle's own rule (`active` or
/// `cooling_off`), and the row's step-up when one of its effective workspace roles has the row in
/// the parsed matrix; nothing while the oracle reads the history as broken.
fn expected(o: &Oracle, row: &Row, asker: u128, at: i64) -> (bool, Option<StepUp>) {
    let (state, effective, _) = o.reading(asker, at);
    let reaches = matches!(
        state,
        Some(MembershipState::Active | MembershipState::CoolingOff)
    );
    let granted = ROLE_COLUMNS.iter().any(|(column, role)| {
        !ORG_ROLES.contains(role) && effective.contains(role) && row.grants(column, false)
    });
    let allowed = !o.broken && !row.inactive && reaches && granted;
    (reaches, allowed.then_some(row.step_up))
}

/// One history, probed in time order at each record, a second before and at a day after it, and a
/// week past the last; at each probe every (asker, row) pair of `asks` goes through the system.
fn interleave(
    flaw: Flaw,
    rows: &[Row],
    ops: Vec<Op>,
    asks: &[(usize, usize)],
) -> Result<(), TestCaseError> {
    let (fold, snapshots, end) = history(ops);
    let mut probes: Vec<i64> = snapshots
        .iter()
        .flat_map(|(at, _)| [*at, at + DAY - 1, at + DAY])
        .chain([end + 8 * DAY])
        .collect();
    probes.sort_unstable();
    probes.dedup();
    let served = RefCell::new(BTreeMap::new());
    let session = Session::new(
        Seal::grant(),
        SessionRef(0x41),
        SessionKind::Full,
        Vec::new(),
    );
    for p in probes {
        let o = &snapshots.iter().rev().find(|(at, _)| *at <= p).unwrap().1;
        let (lookup, now) = (
            FoldLookup {
                fold: &fold,
                at: t(p),
                flaw,
                served: &served,
            },
            t(p),
        );
        for &(who, which) in asks {
            let (asker, row) = (ASKERS[who], &rows[which % rows.len()]);
            let user = Principal::User {
                id: PrincipalId(asker),
            };
            let got = match flaw {
                Flaw::AlwaysAllows => Some(row.step_up),
                _ => authorize(&lookup, &user, &session, SCOPE, row.permission, now)
                    .ok()
                    .as_ref()
                    .map(Authorized::step_up),
            };
            let (reaches, want) = expected(o, row, asker, p);
            prop_assert!(
                reaches || got.is_none(),
                "ID-3: {:?} authorized for {} at {} with no membership reaching W",
                row.permission,
                asker,
                p
            );
            prop_assert_eq!(got, want, "{:?} for {} at {}", row.permission, asker, p);
        }
    }
    Ok(())
}

fn run(flaw: Flaw) -> Result<(), String> {
    let rows = matrix();
    let asks = proptest::collection::vec((0..ASKERS.len(), 0..64usize), 4..10);
    let strategy = (proptest::collection::vec(op(), 1..32), asks);
    let config = ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..Default::default()
    };
    TestRunner::new(config)
        .run(&strategy, |(ops, asks)| interleave(flaw, &rows, ops, &asks))
        .map_err(|failure| failure.to_string())
}

/// ID-3 over random histories interleaved with requests: the real `authorize`, reading the fold's
/// state at each request's instant, allows exactly what the oracle's seat fold, its reach rule, and
/// the §4.2 matrix parsed from the spec allow, so no request authorized at or after a deactivation
/// or a removal succeeds, and a role grants nothing in W before its cool-off ends (§8.3).
#[test]
fn id3_no_request_authorized_after_a_deactivation_reaches_the_workspace() {
    if let Err(failure) = run(Flaw::Honest) {
        panic!("{failure}");
    }
}

/// The oracle is shown to fail each planted bug: a cached membership read, a reach rule that admits
/// `deactivated`, and a step that allows everything.
#[test]
fn id3_the_oracle_catches_each_planted_bug() {
    for flaw in [
        Flaw::CachedRead,
        Flaw::AdmitsDeactivated,
        Flaw::AlwaysAllows,
    ] {
        assert!(run(flaw).is_err(), "the ID-3 oracle missed {flaw:?}");
    }
}

/// The same check on one history, B invited and activated as a viewer and deactivated a minute
/// later, with B and the outsider asking to view agents: the real step refuses both after the
/// deactivation, and each planted bug is caught granting a request with no membership reaching W.
#[test]
fn id3_a_deactivated_viewer_is_refused_and_each_planted_bug_is_caught_granting_it() {
    let rows = matrix();
    let view = rows
        .iter()
        .position(|row| row.permission == Permission::ViewAgents)
        .unwrap();
    let viewer = 0b01000;
    let ops: Vec<Op> = [0, 0, 1]
        .map(|kind| (kind, 1, 1, viewer, false, true, 1))
        .into();
    let asks = [(1, view), (3, view)];
    if let Err(failure) = interleave(Flaw::Honest, &rows, ops.clone(), &asks) {
        panic!("{failure}");
    }
    for flaw in [
        Flaw::CachedRead,
        Flaw::AdmitsDeactivated,
        Flaw::AlwaysAllows,
    ] {
        let failure = interleave(flaw, &rows, ops.clone(), &asks).unwrap_err();
        assert!(failure.to_string().contains("ID-3:"), "{flaw:?}: {failure}");
    }
}
