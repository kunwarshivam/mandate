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
use mandate_canon::{Digest, encode_ulid};
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
use support_passkey::{
    CREDENTIAL, OTHER_CREDENTIAL, Signed, Signer, assert_by, asserted, backup_eligible, enrol,
    enrol_at, enrolled, removed, version_1,
};

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
        (Role::Approver, at(ISSUED - 86_400)),
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

/// Subject links that name the owner alone, so the owner's `cli_confirm` can count.
struct OwnerLinked;

impl NamesPrincipal for OwnerLinked {
    fn names(&self, principal: PrincipalId) -> Result<bool, LinkRefusal> {
        Ok(principal == OWNER)
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
            links: Arc::new(OwnerLinked),
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

/// `ask` counts: the port returns a record to append, and shows what it answered otherwise.
fn counts(w: &World, ask: &Ask) {
    let answer = w.judge(ask);
    assert!(
        matches!(answer, Ok(Some(_))),
        "the step-up counts, not {answer:?}"
    );
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
    counts(&w, &last);
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

/// The test's own Crockford base-32 of the first 40 bits of `bytes` (DEC-2104 item 1).
fn crockford40(bytes: &[u8]) -> String {
    const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
    let bits = bytes
        .iter()
        .take(5)
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    (0..8)
        .rev()
        .map(|i| char::from(CROCKFORD[usize::try_from((bits >> (i * 5)) & 31).expect("5 bits")]))
        .collect()
}

/// The owner's `StepUpConfirmed` of the held challenge on the host (journal spec §9.29), 20 s
/// after it was issued, with the code the web app shows for it.
fn confirm_on_the_host(w: &mut World) {
    let challenge = w.record.webauthn_challenge().expect("a canonical record");
    w.rows.hosted(
        "StepUpConfirmed",
        ISSUED + 20,
        json!({
            "challenge": CHALLENGE, "action_kind": "connection",
            "action_digest": format!("sha256:{}", w.action.digest.to_hex()),
            "principal": encode_ulid(OWNER.0), "code": crockford40(&unb64(&challenge.base64url())),
            "confirmed_at": ISSUED + 20,
        }),
    );
}

#[test]
#[ignore = "pending E9-4"]
fn a_credential_not_the_principal_s_held_and_past_cool_off_is_missing() {
    let unknown = world();
    let mut ask = unknown.ask();
    ask.assertion = Some(unknown.signed(9).wire(OTHER_CREDENTIAL));
    refused(&unknown, &ask, ProblemCode::StepUpMissing);

    let mut cooling = world();
    cooling.rows = Rows::opened();
    enrol(
        &mut cooling.rows,
        T0 - 900,
        enrolled(OWNER, CREDENTIAL, &cooling.signer, 0, Some(ISSUED + 31)),
    );
    refused(&cooling, &cooling.ask(), ProblemCode::StepUpMissing);

    let mut gone = world();
    gone.rows
        .hosted("CredentialRemoved", T0 - 100, removed(OWNER, CREDENTIAL));
    refused(&gone, &gone.ask(), ProblemCode::StepUpMissing);

    let mut theirs = world();
    theirs.rows = Rows::opened();
    enrol(
        &mut theirs.rows,
        T0 - 900,
        enrolled(OTHER, CREDENTIAL, &theirs.signer, 0, None),
    );
    refused(&theirs, &theirs.ask(), ProblemCode::StepUpMissing);
}

#[test]
#[ignore = "pending E9-4"]
fn a_counter_not_above_the_last_is_missing() {
    let mut enrolled_at_nine = world();
    enrolled_at_nine.rows = Rows::opened();
    enrol(
        &mut enrolled_at_nine.rows,
        T0 - 900,
        enrolled(OWNER, CREDENTIAL, &enrolled_at_nine.signer, 9, None),
    );
    refused(
        &enrolled_at_nine,
        &enrolled_at_nine.ask(),
        ProblemCode::StepUpMissing,
    );

    let mut asserted_at_twelve = world();
    assert_by(
        &mut asserted_at_twelve.rows,
        OWNER,
        T0 - 50,
        asserted(UNKNOWN_CHALLENGE, CREDENTIAL, 12),
    );
    let mut eleven = asserted_at_twelve.ask();
    eleven.assertion = Some(asserted_at_twelve.signed(11).wire(CREDENTIAL));
    refused(&asserted_at_twelve, &eleven, ProblemCode::StepUpMissing);
    let mut twelve = asserted_at_twelve.ask();
    twelve.assertion = Some(asserted_at_twelve.signed(12).wire(CREDENTIAL));
    refused(&asserted_at_twelve, &twelve, ProblemCode::StepUpMissing);
    let mut thirteen = asserted_at_twelve.ask();
    thirteen.assertion = Some(asserted_at_twelve.signed(13).wire(CREDENTIAL));
    counts(&asserted_at_twelve, &thirteen);
}

#[test]
#[ignore = "pending E9-4"]
fn a_counter_the_authenticator_does_not_keep_counts_while_both_are_zero() {
    let enrolled_at_zero = world();
    let mut zero = enrolled_at_zero.ask();
    zero.assertion = Some(enrolled_at_zero.signed(0).wire(CREDENTIAL));
    counts(&enrolled_at_zero, &zero);

    let mut asserted_at_zero = world();
    assert_by(
        &mut asserted_at_zero.rows,
        OWNER,
        T0 - 50,
        asserted(UNKNOWN_CHALLENGE, CREDENTIAL, 0),
    );
    let mut again = asserted_at_zero.ask();
    again.assertion = Some(asserted_at_zero.signed(0).wire(CREDENTIAL));
    counts(&asserted_at_zero, &again);
}

#[test]
#[ignore = "pending E9-4"]
fn a_credential_stream_the_fold_cannot_read_is_journal_unavailable() {
    let mut w = world();
    w.rows.hosted(
        "CredentialEnrolled",
        T0 - 500,
        json!({"member": encode_ulid(OTHER.0), "credential": OTHER_CREDENTIAL, "alg": "rs256"}),
    );
    refused(&w, &w.ask(), ProblemCode::JournalUnavailable);
}

/// The owner's credential enrolled at counter 5, then `asserted` written by the owner.
fn broken_by(asserted_records: &[(&str, u32)]) -> World {
    let mut w = world();
    w.rows = Rows::opened();
    enrol(
        &mut w.rows,
        T0 - 900,
        enrolled(OWNER, CREDENTIAL, &w.signer, 5, None),
    );
    for (seq, (challenge, counter)) in asserted_records.iter().enumerate() {
        let secs = T0 - 100 + i64::try_from(seq).expect("small");
        assert_by(
            &mut w.rows,
            OWNER,
            secs,
            asserted(challenge, CREDENTIAL, *counter),
        );
    }
    w
}

#[test]
#[ignore = "pending E9-4"]
fn a_holder_whose_assertions_break_rule_254_has_no_passkey_that_counts() {
    let below_the_last = broken_by(&[(UNKNOWN_CHALLENGE, 3)]);
    refused(
        &below_the_last,
        &below_the_last.ask(),
        ProblemCode::StepUpMissing,
    );
    let one_challenge_twice = broken_by(&[(UNKNOWN_CHALLENGE, 6), (UNKNOWN_CHALLENGE, 7)]);
    refused(
        &one_challenge_twice,
        &one_challenge_twice.ask(),
        ProblemCode::StepUpMissing,
    );
    let kept = broken_by(&[(UNKNOWN_CHALLENGE, 6)]);
    counts(&kept, &kept.ask());
}

#[test]
#[ignore = "pending E9-4"]
fn a_member_whose_records_break_rule_251_has_no_passkey_that_counts() {
    let mut w = world();
    enrol(
        &mut w.rows,
        T0 - 500,
        enrolled(OWNER, CREDENTIAL, &w.signer, 0, None),
    );
    refused(&w, &w.ask(), ProblemCode::StepUpMissing);
}

#[test]
#[ignore = "pending E9-4"]
fn a_tampered_assertion_is_missing() {
    let w = world();
    let signed = w.signed(9);
    let mut flipped = signed.clone();
    let last = flipped.signature.len() - 1;
    flipped.signature[last] ^= 1;
    let mut ask = w.ask();
    ask.assertion = Some(flipped.wire(CREDENTIAL));
    refused(&w, &ask, ProblemCode::StepUpMissing);

    let mut unverified = signed;
    unverified.authenticator_data[32] = 0x01;
    let mut ask = w.ask();
    ask.assertion = Some(unverified.wire(CREDENTIAL));
    refused(&w, &ask, ProblemCode::StepUpMissing);
}

#[test]
#[ignore = "pending E9-4"]
fn passkey_evidence_without_an_assertion_is_missing_and_cli_confirm_with_one_is_too() {
    let mut w = world();
    let mut bare = w.ask();
    bare.assertion = None;
    refused(&w, &bare, ProblemCode::StepUpMissing);

    confirm_on_the_host(&mut w);
    let mut confirmed_and_signed = w.ask();
    confirmed_and_signed.evidence = evidence(CHALLENGE, StepUpMethod::CliConfirm);
    refused(&w, &confirmed_and_signed, ProblemCode::StepUpMissing);
    let mut confirmed = confirmed_and_signed;
    confirmed.assertion = None;
    assert_eq!(
        w.judge(&confirmed),
        Ok(None),
        "the same confirmation counts without the assertion, which alone refused it"
    );
}

#[test]
#[ignore = "pending E9-4"]
fn a_challenge_issued_for_another_kind_with_the_same_digest_is_mismatch() {
    let w = world();
    let approval = stepup::Action {
        kind: StepUpActionKind::Approve,
        digest: w.action.digest,
    };
    let record = w
        .challenges
        .issue(
            &context(OWNER, approval.kind, workspace(WORKSPACE)),
            approval,
            AssertionId(UNKNOWN_CHALLENGE.to_owned()),
            at(ISSUED),
        )
        .expect("the owner obtains an approval's challenge");
    let challenge = record.webauthn_challenge().expect("a canonical record");
    let mut ask = w.ask();
    ask.evidence = evidence(UNKNOWN_CHALLENGE, StepUpMethod::Passkey);
    ask.assertion = Some(
        w.signer
            .assert(&unb64(&challenge.base64url()), 9)
            .wire(CREDENTIAL),
    );
    assert_eq!(
        w.judge(&ask),
        Err(Unverified::Code(ProblemCode::StepUpMismatch))
    );
    assert!(w.challenges.held(UNKNOWN_CHALLENGE).is_some());
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

#[test]
#[ignore = "pending E9-4"]
fn a_credential_enrolled_at_version_1_never_counts() {
    let mut w = world();
    w.rows = Rows::opened();
    let payload = version_1(enrolled(OWNER, CREDENTIAL, &w.signer, 0, None));
    enrol_at(&mut w.rows, 1, T0 - 900, payload);
    refused(&w, &w.ask(), ProblemCode::StepUpMissing);
}

#[test]
#[ignore = "pending E9-4"]
fn a_backup_eligible_flag_other_than_the_registration_s_is_missing() {
    let mut eligible = world();
    eligible.rows = Rows::opened();
    let payload = backup_eligible(enrolled(OWNER, CREDENTIAL, &eligible.signer, 0, None));
    enrol(&mut eligible.rows, T0 - 900, payload);
    refused(&eligible, &eligible.ask(), ProblemCode::StepUpMissing);

    let not_eligible = world();
    let challenge = not_eligible
        .record
        .webauthn_challenge()
        .expect("a canonical record");
    let mut ask = not_eligible.ask();
    let signed = not_eligible
        .signer
        .assert_backup_eligible(&unb64(&challenge.base64url()), 9);
    ask.assertion = Some(signed.wire(CREDENTIAL));
    refused(&not_eligible, &ask, ProblemCode::StepUpMissing);
}
