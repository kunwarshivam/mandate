//! A step-up presentation names its workspace and principal only through the request's context
//! (identity spec ID-8, §7.2 step 4, DEC-649): a challenge issued under one `TenantContext` counts
//! only when presented under a context for the same workspace and principal. Every context here
//! comes from the real `authorize` over the testkit's store, and the oracle compares the indices
//! the test drew, never the crate's IDs or accessors.

use mandate_canon::Digest;
use mandate_identity::MembershipState::Active;
use mandate_identity::SessionKind::Full;
use mandate_identity::{
    AssertionId, Authorized, OrgId, Permission, Principal, PrincipalId, Role, Scope, SessionRef,
    StepUpActionKind, TenantContext, WorkspaceId, authorize,
};
use mandate_identity_testkit::{StaticLookup, membership, session};
use mandate_passkey::RelyingParty;
use mandate_passkey::stepup::{
    Action, ChallengeState, Environment, Presentation, Proof, StepUpRefusal, consume,
    issue_challenge,
};
use mandate_time::UtcNanos;
use proptest::test_runner::{Config, TestCaseError, TestRunner};

const O1: OrgId = OrgId(0x11);
const WORKSPACES: [WorkspaceId; 3] = [WorkspaceId(0x21), WorkspaceId(0x22), WorkspaceId(0x23)];
const PRINCIPALS: [PrincipalId; 2] = [PrincipalId(0x31), PrincipalId(0x32)];
const SESSION: SessionRef = SessionRef(0x41);
const ISSUED_SECS: i64 = 1_790_000_000;
const CHALLENGE_ID: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0T";

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 123_456_789).unwrap()
}

fn ws(workspace: WorkspaceId) -> Scope {
    Scope::Workspace { org: O1, workspace }
}

fn approval() -> Action {
    Action {
        kind: StepUpActionKind::Approve,
        digest: Digest::of(b"the approval's content object"),
    }
}

/// The context `authorize` yields for the approve row to principal `p` in workspace `w`, both
/// indices into the fixed lists; every principal is an approver in every workspace, through a
/// full session that holds all of its memberships.
fn context(w: usize, p: usize) -> TenantContext {
    let day_ago = at(ISSUED_SECS - 86_400);
    let member =
        |who, workspace| membership(who, ws(workspace), Active, &[(Role::Approver, day_ago)]);
    let all: Vec<_> = PRINCIPALS
        .iter()
        .flat_map(|who| WORKSPACES.iter().map(|w| member(*who, *w)))
        .collect();
    let theirs = WORKSPACES
        .iter()
        .map(|w| member(PRINCIPALS[p], *w))
        .collect();
    let user = Principal::User { id: PRINCIPALS[p] };
    let s = session(SESSION, Full, theirs);
    let authorized = authorize(
        &StaticLookup(all),
        &user,
        &s,
        ws(WORKSPACES[w]),
        Permission::Approve,
        at(ISSUED_SECS),
    );
    match authorized {
        Ok(Authorized::Workspace { tenant, .. }) => tenant,
        other => panic!("the fixture authorizes the approve row: {other:?}"),
    }
}

/// Issues an approval's challenge under `issuer` and presents it 30 s later under `presenter`
/// with the CLI's confirmation in paper, the method that needs no assertion, so the binding is
/// the only check that can refuse it. `Ok` carries the evidence's assertion ID.
fn issue_then_present(
    issuer: &TenantContext,
    presenter: &TenantContext,
) -> Result<AssertionId, StepUpRefusal> {
    let issued = issue_challenge(
        issuer,
        approval(),
        AssertionId(CHALLENGE_ID.to_owned()),
        at(ISSUED_SECS),
    )
    .expect("the approve row's context obtains an approval's challenge");
    let rp = RelyingParty {
        rp_id: "mandate.example".to_owned(),
        origin: "https://mandate.example".to_owned(),
    };
    let presented = Presentation::new(presenter, approval(), Proof::CliConfirm);
    let state = ChallengeState::Issued(issued.record());
    consume(
        &rp,
        Environment::Paper,
        state,
        &presented,
        at(ISSUED_SECS + 30),
    )
    .map(|consumed| consumed.evidence().assertion_id.clone())
}

/// A challenge workspace A's context obtained, presented with workspace B's context by the same
/// principal, is a mismatch; the same challenge presented with A's context counts.
#[test]
fn a_step_up_challenge_from_another_workspace_never_counts() {
    let (in_a, in_b) = (context(0, 0), context(1, 0));
    assert_eq!(
        issue_then_present(&in_a, &in_b),
        Err(StepUpRefusal::Mismatch)
    );
    assert_eq!(
        issue_then_present(&in_a, &in_a),
        Ok(AssertionId(CHALLENGE_ID.to_owned()))
    );
}

/// The issuing and the presenting (workspace, principal), each an index pair.
type Draw = ((usize, usize), (usize, usize));

fn case(((iw, ip), (pw, pp)): Draw) -> Result<(), TestCaseError> {
    let got = issue_then_present(&context(iw, ip), &context(pw, pp));
    let expected = match (iw, ip) == (pw, pp) {
        true => Ok(AssertionId(CHALLENGE_ID.to_owned())),
        false => Err(StepUpRefusal::Mismatch),
    };
    if got != expected {
        let drawn = ((iw, ip), (pw, pp));
        return Err(TestCaseError::fail(format!(
            "{drawn:?}: got {got:?}, expected {expected:?}"
        )));
    }
    Ok(())
}

/// Over every pair of contexts, a challenge counts exactly when it is presented under a context
/// for the workspace and the principal it was issued to.
#[test]
fn a_step_up_counts_only_under_the_context_its_challenge_was_issued_to() {
    let pair = (0..WORKSPACES.len(), 0..PRINCIPALS.len());
    let mut config = Config::with_cases(64);
    config.failure_persistence = None;
    if let Err(failure) = TestRunner::new(config).run(&(pair.clone(), pair), case) {
        panic!("{failure}");
    }
}
