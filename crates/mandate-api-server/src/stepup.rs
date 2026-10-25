//! The step-up port (§3.6, §3.11 items 11 and 12, [DEC-2101](../../../docs/project/decisions/DEC-2101.md)
//! item 2): the one place the server judges an assertion bound to an action's kind and digest.
//!
//! Until E9-4 binds a verifier, [`Unbound`] verifies nothing, so every call that needs step-up is
//! refused and no unverified assertion is ever recorded as step-up (DEC-1007 item 3). A call that
//! carries no evidence never reaches the port: its caller answers `step_up_required` first (§3.5).

use mandate_api::envelope::{StepUpAssertion, StepUpEvidence};
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

/// What a passkey step-up leaves to append after the event it authorized (journal spec §9.32,
/// [DEC-2126](../../../docs/project/decisions/DEC-2126.md) item 2): the challenge, the credential
/// that signed, the counter its assertion carried, and the assertion's canonical bytes, which the
/// caller stores as the raw-assertion artifact. Only a verifier in this crate builds one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asserted {
    challenge: String,
    credential: String,
    sign_count: u32,
    assertion: Vec<u8>,
}

impl Asserted {
    pub(crate) fn new(
        challenge: String,
        credential: String,
        sign_count: u32,
        assertion: Vec<u8>,
    ) -> Self {
        Self {
            challenge,
            credential,
            sign_count,
            assertion,
        }
    }

    /// The challenge's ULID text, the authorized event's `step_up.assertion_id`.
    pub fn challenge(&self) -> &str {
        &self.challenge
    }

    /// The credential id, unpadded base64url, as §9.31 records it.
    pub fn credential(&self) -> &str {
        &self.credential
    }

    pub fn sign_count(&self) -> u32 {
        self.sign_count
    }

    /// The canonical object of the request's `step_up_assertion` (DEC-2108 item 7).
    pub fn assertion(&self) -> &[u8] {
        &self.assertion
    }
}

/// Why [`StepUp::asserted`] counted no step-up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unverified {
    /// The §3.5 code of the first check that failed, or `journal_unavailable` for a read that
    /// could not answer.
    Code(ProblemCode),
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

    /// [`StepUp::verify`] with the `step_up_assertion` the request carried beside `evidence`
    /// (DEC-2126 item 4): `Ok(None)` when the evidence counts and leaves nothing to append, and
    /// `Ok(Some)` with the `StepUpAsserted` to append after the event when a passkey assertion
    /// counts. A verifier that judges no assertion refuses any as `step_up_missing`, and otherwise
    /// answers as `verify` does.
    ///
    /// # Errors
    /// [`Unverified::Code`] with a `step_up_*` code but `step_up_required`, or
    /// `journal_unavailable`.
    fn asserted(
        &self,
        evidence: &StepUpEvidence,
        assertion: Option<&StepUpAssertion>,
        action: &Action,
        judged: &Judged<'_>,
    ) -> Result<Option<Asserted>, Unverified> {
        if assertion.is_some() {
            return Err(Unverified::Code(ProblemCode::StepUpMissing));
        }
        self.verify(evidence, action, judged)
            .map(|()| None)
            .map_err(Unverified::Code)
    }
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

#[cfg(test)]
mod asserted {
    //! A `StepUpAsserted` draft answers each member exactly as it was built: values none of the
    //! getters' defaults or stand-ins could give.

    use super::Asserted;

    #[test]
    fn each_getter_answers_what_the_draft_was_built_with() {
        let assertion = vec![0x7b, 0x22, 0x61, 0x22, 0x7d];
        let asserted = Asserted::new(
            "01J9ZQ4B7Y8K3M5N6P7Q8R9S0T".to_owned(),
            "AAECAwQFBgcICQoLDA0ODw".to_owned(),
            9,
            assertion.clone(),
        );
        assert_eq!(asserted.challenge(), "01J9ZQ4B7Y8K3M5N6P7Q8R9S0T");
        assert_eq!(asserted.credential(), "AAECAwQFBgcICQoLDA0ODw");
        assert_eq!(asserted.sign_count(), 9);
        assert_eq!(asserted.assertion(), assertion.as_slice());
    }
}
