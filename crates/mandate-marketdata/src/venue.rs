//! Each feed's venue hours (backlog E2-4, brief interpretation 6): whether the feed's venue is
//! open at an instant. US-equity feeds follow the exchange calendar of `mandate-time` and their
//! own hours, checked in as `data/<feed>.venue` with their sources (trading domain spec §1
//! principle 2); crypto trades continuously.

use mandate_time::{CalendarDataError, Date, ExchangeCalendar, NewYorkTime, TimeError, UtcNanos};

use crate::model::Feed;

const SIP: &str = include_str!("../data/sip.venue");
const IEX: &str = include_str!("../data/iex.venue");

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum VenueError {
    #[error("line {line}: expected `valid`, then `hours`, then at most one `unpublished_after_early_close`, each with its fields")]
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
        ""
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
    pub fn parse(_text: &str) -> Result<Self, VenueError> {
        Err(VenueError::Incomplete)
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
    /// venue that does not publish its early-close hours.
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
    pub fn of(_feed: Feed) -> Result<Self, VenueError> {
        Ok(Self::continuous())
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
    pub fn day_state(&self, _date: Date) -> Result<VenueState, TimeError> {
        Ok(VenueState::Closed)
    }

    /// A reader of states that reuses each New York day's hours across instants.
    pub fn states(&self) -> States<'_> {
        States { venue: self }
    }
}

/// States of one venue at many instants.
#[derive(Debug)]
pub struct States<'a> {
    venue: &'a Venue,
}

impl States<'_> {
    pub fn at(&mut self, _at: UtcNanos) -> Result<VenueState, TimeError> {
        let _ = self.venue;
        Ok(VenueState::Closed)
    }
}
