//! The dispatcher's delivery policy (notifications spec §3.4, §5.3, §5.4, NT-6; E8-10 slice D1,
//! DEC-701 item 1, DEC-725 items 5 to 7): the coalescing fold, the retry schedule, and one notice
//! per cause. Every instant is the caller's.

use mandate_time::UtcNanos;

use crate::{Class, NotifyError, Outcome, Reason};

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
/// a window's message before a same-instant pass's own, and one pass's own in the read order of
/// their first causes (DEC-725 item 5). A `safety` cause is never dropped; an `action` or `info`
/// cause, or a stale `safety` one, goes alone at its pass.
///
/// # Errors
/// [`NotifyError::Unrepresentable`] for a pass whose `at` is before the previous pass's.
pub fn coalesce<C: Clone>(passes: &[Pass<C>]) -> Result<Vec<Message<C>>, NotifyError> {
    let mut messages = Vec::new();
    let mut open: Option<Window<C>> = None;
    let mut previous: Option<UtcNanos> = None;
    for pass in passes {
        if previous.is_some_and(|previous| pass.at < previous) {
            return Err(NotifyError::Unrepresentable {
                what: "pass before the previous pass",
            });
        }
        previous = Some(pass.at);
        if let Some(window) = open.take_if(|window| window.ends <= pass.at)
            && !window.joined.is_empty()
        {
            messages.push(Message {
                send_at: window.ends,
                causes: window.joined,
            });
        }
        let mut own: Vec<Message<C>> = Vec::new();
        let mut opening: Option<usize> = None;
        for read in &pass.reads {
            let bound = after(read.committed, COALESCING_WINDOW_SECS)?;
            let fresh_safety = read.class == Class::Safety && pass.at < bound;
            if let (true, Some(at)) = (fresh_safety, opening)
                && let Some(message) = own.get_mut(at)
            {
                message.causes.push(read.cause.clone());
            } else if let (true, Some(window)) = (fresh_safety, open.as_mut()) {
                window.ends = window.ends.min(bound);
                window.joined.push(read.cause.clone());
            } else {
                if fresh_safety {
                    opening = Some(own.len());
                    open = Some(Window {
                        ends: after(pass.at, COALESCING_WINDOW_SECS)?,
                        joined: Vec::new(),
                    });
                }
                own.push(Message {
                    send_at: pass.at,
                    causes: vec![read.cause.clone()],
                });
            }
        }
        messages.append(&mut own);
    }
    if let Some(window) = open.filter(|window| !window.joined.is_empty()) {
        messages.push(Message {
            send_at: window.ends,
            causes: window.joined,
        });
    }
    Ok(messages)
}

/// §5.4's bound: a window's message is sent by 60 seconds after its opening and after its earliest
/// joined commit, and a cause read 60 seconds or more after its commit is stale (DEC-725 item 5).
const COALESCING_WINDOW_SECS: i64 = 60;

/// An open window: the instant it ends, and the causes that joined it after its opening message.
struct Window<C> {
    ends: UtcNanos,
    joined: Vec<C>,
}

/// `secs` seconds after `at`.
fn after(at: UtcNanos, secs: i64) -> Result<UtcNanos, NotifyError> {
    at.secs()
        .checked_add(secs)
        .and_then(|later| UtcNanos::from_parts(later, at.nanos()).ok())
        .ok_or(NotifyError::Unrepresentable {
            what: "instant after the latest UtcNanos",
        })
}

/// What follows a failed attempt (§5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Retry {
    At(UtcNanos),
    /// A `permanent` result: journaled `failed` with its reason, and that channel stops.
    Failed(Reason),
    /// The class's window has no room for the next attempt: journaled `abandoned`.
    RetryWindowEnded,
}

/// The next attempt after `attempts` attempts, the first at `first` and the latest at `last`, the
/// latest answered `outcome` (§5.2, §5.3, DEC-725 item 6). The provider's verdict decides, not
/// its reason, and a `permanent` one stops before the window is read; a timeout with no answer is
/// the caller's `retryable { timeout }`. An `action` notice has no window; its `not_pending` stop
/// is the caller's, from the approval.
///
/// # Errors
/// [`NotifyError::Unrepresentable`] for zero attempts, an `accepted` outcome, or an instant out of
/// range.
pub fn next_attempt(
    class: Class,
    first: UtcNanos,
    last: UtcNanos,
    attempts: u32,
    outcome: &Outcome,
) -> Result<Retry, NotifyError> {
    if attempts == 0 {
        return Err(NotifyError::Unrepresentable {
            what: "retry after no attempt",
        });
    }
    match outcome {
        Outcome::Accepted { .. } => {
            return Err(NotifyError::Unrepresentable {
                what: "retry after an accepted attempt",
            });
        }
        Outcome::Permanent { reason } => return Ok(Retry::Failed(*reason)),
        Outcome::Retryable { .. } => {}
    }
    let gap = match attempts {
        1 => 15,
        2 => 60,
        3 => 300,
        _ => 900,
    };
    let due = after(last, gap)?;
    let window = match class {
        Class::Safety => Some(86_400),
        Class::Info => Some(21_600),
        Class::Action => None,
    };
    if let Some(window) = window
        && due > after(first, window)?
    {
        return Ok(Retry::RetryWindowEnded);
    }
    Ok(Retry::At(due))
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
/// Never.
pub fn notice_keys<C: Clone + Eq>(alerts: &[Alert<C>]) -> Result<Vec<C>, NotifyError> {
    let mut keys: Vec<C> = Vec::new();
    for alert in alerts {
        let key = alert.owner_command.as_ref().unwrap_or(&alert.event);
        if !keys.contains(key) {
            keys.push(key.clone());
        }
    }
    Ok(keys)
}
