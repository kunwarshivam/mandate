//! The lookup over the parsed control stream against an independent fold (E21-55, DEC-2110):
//! random valid histories of grants, revokes and role changes, interleaved with rows of other
//! workspaces and environments and of unrelated types, read at a random instant. The oracle folds
//! the generated steps itself (identity spec §5.1, §8.3), and each seeded bug must be caught.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::control::{
    Rows, T0, activation, change, hosted_stream, member, membership, other_stream, query,
    reactivation, roles,
};
use common::{at, hosted};
use mandate_api_server::members::{ControlLookup, RowsRefused, membership_records};
use mandate_identity::{
    LookupFailed, Membership, MembershipEvent, MembershipRecord, MembershipState, PrincipalId, Role,
};
use mandate_journal::StoredEvent;
use proptest::prelude::*;
use proptest::test_runner::{TestCaseError, TestError, TestRunner};
use serde_json::json;

/// The members a history acts on; [`STRANGER`] is granted only in another workspace's stream.
const MEMBERS: [PrincipalId; 3] = [PrincipalId(11), PrincipalId(12), PrincipalId(13)];
const STRANGER: PrincipalId = PrincipalId(14);
const ROLES: [Role; 5] = [
    Role::Approver,
    Role::Auditor,
    Role::Operator,
    Role::Viewer,
    Role::WorkspaceAdmin,
];

/// One generated row, valid against the history before it.
#[derive(Debug, Clone)]
enum Step {
    Activate(PrincipalId, BTreeSet<Role>, i64),
    Change(PrincipalId, BTreeMap<Role, i64>, BTreeSet<Role>),
    Deactivate(PrincipalId),
    Reactivate(PrincipalId, BTreeSet<Role>, i64),
    Remove(PrincipalId),
    /// A hosted record that is no membership, in `live` when set, else `paper`.
    Unrelated(bool),
    /// A grant, or when set a revoke, in another workspace's stream.
    Foreign(PrincipalId, bool),
}

/// A history, each step with its `event_time`, and the lookup's instant.
#[derive(Debug, Clone)]
struct Case {
    steps: Vec<(i64, Step)>,
    now: i64,
}

impl Case {
    /// The stored rows; with `foreign_hosted`, another workspace's rows are moved into this one's.
    fn rows(&self, foreign_hosted: bool) -> Vec<StoredEvent> {
        let mut rows = Rows::opened();
        for (secs, step) in &self.steps {
            let secs = *secs;
            match step {
                Step::Activate(m, granted, ends) => {
                    rows.hosted("MemberActivated", secs, activation(*m, granted, *ends));
                }
                Step::Change(m, added, removed) => {
                    rows.hosted("MemberRoleChanged", secs, change(*m, added, removed));
                }
                Step::Deactivate(m) => rows.hosted("MemberDeactivated", secs, member(*m)),
                Step::Reactivate(m, kept, ends) => {
                    rows.hosted("MemberReactivated", secs, reactivation(*m, kept, *ends));
                }
                Step::Remove(m) => rows.hosted("MemberRemoved", secs, member(*m)),
                Step::Unrelated(live) => {
                    let env = if *live { "live" } else { "paper" };
                    let payload = json!({"agent_id": "a"});
                    rows.push(&hosted_stream(), env, "AgentDeployed", secs, payload);
                }
                Step::Foreign(m, revoke) => {
                    let stream = if foreign_hosted {
                        hosted_stream()
                    } else {
                        other_stream()
                    };
                    let (kind, payload) = if *revoke {
                        ("MemberDeactivated", member(*m))
                    } else {
                        let admin = roles(&[Role::WorkspaceAdmin]);
                        ("MemberActivated", activation(*m, &admin, secs))
                    };
                    rows.push(&stream, "paper", kind, secs, payload);
                }
            }
        }
        rows.rows
    }
}

/// A generated member's standing, so that only valid steps are emitted.
#[derive(Debug, Clone)]
enum Standing {
    Live(BTreeSet<Role>),
    Deactivated(BTreeSet<Role>),
    Removed,
}

fn picked(bits: u8) -> BTreeSet<Role> {
    let chosen = ROLES
        .iter()
        .enumerate()
        .filter(|(i, _)| bits & (1 << i) != 0);
    chosen.map(|(_, role)| *role).collect()
}

/// A valid history from raw draws: a draw whose step does not fit its member is unrelated.
fn history(draws: Vec<(u8, u8, u8, u8, u8)>, now: u8) -> Case {
    let mut standing: BTreeMap<PrincipalId, Standing> = BTreeMap::new();
    let mut secs = T0;
    let mut steps = Vec::new();
    for (kind, who, first, second, delta) in draws {
        secs += i64::from(delta % 3);
        let m = MEMBERS[usize::from(who) % MEMBERS.len()];
        let ends = secs + i64::from(delta % 4);
        let step = match (kind % 8, standing.get(&m).cloned()) {
            (0 | 1, None | Some(Standing::Removed)) => {
                standing.insert(m, Standing::Live(picked(first)));
                Step::Activate(m, picked(first), ends)
            }
            (2, Some(Standing::Live(held))) => {
                let removed: BTreeSet<Role> = picked(first).intersection(&held).copied().collect();
                let fresh = picked(second);
                let added: BTreeMap<Role, i64> = fresh
                    .difference(&held)
                    .map(|role| (*role, secs + i64::from(second % 5)))
                    .collect();
                let mut kept: BTreeSet<Role> = held.difference(&removed).copied().collect();
                kept.extend(added.keys().copied());
                standing.insert(m, Standing::Live(kept));
                Step::Change(m, added, removed)
            }
            (2, Some(Standing::Deactivated(kept))) => {
                let removed: BTreeSet<Role> = picked(first).intersection(&kept).copied().collect();
                let left = kept.difference(&removed).copied().collect();
                standing.insert(m, Standing::Deactivated(left));
                Step::Change(m, BTreeMap::new(), removed)
            }
            (3, Some(Standing::Live(held))) => {
                standing.insert(m, Standing::Deactivated(held));
                Step::Deactivate(m)
            }
            (4, Some(Standing::Deactivated(kept))) if !kept.is_empty() => {
                standing.insert(m, Standing::Live(kept.clone()));
                Step::Reactivate(m, kept, ends)
            }
            (5, Some(Standing::Deactivated(_))) => {
                standing.insert(m, Standing::Removed);
                Step::Remove(m)
            }
            (6, _) if first % 2 == 0 => Step::Foreign(STRANGER, false),
            (6, _) => Step::Foreign(m, second % 2 == 1),
            _ => Step::Unrelated(first % 2 == 1),
        };
        steps.push((secs, step));
    }
    Case {
        steps,
        now: T0 - 2 + i64::from(now),
    }
}

fn case() -> impl Strategy<Value = Case> {
    let draws = proptest::collection::vec(any::<(u8, u8, u8, u8, u8)>(), 0..24);
    (draws, 0u8..60).prop_map(|(draws, now)| history(draws, now))
}

/// The oracle's member: live until its cool-off ends, each role with the instant it takes
/// effect; deactivated; or removed.
enum Seat {
    Live(i64, BTreeMap<Role, i64>),
    Deactivated,
    Removed,
}

/// A membership started at `at` with `granted` and a cool-off ending at `ends`: operator and
/// approver wait for it, every other role is effective at once (identity spec §8.3).
fn started(granted: &BTreeSet<Role>, at: i64, ends: i64) -> Seat {
    let since = |role: &Role| match role {
        Role::Operator | Role::Approver => ends,
        _ => at,
    };
    Seat::Live(ends, granted.iter().map(|r| (*r, since(r))).collect())
}

/// Every member's membership at `case.now`, folded from the steps dated at or before it.
fn expected(case: &Case) -> BTreeMap<PrincipalId, Vec<Membership>> {
    let now = case.now;
    let mut seats: BTreeMap<PrincipalId, Seat> = BTreeMap::new();
    for (secs, step) in case.steps.iter().filter(|(secs, _)| *secs <= now) {
        match step {
            Step::Activate(m, granted, ends) | Step::Reactivate(m, granted, ends) => {
                seats.insert(*m, started(granted, *secs, *ends));
            }
            Step::Change(m, added, removed) => {
                if let Some(Seat::Live(_, held)) = seats.get_mut(m) {
                    held.retain(|role, _| !removed.contains(role));
                    held.extend(added.iter().map(|(role, ends)| (*role, *ends)));
                }
            }
            Step::Deactivate(m) => drop(seats.insert(*m, Seat::Deactivated)),
            Step::Remove(m) => drop(seats.insert(*m, Seat::Removed)),
            Step::Unrelated(_) | Step::Foreign(..) => {}
        }
    }
    let read = |m: PrincipalId, seat: &Seat| match seat {
        Seat::Live(until, held) => {
            let state = if now >= *until {
                MembershipState::Active
            } else {
                MembershipState::CoolingOff
            };
            let effective = held.iter().filter(|(_, since)| **since <= now);
            let effective: Vec<Role> = effective.map(|(role, _)| *role).collect();
            membership(m, state, &effective, now)
        }
        Seat::Deactivated => membership(m, MembershipState::Deactivated, &[], now),
        Seat::Removed => membership(m, MembershipState::Removed, &[], now),
    };
    let everyone = MEMBERS.iter().chain([&STRANGER]);
    everyone
        .map(|m| {
            (
                *m,
                seats
                    .get(m)
                    .map(|seat| read(*m, seat))
                    .into_iter()
                    .collect(),
            )
        })
        .collect()
}

type Parse = fn(&Case) -> Result<Vec<MembershipRecord>, RowsRefused>;

/// Whether the lookup over `parse`'s records answers every member as the oracle does.
fn agrees(case: &Case, parse: Parse) -> Result<(), TestCaseError> {
    let parsed = parse(case);
    let records = parsed.clone().map_err(|_| LookupFailed);
    let lookup = ControlLookup::new(hosted(), records, at(case.now));
    for (m, want) in expected(case) {
        let got = query(&lookup, m);
        if got != Ok(want.clone()) {
            return Err(TestCaseError::fail(format!(
                "parsed {parsed:?}: {m:?} reads {got:?}, the oracle {want:?}"
            )));
        }
    }
    Ok(())
}

fn runner() -> TestRunner {
    TestRunner::new(ProptestConfig {
        cases: 256,
        failure_persistence: None,
        ..ProptestConfig::default()
    })
}

fn real(case: &Case) -> Result<Vec<MembershipRecord>, RowsRefused> {
    membership_records(hosted().workspace, &case.rows(false))
}

fn drops_revokes(case: &Case) -> Result<Vec<MembershipRecord>, RowsRefused> {
    let revoke = |e: &MembershipEvent| {
        matches!(
            e,
            MembershipEvent::Deactivated { .. } | MembershipEvent::Removed { .. }
        )
    };
    real(case).map(|records| records.into_iter().filter(|r| !revoke(r.event())).collect())
}

fn drops_role_changes(case: &Case) -> Result<Vec<MembershipRecord>, RowsRefused> {
    let changed = |e: &MembershipEvent| matches!(e, MembershipEvent::RoleChanged { .. });
    real(case).map(|records| {
        records
            .into_iter()
            .filter(|r| !changed(r.event()))
            .collect()
    })
}

fn reverses_the_order(case: &Case) -> Result<Vec<MembershipRecord>, RowsRefused> {
    real(case).map(|records| records.into_iter().rev().collect())
}

fn reads_other_workspaces(case: &Case) -> Result<Vec<MembershipRecord>, RowsRefused> {
    membership_records(hosted().workspace, &case.rows(true))
}

/// The seeded bugs, each of which the oracle must catch, by what they do.
const SEEDED: [(&str, Parse); 4] = [
    ("drops revokes", drops_revokes),
    ("drops role changes", drops_role_changes),
    ("reverses the stream order", reverses_the_order),
    ("reads other workspaces' rows", reads_other_workspaces),
];

#[test]
fn the_lookup_over_parsed_rows_matches_an_independent_fold() {
    if let Err(failure) = runner().run(&case(), |case| agrees(&case, real)) {
        panic!("{failure}");
    }
    for (name, bug) in SEEDED {
        let caught = runner().run(&case(), |case| agrees(&case, bug));
        assert!(
            matches!(caught, Err(TestError::Fail(..))),
            "the oracle catches the seeded bug that {name}"
        );
    }
}
