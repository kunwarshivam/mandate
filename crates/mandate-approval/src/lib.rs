#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The pure core of the approval flow ([task brief](../../../docs/project/tasks/M7-escalation-v0.md),
//! backlog E8-1 to E8-3, DEC-155, DEC-156, DEC-158, DEC-165).
//!
//! **It compares values; it decides no limit.** The crate sits at layer 1 and depends on layer 0
//! only, so it cannot reach `mandate-spec` or `mandate-risk`: every limit comparison stays in the
//! gate, and what arrives here is the gate's verdict, the builder's classification, and the folded
//! mark as plain values ([`Current`]). That boundary is rung 1 of the trust ladder (DEC-155 item 1).
//!
//! **Every outcome only ever skips.** [`admit`] runs the brief's checks 1 to 7 and [`revalidate`]
//! checks 8 to 12, in that order, and the first failure names the outcome. [`Admission`] and
//! [`Revalidation`] have exactly the variants `ApprovalResponded.result` and
//! `ApprovalRevalidated.result` journal, and the one variant that acts, [`Revalidation::Act`],
//! carries a [`GrantedOrder`] that can only be the bound order (EI-4).
//!
//! **Only a kill switch is never refused.** [`kill_switch`] returns a [`KillSwitchAuthority`], which
//! has no refusing variant: without valid step-up the switch still stops and flattens as an
//! automated flatten does (DEC-158 option (c), `AGENTS.md` rule 13).
//!
//! Every entry point is pure: no clock, no randomness, no I/O, ordered collections only (ES-21).
//! In this tests PR every entry point returns [`ApprovalError::Unimplemented`] (DEC-77).

mod admit;
mod budget;
mod content;
mod drift;
mod notify;
mod quiet;
mod revalidate;
mod stepup;

pub use admit::{
    ActorKind, Admission, AdmissionContext, OpaqueUser, Refusal, Request, Response, Verdict, admit,
};
pub use budget::{
    ASK_BUDGET_PER_RISK_DAY, AskEvent, AskLedger, AskPermit, Suppression, ask_permit,
};
pub use content::{
    AskablePurpose, AssetClass, BoundAction, ConfirmationCode, ContentHash, EvidenceAuthor,
    EvidenceRef, ReferenceMark, RequestContent, RiskField, RiskFigure, confirmation_code,
    content_hash, content_object,
};
pub use drift::{band_bp, within_band};
pub use notify::{ApprovalRef, GenericText, Notification, notification_for, notification_payload};
pub use quiet::{Channel, Delivery, QuietHours, deliver_now};
pub use revalidate::{
    Classification, Current, DryRun, GrantedOrder, ModeNow, Revalidation, SkipReason, revalidate,
};
pub use stepup::{
    AssertionId, CommandAuthority, Environment, KillScope, KillSwitchAuthority, OwnerCommandKind,
    STEP_UP_WINDOW_S, StepUp, StepUpMethod, StepUpRefusal, kill_switch, kill_switch_code,
    owner_command,
};

/// The runtime's whole-second risk clock (mandate spec §5.2), as seconds since the Unix epoch. The
/// runtime converts its own `RiskClock` into this one; nothing here reads a wall clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RiskClock(pub i64);

/// Every way an entry point can refuse to answer.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApprovalError {
    /// The body of every stub in the tests PR (DEC-77, DEC-83).
    #[error("{story} has not been implemented yet")]
    Unimplemented { story: &'static str },
}
