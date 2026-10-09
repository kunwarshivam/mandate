//! Action-bound step-up ([identity spec §7](../../../docs/specs/identity.md#7-step-up); backlog
//! E9-4, [DEC-662]): a challenge record bound to one principal, one workspace, and one action; the
//! WebAuthn challenge its canonical form hashes to; and the consume step that judges a presented
//! step-up in §7.2 step 4's order and returns the evidence and the used marker together, so the
//! caller commits both in the transaction that commits the action (§7.2 step 5).
//!
//! [DEC-662]: ../../../docs/project/decisions/DEC-662.md

use mandate_canon::Digest;
use mandate_time::UtcNanos;

use crate::{Assertion, Challenge, Credential, Refusal, RelyingParty, Verified};
use mandate_identity::{
    AssertionId, PrincipalId, StepUpActionKind, StepUpEvidence, UlidTextError, WorkspaceId,
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
    /// The stubs of E9-4's tests PR return this, so every pending test fails on them (DEC-77,
    /// DEC-137); the implementation PR replaces the stubs and removes the variant.
    #[error("step-up is not implemented yet")]
    Unimplemented,
}

/// Why a challenge record has no canonical form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CanonicalError {
    /// An identifier has no ULID text.
    #[error(transparent)]
    Ulid(#[from] UlidTextError),
    /// The stubs of E9-4's tests PR return this, so every pending test fails on them (DEC-77,
    /// DEC-137); the implementation PR replaces the stubs and removes the variant.
    #[error("step-up is not implemented yet")]
    Unimplemented,
}

/// What workspace services issue for one step-up (§7.2 step 1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChallengeRecord {
    pub challenge_id: AssertionId,
    pub workspace_id: WorkspaceId,
    pub principal_id: PrincipalId,
    pub action: Action,
    pub issued_at: UtcNanos,
    pub expires_at: UtcNanos,
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
        let _ = (challenge_id, workspace_id, principal_id, action, issued_at);
        Err(IssueError::Unimplemented)
    }

    /// The record's canonical form (journal spec §4): an object of seven text members, the
    /// digest as `sha256:` and its lowercase hex, the instants in the journal's timestamp form.
    pub fn canonical(&self) -> Result<Vec<u8>, CanonicalError> {
        let _ = self;
        Err(CanonicalError::Unimplemented)
    }

    /// The WebAuthn challenge: SHA-256 of [`ChallengeRecord::canonical`] (§7.2 step 2), so the
    /// authenticator signs the action itself.
    pub fn webauthn_challenge(&self) -> Result<Challenge, CanonicalError> {
        let _ = self;
        Err(CanonicalError::Unimplemented)
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
/// commits the action's event with [`Consumed::evidence`] (§7.2 step 5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Used {
    pub challenge_id: AssertionId,
}

/// A step-up that passed: the evidence to record, the challenge to mark used, and for a passkey
/// the signature counter to store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumed {
    pub evidence: StepUpEvidence,
    pub used: Used,
    pub sign_count: Option<u32>,
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
    /// The stubs of E9-4's tests PR return this (DEC-77, DEC-137); the implementation PR
    /// removes it.
    #[error("step-up is not implemented yet")]
    Unimplemented,
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
            Self::Unimplemented => "unimplemented",
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
    let _ = (rp, environment, challenge, presented, now);
    Err(StepUpRefusal::Unimplemented)
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
    let _ = (rp, record, credential, assertion);
    Err(StepUpRefusal::Unimplemented)
}
