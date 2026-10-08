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
//!   principal scope, to a user, with no membership read.
//! - **Column scopes:** OO, OA, Bill at an org's scope through an org membership; WA to Au at a
//!   workspace's scope through its membership; Cl in its token's workspace, bounded by its user's
//!   effective roles; SA in its named workspaces; HC in its own workspace; PO in its window's.
//! - **Effective roles:** a membership reaches its scope when `active` or `cooling_off`, and a role
//!   in it counts once its effective-from instant is at or before `now`.
//! - **Inactive rows:** a permission that applies "only if DEC-437 item" creates its state is
//!   refused to everyone.

use std::collections::{BTreeMap, BTreeSet};

use mandate_time::UtcNanos;

use crate::{
    Authorized, Membership, MembershipLookup, MembershipState, OrgId, Permission, Principal,
    PrincipalId, PrincipalKind, Refusal, Role, Scope, Session, SessionKind, SessionRef, StepUp,
    WorkspaceId, authorize,
};

use super::rows::{ORG_ROLES, OWED, RISK_REDUCING, ROLE_COLUMNS, ROWS};
use super::{ByMember, Everything, Failing};

const SPEC: &str = include_str!("../../../../docs/specs/identity.md");

const O1: OrgId = OrgId(0x11);
const O2: OrgId = OrgId(0x12);
const W1: WorkspaceId = WorkspaceId(0x21);
const W2: WorkspaceId = WorkspaceId(0x22);
const W3: WorkspaceId = WorkspaceId(0x23);
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

const WS1: Scope = Scope::Workspace {
    org: O1,
    workspace: W1,
};

const SCOPES: [Scope; 6] = [
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
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cell {
    Blank,
    Grant,
    OrgOnly,
    SelfOnly,
}

struct Row {
    permission: Permission,
    step_up: StepUp,
    inactive: bool,
    reduction: bool,
    risk_reducing: bool,
    self_row: bool,
    cells: BTreeMap<String, Cell>,
}

impl Row {
    fn grants(&self, column: &str, org_scope: bool) -> bool {
        match self.cells.get(column).copied().unwrap_or(Cell::Blank) {
            Cell::Blank | Cell::SelfOnly => false,
            Cell::Grant => true,
            Cell::OrgOnly => org_scope,
        }
    }
}

fn cell(text: &str) -> Cell {
    match text {
        "" => Cell::Blank,
        "✓" | "own" | "✓ (propose only)" | "✓ (not owner)" => Cell::Grant,
        "✓ (org)" => Cell::OrgOnly,
        "self" => Cell::SelfOnly,
        other => panic!("§4.2 cell {other:?} is outside DEC-641's grammar"),
    }
}

fn step_up(text: &str) -> StepUp {
    match text {
        "" => StepUp::NotRequired,
        "S" => StepUp::Required,
        "S for invite" => StepUp::ForInvite,
        "S for grant" => StepUp::ForGrant,
        other => panic!("§4.2 S cell {other:?} is outside DEC-641's grammar"),
    }
}

/// The rows of §4.2, from the section's one table.
fn matrix() -> Vec<Row> {
    let section = SPEC
        .split("### 4.2 Permission matrix")
        .nth(1)
        .and_then(|rest| rest.split("\n### ").next())
        .expect("identity spec §4.2");
    let mut lines = section.lines().filter(|l| l.starts_with('|'));
    let split = |l: &str| -> Vec<String> {
        let inner = l.trim().trim_start_matches('|').trim_end_matches('|');
        inner.split('|').map(|c| c.trim().to_owned()).collect()
    };
    let header = split(lines.next().expect("the matrix header"));
    assert_eq!(
        header.get(..2),
        Some(&["Permission".to_owned(), "S".to_owned()][..])
    );
    let members: Vec<&str> = ROLE_COLUMNS.iter().map(|(c, _)| *c).collect();
    let mut used = BTreeSet::new();
    let rows: Vec<Row> = lines
        .filter(|l| !l.starts_with("|---"))
        .map(|l| {
            let texts = split(l);
            assert_eq!(texts.len(), header.len(), "row {l}");
            let text = &texts[0];
            let matches: Vec<Permission> = ROWS
                .iter()
                .filter(|(prefix, _)| text.starts_with(prefix))
                .map(|(_, p)| *p)
                .collect();
            assert_eq!(matches.len(), 1, "row {text:?} must match one permission");
            assert!(used.insert(matches[0]), "row {text:?} repeats a permission");
            let cells: BTreeMap<String, Cell> = header[2..]
                .iter()
                .cloned()
                .zip(texts[2..].iter().map(|c| cell(c)))
                .collect();
            let self_row = cells.values().any(|c| *c == Cell::SelfOnly);
            if self_row {
                for (column, c) in &cells {
                    let want = match members.contains(&column.as_str()) {
                        true => Cell::SelfOnly,
                        false => Cell::Blank,
                    };
                    assert_eq!(*c, want, "the self row {text:?} at {column}");
                }
            }
            Row {
                permission: matches[0],
                step_up: step_up(&texts[1]),
                inactive: text.contains("only if DEC-437 item"),
                risk_reducing: RISK_REDUCING.iter().any(|p| text.starts_with(p)),
                reduction: text.starts_with("Pause")
                    || (text.starts_with("Kill switch") && text.ends_with(": engage")),
                self_row,
                cells,
            }
        })
        .collect();
    for (prefix, permission) in ROWS {
        assert!(
            used.contains(&permission) || OWED.contains(&permission),
            "§4.2 has no row {prefix:?}"
        );
    }
    rows
}

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
    if scope == Scope::Principal {
        return match (row.self_row, principal) {
            (false, _) => Err(Refusal::NoMembership),
            (true, Principal::User { .. }) => Ok(row.step_up),
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
    match expected(row, principal, ms, scope) {
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
/// exactly `now` for the rest, so the boundary itself is effective.
fn dated(roles: &BTreeSet<Role>, cooling: &BTreeSet<Role>) -> BTreeMap<Role, UtcNanos> {
    roles
        .iter()
        .map(|r| match cooling.contains(r) {
            true => (*r, just_after_now()),
            false => (*r, now()),
        })
        .collect()
}

fn membership(
    member: PrincipalId,
    scope: Scope,
    state: MembershipState,
    roles: &BTreeSet<Role>,
    cooling: &BTreeSet<Role>,
) -> Membership {
    Membership {
        member,
        scope,
        state,
        roles: dated(roles, cooling),
    }
}

/// The member's org membership in O1 and workspace membership in W1, both holding `roles` in
/// `state`, of which only the scope's kind may act (no inheritance either way), and none in W2,
/// O2, or W3; plus a decoy user holding every role in both, whose roles reach nobody else.
fn memberships(
    member: PrincipalId,
    roles: &BTreeSet<Role>,
    cooling: &BTreeSet<Role>,
    state: MembershipState,
) -> Vec<Membership> {
    let all: BTreeSet<Role> = Role::ALL.into_iter().collect();
    let none = BTreeSet::new();
    let active = MembershipState::Active;
    vec![
        membership(member, Scope::Org(O1), state, roles, cooling),
        membership(member, WS1, state, roles, cooling),
        membership(DECOY, Scope::Org(O1), active, &all, &none),
        membership(DECOY, WS1, active, &all, &none),
    ]
}

/// A session of `kind` whose roles snapshot is `snapshot`.
fn session(kind: SessionKind, snapshot: Vec<Membership>) -> Session {
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
        if let Ok(a) = got {
            match (scope, &a) {
                (Scope::Principal, Authorized::Principal { .. })
                | (Scope::Org(_), Authorized::Org { .. }) => {}
                (Scope::Workspace { org, workspace }, Authorized::Workspace { tenant: t, .. }) => {
                    assert_eq!(
                        (t.org(), t.workspace(), t.principal(), t.permission()),
                        (org, workspace, id(principal), row.permission)
                    );
                    assert_eq!(
                        (t.kind(), t.session(), t.membership_unverified()),
                        (kind(principal), SESSION, false)
                    );
                }
                (s, a) => panic!("{s:?} gave {a:?}"),
            }
        }
        *checked += 1;
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
}

#[test]
#[ignore = "pending E9-2"]
fn id2_authorize_grants_exactly_the_matrix_for_every_role_set_kind_and_scope() {
    let rows = matrix();
    let mut checked = 0u64;
    let reviewable = BTreeSet::from([Role::Operator, Role::Approver]);
    for roles in subsets(&Role::ALL) {
        let coolings = [
            BTreeSet::new(),
            roles.intersection(&reviewable).copied().collect(),
            roles.clone(),
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
            for cooling in &coolings {
                let ms = memberships(USER, &roles, cooling, state);
                let user = Principal::User { id: USER };
                check(&rows, &user, &ByMember(&ms), &ms, &mut checked);
                if matches!(state, MembershipState::Active | MembershipState::CoolingOff) {
                    check(&rows, &user, &Everything(&ms), &ms, &mut checked);
                }
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
    assert!(checked > 2_000_000, "only {checked} cases checked");
}

/// A failed membership read never refuses a risk-reducing row (identity spec §4.5, DEC-642 item 10):
/// a user or a client is authorized from its session's roles snapshot against the same row, cooling
/// roles included, flagged `membership_unverified`, and refused `membership_unavailable` on every
/// other row; a reduction-only session still reaches only its rows; principal scope reads nothing;
/// and the principals that never read memberships answer as their column says.
#[test]
#[ignore = "pending E9-2"]
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
            BTreeSet::new(),
            roles.intersection(&reviewable).copied().collect(),
        ];
        for cooling in coolings {
            let snapshot: Vec<Membership> =
                memberships(USER, &roles, &cooling, MembershipState::Active)
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
                            let read = scope != Scope::Principal && !client_outside;
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
                            if let Ok(Authorized::Workspace { tenant, .. }) = &got {
                                assert!(tenant.membership_unverified(), "the gap is flagged");
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
#[ignore = "pending E9-2"]
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
