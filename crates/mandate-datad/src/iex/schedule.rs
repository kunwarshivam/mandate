//! When a generation is due (DEC-2283 items 4 and 5, data-plane spec §3.8 "When"), from the
//! checked-in calendar and the clock the binary hands in, never a guess from silence.
//!
//! - A daily generation of session `D` is due once `D`'s last calendar span, extended hours
//!   included, has ended plus [`SETTLE_S`] and the workspace's daily offset: a bar still forming is
//!   never stored. `D` is the clock's last completed regular session, the one readers name.
//! - A minute generation of the whole minute `M` is due during the regular session, once the clock
//!   is the workspace's minute offset past `M`.

use mandate_time::{Date, ExchangeCalendar, Session, TimeError, UtcNanos, new_york_midnight};

/// How long after a session's last span ends its daily bars are taken as settled.
pub const SETTLE_S: i64 = 900;
/// How long an instrument whose newest session the vendor answered with no bar waits before that
/// empty day is stored.
pub const CONFIRM_S: i64 = 7_200;
/// How often such an instrument is asked again while it waits.
pub const CONFIRM_EVERY_S: i64 = 300;
/// The bound of a workspace's offset after the settle time.
pub const DAILY_JITTER_S: i64 = 300;
/// The bound of a workspace's offset into a minute.
pub const MINUTE_JITTER_S: i64 = 20;
/// The regular sessions a daily generation spans, ending on its own.
pub const SESSIONS: usize = 30;
const MINUTE_S: i64 = 60;
const DAY_S: i64 = 86_400;

/// A workspace's fixed offsets, so workspaces sharing the platform key do not all ask at once. An
/// offset never moves a generation's span.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Jitter {
    pub daily_s: i64,
    pub minute_s: i64,
}

impl Jitter {
    /// The offsets of `workspace`: an FNV-1a hash of its id, reduced into each bound.
    pub fn of(workspace: &str) -> Self {
        let hash = workspace
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
            });
        let within = |bound: i64, shift: u32| {
            let bound = u64::try_from(bound).unwrap_or(1).max(1);
            hash.checked_shr(shift)
                .and_then(|bits| bits.checked_rem(bound))
                .and_then(|offset| i64::try_from(offset).ok())
                .unwrap_or(0)
        };
        Self {
            daily_s: within(DAILY_JITTER_S, 0),
            minute_s: within(MINUTE_JITTER_S, 32),
        }
    }
}

/// The session whose daily generation is due at `now`, if any.
///
/// # Errors
/// The calendar's, for a clock outside it.
pub fn daily_target(
    calendar: &ExchangeCalendar,
    now: UtcNanos,
    jitter: Jitter,
) -> Result<Option<Date>, TimeError> {
    let Some(session) = calendar.last_completed_regular_session(now)? else {
        return Ok(None);
    };
    let Some(last) = calendar.sessions(session)?.last().copied() else {
        return Ok(None);
    };
    let due = SETTLE_S
        .checked_add(jitter.daily_s)
        .and_then(|after| last.end().secs().checked_add(after))
        .ok_or(TimeError::OutsideCalendar)?;
    Ok((now.secs() >= due).then_some(session))
}

/// The [`SESSIONS`] regular sessions ending on `session`, oldest first.
///
/// # Errors
/// [`TimeError::OutsideCalendar`] when `session` is no trading day or the calendar holds too few
/// before it.
pub fn span(calendar: &ExchangeCalendar, session: Date) -> Result<Vec<Date>, TimeError> {
    if !calendar.is_trading_day(session)? {
        return Err(TimeError::OutsideCalendar);
    }
    let mut days = vec![session];
    let mut day = session;
    while days.len() < SESSIONS {
        day = previous(day)?;
        if calendar.is_trading_day(day)? {
            days.push(day);
        }
    }
    days.reverse();
    Ok(days)
}

/// The whole minute whose generation is due at `now`, if any.
///
/// # Errors
/// The calendar's, for a clock outside it.
pub fn minute_target(
    calendar: &ExchangeCalendar,
    now: UtcNanos,
    jitter: Jitter,
) -> Result<Option<UtcNanos>, TimeError> {
    if calendar.session_at(now)? != Some(Session::Regular) {
        return Ok(None);
    }
    let into = now.secs().rem_euclid(MINUTE_S);
    let start = now
        .secs()
        .checked_sub(into)
        .ok_or(TimeError::OutsideCalendar)?;
    if into < jitter.minute_s {
        return Ok(None);
    }
    UtcNanos::from_parts(start, 0).map(Some)
}

/// The calendar day before `day`.
fn previous(day: Date) -> Result<Date, TimeError> {
    let secs = new_york_midnight(day)?
        .secs()
        .checked_sub(DAY_S)
        .ok_or(TimeError::OutsideCalendar)?;
    Ok(UtcNanos::from_parts(secs, 0)?.date())
}
