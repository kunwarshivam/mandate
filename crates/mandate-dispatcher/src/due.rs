//! Which sends are due, read from the notice stream alone (notifications spec §5.1 step 4, §5.3;
//! journal spec §9.15 Lifecycle; E8-10 slice D4, DEC-706, DEC-707, DEC-710 item 5, DEC-725 item
//! 6). [`due`] keeps no state: every answer derives from the committed `NoticeIssued` and
//! `NoticeAttempted` records and the configured audience, so a restarted dispatcher reads the same
//! schedule as the one that stopped.

use mandate_journal::StoredEvent;
use mandate_notify::{IdempotencyKey, NoticeId, PushChannel, Recipient};
use mandate_time::UtcNanos;

use crate::DispatchError;

/// One send that is not yet finished: a notice to one recipient on one push channel, and what is
/// next for it. A send whose last attempt is terminal (`delivered`, `suppressed_quiet_hours`,
/// `abandoned`, or `failed` with the verdict `permanent`; a stored version-1 `failed` reads as
/// `permanent`, DEC-706 item 6) is not listed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Due {
    pub notice: NoticeId,
    pub recipient: Recipient,
    pub channel: PushChannel,
    pub next: Next,
}

/// What is next for one send at the instant [`due`] was asked about (spec §5.3, DEC-725 item 6,
/// DEC-707).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Next {
    /// Make attempt `attempt` now under `key`, the one key every attempt of this send carries
    /// (spec §5.1, DEC-710 item 5). Attempt 1 is due at once; attempt `n + 1` is due at attempt
    /// `n`'s `event_time` plus 15 s, 60 s, 300 s, then 900 s, and is made at any step at or after
    /// that instant when that instant is not after the class's window end.
    Attempt { attempt: u32, key: IdempotencyKey },
    /// Journal attempt `attempt` `abandoned` with reason `retry_window_ended`: the next attempt
    /// would fall after the window's end (`safety` 24 hours, `info` 6 hours from attempt 1's
    /// `event_time`), and that end has come.
    Abandon { attempt: u32 },
    /// Nothing for this send before `until`: the next attempt's instant, or the window's end when
    /// no attempt fits before it.
    Wait { until: UtcNanos },
}

/// The sends of the notice stream `rows` (its committed rows in `seq` order) that are not yet
/// finished at `now`, for every recipient a `NoticeIssued` lists and each of that recipient's
/// channels in `audience`, in the order of the notices, then the recipients, then the channels.
/// Each attempt is timed by its own record's `event_time`, never by `recorded_at` or by `now`
/// (DEC-707). An `action` notice has no window until its `not_pending` stop (slice D7), so it is
/// never [`Next::Abandon`] here (DEC-707). A notice whose kind pushes nothing has no send.
///
/// # Errors
/// [`DispatchError::Notify`] for a committed notice id, recipient, or channel that does not parse.
pub fn due(
    rows: &[StoredEvent],
    audience: &[(&str, PushChannel)],
    now: UtcNanos,
) -> Result<Vec<Due>, DispatchError> {
    let _ = (rows, audience, now);
    Err(DispatchError::Unimplemented { story: "E8-10" })
}

#[cfg(test)]
mod tests;
