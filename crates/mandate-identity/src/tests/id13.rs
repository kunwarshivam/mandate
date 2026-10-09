//! E9-11, ID-13: roles change only by journaled membership events, and never by their holder
//! (identity spec §2, §5; DEC-643, DEC-654). A fuzz of workspace role commands runs each through
//! the real `change_roles`, journals each accepted one as the `MemberRoleChanged` and
//! `MemberDeactivated` records a writer builds from the fold's state, and folds them with the real
//! `MembershipFold`. After every command the fold's effective and kept roles must equal an
//! accumulator this file keeps from the accepted changes alone, no accepted change may name its
//! author in a grant or a removal, and the outcome must be the one written here from the
//! accumulator and the spec's order of refusals (§4.5), never from the crate's own checks. It runs
//! at workspace scope only, the scope the fold covers, so the org-scope `last_owner` rule and
//! `owner_role_reserved` are not exercised here.

use std::collections::{BTreeMap, BTreeSet};

use proptest::prelude::*;
use proptest::strategy::ValueTree;
use proptest::test_runner::{Config, TestRunner};

use mandate_identity_seal::Seal;
use mandate_time::UtcNanos;

use crate::Role::{Approver, Auditor, Operator, Viewer, WorkspaceAdmin};
use crate::{
    Membership, MembershipEvent, MembershipFold, MembershipRecord, MembershipState, Principal,
    PrincipalId, Refusal, Role, RoleChange, Scope, Session, SessionKind, SessionRef, StepUp,
    change_roles,
};

use super::{ByMember, O1, W1};

const WS: Scope = Scope::Workspace {
    org: O1,
    workspace: W1,
};
const START_SECS: i64 = 1_790_000_000;
const MEMBERS: [PrincipalId; 4] = [
    PrincipalId(1),
    PrincipalId(2),
    PrincipalId(3),
    PrincipalId(4),
];
const ROLES: [Role; 5] = [WorkspaceAdmin, Operator, Approver, Viewer, Auditor];

type Outcome = Result<StepUp, Refusal>;
/// A subject, a role, and an action: 0 grants the role, 1 removes it, 2 deactivates the subject.
type Entry = (usize, usize, u8);
/// The author, the command's entries, and the seconds since the command before.
type Command = (usize, Vec<Entry>, u8);
/// What decides a command: the store, the author, the change, and the instant.
type Decide<'f> = &'f dyn Fn(&[Membership], PrincipalId, &RoleChange, UtcNanos) -> Outcome;
/// Each member: whether it is live (not deactivated), and the roles it holds or keeps.
type Oracle = BTreeMap<PrincipalId, (bool, BTreeSet<Role>)>;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(START_SECS + secs, 0).unwrap()
}

fn commands() -> impl Strategy<Value = Vec<Command>> {
    let entries = prop::collection::vec((0usize..4, 0usize..5, 0u8..3), 1..4);
    prop::collection::vec((0usize..4, entries, 0u8..3), 1..16)
}

/// The founding memberships: two admins, an operator-approver, and a viewer, none cooling.
fn founding() -> Oracle {
    let roles: [&[Role]; 4] = [
        &[WorkspaceAdmin],
        &[WorkspaceAdmin, Operator],
        &[Operator, Approver],
        &[Viewer],
    ];
    MEMBERS
        .iter()
        .zip(roles)
        .map(|(id, roles)| (*id, (true, roles.iter().copied().collect())))
        .collect()
}

/// The change the entries name; a later grant or removal of the same pair replaces an earlier one.
fn change_of(entries: &[Entry]) -> RoleChange {
    let mut change = RoleChange::default();
    let mut last = BTreeMap::new();
    for (subject, role, action) in entries {
        match action {
            2 => {
                change.deactivations.insert(MEMBERS[*subject]);
            }
            _ => {
                last.insert((MEMBERS[*subject], ROLES[*role]), *action);
            }
        }
    }
    for (pair, action) in last {
        match action {
            0 => change.grants.insert(pair),
            _ => change.removals.insert(pair),
        };
    }
    change
}

/// The outcome the spec gives the change on the accumulator's state (§4.2, §4.5, ID-13, §5.2).
fn expected(oracle: &Oracle, author: PrincipalId, change: &RoleChange) -> Outcome {
    let (live, roles) = &oracle[&author];
    let entries = || change.grants.iter().chain(&change.removals);
    let roles_row = entries().next().is_some() || change.deactivations.is_empty();
    let members_row = change.deactivations.iter().any(|d| *d != author);
    let leaving = change.deactivations.contains(&author);
    if !live {
        return Err(Refusal::NoMembership);
    }
    let admin = roles.contains(&WorkspaceAdmin);
    if ((roles_row || members_row) && !admin) || (leaving && roles.is_empty()) {
        return Err(Refusal::Forbidden);
    }
    if change.grants.iter().any(|(who, _)| !oracle[who].0) {
        return Err(Refusal::Forbidden);
    }
    if entries().any(|(who, _)| *who == author) {
        return Err(Refusal::OwnRoles);
    }
    let an_admin_stays = oracle.iter().any(|(id, (live, roles))| {
        let holds =
            roles.contains(&WorkspaceAdmin) && !change.removals.contains(&(*id, WorkspaceAdmin));
        *live
            && !change.deactivations.contains(id)
            && (holds || change.grants.contains(&(*id, WorkspaceAdmin)))
    });
    match (an_admin_stays, change.grants.is_empty()) {
        (false, _) => Err(Refusal::LastAdmin),
        (true, true) => Ok(StepUp::NotRequired),
        (true, false) => Ok(StepUp::Required),
    }
}

/// The accumulator after an accepted change: grants added, removals taken, deactivations ended.
fn accumulate(oracle: &mut Oracle, change: &RoleChange) {
    for (who, role) in &change.grants {
        oracle.get_mut(who).unwrap().1.insert(*role);
    }
    for (who, role) in &change.removals {
        oracle.get_mut(who).unwrap().1.remove(role);
    }
    for who in &change.deactivations {
        oracle.get_mut(who).unwrap().0 = false;
    }
}

/// The store `change_roles` reads, as workspace services read it from the fold at `now`.
fn store_of(fold: &MembershipFold, now: UtcNanos) -> Vec<Membership> {
    let since = |roles: BTreeSet<Role>| roles.into_iter().map(|r| (r, UtcNanos::EPOCH)).collect();
    MEMBERS
        .iter()
        .map(|id| {
            let state = fold.state(*id, now).unwrap();
            let roles = match state {
                MembershipState::Deactivated => since(fold.kept_roles(*id, now)),
                _ => since(fold.effective_roles(*id, now)),
            };
            Membership {
                member: *id,
                scope: WS,
                state,
                roles,
            }
        })
        .collect()
}

/// The records a writer journals for an accepted change, from the fold's state at `now`: one
/// `MemberRoleChanged` per member whose roles it changes (no cool-off, independence off), then a
/// `MemberDeactivated` per live member it deactivates. `drop_removals` is a planted writer bug.
fn events_of(
    fold: &MembershipFold,
    change: &RoleChange,
    now: UtcNanos,
    drop_removals: bool,
) -> Vec<MembershipEvent> {
    let mut events = Vec::new();
    for member in MEMBERS {
        let held: BTreeSet<Role> = fold
            .effective_roles(member, now)
            .union(&fold.kept_roles(member, now))
            .copied()
            .collect();
        let added: BTreeMap<Role, UtcNanos> = change
            .grants
            .iter()
            .filter(|(w, r)| *w == member && !held.contains(r))
            .map(|(_, r)| (*r, now))
            .collect();
        let removed: BTreeSet<Role> = change
            .removals
            .iter()
            .filter(|(w, r)| *w == member && held.contains(r) && !drop_removals)
            .map(|(_, r)| *r)
            .collect();
        if !added.is_empty() || !removed.is_empty() {
            events.push(MembershipEvent::RoleChanged {
                member,
                added,
                removed,
                independent_approval_required: false,
            });
        }
    }
    for member in &change.deactivations {
        if fold.state(*member, now) == Some(MembershipState::Active) {
            events.push(MembershipEvent::Deactivated { member: *member });
        }
    }
    events
}

/// What one run of commands found: how many changes were accepted and refused, and each breach.
#[derive(Debug, Default)]
struct Run {
    accepted: u64,
    refusals: Vec<Refusal>,
    breaches: Vec<String>,
}

/// Runs the commands through `decide`, journals and folds each accepted change, and checks the
/// three clauses of ID-13's test after every command.
fn run(commands: &[Command], decide: Decide<'_>, drop_removals: bool) -> Run {
    let mut oracle = founding();
    let mut records: Vec<MembershipRecord> = MEMBERS
        .iter()
        .zip(1u64..)
        .map(|(member, seq)| {
            let roles = oracle[member].1.clone();
            let event = MembershipEvent::Activated {
                member: *member,
                invitation: None,
                roles,
                independent_approval_required: false,
                cool_off_ends_at: at(0),
            };
            MembershipRecord::new(Seal::grant(), seq, at(0), event)
        })
        .collect();
    let (mut found, mut secs) = (Run::default(), 0i64);
    for (step, (author, entries, delay)) in commands.iter().enumerate() {
        secs += i64::from(*delay);
        let (now, author, change) = (at(secs), MEMBERS[*author], change_of(entries));
        let fold = MembershipFold::new(Seal::grant(), records.clone());
        let got = decide(&store_of(&fold, now), author, &change, now);
        let want = expected(&oracle, author, &change);
        if got != want {
            found.breaches.push(format!(
                "(c) step {step}: {author:?} {change:?}: got {got:?}, the spec says {want:?}"
            ));
        }
        if let Err(code) = got {
            found.refusals.push(code);
            continue;
        }
        found.accepted += 1;
        if change
            .grants
            .iter()
            .chain(&change.removals)
            .any(|(who, _)| *who == author)
        {
            found.breaches.push(format!(
                "(b) step {step}: {author:?} changed its own roles: {change:?}"
            ));
        }
        accumulate(&mut oracle, &change);
        for event in events_of(&fold, &change, now, drop_removals) {
            let seq = records.last().unwrap().seq() + 1;
            records.push(MembershipRecord::new(Seal::grant(), seq, now, event));
        }
        let fold = MembershipFold::new(Seal::grant(), records.clone());
        for (id, (live, roles)) in &oracle {
            let (effective, kept) = match live {
                true => (roles.clone(), BTreeSet::new()),
                false => (BTreeSet::new(), roles.clone()),
            };
            let folded = (fold.effective_roles(*id, now), fold.kept_roles(*id, now));
            if fold.unreadable(now) || folded != (effective.clone(), kept.clone()) {
                found.breaches.push(format!("(a) step {step}: {id:?} folds to {folded:?}, accumulated {effective:?} kept {kept:?}"));
            }
        }
    }
    found
}

fn real(store: &[Membership], author: PrincipalId, change: &RoleChange, now: UtcNanos) -> Outcome {
    let (user, snapshot) = (Principal::User { id: author }, Vec::new());
    let (reference, kind) = (SessionRef(0x41), SessionKind::Full);
    let session = Session {
        reference,
        kind,
        snapshot,
    };
    change_roles(&ByMember(store), &user, &session, WS, change, now)
}

/// ID-13's required test: a fuzz of membership commands whose oracle role state equals the fold
/// of the journaled `Member*` records, with no accepted change naming its author as a subject.
#[test]
fn id13_role_state_equals_the_fold_and_no_change_names_its_author() {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    let outcome = TestRunner::new(config).run(&commands(), |commands| {
        let found = run(&commands, &real, false);
        prop_assert!(found.breaches.is_empty(), "{:?}", found.breaches);
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}

/// A planted `change_roles` that drops the own-roles check for the author's grants (`grants`) or
/// removals, accepting the change whenever the rest of it passes.
fn ignoring_own(
    grants: bool,
) -> impl Fn(&[Membership], PrincipalId, &RoleChange, UtcNanos) -> Outcome {
    move |store, author, change, now| {
        let honest = real(store, author, change, now);
        let mut rest = change.clone();
        let (skipped, other) = match grants {
            true => (&mut rest.grants, &change.removals),
            false => (&mut rest.removals, &change.grants),
        };
        skipped.retain(|(who, _)| *who != author);
        match honest == Err(Refusal::OwnRoles) && other.iter().all(|(who, _)| *who != author) {
            true => real(store, author, &rest, now),
            false => honest,
        }
    }
}

/// The oracle agrees with the real path on a deterministic sample that accepts and refuses
/// changes, own-roles and last-admin refusals among them, and finds a breach under each planted
/// bug: the own-roles check skipped for removals, the same check letting a self-grant through, a
/// `change_roles` that accepts everything, and a writer that journals no removals.
#[test]
fn id13_oracle_catches_each_planted_bug() {
    let mut runner = TestRunner::deterministic();
    let samples: Vec<Vec<Command>> = (0..200)
        .map(|_| commands().new_tree(&mut runner).unwrap().current())
        .collect();
    let tally = |decide: Decide<'_>, drop_removals: bool| {
        samples.iter().fold(Run::default(), |mut sum, commands| {
            let found = run(commands, decide, drop_removals);
            sum.accepted += found.accepted;
            sum.refusals.extend(found.refusals);
            sum.breaches.extend(found.breaches);
            sum
        })
    };
    let honest = tally(&real, false);
    assert!(
        honest.breaches.is_empty(),
        "the real path breaches: {:?}",
        honest.breaches
    );
    let reached = |code| honest.refusals.contains(&code);
    assert!(
        honest.accepted > 0 && reached(Refusal::OwnRoles) && reached(Refusal::LastAdmin),
        "the sample must accept changes and refuse some own_roles and some last_admin"
    );
    let accept_all =
        |_: &[Membership], _: PrincipalId, _: &RoleChange, _: UtcNanos| Ok(StepUp::Required);
    let (removals, grants) = (ignoring_own(false), ignoring_own(true));
    let planted: [(&str, Decide<'_>, bool, &str); 4] = [
        ("own-roles skipped for removals", &removals, false, "(b)"),
        ("own-roles lets a self-grant through", &grants, false, "(b)"),
        ("every change accepted unchecked", &accept_all, false, "(c)"),
        ("writer journals no removals", &real, true, "(a)"),
    ];
    for (bug, decide, drop_removals, clause) in planted {
        let found = tally(decide, drop_removals);
        let caught = found.breaches.iter().any(|b| b.starts_with(clause));
        assert!(caught, "clause {clause} misses the planted bug: {bug}");
    }
}
