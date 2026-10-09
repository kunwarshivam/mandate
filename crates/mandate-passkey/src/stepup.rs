//! Action-bound step-up ([identity spec §7](../../../docs/specs/identity.md#7-step-up); backlog
//! E9-4, [DEC-662]): a challenge record bound to one principal, one workspace, and one action; the
//! WebAuthn challenge its canonical form hashes to; and the consume step that judges a presented
//! step-up in §7.2 step 4's order and returns the evidence and the used marker together, so the
//! caller commits both in the transaction that commits the action (§7.2 step 5).
//!
//! [DEC-662]: ../../../docs/project/decisions/DEC-662.md

use mandate_canon::{Digest, Value, to_canonical};
use mandate_time::UtcNanos;

use crate::{Assertion, Challenge, Credential, Refusal, RelyingParty, Verified, verify};
use mandate_identity::{
    AssertionId, PrincipalId, StepUpActionKind, StepUpEvidence, StepUpMethod, UlidTextError,
    WorkspaceId,
};

/// How long a challenge stays usable after it is issued (§7.1, §7.2 step 1).
pub const CHALLENGE_LIFETIME_S: i64 = 300;

/// The action a step-up authorizes: its kind and the SHA-256 of its canonical object
/// (workspace API spec §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    pub kind: StepUpActionKind,
    pub digest: Digest,
}

/// Why a challenge record cannot be issued.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum IssueError {
    /// The expiry falls after the last instant the journal can record.
    #[error("the challenge would expire after the last representable instant")]
    OutOfRange,
}

/// Why a challenge record has no canonical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalError {
    /// An identifier has no ULID text.
    #[error(transparent)]
    Ulid(#[from] UlidTextError),
}

/// What workspace services issue for one step-up (§7.2 step 1). Only [`ChallengeRecord::issue`]
/// builds one, so `expires_at` is always `issued_at` plus [`CHALLENGE_LIFETIME_S`]: a record read
/// back from the challenge store is rebuilt with `issue` from its stored members, never with an
/// expiry of its own.
///
/// ```compile_fail,E0451
/// use mandate_canon::Digest;
/// use mandate_identity::{AssertionId, PrincipalId, StepUpActionKind, WorkspaceId};
/// use mandate_passkey::stepup::{Action, ChallengeRecord};
/// use mandate_time::UtcNanos;
///
/// let a_day_long_challenge = ChallengeRecord {
///     challenge_id: AssertionId("01J9ZQ4B7Y8K3M5N6P7Q8R9S0T".to_owned()),
///     workspace_id: WorkspaceId(1),
///     principal_id: PrincipalId(2),
///     action: Action { kind: StepUpActionKind::Approve, digest: Digest::of(b"an approval") },
///     issued_at: UtcNanos::EPOCH,
///     expires_at: UtcNanos::from_parts(86_400, 0).unwrap(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeRecord {
    challenge_id: AssertionId,
    workspace_id: WorkspaceId,
    principal_id: PrincipalId,
    action: Action,
    issued_at: UtcNanos,
    expires_at: UtcNanos,
}

impl ChallengeRecord {
    /// The record for `action`, expiring [`CHALLENGE_LIFETIME_S`] after `issued_at`.
    pub fn issue(
        challenge_id: AssertionId,
        workspace_id: WorkspaceId,
        principal_id: PrincipalId,
        action: Action,
        issued_at: UtcNanos,
    ) -> Result<Self, IssueError> {
        let expires_at = issued_at
            .secs()
            .checked_add(CHALLENGE_LIFETIME_S)
            .and_then(|secs| UtcNanos::from_parts(secs, issued_at.nanos()).ok())
            .ok_or(IssueError::OutOfRange)?;
        Ok(Self {
            challenge_id,
            workspace_id,
            principal_id,
            action,
            issued_at,
            expires_at,
        })
    }

    /// The record's canonical form (journal spec §4): an object of seven text members, the
    /// digest as `sha256:` and its lowercase hex, the instants in the journal's timestamp form.
    /// The member names are fixed and listed in their canonical (byte) order, and every value is
    /// written by `mandate-canon` as a JSON string, so the challenge ID's text is escaped as the
    /// journal escapes it.
    pub fn canonical(&self) -> Result<Vec<u8>, CanonicalError> {
        let members_in_key_order = [
            (
                "action_digest",
                format!("sha256:{}", self.action.digest.to_hex()),
            ),
            ("action_kind", self.action.kind.code().to_owned()),
            ("challenge_id", self.challenge_id.0.clone()),
            ("expires_at", self.expires_at.to_string()),
            ("issued_at", self.issued_at.to_string()),
            ("principal_id", self.principal_id.to_ulid_text()?),
            ("workspace_id", self.workspace_id.to_ulid_text()?),
        ];
        let members: Vec<Vec<u8>> = members_in_key_order
            .into_iter()
            .map(|(key, value)| {
                [
                    to_canonical(&Value::Str(key.to_owned())),
                    b":".to_vec(),
                    to_canonical(&Value::Str(value)),
                ]
                .concat()
            })
            .collect();
        Ok([b"{".to_vec(), members.join(&b","[..]), b"}".to_vec()].concat())
    }

    /// The WebAuthn challenge: SHA-256 of [`ChallengeRecord::canonical`] (§7.2 step 2), so the
    /// authenticator signs the action itself.
    pub fn webauthn_challenge(&self) -> Result<Challenge, CanonicalError> {
        let digest = Digest::of(&self.canonical()?);
        Ok(Challenge(digest.as_bytes().to_vec()))
    }

    pub fn challenge_id(&self) -> &AssertionId {
        &self.challenge_id
    }

    pub fn workspace_id(&self) -> &WorkspaceId {
        &self.workspace_id
    }

    pub fn principal_id(&self) -> &PrincipalId {
        &self.principal_id
    }

    pub fn action(&self) -> Action {
        self.action
    }

    pub fn issued_at(&self) -> UtcNanos {
        self.issued_at
    }

    pub fn expires_at(&self) -> UtcNanos {
        self.expires_at
    }

    /// Whether the record is usable at `now`: from its `issued_at` until just before its
    /// `expires_at` (DEC-662 item 5).
    fn is_current_at(&self, now: UtcNanos) -> bool {
        self.issued_at <= now && now < self.expires_at
    }
}

/// Where the action runs, which decides the methods step-up accepts (§7.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    Paper,
    Live,
}

/// A challenge as workspace services hold it when a step-up is presented.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeState<'a> {
    /// No challenge with the presented ID exists.
    Unknown,
    Issued(&'a ChallengeRecord),
    Used(&'a ChallengeRecord),
}

/// An enrolled passkey: the credential, its holder, and when its enrolment cool-off ends
/// (§10.1; the enrolment instant itself when there is no cool-off).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnrolledCredential {
    pub principal_id: PrincipalId,
    pub credential: Credential,
    pub cool_off_ends: UtcNanos,
}

/// How the principal proved presence.
#[derive(Debug, Clone, Copy)]
pub enum Proof<'a> {
    /// A WebAuthn assertion over the challenge, with the enrolled credential it names.
    Passkey {
        assertion: Assertion<'a>,
        credential: Option<&'a EnrolledCredential>,
    },
    /// The CLI's local confirmation (mandate spec §6.1), paper only.
    CliConfirm,
}

/// A step-up as the caller presents it with the action it wants to commit.
#[derive(Debug, Clone, Copy)]
pub struct Presentation<'a> {
    pub workspace_id: &'a WorkspaceId,
    pub principal_id: &'a PrincipalId,
    pub action: Action,
    pub proof: Proof<'a>,
}

/// The challenge a consumed step-up used. The caller marks it used in the same transaction that
/// commits the action's event with [`Consumed::evidence`] (§7.2 step 5). Only [`consume`] builds
/// one.
///
/// ```compile_fail,E0451
/// use mandate_identity::AssertionId;
/// use mandate_passkey::stepup::Used;
///
/// let marked = Used { challenge_id: AssertionId("01J9ZQ4B7Y8K3M5N6P7Q8R9S0T".to_owned()) };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    challenge_id: AssertionId,
}

impl Used {
    pub fn challenge_id(&self) -> &AssertionId {
        &self.challenge_id
    }
}

/// A step-up that passed: the evidence to record, the challenge to mark used, and for a passkey
/// the signature counter to store. Only [`consume`] builds one, so no caller holds step-up
/// evidence that did not pass it.
///
/// ```compile_fail,E0451
/// use mandate_identity::StepUpEvidence;
/// use mandate_passkey::stepup::{Consumed, Used};
///
/// fn forge(evidence: StepUpEvidence, used: Used) -> Consumed {
///     Consumed { evidence, used, sign_count: None }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumed {
    evidence: StepUpEvidence,
    used: Used,
    sign_count: Option<u32>,
}

impl Consumed {
    /// `{assertion_id, authenticated_at, method}`, where `authenticated_at` is the challenge's
    /// `issued_at`, the earliest instant the gesture can have happened (DEC-662 item 7).
    pub fn evidence(&self) -> &StepUpEvidence {
        &self.evidence
    }

    pub fn used(&self) -> &Used {
        &self.used
    }

    pub fn sign_count(&self) -> Option<u32> {
        self.sign_count
    }
}

/// Why a presented step-up does not count.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StepUpRefusal {
    #[error("no step-up counts: {0:?}")]
    Missing(Missing),
    #[error("the challenge has expired or is not yet valid")]
    Stale,
    #[error("the challenge was already used")]
    Reused,
    #[error("the method is not allowed in this environment")]
    Method,
    #[error("the challenge is bound to another principal, workspace, or action")]
    Mismatch,
}

/// What made a step-up count as missing (DEC-662 items 2 and 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    UnknownChallenge,
    /// The assertion names no credential of this principal.
    NotTheirCredential,
    /// The credential is in its enrolment cool-off (§10.1).
    CoolingOff,
    /// The assertion did not verify.
    Passkey(Refusal),
    /// The record's identifiers have no ULID text, so it has no challenge to verify against.
    UnencodableChallenge,
}

impl StepUpRefusal {
    /// The API and journal reason code (§7.2 step 6; mandate spec §6.1).
    pub fn code(self) -> &'static str {
        match self {
            Self::Missing(_) => "step_up_missing",
            Self::Stale => "step_up_stale",
            Self::Reused => "step_up_reused",
            Self::Method => "step_up_method",
            Self::Mismatch => "step_up_mismatch",
        }
    }
}

/// Judges a presented step-up at `now`, in DEC-662's order: the method for the environment
/// (§7.3), then §7.2 step 4: the challenge exists, is unused, is not expired, and names this
/// principal and workspace; the credential is the principal's and past its cool-off; the
/// assertion verifies (signature, user verification, counter); and the action is the challenge's.
pub fn consume(
    rp: &RelyingParty,
    environment: Environment,
    challenge: ChallengeState<'_>,
    presented: &Presentation<'_>,
    now: UtcNanos,
) -> Result<Consumed, StepUpRefusal> {
    let method = match (presented.proof, environment) {
        (Proof::Passkey { .. }, _) => StepUpMethod::Passkey,
        (Proof::CliConfirm, Environment::Paper) => StepUpMethod::CliConfirm,
        (Proof::CliConfirm, Environment::Live) => return Err(StepUpRefusal::Method),
    };
    let record = match challenge {
        ChallengeState::Unknown => {
            return Err(StepUpRefusal::Missing(Missing::UnknownChallenge));
        }
        ChallengeState::Used(_) => return Err(StepUpRefusal::Reused),
        ChallengeState::Issued(record) => record,
    };
    if !record.is_current_at(now) {
        return Err(StepUpRefusal::Stale);
    }
    if *presented.principal_id != record.principal_id
        || *presented.workspace_id != record.workspace_id
    {
        return Err(StepUpRefusal::Mismatch);
    }
    let sign_count = match presented.proof {
        Proof::Passkey {
            assertion,
            credential,
        } => Some(passkey_sign_count(rp, record, &assertion, credential, now)?),
        Proof::CliConfirm => None,
    };
    if presented.action != record.action {
        return Err(StepUpRefusal::Mismatch);
    }
    Ok(Consumed {
        evidence: StepUpEvidence {
            assertion_id: record.challenge_id.clone(),
            authenticated_at: record.issued_at,
            method,
        },
        used: Used {
            challenge_id: record.challenge_id.clone(),
        },
        sign_count,
    })
}

/// §7.2 step 4's credential checks, then step 6 in [`verify`]'s own order (DEC-662 item 6): the
/// credential is the record principal's and past its cool-off at `now`, and the assertion
/// verifies over the record's WebAuthn challenge. Returns the counter to store.
fn passkey_sign_count(
    rp: &RelyingParty,
    record: &ChallengeRecord,
    assertion: &Assertion<'_>,
    credential: Option<&EnrolledCredential>,
    now: UtcNanos,
) -> Result<u32, StepUpRefusal> {
    let enrolled = credential
        .filter(|enrolled| enrolled.principal_id == record.principal_id)
        .ok_or(StepUpRefusal::Missing(Missing::NotTheirCredential))?;
    if now < enrolled.cool_off_ends {
        return Err(StepUpRefusal::Missing(Missing::CoolingOff));
    }
    let challenge = signed_challenge(record)?;
    verify(rp, &challenge, &enrolled.credential, assertion)
        .map(|verified| verified.sign_count)
        .map_err(|refusal| StepUpRefusal::Missing(Missing::Passkey(refusal)))
}

/// The record's WebAuthn challenge; a record with no canonical form has none to verify against,
/// so its step-up counts as missing.
fn signed_challenge(record: &ChallengeRecord) -> Result<Challenge, StepUpRefusal> {
    record
        .webauthn_challenge()
        .map_err(|_| StepUpRefusal::Missing(Missing::UnencodableChallenge))
}

/// Verifies a stored raw assertion again against the credential's public key, as an auditor does
/// (§7.2 step 5): everything [`verify`] checks except the counter, which has moved on since. An
/// assertion that no longer verifies counts as missing, with its reason, as in [`consume`].
pub fn reverify(
    rp: &RelyingParty,
    record: &ChallengeRecord,
    credential: &Credential,
    assertion: &Assertion<'_>,
) -> Result<Verified, StepUpRefusal> {
    let challenge = signed_challenge(record)?;
    let counter_not_kept = Credential {
        sign_count: 0,
        ..credential.clone()
    };
    verify(rp, &challenge, &counter_not_kept, assertion)
        .map_err(|refusal| StepUpRefusal::Missing(Missing::Passkey(refusal)))
}
