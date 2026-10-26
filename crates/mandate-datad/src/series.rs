//! The cell's recorded daily series, laid onto the US-equities calendar.
//!
//! The series is a recording, not market data: every session's open, high, low, close and VWAP are
//! one price, [`STEP`] above the session before it, from the close an [`Anchor`] names on its
//! date, with [`VOLUME`] shares a session. Because a close depends on its date alone, a session
//! once stored never changes, so a later clock adds the new session and rewrites none
//! (`Store::put_day` refuses a stored day whose bytes differ). The dataset lives beneath
//! [`RECORDED_ROOT`], so its path says what it is.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mandate_canon::DecStr;
use mandate_marketdata::dataset::{DatasetError, Status, Store};
use mandate_marketdata::model::{
    AssetClass, Bar, DatasetId, Feed, Kind, ModelError, Records, Symbol,
};
use mandate_num::{NumError, Price, Usd};
use mandate_time::{Date, ExchangeCalendar, TimeError, UtcNanos, new_york_midnight};

/// The sessions stored: past the model host's slow window (20) and the liquidity span (20), as
/// the R0 rehearsal's bars are.
pub const SESSIONS: usize = 30;
/// The rise from one session's close to the next.
pub const STEP: &str = "0.1";
/// Each session's volume: well above any participation cap's floor.
pub const VOLUME: &str = "50000000";
/// Each session's trade count.
pub const TRADES: u64 = 400_000;
/// Where the recorded datasets live under the store root.
pub const RECORDED_ROOT: &str = "cell-recorded";
/// The farthest a session may be from the anchor, in calendar days: past it the walk is refused
/// rather than run, so a wrong anchor cannot drift a price without bound.
pub const REACH_DAYS: u32 = 400;
/// How often the binary stores the series again, so a session that completes while it runs is
/// added within this.
pub const REFRESH: Duration = Duration::from_secs(60);

/// Whether a write is due: at start (`since_last` is `None`), and once [`REFRESH`] has passed since
/// the last one.
pub fn due(since_last: Option<Duration>) -> bool {
    since_last.is_none_or(|elapsed| elapsed >= REFRESH)
}

/// How far back the walk for the sessions starts, in days: comfortably more than [`SESSIONS`]
/// sessions, holidays included.
const LOOKBACK_DAYS: i64 = 60;
const DAY_S: i64 = 86_400;

/// Why the series could not be laid or stored. Each refuses the whole write.
#[derive(Debug, thiserror::Error)]
pub enum SeriesError {
    #[error("the anchor is not `<YYYY-MM-DD>=<positive close>`")]
    Anchor,
    #[error("the calendar cannot name the sessions: {0}")]
    Calendar(#[from] TimeError),
    #[error("the calendar names no completed session at the clock")]
    NoSession,
    #[error("the session is more than the reach from the anchor")]
    OutOfReach,
    #[error("the walk leaves a close that is not a positive price: {0}")]
    Price(#[from] NumError),
    #[error("the dataset: {0}")]
    Model(#[from] ModelError),
    #[error("the store: {0}")]
    Store(#[from] DatasetError),
    #[error("a bar value is not a decimal")]
    Decimal,
}

impl SeriesError {
    /// A fixed code for the log: no path, symbol or price (`AGENTS.md` rule 6's spirit for logs).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Anchor => "anchor_malformed",
            Self::Calendar(_) => "calendar_unavailable",
            Self::NoSession => "no_completed_session",
            Self::OutOfReach => "anchor_out_of_reach",
            Self::Price(_) => "close_not_a_price",
            Self::Model(_) => "dataset_invalid",
            Self::Store(_) => "store_refused",
            Self::Decimal => "bar_not_decimal",
        }
    }
}

/// The close the series has on one date.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    pub date: Date,
    pub close: Usd,
}

impl Anchor {
    /// `<YYYY-MM-DD>=<close>`, the close positive.
    ///
    /// # Errors
    /// [`SeriesError::Anchor`] for anything else.
    pub fn parse(text: &str) -> Result<Self, SeriesError> {
        let (date, close) = text.split_once('=').ok_or(SeriesError::Anchor)?;
        let date = Date::parse(date).map_err(|_| SeriesError::Anchor)?;
        Price::parse(close).map_err(|_| SeriesError::Anchor)?;
        let close = Usd::parse(close).map_err(|_| SeriesError::Anchor)?;
        Ok(Self { date, close })
    }
}

/// What one write stored: the dataset's directory and each session's outcome, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Published {
    pub dir: PathBuf,
    pub outcomes: Vec<Stored>,
}

/// One session's write: `written` is false when the day was already stored with the same bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    pub day: Date,
    pub written: bool,
}

/// The last [`SESSIONS`] regular sessions completed at `now`, oldest first, ending on
/// `last_completed_regular_session(now)`, the session the model host and the shell's trust check
/// both name.
///
/// # Errors
/// [`SeriesError::NoSession`] when the calendar names none, or fewer than [`SESSIONS`] lie in the
/// lookback; [`SeriesError::Calendar`] past the calendar's range.
pub fn sessions(calendar: &ExchangeCalendar, now: UtcNanos) -> Result<Vec<Date>, SeriesError> {
    let latest = calendar
        .last_completed_regular_session(now)?
        .ok_or(SeriesError::NoSession)?;
    let back = LOOKBACK_DAYS
        .checked_mul(DAY_S)
        .and_then(|span| now.secs().checked_sub(span))
        .ok_or(SeriesError::NoSession)?;
    let mut day = UtcNanos::from_parts(back, 0)?.date();
    let mut days = Vec::new();
    while day <= latest {
        if calendar.is_trading_day(day)? {
            days.push(day);
        }
        day = day.next()?;
    }
    let from = days
        .len()
        .checked_sub(SESSIONS)
        .ok_or(SeriesError::NoSession)?;
    days.drain(..from);
    Ok(days)
}

/// The series' close on `day`: the anchor's close plus one [`STEP`] for each trading day after the
/// anchor up to `day`, or less one for each trading day after `day` up to the anchor.
///
/// # Errors
/// [`SeriesError::OutOfReach`] more than [`REACH_DAYS`] from the anchor, and
/// [`SeriesError::Price`] when the close would not be a positive price.
pub fn close_on(
    calendar: &ExchangeCalendar,
    anchor: &Anchor,
    day: Date,
) -> Result<Price, SeriesError> {
    let step = Usd::parse(STEP)?;
    let rising = day >= anchor.date;
    let (from, to) = if rising {
        (anchor.date, day)
    } else {
        (day, anchor.date)
    };
    let mut close = anchor.close;
    let mut walked = 0_u32;
    let mut at = from;
    while at < to {
        at = at.next()?;
        walked = walked.checked_add(1).ok_or(SeriesError::OutOfReach)?;
        if walked > REACH_DAYS {
            return Err(SeriesError::OutOfReach);
        }
        if calendar.is_trading_day(at)? {
            close = if rising {
                close.checked_add(step)?
            } else {
                close.checked_sub(step)?
            };
        }
    }
    Ok(Price::parse(&close.to_string())?)
}

/// The session's bar: one price throughout, starting at New York midnight on its date, as the
/// vendor's daily bars do, so its UTC date is its session.
///
/// # Errors
/// [`SeriesError::Decimal`] or [`SeriesError::Calendar`] when a value cannot be formed.
pub fn bar(day: Date, close: Price) -> Result<Bar, SeriesError> {
    let price = DecStr::parse(&close.to_string()).map_err(|_| SeriesError::Decimal)?;
    Ok(Bar {
        start: new_york_midnight(day)?,
        open: price.clone(),
        high: price.clone(),
        low: price.clone(),
        close: price.clone(),
        volume: DecStr::parse(VOLUME).map_err(|_| SeriesError::Decimal)?,
        vwap: price,
        trade_count: TRADES,
    })
}

/// Stores the series for `symbol` at `now` under `root`'s [`RECORDED_ROOT`]: one
/// `Store::put_day` per session of [`sessions`], each idempotent.
///
/// # Errors
/// Any [`SeriesError`]; a session stored with other bytes is the store's conflict, and nothing
/// after it is written.
pub fn publish(
    root: &Path,
    symbol: &str,
    anchor: &Anchor,
    calendar: &ExchangeCalendar,
    now: UtcNanos,
) -> Result<Published, SeriesError> {
    let kind = Kind::Bars("1Day".parse().map_err(|_| SeriesError::Decimal)?);
    let dataset = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        kind,
        Symbol::parse(symbol)?,
    )?;
    let store = Store::new(root.join(RECORDED_ROOT));
    let mut outcomes = Vec::with_capacity(SESSIONS);
    for day in sessions(calendar, now)? {
        let close = close_on(calendar, anchor, day)?;
        let outcome = store.put_day(&dataset, day, &Records::Bars(vec![bar(day, close)?]))?;
        outcomes.push(Stored {
            day,
            written: outcome.status == Status::Written,
        });
    }
    Ok(Published {
        dir: store.dataset_dir(&dataset),
        outcomes,
    })
}
