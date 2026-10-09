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
/// The verdict is for push channels only: NT-7 leaves pull channels untouched, so the caller
/// applies it to push and always delivers to pull channels. The function takes no channel.
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
    let Some(window) = window.filter(|_| class != Class::Safety) else {
        return Ok(QuietVerdict::Send);
    };
    let free = first_outside(window, at, offsets)?;
    Ok(if free == at {
        QuietVerdict::Send
    } else if class == Class::Action {
        QuietVerdict::Suppress
    } else {
        QuietVerdict::HoldUntil(free)
    })
}

const MINUTES_PER_DAY: i64 = 1_440;

fn unrepresentable<T>(value: Option<T>) -> Result<T, NotifyError> {
    value.ok_or(NotifyError::Unrepresentable {
        what: "quiet-hours instant",
    })
}

fn minute_of_day(time: NewYorkTime) -> i64 {
    i64::from(time.hour())
        .saturating_mul(60)
        .saturating_add(i64::from(time.minute()))
}

/// The offset in force at `at`, in minutes east: the entry with the latest `from` at or before
/// it, the later in `offsets` of two with the same `from` (DEC-725 item 2).
fn offset_at(offsets: &[Offset], at: UtcNanos) -> Result<i64, NotifyError> {
    let in_force = offsets.iter().filter(|offset| offset.from <= at).fold(
        None,
        |best: Option<&Offset>, offset| match best {
            Some(best) if best.from > offset.from => Some(best),
            _ => Some(offset),
        },
    );
    unrepresentable(in_force.map(|offset| i64::from(offset.minutes_east)))
}

/// The earliest `from` strictly after `at`: the next instant the offset in force may change.
fn next_change(offsets: &[Offset], at: UtcNanos) -> Option<UtcNanos> {
    offsets
        .iter()
        .map(|offset| offset.from)
        .filter(|from| *from > at)
        .min()
}

/// The first instant at or after `at` whose wall minute is outside `window` (DEC-725 item 3).
/// Under one offset the wall minute changes only on a UTC minute, so each step goes to the
/// window's end under the offset in force, or to the next offset change if that comes first. A
/// probe outside the window is its own end, so it is the answer.
/// Each step passes one change, so the walk takes at most one step per entry of `offsets`.
fn first_outside(
    window: QuietHours,
    at: UtcNanos,
    offsets: &[Offset],
) -> Result<UtcNanos, NotifyError> {
    let start = minute_of_day(window.start);
    let length = unrepresentable(
        minute_of_day(window.end)
            .checked_sub(start)
            .and_then(|span| span.checked_rem_euclid(MINUTES_PER_DAY)),
    )?;
    let mut probe = at;
    for _ in 0..=offsets.len() {
        let east = offset_at(offsets, probe)?;
        let utc_minute = unrepresentable(probe.secs().checked_div_euclid(60))?;
        let into = unrepresentable(
            utc_minute
                .checked_add(east)
                .and_then(|wall| wall.checked_sub(start))
                .and_then(|wall| wall.checked_rem_euclid(MINUTES_PER_DAY)),
        )?;
        let ends = unrepresentable(
            length
                .checked_sub(into)
                .and_then(|left| utc_minute.checked_add(left.max(0)))
                .and_then(|minute| minute.checked_mul(60)),
        )?;
        let ends = unrepresentable(UtcNanos::from_parts(ends, 0).ok())?.max(probe);
        match next_change(offsets, probe) {
            Some(change) if change <= ends => probe = change,
            _ => return Ok(ends),
        }
    }
    unrepresentable(None)
}
