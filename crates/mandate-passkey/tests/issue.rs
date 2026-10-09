//! Who may obtain a step-up challenge (identity spec §7.2 step 1, DEC-816 item 3, DEC-665): exactly
//! a principal granted the action's §4.2 row. The oracle parses the spec's own table and finds each
//! kind's row by its text (workspace API §3.6), never through either crate. A challenge for one
//! action consumed for another is `consume.rs`'s `a_step_up_for_another_action_is_a_mismatch`.

use mandate_canon::Digest;
use mandate_identity::MembershipState::{Active, CoolingOff, Deactivated, Invited, Removed};
use mandate_identity::Permission as P;
use mandate_identity::Refusal::{Forbidden, MembershipUnavailable, NoMembership, ReductionOnly};
use mandate_identity::SessionKind::{self, Full};
use mandate_identity::StepUpActionKind as K;
use mandate_identity::{
    AssertionId, Authorized, MembershipLookup, OrgId, Principal, PrincipalId, Refusal, Role, Scope,
    Session, SessionRef, TenantContext, WorkspaceId, authorize,
};
use mandate_identity_testkit::{FailingLookup, StaticLookup, membership, session};
use mandate_passkey::stepup::{Action, IssueRefusal, Issued, issue_challenge};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};

const SPEC: &str = include_str!("../../../docs/specs/identity.md");
const O1: OrgId = OrgId(0x11);
const W1: WorkspaceId = WorkspaceId(0x21);
const W2: WorkspaceId = WorkspaceId(0x22);
const USER: PrincipalId = PrincipalId(0x31);
const OTHER: PrincipalId = PrincipalId(0x32);
const STRANGER: PrincipalId = PrincipalId(0x33);
const SESSION: SessionRef = SessionRef(0x41);
const NOW_SECS: i64 = 1_790_000_000;

/// The start of the §4.2 row an action kind needs, and the row the caller authorizes.
fn row_of(kind: K) -> (&'static str, P) {
    match kind {
        K::ConfirmVersion => ("Confirm a version: risk", P::ConfirmRiskIncreasing),
        K::Deploy => ("Deploy, go live", P::Deploy),
        K::Approve => ("Answer an approval: approve", P::Approve),
        K::Connection => ("Connect, change, or revoke", P::BrokerConnection),
        K::Resume | K::Stop => ("Resume, Stop", P::ResumeOrStop),
        K::Acknowledge => ("Acknowledge (", P::Acknowledge),
        K::OwnerExit => ("Owner exit", P::OwnerExit),
        K::KillSwitchPrivilege => ("Kill switch, any scope", P::KillSwitchPrivileges),
        K::Disclosure => ("Accept a disclosure", P::AcceptDisclosure),
        K::PolicyLoosen => ("Workspace policy: loosen", P::WorkspacePolicyLoosen),
        K::MemberInvite => ("Invite, deactivate", P::WorkspaceMembers),
        K::RoleGrant => ("Grant or remove a workspace role", P::WorkspaceRoles),
        K::ClientConnect => ("Connect a client", P::ConnectClient),
        K::CredentialEnrol => ("Enrol or remove one's own passkey", P::OwnPasskey),
        K::NotificationAddress => ("Add or remove one's own", P::NotificationAddress),
        K::BreakGlassApprove => ("Approve a break-glass", P::ApproveBreakGlass),
        K::LiftHold => ("Lift a hold a client set", P::LiftClientHold),
    }
}

/// Rows without **S** a context can also be authorized for; none is any kind's row.
const UNSTEPPED: [P; 4] = [P::Pause, P::ViewAgents, P::Skip, P::KillSwitchAgent];

/// The workspace roles' columns; org roles never act at a workspace's scope (§4.1).
const COLUMNS: [(Role, &str); 5] = [
    (Role::WorkspaceAdmin, "WA"),
    (Role::Operator, "Op"),
    (Role::Approver, "Ap"),
    (Role::Viewer, "Vi"),
    (Role::Auditor, "Au"),
];

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 123_456_789).unwrap()
}

fn now() -> UtcNanos {
    at(NOW_SECS)
}

/// The oracle: whether `kind`'s row is granted at a workspace's scope to a member holding
/// `effective` roles, through a full or a reduction-only session, with the store read or not.
/// Only pause and the kill switch's engage rows reach a reduction-only session, and an outage
/// refuses every **S** row but the owner exit (§4.5).
fn granted(kind: K, effective: &[Role], full: bool, outage: bool) -> bool {
    let section = SPEC.split("### 4.2 Permission matrix").nth(1).unwrap();
    let table = section.split("**Reading the matrix**").next().unwrap();
    let rows: Vec<Vec<&str>> = table
        .lines()
        .filter(|l| l.starts_with("| "))
        .map(|l| l.trim_matches('|').split('|').map(str::trim).collect())
        .collect();
    let start = row_of(kind).0;
    let found: Vec<_> = rows.iter().filter(|r| r[0].starts_with(start)).collect();
    assert_eq!(found.len(), 1, "one §4.2 row for {kind:?}");
    let cell = |name| found[0][rows[0].iter().position(|h| *h == name).unwrap()];
    let role_grants = COLUMNS.iter().any(|(role, name)| {
        effective.contains(role)
            && match cell(*name) {
                "✓" | "own" | "✓ (propose only)" | "✓ (not owner)" => true,
                "" | "✓ (org)" | "self" => false,
                other => panic!("§4.2 cell {other:?} is outside DEC-641's grammar"),
            }
    });
    let reduction = found[0][0] == "Pause" || found[0][0].ends_with(": engage");
    matches!(cell("S"), "S" | "S for invite" | "S for grant")
        && role_grants
        && (full || reduction)
        && (!outage || found[0][0] == "Owner exit (close a position)")
}

fn ws(workspace: WorkspaceId) -> Scope {
    Scope::Workspace { org: O1, workspace }
}

fn auth(
    lookup: &impl MembershipLookup,
    who: PrincipalId,
    session: &Session,
    workspace: WorkspaceId,
    permission: P,
) -> Result<Authorized, Refusal> {
    let user = Principal::User { id: who };
    authorize(lookup, &user, session, ws(workspace), permission, now())
}

/// The workspace context an authorization yields, an `own` row's through `into_tenant`.
fn tenant(authorized: Result<Authorized, Refusal>) -> Option<TenantContext> {
    match authorized {
        Ok(Authorized::Workspace { tenant, .. }) => Some(tenant),
        Ok(Authorized::Principal { context, .. }) => context.into_tenant().ok(),
        Ok(Authorized::Org { .. }) | Err(_) => None,
    }
}

fn action(kind: K) -> Action {
    let digest = Digest::of(kind.code().as_bytes());
    Action { kind, digest }
}

fn challenge_id() -> AssertionId {
    AssertionId("01J9ZQ4B7Y8K3M5N6P7Q8R9S0T".to_owned())
}

fn issue(context: &TenantContext, kind: K) -> Result<Issued, IssueRefusal> {
    issue_challenge(context, action(kind), challenge_id(), now())
}

fn refused(context: &TenantContext, kind: K) {
    let got = issue(context, kind).map(drop);
    assert_eq!(got, Err(IssueRefusal::Refused(Forbidden)), "{kind:?}");
}

/// The record binds the context's principal and workspace, the action, and `now`, expiring 300 s
/// later to the nanosecond (§7.2 step 1).
fn assert_bound(issued: &Issued, kind: K, unverified: bool) {
    let record = issued.record();
    assert_eq!((*record.principal_id(), *record.workspace_id()), (USER, W1));
    assert_eq!(record.action(), action(kind));
    assert_eq!(record.challenge_id(), &challenge_id());
    assert_eq!(record.issued_at(), now());
    assert_eq!(record.expires_at(), at(NOW_SECS + 300));
    assert_eq!(issued.membership_unverified(), unverified, "{kind:?}");
}

#[test]
fn a_principal_who_may_not_act_cannot_obtain_a_challenge() {
    let day_ago = at(NOW_SECS - 86_400);
    let operator = membership(USER, ws(W1), Active, &[(Role::Operator, day_ago)]);
    let viewer = membership(OTHER, ws(W1), Active, &[(Role::Viewer, day_ago)]);
    let elsewhere = membership(OTHER, ws(W2), Active, &[(Role::Operator, day_ago)]);
    let live = StaticLookup(vec![operator.clone(), viewer, elsewhere]);
    let full = session(SESSION, Full, vec![operator.clone()]);
    let ro = session(SESSION, SessionKind::ReductionOnly, vec![operator]);
    let refusal = |who, s, w, p| auth(&live, who, s, w, p).err();
    assert_eq!(refusal(STRANGER, &full, W1, P::Deploy), Some(NoMembership));
    assert_eq!(refusal(USER, &full, W2, P::Deploy), Some(NoMembership));
    assert_eq!(refusal(OTHER, &full, W1, P::Deploy), Some(Forbidden));
    let client = Principal::Client {
        id: STRANGER,
        on_behalf_of: USER,
        workspace: W1,
    };
    let by_client = authorize(&live, &client, &full, ws(W1), P::OwnerExit, now());
    assert_eq!(by_client.err(), Some(Forbidden));
    assert_eq!(refusal(USER, &ro, W1, P::OwnerExit), Some(ReductionOnly));
    let reduction_pause = tenant(auth(&live, USER, &ro, W1, P::Pause)).unwrap();
    refused(&reduction_pause, K::OwnerExit);
    let outage = FailingLookup(vec![(W1, O1)]);
    let during_outage = |p| auth(&outage, USER, &full, W1, p);
    assert_eq!(during_outage(P::Deploy).err(), Some(MembershipUnavailable));
    let unverified_pause = tenant(during_outage(P::Pause)).unwrap();
    assert!(unverified_pause.membership_unverified());
    for kind in [K::OwnerExit, K::Deploy, K::KillSwitchPrivilege] {
        refused(&unverified_pause, kind);
    }
    let deploy = tenant(auth(&live, USER, &full, W1, P::Deploy)).unwrap();
    refused(&deploy, K::Approve);
    assert_bound(&issue(&deploy, K::Deploy).unwrap(), K::Deploy, false);
}

#[test]
fn an_authorized_principal_obtains_a_challenge_bound_to_its_action() {
    let day_ago = at(NOW_SECS - 86_400);
    let roles = [Role::WorkspaceAdmin, Role::Operator, Role::Approver].map(|r| (r, day_ago));
    let bundle = membership(USER, ws(W1), Active, &roles);
    let live = StaticLookup(vec![bundle.clone()]);
    let full = session(SESSION, Full, vec![bundle]);
    for kind in K::ALL {
        let context = tenant(auth(&live, USER, &full, W1, row_of(kind).1)).unwrap();
        assert_bound(&issue(&context, kind).unwrap(), kind, false);
        let another_row = [K::Deploy, K::Approve][usize::from(kind == K::Deploy)];
        refused(&context, another_row);
    }
    let outage = FailingLookup(vec![(W1, O1)]);
    let owner_exit = tenant(auth(&outage, USER, &full, W1, P::OwnerExit)).unwrap();
    let issued = issue(&owner_exit, K::OwnerExit).unwrap();
    assert_bound(&issued, K::OwnerExit, true);
}

/// Identity spec §7.2 step 1: a service account and the host CLI are blank on every **S** row of
/// §4.2, so neither is authorized for any action kind's row and no context exists to obtain a
/// challenge with, even under an id that holds every role the user does; the user, with the same
/// roles, is authorized for each.
#[test]
fn a_service_account_or_the_host_cli_never_holds_a_context_for_a_challenge() {
    let day_ago = at(NOW_SECS - 86_400);
    let roles = [Role::WorkspaceAdmin, Role::Operator, Role::Approver].map(|r| (r, day_ago));
    let bundle = membership(USER, ws(W1), Active, &roles);
    let live = StaticLookup(vec![bundle.clone()]);
    let full = session(SESSION, Full, vec![bundle]);
    let service = Principal::ServiceAccount {
        id: USER,
        workspaces: [W1].into(),
    };
    let host_cli = Principal::HostCli {
        id: USER,
        workspace: W1,
        on_behalf_of: USER,
    };
    for kind in K::ALL {
        let row = row_of(kind).1;
        for machine in [&service, &host_cli] {
            let got = tenant(authorize(&live, machine, &full, ws(W1), row, now()));
            assert!(got.is_none(), "{machine:?} {kind:?}");
        }
        assert!(
            tenant(auth(&live, USER, &full, W1, row)).is_some(),
            "{kind:?}"
        );
    }
}

/// When a role becomes effective: drawn 1, a second ago; drawn 2, still cooling off.
fn effective_from(drawn: u8) -> UtcNanos {
    at(NOW_SECS - 3 + 2 * i64::from(drawn))
}

/// Each of the eight roles absent (0), effective (1), or still cooling off (2); the membership's
/// state; a full session (bit 0) and an outage (bit 1); the row authorized, half the time the
/// kind's own; and the kind asked for.
type Draw = (Vec<u8>, usize, u8, usize, usize);

fn case((roles, state, flags, asked, kind): Draw) -> Result<(), TestCaseError> {
    let (full, outage) = (flags & 1 == 1, flags & 2 == 2);
    let kind = K::ALL[kind];
    let rows = K::ALL.map(|k| row_of(k).1);
    let asked = [&rows[..], &UNSTEPPED].concat().get(asked).copied();
    let asked = asked.unwrap_or(row_of(kind).1);
    let drawn = Role::ALL.into_iter().zip(roles).filter(|r| r.1 > 0);
    let held: Vec<_> = drawn.clone().map(|(r, d)| (r, effective_from(d))).collect();
    let effective: Vec<Role> = drawn.filter(|r| r.1 == 1).map(|r| r.0).collect();
    let state = [Invited, CoolingOff, Active, Deactivated, Removed][state];
    let member = membership(USER, ws(W1), state, &held);
    let reaches = matches!(state, Active | CoolingOff);
    let snapshot = reaches.then(|| member.clone()).into_iter().collect();
    let session_kind = [SessionKind::ReductionOnly, Full][usize::from(full)];
    let s = session(SESSION, session_kind, snapshot);
    let authorized = match outage {
        true => auth(&FailingLookup(vec![(W1, O1)]), USER, &s, W1, asked),
        false => auth(&StaticLookup(vec![member]), USER, &s, W1, asked),
    };
    let expected = asked == row_of(kind).1 && reaches && granted(kind, &effective, full, outage);
    match tenant(authorized).map(|context| issue(&context, kind)) {
        Some(Ok(issued)) if expected => assert_bound(&issued, kind, outage),
        None | Some(Err(IssueRefusal::Refused(Forbidden))) if !expected => {}
        other => return Err(TestCaseError::fail(format!("{expected}: {other:?}"))),
    }
    Ok(())
}

#[test]
fn a_challenge_is_issued_exactly_when_the_matrix_grants_the_action() {
    let roles = prop::collection::vec(0u8..3, 8);
    let draw = (roles, 0..5usize, 0..4u8, 0..44usize, 0..18usize);
    let mut config = Config::with_cases(512);
    config.failure_persistence = None;
    if let Err(failure) = TestRunner::new(config).run(&draw, case) {
        panic!("{failure}");
    }
}
