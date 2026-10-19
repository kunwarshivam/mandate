//! The step-up port (§3.6, §3.11 items 11 and 12, [DEC-2101](../../../docs/project/decisions/DEC-2101.md)
//! item 2): the one place the server judges an assertion bound to an action's kind and digest.
//!
//! Until E9-4 binds a verifier, [`Unbound`] verifies nothing, so every call that needs step-up is
//! refused and no unverified assertion is ever recorded as step-up (DEC-1007 item 3). A call that
//! carries no evidence never reaches the port: its caller answers `step_up_required` first (§3.5).

use mandate_api::envelope::StepUpEvidence;
use mandate_api::problem::ProblemCode;
use mandate_api::wire::encode;
use mandate_canon::{Digest, Value, parse};
use mandate_identity::{StepUpActionKind, TenantContext};
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

/// The one action a step-up assertion must be bound to (§3.6, API-17).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Action {
    /// §3.6's kind.
    pub kind: StepUpActionKind,
    /// The SHA-256 of the action's canonical object.
    pub digest: Digest,
}

/// What a call's evidence is judged under: the context `authorize` returned for the request
/// (identity spec ID-8), whose principal and workspace the evidence must be bound to; the
/// workspace's control stream as the request read it; and the server's clock at receipt.
#[derive(Debug, Clone, Copy)]
pub struct Judged<'a> {
    pub tenant: &'a TenantContext,
    pub control: &'a [StoredEvent],
    pub now: UtcNanos,
}

/// Judges evidence a call carries for one action.
pub trait StepUp: Send + Sync {
    /// `Ok` when `evidence` is a valid, unused assertion by the request's principal, bound to
    /// `action` in the request's workspace, at `judged.now`; else the §3.5 code of the first check
    /// that fails, in identity spec §7.2 step 4's order. The journal records `evidence` exactly as
    /// presented, so a verifier accepts only evidence it would have the journal hold.
    ///
    /// # Errors
    /// One of the five `step_up_*` codes but `step_up_required`.
    fn verify(
        &self,
        evidence: &StepUpEvidence,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<(), ProblemCode>;
}

/// The port before E9-4 binds it: no evidence counts (DEC-2101 item 2).
#[derive(Debug, Clone, Copy, Default)]
pub struct Unbound;

impl StepUp for Unbound {
    fn verify(
        &self,
        _evidence: &StepUpEvidence,
        _action: &Action,
        _judged: &Judged<'_>,
    ) -> Result<(), ProblemCode> {
        Err(ProblemCode::StepUpMissing)
    }
}

/// Step-up evidence as the journal records it (journal spec §9.2), or `null` without any; `None`
/// only for evidence the canonical form cannot hold.
pub fn journaled(evidence: Option<&StepUpEvidence>) -> Option<Value> {
    match evidence {
        Some(evidence) => parse(&encode(evidence).ok()?).ok(),
        None => Some(Value::Null),
    }
}
