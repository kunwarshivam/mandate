//! Writing an `alpaca-iex` root's generations (DEC-2283 items 4 to 6, data-plane spec §3.8), in
//! the layout `mandate_bars::generations` reads, DP-15 and DP-16:
//!
//! - **Whole or not at all.** An instrument's dataset is built under the root's `.staging` and
//!   renamed into its generation only once its every day, and for a daily generation its corporate
//!   actions, are written, so a reader never sees part of one. A dataset already in place is never
//!   rewritten or asked for again.
//! - **One request for every instrument due**, bars and then actions, through a [`BarsSource`].
//! - **Never guessed.** Every bar stored is one the source answered for that instrument. A day the
//!   source answers empty is stored empty. A bar on a day the span does not hold refuses that
//!   instrument's dataset. An empty newest session waits [`CONFIRM_S`], asked again every
//!   [`CONFIRM_EVERY_S`], before it is stored.
//! - **Retention.** Daily generations are kept for two sessions and minute generations for ten
//!   minutes; removing one past that is the one deletion DP-2 allows here.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use mandate_bars::generations::{daily_generation, minute_generation};
use mandate_bars::{daily_dir, minute_dir};
use mandate_journal::ArtifactRef;
use mandate_marketdata::actions::{RecordedActions, write_actions};
use mandate_marketdata::batch::{BarsBatch, BatchError};
use mandate_marketdata::client::FetchError;
use mandate_marketdata::dataset::{MANIFEST, Store};
use mandate_marketdata::model::{
    AssetClass, Bar, CorporateActions, DatasetId, DayRange, Feed, Kind, Records, Symbol, TimeUnit,
    Timeframe,
};
use mandate_time::{Date, ExchangeCalendar, TimeError, UtcNanos, new_york_midnight};

use crate::iex::instruments::Instrument;
use crate::iex::schedule::{CONFIRM_EVERY_S, CONFIRM_S, span};
use crate::observation::put_closes;

/// The trailing window a minute generation holds, in seconds: the participation check's five
/// minutes (trading spec §9.6).
pub const MINUTE_WINDOW_S: i64 = 300;
/// How long a minute generation is kept, in seconds.
pub const MINUTE_RETENTION_S: i64 = 600;
const MINUTE_S: i64 = 60;
const STAGING: &str = ".staging";

/// Where a generation's bars and actions come from: the binary's batched fetch over the platform
/// data key, or a test's fake.
pub trait BarsSource {
    /// Every bar of `batch`, each symbol's apart.
    ///
    /// # Errors
    /// The fetch's, for an answer that was refused or could not be read.
    fn bars(&self, batch: &BarsBatch) -> Result<BTreeMap<Symbol, Vec<Bar>>, FetchError>;

    /// Every corporate action of `symbols` dated in `range`, each symbol's apart.
    ///
    /// # Errors
    /// The fetch's, for an answer that was refused or could not be read.
    fn actions(
        &self,
        symbols: &BTreeSet<Symbol>,
        range: DayRange,
    ) -> Result<BTreeMap<Symbol, CorporateActions>, FetchError>;
}

#[derive(Debug, thiserror::Error)]
pub enum GenerationError {
    #[error("the source: {0}")]
    Fetch(FetchError),
    #[error("the batch: {0}")]
    Batch(BatchError),
    #[error("the calendar: {0}")]
    Time(#[from] TimeError),
    #[error("the store: {what}")]
    Store { what: &'static str },
}

impl GenerationError {
    /// Stable reason code (ADR-0001 ES-09), naming no symbol or price.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Fetch(_) => "fetch",
            Self::Batch(_) => "batch",
            Self::Time(_) => "time",
            Self::Store { .. } => "store",
        }
    }

    /// Whether the source refused the platform key itself (`401` or `403`): the process then stops,
    /// and its restart reads the key again (data-plane spec §3.8 "Key").
    pub fn key_refused(&self) -> bool {
        matches!(self, Self::Fetch(FetchError::Status { status: 401 | 403 }))
    }
}

fn store(what: &'static str) -> impl Fn(std::io::Error) -> GenerationError {
    move |_| GenerationError::Store { what }
}

/// What one daily pass did, each list in symbol order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Daily {
    /// Instruments whose dataset this pass put in the generation.
    pub written: Vec<Symbol>,
    /// Instruments whose newest session the source answered empty, still within [`CONFIRM_S`].
    pub waiting: Vec<Symbol>,
    /// Instruments the source answered a bar for on a day the span does not hold.
    pub refused: Vec<Symbol>,
}

/// When an instrument's empty newest session was first and last asked for.
struct Waiting {
    first: i64,
    last: i64,
}

/// The writer of one `alpaca-iex` root, and the closes objects it puts in `artifacts`.
pub struct Writer {
    root: PathBuf,
    artifacts: PathBuf,
    waiting: BTreeMap<(Date, Symbol), Waiting>,
    put: BTreeSet<(Date, String)>,
}

impl Writer {
    /// A writer of `root`, clearing what an interrupted write left in its staging.
    ///
    /// # Errors
    /// [`GenerationError::Store`] when the staging cannot be cleared.
    pub fn new(root: PathBuf, artifacts: PathBuf) -> Result<Self, GenerationError> {
        let staging = root.join(STAGING);
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(store("staging"))?;
        }
        Ok(Self {
            root,
            artifacts,
            waiting: BTreeMap::new(),
            put: BTreeSet::new(),
        })
    }

    /// Writes the daily generation of `session` for every instrument of `instruments` not yet in
    /// it and not waiting out [`CONFIRM_EVERY_S`], at the clock `now`; one whose [`CONFIRM_S`] has
    /// run out is asked at once, so its empty day is stored then.
    ///
    /// # Errors
    /// [`GenerationError`] when the calendar, a request, or the store fails; nothing partial is
    /// then in the generation.
    pub fn daily(
        &mut self,
        source: &dyn BarsSource,
        calendar: &ExchangeCalendar,
        session: Date,
        instruments: &[Instrument],
        now: UtcNanos,
    ) -> Result<Daily, GenerationError> {
        let days = span(calendar, session)?;
        let generation = daily_generation(&self.root, session);
        let mut due = BTreeSet::new();
        for instrument in instruments {
            let symbol = &instrument.symbol;
            if daily_dir(&generation, symbol.as_str())
                .map_err(|_| GenerationError::Store { what: "layout" })?
                .exists()
            {
                continue;
            }
            let key = (session, symbol.clone());
            let asked_lately = self.waiting.get(&key).is_some_and(|waiting| {
                now.secs().saturating_sub(waiting.last) < CONFIRM_EVERY_S
                    && now.secs().saturating_sub(waiting.first) < CONFIRM_S
            });
            if !asked_lately {
                due.insert(symbol.clone());
            }
        }
        let mut done = Daily::default();
        let Some(first) = days.first().copied() else {
            return Ok(done);
        };
        if due.is_empty() {
            return Ok(done);
        }
        let start = new_york_midnight(first)?;
        let end = new_york_midnight(session.next()?)?;
        let batch = BarsBatch::new(due.clone(), timeframe(TimeUnit::Day)?, start, end)
            .map_err(GenerationError::Batch)?;
        let mut bars = source.bars(&batch).map_err(GenerationError::Fetch)?;
        let range =
            DayRange::new(first, session).map_err(|_| GenerationError::Store { what: "range" })?;
        let mut actions = source
            .actions(&due, range)
            .map_err(GenerationError::Fetch)?;
        let span_days: BTreeSet<Date> = days.iter().copied().collect();
        for symbol in due {
            let by_day = by_day(bars.remove(&symbol).unwrap_or_default());
            if by_day.keys().any(|d| !span_days.contains(d)) {
                done.refused.push(symbol);
                continue;
            }
            let key = (session, symbol.clone());
            if !by_day.contains_key(&session) {
                let waiting = self.waiting.entry(key).or_insert(Waiting {
                    first: now.secs(),
                    last: now.secs(),
                });
                waiting.last = now.secs();
                if now.secs().saturating_sub(waiting.first) < CONFIRM_S {
                    done.waiting.push(symbol);
                    continue;
                }
            }
            let recorded = RecordedActions {
                range,
                actions: actions
                    .remove(&symbol)
                    .unwrap_or_else(|| CorporateActions::none(symbol.clone()))
                    .dated_in(range),
            };
            self.publish(
                &format!("daily-{session}-{}", symbol.as_str()),
                &generation,
                &symbol,
                TimeUnit::Day,
                &days,
                by_day,
                Some(&recorded),
            )?;
            self.waiting.remove(&(session, symbol.clone()));
            done.written.push(symbol);
        }
        Ok(done)
    }

    /// Writes the minute generation of the whole minute `minute` for every instrument of
    /// `instruments` not yet in it: the complete one-minute bars of the [`MINUTE_WINDOW_S`] before
    /// it, stored empty for an instrument with none. Returns the instruments written, in symbol
    /// order.
    ///
    /// # Errors
    /// [`GenerationError`] when a request or the store fails; nothing partial is then in the
    /// generation.
    pub fn minute(
        &mut self,
        source: &dyn BarsSource,
        minute: UtcNanos,
        instruments: &[Instrument],
    ) -> Result<Vec<Symbol>, GenerationError> {
        let generation = minute_generation(&self.root, minute)
            .map_err(|_| GenerationError::Store { what: "layout" })?;
        let mut due = BTreeSet::new();
        for instrument in instruments {
            if !minute_dir(&generation, instrument.symbol.as_str())
                .map_err(|_| GenerationError::Store { what: "layout" })?
                .exists()
            {
                due.insert(instrument.symbol.clone());
            }
        }
        if due.is_empty() {
            return Ok(Vec::new());
        }
        let start_secs = minute
            .secs()
            .checked_sub(MINUTE_WINDOW_S)
            .ok_or(TimeError::OutsideCalendar)?;
        let start = UtcNanos::from_parts(start_secs, 0)?;
        let batch = BarsBatch::new(due.clone(), timeframe(TimeUnit::Minute)?, start, minute)
            .map_err(GenerationError::Batch)?;
        let mut bars = source.bars(&batch).map_err(GenerationError::Fetch)?;
        let mut written = Vec::new();
        for symbol in due {
            let by_day = by_day(bars.remove(&symbol).unwrap_or_default());
            let mut days: Vec<Date> = by_day.keys().copied().collect();
            if days.is_empty() {
                days.push(start.date());
            }
            self.publish(
                &format!("minute-{}-{}", minute.secs(), symbol.as_str()),
                &generation,
                &symbol,
                TimeUnit::Minute,
                &days,
                by_day,
                None,
            )?;
            written.push(symbol);
        }
        Ok(written)
    }

    /// Puts the closes object of each instrument whose dataset is in the daily generation of
    /// `session` and trusted at `now`, once for each: the bytes the runtime's composer derives from
    /// the same generation (DEC-2283 item 6). Returns each asset id put with its object's reference.
    /// An instrument whose dataset is not there, or is refused, is tried again at the next call.
    pub fn closes(
        &mut self,
        session: Date,
        instruments: &[Instrument],
        now: UtcNanos,
    ) -> Vec<(String, ArtifactRef)> {
        let generation = daily_generation(&self.root, session);
        let mut put = Vec::new();
        for instrument in instruments {
            let key = (session, instrument.asset_id.clone());
            if self.put.contains(&key) {
                continue;
            }
            let Ok(dir) = daily_dir(&generation, instrument.symbol.as_str()) else {
                continue;
            };
            if !dir.join(MANIFEST).is_file() {
                continue;
            }
            if let Ok(reference) = put_closes(
                &self.artifacts,
                &dir,
                instrument.symbol.as_str(),
                &instrument.asset_id,
                now,
            ) {
                self.put.insert(key);
                put.push((instrument.asset_id.clone(), reference));
            }
        }
        put
    }

    /// Removes the daily generations older than the session before `now`'s last completed one, and
    /// the minute generations older than [`MINUTE_RETENTION_S`] before `now`'s minute. A directory
    /// whose name is no generation's is left as it is.
    ///
    /// # Errors
    /// [`GenerationError`] when the calendar cannot name the sessions or a directory cannot be read
    /// or removed.
    pub fn prune(&self, calendar: &ExchangeCalendar, now: UtcNanos) -> Result<(), GenerationError> {
        if let Some(session) = calendar.last_completed_regular_session(now)? {
            let days = span(calendar, session)?;
            let keep_from = days
                .len()
                .checked_sub(2)
                .and_then(|at| days.get(at))
                .copied()
                .unwrap_or(session);
            for (name, path) in entries(&self.root.join("daily"))? {
                if Date::parse(&name).is_ok_and(|d| d < keep_from) {
                    fs::remove_dir_all(&path).map_err(store("prune"))?;
                }
            }
        }
        let into = now.secs().rem_euclid(MINUTE_S);
        let oldest = now
            .secs()
            .checked_sub(into)
            .and_then(|minute| minute.checked_sub(MINUTE_RETENTION_S))
            .ok_or(TimeError::OutsideCalendar)?;
        for (name, path) in entries(&self.root.join("minutes"))? {
            if minute_named(&name).is_some_and(|secs| secs < oldest) {
                fs::remove_dir_all(&path).map_err(store("prune"))?;
            }
        }
        Ok(())
    }

    /// Builds `symbol`'s dataset of `unit` bars under the staging as `label`, one partition for
    /// each of `days` from `by_day` and `actions` beside them, and renames it into `generation`.
    #[allow(
        clippy::too_many_arguments,
        reason = "each argument is one part of the one dataset the staging builds"
    )]
    fn publish(
        &self,
        label: &str,
        generation: &Path,
        symbol: &Symbol,
        unit: TimeUnit,
        days: &[Date],
        mut by_day: BTreeMap<Date, Vec<Bar>>,
        actions: Option<&RecordedActions>,
    ) -> Result<(), GenerationError> {
        let staging = self.root.join(STAGING).join(label);
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(store("staging"))?;
        }
        let id = DatasetId::new(
            AssetClass::UsEquity,
            Feed::Iex,
            Kind::Bars(timeframe(unit)?),
            symbol.clone(),
        )
        .map_err(|_| GenerationError::Store { what: "dataset" })?;
        let built = Store::new(&staging);
        for day in days {
            let bars = by_day.remove(day).unwrap_or_default();
            built
                .put_day(&id, *day, &Records::Bars(bars))
                .map_err(|_| GenerationError::Store { what: "partition" })?;
        }
        let dir = built.dataset_dir(&id);
        if let Some(recorded) = actions {
            write_actions(&dir, recorded)
                .map_err(|_| GenerationError::Store { what: "actions" })?;
        }
        let target = Store::new(generation).dataset_dir(&id);
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(store("generation"))?;
        }
        fs::rename(&dir, &target).map_err(store("publish"))?;
        fs::remove_dir_all(&staging).map_err(store("staging"))?;
        let _left_for_a_concurrent_build = fs::remove_dir(self.root.join(STAGING));
        Ok(())
    }
}

fn timeframe(unit: TimeUnit) -> Result<Timeframe, GenerationError> {
    Timeframe::new(1, unit).map_err(|_| GenerationError::Store { what: "timeframe" })
}

/// `bars` by the UTC date each starts on, in time order.
fn by_day(bars: Vec<Bar>) -> BTreeMap<Date, Vec<Bar>> {
    let mut days: BTreeMap<Date, Vec<Bar>> = BTreeMap::new();
    for bar in bars {
        days.entry(bar.start.date()).or_default().push(bar);
    }
    days
}

/// The name and path of each directory directly under `dir`; none when `dir` does not exist.
fn entries(dir: &Path) -> Result<Vec<(String, PathBuf)>, GenerationError> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).map_err(store("read"))? {
        let entry = entry.map_err(store("read"))?;
        if let Some(name) = entry.file_name().to_str() {
            found.push((name.to_owned(), entry.path()));
        }
    }
    Ok(found)
}

/// The start, in seconds, of the minute a generation's name `YYYY-MM-DDTHHMM` names.
fn minute_named(name: &str) -> Option<i64> {
    let (date, time) = name.split_once('T')?;
    if time.len() != 4 || !time.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let text = format!("{date}T{}:{}:00Z", time.get(0..2)?, time.get(2..4)?);
    UtcNanos::parse_rfc3339(&text).ok().map(UtcNanos::secs)
}
