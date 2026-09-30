//! Admission: the brief's checks 1 to 7, which decide `ApprovalResponded.result` (E8-2, E8-3;
//! DEC-156 items 1, 2, 6, and 7).

use std::collections::BTreeSet;
use std::num::NonZeroU8;

use mandate_num::Usd;

use crate::content::{BoundAction, ContentHash, RequestContent};
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
    /// The workspace policy overlay current at the response's effective time, which check 7 reads
    /// beside the bound requirement (DEC-173 item 13).
    pub policy: PolicyOverlay,
}

/// The two workspace policy keys check 7 reads (mandate spec §4.3, §6.4), as every
/// `PolicyChanged` the runtime folded before the step leaves them. They are policy, not mandate
/// fields, so they can change while an approval is pending; check 7 takes the stricter of them and
/// the bound requirement, so a change only ever tightens it (DEC-173 item 13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyOverlay {
    /// `independent_approval_required`: the workspace's maker-checker requirement.
    pub independent_approval_required: bool,
    /// `two_approver_above_usd`: an order value above it needs two approvers. `None` when unset.
    pub two_approver_above_usd: Option<Usd>,
}

impl PolicyOverlay {
    /// No policy key set: check 7 reads the bound requirement alone.
    pub const NONE: Self = Self {
        independent_approval_required: false,
        two_approver_above_usd: None,
    };
}

/// Check 7's requirement, which `ApprovalResponded` records: how many distinct approvers admit a
/// grant, and whether the mandate's author is excluded from them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quorum {
    pub approvers_required: NonZeroU8,
    pub independent_required: bool,
}

/// Check 7's requirement: the stricter of what the request bound and `policy`. Independence is
/// required if either requires it, and the approver count is the larger of the bound one and the
/// overlay's, which is 2 when `two_approver_above_usd` is set and the bound order value exceeds
/// it, else 1 (mandate spec §6.4 check 7, DEC-173 item 13).
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until the overlay's part of check 7 lands (E8-3's follow-up);
/// [`ApprovalError::Unrepresentable`] once it has, for an order value that overflows.
pub fn quorum(_bound: &BoundAction, _policy: &PolicyOverlay) -> Result<Quorum, ApprovalError> {
    Err(ApprovalError::Unimplemented { story: "E8-3" })
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
/// [`quorum`]'s, for a grant that reaches check 7 under a policy overlay.
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
    check_7(request, response, ctx)
}

/// Check 7: the responder joins the grant set, once, and not as the author of a version that
/// needs independent approval.
///
/// While [`quorum`] is a stub, the overlay's part of the check is owed and fails closed, as a
/// partial gate does for adding risk (DEC-129 item 29): with no policy key set the bound
/// requirement is the whole answer, and any other overlay returns [`quorum`]'s `Unimplemented`,
/// so no grant is admitted on the bound requirement alone when the policy may be stricter.
fn check_7(
    request: &Request,
    response: &Response,
    ctx: &AdmissionContext,
) -> Result<Admission, ApprovalError> {
    let bound = &request.content.bound;
    let required = if ctx.policy == PolicyOverlay::NONE {
        Quorum {
            approvers_required: bound.approvers_required,
            independent_required: bound.independent_required,
        }
    } else {
        quorum(bound, &ctx.policy)?
    };
    if request.grants.contains(&response.responder) {
        return Ok(Admission::Refused(Refusal::DuplicateApprover));
    }
    if required.independent_required && response.responder == ctx.author {
        return Ok(Admission::Refused(Refusal::NotIndependent));
    }
    let mut grants = request.grants.clone();
    grants.insert(response.responder.clone());
    Ok(
        if grants.len() >= usize::from(required.approvers_required.get()) {
            Admission::Admitted
        } else {
            Admission::Counted
        },
    )
}
