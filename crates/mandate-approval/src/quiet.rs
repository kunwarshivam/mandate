//! Quiet hours govern push channels only (DEC-156 item 6). `cli_inbox` is a pull channel: the
//! inbox is the journal, so it is delivered in the request's own batch and never suppressed.

use mandate_time::{NewYorkTime, UtcNanos};

use crate::ApprovalError;

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

/// Whether `channel` delivers at `at`: the start of the window suppresses, its end sends.
///
/// # Errors
/// [`ApprovalError::Unimplemented`] until E8-1.
pub fn deliver_now(
    channel: Channel,
    quiet_hours: Option<QuietHours>,
    at: UtcNanos,
) -> Result<Delivery, ApprovalError> {
    let _ = (channel, quiet_hours, at);
    Err(ApprovalError::Unimplemented { story: "E8-1" })
}
