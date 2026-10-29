//! The step-up port's binding ([DEC-2126](../../../docs/project/decisions/DEC-2126.md) item 2,
//! [DEC-2108](../../../docs/project/decisions/DEC-2108.md) item 8): [`Methods`] dispatches each
//! evidence's `method` to its own verifier, `passkey` to [`Passkey`] and `cli_confirm` to
//! DEC-2104's [`CliConfirm`], so one binding serves every kind the challenge route issues.
//!
//! [`Passkey`] folds the member's passkeys from the control stream as the request read it, the
//! same `CredentialFold` the passkey routes read, and judges the assertion with
//! `mandate_passkey::stepup::consume` in its order, in `paper`. A stream whose credential records
//! cannot be read is `journal_unavailable`; a member whose records break rule 251 or 254, a
//! credential enrolled at version 1 (DEC-2122 item 3), and a key that does not resolve each leave
//! no credential to count, so the step-up is `step_up_missing`. Evidence of method `passkey` with
//! no assertion has nothing to judge and is `step_up_missing` too.

use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use mandate_api::envelope::{StepUpAssertion, StepUpEvidence, StepUpMethod};
use mandate_api::problem::ProblemCode;
use mandate_api::wire::encode;
use mandate_canon::{Value, parse, to_canonical};
use mandate_journal::{ArtifactRef, ArtifactSource, StoredEvent, get_artifact};
use mandate_passkey::credentials::{CredentialFold, Enrolment};
use mandate_passkey::reduction::cose_key;
use mandate_passkey::stepup::{
    self, ChallengeState, EnrolledCredential, Environment, Presentation, Proof, StepUpRefusal,
    consume,
};
use mandate_passkey::{Assertion, Credential, RelyingParty};
use mandate_time::UtcNanos;

use crate::challenges::Challenges;
use crate::confirm::CliConfirm;
use crate::members::credential_records;
use crate::stepup::{Action, Asserted, CoolOffWaived, Judged, StepUp, Unverified, journaled};

/// DEC-2108 item 8's passkey verifier.
pub struct Passkey {
    /// The challenges the server holds, from which a verified assertion's is taken.
    pub challenges: Arc<Challenges>,
    /// The deployment's relying party (DEC-2108 item 1).
    pub rp: RelyingParty,
    /// The store the passkeys' public keys are artifacts in; `None` when the deployment names
    /// none, and then no key resolves.
    pub keys: Option<Arc<dyn ArtifactSource + Send + Sync>>,
    /// The test cell's waiver of a first passkey's enrolment cool-off
    /// ([DEC-2146](../../../docs/project/decisions/DEC-2146.md)), set only by
    /// [`crate::config::cool_off_waiver`] at start; `None` everywhere else.
    pub cool_off_waiver: Option<CoolOffWaiver>,
    /// The waiver's expiry ([DEC-2150](../../../docs/project/decisions/DEC-2150.md) item 4), set
    /// only by [`crate::config::cool_off_waiver`] at start: no step-up judged at or after it is
    /// waived ([`CoolOffWaiving::at`]). `None` is no expiry, which only a `dev` or `ci` cell
    /// starts with.
    pub cool_off_waiver_until: Option<UtcNanos>,
}

/// The tier of a cell that waives a first passkey's enrolment cool-off: a `dev` or `ci` test cell
/// (DEC-2146 item 2), or a staging cell until its expiry (DEC-2150 item 1). Production never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoolOffWaiver {
    Dev,
    Ci,
    Staging,
}

impl CoolOffWaiver {
    /// The tier as `StepUpCoolOffWaived` records it (journal spec §9.38).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dev => "dev",
            Self::Ci => "ci",
            Self::Staging => "staging",
        }
    }
}

/// The assertion's four members, decoded from unpadded base64url.
struct Decoded {
    credential_id: Vec<u8>,
    authenticator_data: Vec<u8>,
    client_data_json: Vec<u8>,
    signature: Vec<u8>,
}

impl Decoded {
    fn of(assertion: &StepUpAssertion) -> Option<Self> {
        let bytes = |text: &str| URL_SAFE_NO_PAD.decode(text).ok();
        Some(Self {
            credential_id: bytes(&assertion.passkey_id)?,
            authenticator_data: bytes(&assertion.authenticator_data)?,
            client_data_json: bytes(&assertion.client_data_json)?,
            signature: bytes(&assertion.signature)?,
        })
    }

    fn assertion(&self) -> Assertion<'_> {
        Assertion {
            credential_id: &self.credential_id,
            authenticator_data: &self.authenticator_data,
            client_data_json: &self.client_data_json,
            signature: &self.signature,
        }
    }
}

/// The server's waiver of a first passkey's enrolment cool-off as one `Copy` value: its tier, and
/// its expiry when it has one (DEC-2146 item 2,
/// [DEC-2150](../../../docs/project/decisions/DEC-2150.md) items 2 and 4). The default waives
/// nothing. A verifier other than [`Passkey`] holds a copy, so every reader judges by one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CoolOffWaiving {
    pub tier: Option<CoolOffWaiver>,
    pub until: Option<UtcNanos>,
}

impl CoolOffWaiving {
    /// The waiving [`crate::config::cool_off_waiver`] answered at start: none, or the tier and its
    /// expiry.
    pub fn from_start(set: Option<(CoolOffWaiver, Option<UtcNanos>)>) -> Self {
        match set {
            Some((tier, until)) => Self {
                tier: Some(tier),
                until,
            },
            None => Self::default(),
        }
    }

    /// The waiver's tier when it holds at `now`: the server started with one, and `now` is before
    /// its expiry when it has one (DEC-2150 item 4). The expiry is judged against the verifier's
    /// own instant, so the clock can only end the waiver.
    pub fn at(self, now: UtcNanos) -> Option<CoolOffWaiver> {
        self.tier
            .filter(|_| self.until.is_none_or(|until| now < until))
    }

    /// Whether a credential whose enrolment names `cool_off_ends` is past its cool-off at `now`,
    /// the waiver included: it has no cool-off, its cool-off has ended, or the waiver holds at
    /// `now`. Step-up journals each waiver it counts only on a route that appends its assertion
    /// (DEC-2146 item 3); another reader of this rule records its own reading.
    pub fn past(self, cool_off_ends: Option<UtcNanos>, now: UtcNanos) -> bool {
        cool_off_ends.is_none_or(|ends| ends <= now) || self.at(now).is_some()
    }
}

impl Passkey {
    /// The waiving this verifier holds.
    pub fn waiving(&self) -> CoolOffWaiving {
        CoolOffWaiving {
            tier: self.cool_off_waiver,
            until: self.cool_off_waiver_until,
        }
    }

    /// This verifier holding `waiving`, its tier and its expiry together, so a caller cannot set
    /// one without the other.
    #[must_use]
    pub fn waived_by(mut self, waiving: CoolOffWaiving) -> Self {
        self.cool_off_waiver = waiving.tier;
        self.cool_off_waiver_until = waiving.until;
        self
    }

    /// The enrolled credential `enrolment` is, with its last counter and its key read from the
    /// store; `None` for a version 1 enrolment, which records no backup-eligible flag, for a
    /// holder the fold finds broken, and for a key that does not resolve or parse.
    fn credential(
        &self,
        fold: &CredentialFold,
        enrolment: &Enrolment,
        id: &[u8],
    ) -> Option<EnrolledCredential> {
        let backup_eligible = enrolment.backup_eligible?;
        let last_count = fold.last_count(&enrolment.credential).ok()??;
        let reference = ArtifactRef::parse(&enrolment.public_key)?;
        let bytes = get_artifact(self.keys.as_deref()?, &reference).ok()?;
        Some(EnrolledCredential {
            principal_id: enrolment.member,
            credential: Credential {
                id: id.to_vec(),
                public_key: cose_key(&bytes).ok()?,
                sign_count: last_count,
                backup_eligible,
            },
            cool_off_ends: enrolment.cool_off_ends.unwrap_or(UtcNanos::EPOCH),
        })
    }
}

impl StepUp for Passkey {
    /// Evidence alone carries no assertion to judge: `step_up_missing`.
    fn verify(
        &self,
        _evidence: &StepUpEvidence,
        _action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        Err(ProblemCode::StepUpMissing)
    }

    /// `consume`'s checks in order, then the evidence's `authenticated_at` against the challenge's
    /// `issued_at` (DEC-2108 item 7), then the challenge taken, so it verifies once.
    fn asserted(
        &self,
        evidence: &StepUpEvidence,
        assertion: Option<&StepUpAssertion>,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        let missing = || Unverified::Code(ProblemCode::StepUpMissing);
        let assertion = assertion.ok_or_else(missing)?;
        let presented = journaled(Some(evidence)).ok_or_else(missing)?;
        let text = |name: &str| presented.get(name).and_then(Value::as_str);
        let id = text("assertion_id").ok_or_else(missing)?;
        let authenticated = text("authenticated_at")
            .and_then(|at| UtcNanos::parse(at).ok())
            .ok_or_else(missing)?;
        let records = credential_records(judged.tenant.workspace(), judged.control)
            .map_err(|_| Unverified::Code(ProblemCode::JournalUnavailable))?;
        let fold = CredentialFold::new(records);
        let decoded = Decoded::of(assertion).ok_or_else(missing)?;
        let mut enrolled = fold
            .passkeys(judged.tenant.principal())
            .unwrap_or_default()
            .into_iter()
            .find(|enrolment| enrolment.credential == assertion.passkey_id)
            .and_then(|enrolment| self.credential(&fold, enrolment, &decoded.credential_id));
        let waived = match (self.waiving().at(judged.now), enrolled.as_mut()) {
            (Some(tier), Some(credential)) if judged.now < credential.cool_off_ends => {
                let waived = CoolOffWaived {
                    cool_off_ends: credential.cool_off_ends,
                    tier,
                };
                credential.cool_off_ends = judged.now;
                Some(waived)
            }
            _ => None,
        };
        let record = self.challenges.held(id);
        let state = match &record {
            None => ChallengeState::Unknown,
            Some(record) if spent(judged.control, id) => ChallengeState::Used(record),
            Some(record) => ChallengeState::Issued(record),
        };
        let presentation = Presentation::new(
            judged.tenant,
            stepup::Action {
                kind: action.kind,
                digest: action.digest,
            },
            Proof::Passkey {
                assertion: decoded.assertion(),
                credential: enrolled.as_ref(),
            },
        );
        let consumed = consume(
            &self.rp,
            Environment::Paper,
            state,
            &presentation,
            judged.now,
        )
        .map_err(|refusal| Unverified::Code(code(refusal)))?;
        if consumed.evidence().authenticated_at != authenticated {
            return Err(Unverified::Code(ProblemCode::StepUpStale));
        }
        if !self.challenges.take(id) {
            return Err(missing());
        }
        let canonical = encode(assertion)
            .ok()
            .and_then(|bytes| parse(&bytes).ok())
            .ok_or_else(missing)?;
        Ok(Some(
            Asserted::new(
                id.to_owned(),
                assertion.passkey_id.clone(),
                consumed.sign_count().unwrap_or_default(),
                to_canonical(&canonical),
            )
            .with_waiver(waived),
        ))
    }
}

/// Whether an event on `control` already carries step-up naming the challenge `id`, so the
/// challenge is used (API-17).
fn spent(control: &[StoredEvent], id: &str) -> bool {
    control.iter().any(|row| {
        parse(&row.body).ok().is_some_and(|event| {
            event
                .get("payload")
                .and_then(|payload| payload.get("step_up"))
                .and_then(|step_up| step_up.get("assertion_id"))
                .and_then(Value::as_str)
                == Some(id)
        })
    })
}

/// The §3.5 code `consume`'s refusal names (`StepUpRefusal::code`).
fn code(refusal: StepUpRefusal) -> ProblemCode {
    match refusal {
        StepUpRefusal::Missing(_) => ProblemCode::StepUpMissing,
        StepUpRefusal::Stale => ProblemCode::StepUpStale,
        StepUpRefusal::Reused => ProblemCode::StepUpReused,
        StepUpRefusal::Method => ProblemCode::StepUpMethod,
        StepUpRefusal::Mismatch => ProblemCode::StepUpMismatch,
    }
}

/// The binding the server's port holds: each method to its own verifier.
pub struct Methods {
    pub passkey: Passkey,
    pub cli_confirm: CliConfirm,
}

impl StepUp for Methods {
    fn verify(
        &self,
        evidence: &StepUpEvidence,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        match evidence.method {
            StepUpMethod::Passkey => self.passkey.verify(evidence, action, judged),
            StepUpMethod::CliConfirm => self.cli_confirm.verify(evidence, action, judged),
        }
    }

    fn asserted(
        &self,
        evidence: &StepUpEvidence,
        assertion: Option<&StepUpAssertion>,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        match evidence.method {
            StepUpMethod::Passkey => self.passkey.asserted(evidence, assertion, action, judged),
            StepUpMethod::CliConfirm => self
                .cli_confirm
                .asserted(evidence, assertion, action, judged),
        }
    }
}
