//! Admission: the brief's checks 1 to 7, which decide `ApprovalResponded.result` (E8-2, E8-3;
//! DEC-156 items 1, 2, 6, and 7).

use std::collections::BTreeSet;

use crate::content::{ContentHash, RequestContent};
use crate::notify::ApprovalRef;
use crate::stepup::{AssertionId, Environment, StepUp};
use crate::{ApprovalError, RiskClock};

/// An owner's opaque user id (journal spec §6.4): no personal data.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OpaqueUser(pub String);

/// The envelope's `actor.kind` (journal spec §3). Only [`ActorKind::User`] can be admitted (EI-10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ActorKind {
    System,
    Agent,
    User,
    Broker,
    PlatformOperator,
}

/// A request as the runtime folds it: its content, its hash, whether any channel delivered it, and
/// the approvers whose grants were `counted` or `admitted` so far.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub id: ApprovalRef,
    pub content: RequestContent,
    pub content_hash: ContentHash,
    pub delivered: bool,
    pub grants: BTreeSet<OpaqueUser>,
}

/// What the owner answered. A grant carries its step-up evidence, or `None` when it has none,
/// which admission refuses; a skip never needs any (PX-7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    Approve(Option<StepUp>),
    Skip,
}

/// One `ApprovalResponseSubmitted` read from the workspace control stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    /// The control-stream `event_id`: the idempotency key (DEC-155 item 2).
    pub source: String,
    pub approval: ApprovalRef,
    pub actor_kind: ActorKind,
    pub responder: OpaqueUser,
    pub verdict: Verdict,
    pub content_hash: ContentHash,
    pub submitted_at: RiskClock,
}

/// What admission reads besides the request and the response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdmissionContext {
    /// The runtime's folded risk clock when it processes the response.
    pub folded_clock: RiskClock,
    /// `autonomy.approval.approvers` of the bound mandate version.
    pub approvers: BTreeSet<OpaqueUser>,
    /// The bound mandate version's author, whom independent approval excludes.
    pub author: OpaqueUser,
    pub environment: Environment,
    /// Every step-up assertion this workspace has already used (EI-11).
    pub used_assertions: BTreeSet<AssertionId>,
}

/// Why a response was refused. Not terminal: the approval stays pending until its deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refusal {
    NotPending,
    Late,
    NotAnApprover,
    NotDelivered,
    ContentMismatch,
    StepUpMissing,
    StepUpStale,
    StepUpReused,
    StepUpMethod,
    DuplicateApprover,
    NotIndependent,
}

/// `ApprovalResponded.result`, one variant per journaled value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    /// A skip, or a grant that reached the quorum; a grant goes on to [`crate::revalidate`].
    Admitted,
    /// A valid grant short of `approvers_required`: recorded, not terminal.
    Counted,
    Refused(Refusal),
}

/// Checks 1 to 7 in order; the first failure names the refusal. `pending` is the request as the
/// fold holds it after this step's own cancellations, or `None` when it is not pending (DEC-156
/// item 9).
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-3.
pub fn admit(
    pending: Option<&Request>,
    response: &Response,
    ctx: &AdmissionContext,
) -> Result<Admission, ApprovalError> {
    let _ = (pending, response, ctx);
    Err(ApprovalError::Unimplemented { story: "E8-3" })
}
