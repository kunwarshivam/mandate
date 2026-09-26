//! Each feed's venue hours (backlog E2-4, brief interpretation 6): whether the feed's venue is
//! open at an instant. US-equity feeds follow the exchange calendar of `mandate-time` and their
//! own hours, checked in as `data/<feed>.venue` with their sources (trading domain spec §1
//! principle 2); crypto trades continuously.

use mandate_time::{
    CalendarDataError, Date, ExchangeCalendar, NewYorkTime, TimeError, UtcNanos,
    new_york_date_and_hour, new_york_instant,
};

use crate::model::Feed;

const SIP: &str = include_str!("../data/sip.venue");
const IEX: &str = include_str!("../data/iex.venue");

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VenueError {
    #[error(
        "line {line}: expected `valid`, then `hours`, then at most one `unpublished_after_early_close`, each with its fields"
    )]
    Syntax { line: usize },
    #[error("line {line}: {source}")]
    Value { line: usize, source: TimeError },
    #[error("line {line}: the dates or times must increase")]
    Order { line: usize },
    #[error("the venue hours need a `valid` and an `hours` record")]
    Incomplete,
    #[error("the exchange calendar: {0}")]
    Calendar(CalendarDataError),
}

impl VenueError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Syntax { .. } => "syntax",
            Self::Value { .. } => "value",
            Self::Order { .. } => "order",
            Self::Incomplete => "incomplete",
            Self::Calendar(_) => "calendar",
        }
    }
}

/// Whether a venue trades at an instant or on a day.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VenueState {
    Open,
    Closed,
    /// The venue's published hours do not say: an unpublished close, or a date outside the
    /// recorded hours or the exchange calendar.
    Unclassified,
}

/// A US-equity venue's hours on a full trading day, America/New_York wall-clock times, over the
/// dates they hold for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VenueHours {
    valid_from: Date,
    valid_to: Date,
    open: NewYorkTime,
    regular_open: NewYorkTime,
    regular_close: NewYorkTime,
    close: NewYorkTime,
    unpublished_after_early_close: Option<NewYorkTime>,
}

impl VenueHours {
    /// Parses the format described in the header of `data/sip.venue`.
    pub fn parse(text: &str) -> Result<Self, VenueError> {
        let mut range = None;
        let mut hours: Option<[NewYorkTime; 4]> = None;
        let mut until = None;
        let records = text
            .split('\n')
            .zip(1..)
            .filter(|(line, _)| !line.is_empty() && !line.starts_with('#'));
        for (record, line) in records {
            let fields: Vec<&str> = record.split(' ').collect();
            match (fields.as_slice(), range, hours, until) {
                (["valid", from, to], None, None, None) => {
                    let (from, to) = (date(line, from)?, date(line, to)?);
                    increasing(line, &[from, to], |a, b| a <= b)?;
                    range = Some((from, to));
                }
                (["hours", open, regular_open, regular_close, close], Some(_), None, None) => {
                    let times = [
                        time(line, open)?,
                        time(line, regular_open)?,
                        time(line, regular_close)?,
                        time(line, close)?,
                    ];
                    increasing(line, &times, |a, b| a < b)?;
                    hours = Some(times);
                }
                (
                    ["unpublished_after_early_close", at],
                    Some(_),
                    Some([_, _, regular_close, _]),
                    None,
                ) => {
                    let at = time(line, at)?;
                    increasing(line, &[regular_close, at], |a, b| a < b)?;
                    until = Some(at);
                }
                _ => return Err(VenueError::Syntax { line }),
            }
        }
        let (Some((valid_from, valid_to)), Some([open, regular_open, regular_close, close])) =
            (range, hours)
        else {
            return Err(VenueError::Incomplete);
        };
        Ok(Self {
            valid_from,
            valid_to,
            open,
            regular_open,
            regular_close,
            close,
            unpublished_after_early_close: until,
        })
    }

    /// The consolidated tape's hours, checked in as `data/sip.venue`.
    pub fn sip() -> Result<Self, VenueError> {
        Self::parse(SIP)
    }

    /// IEX's hours, checked in as `data/iex.venue`.
    pub fn iex() -> Result<Self, VenueError> {
        Self::parse(IEX)
    }

    pub fn valid_from(&self) -> Date {
        self.valid_from
    }

    pub fn valid_to(&self) -> Date {
        self.valid_to
    }

    pub fn open(&self) -> NewYorkTime {
        self.open
    }

    pub fn regular_open(&self) -> NewYorkTime {
        self.regular_open
    }

    pub fn regular_close(&self) -> NewYorkTime {
        self.regular_close
    }

    pub fn close(&self) -> NewYorkTime {
        self.close
    }

    /// Until when an early-close day is unclassified after the calendar's regular close, for a
    /// venue that does not publish its early-close hours. The window never outlasts the venue's
    /// own close or the calendar's after-hours session: a venue is open, or possibly open, only
    /// while both say so.
    pub fn unpublished_after_early_close(&self) -> Option<NewYorkTime> {
        self.unpublished_after_early_close
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Schedule {
    Continuous,
    Exchange {
        calendar: ExchangeCalendar,
        hours: VenueHours,
    },
}

/// When one feed's venue trades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Venue {
    schedule: Schedule,
}

impl Venue {
    /// The venue of `feed`: SIP and IEX by the US-equities calendar and their checked-in hours,
    /// crypto continuous.
    pub fn of(feed: Feed) -> Result<Self, VenueError> {
        let hours = match feed {
            Feed::Sip => VenueHours::sip()?,
            Feed::Iex => VenueHours::iex()?,
            Feed::CryptoUs => return Ok(Self::continuous()),
        };
        let calendar = ExchangeCalendar::us_equities().map_err(VenueError::Calendar)?;
        Ok(Self::exchange(calendar, hours))
    }

    /// A venue that never closes.
    pub fn continuous() -> Self {
        Self {
            schedule: Schedule::Continuous,
        }
    }

    /// A venue open while `calendar` has a pre-market, regular, or after-hours session and
    /// `hours` say it is open.
    pub fn exchange(calendar: ExchangeCalendar, hours: VenueHours) -> Self {
        Self {
            schedule: Schedule::Exchange { calendar, hours },
        }
    }

    pub fn state_at(&self, at: UtcNanos) -> Result<VenueState, TimeError> {
        self.states().at(at)
    }

    /// Whether the venue trades at all on `date`, as daily bars need.
    pub fn day_state(&self, date: Date) -> Result<VenueState, TimeError> {
        let Schedule::Exchange { calendar, hours } = &self.schedule else {
            return Ok(VenueState::Open);
        };
        if !hours.holds_on(date) {
            return Ok(VenueState::Unclassified);
        }
        match calendar.is_trading_day(date) {
            Ok(true) => Ok(VenueState::Open),
            Ok(false) => Ok(VenueState::Closed),
            Err(TimeError::OutsideCalendar) => Ok(VenueState::Unclassified),
            Err(e) => Err(e),
        }
    }

    /// A reader of states that reuses each New York day's hours across instants.
    pub fn states(&self) -> States<'_> {
        States {
            venue: self,
            day: None,
        }
    }

    /// The venue's state through the New York day `date`: `base`, except over its periods.
    fn day(&self, date: Date) -> Result<Day, TimeError> {
        let (base, periods) = match &self.schedule {
            Schedule::Continuous => (VenueState::Open, Vec::new()),
            Schedule::Exchange { calendar, hours } => hours.periods(calendar, date)?,
        };
        Ok(Day {
            date,
            base,
            periods,
        })
    }
}

impl VenueHours {
    fn holds_on(&self, date: Date) -> bool {
        (self.valid_from..=self.valid_to).contains(&date)
    }

    /// The periods of `date` during which the venue is not closed, with the state of each.
    fn periods(
        &self,
        calendar: &ExchangeCalendar,
        date: Date,
    ) -> Result<(VenueState, Vec<Period>), TimeError> {
        if !self.holds_on(date) {
            return Ok((VenueState::Unclassified, Vec::new()));
        }
        let spans = match calendar.sessions(date) {
            Err(TimeError::OutsideCalendar) => return Ok((VenueState::Unclassified, Vec::new())),
            spans => spans?,
        };
        let [_overnight, pre_market, regular, after_hours] = spans.as_slice() else {
            return Ok((VenueState::Closed, Vec::new()));
        };
        let open = pre_market.start().max(new_york_instant(date, self.open)?);
        let close = after_hours.end().min(new_york_instant(date, self.close)?);
        let early = regular.end() < new_york_instant(date, self.regular_close)?;
        let periods = match self.unpublished_after_early_close {
            Some(until) if early => vec![
                (open, regular.end(), VenueState::Open),
                (
                    regular.end(),
                    close.min(new_york_instant(date, until)?),
                    VenueState::Unclassified,
                ),
            ],
            _ => vec![(open, close, VenueState::Open)],
        };
        Ok((VenueState::Closed, periods))
    }
}

type Period = (UtcNanos, UtcNanos, VenueState);

#[derive(Debug)]
struct Day {
    date: Date,
    base: VenueState,
    periods: Vec<Period>,
}

impl Day {
    fn state_at(&self, at: UtcNanos) -> VenueState {
        self.periods
            .iter()
            .find(|(start, end, _)| *start <= at && at < *end)
            .map_or(self.base, |(_, _, state)| *state)
    }
}

/// States of one venue at many instants.
#[derive(Debug)]
pub struct States<'a> {
    venue: &'a Venue,
    day: Option<Day>,
}

impl States<'_> {
    pub fn at(&mut self, at: UtcNanos) -> Result<VenueState, TimeError> {
        let (date, _) = new_york_date_and_hour(at)?;
        if let Some(day) = &self.day
            && day.date == date
        {
            return Ok(day.state_at(at));
        }
        let day = self.venue.day(date)?;
        let state = day.state_at(at);
        self.day = Some(day);
        Ok(state)
    }
}

fn date(line: usize, field: &str) -> Result<Date, VenueError> {
    Date::parse(field).map_err(|source| VenueError::Value { line, source })
}

fn time(line: usize, field: &str) -> Result<NewYorkTime, VenueError> {
    NewYorkTime::parse(field).map_err(|source| VenueError::Value { line, source })
}

fn increasing<T: Copy>(
    line: usize,
    values: &[T],
    ordered: impl Fn(T, T) -> bool,
) -> Result<(), VenueError> {
    if values
        .windows(2)
        .all(|pair| matches!(pair, [a, b] if ordered(*a, *b)))
    {
        Ok(())
    } else {
        Err(VenueError::Order { line })
    }
}
