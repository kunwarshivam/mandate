//! ID-2: `authorize` grants exactly identity spec §4.2's matrix, checked exhaustively against the
//! table parsed from the spec itself, never from the code (E9-2, DEC-641).
//!
//! The parser reads the cells by the grammar DEC-641 records and fails on any other text:
//!
//! - **S cell:** blank, `S`, `S for invite`, or `S for grant`.
//! - **Principal cells:** blank (denied), `✓`, `own`, `✓ (propose only)`, `✓ (org)` (org scope
//!   only), or `✓ (not owner)`; each of the last five grants the row (the owner-role exception of
//!   `✓ (not owner)` is `change_roles`'s, not `authorize`'s).
//! - **Column scopes:** OO, OA, Bill at an org's scope through an org membership; WA to Au at a
//!   workspace's scope through its membership; Cl in its token's workspace, bounded by its user's
//!   effective roles; SA in its named workspaces; HC in its own workspace; PO in its window's.
//! - **Inactive rows:** a permission that applies "only if DEC-437 item" creates its state is
//!   refused to everyone.

use std::collections::{BTreeMap, BTreeSet};

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

const SCOPES: [Scope; 5] = [
    Scope::Org(O1),
    Scope::Org(O2),
    Scope::Workspace {
        org: O1,
        workspace: W1,
    },
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
}

struct Row {
    permission: Permission,
    step_up: StepUp,
    inactive: bool,
    reduction: bool,
    risk_reducing: bool,
    cells: BTreeMap<String, Cell>,
}

impl Row {
    fn grants(&self, column: &str, org_scope: bool) -> bool {
        match self.cells.get(column).copied().unwrap_or(Cell::Blank) {
            Cell::Blank => false,
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
    let mut used = BTreeSet::new();
    let rows: Vec<Row> = lines
        .filter(|l| !l.starts_with("|---"))
        .map(|l| {
            let cells = split(l);
            assert_eq!(cells.len(), header.len(), "row {l}");
            let text = &cells[0];
            let matches: Vec<Permission> = ROWS
                .iter()
                .filter(|(prefix, _)| text.starts_with(prefix))
                .map(|(_, p)| *p)
                .collect();
            assert_eq!(matches.len(), 1, "row {text:?} must match one permission");
            assert!(used.insert(matches[0]), "row {text:?} repeats a permission");
            Row {
                permission: matches[0],
                step_up: step_up(&cells[1]),
                inactive: text.contains("only if DEC-437 item"),
                risk_reducing: RISK_REDUCING.iter().any(|p| text.starts_with(p)),
                reduction: text.starts_with("Pause")
                    || (text.starts_with("Kill switch") && text.ends_with(": engage")),
                cells: header[2..]
                    .iter()
                    .cloned()
                    .zip(cells[2..].iter().map(|c| cell(c)))
                    .collect(),
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

fn effective(m: &Membership) -> BTreeSet<Role> {
    let reaches = matches!(
        m.state,
        MembershipState::Active | MembershipState::CoolingOff
    );
    match reaches {
        true => m.roles.difference(&m.cooling).copied().collect(),
        false => BTreeSet::new(),
    }
}

fn reaches(m: &Membership) -> bool {
    matches!(
        m.state,
        MembershipState::Active | MembershipState::CoolingOff
    )
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

fn expected(
    row: &Row,
    principal: &Principal,
    memberships: &[Membership],
    scope: Scope,
) -> Expected {
    if row.inactive {
        return Err(Refusal::InactivePermission);
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
    let ws = Scope::Workspace {
        org: O1,
        workspace: W1,
    };
    vec![
        Membership {
            member,
            scope: Scope::Org(O1),
            state,
            roles: roles.clone(),
            cooling: cooling.clone(),
        },
        Membership {
            member,
            scope: ws,
            state,
            roles: roles.clone(),
            cooling: cooling.clone(),
        },
        Membership {
            member: DECOY,
            scope: Scope::Org(O1),
            state: MembershipState::Active,
            roles: all.clone(),
            cooling: none.clone(),
        },
        Membership {
            member: DECOY,
            scope: ws,
            state: MembershipState::Active,
            roles: all,
            cooling: none,
        },
    ]
}

/// A session of `kind` in W1 whose roles snapshot is `snapshot`.
fn session(kind: SessionKind, snapshot: Vec<Membership>) -> Session {
    Session {
        reference: SESSION,
        workspace: W1,
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
        let want = match expected(row, principal, ms, scope) {
            Ok(_) if kind_of_session == SessionKind::ReductionOnly && !row.reduction => {
                Err(Refusal::ReductionOnly)
            }
            other => other,
        };
        let got = authorize(lookup, principal, session, scope, row.permission);
        let summary = got.as_ref().map(|a| a.step_up()).map_err(|r| *r);
        assert_eq!(
            summary, want,
            "{principal:?} {:?} at {scope:?} in a {kind_of_session:?} session with {ms:?}",
            row.permission
        );
        if let Ok(a) = got {
            match (scope, &a) {
                (Scope::Org(_), Authorized::Org { .. }) => {}
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

/// The grammar holds over the whole table, and the table grants something in every column, so
/// the exhaustive test below is not vacuous.
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
/// a user or a client is authorized from its session's roles snapshot against the same row, flagged
/// `membership_unverified`, and refused `membership_unavailable` on every other row; a reduction-only
/// session still reaches only its rows, and the principals that never read memberships answer as
/// their column says. Every role set of the snapshot is tried, for both session kinds.
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
    let mut checked = 0u64;
    for roles in subsets(&Role::ALL) {
        let snapshot: Vec<Membership> =
            memberships(USER, &roles, &BTreeSet::new(), MembershipState::Active)
                .into_iter()
                .filter(|m| m.member == USER)
                .collect();
        for kind_of_session in SESSION_KINDS {
            let session = session(kind_of_session, snapshot.clone());
            for row in &rows {
                for principal in [&user, &client] {
                    for scope in SCOPES {
                        let outside = matches!(principal, Principal::Client { .. })
                            && scope
                                != (Scope::Workspace {
                                    org: O1,
                                    workspace: W1,
                                });
                        let want = match (row.inactive, outside, row.risk_reducing) {
                            (true, _, _) => Err(Refusal::InactivePermission),
                            (false, true, _) => Err(Refusal::NoMembership),
                            (false, false, false) => Err(Refusal::MembershipUnavailable),
                            (false, false, true) => {
                                match expected(row, principal, &snapshot, scope) {
                                    Ok(_)
                                        if kind_of_session == SessionKind::ReductionOnly
                                            && !row.reduction =>
                                    {
                                        Err(Refusal::ReductionOnly)
                                    }
                                    other => other,
                                }
                            }
                        };
                        let got = authorize(&Failing, principal, &session, scope, row.permission);
                        if let Ok(Authorized::Workspace { tenant, .. }) = &got {
                            assert!(tenant.membership_unverified(), "the gap is flagged");
                        }
                        let got = got.map(|a| a.step_up());
                        assert_eq!(
                            got, want,
                            "{principal:?} {:?} at {scope:?} {kind_of_session:?} from {roles:?}",
                            row.permission
                        );
                        checked += 1;
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
        ] {
            for row in &rows {
                for scope in SCOPES {
                    let want = match expected(row, &principal, &none, scope) {
                        Ok(_)
                            if kind_of_session == SessionKind::ReductionOnly && !row.reduction =>
                        {
                            Err(Refusal::ReductionOnly)
                        }
                        other => other,
                    };
                    let got = authorize(&Failing, &principal, &session, scope, row.permission);
                    assert_eq!(got.map(|a| a.step_up()), want, "{principal:?} {scope:?}");
                }
            }
        }
    }
    assert!(checked > 200_000, "only {checked} cases checked");
}
