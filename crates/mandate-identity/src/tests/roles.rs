//! ID-13, ID-4 and §5.2 for `change_roles`, and DEC-832 item 7 for `PrincipalContext::into_tenant`
//! (E9-2, identity spec §4.2, §4.5, §5.2; DEC-643, DEC-654). Every expected outcome is written from
//! the spec's text, or in the two properties computed by a fold of the change written here, never
//! from the crate's own checks. Each case runs on a store answering the query and on one
//! answering every membership of every scope, so the change must pick its scope and members itself.

use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};

use mandate_time::UtcNanos;

use crate::MembershipState::{Active, CoolingOff, Deactivated, Invited, Removed};
use crate::Role::{
    Approver, Auditor, BillingAdmin, Operator as Op, OrgAdmin, OrgOwner, Viewer,
    WorkspaceAdmin as Wa,
};
use crate::{
    Authorized, Membership, MembershipState, OrgId, Permission, Principal, PrincipalContext,
    PrincipalId, PrincipalKind, Refusal, Role, RoleChange, Scope, Session, SessionKind, SessionRef,
    StepUp, WorkspaceId, authorize, change_roles,
};

use super::{ByMember, Everything, O1, O2, W1, W3};

const ORG: Scope = Scope::Org(O1);
const WS: Scope = Scope::Workspace {
    org: O1,
    workspace: W1,
};
const WS3: Scope = Scope::Workspace {
    org: O2,
    workspace: W3,
};
const SESSION: SessionRef = SessionRef(0x41);
const NOW_SECS: i64 = 1_790_000_000;

const OO1: PrincipalId = PrincipalId(0x51);
const OO2: PrincipalId = PrincipalId(0x52);
const OO3: PrincipalId = PrincipalId(0x53);
const OO4: PrincipalId = PrincipalId(0x54);
const OA: PrincipalId = PrincipalId(0x55);
const BILL: PrincipalId = PrincipalId(0x56);
const WA1: PrincipalId = PrincipalId(0x61);
const WA2: PrincipalId = PrincipalId(0x62);
const WA3: PrincipalId = PrincipalId(0x63);
const ADM: PrincipalId = PrincipalId(0x64);
const OPR: PrincipalId = PrincipalId(0x65);
const VIEW: PrincipalId = PrincipalId(0x66);
const ELSEWHERE: PrincipalId = PrincipalId(0x67);

type Outcome = Result<StepUp, Refusal>;
const REQUIRED: Outcome = Ok(StepUp::Required);
const NOT_REQUIRED: Outcome = Ok(StepUp::NotRequired);

fn at(nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(NOW_SECS, nanos).unwrap()
}

/// A membership whose roles are effective from `from`.
fn since(
    id: PrincipalId,
    scope: Scope,
    state: MembershipState,
    roles: &[Role],
    from: UtcNanos,
) -> Membership {
    let roles = roles.iter().map(|r| (*r, from)).collect();
    Membership {
        member: id,
        scope,
        state,
        roles,
    }
}

/// A membership whose roles have long been effective.
fn m(id: PrincipalId, scope: Scope, state: MembershipState, roles: &[Role]) -> Membership {
    since(id, scope, state, roles, UtcNanos::EPOCH)
}

/// O1: one active owner (OO1), a suspended one (OO2), a cooling-off one (OO3), an org admin and a
/// billing admin. W1: one active admin (WA1), a suspended one (WA2), a cooling-off one (WA3), an
/// operator-approver and a viewer. Active holders elsewhere (O2, W3) never count for O1 or W1.
fn store(extra: &[Membership]) -> Vec<Membership> {
    let mut store = vec![
        m(OO1, ORG, Active, &[OrgOwner]),
        m(OO2, ORG, Deactivated, &[OrgOwner]),
        m(OO3, ORG, CoolingOff, &[OrgOwner]),
        m(OA, ORG, Active, &[OrgAdmin]),
        m(BILL, ORG, Active, &[BillingAdmin]),
        m(WA1, WS, Active, &[Wa, Op]),
        m(WA2, WS, Deactivated, &[Wa]),
        m(WA3, WS, CoolingOff, &[Wa]),
        m(OPR, WS, Active, &[Op, Approver]),
        m(VIEW, WS, Active, &[Viewer]),
        m(VIEW, WS3, Active, &[Viewer]),
        m(ELSEWHERE, Scope::Org(O2), Active, &[OrgOwner]),
        m(ELSEWHERE, WS3, Active, &[Wa]),
    ];
    store.extend_from_slice(extra);
    store
}

type Entries<'a> = &'a [(PrincipalId, Role)];

fn ch(grants: Entries, removals: Entries, off: &[PrincipalId]) -> RoleChange {
    RoleChange {
        grants: grants.iter().copied().collect(),
        removals: removals.iter().copied().collect(),
        deactivations: off.iter().copied().collect(),
    }
}

fn run_in(
    kind: SessionKind,
    store: &[Membership],
    by: PrincipalId,
    scope: Scope,
    change: &RoleChange,
) -> Outcome {
    let user = Principal::User { id: by };
    let session = Session {
        reference: SESSION,
        kind,
        snapshot: Vec::new(),
    };
    let asked = change_roles(&ByMember(store), &user, &session, scope, change, at(0));
    let all = change_roles(&Everything(store), &user, &session, scope, change, at(0));
    assert_eq!(
        asked, all,
        "a store answering more than the query changes nothing"
    );
    asked
}

/// Each case: the store, the author, the scope, the change, and what the spec says of it.
type Case<'a> = (&'a [Membership], PrincipalId, Scope, RoleChange, Outcome);

fn check(cases: &[Case]) {
    assert!(!cases.is_empty());
    for (store, by, scope, change, expected) in cases {
        let got = run_in(SessionKind::Full, store, *by, *scope, change);
        assert_eq!(got, *expected, "{by:?} at {scope:?}: {change:?}");
    }
}

/// ID-13 read tightly (DEC-643): a change granting or removing a role of its own author is refused
/// `own_roles`, raising or lowering, with another owner left, and before `owner_role_reserved`
/// (§4.5's order); the same change naming another member passes.
#[test]
#[ignore = "pending E9-2"]
fn no_principal_changes_its_own_roles_up_or_down() {
    let (s, two) = (store(&[]), store(&[m(OO4, ORG, Active, &[OrgOwner])]));
    let own = Err(Refusal::OwnRoles);
    check(&[
        (&s, WA1, WS, ch(&[(VIEW, Approver)], &[], &[]), REQUIRED),
        (&s, WA1, WS, ch(&[(WA1, Approver)], &[], &[]), own),
        (&s, WA1, WS, ch(&[], &[(WA1, Op)], &[]), own),
        (&two, OO1, ORG, ch(&[], &[(OO4, OrgOwner)], &[]), REQUIRED),
        (&two, OO1, ORG, ch(&[], &[(OO1, OrgOwner)], &[]), own),
        (&s, OA, ORG, ch(&[(OA, OrgOwner)], &[], &[]), own),
        (
            &s,
            OA,
            ORG,
            ch(&[(BILL, OrgAdmin)], &[(OA, OrgAdmin)], &[]),
            own,
        ),
    ]);
}

/// The way down is the leave row (DEC-641 item 5): no step-up, and §5.2 still holds on it; a
/// suspended, cooling-off, or other scope's holder is not an active owner or admin (DEC-654 item 2).
#[test]
#[ignore = "pending E9-2"]
fn leaving_needs_no_step_up_and_never_leaves_no_active_owner_or_admin() {
    let s = store(&[]);
    check(&[
        (&s, OPR, WS, ch(&[], &[], &[OPR]), NOT_REQUIRED),
        (&s, VIEW, WS, ch(&[], &[], &[VIEW]), NOT_REQUIRED),
        (&s, OA, ORG, ch(&[], &[], &[OA]), NOT_REQUIRED),
        (&s, WA1, WS, ch(&[], &[], &[WA1]), Err(Refusal::LastAdmin)),
        (&s, OO1, ORG, ch(&[], &[], &[OO1]), Err(Refusal::LastOwner)),
    ]);
}

/// §5.2: no change leaves O1 without an active org owner or W1 without an active admin, unless it
/// adds an active successor in the same command (a cooling-off one is not); a role counts once
/// effective at `now`, the boundary included (DEC-654 item 2).
#[test]
#[ignore = "pending E9-2"]
fn the_last_active_owner_or_admin_is_never_removed_or_demoted() {
    let s = store(&[]);
    let newcomer = store(&[m(OO4, ORG, CoolingOff, &[BillingAdmin])]);
    let owners = store(&[m(OO4, ORG, Active, &[OrgOwner])]);
    let admins = store(&[m(ADM, WS, Active, &[Wa])]);
    let (at_now, later) = (
        store(&[since(ADM, WS, Active, &[Wa], at(0))]),
        store(&[since(ADM, WS, Active, &[Wa], at(1))]),
    );
    let (owner, admin) = (Err(Refusal::LastOwner), Err(Refusal::LastAdmin));
    check(&[
        (&s, OO1, ORG, ch(&[(BILL, OrgOwner)], &[], &[OO1]), REQUIRED),
        (
            &newcomer,
            OO1,
            ORG,
            ch(&[(OO4, OrgOwner)], &[], &[OO1]),
            owner,
        ),
        (&owners, OO1, ORG, ch(&[], &[], &[OO4]), REQUIRED),
        (&owners, OO1, ORG, ch(&[], &[], &[OO4, OO1]), owner),
        (
            &owners,
            OO1,
            ORG,
            ch(&[], &[(OO4, OrgOwner)], &[OO1]),
            owner,
        ),
        (&s, WA1, WS, ch(&[(OPR, Wa)], &[], &[WA1]), REQUIRED),
        (&admins, WA1, WS, ch(&[], &[(ADM, Wa)], &[]), NOT_REQUIRED),
        (&admins, WA1, WS, ch(&[], &[(ADM, Wa)], &[WA1]), admin),
        (&admins, WA1, WS, ch(&[], &[], &[ADM]), NOT_REQUIRED),
        (&admins, WA1, WS, ch(&[], &[], &[ADM, WA1]), admin),
        (&at_now, WA1, WS, ch(&[], &[], &[WA1]), NOT_REQUIRED),
        (&later, WA1, WS, ch(&[], &[], &[WA1]), admin),
    ]);
}

/// §4.2's workspace rows: only a workspace admin grants (step-up, ID-4), removes, or deactivates
/// (neither needs it); an org owner has no workspace role (§4.1); a role of the other scope's kind
/// is `forbidden` (DEC-654 item 3); a suspended admin, another workspace, a pair the store does not
/// hold, and a reduction-only session reach nothing.
#[test]
#[ignore = "pending E9-2"]
fn only_a_workspace_admin_changes_workspace_roles() {
    let s = store(&[]);
    let grant = || ch(&[(VIEW, Op)], &[], &[]);
    let (forbidden, none) = (Err(Refusal::Forbidden), Err(Refusal::NoMembership));
    check(&[
        (&s, WA1, WS, grant(), REQUIRED),
        (&s, WA1, WS, ch(&[], &[(VIEW, Viewer)], &[]), NOT_REQUIRED),
        (&s, WA1, WS, ch(&[], &[], &[VIEW]), NOT_REQUIRED),
        (
            &s,
            WA1,
            WS,
            ch(&[(VIEW, Approver)], &[(OPR, Op)], &[]),
            REQUIRED,
        ),
        (&s, OPR, WS, grant(), forbidden),
        (&s, VIEW, WS, ch(&[], &[(OPR, Op)], &[]), forbidden),
        (&s, OPR, WS, ch(&[], &[], &[VIEW]), forbidden),
        (&s, WA1, WS, ch(&[(VIEW, OrgOwner)], &[], &[]), forbidden),
        (&s, OO1, WS, grant(), none),
        (&s, WA2, WS, grant(), none),
        (&s, WA1, WS3, grant(), none),
        (
            &s,
            WA1,
            Scope::Workspace {
                org: O2,
                workspace: W1,
            },
            grant(),
            none,
        ),
    ]);
    let reduction = run_in(SessionKind::ReductionOnly, &s, WA1, WS, &grant());
    assert_eq!(reduction, Err(Refusal::ReductionOnly));
}

/// §4.2's org memberships row: an org owner or admin, step-up on every use, except that only an org
/// owner grants, removes, or (by deactivating its holder) takes the org owner role, which is
/// refused before the last-owner rule (§4.5's order).
#[test]
#[ignore = "pending E9-2"]
fn only_an_org_owner_changes_the_owner_role() {
    let s = store(&[]);
    let reserved = Err(Refusal::OwnerRoleReserved);
    check(&[
        (&s, OO1, ORG, ch(&[(BILL, OrgAdmin)], &[], &[]), REQUIRED),
        (
            &s,
            OO1,
            ORG,
            ch(&[], &[(BILL, BillingAdmin)], &[]),
            REQUIRED,
        ),
        (&s, OO1, ORG, ch(&[(BILL, OrgOwner)], &[], &[]), REQUIRED),
        (&s, OA, ORG, ch(&[(BILL, OrgAdmin)], &[], &[]), REQUIRED),
        (&s, OA, ORG, ch(&[], &[], &[BILL]), REQUIRED),
        (&s, OA, ORG, ch(&[(BILL, OrgOwner)], &[], &[]), reserved),
        (&s, OA, ORG, ch(&[], &[(OO1, OrgOwner)], &[]), reserved),
        (&s, OA, ORG, ch(&[], &[], &[OO1]), reserved),
        (
            &s,
            BILL,
            ORG,
            ch(&[(OA, BillingAdmin)], &[], &[]),
            Err(Refusal::Forbidden),
        ),
        (
            &s,
            OO1,
            ORG,
            ch(&[(BILL, Wa)], &[], &[]),
            Err(Refusal::Forbidden),
        ),
        (
            &s,
            OO2,
            ORG,
            ch(&[(BILL, OrgAdmin)], &[], &[]),
            Err(Refusal::NoMembership),
        ),
        (
            &s,
            ELSEWHERE,
            ORG,
            ch(&[(BILL, OrgAdmin)], &[], &[]),
            Err(Refusal::NoMembership),
        ),
    ]);
}

/// The scope, its kind's roles, and its admin role.
fn kind(org: bool) -> (Scope, &'static [Role], Role) {
    match org {
        true => (ORG, &[OrgOwner, OrgAdmin, BillingAdmin], OrgOwner),
        false => (WS, &[Wa, Op, Approver, Viewer, Auditor], Wa),
    }
}

fn property<S: Strategy>(strategy: S, test: impl Fn(S::Value) -> Result<(), TestCaseError>) {
    let config = Config {
        failure_persistence: None,
        ..Config::default()
    };
    if let Err(failure) = TestRunner::new(config).run(&strategy, test) {
        panic!("{failure}");
    }
}

/// Org scope or not; the author's entries, grants or removals; one entry per other member.
type OwnDraw = (bool, bool, Vec<usize>, Vec<(u8, usize)>);

/// ID-13 as a property: an admin's change naming itself in a grant or a removal is refused
/// `own_roles`, whatever else it holds; without those entries it passes, with step-up for a grant
/// at a workspace and for any change at an org (§4.2).
#[test]
#[ignore = "pending E9-2"]
fn no_change_naming_its_author_passes_and_the_rest_of_it_does() {
    let own = prop::collection::vec(0usize..5, 1..4);
    let others = prop::collection::vec((0u8..3, 0usize..5), 1..5);
    property(
        (any::<bool>(), any::<bool>(), own, others),
        |(org, grant, own, others): OwnDraw| {
            let (scope, roles, admin) = kind(org);
            let (author, role) = (PrincipalId(0x100), |r: usize| roles[r % roles.len()]);
            let mut store = vec![m(author, scope, Active, &[admin])];
            let mut rest = RoleChange::default();
            for (i, (action, r)) in others.into_iter().enumerate() {
                let id = PrincipalId(0x200 + i as u128);
                store.push(m(id, scope, Active, &[role(i)]));
                match action {
                    0 => rest.grants.insert((id, role(r))),
                    1 => rest.removals.insert((id, role(r))),
                    _ => rest.deactivations.insert(id),
                };
            }
            let mut named = rest.clone();
            let mine = own.into_iter().map(|r| (author, role(r)));
            match grant {
                true => named.grants.extend(mine),
                false => named.removals.extend(mine),
            }
            prop_assert_eq!(
                run_in(SessionKind::Full, &store, author, scope, &named),
                Err(Refusal::OwnRoles)
            );
            let step = match org || !rest.grants.is_empty() {
                true => StepUp::Required,
                false => StepUp::NotRequired,
            };
            prop_assert_eq!(
                run_in(SessionKind::Full, &store, author, scope, &rest),
                Ok(step)
            );
            Ok(())
        },
    );
}

/// Org scope or not; per other member its state, whether it holds the admin role, and its action;
/// whether the author leaves.
type LastDraw = (bool, Vec<(usize, bool, u8)>, bool);

/// §5.2 as a property: a change by an admin is refused `last_owner` (`last_admin`) exactly when
/// this fold leaves no member `active` holding the admin role: the author unless it leaves, a
/// successor granted it in the change, never a suspended, invited, cooling-off, or removed holder,
/// nor one of another scope. Only a membership that reaches its scope is changed.
#[test]
#[ignore = "pending E9-2"]
fn no_change_leaves_a_scope_without_an_active_owner_or_admin() {
    let members = prop::collection::vec((0usize..5, any::<bool>(), 0u8..4), 1..5);
    property(
        (any::<bool>(), members, any::<bool>()),
        |(org, members, leave): LastDraw| {
            let (scope, _, admin) = kind(org);
            let states = [Active, CoolingOff, Deactivated, Invited, Removed];
            let author = PrincipalId(0x100);
            let elsewhere = if org { Scope::Org(O2) } else { WS3 };
            let mut store = vec![
                m(author, scope, Active, &[admin]),
                m(ELSEWHERE, elsewhere, Active, &[admin]),
            ];
            let mut change = RoleChange::default();
            let mut left = 0usize;
            for (i, (state, holds, action)) in members.into_iter().enumerate() {
                let id = PrincipalId(0x200 + i as u128);
                let held = [admin];
                let roles = if holds { &held[..] } else { &[] };
                store.push(m(id, scope, states[state], roles));
                let acts = matches!(states[state], Active | CoolingOff);
                let (grant, removal, off) = (
                    acts && !holds && action == 0,
                    acts && holds && action == 1,
                    acts && action == 2,
                );
                if grant {
                    change.grants.insert((id, admin));
                }
                if removal {
                    change.removals.insert((id, admin));
                }
                if off {
                    change.deactivations.insert(id);
                }
                left +=
                    usize::from(states[state] == Active && !off && ((holds && !removal) || grant));
            }
            if leave || change == RoleChange::default() {
                change.deactivations.insert(author);
            } else {
                left += 1;
            }
            let others = change.deactivations.iter().any(|d| *d != author);
            let step = match (org && (others || !change.removals.is_empty()))
                || !change.grants.is_empty()
            {
                true => StepUp::Required,
                false => StepUp::NotRequired,
            };
            let expected = match (left, org) {
                (0, true) => Err(Refusal::LastOwner),
                (0, false) => Err(Refusal::LastAdmin),
                _ => Ok(step),
            };
            prop_assert_eq!(
                run_in(SessionKind::Full, &store, author, scope, &change),
                expected
            );
            Ok(())
        },
    );
}

/// The context an `own`, leave, or `self` row yields to `who` at `scope`, from the live `authorize`.
fn own(who: PrincipalId, scope: Scope, permission: Permission) -> PrincipalContext {
    let session = Session {
        reference: SESSION,
        kind: SessionKind::Full,
        snapshot: Vec::new(),
    };
    match authorize(
        &ByMember(&store(&[])),
        &Principal::User { id: who },
        &session,
        scope,
        permission,
        at(0),
    ) {
        Ok(Authorized::Principal { context, .. }) => context,
        other => panic!("{permission:?} at {scope:?} for {who:?}: {other:?}"),
    }
}

type Seen = (
    OrgId,
    WorkspaceId,
    PrincipalId,
    PrincipalKind,
    SessionRef,
    Permission,
    bool,
);

fn into_tenant(context: PrincipalContext) -> Result<Seen, Refusal> {
    let tenant = context.into_tenant()?;
    Ok((
        tenant.org(),
        tenant.workspace(),
        tenant.principal(),
        tenant.kind(),
        tenant.session(),
        tenant.permission(),
        tenant.membership_unverified(),
    ))
}

/// DEC-832 item 7: a `PrincipalContext` becomes the context of exactly the workspace its membership
/// reached, with the same principal, session, permission, and flag; at an organization's or the
/// principal's own scope it is `no_membership`. A workspace the member does not reach, or a pair
/// the store does not hold, yields no `PrincipalContext` to begin with.
#[test]
#[ignore = "pending E9-2"]
fn into_tenant_yields_only_the_workspace_its_membership_reached() {
    let (user, leave, passkey) = (
        PrincipalKind::User,
        Permission::Leave,
        Permission::OwnPasskey,
    );
    assert_eq!(
        into_tenant(own(VIEW, WS, leave)),
        Ok((O1, W1, VIEW, user, SESSION, leave, false))
    );
    assert_eq!(
        into_tenant(own(VIEW, WS3, passkey)),
        Ok((O2, W3, VIEW, user, SESSION, passkey, false))
    );
    assert_eq!(
        into_tenant(own(OO1, ORG, passkey)),
        Err(Refusal::NoMembership)
    );
    let principal = own(VIEW, Scope::Principal, Permission::ListOwnMemberships);
    assert_eq!(into_tenant(principal), Err(Refusal::NoMembership));
    let flagged = PrincipalContext {
        principal: OPR,
        kind: user,
        session: SessionRef(0x42),
        permission: leave,
        scope: WS3,
        membership_unverified: true,
    };
    assert_eq!(
        into_tenant(flagged),
        Ok((O2, W3, OPR, user, SessionRef(0x42), leave, true))
    );
    let session = Session {
        reference: SESSION,
        kind: SessionKind::Full,
        snapshot: Vec::new(),
    };
    let foreign = Scope::Workspace {
        org: O2,
        workspace: W1,
    };
    for scope in [foreign, WS3] {
        let asked = authorize(
            &ByMember(&store(&[])),
            &Principal::User { id: OPR },
            &session,
            scope,
            leave,
            at(0),
        );
        assert_eq!(asked, Err(Refusal::NoMembership), "{scope:?}");
    }
}
