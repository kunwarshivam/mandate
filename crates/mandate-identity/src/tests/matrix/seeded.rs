//! E9-11: the ID-2 and ID-5 oracles are shown to fail on a seeded bug before they are trusted.
//!
//! Each planted bug wraps the real `authorize`, or the membership store it reads, and perturbs its
//! answer. Every test sweeps a sample of the exhaustive space of `matrix.rs` (every third role
//! set for ID-2, with fewer membership states and cool-offs than the full ID-2 run), and asserts
//! that the oracle agrees with the real `authorize` on every case and disagrees with the planted
//! bug on at least one.

use std::collections::BTreeSet;

use mandate_identity_seal::LookupSeal;
use mandate_time::UtcNanos;

use super::super::{ByMember, Failing, W1, hosted_in, record_of};
use super::{
    Expected, OTHER, Row, SCOPES, SESSION_KINDS, USER, a_day_before_now, expected_in, matrix,
    memberships_since, now, outage_expected, session, subsets, wide_snapshot,
};
use crate::{
    Authorized, LookupFailed, Membership, MembershipLookup, MembershipQuery, MembershipState,
    OrgId, Permission, Principal, Refusal, Role, Scope, Session, SessionKind, StepUp, WorkspaceId,
    authorize,
};

/// What the store answers in a case: the memberships live, or a failed read (identity spec §4.5).
#[derive(Clone, Copy)]
enum Read<'a> {
    Live(&'a [Membership]),
    Failed,
}

/// An implementation under test, answering one request as `authorize` would.
type Answer<'f> = &'f dyn Fn(Read<'_>, &Principal, &Session, Scope, Permission) -> Expected;

/// The answer `authorize` gives, as the oracles state it.
fn summary(got: Result<Authorized, Refusal>) -> Expected {
    got.map(|a| a.step_up())
}

/// The real `authorize`, reading the case's store.
fn real(read: Read<'_>, p: &Principal, s: &Session, scope: Scope, perm: Permission) -> Expected {
    match read {
        Read::Live(ms) => summary(authorize(&ByMember(ms), p, s, scope, perm, now())),
        Read::Failed => summary(authorize(&Failing, p, s, scope, perm, now())),
    }
}

/// How many cases the sweep ran, how many the real `authorize` failed, and how many the planted
/// bug failed.
#[derive(Debug, Default)]
struct Tally {
    cases: u64,
    real_misses: u64,
    planted_misses: u64,
}

/// The two principals that read memberships: the user, and a client acting for it in W1.
fn readers() -> [Principal; 2] {
    [
        Principal::User { id: USER },
        Principal::Client {
            id: OTHER,
            on_behalf_of: USER,
            workspace: W1,
        },
    ]
}

/// The ID-2 oracle over a live store: every third role set, three membership states, and roles
/// settled at `now` or still cooling, for the user and its client, at every scope, row, and
/// session kind.
fn id2_sweep(planted: Answer<'_>) -> Tally {
    let rows = matrix();
    let mut tally = Tally::default();
    for roles in subsets(&Role::ALL).into_iter().step_by(3) {
        for state in [
            MembershipState::Active,
            MembershipState::CoolingOff,
            MembershipState::Deactivated,
        ] {
            for cooling in [BTreeSet::new(), roles.clone()] {
                let ms = memberships_since(USER, &roles, &cooling, now(), state);
                for principal in readers() {
                    for_each_request(&rows, |row, scope, kind, session| {
                        let want = expected_in(row, &principal, &ms, scope, kind);
                        let ask = |f: Answer<'_>| {
                            f(Read::Live(&ms), &principal, session, scope, row.permission)
                        };
                        tally.count(&want, ask(&real), ask(planted));
                    });
                }
            }
        }
    }
    tally
}

/// The ID-5 oracle over every role set, settled a day before `now` or with the reviewable roles
/// cooling, for the user and its client, with the store failing and the session's snapshot
/// holding the user's memberships.
fn id5_sweep(planted: Answer<'_>) -> Tally {
    let rows = matrix();
    let reviewable = BTreeSet::from([Role::Operator, Role::Approver]);
    let mut tally = Tally::default();
    for roles in subsets(&Role::ALL) {
        for cooling in [
            BTreeSet::new(),
            roles.intersection(&reviewable).copied().collect(),
        ] {
            let snapshot: Vec<Membership> = memberships_since(
                USER,
                &roles,
                &cooling,
                a_day_before_now(),
                MembershipState::Active,
            )
            .into_iter()
            .filter(|m| m.member == USER)
            .collect();
            for principal in readers() {
                for kind in SESSION_KINDS {
                    let session = session(kind, snapshot.clone());
                    for row in &rows {
                        for scope in SCOPES {
                            let want = outage_expected(row, &principal, &snapshot, scope, kind);
                            let ask = |f: Answer<'_>| {
                                f(Read::Failed, &principal, &session, scope, row.permission)
                            };
                            tally.count(&want, ask(&real), ask(planted));
                        }
                    }
                }
            }
        }
    }
    tally
}

/// Calls `each` for every row, scope, and session kind, the session holding the wide snapshot a
/// live read must ignore.
fn for_each_request(rows: &[Row], mut each: impl FnMut(&Row, Scope, SessionKind, &Session)) {
    let sessions = SESSION_KINDS.map(|k| (k, session(k, wide_snapshot())));
    for row in rows {
        for (kind, session) in &sessions {
            for scope in SCOPES {
                each(row, scope, *kind, session);
            }
        }
    }
}

impl Tally {
    fn count(&mut self, want: &Expected, real: Expected, planted: Expected) {
        self.cases += 1;
        self.real_misses += u64::from(real != *want);
        self.planted_misses += u64::from(planted != *want);
    }

    /// Asserts the oracle agreed with the real `authorize` on the whole sweep and caught `bug`.
    fn assert_caught(&self, bug: &str) {
        assert!(self.cases > 50_000, "{bug}: only {} cases", self.cases);
        assert_eq!(
            self.real_misses, 0,
            "{bug}: the oracle disagrees with the real authorize, {self:?}"
        );
        assert!(
            self.planted_misses > 0,
            "{bug}: the oracle never catches the planted bug, {self:?}"
        );
    }
}

/// The planted bug of ID-2 (a): a workspace-scope request the real step refuses is granted when the
/// same row is granted at the workspace's organization, so a role held through the org membership
/// reaches its workspaces.
fn inherits_from_the_org(
    read: Read<'_>,
    p: &Principal,
    s: &Session,
    scope: Scope,
    perm: Permission,
) -> Expected {
    match (real(read, p, s, scope, perm), scope) {
        (Err(Refusal::NoMembership | Refusal::Forbidden), Scope::Workspace { org, .. }) => {
            real(read, p, s, Scope::Org(org), perm)
        }
        (answer, _) => answer,
    }
}

/// A store whose memberships hold every role as effective from the epoch, so a role still in its
/// §8.3 cool-off counts before the cool-off ends.
struct IgnoresCoolOff<'a>(&'a [Membership]);

impl LookupSeal for IgnoresCoolOff<'_> {}

impl MembershipLookup for IgnoresCoolOff<'_> {
    fn memberships(&self, query: &MembershipQuery) -> Result<Vec<Membership>, LookupFailed> {
        let early = |m: Membership| Membership {
            roles: m.roles.keys().map(|r| (*r, UtcNanos::EPOCH)).collect(),
            ..m
        };
        Ok(ByMember(self.0)
            .memberships(query)?
            .into_iter()
            .map(early)
            .collect())
    }

    fn workspaces(&self, org: OrgId) -> Result<BTreeSet<WorkspaceId>, LookupFailed> {
        Ok(hosted_in(org))
    }

    fn workspace_org(&self, workspace: WorkspaceId) -> Result<Option<OrgId>, LookupFailed> {
        Ok(record_of(workspace))
    }
}

/// The planted bug of ID-2 (b): the real step, reading a store that ignores cool-offs.
fn counts_cooling_roles(
    read: Read<'_>,
    p: &Principal,
    s: &Session,
    scope: Scope,
    perm: Permission,
) -> Expected {
    match read {
        Read::Live(ms) => summary(authorize(&IgnoresCoolOff(ms), p, s, scope, perm, now())),
        Read::Failed => real(read, p, s, scope, perm),
    }
}

/// The planted bug of ID-5 (a): pausing and engaging a kill switch ask for step-up.
fn steps_up_risk_reduction(
    read: Read<'_>,
    p: &Principal,
    s: &Session,
    scope: Scope,
    perm: Permission,
) -> Expected {
    let reduction = matches!(
        perm,
        Permission::Pause
            | Permission::KillSwitchAgent
            | Permission::KillSwitchConnectionOrWorkspace
            | Permission::KillSwitchOrg
    );
    real(read, p, s, scope, perm).map(|step_up| match reduction {
        true => StepUp::Required,
        false => step_up,
    })
}

/// The planted bug of ID-5 (b): a grant made while the membership read failed, from the session's
/// snapshot, is refused `membership_unavailable` instead.
fn refuses_on_a_failed_read(
    read: Read<'_>,
    p: &Principal,
    s: &Session,
    scope: Scope,
    perm: Permission,
) -> Expected {
    let got = match read {
        Read::Live(ms) => authorize(&ByMember(ms), p, s, scope, perm, now()),
        Read::Failed => authorize(&Failing, p, s, scope, perm, now()),
    };
    let unverified = match &got {
        Ok(Authorized::Principal { context, .. }) => context.membership_unverified(),
        Ok(Authorized::Org { context, .. }) => context.membership_unverified(),
        Ok(Authorized::Workspace { tenant, .. }) => tenant.membership_unverified(),
        Err(_) => false,
    };
    match unverified {
        true => Err(Refusal::MembershipUnavailable),
        false => summary(got),
    }
}

/// The do-nothing implementations: one that refuses everything, and one that grants everything.
fn always_deny(_: Read<'_>, _: &Principal, _: &Session, _: Scope, _: Permission) -> Expected {
    Err(Refusal::Forbidden)
}

fn always_allow(_: Read<'_>, _: &Principal, _: &Session, _: Scope, _: Permission) -> Expected {
    Ok(StepUp::NotRequired)
}

#[test]
fn id2_oracle_catches_a_role_inherited_from_the_org() {
    id2_sweep(&inherits_from_the_org).assert_caught("a role inherited from the org");
}

#[test]
fn id2_oracle_catches_a_cooling_off_role_counted_before_its_cool_off_ends() {
    id2_sweep(&counts_cooling_roles).assert_caught("a cooling-off role counted early");
}

#[test]
fn id2_oracle_catches_an_implementation_that_does_nothing() {
    id2_sweep(&always_deny).assert_caught("always deny");
    id2_sweep(&always_allow).assert_caught("always allow");
}

#[test]
fn id5_oracle_catches_step_up_on_pause_or_the_kill_switch() {
    id5_sweep(&steps_up_risk_reduction).assert_caught("step-up on pause or the kill switch");
    id2_sweep(&steps_up_risk_reduction).assert_caught("step-up on a live pause or kill switch");
}

#[test]
fn id5_oracle_catches_a_failed_membership_read_refusing_risk_reduction() {
    id5_sweep(&refuses_on_a_failed_read).assert_caught("a failed read refusing risk reduction");
}

#[test]
fn id5_oracle_catches_an_implementation_that_does_nothing() {
    id5_sweep(&always_deny).assert_caught("always deny");
    id5_sweep(&always_allow).assert_caught("always allow");
}
