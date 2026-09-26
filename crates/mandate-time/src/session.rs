//! The US-equities exchange calendar: trading days, early closes, and the overnight, pre-market,
//! regular, and after-hours sessions of every trading day, in UTC
//! ([trading-domain spec §2.2, §4.3](../../../docs/specs/trading-domain.md#43-sessions-us-equities)).
//! The schedule is data with its provenance (`data/us-equities.calendar`), never code (spec §1,
//! principle 2), and a question about a date outside its validity range is an error, never a guess.

use core::fmt;

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
        let _ = (hour, minute);
        Err(TimeError::InvalidDate)
    }

    pub fn hour(self) -> u8 {
        self.hour
    }

    pub fn minute(self) -> u8 {
        self.minute
    }

    /// Parses exactly `HH:MM`.
    pub fn parse(s: &str) -> Result<Self, TimeError> {
        let _ = s;
        Err(TimeError::Syntax)
    }
}

impl fmt::Display for NewYorkTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

/// The instant at which America/New_York shows `time` on `date`. A time the day's daylight-saving
/// change skips (02:30 on the second Sunday of March) or repeats (01:30 on the first Sunday of
/// November) is `InvalidDate`, never resolved by a guess.
pub fn new_york_instant(date: Date, time: NewYorkTime) -> Result<UtcNanos, TimeError> {
    let _ = (date, time);
    Err(TimeError::InvalidDate)
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
        let _ = at;
        false
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

/// A versioned exchange calendar over a validity range: weekdays not listed as closed trade, with
/// the listed early closes. The overnight session of a trading day runs from the after-hours close
/// on the previous calendar day to the pre-market open, so it runs Sunday night to Friday morning
/// and never ends on a closed day.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeCalendar {
    valid_from: Date,
    valid_to: Date,
}

impl ExchangeCalendar {
    /// The NYSE and Nasdaq calendar checked in as `data/us-equities.calendar`.
    pub fn us_equities() -> Result<Self, CalendarDataError> {
        Self::parse(US_EQUITIES)
    }

    /// Parses the calendar format described in the header of `data/us-equities.calendar`.
    pub fn parse(text: &str) -> Result<Self, CalendarDataError> {
        let _ = text;
        Err(CalendarDataError::Incomplete)
    }

    pub fn valid_from(&self) -> Date {
        self.valid_from
    }

    pub fn valid_to(&self) -> Date {
        self.valid_to
    }

    pub fn is_trading_day(&self, date: Date) -> Result<bool, TimeError> {
        let _ = date;
        Err(TimeError::OutsideCalendar)
    }

    /// The overnight, pre-market, regular, and after-hours sessions of `date`, in that order and
    /// each ending where the next starts; none on a closed day.
    pub fn sessions(&self, date: Date) -> Result<Vec<SessionSpan>, TimeError> {
        let _ = date;
        Err(TimeError::OutsideCalendar)
    }

    /// The session in progress at `at`, or `None` while the market is closed. An instant from the
    /// after-hours close onwards also needs the next calendar day, whose overnight session may
    /// already have started, so on the last valid day that is an error.
    pub fn session_at(&self, at: UtcNanos) -> Result<Option<Session>, TimeError> {
        let _ = at;
        Err(TimeError::OutsideCalendar)
    }
}
