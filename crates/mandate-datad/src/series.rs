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
use mandate_time::{
    Date, ExchangeCalendar, Session, TimeError, UtcNanos, new_york_date_and_hour, new_york_midnight,
};

/// The sessions stored: past the model host's slow window (20) and the liquidity span (20), as
/// the R0 rehearsal's bars are.
pub const SESSIONS: usize = 30;
/// The rise from one session's close to the next.
pub const STEP: &str = "0.1";
/// Each session's volume: well above any participation cap's floor.
pub const VOLUME: &str = "50000000";
/// Each session's trade count.
pub const TRADES: u64 = 400_000;
/// Each one-minute bar's volume in the recorded minute series.
pub const MINUTE_VOLUME: &str = "100000";
/// Each one-minute bar's trade count.
pub const MINUTE_TRADES: u64 = 1_000;
/// One minute bar's span, in seconds.
const MINUTE_S: i64 = 60;
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

/// How far back, in calendar days, [`minute_day`] looks for a trading day: past any run of
/// closed days the calendar holds.
pub const MINUTE_LOOKBACK_DAYS: i64 = 10;

/// The latest trading day on or before `now`'s New York date, within [`MINUTE_LOOKBACK_DAYS`]:
/// the day whose minute bars [`publish_minutes`] stores.
///
/// # Errors
/// [`SeriesError::Calendar`] past the calendar's range.
pub fn minute_day(calendar: &ExchangeCalendar, now: UtcNanos) -> Result<Option<Date>, SeriesError> {
    let (today, _) = new_york_date_and_hour(now)?;
    let back = MINUTE_LOOKBACK_DAYS
        .checked_mul(DAY_S)
        .and_then(|span| now.secs().checked_sub(span))
        .ok_or(SeriesError::OutOfReach)?;
    let (mut day, _) = new_york_date_and_hour(UtcNanos::from_parts(back, 0)?)?;
    let mut latest = None;
    while day <= today {
        if calendar.is_trading_day(day)? {
            latest = Some(day);
        }
        day = day.next()?;
    }
    Ok(latest)
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
    #[error("the trust check refused the stored series")]
    Untrusted,
    #[error("the artifact store cannot be opened or written")]
    Artifacts,
    #[error("the instruments are not `<symbol>:<asset-id>:<anchor>`, comma-separated, each once")]
    Instruments,
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
            Self::Untrusted => "series_untrusted",
            Self::Artifacts => "artifacts_unavailable",
            Self::Instruments => "instruments_malformed",
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

/// One instrument the process stores: its symbol, the asset id its closes object names, and its
/// series' anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instrument {
    pub symbol: String,
    pub asset_id: String,
    pub anchor: Anchor,
}

impl Instrument {
    /// `<symbol>:<asset-id>:<YYYY-MM-DD>=<close>`, the symbol one the dataset admits and the asset
    /// id not empty.
    ///
    /// # Errors
    /// [`SeriesError::Instruments`] for a malformed symbol, asset id or shape, and
    /// [`SeriesError::Anchor`] for a malformed anchor.
    pub fn parse(text: &str) -> Result<Self, SeriesError> {
        let mut parts = text.split(':');
        let (Some(symbol), Some(asset_id), Some(anchor), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(SeriesError::Instruments);
        };
        Symbol::parse(symbol).map_err(|_| SeriesError::Instruments)?;
        if asset_id.is_empty() {
            return Err(SeriesError::Instruments);
        }
        Ok(Self {
            symbol: symbol.to_owned(),
            asset_id: asset_id.to_owned(),
            anchor: Anchor::parse(anchor)?,
        })
    }

    /// The chart's `--instruments`: one or more [`Instrument::parse`] texts, comma-separated, no
    /// symbol twice, in the order given.
    ///
    /// # Errors
    /// [`SeriesError::Instruments`] for an empty list or a symbol named twice, and any error of
    /// [`Instrument::parse`].
    pub fn parse_list(text: &str) -> Result<Vec<Self>, SeriesError> {
        let mut all: Vec<Self> = Vec::new();
        for one in text.split(',') {
            let instrument = Self::parse(one)?;
            if all.iter().any(|seen| seen.symbol == instrument.symbol) {
                return Err(SeriesError::Instruments);
            }
            all.push(instrument);
        }
        Ok(all)
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

/// The minute bars of `day`'s regular session, early closes included, one a minute from its open to
/// its close, each at the series' close for `day` and [`MINUTE_VOLUME`] shares; none on a closed
/// day. The whole session is one function of its date, so its partition is written once and never
/// changes; a reader takes only bars that have ended by its clock (`mandate_bars::trailing_minutes`).
///
/// # Errors
/// [`SeriesError::Calendar`] past the calendar's range, and [`close_on`]'s errors.
pub fn session_minutes(
    calendar: &ExchangeCalendar,
    anchor: &Anchor,
    day: Date,
) -> Result<Vec<Bar>, SeriesError> {
    let Some(regular) = calendar
        .sessions(day)?
        .into_iter()
        .find(|span| span.session() == Session::Regular)
    else {
        return Ok(Vec::new());
    };
    let price = DecStr::parse(&close_on(calendar, anchor, day)?.to_string())
        .map_err(|_| SeriesError::Decimal)?;
    let volume = DecStr::parse(MINUTE_VOLUME).map_err(|_| SeriesError::Decimal)?;
    let mut bars = Vec::new();
    let mut start = regular.start();
    while start < regular.end() {
        bars.push(Bar {
            start,
            open: price.clone(),
            high: price.clone(),
            low: price.clone(),
            close: price.clone(),
            volume: volume.clone(),
            vwap: price.clone(),
            trade_count: MINUTE_TRADES,
        });
        let next = start
            .secs()
            .checked_add(MINUTE_S)
            .ok_or(SeriesError::OutOfReach)?;
        start = UtcNanos::from_parts(next, start.nanos())?;
    }
    Ok(bars)
}

/// Stores the regular-session minute bars of the latest trading day up to `now`'s New York date
/// ([`minute_day`]) for `symbol` under `root`'s [`RECORDED_ROOT`]: today's on a trading day, and the
/// last session's on a closed one, so the dataset is there to inspect on any day, and a reader at
/// a closed day's clock finds none of its bars inside the trailing window. `None` only when no
/// trading day lies within [`MINUTE_LOOKBACK_DAYS`]. Idempotent: a day's bars never change, so a
/// second write leaves them as they are.
///
/// # Errors
/// Any [`SeriesError`], the store's conflict included.
pub fn publish_minutes(
    root: &Path,
    symbol: &str,
    anchor: &Anchor,
    calendar: &ExchangeCalendar,
    now: UtcNanos,
) -> Result<Option<Published>, SeriesError> {
    let Some(day) = minute_day(calendar, now)? else {
        return Ok(None);
    };
    let bars = session_minutes(calendar, anchor, day)?;
    let Some(first) = bars.first() else {
        return Ok(None);
    };
    let partition = first.start.date();
    let kind = Kind::Bars("1Min".parse().map_err(|_| SeriesError::Decimal)?);
    let dataset = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        kind,
        Symbol::parse(symbol)?,
    )?;
    let store = Store::new(root.join(RECORDED_ROOT));
    let outcome = store.put_day(&dataset, partition, &Records::Bars(bars))?;
    Ok(Some(Published {
        dir: store.dataset_dir(&dataset),
        outcomes: vec![Stored {
            day: partition,
            written: outcome.status == Status::Written,
        }],
    }))
}
