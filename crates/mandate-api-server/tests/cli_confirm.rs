//! `cli_confirm` step-up for a paper grant made in the web app (DEC-2104, DEC-2103 item 7, decided
//! by the founder in DEC-2605 row 5): the code the web app shows, and the port's judgment of a
//! grant's evidence against the challenge this server holds and the owner's `StepUpConfirmed`
//! (journal spec §9.29), one test per check of DEC-2104 item 6, the checks' order, and their
//! window bounds.
//!
//! Every context comes from the real `authorize` over the testkit's store, in a workspace and org
//! the test names. The oracles are the test's own: the code is encoded here from the challenge's
//! bytes, decoded here from its base64url; the expected refusal is the one the test arranged; and
//! the property test names, for each member it changes, the one code DEC-2104 item 6 answers.

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
    AssertionId, Authorized, OrgId, Principal, PrincipalId, Role, Scope, SessionRef,
    StepUpActionKind, TenantContext, WorkspaceId, authorize,
};
use mandate_identity_testkit::{StaticLookup, membership, session};
use mandate_journal::{Draft, StoredEvent};
use mandate_passkey::stepup::{self, ChallengeRecord, permission_for};
use mandate_time::UtcNanos;
use proptest::prelude::any;
use proptest::test_runner::{Config, TestCaseError, TestRunner};

const OWNER: PrincipalId = PrincipalId(0x31);
const OTHER: PrincipalId = PrincipalId(0x32);
const SESSION: SessionRef = SessionRef(0x41);
const ISSUED: i64 = 1_790_000_000;
/// A challenge's life in seconds (identity spec §7.1).
const LIFE: i64 = 300;
const CHALLENGE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0T";
const ANOTHER_CHALLENGE: &str = "01J9ZQ4B7Y8K3M5N6P7Q8R9S0V";
const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
const BASE64URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
/// The kinds other than `approve` a principal holding these roles may act on, for check 1.
const OTHER_KINDS: [StepUpActionKind; 6] = [
    StepUpActionKind::Stop,
    StepUpActionKind::Resume,
    StepUpActionKind::Deploy,
    StepUpActionKind::OwnerExit,
    StepUpActionKind::Acknowledge,
    StepUpActionKind::ConfirmVersion,
];
/// A confirmation's `action_kind` values that are not the challenge's `approve`, for check 7.
const OTHER_KIND_TEXTS: [&str; 6] = [
    "stop",
    "confirm_version",
    "deploy",
    "owner_exit",
    "Approve",
    "approve ",
];

/// The org and workspace a request is made in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Place {
    org: OrgId,
    workspace: WorkspaceId,
}

/// Where the challenge is issued.
const HOME: Place = Place {
    org: OrgId(0x11),
    workspace: WorkspaceId(0x21),
};
/// Another workspace of the same org.
const NEIGHBOUR: Place = Place {
    org: OrgId(0x11),
    workspace: WorkspaceId(0x22),
};
/// A workspace of another org.
const ABROAD: Place = Place {
    org: OrgId(0x12),
    workspace: WorkspaceId(0x23),
};

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

/// The context `authorize` yields to `who` for `kind`'s row in `place`: the owner and the other
/// principal are workspace admins, operators, and approvers in every workspace named here and in
/// `place`, each through a full session holding its own memberships.
fn context(who: PrincipalId, kind: StepUpActionKind, place: Place) -> TenantContext {
    let roles = [
        (Role::WorkspaceAdmin, at(ISSUED - 86_400)),
        (Role::Operator, at(ISSUED - 86_400)),
        (Role::Approver, at(ISSUED - 86_400)),
    ];
    let mut places = vec![HOME, NEIGHBOUR, ABROAD];
    if !places.contains(&place) {
        places.push(place);
    }
    let scope = |p: Place| Scope::Workspace {
        org: p.org,
        workspace: p.workspace,
    };
    let member = |who, p| membership(who, scope(p), Active, &roles);
    let all = [OWNER, OTHER, who]
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

/// One control-stream row of `event_type` at `seq`: a whole envelope written by the user
/// `actor` at `secs`, its body canonical.
fn row(
    seq: u64,
    event_id: &str,
    event_type: &str,
    actor: PrincipalId,
    secs: i64,
    payload: &str,
    artifact_refs: &[String],
) -> StoredEvent {
    let refs: Vec<String> = artifact_refs.iter().map(|r| format!("\"{r}\"")).collect();
    let text = format!(
        r#"{{"actor":{{"build":null,"id":"{}","kind":"user","version":"1"}},"artifact_refs":[{}],"causation_id":null,"clock_source":"local","config_refs":{{}},"correlation_id":null,"environment":"paper","envelope_version":1,"event_id":"{event_id}","event_time":"{}","event_type":"{event_type}","payload":{payload},"pii_refs":[],"schema_version":1,"stream_id":"ctl:ws_1"}}"#,
        ulid(actor),
        refs.join(","),
        at(secs),
    );
    let body = to_canonical(&parse(text.as_bytes()).expect("JSON"));
    StoredEvent {
        stream_id: "ctl:ws_1".to_owned(),
        seq,
        event_id: event_id.to_owned(),
        event_type: event_type.to_owned(),
        schema_version: 1,
        environment: "paper".to_owned(),
        recorded_at: at(secs).to_string(),
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
        let digest = format!("sha256:{}", self.digest);
        let payload = format!(
            r#"{{"action_digest":"{digest}","action_kind":"{}","challenge":"{}","code":"{}","confirmed_at":{},"principal":"{}"}}"#,
            self.kind,
            self.challenge,
            self.code,
            self.confirmed_at,
            ulid(self.principal),
        );
        row(
            seq,
            event_id,
            "StepUpConfirmed",
            self.principal,
            self.confirmed_at,
            &payload,
            &[digest],
        )
    }
}

/// An earlier grant on the stream whose step-up names `challenge` (API-17), checked against the
/// journal's own schema of `ApprovalResponseSubmitted` so it is a record the stream could hold.
fn used(seq: u64, challenge: &str) -> StoredEvent {
    let content = format!("sha256:{}", Digest::of(b"an earlier approval"));
    let payload = format!(
        r#"{{"agent":"agent_a","approval":"01JAA2P0A000000000000000A1","content_hash":"{content}","responder":"{}","role":"approver","step_up":{{"assertion_id":"{challenge}","authenticated_at":{ISSUED},"method":"cli_confirm"}},"submitted_at":{ISSUED},"verdict":"approved"}}"#,
        ulid(OWNER)
    );
    let grant = row(
        seq,
        "01JAA2P4A000000000000000R1",
        "ApprovalResponseSubmitted",
        OWNER,
        ISSUED,
        &payload,
        &[content],
    );
    let parsed = Draft::parse(&grant.body).map(|d| d.event_type().to_owned());
    assert_eq!(
        parsed.as_deref(),
        Ok("ApprovalResponseSubmitted"),
        "the earlier grant is one the journal accepts"
    );
    grant
}

/// The grant's `cli_confirm` evidence for the challenge `id`, authenticated at `secs`.
fn evidence(id: &str, secs: i64) -> StepUpEvidence {
    StepUpEvidence {
        assertion_id: EventId::try_from(id.to_owned()).expect("a ULID"),
        authenticated_at: Timestamp::try_from(at(secs).to_string()).expect("a timestamp"),
        method: StepUpMethod::CliConfirm,
    }
}

/// A port over a store that holds one approval challenge, issued to the owner in `HOME` at
/// `ISSUED`, with the subject links naming the principals given.
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
            &context(OWNER, StepUpActionKind::Approve, HOME),
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

/// One request against the world: who asks, where, for what, with which evidence, at which
/// second, over which control rows.
#[derive(Clone)]
struct Ask {
    who: PrincipalId,
    place: Place,
    action: stepup::Action,
    evidence: StepUpEvidence,
    now: i64,
}

impl World {
    /// The owner's grant of the held challenge in `HOME`, 30 s after it was issued.
    fn ask(&self) -> Ask {
        Ask {
            who: OWNER,
            place: HOME,
            action: self.action,
            evidence: evidence(CHALLENGE, ISSUED),
            now: ISSUED + 30,
        }
    }

    /// The port's verdict on `ask` over `control`.
    fn judge(&self, ask: &Ask, control: &[StoredEvent]) -> Result<(), ProblemCode> {
        let tenant = context(ask.who, ask.action.kind, ask.place);
        let judged = Judged {
            tenant: &tenant,
            control,
            now: at(ask.now),
        };
        self.port.verify(&ask.evidence, &asked(ask.action), &judged)
    }

    /// The owner's grant, as [`World::ask`] makes it, over `control`.
    fn grant(&self, control: &[StoredEvent]) -> Result<(), ProblemCode> {
        self.judge(&self.ask(), control)
    }

    /// The owner's confirmation of the held challenge, as one control row.
    fn confirmed(&self) -> Vec<StoredEvent> {
        vec![Confirmation::of(&self.record).row(1, "01JAA2P4A000000000000000C1")]
    }
}

#[test]
fn the_code_is_the_challenges_first_40_bits_in_crockford_base_32() {
    assert_eq!(code(&[0u8; 32]).as_deref(), Some("00000000"));
    assert_eq!(code(&[0xff; 32]).as_deref(), Some("ZZZZZZZZ"));
    let bytes: Vec<u8> = (1..=32).collect();
    assert_eq!(code(&bytes), Some(crockford40(&bytes)));
    assert_eq!(code(&bytes).map(|c| c.len()), Some(CODE_LEN));
    assert_eq!(code(&bytes[..4]), None, "fewer than 40 bits has no code");
    assert_eq!(
        code(&bytes[..5]),
        Some(crockford40(&bytes[..5])),
        "40 bits is enough"
    );
    let w = world(&[OWNER]);
    let challenge = w.record.webauthn_challenge().expect("a canonical record");
    assert_eq!(
        code(&unbase64url(&challenge.base64url())).as_deref(),
        Some(shown(&w.record).as_str())
    );
}

#[test]
fn a_confirmed_paper_approve_verifies_once() {
    let w = world(&[OWNER]);
    let control = w.confirmed();
    assert_eq!(w.grant(&control), Ok(()));
    assert_eq!(
        w.grant(&control),
        Err(ProblemCode::StepUpMissing),
        "the challenge is dropped once a grant verifies with it"
    );
}

#[test]
fn check_1_cli_confirm_for_every_other_kind_is_step_up_method() {
    for kind in OTHER_KINDS {
        let w = world(&[OWNER]);
        let mut ask = w.ask();
        ask.action.kind = kind;
        assert_eq!(
            w.judge(&ask, &w.confirmed()),
            Err(ProblemCode::StepUpMethod),
            "{kind:?}"
        );
    }
}

#[test]
fn check_2_a_challenge_the_server_does_not_hold_is_missing() {
    let w = world(&[OWNER]);
    let mut other = Confirmation::of(&w.record);
    other.challenge = ANOTHER_CHALLENGE.to_owned();
    let control = [other.row(1, "01JAA2P4A000000000000000C1")];
    let mut ask = w.ask();
    ask.evidence = evidence(ANOTHER_CHALLENGE, ISSUED);
    assert_eq!(w.judge(&ask, &control), Err(ProblemCode::StepUpMissing));
}

/// Check 2 as DEC-2105 item 3 reads it: the lookup comes before the store's expiry drop, so an
/// expired challenge is still held and stale at check 4, and only once the store has dropped it,
/// when it next issues one, is it missing.
#[test]
fn check_2_an_expired_challenge_is_stale_until_the_store_drops_it() {
    let w = world(&[OWNER]);
    let control = w.confirmed();
    let expired = Ask {
        now: ISSUED + LIFE,
        ..w.ask()
    };
    assert_eq!(w.judge(&expired, &control), Err(ProblemCode::StepUpStale));
    w.port
        .challenges
        .issue(
            &context(OWNER, StepUpActionKind::Approve, HOME),
            approval(b"a later approval's content object"),
            AssertionId(ANOTHER_CHALLENGE.to_owned()),
            at(ISSUED + LIFE),
        )
        .expect("a later challenge, issued as the first expires");
    assert_eq!(
        w.judge(&expired, &control),
        Err(ProblemCode::StepUpMissing),
        "dropped by the later issue"
    );
}

/// Check 3 binds the challenge to its principal, workspace, and org, and to the action's digest:
/// the owner's challenge presented by another principal, in another workspace of the same org, in
/// another org's workspace, or for another approval, is a mismatch. `ChallengeRecord` holds no
/// org: the workspace id is what binds it, since no two orgs share a workspace.
#[test]
fn check_3_another_digest_principal_workspace_or_org_is_a_mismatch() {
    let w = world(&[OWNER, OTHER]);
    let control = w.confirmed();
    let base = w.ask();
    let cases: [(&str, Ask); 4] = [
        (
            "another digest",
            Ask {
                action: approval(b"another approval's content object"),
                ..base.clone()
            },
        ),
        (
            "another principal",
            Ask {
                who: OTHER,
                ..base.clone()
            },
        ),
        (
            "another workspace of the org",
            Ask {
                place: NEIGHBOUR,
                ..base.clone()
            },
        ),
        (
            "another org",
            Ask {
                place: ABROAD,
                ..base.clone()
            },
        ),
    ];
    for (name, ask) in cases {
        assert_eq!(
            w.judge(&ask, &control),
            Err(ProblemCode::StepUpMismatch),
            "{name}"
        );
    }
}

/// Check 4 as DEC-2105 item 1 reads it: `now` is within `[issued_at, expires_at)`, the half-open life
/// `ChallengeRecord::is_current_at` and the store keep, and `authenticated_at` is `issued_at`.
#[test]
fn check_4_the_challenges_life_is_issued_at_up_to_but_not_including_expiry() {
    for (now, want) in [
        (ISSUED - 1, Err(ProblemCode::StepUpStale)),
        (ISSUED, Ok(())),
        (ISSUED + LIFE - 1, Ok(())),
        (ISSUED + LIFE, Err(ProblemCode::StepUpStale)),
        (ISSUED + LIFE + 1, Err(ProblemCode::StepUpStale)),
    ] {
        let w = world(&[OWNER]);
        let mut confirmed = Confirmation::of(&w.record);
        confirmed.confirmed_at = ISSUED;
        let control = [confirmed.row(1, "01JAA2P4A000000000000000C1")];
        let ask = Ask { now, ..w.ask() };
        assert_eq!(w.judge(&ask, &control), want, "now {}", now - ISSUED);
    }
    let w = world(&[OWNER]);
    let ask = Ask {
        evidence: evidence(CHALLENGE, ISSUED + 5),
        ..w.ask()
    };
    assert_eq!(
        w.judge(&ask, &w.confirmed()),
        Err(ProblemCode::StepUpStale),
        "authenticated_at is not the challenge's issued_at"
    );
}

#[test]
fn check_5_a_challenge_an_earlier_event_carries_is_reused() {
    let w = world(&[OWNER]);
    let mut control = w.confirmed();
    control.push(used(2, CHALLENGE));
    assert_eq!(w.grant(&control), Err(ProblemCode::StepUpReused));
}

#[test]
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
fn check_7_a_confirmation_for_another_action_or_principal_is_a_mismatch() {
    let w = world(&[OWNER, OTHER]);
    let mut digest = Confirmation::of(&w.record);
    digest.digest = Digest::of(b"another approval's content object");
    assert_eq!(
        w.grant(&[digest.row(1, "01JAA2P4A000000000000000C1")]),
        Err(ProblemCode::StepUpMismatch),
        "another digest"
    );
    for kind in OTHER_KIND_TEXTS {
        let w = world(&[OWNER]);
        let mut other = Confirmation::of(&w.record);
        other.kind = kind;
        assert_eq!(
            w.grant(&[other.row(1, "01JAA2P4A000000000000000C1")]),
            Err(ProblemCode::StepUpMismatch),
            "kind {kind:?}"
        );
    }
    let w = world(&[OWNER, OTHER]);
    let mut principal = Confirmation::of(&w.record);
    principal.principal = OTHER;
    assert_eq!(
        w.grant(&[principal.row(1, "01JAA2P4A000000000000000C1")]),
        Err(ProblemCode::StepUpMismatch),
        "another principal"
    );
}

#[test]
fn check_7_a_principal_the_subject_link_no_longer_names_is_missing() {
    let w = world(&[OTHER]);
    assert_eq!(w.grant(&w.confirmed()), Err(ProblemCode::StepUpMissing));
}

/// Check 8 as DEC-2105 item 2 reads it: `confirmed_at` is a whole second, and the whole of it lies in the challenge's
/// half-open life: no earlier than `issued_at`'s second, and ending by `expires_at`.
#[test]
fn check_8_the_confirmations_second_lies_wholly_within_the_challenges_life() {
    for (second, want) in [
        (ISSUED - 1, Err(ProblemCode::StepUpStale)),
        (ISSUED, Ok(())),
        (ISSUED + LIFE - 1, Ok(())),
        (ISSUED + LIFE, Err(ProblemCode::StepUpStale)),
    ] {
        let w = world(&[OWNER]);
        let mut confirmed = Confirmation::of(&w.record);
        confirmed.confirmed_at = second;
        let control = [confirmed.row(1, "01JAA2P4A000000000000000C1")];
        let ask = Ask {
            now: ISSUED + 10,
            ..w.ask()
        };
        assert_eq!(
            w.judge(&ask, &control),
            want,
            "confirmed at {}",
            second - ISSUED
        );
    }
}

/// `code` with its first two different characters swapped: wrong at two places by the same bits,
/// which a comparison that lets two differences cancel would accept.
fn swapped(code: &str) -> String {
    let mut chars: Vec<char> = code.chars().collect();
    let second = chars
        .iter()
        .position(|c| *c != chars[0])
        .expect("two different characters");
    chars.swap(0, second);
    chars.into_iter().collect()
}

#[test]
fn check_9_any_other_code_is_missing() {
    let w = world(&[OWNER]);
    let right = shown(&w.record);
    let first = if right.starts_with('0') { "1" } else { "0" };
    let wrong = [
        format!("{first}{}", &right[1..]),
        right[..7].to_owned(),
        format!("{right}0"),
        right.to_lowercase(),
        format!("{}-{}", &right[..4], &right[4..]),
        String::new(),
        swapped(&right),
    ];
    for code in wrong {
        let w = world(&[OWNER]);
        let mut confirmed = Confirmation::of(&w.record);
        confirmed.code = code.clone();
        let control = [confirmed.row(1, "01JAA2P4A000000000000000C1")];
        assert_eq!(
            w.grant(&control),
            Err(ProblemCode::StepUpMissing),
            "code {code:?}"
        );
    }
}

/// DEC-2104 item 6's order: a step-up failing two checks gets the earlier check's code.
#[test]
fn a_step_up_failing_two_checks_gets_the_earlier_ones_code() {
    let w = world(&[OWNER]);
    let ask = Ask {
        action: stepup::Action {
            kind: StepUpActionKind::Stop,
            digest: w.action.digest,
        },
        evidence: evidence(ANOTHER_CHALLENGE, ISSUED),
        ..w.ask()
    };
    assert_eq!(
        w.judge(&ask, &w.confirmed()),
        Err(ProblemCode::StepUpMethod),
        "1 before 2: another kind and a challenge not held"
    );
    let w = world(&[OWNER]);
    let ask = Ask {
        action: approval(b"another approval's content object"),
        now: ISSUED + LIFE + 1,
        ..w.ask()
    };
    assert_eq!(
        w.judge(&ask, &w.confirmed()),
        Err(ProblemCode::StepUpMismatch),
        "3 before 4: another digest and expired"
    );
    let w = world(&[OWNER]);
    let mut control = w.confirmed();
    control.push(used(2, CHALLENGE));
    let ask = Ask {
        now: ISSUED + LIFE + 1,
        ..w.ask()
    };
    assert_eq!(
        w.judge(&ask, &control),
        Err(ProblemCode::StepUpStale),
        "4 before 5: expired and reused"
    );
    let w = world(&[OWNER]);
    assert_eq!(
        w.grant(&[used(1, CHALLENGE)]),
        Err(ProblemCode::StepUpReused),
        "5 before 6: reused and not confirmed"
    );
    let w = world(&[OWNER]);
    let mut confirmed = Confirmation::of(&w.record);
    confirmed.digest = Digest::of(b"another approval's content object");
    confirmed.code = "00000000".to_owned();
    assert_eq!(
        w.grant(&[confirmed.row(1, "01JAA2P4A000000000000000C1")]),
        Err(ProblemCode::StepUpMismatch),
        "7 before 9: another digest and another code"
    );
}

/// A request or a confirmation that differs from the held challenge in one bound member gets the
/// one code DEC-2104 item 6 names for that member, whatever it differs by: a confirmation for
/// another kind, digest, or principal is a mismatch at check 7; one naming another challenge
/// leaves the held one unconfirmed, missing at check 6; a request from another workspace or org
/// is a mismatch at check 3. Every principal drawn is linked, so only the binding can refuse it.
#[test]
fn one_bound_member_changed_gets_that_members_code() {
    let case = (
        0..6usize,
        0..OTHER_KIND_TEXTS.len(),
        proptest::collection::vec(any::<u8>(), 0..64),
        any::<u128>(),
    );
    let mut runner = TestRunner::new(Config {
        cases: 192,
        failure_persistence: None,
        ..Config::default()
    });
    let outcome = runner.run(&case, |(member, kind, salt, n)| {
        let other = PrincipalId(if n == OWNER.0 { n ^ 1 } else { n });
        let w = world(&[OWNER, other]);
        let mut confirmed = Confirmation::of(&w.record);
        let mut ask = w.ask();
        let want = match member {
            0 => {
                confirmed.kind = OTHER_KIND_TEXTS[kind];
                ProblemCode::StepUpMismatch
            }
            1 => {
                confirmed.digest = Digest::of(&[b"not the approval:".as_slice(), &salt].concat());
                ProblemCode::StepUpMismatch
            }
            2 => {
                confirmed.principal = other;
                ProblemCode::StepUpMismatch
            }
            3 => {
                let id = ulid(other);
                confirmed.challenge = if id == CHALLENGE {
                    ANOTHER_CHALLENGE.to_owned()
                } else {
                    id
                };
                ProblemCode::StepUpMissing
            }
            4 => {
                let workspace = ((n & u128::from(u64::MAX)) | 1) << 8;
                ask.place = Place {
                    org: HOME.org,
                    workspace: WorkspaceId(workspace),
                };
                ProblemCode::StepUpMismatch
            }
            _ => {
                let workspace = ((n & u128::from(u64::MAX)) | 1) << 8;
                ask.place = Place {
                    org: OrgId(((n & u128::from(u64::MAX)) | 1) << 16),
                    workspace: WorkspaceId(workspace),
                };
                ProblemCode::StepUpMismatch
            }
        };
        let control = [confirmed.row(1, "01JAA2P4A000000000000000C1")];
        let got = w.judge(&ask, &control);
        if got != Err(want) {
            return Err(TestCaseError::fail(format!(
                "member {member}: {confirmed:?} at {:?} gave {got:?}, not {want:?}",
                ask.place
            )));
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
fn the_deployment_names_principals_from_the_fixed_path_only() {
    let fixed = std::path::Path::new(SUBJECT_LINKS);
    for principal in [OWNER, OTHER] {
        assert_eq!(Fixed.names(principal), names_at(fixed, principal));
    }
}

/// Every step-up kind but `approve` and `connection`, written out here from the identity spec's
/// list rather than read from `StepUpActionKind::ALL`, so a kind added or admitted without a
/// decision fails (DEC-2125 item 1).
const REFUSED_KINDS: [StepUpActionKind; 16] = [
    StepUpActionKind::ConfirmVersion,
    StepUpActionKind::Deploy,
    StepUpActionKind::Resume,
    StepUpActionKind::Stop,
    StepUpActionKind::Acknowledge,
    StepUpActionKind::OwnerExit,
    StepUpActionKind::KillSwitchPrivilege,
    StepUpActionKind::Disclosure,
    StepUpActionKind::PolicyLoosen,
    StepUpActionKind::MemberInvite,
    StepUpActionKind::RoleGrant,
    StepUpActionKind::ClientConnect,
    StepUpActionKind::CredentialEnrol,
    StepUpActionKind::NotificationAddress,
    StepUpActionKind::BreakGlassApprove,
    StepUpActionKind::LiftHold,
];

/// A world whose one challenge is the owner's for a connect: the start route's digest, here any
/// digest the challenge binds, issued in `HOME` at `ISSUED`.
fn connect_world() -> World {
    let challenges = Arc::new(Challenges::default());
    let action = stepup::Action {
        kind: StepUpActionKind::Connection,
        digest: Digest::of(b"the connect's workspace and disclosure"),
    };
    let record = challenges
        .issue(
            &context(OWNER, StepUpActionKind::Connection, HOME),
            action,
            AssertionId(CHALLENGE.to_owned()),
            at(ISSUED),
        )
        .expect("the owner obtains a connect's challenge");
    World {
        port: CliConfirm {
            challenges,
            links: Arc::new(Linked(vec![OWNER])),
        },
        record,
        action,
    }
}

/// The owner's confirmation of a connect's challenge, kind `connection`, as one control row.
fn connect_confirmed(w: &World) -> Vec<StoredEvent> {
    let confirmation = Confirmation {
        kind: "connection",
        ..Confirmation::of(&w.record)
    };
    vec![confirmation.row(1, "01JAA2P4A000000000000000C1")]
}

/// DEC-2125 item 1: a connect confirmed with the code shown verifies, once.
#[test]
#[ignore = "pending E10-13"]
fn a_confirmed_connect_verifies_once() {
    let w = connect_world();
    let control = connect_confirmed(&w);
    assert_eq!(w.grant(&control), Ok(()));
    assert_eq!(
        w.grant(&control),
        Err(ProblemCode::StepUpMissing),
        "the challenge is dropped once a connect verifies with it"
    );
}

/// A connect's confirmation with another code, a challenge an earlier event already carries, or
/// presented once its life is over, is refused as an approval's is.
#[test]
#[ignore = "pending E10-13"]
fn a_connect_with_a_wrong_replayed_or_expired_code_is_refused() {
    let w = connect_world();
    let mut wrong = Confirmation {
        kind: "connection",
        ..Confirmation::of(&w.record)
    };
    wrong.code = swapped(&wrong.code);
    assert_eq!(
        w.grant(&[wrong.row(1, "01JAA2P4A000000000000000C1")]),
        Err(ProblemCode::StepUpMissing),
        "another code"
    );
    let w = connect_world();
    let mut replayed = connect_confirmed(&w);
    replayed.insert(0, used(1, CHALLENGE));
    replayed[1].seq = 2;
    assert_eq!(
        w.grant(&replayed),
        Err(ProblemCode::StepUpReused),
        "replayed"
    );
    let w = connect_world();
    let expired = Ask {
        now: ISSUED + LIFE,
        ..w.ask()
    };
    assert_eq!(
        w.judge(&expired, &connect_confirmed(&w)),
        Err(ProblemCode::StepUpStale),
        "expired"
    );
}

/// Every kind but `approve` and `connection` is still `step_up_method`, whatever its confirmation
/// says; the list is the test's own, and with the two admitted it is every kind there is.
#[test]
fn every_kind_but_approve_and_connection_is_still_refused() {
    let mut listed: Vec<StepUpActionKind> = REFUSED_KINDS.to_vec();
    listed.extend([StepUpActionKind::Approve, StepUpActionKind::Connection]);
    let mut all = StepUpActionKind::ALL.to_vec();
    listed.sort_by_key(|k| k.code());
    all.sort_by_key(|k| k.code());
    assert_eq!(
        listed, all,
        "the refused list and the two admitted are every kind"
    );
    let tenant = context(OWNER, StepUpActionKind::Connection, HOME);
    for kind in REFUSED_KINDS {
        let w = connect_world();
        let control = connect_confirmed(&w);
        let judged = Judged {
            tenant: &tenant,
            control: &control,
            now: at(ISSUED + 30),
        };
        let action = Action {
            kind,
            digest: w.action.digest,
        };
        assert_eq!(
            w.port
                .verify(&evidence(CHALLENGE, ISSUED), &action, &judged),
            Err(ProblemCode::StepUpMethod),
            "{kind:?}"
        );
    }
}
