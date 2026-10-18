//! `cli_confirm` step-up for a paper grant made in the web app (DEC-2104, DEC-2103 item 7, decided
//! by the founder in DEC-2605 row 5): the code the web app shows, and the port's judgment of a
//! grant's evidence against the challenge this server holds and the owner's `StepUpConfirmed`
//! (journal spec §9.29), one test per check of DEC-2104 item 6, in its order.
//!
//! Every context comes from the real `authorize` over the testkit's store. The oracles are the
//! test's own: the code is encoded here from the challenge's bytes, decoded here from its
//! base64url; the expected refusal is the one the test arranged; and the property test asks only
//! that a confirmation differing from the challenge in one bound member never verifies.

use std::sync::Arc;

use mandate_api::envelope::{StepUpEvidence, StepUpMethod};
use mandate_api::problem::ProblemCode;
use mandate_api::wire::{EventId, Timestamp};
use mandate_api_server::challenges::Challenges;
use mandate_api_server::confirm::{CODE_LEN, CliConfirm, code};
use mandate_api_server::stepup::{Action, Judged, StepUp};
use mandate_api_server::subject::{Fixed, LinkRefusal, NamesPrincipal, SUBJECT_LINKS, names_at};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_identity::MembershipState::Active;
use mandate_identity::SessionKind::Full;
use mandate_identity::{
    AssertionId, Authorized, OrgId, Permission, Principal, PrincipalId, Role, Scope, SessionRef,
    StepUpActionKind, TenantContext, WorkspaceId, authorize,
};
use mandate_identity_testkit::{StaticLookup, membership, session};
use mandate_journal::StoredEvent;
use mandate_passkey::stepup::{self, ChallengeRecord, permission_for};
use mandate_time::UtcNanos;
use proptest::prelude::{any, prop_assume};
use proptest::test_runner::{Config, TestCaseError, TestRunner};

const ORG: OrgId = OrgId(0x11);
const WORKSPACE: WorkspaceId = WorkspaceId(0x21);
const OWNER: PrincipalId = PrincipalId(0x31);
const OTHER: PrincipalId = PrincipalId(0x32);
const SESSION: SessionRef = SessionRef(0x41);
const ISSUED: i64 = 1_790_000_000;
const CHALLENGE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0T";
const ANOTHER_CHALLENGE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0V";
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).expect("a time")
}

fn ulid(id: PrincipalId) -> String {
    id.to_ulid_text().expect("a principal's ULID text")
}

fn approval(content: &[u8]) -> stepup::Action {
    stepup::Action {
        kind: StepUpActionKind::Approve,
        digest: Digest::of(content),
    }
}

fn asked(action: stepup::Action) -> Action {
    Action {
        kind: action.kind,
        digest: action.digest,
    }
}

/// The context `authorize` yields to `who` for `kind`'s row in the workspace: every principal is
/// an operator and an approver there, through a full session holding its membership.
fn context(who: PrincipalId, kind: StepUpActionKind) -> TenantContext {
    let scope = Scope::Workspace {
        org: ORG,
        workspace: WORKSPACE,
    };
    let roles = [
        (Role::Approver, at(ISSUED - 86_400)),
        (Role::Operator, at(ISSUED - 86_400)),
    ];
    let member = |p| membership(p, scope, Active, &roles);
    let lookup = StaticLookup(vec![member(OWNER), member(OTHER)]);
    let held = session(SESSION, Full, vec![member(who)]);
    let permission: Permission = permission_for(kind);
    match authorize(
        &lookup,
        &Principal::User { id: who },
        &held,
        scope,
        permission,
        at(ISSUED),
    ) {
        Ok(Authorized::Workspace { tenant, .. }) => tenant,
        other => panic!("the fixture authorizes {kind:?}'s row: {other:?}"),
    }
}

/// Subject links that name exactly the principals given.
struct Linked(Vec<PrincipalId>);

impl NamesPrincipal for Linked {
    fn names(&self, principal: PrincipalId) -> Result<bool, LinkRefusal> {
        Ok(self.0.contains(&principal))
    }
}

/// The test's own Crockford base-32 of the first 40 bits of `bytes` (DEC-2104 item 1).
fn crockford40(bytes: &[u8]) -> String {
    let bits = bytes
        .iter()
        .take(5)
        .fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    (0..8)
        .rev()
        .map(|i| char::from(CROCKFORD[usize::try_from((bits >> (i * 5)) & 31).expect("5 bits")]))
        .collect()
}

/// The test's own decoding of unpadded base64url.
fn unbase64url(text: &str) -> Vec<u8> {
    let sextets: Vec<u32> = text
        .bytes()
        .map(|c| {
            let at = BASE64URL.iter().position(|b| *b == c).expect("base64url");
            u32::try_from(at).expect("6 bits")
        })
        .collect();
    sextets
        .chunks(4)
        .flat_map(|chunk| {
            let n = chunk.iter().fold(0u32, |acc, s| (acc << 6) | s) << (6 * (4 - chunk.len()));
            let bytes = n.to_be_bytes();
            bytes[1..chunk.len()].to_vec()
        })
        .collect()
}

/// The code the web app shows for `record`, computed here from its base64url challenge.
fn shown(record: &ChallengeRecord) -> String {
    let challenge = record.webauthn_challenge().expect("a canonical record");
    crockford40(&unbase64url(&challenge.base64url()))
}

/// One control-stream row of `event_type` at `seq`, its body the canonical form of `payload`
/// written by `actor`.
fn row(
    seq: u64,
    event_id: &str,
    event_type: &str,
    actor: PrincipalId,
    payload: &str,
) -> StoredEvent {
    let text = format!(
        r#"{{"actor":{{"build":null,"id":"{}","kind":"user","version":"1"}},"causation_id":null,"environment":"paper","event_id":"{event_id}","event_type":"{event_type}","payload":{payload},"schema_version":1,"stream_id":"ctl:ws"}}"#,
        ulid(actor)
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: "ctl:ws".to_owned(),
        seq,
        event_id: event_id.to_owned(),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: "2026-10-18T22:30:00.000000000Z".to_owned(),
        prev_hash: Digest::ZERO,
        hash: Digest::of(&body),
        body,
    }
}

/// What a `StepUpConfirmed` names (journal spec §9.29).
#[derive(Debug, Clone)]
struct Confirmation {
    challenge: String,
    kind: &'static str,
    digest: Digest,
    principal: PrincipalId,
    code: String,
    confirmed_at: i64,
}

impl Confirmation {
    /// The owner's confirmation of `record`, 20 s after it was issued, with the code shown.
    fn of(record: &ChallengeRecord) -> Self {
        Self {
            challenge: record.challenge_id().0.clone(),
            kind: "approve",
            digest: record.action().digest,
            principal: *record.principal_id(),
            code: shown(record),
            confirmed_at: ISSUED + 20,
        }
    }

    fn row(&self, seq: u64, event_id: &str) -> StoredEvent {
        let payload = format!(
            r#"{{"action_digest":"sha256:{}","action_kind":"{}","challenge":"{}","code":"{}","confirmed_at":{},"principal":"{}"}}"#,
            self.digest,
            self.kind,
            self.challenge,
            self.code,
            self.confirmed_at,
            ulid(self.principal),
        );
        row(seq, event_id, "StepUpConfirmed", self.principal, &payload)
    }
}

/// An earlier grant on the stream whose step-up names `challenge` (API-17).
fn used(seq: u64, challenge: &str) -> StoredEvent {
    let payload = format!(
        r#"{{"agent":"agent_a","approval":"01JAA2P0A000000000000000A1","content_hash":"sha256:{}","responder":"{}","role":"approver","step_up":{{"assertion_id":"{challenge}","authenticated_at":{ISSUED},"method":"cli_confirm"}},"submitted_at":{ISSUED},"verdict":"approved"}}"#,
        Digest::of(b"an earlier approval"),
        ulid(OWNER)
    );
    row(
        seq,
        "01JAA2P4A000000000000000U1",
        "ApprovalResponseSubmitted",
        OWNER,
        &payload,
    )
}

/// The grant's `cli_confirm` evidence for the challenge `id`, authenticated at `secs`.
fn evidence(id: &str, secs: i64) -> StepUpEvidence {
    StepUpEvidence {
        assertion_id: EventId::try_from(id.to_owned()).expect("a ULID"),
        authenticated_at: Timestamp::try_from(at(secs).to_string()).expect("a timestamp"),
        method: StepUpMethod::CliConfirm,
    }
}

/// A port over a store that holds one approval challenge, issued to the owner at `ISSUED`.
struct World {
    port: CliConfirm,
    record: ChallengeRecord,
    action: stepup::Action,
}

fn world(linked: &[PrincipalId]) -> World {
    let challenges = Arc::new(Challenges::default());
    let action = approval(b"the approval's content object");
    let record = challenges
        .issue(
            &context(OWNER, StepUpActionKind::Approve),
            action,
            AssertionId(CHALLENGE.to_owned()),
            at(ISSUED),
        )
        .expect("the owner obtains an approval's challenge");
    World {
        port: CliConfirm {
            challenges,
            links: Arc::new(Linked(linked.to_vec())),
        },
        record,
        action,
    }
}

impl World {
    /// The port's verdict on `presented` for `action`, under `who`'s context, at `now`.
    fn judge(
        &self,
        presented: &StepUpEvidence,
        action: stepup::Action,
        who: PrincipalId,
        control: &[StoredEvent],
        now: i64,
    ) -> Result<(), ProblemCode> {
        let tenant = context(who, action.kind);
        let judged = Judged {
            tenant: &tenant,
            control,
            now: at(now),
        };
        self.port.verify(presented, &asked(action), &judged)
    }

    /// The owner's grant of the held challenge, as confirmed by `confirmation`, at 30 s.
    fn grant(&self, control: &[StoredEvent]) -> Result<(), ProblemCode> {
        self.judge(
            &evidence(CHALLENGE, ISSUED),
            self.action,
            OWNER,
            control,
            ISSUED + 30,
        )
    }
}

#[test]
#[ignore = "pending E9-4"]
fn the_code_is_the_challenges_first_40_bits_in_crockford_base_32() {
    assert_eq!(code(&[0u8; 32]).as_deref(), Some("00000000"));
    assert_eq!(code(&[0xff; 32]).as_deref(), Some("ZZZZZZZZ"));
    let bytes: Vec<u8> = (1..=32).collect();
    assert_eq!(code(&bytes), Some(crockford40(&bytes)));
    assert_eq!(code(&bytes).map(|c| c.len()), Some(CODE_LEN));
    assert_eq!(code(&bytes[..4]), None, "fewer than 40 bits has no code");
    let w = world(&[OWNER]);
    let challenge = w.record.webauthn_challenge().expect("a canonical record");
    assert_eq!(
        code(&unbase64url(&challenge.base64url())).as_deref(),
        Some(shown(&w.record).as_str())
    );
}

#[test]
#[ignore = "pending E9-4"]
fn a_confirmed_paper_approve_verifies_once() {
    let w = world(&[OWNER]);
    let control = [Confirmation::of(&w.record).row(1, "01JAA2P4A000000000000000C1")];
    assert_eq!(w.grant(&control), Ok(()));
    assert_eq!(
        w.grant(&control),
        Err(ProblemCode::StepUpMissing),
        "the challenge is dropped once a grant verifies with it"
    );
}

#[test]
#[ignore = "pending E9-4"]
fn check_1_cli_confirm_for_another_kind_is_step_up_method() {
    let w = world(&[OWNER]);
    let control = [Confirmation::of(&w.record).row(1, "01JAA2P4A000000000000000C1")];
    let stop = stepup::Action {
        kind: StepUpActionKind::Stop,
        digest: w.action.digest,
    };
    let got = w.judge(
        &evidence(CHALLENGE, ISSUED),
        stop,
        OWNER,
        &control,
        ISSUED + 30,
    );
    assert_eq!(got, Err(ProblemCode::StepUpMethod));
}

#[test]
#[ignore = "pending E9-4"]
fn check_2_a_challenge_the_server_does_not_hold_is_missing() {
    let w = world(&[OWNER]);
    let mut other = Confirmation::of(&w.record);
    other.challenge = ANOTHER_CHALLENGE.to_owned();
    let control = [other.row(1, "01JAA2P4A000000000000000C1")];
    let got = w.judge(
        &evidence(ANOTHER_CHALLENGE, ISSUED),
        w.action,
        OWNER,
        &control,
        ISSUED + 30,
    );
    assert_eq!(got, Err(ProblemCode::StepUpMissing));
}

#[test]
#[ignore = "pending E9-4"]
fn check_3_a_challenge_for_another_digest_or_principal_is_a_mismatch() {
    let w = world(&[OWNER, OTHER]);
    let control = [Confirmation::of(&w.record).row(1, "01JAA2P4A000000000000000C1")];
    let other_approval = approval(b"another approval's content object");
    let digest = w.judge(
        &evidence(CHALLENGE, ISSUED),
        other_approval,
        OWNER,
        &control,
        ISSUED + 30,
    );
    assert_eq!(digest, Err(ProblemCode::StepUpMismatch));
    let principal = w.judge(
        &evidence(CHALLENGE, ISSUED),
        w.action,
        OTHER,
        &control,
        ISSUED + 30,
    );
    assert_eq!(principal, Err(ProblemCode::StepUpMismatch));
}

#[test]
#[ignore = "pending E9-4"]
fn check_4_outside_the_challenges_life_or_another_instant_is_stale() {
    let w = world(&[OWNER]);
    let control = [Confirmation::of(&w.record).row(1, "01JAA2P4A000000000000000C1")];
    let presented = evidence(CHALLENGE, ISSUED);
    assert_eq!(
        w.judge(&presented, w.action, OWNER, &control, ISSUED + 301),
        Err(ProblemCode::StepUpStale),
        "after expires_at"
    );
    assert_eq!(
        w.judge(&presented, w.action, OWNER, &control, ISSUED - 1),
        Err(ProblemCode::StepUpStale),
        "before issued_at"
    );
    assert_eq!(
        w.judge(
            &evidence(CHALLENGE, ISSUED + 5),
            w.action,
            OWNER,
            &control,
            ISSUED + 30
        ),
        Err(ProblemCode::StepUpStale),
        "authenticated_at is not the challenge's issued_at"
    );
}

#[test]
#[ignore = "pending E9-4"]
fn check_5_a_challenge_an_earlier_event_carries_is_reused() {
    let w = world(&[OWNER]);
    let control = [
        Confirmation::of(&w.record).row(1, "01JAA2P4A000000000000000C1"),
        used(2, CHALLENGE),
    ];
    assert_eq!(w.grant(&control), Err(ProblemCode::StepUpReused));
}

#[test]
#[ignore = "pending E9-4"]
fn check_6_no_confirmation_or_two_is_missing() {
    let w = world(&[OWNER]);
    assert_eq!(w.grant(&[]), Err(ProblemCode::StepUpMissing), "none");
    let confirmed = Confirmation::of(&w.record);
    let twice = [
        confirmed.row(1, "01JAA2P4A000000000000000C1"),
        confirmed.row(2, "01JAA2P4A000000000000000C2"),
    ];
    assert_eq!(w.grant(&twice), Err(ProblemCode::StepUpMissing), "two");
}

#[test]
#[ignore = "pending E9-4"]
fn check_7_a_confirmation_for_another_action_or_principal_is_a_mismatch() {
    let w = world(&[OWNER, OTHER]);
    let mut digest = Confirmation::of(&w.record);
    digest.digest = Digest::of(b"another approval's content object");
    assert_eq!(
        w.grant(&[digest.row(1, "01JAA2P4A000000000000000C1")]),
        Err(ProblemCode::StepUpMismatch),
        "another digest"
    );
    let mut kind = Confirmation::of(&w.record);
    kind.kind = "stop";
    assert_eq!(
        w.grant(&[kind.row(1, "01JAA2P4A000000000000000C1")]),
        Err(ProblemCode::StepUpMismatch),
        "another kind"
    );
    let mut principal = Confirmation::of(&w.record);
    principal.principal = OTHER;
    assert_eq!(
        w.grant(&[principal.row(1, "01JAA2P4A000000000000000C1")]),
        Err(ProblemCode::StepUpMismatch),
        "another principal"
    );
}

#[test]
#[ignore = "pending E9-4"]
fn check_7_a_principal_the_subject_link_no_longer_names_is_missing() {
    let w = world(&[OTHER]);
    let control = [Confirmation::of(&w.record).row(1, "01JAA2P4A000000000000000C1")];
    assert_eq!(w.grant(&control), Err(ProblemCode::StepUpMissing));
}

#[test]
#[ignore = "pending E9-4"]
fn check_8_a_confirmation_outside_the_challenges_life_is_stale() {
    let w = world(&[OWNER]);
    for (secs, when) in [
        (ISSUED - 1, "before issued_at"),
        (ISSUED + 301, "after expires_at"),
    ] {
        let mut late = Confirmation::of(&w.record);
        late.confirmed_at = secs;
        let control = [late.row(1, "01JAA2P4A000000000000000C1")];
        assert_eq!(w.grant(&control), Err(ProblemCode::StepUpStale), "{when}");
    }
}

#[test]
#[ignore = "pending E9-4"]
fn check_9_another_code_is_missing() {
    let w = world(&[OWNER]);
    let mut wrong = Confirmation::of(&w.record);
    let first = if wrong.code.starts_with('0') {
        "1"
    } else {
        "0"
    };
    wrong.code = format!("{first}{}", &wrong.code[1..]);
    let control = [wrong.row(1, "01JAA2P4A000000000000000C1")];
    assert_eq!(w.grant(&control), Err(ProblemCode::StepUpMissing));
}

/// A `StepUpConfirmed` that differs from the held challenge in its kind, digest, challenge, or
/// principal never verifies the grant, whatever it differs by. Every other principal is linked,
/// so only the binding can refuse it.
#[test]
#[ignore = "pending E9-4"]
fn a_confirmation_for_another_kind_digest_challenge_or_principal_never_verifies() {
    let kinds = [
        "confirm_version",
        "deploy",
        "stop",
        "owner_exit",
        "Approve",
        "",
    ];
    let case = (
        0..4usize,
        0..kinds.len(),
        proptest::collection::vec(any::<u8>(), 1..64),
        any::<u128>(),
    );
    let mut runner = TestRunner::new(Config {
        cases: 128,
        failure_persistence: None,
        ..Config::default()
    });
    let outcome = runner.run(&case, |(differs, kind, salt, id)| {
        let other = PrincipalId(if id == OWNER.0 { id ^ 1 } else { id });
        let w = world(&[OWNER, other]);
        let mut confirmed = Confirmation::of(&w.record);
        match differs {
            0 => confirmed.kind = kinds[kind],
            1 => confirmed.digest = Digest::of(&salt),
            2 => confirmed.challenge = ulid(other),
            _ => confirmed.principal = other,
        }
        prop_assume!(confirmed.digest != w.action.digest || differs != 1);
        prop_assume!(confirmed.challenge != CHALLENGE);
        let control = [confirmed.row(1, "01JAA2P4A000000000000000C1")];
        if w.grant(&control).is_ok() {
            return Err(TestCaseError::fail(format!("{confirmed:?} verified")));
        }
        Ok(())
    });
    if let Err(failure) = outcome {
        panic!("{failure}");
    }
}

/// A file of subject links at a fresh path, owned by this process and writable only by it.
fn links_file(name: &str, lines: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "mandate-api-server-names-{name}-{}",
        std::process::id()
    ));
    std::fs::write(&path, lines).expect("written");
    let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o600);
    std::fs::set_permissions(&path, permissions).expect("mode");
    path
}

#[test]
#[ignore = "pending E9-4"]
fn the_links_name_a_principal_only_on_one_of_their_lines() {
    let line = format!("sha256:{} {}\n", "a".repeat(64), ulid(OWNER));
    let path = links_file("lines", &line);
    assert_eq!(names_at(&path, OWNER), Ok(true));
    assert_eq!(names_at(&path, OTHER), Ok(false));
    let malformed = links_file("malformed", &format!("{line}not a line\n"));
    assert_eq!(
        names_at(&malformed, OWNER),
        Err(LinkRefusal::Malformed),
        "a file refused whole names nobody"
    );
    let open = links_file("open", &line);
    let mut permissions = std::fs::metadata(&open).expect("metadata").permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o622);
    std::fs::set_permissions(&open, permissions).expect("mode");
    assert_eq!(names_at(&open, OWNER), Err(LinkRefusal::Exposed));
}

/// DEC-1023 and DEC-2104 item 4: the deployment's links are read at the fixed path, whatever that
/// file holds on the machine running the test, absent included.
#[test]
#[ignore = "pending E9-4"]
fn the_deployment_names_principals_from_the_fixed_path_only() {
    let fixed = std::path::Path::new(SUBJECT_LINKS);
    for principal in [OWNER, OTHER] {
        assert_eq!(Fixed.names(principal), names_at(fixed, principal));
    }
}
