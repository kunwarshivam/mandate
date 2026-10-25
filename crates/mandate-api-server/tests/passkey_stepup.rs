//! Passkey step-up at the port ([DEC-2126] item 2, DEC-2108 item 8): the binding dispatches each
//! method to its own verifier, and the passkey verifier judges an assertion in
//! `mandate_passkey::stepup::consume`'s order (identity spec §7.2 step 4), takes the challenge only
//! when it counts, and returns the `StepUpAsserted` to append (journal spec §9.32).
//!
//! Every context comes from the real `authorize` over the testkit's store, in the hosted workspace
//! or a neighbour. The control rows are written here record by record, the assertions are signed
//! here by a software authenticator, and every expected outcome is the code the test arranged, or
//! the record it computed by hand. Credentials are enrolled at `CredentialEnrolled` version 2, which
//! records the registration's backup-eligible flag; one at version 1 never counts (DEC-2122 items
//! 2 and 3).
//!
//! [DEC-2126]: ../../../docs/project/decisions/DEC-2126.md

mod common;
mod support_passkey;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::control::{Rows, T0};
use common::{ORG, WORKSPACE};
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence, StepUpMethod};
use mandate_api::problem::ProblemCode;
use mandate_api::wire::{EventId, Timestamp};
use mandate_api_server::challenges::Challenges;
use mandate_api_server::confirm::CliConfirm;
use mandate_api_server::methods::{Methods, Passkey};
use mandate_api_server::stepup::{Action, Asserted, Judged, StepUp, Unverified};
use mandate_api_server::subject::{LinkRefusal, NamesPrincipal};
use mandate_canon::Digest;
use mandate_identity::MembershipState::Active;
use mandate_identity::SessionKind::Full;
use mandate_identity::{
    AssertionId, Authorized, OrgId, Principal, PrincipalId, Role, Scope, SessionRef,
    StepUpActionKind, TenantContext, WorkspaceId, authorize,
};
use mandate_identity_testkit::{StaticLookup, membership, session};
use mandate_journal::StoredEvent;
use mandate_passkey::stepup::{self, ChallengeRecord, permission_for};
use serde_json::json;
use support_passkey::{CREDENTIAL, Signed, Signer, enrol, enrolled};

const OWNER: PrincipalId = PrincipalId(0x31);
const OTHER: PrincipalId = PrincipalId(0x32);
const SESSION: SessionRef = SessionRef(0x41);
const ISSUED: i64 = T0;
/// A challenge's life in seconds (identity spec §7.1).
const LIFE: i64 = 300;
const CHALLENGE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0T";
const UNKNOWN_CHALLENGE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0V";
const NEIGHBOUR: &str = "01JB3K6F0C9R2V7N4M8Q1T5W40";

fn at(secs: i64) -> mandate_time::UtcNanos {
    common::at(secs)
}

fn workspace(text: &str) -> WorkspaceId {
    WorkspaceId::from_ulid_text(text).expect("a ULID")
}

/// The context `authorize` yields to `who` for `kind`'s row in `place`: the owner and the other
/// principal are workspace admins and operators of the hosted workspace and its neighbour.
fn context(who: PrincipalId, kind: StepUpActionKind, place: WorkspaceId) -> TenantContext {
    let org = OrgId::from_ulid_text(ORG).expect("a ULID");
    let roles = [
        (Role::WorkspaceAdmin, at(ISSUED - 86_400)),
        (Role::Operator, at(ISSUED - 86_400)),
    ];
    let scope = |w: WorkspaceId| Scope::Workspace { org, workspace: w };
    let places = [workspace(WORKSPACE), workspace(NEIGHBOUR)];
    let member = |who, w| membership(who, scope(w), Active, &roles);
    let all = [OWNER, OTHER]
        .iter()
        .flat_map(|w| places.iter().map(move |p| member(*w, *p)))
        .collect();
    let theirs = places.iter().map(|p| member(who, *p)).collect();
    match authorize(
        &StaticLookup(all),
        &Principal::User { id: who },
        &session(SESSION, Full, theirs),
        scope(place),
        permission_for(kind),
        at(ISSUED),
    ) {
        Ok(Authorized::Workspace { tenant, .. }) => tenant,
        other => panic!("the fixture authorizes {kind:?}'s row in {place:?}: {other:?}"),
    }
}

/// Subject links that name nobody: no `cli_confirm` evidence counts here.
struct Unlinked;

impl NamesPrincipal for Unlinked {
    fn names(&self, _principal: PrincipalId) -> Result<bool, LinkRefusal> {
        Ok(false)
    }
}

fn connection(content: &[u8]) -> stepup::Action {
    stepup::Action {
        kind: StepUpActionKind::Connection,
        digest: Digest::of(content),
    }
}

/// The owner's passkey evidence for the challenge `id`.
fn evidence(id: &str, method: StepUpMethod) -> StepUpEvidence {
    StepUpEvidence {
        assertion_id: EventId::try_from(id.to_owned()).expect("a ULID"),
        authenticated_at: Timestamp::try_from(at(ISSUED).to_string()).expect("a timestamp"),
        method,
    }
}

/// The hosted workspace with the owner's passkey enrolled as [`CREDENTIAL`] at counter 0, past
/// its cool-off, a held connect challenge issued to the owner at [`ISSUED`], and the binding.
struct World {
    signer: Signer,
    rows: Rows,
    challenges: Arc<Challenges>,
    record: ChallengeRecord,
    action: stepup::Action,
    port: Methods,
}

fn world_with(signer: Signer) -> World {
    let mut rows = Rows::opened();
    enrol(
        &mut rows,
        T0 - 900,
        enrolled(OWNER, CREDENTIAL, &signer, 0, Some(T0 - 800)),
    );
    let challenges = Arc::new(Challenges::default());
    let action = connection(b"the connect's action object");
    let record = challenges
        .issue(
            &context(OWNER, action.kind, workspace(WORKSPACE)),
            action,
            AssertionId(CHALLENGE.to_owned()),
            at(ISSUED),
        )
        .expect("the owner obtains a connect challenge");
    let key = signer.cose_key();
    let keys: BTreeMap<Digest, Vec<u8>> = BTreeMap::from([(Digest::of(&key), key)]);
    let port = Methods {
        passkey: Passkey {
            challenges: Arc::clone(&challenges),
            rp: support_passkey::rp(),
            keys: Some(Arc::new(keys)),
        },
        cli_confirm: CliConfirm {
            challenges: Arc::clone(&challenges),
            links: Arc::new(Unlinked),
        },
    };
    World {
        signer,
        rows,
        challenges,
        record,
        action,
        port,
    }
}

fn world() -> World {
    world_with(Signer::es256())
}

/// One request: who asks, in which workspace, for what, with which evidence and assertion, at
/// which second, over which control rows.
#[derive(Clone)]
struct Ask {
    who: PrincipalId,
    place: WorkspaceId,
    action: stepup::Action,
    evidence: StepUpEvidence,
    assertion: Option<StepUpAssertion>,
    now: i64,
}

impl World {
    /// The owner's authenticator's assertion over the held challenge at `counter`.
    fn signed(&self, counter: u32) -> Signed {
        let challenge = self
            .record
            .webauthn_challenge()
            .expect("a canonical record");
        self.signer.assert(&unb64(&challenge.base64url()), counter)
    }

    /// The owner's connect in the hosted workspace, 30 s after the challenge, signed at counter 9.
    fn ask(&self) -> Ask {
        Ask {
            who: OWNER,
            place: workspace(WORKSPACE),
            action: self.action,
            evidence: evidence(CHALLENGE, StepUpMethod::Passkey),
            assertion: Some(self.signed(9).wire(CREDENTIAL)),
            now: ISSUED + 30,
        }
    }

    fn judge_over(
        &self,
        ask: &Ask,
        control: &[StoredEvent],
    ) -> Result<Option<Asserted>, Unverified> {
        let tenant = context(ask.who, ask.action.kind, ask.place);
        let judged = Judged {
            tenant: &tenant,
            control,
            now: at(ask.now),
        };
        let action = Action {
            kind: ask.action.kind,
            digest: ask.action.digest,
        };
        self.port
            .asserted(&ask.evidence, ask.assertion.as_ref(), &action, &judged)
    }

    fn judge(&self, ask: &Ask) -> Result<Option<Asserted>, Unverified> {
        self.judge_over(ask, &self.rows.rows)
    }

    /// Whether the challenge is still held.
    fn held(&self) -> bool {
        self.challenges.held(CHALLENGE).is_some()
    }
}

fn unb64(text: &str) -> Vec<u8> {
    support_passkey::unb64url(text)
}

/// `code`, and the challenge still held: a refusal takes nothing.
fn refused(w: &World, ask: &Ask, code: ProblemCode) {
    assert_eq!(w.judge(ask), Err(Unverified::Code(code)));
    assert!(w.held(), "a refused step-up leaves the challenge held");
}

#[test]
#[ignore = "pending E9-4"]
fn an_unknown_challenge_is_missing() {
    let w = world();
    let mut ask = w.ask();
    ask.evidence = evidence(UNKNOWN_CHALLENGE, StepUpMethod::Passkey);
    refused(&w, &ask, ProblemCode::StepUpMissing);
}

#[test]
#[ignore = "pending E9-4"]
fn a_challenge_outside_its_window_is_stale() {
    let w = world();
    for now in [ISSUED - 1, ISSUED + LIFE, ISSUED + LIFE + 1] {
        let mut ask = w.ask();
        ask.now = now;
        refused(&w, &ask, ProblemCode::StepUpStale);
    }
    let mut last = w.ask();
    last.now = ISSUED + LIFE - 1;
    assert!(
        matches!(w.judge(&last), Ok(Some(_))),
        "the last second counts"
    );
}

/// An earlier event on the stream whose step-up names `challenge`.
fn spent(rows: &mut Rows, challenge: &str) {
    rows.hosted(
        "ConnectionRequested",
        T0 - 10,
        json!({
            "connection_id": "conn_01JAA3P4A000000000000000C1", "account_ref": "01JAA3P4A000000000000000A1",
            "step_up": {"assertion_id": challenge, "authenticated_at": at(ISSUED).to_string(), "method": "passkey"},
        }),
    );
}

#[test]
#[ignore = "pending E9-4"]
fn evidence_authenticated_at_another_instant_than_the_challenge_s_is_stale() {
    let w = world();
    for secs in [ISSUED - 1, ISSUED + 1, ISSUED + 30] {
        let mut ask = w.ask();
        ask.evidence.authenticated_at =
            Timestamp::try_from(at(secs).to_string()).expect("a timestamp");
        refused(&w, &ask, ProblemCode::StepUpStale);
    }
}

#[test]
#[ignore = "pending E9-4"]
fn a_challenge_an_event_already_spent_is_reused() {
    let mut w = world();
    spent(&mut w.rows, CHALLENGE);
    refused(&w, &w.ask(), ProblemCode::StepUpReused);
}

#[test]
#[ignore = "pending E9-4"]
fn cli_confirm_evidence_for_a_kind_it_does_not_serve_is_the_method_refusal() {
    let w = world();
    let mut ask = w.ask();
    ask.action.kind = StepUpActionKind::Deploy;
    ask.evidence = evidence(CHALLENGE, StepUpMethod::CliConfirm);
    ask.assertion = None;
    refused(&w, &ask, ProblemCode::StepUpMethod);
}

#[test]
#[ignore = "pending E9-4"]
fn a_challenge_bound_to_another_principal_workspace_or_digest_is_mismatch() {
    let w = world();
    let mut other_principal = w.ask();
    other_principal.who = OTHER;
    refused(&w, &other_principal, ProblemCode::StepUpMismatch);

    let mut other_digest = w.ask();
    other_digest.action = connection(b"another connect's action object");
    refused(&w, &other_digest, ProblemCode::StepUpMismatch);

    let mut neighbour = Rows { rows: Vec::new() };
    neighbour.push(
        &format!("ctl:{NEIGHBOUR}"),
        "paper",
        "StreamOpened",
        T0 - 1_000,
        json!({"stream_type": "control"}),
    );
    let mut other_workspace = w.ask();
    other_workspace.place = workspace(NEIGHBOUR);
    assert_eq!(
        w.judge_over(&other_workspace, &neighbour.rows),
        Err(Unverified::Code(ProblemCode::StepUpMismatch))
    );
    assert!(w.held());
}

#[test]
#[ignore = "pending E9-4"]
fn the_first_failing_check_answers_in_consume_s_order() {
    let w = world();
    let mut unknown_and_stale = w.ask();
    unknown_and_stale.evidence = evidence(UNKNOWN_CHALLENGE, StepUpMethod::Passkey);
    unknown_and_stale.now = ISSUED + LIFE;
    refused(&w, &unknown_and_stale, ProblemCode::StepUpMissing);

    let mut stale_and_mismatched = w.ask();
    stale_and_mismatched.now = ISSUED + LIFE;
    stale_and_mismatched.who = OTHER;
    refused(&w, &stale_and_mismatched, ProblemCode::StepUpStale);

    let mut mismatched_and_unsigned = w.ask();
    mismatched_and_unsigned.who = OTHER;
    mismatched_and_unsigned.assertion = Some(
        Signer::es256()
            .assert(b"0123456789abcdef", 9)
            .wire(CREDENTIAL),
    );
    refused(&w, &mismatched_and_unsigned, ProblemCode::StepUpMismatch);

    let mut unsigned_and_other_digest = w.ask();
    unsigned_and_other_digest.action = connection(b"another connect's action object");
    unsigned_and_other_digest.assertion = Some(
        Signer::es256()
            .assert(b"0123456789abcdef", 9)
            .wire(CREDENTIAL),
    );
    refused(&w, &unsigned_and_other_digest, ProblemCode::StepUpMissing);

    let mut reused_and_stale = Rows {
        rows: w.rows.rows.clone(),
    };
    spent(&mut reused_and_stale, CHALLENGE);
    let mut late = w.ask();
    late.now = ISSUED + LIFE;
    assert_eq!(
        w.judge_over(&late, &reused_and_stale.rows),
        Err(Unverified::Code(ProblemCode::StepUpReused))
    );
}

#[test]
#[ignore = "pending E9-4"]
fn passkey_evidence_without_an_assertion_is_missing_and_cli_confirm_with_one_is_too() {
    let w = world();
    let mut bare = w.ask();
    bare.assertion = None;
    refused(&w, &bare, ProblemCode::StepUpMissing);

    let mut confirmed_and_signed = w.ask();
    confirmed_and_signed.evidence = evidence(CHALLENGE, StepUpMethod::CliConfirm);
    refused(&w, &confirmed_and_signed, ProblemCode::StepUpMissing);
}

#[test]
fn passkey_evidence_alone_is_missing_at_verify() {
    let w = world();
    let tenant = context(OWNER, w.action.kind, workspace(WORKSPACE));
    let judged = Judged {
        tenant: &tenant,
        control: &w.rows.rows,
        now: at(ISSUED + 30),
    };
    let action = Action {
        kind: w.action.kind,
        digest: w.action.digest,
    };
    let bare = evidence(CHALLENGE, StepUpMethod::Passkey);
    assert_eq!(
        w.port.verify(&bare, &action, &judged),
        Err(ProblemCode::StepUpMissing)
    );
    assert_eq!(
        w.port.passkey.verify(&bare, &action, &judged),
        Err(ProblemCode::StepUpMissing)
    );
    assert!(w.held());
}
