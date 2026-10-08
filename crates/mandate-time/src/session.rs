//! The US-equities exchange calendar: trading days, early closes, and the overnight, pre-market,
//! regular, and after-hours sessions of every trading day, in UTC
//! ([trading-domain spec §2.2, §4.3](../../../docs/specs/trading-domain.md#43-sessions-us-equities)).
//! The schedule is data with its provenance (`data/us-equities.calendar`), never code (spec §1,
//! principle 2), and a question about a date outside its validity range is an error, never a guess.

use std::collections::{BTreeMap, BTreeSet};

use core::fmt;

use jiff::tz::Disambiguation;

use crate::calendar::{from_civil, new_york, new_york_date_and_hour, to_civil};
use crate::{Date, TimeError, UtcNanos};

/// NYSE and Nasdaq closures and early closes, 2018 to 2028, with their sources in the header.
const US_EQUITIES: &str = include_str!("../data/us-equities.calendar");

/// A US-equities trading session (spec §4.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Session {
    Overnight,
    PreMarket,
    Regular,
    AfterHours,
}

impl Session {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Overnight => "overnight",
            Self::PreMarket => "pre_market",
            Self::Regular => "regular",
            Self::AfterHours => "after_hours",
        }
    }
}

impl fmt::Display for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An America/New_York wall-clock time of day, to the minute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NewYorkTime {
    hour: u8,
    minute: u8,
}

impl NewYorkTime {
    pub fn new(hour: u8, minute: u8) -> Result<Self, TimeError> {
        if hour > 23 || minute > 59 {
            return Err(TimeError::InvalidDate);
        }
        Ok(Self { hour, minute })
    }

    pub fn hour(self) -> u8 {
        self.hour
    }

    pub fn minute(self) -> u8 {
        self.minute
    }

    /// Parses exactly `HH:MM`.
    pub fn parse(s: &str) -> Result<Self, TimeError> {
        let (hour, minute) = s.split_once(':').ok_or(TimeError::Syntax)?;
        Self::new(two_digits(hour)?, two_digits(minute)?)
    }
}

impl fmt::Display for NewYorkTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

fn two_digits(s: &str) -> Result<u8, TimeError> {
    if s.len() != 2 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(TimeError::Syntax);
    }
    s.parse().map_err(|_| TimeError::Syntax)
}

/// The instant at which America/New_York shows `time` on `date`. A time the day's daylight-saving
/// change skips (02:30 on the second Sunday of March) or repeats (01:30 on the first Sunday of
/// November) is `InvalidDate`, never resolved by a guess.
pub fn new_york_instant(date: Date, time: NewYorkTime) -> Result<UtcNanos, TimeError> {
    let hour = i8::try_from(time.hour).map_err(|_| TimeError::OutOfRange)?;
    let minute = i8::try_from(time.minute).map_err(|_| TimeError::OutOfRange)?;
    let local = to_civil(date)?.at(hour, minute, 0, 0);
    let timestamp = new_york()?
        .to_ambiguous_timestamp(local)
        .disambiguate(Disambiguation::Reject)
        .map_err(|_| TimeError::InvalidDate)?;
    UtcNanos::from_parts(timestamp.as_second(), 0)
}

/// One session of one trading day: `start` inclusive, `end` exclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionSpan {
    session: Session,
    start: UtcNanos,
    end: UtcNanos,
}

impl SessionSpan {
    pub fn session(self) -> Session {
        self.session
    }

    pub fn start(self) -> UtcNanos {
        self.start
    }

    pub fn end(self) -> UtcNanos {
        self.end
    }

    pub fn contains(self, at: UtcNanos) -> bool {
        self.start <= at && at < self.end
    }
}

/// Why the calendar data was rejected, with its 1-based line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CalendarDataError {
    #[error("line {line}: not a record of the calendar format, or a field is missing")]
    Syntax { line: usize },
    #[error("line {line}: {source}")]
    Value { line: usize, source: TimeError },
    #[error("line {line}: `valid` comes first and `hours` second, each once")]
    Header { line: usize },
    #[error("line {line}: the validity range ends before it starts")]
    InvertedRange { line: usize },
    #[error("line {line}: the session hours are not in increasing order")]
    Hours { line: usize },
    #[error("line {line}: the date is outside the validity range")]
    OutsideRange { line: usize },
    #[error("line {line}: weekends are always closed and are not listed")]
    Weekend { line: usize },
    #[error("line {line}: dates are listed in increasing order, each once")]
    Order { line: usize },
    #[error(
        "line {line}: an early close must end the regular session after it opens and before its usual close, and the after-hours session after that and no later than usual"
    )]
    EarlyClose { line: usize },
    #[error("the calendar has no `valid` or no `hours` record")]
    Incomplete,
}

impl CalendarDataError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Syntax { .. } => "syntax",
            Self::Value { .. } => "value",
            Self::Header { .. } => "header",
            Self::InvertedRange { .. } => "inverted_range",
            Self::Hours { .. } => "hours",
            Self::OutsideRange { .. } => "outside_range",
            Self::Weekend { .. } => "weekend",
            Self::Order { .. } => "order",
            Self::EarlyClose { .. } => "early_close",
            Self::Incomplete => "incomplete",
        }
    }
}

/// The sessions of a full trading day.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Hours {
    pre_market_open: NewYorkTime,
    regular_open: NewYorkTime,
    regular_close: NewYorkTime,
    after_hours_close: NewYorkTime,
}

/// The ends of an early-close day's regular and after-hours sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EarlyClose {
    regular_close: NewYorkTime,
    after_hours_close: NewYorkTime,
}

/// A versioned exchange calendar over a validity range: weekdays not listed as closed trade, with
/// the listed early closes. The overnight session of a trading day runs from the after-hours close
/// on the previous calendar day to the pre-market open, so it runs Sunday night to Friday morning
/// and never ends on a closed day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendar {
    valid_from: Date,
    valid_to: Date,
    hours: Hours,
    closed: BTreeSet<Date>,
    early_closes: BTreeMap<Date, EarlyClose>,
}

impl ExchangeCalendar {
    /// The NYSE and Nasdaq calendar checked in as `data/us-equities.calendar`.
    pub fn us_equities() -> Result<Self, CalendarDataError> {
        Self::parse(US_EQUITIES)
    }

    /// Parses the calendar format described in the header of `data/us-equities.calendar`.
    pub fn parse(text: &str) -> Result<Self, CalendarDataError> {
        let mut range: Option<(Date, Date)> = None;
        let mut hours: Option<Hours> = None;
        let mut closed = BTreeSet::new();
        let mut early_closes = BTreeMap::new();
        let mut last_listed: Option<Date> = None;
        for (line, text) in (1..).zip(text.split('\n')) {
            if text.is_empty() || text.starts_with('#') {
                continue;
            }
            let (record, rest) = text
                .split_once(' ')
                .ok_or(CalendarDataError::Syntax { line })?;
            match (record, range, hours) {
                ("valid", None, None) => range = Some(parse_range(line, rest)?),
                ("hours", Some(_), None) => hours = Some(parse_hours(line, rest)?),
                ("closed", Some(range), Some(_)) => {
                    let (date, label) = rest
                        .split_once(' ')
                        .ok_or(CalendarDataError::Syntax { line })?;
                    let date = value(line, Date::parse(date))?;
                    labelled(line, label)?;
                    listed(line, date, range, &mut last_listed)?;
                    closed.insert(date);
                }
                ("early_close", Some(range), Some(hours)) => {
                    let (date, close) = parse_early_close(line, rest, hours)?;
                    listed(line, date, range, &mut last_listed)?;
                    early_closes.insert(date, close);
                }
                ("valid" | "hours" | "closed" | "early_close", _, _) => {
                    return Err(CalendarDataError::Header { line });
                }
                _ => return Err(CalendarDataError::Syntax { line }),
            }
        }
        let ((valid_from, valid_to), hours) =
            range.zip(hours).ok_or(CalendarDataError::Incomplete)?;
        Ok(Self {
            valid_from,
            valid_to,
            hours,
            closed,
            early_closes,
        })
    }

    pub fn valid_from(&self) -> Date {
        self.valid_from
    }

    pub fn valid_to(&self) -> Date {
        self.valid_to
    }

    fn check(&self, date: Date) -> Result<Date, TimeError> {
        if (self.valid_from..=self.valid_to).contains(&date) {
            Ok(date)
        } else {
            Err(TimeError::OutsideCalendar)
        }
    }

    pub fn is_trading_day(&self, date: Date) -> Result<bool, TimeError> {
        let date = self.check(date)?;
        Ok(!date.is_weekend() && !self.closed.contains(&date))
    }

    /// The overnight, pre-market, regular, and after-hours sessions of `date`, in that order and
    /// each ending where the next starts; none on a closed day.
    pub fn sessions(&self, date: Date) -> Result<Vec<SessionSpan>, TimeError> {
        if !self.is_trading_day(date)? {
            return Ok(Vec::new());
        }
        let hours = self.hours;
        let close = self.early_closes.get(&date).copied().unwrap_or(EarlyClose {
            regular_close: hours.regular_close,
            after_hours_close: hours.after_hours_close,
        });
        let overnight = new_york_instant(previous(date)?, hours.after_hours_close)?;
        let pre_market = new_york_instant(date, hours.pre_market_open)?;
        let regular = new_york_instant(date, hours.regular_open)?;
        let after_hours = new_york_instant(date, close.regular_close)?;
        let end = new_york_instant(date, close.after_hours_close)?;
        Ok(vec![
            span(Session::Overnight, overnight, pre_market),
            span(Session::PreMarket, pre_market, regular),
            span(Session::Regular, regular, after_hours),
            span(Session::AfterHours, after_hours, end),
        ])
    }

    /// The session in progress at `at`, or `None` while the market is closed. An instant from the
    /// after-hours close onwards also needs the next calendar day, whose overnight session may
    /// already have started, so on the last valid day that is an error.
    pub fn session_at(&self, at: UtcNanos) -> Result<Option<Session>, TimeError> {
        let date = new_york_date(at)?;
        if let Some(found) = self.sessions(date)?.into_iter().find(|s| s.contains(at)) {
            return Ok(Some(found.session));
        }
        if at < new_york_instant(date, self.hours.after_hours_close)? {
            return Ok(None);
        }
        Ok(self
            .sessions(date.next()?)?
            .into_iter()
            .find(|s| s.contains(at))
            .map(SessionSpan::session))
    }

    /// The newest trading day whose regular session has ended at `now`: its end, an early close's
    /// included, is not after `now`. `None` when no regular session of the validity range has ended
    /// by `now`, a clock before the range included. Days after the range are never asked about, so
    /// a clock past the range is answered with the range's last completed session.
    pub fn last_completed_regular_session(&self, now: UtcNanos) -> Result<Option<Date>, TimeError> {
        let mut day = self.valid_from;
        let last = self.valid_to.min(now.date());
        let mut completed = None;
        while day <= last {
            if self
                .sessions(day)?
                .iter()
                .any(|span| span.session == Session::Regular && span.end <= now)
            {
                completed = Some(day);
            }
            if day == last {
                break;
            }
            day = day.next()?;
        }
        Ok(completed)
    }
}

fn span(session: Session, start: UtcNanos, end: UtcNanos) -> SessionSpan {
    SessionSpan {
        session,
        start,
        end,
    }
}

fn new_york_date(at: UtcNanos) -> Result<Date, TimeError> {
    new_york_date_and_hour(at).map(|(date, _)| date)
}

fn previous(date: Date) -> Result<Date, TimeError> {
    to_civil(date)?
        .yesterday()
        .map_err(|_| TimeError::OutOfRange)
        .and_then(from_civil)
}

fn value<T>(line: usize, parsed: Result<T, TimeError>) -> Result<T, CalendarDataError> {
    parsed.map_err(|source| CalendarDataError::Value { line, source })
}

fn fields<const N: usize>(line: usize, rest: &str) -> Result<[&str; N], CalendarDataError> {
    let parts: Vec<&str> = rest.split(' ').collect();
    <[&str; N]>::try_from(parts).map_err(|_| CalendarDataError::Syntax { line })
}

fn labelled(line: usize, label: &str) -> Result<(), CalendarDataError> {
    if label.trim().is_empty() {
        return Err(CalendarDataError::Syntax { line });
    }
    Ok(())
}

fn parse_range(line: usize, rest: &str) -> Result<(Date, Date), CalendarDataError> {
    let [from, to] = fields(line, rest)?;
    let (from, to) = (
        value(line, Date::parse(from))?,
        value(line, Date::parse(to))?,
    );
    if to < from {
        return Err(CalendarDataError::InvertedRange { line });
    }
    Ok((from, to))
}

fn parse_hours(line: usize, rest: &str) -> Result<Hours, CalendarDataError> {
    let [a, b, c, d] = fields(line, rest)?;
    let hours = Hours {
        pre_market_open: value(line, NewYorkTime::parse(a))?,
        regular_open: value(line, NewYorkTime::parse(b))?,
        regular_close: value(line, NewYorkTime::parse(c))?,
        after_hours_close: value(line, NewYorkTime::parse(d))?,
    };
    if hours.pre_market_open < hours.regular_open
        && hours.regular_open < hours.regular_close
        && hours.regular_close < hours.after_hours_close
    {
        Ok(hours)
    } else {
        Err(CalendarDataError::Hours { line })
    }
}

fn parse_early_close(
    line: usize,
    rest: &str,
    hours: Hours,
) -> Result<(Date, EarlyClose), CalendarDataError> {
    let mut parts = rest.splitn(4, ' ');
    let mut next = || parts.next().ok_or(CalendarDataError::Syntax { line });
    let (date, regular, after, label) = (next()?, next()?, next()?, next()?);
    labelled(line, label)?;
    let date = value(line, Date::parse(date))?;
    let close = EarlyClose {
        regular_close: value(line, NewYorkTime::parse(regular))?,
        after_hours_close: value(line, NewYorkTime::parse(after))?,
    };
    if hours.regular_open < close.regular_close
        && close.regular_close < hours.regular_close
        && close.regular_close < close.after_hours_close
        && close.after_hours_close <= hours.after_hours_close
    {
        Ok((date, close))
    } else {
        Err(CalendarDataError::EarlyClose { line })
    }
}

fn listed(
    line: usize,
    date: Date,
    (from, to): (Date, Date),
    last_listed: &mut Option<Date>,
) -> Result<(), CalendarDataError> {
    if !(from..=to).contains(&date) {
        return Err(CalendarDataError::OutsideRange { line });
    }
    if date.is_weekend() {
        return Err(CalendarDataError::Weekend { line });
    }
    if last_listed.is_some_and(|last| date <= last) {
        return Err(CalendarDataError::Order { line });
    }
    *last_listed = Some(date);
    Ok(())
}
