//! E9-7 M1b (DEC-657): the three access-reducing fold gaps the backlog lists under E9-7 (#812's
//! review), the kept roles, the order check (DEC-657 item 4), and the writer's independence check
//! (journal spec §9.12, rule 103's input, DEC-648 item 7).

mod membership;

use std::collections::{BTreeMap, BTreeSet};

use mandate_identity::{
    InvitationId, MembershipEvent as Event, PrincipalId, RecordRefusal, Role, check_independence,
};
use membership::{
    B, activated, cool, fold, founded, founding, invited, reactivated, record, sequenced, set, t,
};

const APPROVER: u8 = 0b00001;
const VIEWER: u8 = 0b01000;
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
fn an_activation_with_a_strict_subset_of_the_invitations_roles_is_refused() {
    let (subset, exact) = (
        joined(AUDITOR_VIEWER, 0b01000),
        joined(AUDITOR_VIEWER, AUDITOR_VIEWER),
    );
    assert_eq!(fold(subset).refused(t(7_200)), Some(3));
    assert_eq!(fold(exact).refused(t(7_200)), None);
}

/// §9.12: a grant of a held role is refused, even one still cooling off (§8.3), so a writer cannot
/// restart or shorten a cool-off by granting the role again. The grant records independence, as
/// B's activation did, and its cool-off follows rule 103.
#[test]
fn a_grant_of_a_role_still_cooling_off_is_refused() {
    let mut h = joined(APPROVER_VIEWER, APPROVER_VIEWER);
    let added = BTreeMap::from([(Role::Approver, cool(10_800, true, &set(APPROVER)))]);
    let (member, removed) = (PrincipalId(B), BTreeSet::new());
    let granted = Event::RoleChanged {
        member,
        added,
        removed,
        independent_approval_required: true,
    };
    h.push((10_800, granted));
    let fold = fold(h);
    assert_eq!(fold.effective_roles(member, t(10_799)), set(VIEWER));
    assert_eq!(fold.refused(t(10_800)), Some(4));
}

/// §9.12 and DEC-654 item 7: a deactivated member keeps every role it held, a cooling one too, less
/// those an admin strips while it is deactivated, and keeps nothing once reactivated.
#[test]
fn a_deactivated_member_keeps_its_cooling_roles_less_those_stripped() {
    let member = PrincipalId(B);
    let mut h = joined(APPROVER_VIEWER, APPROVER_VIEWER);
    h.push((10_800, Event::Deactivated { member }));
    let stripped = Event::RoleChanged {
        member,
        added: BTreeMap::new(),
        removed: set(VIEWER),
        independent_approval_required: true,
    };
    h.push((14_400, stripped));
    h.push((18_000, reactivated(B, set(APPROVER), true, 18_000)));
    let fold = fold(h);
    assert_eq!(
        fold.kept_roles(member, t(10_799)),
        set(0),
        "not deactivated yet"
    );
    assert_eq!(fold.kept_roles(member, t(10_800)), set(APPROVER_VIEWER));
    assert_eq!(fold.effective_roles(member, t(10_800)), set(0));
    assert_eq!(fold.kept_roles(member, t(14_399)), set(APPROVER_VIEWER));
    assert_eq!(fold.kept_roles(member, t(14_400)), set(APPROVER));
    assert_eq!(
        fold.refused(t(18_000)),
        None,
        "the reactivation restores the kept roles"
    );
    assert_eq!(fold.kept_roles(member, t(18_000)), set(0), "reactivated");
}

/// DEC-657 item 4: a record whose `seq` is not above, or whose `event_time` is before, the record
/// before it makes the fold unreadable from its own `event_time`, though the record before it is
/// not yet folded there, and leaves every earlier reading as it was.
#[test]
fn a_record_out_of_order_makes_the_fold_unreadable_from_its_instant() {
    let (f, viewer) = (|| founding().1, || set(VIEWER));
    let seq = sequenced(vec![
        (1, 0, f()),
        (5, 3_600, invited(1, viewer(), 3_600)),
        (4, 7_200, invited(2, viewer(), 7_200)),
    ]);
    assert!(!seq.unreadable(t(7_199)));
    assert_eq!(seq.refused(t(7_200)), Some(4), "a seq not above");
    let back = sequenced(vec![
        (1, 0, f()),
        (2, 3_600, invited(1, viewer(), 3_600)),
        (3, 7_200, activated(B, Some(1), viewer(), true, 7_200)),
        (4, 5_400, invited(2, viewer(), 5_400)),
    ]);
    assert!(!back.unreadable(t(5_399)));
    assert!(
        back.unreadable(t(5_400)),
        "record 3 is after 5_400 but held"
    );
    assert_eq!(back.refused(t(7_200)), Some(4), "an event_time backwards");
    assert_eq!(back.workspace_users(t(7_200)), 1);
}

/// Each record type that carries `independent_approval_required` (§9.12), recording `ind`: an
/// activation by invitation, the founding grant, a role grant, and a reactivation.
fn flagged(ind: bool) -> [(&'static str, Event); 4] {
    let added = BTreeMap::from([(Role::Approver, cool(0, ind, &set(APPROVER)))]);
    let granted = Event::RoleChanged {
        member: PrincipalId(B),
        added,
        removed: BTreeSet::new(),
        independent_approval_required: ind,
    };
    [
        (
            "activation",
            activated(B, Some(1), set(APPROVER_VIEWER), ind, 0),
        ),
        ("founding", founded(ind)),
        ("role change", granted),
        ("reactivation", reactivated(B, set(APPROVER_VIEWER), ind, 0)),
    ]
}

/// §9.12's cross-record check of rule 103's input (DEC-648 item 7): a writer that records
/// `independent_approval_required: false` while independence is in force would skip §8.3's
/// cool-off, so each record type carrying it is refused; a record stating the policy passes.
#[test]
fn a_record_of_no_independence_while_it_is_in_force_is_refused() {
    let refused = Err(RecordRefusal::IndependenceMismatch);
    for ((name, without), (_, with)) in flagged(false).into_iter().zip(flagged(true)) {
        let (without, with) = (record(2, 0, without), record(2, 0, with));
        assert_eq!(check_independence(&without, true), refused, "{name}");
        assert_eq!(check_independence(&with, true), Ok(()), "{name}");
        assert_eq!(check_independence(&without, false), Ok(()), "{name}");
    }
}

/// The check is equality (§9.12): each record type claiming independence the workspace does not
/// require is refused too, and a record type with no such member passes whatever the policy.
#[test]
fn the_recorded_independence_must_equal_the_effective_policy() {
    let refused = Err(RecordRefusal::IndependenceMismatch);
    for (name, with) in flagged(true) {
        assert_eq!(
            check_independence(&record(2, 0, with), false),
            refused,
            "{name}"
        );
    }
    let member = PrincipalId(B);
    let unflagged = [
        invited(1, set(VIEWER), 0),
        Event::InvitationRevoked {
            invitation: InvitationId(1),
        },
        Event::Deactivated { member },
        Event::Removed { member },
    ];
    for event in unflagged {
        for policy in [false, true] {
            let r = record(3, 0, event.clone());
            assert_eq!(check_independence(&r, policy), Ok(()), "{event:?}");
        }
    }
}
