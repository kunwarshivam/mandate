//! Admission: the brief's checks 1 to 7, which decide `ApprovalResponded.result` (E8-2, E8-3;
//! DEC-156 items 1, 2, 6, and 7).

use std::collections::BTreeSet;

use crate::content::{ContentHash, RequestContent};
use crate::notify::ApprovalRef;
use crate::stepup::{AssertionId, Environment, StepUp, StepUpRefusal, judge_step_up};
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
/// A response's effective time is the later of its `submitted_at` and the folded clock (DEC-156
/// item 1): lateness and step-up freshness are both judged at it. A skip runs checks 1 to 5 only
/// and needs no step-up or quorum (PX-7).
///
/// # Errors
/// None: every input has an answer. The `Result` is the stub API's shape.
pub fn admit(
    pending: Option<&Request>,
    response: &Response,
    ctx: &AdmissionContext,
) -> Result<Admission, ApprovalError> {
    let Some(request) = pending.filter(|r| r.id == response.approval) else {
        return Ok(Admission::Refused(Refusal::NotPending));
    };
    let effective = response.submitted_at.max(ctx.folded_clock);
    let refusal = if effective >= request.content.deadline {
        Some(Refusal::Late)
    } else if response.actor_kind != ActorKind::User || !ctx.approvers.contains(&response.responder)
    {
        Some(Refusal::NotAnApprover)
    } else if !request.delivered {
        Some(Refusal::NotDelivered)
    } else if response.content_hash != request.content_hash {
        Some(Refusal::ContentMismatch)
    } else {
        None
    };
    if let Some(refusal) = refusal {
        return Ok(Admission::Refused(refusal));
    }
    let evidence = match &response.verdict {
        Verdict::Skip => return Ok(Admission::Admitted),
        Verdict::Approve(evidence) => evidence.as_ref(),
    };
    if let Some(why) = judge_step_up(evidence, effective, ctx.environment, &ctx.used_assertions) {
        return Ok(Admission::Refused(match why {
            StepUpRefusal::Missing => Refusal::StepUpMissing,
            StepUpRefusal::Stale => Refusal::StepUpStale,
            StepUpRefusal::Reused => Refusal::StepUpReused,
            StepUpRefusal::Method => Refusal::StepUpMethod,
        }));
    }
    Ok(quorum(request, response, ctx))
}

/// Check 7: the responder joins the grant set, once, and not as the author of a version that
/// needs independent approval.
fn quorum(request: &Request, response: &Response, ctx: &AdmissionContext) -> Admission {
    let bound = &request.content.bound;
    if request.grants.contains(&response.responder) {
        return Admission::Refused(Refusal::DuplicateApprover);
    }
    if bound.independent_required && response.responder == ctx.author {
        return Admission::Refused(Refusal::NotIndependent);
    }
    let mut grants = request.grants.clone();
    grants.insert(response.responder.clone());
    if grants.len() >= usize::from(bound.approvers_required.get()) {
        Admission::Admitted
    } else {
        Admission::Counted
    }
}
