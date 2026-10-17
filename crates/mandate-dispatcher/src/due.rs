//! Which sends are due, read from the notice stream alone (notifications spec §5.1 step 4, §5.3;
//! journal spec §9.15 Lifecycle; E8-10 slice D4, DEC-706, DEC-707, DEC-710 item 5, DEC-725 item
//! 6). [`due`] keeps no state: every answer derives from the committed `NoticeIssued` and
//! `NoticeAttempted` records and the configured audience, so a restarted dispatcher reads the same
//! schedule as the one that stopped.

use std::collections::BTreeSet;

use mandate_journal::StoredEvent;
use mandate_notify::{
    AddressHandle, Class, IdempotencyKey, NoticeId, NotifyError, PushChannel, Recipient, TextKey,
};
use mandate_time::UtcNanos;

use crate::DispatchError;
use crate::step::{Attempted, kind, notices_of};

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
    Ok(pending(rows, audience, now)?
        .into_iter()
        .map(|pending| pending.due)
        .collect())
}

/// One unfinished send as the step acts on it: what [`due`] lists, with the recipient as the
/// notice stream writes it and the text key its notice carries.
pub(crate) struct Pending {
    pub(crate) due: Due,
    pub(crate) recipient: String,
    pub(crate) text: TextKey,
}

/// [`due`]'s answer, each entry with what the step journals and sends it under.
pub(crate) fn pending(
    rows: &[StoredEvent],
    audience: &[(&str, PushChannel)],
    now: UtcNanos,
) -> Result<Vec<Pending>, DispatchError> {
    let read = notices_of(rows);
    let audience: BTreeSet<(&str, PushChannel)> = audience.iter().copied().collect();
    let mut listed = Vec::new();
    for issued in &read.issued {
        let Some(kind) = kind(&issued.kind) else {
            continue;
        };
        let Some(text) = kind.text_key()? else {
            continue;
        };
        let window = window(kind.class()?);
        let notice = NoticeId::parse(&issued.notice)?;
        for recipient in &issued.recipients {
            for &(_, channel) in audience.iter().filter(|(r, _)| r == recipient) {
                let channel_key = channel.key()?;
                let history: Vec<&Attempted> = read
                    .attempted
                    .iter()
                    .filter(|a| {
                        a.notice == issued.notice
                            && &a.recipient == recipient
                            && a.channel == channel_key
                    })
                    .collect();
                let Some(plan) = plan(&history, window, now)? else {
                    continue;
                };
                let address = AddressHandle {
                    recipient: Recipient::parse(recipient)?,
                    channel,
                };
                let next = match plan {
                    Plan::Attempt(attempt) => Next::Attempt {
                        attempt,
                        key: IdempotencyKey::of(&notice, &address)?,
                    },
                    Plan::Abandon(attempt) => Next::Abandon { attempt },
                    Plan::Wait(until) => Next::Wait { until },
                };
                listed.push(Pending {
                    due: Due {
                        notice,
                        recipient: address.recipient,
                        channel,
                        next,
                    },
                    recipient: recipient.clone(),
                    text,
                });
            }
        }
    }
    Ok(listed)
}

/// [`Next`] before the send's key is formed.
enum Plan {
    Attempt(u32),
    Abandon(u32),
    Wait(UtcNanos),
}

/// What is next for one send whose committed attempts, in `seq` order, are `history`, under the
/// class's `window` in seconds from attempt 1 (`None` for `action` until slice D7, DEC-707 item
/// 1); `None` when its last attempt is terminal.
fn plan(
    history: &[&Attempted],
    window: Option<i64>,
    now: UtcNanos,
) -> Result<Option<Plan>, DispatchError> {
    let (Some(first), Some(last)) = (history.first(), history.last()) else {
        return Ok(Some(Plan::Attempt(1)));
    };
    if terminal(last) {
        return Ok(None);
    }
    let made = number(last)?;
    let following = made.checked_add(1).ok_or(NotifyError::Unrepresentable {
        what: "next attempt",
    })?;
    let next_at = later(instant(last)?, gap(made))?;
    let end = match window {
        Some(window) => Some(later(instant(first)?, window)?),
        None => None,
    };
    Ok(Some(match end {
        Some(end) if next_at > end => {
            if now >= end {
                Plan::Abandon(following)
            } else {
                Plan::Wait(end)
            }
        }
        _ => {
            if now >= next_at {
                Plan::Attempt(following)
            } else {
                Plan::Wait(next_at)
            }
        }
    }))
}

/// Whether an attempt ends its send (journal spec §9.15 Lifecycle, DEC-706 items 5 and 6):
/// `delivered`, `suppressed_quiet_hours`, and `abandoned` do, and so does `failed` with any
/// verdict but `retryable`, a stored version-1 `failed`, which has none, included. Every other
/// status, `deferred_quiet_hours` among them, leaves the send due: until slice D5 times a deferred
/// send, it waits on this schedule like a `retryable` failure.
fn terminal(attempted: &Attempted) -> bool {
    match attempted.status.as_str() {
        "failed" => attempted.verdict.as_deref() != Some("retryable"),
        "delivered" | "suppressed_quiet_hours" | "abandoned" => true,
        _ => false,
    }
}

/// §5.3's gap in seconds after attempt `made`: 15 s, 60 s, 5 min, then 15 min.
fn gap(made: u32) -> i64 {
    match made {
        1 => 15,
        2 => 60,
        3 => 300,
        _ => 900,
    }
}

/// §5.3's window in seconds from attempt 1: `safety` 24 hours, `info` 6 hours, and none for
/// `action` until slice D7's `not_pending` stop (DEC-707 item 1).
fn window(class: Class) -> Option<i64> {
    match class {
        Class::Safety => Some(86_400),
        Class::Info => Some(21_600),
        Class::Action => None,
    }
}

/// An attempt's number.
fn number(attempted: &Attempted) -> Result<u32, DispatchError> {
    attempted
        .attempt
        .and_then(|n| u32::try_from(n).ok())
        .ok_or(DispatchError::Notify(NotifyError::Unrepresentable {
            what: "attempt",
        }))
}

/// An attempt's own `event_time`, never the journal's `recorded_at` (DEC-707 item 6).
fn instant(attempted: &Attempted) -> Result<UtcNanos, DispatchError> {
    UtcNanos::parse(&attempted.event_time)
        .map_err(|_| DispatchError::Notify(NotifyError::Unrepresentable { what: "event_time" }))
}

/// `secs` seconds after `at`.
fn later(at: UtcNanos, secs: i64) -> Result<UtcNanos, DispatchError> {
    at.secs()
        .checked_add(secs)
        .and_then(|later| UtcNanos::from_parts(later, at.nanos()).ok())
        .ok_or(DispatchError::Notify(NotifyError::Unrepresentable {
            what: "instant",
        }))
}

#[cfg(test)]
mod tests;
