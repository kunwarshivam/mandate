//! The step-up port's binding ([DEC-2126](../../../docs/project/decisions/DEC-2126.md) item 2,
//! [DEC-2108](../../../docs/project/decisions/DEC-2108.md) item 8): [`Methods`] dispatches each
//! evidence's `method` to its own verifier, `passkey` to [`Passkey`] and `cli_confirm` to
//! DEC-2104's [`CliConfirm`], so one binding serves every kind the challenge route issues.
//!
//! [`Passkey`] takes the held challenge, folds the member's passkeys from the control stream as
//! the request read it, and judges the assertion with `mandate_passkey::stepup::consume` in its
//! order, in `paper`. Evidence of method `passkey` with no assertion has nothing to judge and is
//! `step_up_missing`.

use std::sync::Arc;

use mandate_api::envelope::{StepUpAssertion, StepUpEvidence, StepUpMethod};
use mandate_api::problem::ProblemCode;
use mandate_journal::ArtifactSource;
use mandate_passkey::RelyingParty;

use crate::challenges::Challenges;
use crate::confirm::CliConfirm;
use crate::stepup::{Action, Asserted, Judged, StepUp, Unverified};

/// The story the passkey binding's stubs report until it lands.
const STORY: &str = "E9-4";

/// DEC-2108 item 8's passkey verifier.
pub struct Passkey {
    /// The challenges the server holds, from which a verified assertion's is taken.
    pub challenges: Arc<Challenges>,
    /// The deployment's relying party (DEC-2108 item 1).
    pub rp: RelyingParty,
    /// The store the passkeys' public keys are artifacts in; `None` when the deployment names
    /// none, and then no key resolves.
    pub keys: Option<Arc<dyn ArtifactSource + Send + Sync>>,
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

    fn asserted(
        &self,
        _evidence: &StepUpEvidence,
        _assertion: Option<&StepUpAssertion>,
        _action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        Err(Unverified::Unimplemented { story: STORY })
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
        _evidence: &StepUpEvidence,
        _assertion: Option<&StepUpAssertion>,
        _action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        Err(Unverified::Unimplemented { story: STORY })
    }
}
