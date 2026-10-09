//! E9-7 M1b (DEC-657): the three access-reducing fold gaps the backlog lists under E9-7 (#812's
//! review), and the writer's independence check (journal spec §9.12, rule 103's input, DEC-648
//! item 7).

mod membership;

use std::collections::{BTreeMap, BTreeSet};

use mandate_identity::{
    MembershipEvent as Event, MembershipRecord, PrincipalId, RecordRefusal, Role,
    check_independence,
};
use membership::{B, activated, fold, founding, invited, reactivated, set, t};

const AUDITOR_VIEWER: u8 = 0b01010;
const APPROVER_VIEWER: u8 = 0b01001;

/// The founder, an invitation naming `invited` at hour 1, and B's activation with `roles` at hour
/// 2, with independence required, so approver and operator cool off for a day.
fn joined(invited_roles: u8, roles: u8) -> Vec<(i64, Event)> {
    let accepted = activated(B, Some(1), set(roles), true, 7_200);
    vec![
        founding(),
        (3_600, invited(1, set(invited_roles), 3_600)),
        (7_200, accepted),
    ]
}

/// §9.12: a reactivation restores exactly the kept roles, so a strict subset is refused.
#[test]
#[ignore = "pending E9-7"]
fn a_reactivation_with_a_strict_subset_of_the_kept_roles_is_refused() {
    let with = |mask| {
        let mut h = joined(AUDITOR_VIEWER, AUDITOR_VIEWER);
        h.push((
            10_800,
            Event::Deactivated {
                member: PrincipalId(B),
            },
        ));
        h.push((14_400, reactivated(B, set(mask), true, 14_400)));
        fold(h)
    };
    assert_eq!(with(0b01000).refused(t(14_400)), Some(5));
    assert_eq!(with(0b01000).workspace_users(t(14_400)), 1);
    assert_eq!(with(AUDITOR_VIEWER).refused(t(14_400)), None);
    assert_eq!(with(AUDITOR_VIEWER).workspace_users(t(14_400)), 2);
}

/// §9.12: an activation takes exactly the invitation's roles, so a strict subset is refused.
#[test]
#[ignore = "pending E9-7"]
fn an_activation_with_a_strict_subset_of_the_invitations_roles_is_refused() {
    let (subset, exact) = (
        joined(AUDITOR_VIEWER, 0b01000),
        joined(AUDITOR_VIEWER, AUDITOR_VIEWER),
    );
    assert_eq!(fold(subset).refused(t(7_200)), Some(3));
    assert_eq!(fold(exact).refused(t(7_200)), None);
}

/// §9.12: a grant of a held role is refused, even one still cooling off (§8.3), so a writer cannot
/// restart or shorten a cool-off by granting the role again.
#[test]
#[ignore = "pending E9-7"]
fn a_grant_of_a_role_still_cooling_off_is_refused() {
    let mut h = joined(APPROVER_VIEWER, APPROVER_VIEWER);
    let added = BTreeMap::from([(Role::Approver, t(10_800))]);
    let (member, removed) = (PrincipalId(B), BTreeSet::new());
    let granted = Event::RoleChanged {
        member,
        added,
        removed,
        independent_approval_required: false,
    };
    h.push((10_800, granted));
    let fold = fold(h);
    assert_eq!(fold.effective_roles(member, t(10_799)), set(0b01000));
    assert_eq!(fold.refused(t(10_800)), Some(4));
}

fn reactivation(independent: bool) -> MembershipRecord {
    MembershipRecord::new(2, t(0), reactivated(B, set(0b01000), independent, 0))
}

/// §9.12's cross-record check of rule 103's input (DEC-648 item 7): a writer that records
/// `independent_approval_required: false` while independence is in force would skip §8.3's
/// cool-off, so the record is refused; a record stating the policy passes.
#[test]
#[ignore = "pending E9-7"]
fn a_record_of_no_independence_while_it_is_in_force_is_refused() {
    let refused = Err(RecordRefusal::IndependenceMismatch);
    assert_eq!(check_independence(&reactivation(false), true), refused);
    assert_eq!(check_independence(&reactivation(true), true), Ok(()));
    assert_eq!(check_independence(&reactivation(false), false), Ok(()));
}

/// The check is equality (§9.12): a record claiming independence the workspace does not require is
/// refused too, and a record with no such member passes whatever the policy.
#[test]
#[ignore = "pending E9-7"]
fn the_recorded_independence_must_equal_the_effective_policy() {
    let refused = Err(RecordRefusal::IndependenceMismatch);
    assert_eq!(check_independence(&reactivation(true), false), refused);
    let left = MembershipRecord::new(
        3,
        t(0),
        Event::Deactivated {
            member: PrincipalId(B),
        },
    );
    assert_eq!(check_independence(&left, true), Ok(()));
}
