//! The ticks of the risk clock and the trading days they start (journal spec §9.18).

use mandate_time::{Date, ExchangeCalendar, UtcNanos, new_york_date_and_hour};

/// The tick after `last`, given the host clock's reading `host`: the later of `last` plus one
/// second and `host` truncated to its second, or that second for a stream's first tick. A host
/// clock that steps back never holds or reverses the risk clock; one that steps forward is
/// followed, and the measurements find it. `None` only past the last representable second.
pub fn next(last: Option<UtcNanos>, host: UtcNanos) -> Option<UtcNanos> {
    let host_second = host.secs();
    let secs = match last {
        Some(last) => last.secs().checked_add(1)?.max(host_second),
        None => host_second,
    };
    UtcNanos::from_parts(secs, 0).ok()
}

/// The trading date `tick` starts, if any: its America/New_York date, when that is a trading day
/// later than `last_day`. A date the calendar cannot answer starts no day, so the previous day's
/// limits stay in force rather than being reset on a guess (`AGENTS.md` rule 3).
pub fn trading_day(
    calendar: &ExchangeCalendar,
    tick: UtcNanos,
    last_day: Option<Date>,
) -> Option<Date> {
    let (date, _) = new_york_date_and_hour(tick).ok()?;
    let later = last_day.is_none_or(|last| date > last);
    (later && calendar.is_trading_day(date).ok()?).then_some(date)
}
