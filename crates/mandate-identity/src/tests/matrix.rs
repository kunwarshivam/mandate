//! ID-2: `authorize` grants exactly identity spec §4.2's matrix, checked exhaustively against the
//! table parsed from the spec itself, never from the code (E9-2, DEC-641, DEC-816).
//!
//! The parser reads the cells by the grammar DEC-641 and DEC-816 record and fails on any other text:
//!
//! - **S cell:** blank, `S`, `S for invite`, or `S for grant`.
//! - **Principal cells:** blank (denied), `✓`, `own`, `✓ (propose only)`, `✓ (org)` (org scope
//!   only), `✓ (not owner)`, or `self`; each of the five after blank but `self` grants the row (the
//!   owner-role exception of `✓ (not owner)` is `change_roles`'s, not `authorize`'s). A `self` row
//!   has `self` in all eight member columns and blank in every other, and is granted only at
//!   principal scope, to a user, with no membership read; there, an agent or a process, which has
//!   no column, is refused `no_membership`, and every other kind `forbidden` (DEC-643).
//! - **Column scopes:** OO, OA, Bill at an org's scope through an org membership; WA to Au at a
//!   workspace's scope through its membership; Cl in its token's workspace, bounded by its user's
//!   effective roles; SA in its named workspaces; HC in its own workspace; PO in its window's.
//! - **Effective roles:** a membership reaches its scope when `active` or `cooling_off`, and a role
//!   in it counts once its effective-from instant is at or before `now`.
//! - **Inactive rows:** a permission that applies "only if DEC-437 item" creates its state is
//!   refused to everyone.
//! - **Workspace pairs:** a workspace scope's (organization, workspace) pair is the route's, so a
//!   workspace under another organization than the one named, or one the deployment does not host,
//!   is refused `no_membership` for every principal kind (user, client, SA, HC, PO, agent, and
//!   process alike), on every row but an inactive one, and before any membership read (DEC-832
//!   item 2). `SCOPES` holds two such pairs, (O2, W1) and (O1, W3).
//! - **What a grant carries** (DEC-642, DEC-832): an `own` row or the leave row, at any scope, and
//!   a `self` row yield `Authorized::Principal` with a `PrincipalContext` bound to the authenticated
//!   principal and the scope; any other org-scope row `Authorized::Org` with an `OrgContext`
//!   carrying the organization and its workspaces as the store holds them; any other
//!   workspace-scope row `Authorized::Workspace` with the `TenantContext` of the pair.

use std::collections::{BTreeMap, BTreeSet};

use mandate_time::UtcNanos;

use crate::{
    Authorized, EveryWorkspace, Membership, MembershipLookup, MembershipState, OneWorkspace,
    Permission, Principal, PrincipalId, PrincipalKind, Refusal, Role, Scope, Session, SessionKind,
    SessionRef, StepUp, WorkspaceId, authorize,
};

use super::grammar::{Row, matrix};
use super::rows::{ORG_ROLES, RISK_REDUCING, ROLE_COLUMNS, ROWS};
use super::{ByMember, Everything, Failing, O1, O2, Unreadable, W1, W2, W3, hosted_in, paired};

const USER: PrincipalId = PrincipalId(0x31);
const DECOY: PrincipalId = PrincipalId(0x32);
const OTHER: PrincipalId = PrincipalId(0x33);
const SESSION: SessionRef = SessionRef(0x41);
const NOW_SECS: i64 = 1_790_000_000;

/// The tests' clock reading.
fn now() -> UtcNanos {
    UtcNanos::from_parts(NOW_SECS, 0).unwrap()
}

/// One nanosecond after `now`: a cool-off that has not ended yet.
fn just_after_now() -> UtcNanos {
    UtcNanos::from_parts(NOW_SECS, 1).unwrap()
}

/// One day before `now`: a role whose cool-off ended well before the request, which still counts.
fn a_day_before_now() -> UtcNanos {
    UtcNanos::from_parts(NOW_SECS - 86_400, 0).unwrap()
}

const WS1: Scope = Scope::Workspace {
    org: O1,
    workspace: W1,
};

/// Every scope the tests ask at: the principal's, both organizations, the three hosted pairs, and
/// two pairs the store does not hold, a workspace named under the other organization.
const SCOPES: [Scope; 8] = [
    Scope::Principal,
    Scope::Org(O1),
    Scope::Org(O2),
    WS1,
    Scope::Workspace {
        org: O1,
        workspace: W2,
    },
    Scope::Workspace {
        org: O2,
        workspace: W3,
    },
    Scope::Workspace {
        org: O2,
        workspace: W1,
    },
    Scope::Workspace {
        org: O1,
        workspace: W3,
    },
];
/// What `authorize` must answer: the step-up requirement, or the refusal (DEC-643).
type Expected = Result<StepUp, Refusal>;

fn reaches(m: &Membership) -> bool {
    matches!(
        m.state,
        MembershipState::Active | MembershipState::CoolingOff
    )
}

/// The roles of `m` that count at the tests' `now`.
fn effective(m: &Membership) -> BTreeSet<Role> {
    match reaches(m) {
        true => m
            .roles
            .iter()
            .filter(|(_, from)| **from <= now())
            .map(|(r, _)| *r)
            .collect(),
        false => BTreeSet::new(),
    }
}

/// The user's own grant at `scope`, from its own memberships only, org columns at org scope.
fn user_expected(row: &Row, ms: &[Membership], member: PrincipalId, scope: Scope) -> Expected {
    let org_scope = matches!(scope, Scope::Org(_));
    let Some(m) = ms
        .iter()
        .find(|m| m.member == member && m.scope == scope && reaches(m))
    else {
        return Err(Refusal::NoMembership);
    };
    let granted = ROLE_COLUMNS.iter().any(|(column, role)| {
        ORG_ROLES.contains(role) == org_scope
            && effective(m).contains(role)
            && row.grants(column, org_scope)
    });
    if granted {
        Ok(row.step_up)
    } else {
        Err(Refusal::Forbidden)
    }
}

/// The answer for a full session; `expected_in` applies a reduction-only session's limit.
fn expected(
    row: &Row,
    principal: &Principal,
    memberships: &[Membership],
    scope: Scope,
) -> Expected {
    if row.inactive {
        return Err(Refusal::InactivePermission);
    }
    if !paired(scope) {
        return Err(Refusal::NoMembership);
    }
    expected_unpaired(row, principal, memberships, scope)
}

/// `expected` past its inactive-row and pair checks: what the memberships and the column say, with
/// the scope's pair taken as the route names it.
fn expected_unpaired(
    row: &Row,
    principal: &Principal,
    memberships: &[Membership],
    scope: Scope,
) -> Expected {
    if scope == Scope::Principal {
        return match (row.self_row, principal) {
            (false, _) => Err(Refusal::NoMembership),
            (true, Principal::User { .. }) => Ok(row.step_up),
            (true, Principal::Agent { .. } | Principal::Process { .. }) => {
                Err(Refusal::NoMembership)
            }
            (true, _) => Err(Refusal::Forbidden),
        };
    }
    let in_workspace =
        |w: WorkspaceId| matches!(scope, Scope::Workspace { workspace, .. } if workspace == w);
    let column = |name: &str, inside: bool| match (inside, row.grants(name, false)) {
        (false, _) => Err(Refusal::NoMembership),
        (true, true) => Ok(row.step_up),
        (true, false) => Err(Refusal::Forbidden),
    };
    match principal {
        Principal::User { id } => user_expected(row, memberships, *id, scope),
        Principal::Client {
            on_behalf_of,
            workspace,
            ..
        } => {
            match (
                in_workspace(*workspace),
                user_expected(row, memberships, *on_behalf_of, scope),
            ) {
                (false, _) | (true, Err(Refusal::NoMembership)) => Err(Refusal::NoMembership),
                (true, Ok(_)) if row.grants("Cl", false) => Ok(row.step_up),
                (true, _) => Err(Refusal::Forbidden),
            }
        }
        Principal::ServiceAccount { workspaces, .. } => {
            column("SA", workspaces.iter().any(|w| in_workspace(*w)))
        }
        Principal::HostCli { workspace, .. } => column("HC", in_workspace(*workspace)),
        Principal::PlatformOperator { window, .. } => {
            column("PO", window.is_some_and(in_workspace))
        }
        Principal::Agent { .. } | Principal::Process { .. } => Err(Refusal::NoMembership),
    }
}

/// `expected`, with a reduction-only session's limit to its own rows applied.
fn expected_in(
    row: &Row,
    principal: &Principal,
    ms: &[Membership],
    scope: Scope,
    kind: SessionKind,
) -> Expected {
    reduction_limited(row, expected(row, principal, ms, scope), kind)
}

/// A reduction-only session's limit to its own rows, applied to an answer.
fn reduction_limited(row: &Row, answer: Expected, kind: SessionKind) -> Expected {
    match answer {
        Ok(_) if kind == SessionKind::ReductionOnly && !row.reduction => {
            Err(Refusal::ReductionOnly)
        }
        other => other,
    }
}

fn subsets(roles: &[Role]) -> Vec<BTreeSet<Role>> {
    (0..1u32 << roles.len())
        .map(|bits| {
            roles
                .iter()
                .enumerate()
                .filter(|(i, _)| bits >> i & 1 == 1)
                .map(|(_, r)| *r)
                .collect()
        })
        .collect()
}

/// `roles` with their effective-from instants: one nanosecond after `now` for those in `cooling`,
/// and `settled` for the rest, which is at or before `now`.
fn dated(
    roles: &BTreeSet<Role>,
    cooling: &BTreeSet<Role>,
    settled: UtcNanos,
) -> BTreeMap<Role, UtcNanos> {
    roles
        .iter()
        .map(|r| match cooling.contains(r) {
            true => (*r, just_after_now()),
            false => (*r, settled),
        })
        .collect()
}

/// A membership whose roles not in `cooling` became effective exactly at `now`, so the boundary
/// itself is effective.
fn membership(
    member: PrincipalId,
    scope: Scope,
    state: MembershipState,
    roles: &BTreeSet<Role>,
    cooling: &BTreeSet<Role>,
) -> Membership {
    membership_since(member, scope, state, roles, cooling, now())
}

/// `membership`, with the roles not in `cooling` effective from `settled`.
fn membership_since(
    member: PrincipalId,
    scope: Scope,
    state: MembershipState,
    roles: &BTreeSet<Role>,
    cooling: &BTreeSet<Role>,
    settled: UtcNanos,
) -> Membership {
    Membership {
        member,
        scope,
        state,
        roles: dated(roles, cooling, settled),
    }
}

/// The member's org membership in O1 and workspace membership in W1, both holding `roles` in
/// `state`, of which only the scope's kind may act (no inheritance either way), and none in W2,
/// O2, or W3; plus a decoy user holding every role in both, whose roles reach nobody else. The
/// member's roles not in `cooling` became effective exactly at `now`.
fn memberships(
    member: PrincipalId,
    roles: &BTreeSet<Role>,
    cooling: &BTreeSet<Role>,
    state: MembershipState,
) -> Vec<Membership> {
    memberships_since(member, roles, cooling, now(), state)
}

/// `memberships`, with the member's roles not in `cooling` effective from `settled`.
fn memberships_since(
    member: PrincipalId,
    roles: &BTreeSet<Role>,
    cooling: &BTreeSet<Role>,
    settled: UtcNanos,
    state: MembershipState,
) -> Vec<Membership> {
    let all: BTreeSet<Role> = Role::ALL.into_iter().collect();
    let none = BTreeSet::new();
    let active = MembershipState::Active;
    vec![
        membership_since(member, Scope::Org(O1), state, roles, cooling, settled),
        membership_since(member, WS1, state, roles, cooling, settled),
        membership(DECOY, Scope::Org(O1), active, &all, &none),
        membership(DECOY, WS1, active, &all, &none),
    ]
}

/// A session of `kind` whose roles snapshot is `snapshot`.
pub(crate) fn session(kind: SessionKind, snapshot: Vec<Membership>) -> Session {
    Session {
        reference: SESSION,
        kind,
        snapshot,
    }
}

/// A snapshot wider than any live membership the tests read, so a step that trusts the snapshot
/// while the live read succeeds is caught.
fn wide_snapshot() -> Vec<Membership> {
    let all: BTreeSet<Role> = Role::ALL.into_iter().collect();
    memberships(USER, &all, &BTreeSet::new(), MembershipState::Active)
        .into_iter()
        .filter(|m| m.member == USER)
        .collect()
}

const SESSION_KINDS: [SessionKind; 2] = [SessionKind::Full, SessionKind::ReductionOnly];

fn check(
    rows: &[Row],
    principal: &Principal,
    lookup: &impl MembershipLookup,
    ms: &[Membership],
    checked: &mut u64,
) {
    let sessions = SESSION_KINDS.map(|k| (k, session(k, wide_snapshot())));
    for (row, (kind_of_session, session), scope) in rows
        .iter()
        .flat_map(|r| sessions.iter().map(move |k| (r, k)))
        .flat_map(|(r, k)| SCOPES.map(|s| (r, k, s)))
    {
        let kind_of_session = *kind_of_session;
        let want = expected_in(row, principal, ms, scope, kind_of_session);
        let got = authorize(lookup, principal, session, scope, row.permission, now());
        let summary = got.as_ref().map(|a| a.step_up()).map_err(|r| *r);
        assert_eq!(
            summary, want,
            "{principal:?} {:?} at {scope:?} in a {kind_of_session:?} session with {ms:?}",
            row.permission
        );
        if let Ok(a) = &got {
            carries(a, row, principal, scope, false);
        }
        *checked += 1;
    }
}

/// Asserts the arm a grant at `scope` must take and what its context carries: the authenticated
/// principal, its kind, the session, the row, the scope (the organization's workspaces as the store
/// holds them, or none read during an outage), and whether the snapshot answered.
fn carries(a: &Authorized, row: &Row, principal: &Principal, scope: Scope, unverified: bool) {
    let who = (id(principal), kind(principal), SESSION, row.permission);
    match (scope, a) {
        (_, Authorized::Principal { context: c, .. })
            if row.self_row == (scope == Scope::Principal) && (row.self_row || row.own_row) =>
        {
            assert_eq!(
                (c.principal(), c.kind(), c.session(), c.permission()),
                who,
                "{scope:?}"
            );
            assert_eq!(
                (c.scope(), c.membership_unverified()),
                (scope, unverified),
                "{:?}",
                row.permission
            );
        }
        (Scope::Org(org), Authorized::Org { context: c, .. }) if !row.own_row => {
            assert_eq!(
                (c.principal(), c.kind(), c.session(), c.permission()),
                who,
                "{scope:?}"
            );
            let set = hosted_in(org);
            assert_eq!(
                (c.org(), c.workspaces(), c.membership_unverified()),
                (org, (!unverified).then_some(&set), unverified),
                "{:?}",
                row.permission
            );
        }
        (Scope::Workspace { org, workspace }, Authorized::Workspace { tenant: t, .. })
            if !row.own_row =>
        {
            assert_eq!(
                (t.principal(), t.kind(), t.session(), t.permission()),
                who,
                "{scope:?}"
            );
            assert_eq!(
                (t.org(), t.workspace(), t.membership_unverified()),
                (org, workspace, unverified),
                "{:?}",
                row.permission
            );
        }
        (s, a) => panic!("{:?} at {s:?} gave {a:?}", row.permission),
    }
}

fn id(p: &Principal) -> PrincipalId {
    match p {
        Principal::User { id }
        | Principal::Client { id, .. }
        | Principal::ServiceAccount { id, .. }
        | Principal::Agent { id }
        | Principal::Process { id }
        | Principal::HostCli { id, .. }
        | Principal::PlatformOperator { id, .. } => *id,
    }
}

fn kind(p: &Principal) -> PrincipalKind {
    match p {
        Principal::User { .. } => PrincipalKind::User,
        Principal::Client { .. } => PrincipalKind::Client,
        Principal::ServiceAccount { .. } => PrincipalKind::ServiceAccount,
        Principal::Agent { .. } => PrincipalKind::Agent,
        Principal::Process { .. } => PrincipalKind::Process,
        Principal::HostCli { .. } => PrincipalKind::HostCli,
        Principal::PlatformOperator { .. } => PrincipalKind::PlatformOperator,
    }
}

/// The grammar holds over the whole table, the table grants something in every column, and each
/// risk-reducing name matches exactly one parsed row, so the exhaustive tests are not vacuous.
#[test]
fn the_matrix_parses_by_its_grammar_and_every_column_grants() {
    let rows = matrix();
    for column in [
        "OO", "OA", "Bill", "WA", "Op", "Ap", "Vi", "Au", "Cl", "SA", "HC", "PO",
    ] {
        assert!(
            rows.iter().any(|r| r.grants(column, true)),
            "column {column} grants nothing"
        );
    }
    assert_eq!(
        rows.iter().filter(|r| r.inactive).count(),
        1,
        "only the halted-scope row is inactive"
    );
    for name in RISK_REDUCING {
        let named: Vec<Permission> = ROWS
            .iter()
            .filter(|(prefix, _)| name.starts_with(prefix))
            .map(|(_, p)| *p)
            .collect();
        assert_eq!(named.len(), 1, "{name:?} names exactly one row");
        let parsed = rows.iter().filter(|r| named.contains(&r.permission));
        assert_eq!(parsed.count(), 1, "{name:?} matches exactly one parsed row");
    }
    assert_eq!(
        rows.iter().filter(|r| r.risk_reducing).count(),
        RISK_REDUCING.len(),
        "every risk-reducing name is the start of exactly one parsed row's text"
    );
}

#[test]
fn id2_authorize_grants_exactly_the_matrix_for_every_role_set_kind_and_scope() {
    let rows = matrix();
    let mut checked = 0u64;
    let reviewable = BTreeSet::from([Role::Operator, Role::Approver]);
    for roles in subsets(&Role::ALL) {
        let coolings = [
            (BTreeSet::new(), now()),
            (BTreeSet::new(), a_day_before_now()),
            (
                roles.intersection(&reviewable).copied().collect(),
                a_day_before_now(),
            ),
            (roles.clone(), now()),
        ];
        for state in [
            MembershipState::Invited,
            MembershipState::CoolingOff,
            MembershipState::Active,
            MembershipState::Deactivated,
            MembershipState::Removed,
            MembershipState::Expired,
            MembershipState::Revoked,
        ] {
            for (cooling, settled) in &coolings {
                let ms = memberships_since(USER, &roles, cooling, *settled, state);
                let user = Principal::User { id: USER };
                check(&rows, &user, &ByMember(&ms), &ms, &mut checked);
                check(&rows, &user, &Everything(&ms), &ms, &mut checked);
                let client = Principal::Client {
                    id: OTHER,
                    on_behalf_of: USER,
                    workspace: W1,
                };
                check(&rows, &client, &Everything(&ms), &ms, &mut checked);
            }
        }
    }
    let everything = memberships(
        OTHER,
        &Role::ALL.into_iter().collect(),
        &BTreeSet::new(),
        MembershipState::Active,
    );
    for principal in [
        Principal::User {
            id: PrincipalId(0x99),
        },
        Principal::ServiceAccount {
            id: OTHER,
            workspaces: BTreeSet::from([W1, W3]),
        },
        Principal::ServiceAccount {
            id: OTHER,
            workspaces: BTreeSet::new(),
        },
        Principal::HostCli {
            id: OTHER,
            workspace: W1,
            on_behalf_of: DECOY,
        },
        Principal::PlatformOperator {
            id: OTHER,
            window: Some(W1),
        },
        Principal::PlatformOperator {
            id: OTHER,
            window: None,
        },
        Principal::Agent { id: OTHER },
        Principal::Process { id: OTHER },
    ] {
        check(
            &rows,
            &principal,
            &ByMember(&everything),
            &everything,
            &mut checked,
        );
    }
    assert!(checked > 15_000_000, "only {checked} cases checked");
}

/// A failed membership read never refuses a risk-reducing row (identity spec §4.5, DEC-642 item 10):
/// a user or a client is authorized from its session's roles snapshot against the same row, cooling
/// roles included, flagged `membership_unverified`, and refused `membership_unavailable` on every
/// other row; a reduction-only session still reaches only its rows; principal scope reads nothing;
/// and the principals that never read memberships answer as their column says.
#[test]
fn a_failed_membership_read_never_refuses_risk_reduction_and_refuses_the_rest() {
    let rows = matrix();
    let user = Principal::User { id: USER };
    let client = Principal::Client {
        id: OTHER,
        on_behalf_of: USER,
        workspace: W1,
    };
    let none: [Membership; 0] = [];
    let reviewable = BTreeSet::from([Role::Operator, Role::Approver]);
    let mut checked = 0u64;
    for roles in subsets(&Role::ALL) {
        let coolings = [
            (BTreeSet::new(), now()),
            (
                roles.intersection(&reviewable).copied().collect(),
                a_day_before_now(),
            ),
        ];
        for (cooling, settled) in coolings {
            let snapshot: Vec<Membership> =
                memberships_since(USER, &roles, &cooling, settled, MembershipState::Active)
                    .into_iter()
                    .filter(|m| m.member == USER)
                    .collect();
            for kind_of_session in SESSION_KINDS {
                let session = session(kind_of_session, snapshot.clone());
                for row in &rows {
                    for principal in [&user, &client] {
                        for scope in SCOPES {
                            let client_outside =
                                matches!(principal, Principal::Client { .. }) && scope != WS1;
                            let read =
                                scope != Scope::Principal && !client_outside && paired(scope);
                            let want = match (row.inactive, read, row.risk_reducing) {
                                (true, _, _) => Err(Refusal::InactivePermission),
                                (false, false, _) | (false, true, true) => {
                                    expected_in(row, principal, &snapshot, scope, kind_of_session)
                                }
                                (false, true, false) => Err(Refusal::MembershipUnavailable),
                            };
                            let got = authorize(
                                &Failing,
                                principal,
                                &session,
                                scope,
                                row.permission,
                                now(),
                            );
                            if let Ok(a) = &got {
                                carries(a, row, principal, scope, read);
                            }
                            assert_eq!(
                                got.map(|a| a.step_up()),
                                want,
                                "{principal:?} {:?} at {scope:?} {kind_of_session:?} from \
                                 {roles:?} cooling {cooling:?}",
                                row.permission
                            );
                            checked += 1;
                        }
                    }
                }
            }
        }
    }
    for kind_of_session in SESSION_KINDS {
        let session = session(kind_of_session, wide_snapshot());
        for principal in [
            Principal::ServiceAccount {
                id: OTHER,
                workspaces: BTreeSet::from([W1]),
            },
            Principal::HostCli {
                id: OTHER,
                workspace: W1,
                on_behalf_of: DECOY,
            },
            Principal::PlatformOperator {
                id: OTHER,
                window: Some(W1),
            },
            Principal::Agent { id: OTHER },
            Principal::Process { id: OTHER },
        ] {
            for row in &rows {
                for scope in SCOPES {
                    let want = expected_in(row, &principal, &none, scope, kind_of_session);
                    let got =
                        authorize(&Failing, &principal, &session, scope, row.permission, now());
                    if let Ok(a) = &got {
                        carries(a, row, &principal, scope, false);
                    }
                    assert_eq!(got.map(|a| a.step_up()), want, "{principal:?} {scope:?}");
                }
            }
        }
    }
    assert!(checked > 400_000, "only {checked} cases checked");
}

/// A snapshot entry serves only its own scope (DEC-816): with W1's entry holding every role and
/// W2's none, a request in W2 during an outage gets nothing from W1's roles.
#[test]
fn a_snapshot_entry_never_serves_another_workspace() {
    let ws2 = Scope::Workspace {
        org: O1,
        workspace: W2,
    };
    let all: BTreeSet<Role> = Role::ALL.into_iter().collect();
    let none = BTreeSet::new();
    let active = MembershipState::Active;
    let snapshot = vec![
        membership(USER, WS1, active, &all, &none),
        membership(USER, ws2, active, &none, &none),
    ];
    let user = Principal::User { id: USER };
    for kind_of_session in SESSION_KINDS {
        let session = session(kind_of_session, snapshot.clone());
        for permission in [
            Permission::Pause,
            Permission::KillSwitchAgent,
            Permission::HoldNewOpenings,
        ] {
            let got = authorize(&Failing, &user, &session, ws2, permission, now());
            assert_eq!(
                got.map(|a| a.step_up()),
                Err(Refusal::Forbidden),
                "{permission:?}"
            );
            let got = authorize(&Failing, &user, &session, WS1, permission, now());
            let want = match (kind_of_session, permission) {
                (SessionKind::ReductionOnly, Permission::HoldNewOpenings) => {
                    Err(Refusal::ReductionOnly)
                }
                _ => Ok(StepUp::NotRequired),
            };
            assert_eq!(got.map(|a| a.step_up()), want, "{permission:?} in W1");
        }
    }
}

/// An org-scope grant reaches workspaces only through the set the store enumerated (DEC-832 items
/// 2 to 4, `org_fanout_workspaces_come_from_the_store`, this crate's part): the route's workspace
/// only when it is in the set, so another organization's workspace and one the deployment does not
/// host are refused `no_membership`, and the every-workspace way yields exactly the set, each
/// context bound to the same principal, session, and row. During a membership-store outage the
/// org kill switch is still authorized from the snapshot, with no set, and the route's workspace is
/// checked against its own record instead.
#[test]
fn org_fanout_workspaces_come_from_the_store() {
    let owner = BTreeSet::from([Role::OrgOwner]);
    let none = BTreeSet::new();
    let ms = vec![membership(
        USER,
        Scope::Org(O1),
        MembershipState::Active,
        &owner,
        &none,
    )];
    let user = Principal::User { id: USER };
    let full = session(SessionKind::Full, ms.clone());
    let unhosted = WorkspaceId(0x29);
    let outage = Failing;
    let reachable = ByMember(&ms);
    for (name, unverified) in [("live", false), ("outage", true)] {
        let org_context = || {
            let got = match unverified {
                false => authorize(
                    &reachable,
                    &user,
                    &full,
                    Scope::Org(O1),
                    Permission::KillSwitchOrg,
                    now(),
                ),
                true => authorize(
                    &outage,
                    &user,
                    &full,
                    Scope::Org(O1),
                    Permission::KillSwitchOrg,
                    now(),
                ),
            };
            match got {
                Ok(Authorized::Org { context, .. }) => context,
                other => panic!("the {name} org kill switch at O1 gave {other:?}"),
            }
        };
        let into = |workspace| match unverified {
            false => org_context().into_workspace(&reachable, workspace),
            true => org_context().into_workspace(&outage, workspace),
        };
        for workspace in [W1, W2] {
            let tenant = match into(workspace) {
                Ok(OneWorkspace::Context(tenant)) => tenant,
                other => panic!("{name} {workspace:?} gave {other:?}"),
            };
            assert_eq!(
                (
                    tenant.org(),
                    tenant.workspace(),
                    tenant.principal(),
                    tenant.kind(),
                    tenant.session(),
                    tenant.permission(),
                    tenant.membership_unverified()
                ),
                (
                    O1,
                    workspace,
                    USER,
                    PrincipalKind::User,
                    SESSION,
                    Permission::KillSwitchOrg,
                    unverified
                ),
                "{name} {workspace:?}"
            );
        }
        for workspace in [W3, unhosted] {
            assert_eq!(
                into(workspace),
                Err(Refusal::NoMembership),
                "{name} {workspace:?}"
            );
        }
    }
    let every = match authorize(
        &reachable,
        &user,
        &full,
        Scope::Org(O1),
        Permission::KillSwitchOrg,
        now(),
    ) {
        Ok(Authorized::Org { context, .. }) => match context.into_every_workspace() {
            Ok(EveryWorkspace::Contexts(every)) => every,
            other => panic!("the live every-workspace way gave {other:?}"),
        },
        other => panic!("the org kill switch at O1 gave {other:?}"),
    };
    let reached: Vec<(WorkspaceId, PrincipalId, SessionRef, Permission)> = every
        .iter()
        .map(|t| {
            assert_eq!(t.org(), O1);
            (t.workspace(), t.principal(), t.session(), t.permission())
        })
        .collect();
    assert_eq!(
        reached,
        vec![
            (W1, USER, SESSION, Permission::KillSwitchOrg),
            (W2, USER, SESSION, Permission::KillSwitchOrg),
        ]
    );
}

/// When a workspace's own record cannot be read, its pair cannot be checked (identity spec §4.5,
/// DEC-832, `AGENTS.md` rule 13), ordered after an inactive row and before everything else: a row
/// that is not risk-reducing is refused `membership_unavailable` for every principal kind, and a
/// risk-reducing row never is. A user or a client is authorized from its session's roles snapshot
/// as DEC-642 item 10 says, the snapshot entry's own pair standing in for the record, so a pair the
/// snapshot does not hold reaches nothing; a service account, the host CLI, and a platform
/// operator by their columns; each grant flagged `membership_unverified`; and a reduction-only
/// session still reaches only its rows.
#[test]
fn an_unreadable_workspace_record_never_refuses_risk_reduction() {
    let rows = matrix();
    let user = Principal::User { id: USER };
    let client = Principal::Client {
        id: OTHER,
        on_behalf_of: USER,
        workspace: W1,
    };
    let columns = [
        Principal::ServiceAccount {
            id: OTHER,
            workspaces: BTreeSet::from([W1]),
        },
        Principal::HostCli {
            id: OTHER,
            workspace: W1,
            on_behalf_of: DECOY,
        },
        Principal::PlatformOperator {
            id: OTHER,
            window: Some(W1),
        },
        Principal::Agent { id: OTHER },
        Principal::Process { id: OTHER },
    ];
    let no_memberships: [Membership; 0] = [];
    let role_sets = [
        BTreeSet::new(),
        BTreeSet::from([Role::Operator]),
        BTreeSet::from([Role::WorkspaceAdmin, Role::Approver]),
        Role::ALL.into_iter().collect(),
    ];
    let mut checked = 0u64;
    for roles in role_sets {
        let snapshot: Vec<Membership> =
            memberships(USER, &roles, &BTreeSet::new(), MembershipState::Active)
                .into_iter()
                .filter(|m| m.member == USER)
                .collect();
        for kind_of_session in SESSION_KINDS {
            let session = session(kind_of_session, snapshot.clone());
            for row in &rows {
                for scope in SCOPES
                    .into_iter()
                    .filter(|s| matches!(s, Scope::Workspace { .. }))
                {
                    for principal in [&user, &client].into_iter().chain(columns.iter()) {
                        let ms: &[Membership] = match principal {
                            Principal::User { .. } | Principal::Client { .. } => &snapshot,
                            _ => &no_memberships,
                        };
                        let want = match (row.inactive, row.risk_reducing) {
                            (true, _) => Err(Refusal::InactivePermission),
                            (false, false) => Err(Refusal::MembershipUnavailable),
                            (false, true) => reduction_limited(
                                row,
                                expected_unpaired(row, principal, ms, scope),
                                kind_of_session,
                            ),
                        };
                        let got = authorize(
                            &Unreadable,
                            principal,
                            &session,
                            scope,
                            row.permission,
                            now(),
                        );
                        if let Ok(a) = &got {
                            carries(a, row, principal, scope, true);
                        }
                        assert_eq!(
                            got.map(|a| a.step_up()),
                            want,
                            "{principal:?} {:?} at {scope:?} {kind_of_session:?} from {roles:?}",
                            row.permission
                        );
                        checked += 1;
                    }
                }
            }
        }
    }
    assert!(checked > 5_000, "only {checked} cases checked");
}

/// The org kill switch's `OrgContext` at O1 for a user holding the org owner role there.
fn org_kill_switch(lookup: &impl MembershipLookup, session: &Session) -> crate::OrgContext {
    match authorize(
        lookup,
        &Principal::User { id: USER },
        session,
        Scope::Org(O1),
        Permission::KillSwitchOrg,
        now(),
    ) {
        Ok(Authorized::Org { context, .. }) => context,
        other => panic!("the org kill switch at O1 gave {other:?}"),
    }
}

/// The org owner's memberships: an active org owner role in O1, and nothing else.
fn org_owner() -> Vec<Membership> {
    vec![membership(
        USER,
        Scope::Org(O1),
        MembershipState::Active,
        &BTreeSet::from([Role::OrgOwner]),
        &BTreeSet::new(),
    )]
}

/// The one-workspace way never refuses for an outage (identity spec §4.5, DEC-832 item 4): an
/// `OrgContext` built while nothing could be read carries no set and is flagged, and with the route
/// workspace's own record unreadable too, every route workspace is pending, never `no_membership`
/// and never granted unchecked, so one organization's kill switch cannot reach another's
/// workspace; a context built live checks the route's workspace against its set alone, needing no
/// record.
#[test]
fn an_org_route_with_no_set_and_no_record_is_pending_never_refused() {
    let ms = org_owner();
    let unhosted = WorkspaceId(0x29);
    for kind_of_session in SESSION_KINDS {
        let session = session(kind_of_session, ms.clone());
        for workspace in [W1, W2, W3, unhosted] {
            let context = org_kill_switch(&Unreadable, &session);
            assert_eq!(
                (context.workspaces(), context.membership_unverified()),
                (None, true),
                "{kind_of_session:?}"
            );
            assert_eq!(
                context.into_workspace(&Unreadable, workspace),
                Ok(OneWorkspace::Pending),
                "{kind_of_session:?} {workspace:?}"
            );
        }
        for (workspace, in_set) in [(W1, true), (W2, true), (W3, false), (unhosted, false)] {
            let got =
                org_kill_switch(&ByMember(&ms), &session).into_workspace(&Unreadable, workspace);
            match (in_set, got) {
                (true, Ok(OneWorkspace::Context(t))) => assert_eq!(
                    (
                        t.org(),
                        t.workspace(),
                        t.principal(),
                        t.membership_unverified()
                    ),
                    (O1, workspace, USER, false),
                    "{kind_of_session:?}"
                ),
                (false, Err(Refusal::NoMembership)) => {}
                (_, other) => panic!("{kind_of_session:?} {workspace:?} gave {other:?}"),
            }
        }
    }
}

/// The every-workspace way during an outage (identity spec §4.5, DEC-832 item 5): with no set
/// enumerated it answers `NoSetYet`, never a refusal, a caller-named set, or part of one; live, it
/// answers exactly the store's set, unflagged.
#[test]
fn every_workspace_has_no_set_during_an_outage_and_the_full_set_live() {
    let ms = org_owner();
    let session = session(SessionKind::Full, ms.clone());
    assert_eq!(
        org_kill_switch(&Failing, &session).into_every_workspace(),
        Ok(EveryWorkspace::NoSetYet)
    );
    assert_eq!(
        org_kill_switch(&Unreadable, &session).into_every_workspace(),
        Ok(EveryWorkspace::NoSetYet)
    );
    match org_kill_switch(&ByMember(&ms), &session).into_every_workspace() {
        Ok(EveryWorkspace::Contexts(every)) => assert_eq!(
            every
                .iter()
                .map(|t| (t.org(), t.workspace(), t.membership_unverified()))
                .collect::<Vec<_>>(),
            vec![(O1, W1, false), (O1, W2, false)]
        ),
        other => panic!("the live every-workspace way gave {other:?}"),
    }
}
