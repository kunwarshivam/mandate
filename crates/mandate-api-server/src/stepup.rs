//! The step-up port (§3.6, §3.11 items 11 and 12, [DEC-2100](../../../docs/project/decisions/DEC-2100.md)
//! item 2): the one place the server judges an assertion bound to an action's kind and digest.
//!
//! Until E9-4 binds a verifier, [`Unbound`] verifies nothing, so every call that needs step-up is
//! refused and no unverified assertion is ever recorded as step-up (DEC-1007 item 3). A call that
//! carries no evidence never reaches the port: its caller answers `step_up_required` first (§3.5).

use mandate_api::envelope::StepUpEvidence;
use mandate_api::problem::ProblemCode;
use mandate_canon::Digest;
use mandate_identity::{PrincipalId, StepUpActionKind};
use mandate_time::UtcNanos;

/// The one action a step-up assertion must be bound to (§3.6, API-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    /// §3.6's kind.
    pub kind: StepUpActionKind,
    /// The SHA-256 of the action's canonical object.
    pub digest: Digest,
}

/// Judges evidence a call carries for one action.
pub trait StepUp: Send + Sync {
    /// `Ok` when `evidence` is a valid, unused assertion by `principal`, bound to `action`, at
    /// `now`; else the §3.5 code of the first check that fails, in identity spec §7.2 step 4's
    /// order. The journal records `evidence` exactly as presented, so a verifier accepts only
    /// evidence it would have the journal hold.
    ///
    /// # Errors
    /// One of the five `step_up_*` codes but `step_up_required`.
    fn verify(
        &self,
        evidence: &StepUpEvidence,
        action: &Action,
        principal: PrincipalId,
        now: UtcNanos,
    ) -> Result<(), ProblemCode>;
}

/// The port before E9-4 binds it: no evidence counts (DEC-2100 item 2).
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbound;

impl StepUp for Unbound {
    fn verify(
        &self,
        _evidence: &StepUpEvidence,
        _action: &Action,
        _principal: PrincipalId,
        _now: UtcNanos,
    ) -> Result<(), ProblemCode> {
        Err(ProblemCode::StepUpMissing)
    }
}
