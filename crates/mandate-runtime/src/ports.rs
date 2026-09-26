//! The injected collaborators, in two kinds. Only the pure ones can change what a step returns.

use crate::error::RuntimeError;
use crate::types::{
    DryRunVerdict, EventId, IntentHandoff, MandateView, Proposal, RiskClock, Seq, SignalInputs,
    TimerId, WriterEpoch,
};

/// Deterministic event identity (ADR-0001 ES-06, ES-21). The Phase 1 implementation derives the id
/// from `(epoch, head, ordinal)` rather than generating one, so a retry after `Unavailable` or
/// `Ambiguous` re-derives the same id and the append answers `AlreadyCommitted` (journal spec §5.1).
/// The epoch is in the derivation so a fenced writer's retry cannot collide with the new writer's
/// id at the same head, which would return `IdempotencyConflict` and hide the `Fenced` the old
/// process needs to see.
pub trait IdGen {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId;
}

/// `mandate-risk`'s gate, called as a dry run only. A verdict can narrow a proposal away; it can
/// never authorise one, because the binding gate runs on the account stream whose owner is the
/// executor (`AGENTS.md` rules 1 and 12).
pub trait GateDryRun {
    fn check(&self, proposal: &Proposal) -> DryRunVerdict;
}

/// `mandate-builder`'s sizing and classification over `mandate-spec`'s mandate view.
pub trait OrderPlan {
    fn plan(&self, view: &MandateView, inputs: &SignalInputs) -> Option<Proposal>;
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> crate::types::Autonomy;
}

/// The pure ports a step reads. Each is a function of its arguments, so `handle` stays
/// deterministic and a test injects fixed implementations.
pub struct Ports<'a> {
    pub ids: &'a dyn IdGen,
    pub gate: &'a dyn GateDryRun,
    pub plan: &'a dyn OrderPlan,
    pub view: &'a MandateView,
}

/// Where an intent goes, driven by the shell from `Effect::Intent`. The core never calls it: it only
/// describes the handoff, which is what keeps `handle` pure and the transport out of the core
/// (ADR-0001 ES-20, DEC-131 item 5).
pub trait IntentSink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<(), SinkError>;
}

/// Keyed timers, driven by the shell from `Effect::Timer`. A timer says when to look; the deadline
/// itself is folded state, so a lost timer costs a late evaluation and never a lost deadline.
pub trait TimerSource {
    fn arm(&mut self, id: TimerId, at: RiskClock);
    fn cancel(&mut self, id: TimerId);
}

/// Why a handoff did not reach the executor. The shell retries; the core is not told, because a
/// handoff that never arrives is re-derived from the fold at the next start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SinkError {
    #[error("the sink is unavailable")]
    Unavailable,
    #[error("the sink refused the handoff: {reason}")]
    Refused { reason: String },
}

impl From<SinkError> for RuntimeError {
    fn from(error: SinkError) -> Self {
        Self::NotInterpreted {
            what: error.to_string(),
            story: "E7-2",
        }
    }
}
