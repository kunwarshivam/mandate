//! Quiet hours by class (notifications spec §5.8, NT-7; E8-10 slice D1, DEC-701 item 1, DEC-725
//! items 2 to 4). The zone's UTC offsets are the caller's: no zone name is resolved and no tz
//! database is read here.

use mandate_time::{NewYorkTime, UtcNanos};

use crate::{Class, NotifyError};

/// `notifications.quiet_hours`: `[start, end)` in America/New_York wall time (§5.8). An end before
/// its start spans midnight; an end equal to its start is an empty window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QuietHours {
    pub start: NewYorkTime,
    pub end: NewYorkTime,
}

/// One UTC offset of America/New_York, in minutes east of UTC, in force from `from` until the
/// next entry's `from` (DEC-725 item 2). The caller resolves the zone; the policy reads none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Offset {
    pub from: UtcNanos,
    pub minutes_east: i16,
}

/// What quiet hours make of one push attempt (§5.8, NT-7).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuietVerdict {
    Send,
    /// An `action` push inside the window, journaled `suppressed_quiet_hours`; never deferred.
    Suppress,
    /// An `info` push inside the window, journaled `deferred_quiet_hours` and held until the
    /// first instant whose wall minute is outside the window (DEC-725 item 3).
    HoldUntil(UtcNanos),
}

/// Quiet hours for a `class` push attempted at `at`. A `safety` push is sent without reading the
/// window or `offsets`. The offset in force at an instant is the entry of `offsets` with the latest
/// `from` at or before it, the later in `offsets` of two with the same `from`.
///
/// # Errors
/// [`NotifyError::Unrepresentable`] when an `action` or `info` attempt needs an offset that no
/// entry covers, or an instant falls outside `UtcNanos`'s range.
pub fn quiet_hours(
    class: Class,
    window: Option<QuietHours>,
    at: UtcNanos,
    offsets: &[Offset],
) -> Result<QuietVerdict, NotifyError> {
    let _ = (class, window, at, offsets);
    Err(NotifyError::Unimplemented { story: "E8-10" })
}
