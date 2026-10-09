//! The dispatcher's delivery policy (notifications spec §3.4, §5.3, §5.4, NT-6; E8-10 slice D1,
//! DEC-701 item 1, DEC-725 items 5 to 7): the coalescing fold, the retry schedule, and one notice
//! per cause. Every instant is the caller's.

use mandate_time::UtcNanos;

use crate::{Class, NotifyError, Reason};

/// One cause as the dispatcher's pass reads it, for one recipient and push channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read<C> {
    pub cause: C,
    pub class: Class,
    /// The cause's journal commit time.
    pub committed: UtcNanos,
}

/// One read pass: its watermark `at` on the dispatcher's clock, and the causes it read, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pass<C> {
    pub at: UtcNanos,
    pub reads: Vec<Read<C>>,
}

/// One message to send at `send_at`. Its first cause's notice id is the message's; every other
/// cause is journaled `coalesced_into` it (§5.4, §5.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Message<C> {
    pub send_at: UtcNanos,
    pub causes: Vec<C>,
}

/// §5.4's fold for one recipient and push channel: every message the passes produce, by `send_at`,
/// a window's message before a same-instant pass's own (DEC-725 item 5). A `safety` cause is
/// never dropped; an `action` or `info` cause goes alone at its pass.
///
/// # Errors
/// [`NotifyError::Unrepresentable`] for a pass whose `at` is before the previous pass's.
pub fn coalesce<C: Clone>(passes: &[Pass<C>]) -> Result<Vec<Message<C>>, NotifyError> {
    let _ = passes;
    Err(NotifyError::Unimplemented { story: "E8-10" })
}

/// What follows a failed attempt (§5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    At(UtcNanos),
    /// A reason that is not retryable: journaled `failed` with it, and that channel stops.
    Failed(Reason),
    /// The class's window has no room for the next attempt: journaled `abandoned`.
    RetryWindowEnded,
}

/// The next attempt after `attempts` attempts, the first at `first` and the latest at `last`, the
/// latest failing with `reason` (§5.3, DEC-725 item 6). An `action` notice has no window; its
/// `not_pending` stop is the caller's, from the approval.
///
/// # Errors
/// [`NotifyError::Unrepresentable`] for zero attempts or an instant out of range.
pub fn next_attempt(
    class: Class,
    first: UtcNanos,
    last: UtcNanos,
    attempts: u32,
    reason: Reason,
) -> Result<Retry, NotifyError> {
    let _ = (class, first, last, attempts, reason);
    Err(NotifyError::Unimplemented { story: "E8-10" })
}

/// A committed `OwnerAlertSent` as the dispatcher reads it: its event id, and for a user's kill
/// switch the `OwnerCommandIssued` it carries out (§3.4, DEC-720).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert<C> {
    pub event: C,
    pub owner_command: Option<C>,
}

/// The notice keys `alerts` issue, one per cause in first-read order (§3.4, DEC-725 item 7).
///
/// # Errors
/// Never, once implemented.
pub fn notice_keys<C: Clone + Eq>(alerts: &[Alert<C>]) -> Result<Vec<C>, NotifyError> {
    let _ = alerts;
    Err(NotifyError::Unimplemented { story: "E8-10" })
}
