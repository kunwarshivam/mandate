//! Quiet hours govern push channels only (DEC-156 item 6). `cli_inbox` is a pull channel: the
//! inbox is the journal, so it is delivered in the request's own batch and never suppressed.

use mandate_time::{NewYorkTime, UtcNanos, new_york_date_and_hour};

use crate::{ApprovalError, RiskClock};

/// `notifications.quiet_hours`, in America/New_York wall time. A window whose end is before its
/// start spans midnight.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietHours {
    pub start: NewYorkTime,
    pub end: NewYorkTime,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Channel {
    CliInbox,
    /// Email, chat, web push: E8-4.
    Push,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    Send,
    SuppressedQuietHours,
}

/// Whether `channel` delivers at `at`: `cli_inbox` always; a push unless `at` falls in
/// `[start, end)` of the quiet hours in America/New_York wall time (DEC-165 item 7).
///
/// # Errors
/// [`ApprovalError::Unrepresentable`] for a clock outside `UtcNanos`'s range.
pub fn deliver_now(
    channel: Channel,
    quiet_hours: Option<QuietHours>,
    at: RiskClock,
) -> Result<Delivery, ApprovalError> {
    let (Channel::Push, Some(window)) = (channel, quiet_hours) else {
        return Ok(Delivery::Send);
    };
    let start = minute_of(window.start);
    let since_start = minutes_after(start, new_york_minute(at)?);
    let length = minutes_after(start, minute_of(window.end));
    Ok(if since_start < length {
        Delivery::SuppressedQuietHours
    } else {
        Delivery::Send
    })
}

const MINUTES_PER_DAY: u16 = 1_440;

/// Minutes from `from` forward to `to` on a 24-hour clock face, so a window that spans midnight
/// is measured like any other and a window whose end equals its start is empty.
fn minutes_after(from: u16, to: u16) -> u16 {
    to.saturating_add(MINUTES_PER_DAY)
        .saturating_sub(from)
        .checked_rem(MINUTES_PER_DAY)
        .unwrap_or(0)
}

fn minute_of(time: NewYorkTime) -> u16 {
    u16::from(time.hour())
        .saturating_mul(60)
        .saturating_add(u16::from(time.minute()))
}

/// The America/New_York minute of the day. New York's offsets are whole hours, so the minute
/// within the hour is UTC's.
fn new_york_minute(at: RiskClock) -> Result<u16, ApprovalError> {
    let unrepresentable = |_| ApprovalError::Unrepresentable { what: "clock" };
    let instant = UtcNanos::from_parts(at.0, 0).map_err(unrepresentable)?;
    let (_, hour) = new_york_date_and_hour(instant).map_err(unrepresentable)?;
    let minute =
        at.0.rem_euclid(3_600)
            .checked_div(60)
            .and_then(|m| u16::try_from(m).ok())
            .ok_or(ApprovalError::Unrepresentable { what: "clock" })?;
    Ok(u16::from(hour).saturating_mul(60).saturating_add(minute))
}
