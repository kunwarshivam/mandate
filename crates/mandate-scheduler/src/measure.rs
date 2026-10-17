//! Measurements of the host clock against its time servers (journal spec §5.4, §9.18).

use mandate_time::{Date, ExchangeCalendar, Session, UtcNanos};

/// How often a measurement is taken outside the other two triggers: half §5.4's 10-minute bound,
/// so one lost measurement never leaves the executor's 15-minute window without one.
pub const PERIOD_SECS: i64 = 300;

/// How long before the regular session opens the `before_session` measurement may be taken.
pub const BEFORE_SESSION_SECS: i64 = 900;

/// §5.4's default tolerance: 50 ms.
pub const TOLERANCE_NS: u64 = 50_000_000;

const NANOS_PER_SEC: u64 = 1_000_000_000;

/// Why a measurement was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Startup,
    BeforeSession,
    Periodic,
}

impl Trigger {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Startup => "startup",
            Self::BeforeSession => "before_session",
            Self::Periodic => "periodic",
        }
    }
}

/// One time server's answer: the host clock minus its time, and the round trip, in nanoseconds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sample {
    pub server: String,
    pub stratum: u8,
    pub offset_ns: i64,
    pub delay_ns: i64,
}

/// A judged measurement, as `ClockOffsetRecorded` carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Measurement {
    pub trigger: Trigger,
    /// The answering servers: stratum 1 to 15, a delay of at least 0, one per server, ascending.
    pub samples: Vec<Sample>,
    /// The median by magnitude of the samples' offsets, and of two the larger; `None` for none.
    pub offset_ns: Option<i64>,
    pub tolerance_ns: u64,
    pub within_tolerance: bool,
}

impl Measurement {
    /// Judges `answers` against `tolerance_ns`. An answer that is unsynchronized (stratum 0 or 16
    /// or above), carries a negative delay, or repeats a server already answered is not a
    /// sample. With no sample there is no offset, which is out of tolerance (`AGENTS.md` rule 3).
    pub fn judge(trigger: Trigger, mut answers: Vec<Sample>, tolerance_ns: u64) -> Self {
        answers.retain(|s| (1..=15).contains(&s.stratum) && s.delay_ns >= 0);
        answers.sort_by(|a, b| a.server.cmp(&b.server));
        answers.dedup_by(|later, earlier| later.server == earlier.server);
        let mut offsets: Vec<i64> = answers.iter().map(|s| s.offset_ns).collect();
        offsets.sort_by_key(|o| (o.unsigned_abs(), *o));
        let offset_ns = offsets.get(offsets.len() / 2).copied();
        let within_tolerance = offset_ns.is_some_and(|o| o.unsigned_abs() <= tolerance_ns);
        Self {
            trigger,
            samples: answers,
            offset_ns,
            tolerance_ns,
            within_tolerance,
        }
    }
}

/// What the schedule has seen: whether this process has measured yet, the tick of the last
/// measurement on the stream, and the date of the last `before_session` one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Schedule {
    pub measured_since_start: bool,
    pub last_measured: Option<UtcNanos>,
    pub before_session_on: Option<Date>,
}

/// The measurement due at `tick`, if any: at start first; then once in the 15 minutes before a
/// trading day's regular session; then whenever [`PERIOD_SECS`] have passed since the last.
pub fn due(schedule: &Schedule, calendar: &ExchangeCalendar, tick: UtcNanos) -> Option<Trigger> {
    if !schedule.measured_since_start {
        return Some(Trigger::Startup);
    }
    if let Some(date) = opening_soon(calendar, tick)
        && schedule.before_session_on != Some(date)
    {
        return Some(Trigger::BeforeSession);
    }
    let elapsed = schedule
        .last_measured
        .and_then(|last| tick.secs().checked_sub(last.secs()));
    elapsed
        .is_none_or(|secs| secs >= PERIOD_SECS)
        .then_some(Trigger::Periodic)
}

/// The trading date whose regular session opens within [`BEFORE_SESSION_SECS`] after `tick`.
pub fn opening_soon(calendar: &ExchangeCalendar, tick: UtcNanos) -> Option<Date> {
    let (date, _) = mandate_time::new_york_date_and_hour(tick).ok()?;
    let regular = calendar
        .sessions(date)
        .ok()?
        .into_iter()
        .find(|span| span.session() == Session::Regular)?;
    let until = regular.start().secs().checked_sub(tick.secs())?;
    (until > 0 && until <= BEFORE_SESSION_SECS).then_some(date)
}

/// `nanos` as §4.6 decimal seconds, from integers alone: no trailing fractional zero, no `-0`.
pub fn seconds(nanos: i64) -> String {
    let sign = if nanos < 0 { "-" } else { "" };
    let magnitude = nanos.unsigned_abs();
    let whole = magnitude.checked_div(NANOS_PER_SEC).unwrap_or_default();
    let part = magnitude.checked_rem(NANOS_PER_SEC).unwrap_or_default();
    if part == 0 {
        return format!("{sign}{whole}");
    }
    let fraction = format!("{part:09}");
    format!("{sign}{whole}.{}", fraction.trim_end_matches('0'))
}

/// `nanos` as §4.6 decimal seconds, for the unsigned tolerance.
pub fn tolerance_seconds(nanos: u64) -> String {
    seconds(i64::try_from(nanos).unwrap_or(i64::MAX))
}
