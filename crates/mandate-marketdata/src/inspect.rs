//! `inspect` (backlog E2-2, E2-4): what a stored dataset covers and whether to trust it. It reads
//! the manifest and every partition it lists, and reports the days listed, empty, and never
//! fetched; exact statistics, with prices also split-adjusted by the corporate actions stored
//! with a stock dataset, and for a quotes dataset each quoted side's extremes, the signed spread,
//! and the locked, crossed, one-sided, and unquoted counts (DEC-116); gaps between consecutive
//! bars, each missing bar slot classified as a session closure, no trade, a true gap, or
//! unclassified (trading domain spec §4.2); records that share a key; records stamped while the
//! venue is closed, zero-volume bars, and single-trade bars whose prices differ, as warnings; and
//! every partition that is missing, altered, unreadable, not listed, or holds a bar whose open or
//! close lies outside its range.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use mandate_canon::{DecStr, Digest};
use mandate_time::{Date, TimeError, UtcNanos};

use crate::actions::{ActionsError, RecordedActions, read_actions};
use crate::dataset::{self, BAR_SCALE, DatasetError, ListedDay, PRICE_SCALE, SIZE_SCALE};
use crate::model::{
    AdjustmentError, AssetClass, Bar, DatasetId, DayRange, Kind, ModelError, PriceAdjuster,
    Records, TimeUnit, Timeframe,
};
use crate::number::{self, NumberError};
use crate::venue::{States, Venue, VenueError, VenueState};

#[derive(Debug, thiserror::Error)]
pub enum InspectError {
    #[error(transparent)]
    Dataset(#[from] DatasetError),
    #[error(transparent)]
    Actions(#[from] ActionsError),
    #[error("venue hours: {0}")]
    Venue(#[from] VenueError),
    #[error("split adjustment: {0}")]
    Adjustment(#[from] AdjustmentError),
    #[error("column `{column}`: {source}")]
    Number {
        column: &'static str,
        source: NumberError,
    },
    #[error("the total of `{column}` does not fit 38 digits")]
    Overflow { column: &'static str },
    #[error("time arithmetic: {0}")]
    Time(#[from] TimeError),
    #[error("day range: {0}")]
    Range(#[from] ModelError),
}

impl InspectError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::Dataset(e) => e.code(),
            Self::Actions(e) => e.code(),
            Self::Venue(_) => "venue",
            Self::Adjustment(_) => "adjustment",
            Self::Number { .. } => "number",
            Self::Overflow { .. } => "overflow",
            Self::Time(_) => "time",
            Self::Range(_) => "range",
        }
    }
}

/// Everything `inspect` found in one dataset directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inspection {
    pub dataset: DatasetId,
    pub coverage: Coverage,
    /// Over every record of the partitions without problems; `None` when there are none.
    pub stats: Option<Stats>,
    pub corporate_actions: ActionsReport,
    /// Between consecutive bars in time order, across partitions; always empty for trades.
    pub gaps: Vec<ClassifiedGap>,
    /// In key order.
    pub duplicates: Vec<Duplicate>,
    /// Over the records of the partitions without problems.
    pub quality: Quality,
    /// Listed days first, in date order, then unlisted files by name.
    pub problems: Vec<Problem>,
}

/// The days the manifest lists, from its first listed day to its last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// `None` when the manifest lists no day.
    pub span: Option<DayRange>,
    pub listed: u64,
    /// Runs of listed days without records on which the venue trades or may trade.
    pub empty: Vec<DayRange>,
    /// Listed days without records on which the venue is closed all day: weekends and holidays,
    /// counted rather than listed.
    pub closed: u64,
    /// Runs of days inside the span that the manifest does not list: never fetched.
    pub missing: Vec<DayRange>,
}

/// How many first examples an [`Occurrences`] keeps.
pub const EXAMPLES: usize = 5;

/// How many records show one finding, and the earliest [`EXAMPLES`] distinct times among them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Occurrences {
    pub count: u64,
    /// Ascending.
    pub first: Vec<UtcNanos>,
}

impl Occurrences {
    fn add(&mut self, at: UtcNanos) {
        self.count = self.count.saturating_add(1);
        if let Err(index) = self.first.binary_search(&at) {
            self.first.insert(index, at);
            self.first.truncate(EXAMPLES);
        }
    }
}

/// What the vendor sent that a reader should weigh before using it, without making a partition
/// untrusted: storage keeps the vendor's records (DEC-89), and each of these still has the fields
/// and the price order of trading domain spec §4.1, so they are warnings rather than
/// [`Problem`]s and leave `inspect`'s exit status alone.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Quality {
    /// Records stamped while the feed's venue is closed: a bar by its start, a daily bar by its
    /// day, a trade or quote by its time. A bar starting at the venue's close is one of them.
    pub closed_period: Occurrences,
    /// Bars with zero volume, which §4.2 says do not exist ("a bar exists only when trades
    /// occur"); always empty for trades and quotes.
    pub zero_volume: Occurrences,
    /// Bars with a trade count of one whose prices are not all equal; always empty for trades
    /// and quotes.
    pub single_trade_spread: Occurrences,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    pub rows: u64,
    pub first: UtcNanos,
    pub last: UtcNanos,
    /// Raw, as stored (DEC-89).
    pub values: Values,
    /// Split-adjusted as of the span's last day; `Some` exactly when the corporate actions are
    /// [`ActionsReport::Applied`] and at least one price is quoted.
    pub adjusted: Option<AdjustedPrices>,
}

/// The extremes of a dataset's prices (bar lows and highs, trade prices, or the quoted sides of
/// quotes), each split-adjusted point in time to the terms of one date (spec §4.5, §8.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdjustedPrices {
    pub low: DecStr,
    pub high: DecStr,
}

/// The corporate actions stored with a dataset and whether its prices are adjusted by them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionsReport {
    /// Crypto has no corporate actions.
    NotApplicable,
    /// A stock dataset without them: stored before E2-4, or not by `download`.
    NotRecorded,
    /// Recorded for days that do not cover the span, so no price is adjusted.
    Incomplete(RecordedActions),
    /// Recorded for the whole span: every split with ex-date up to `as_of`, the span's last day,
    /// adjusts [`Stats::adjusted`]; later splits, cash dividends, and other actions do not.
    Applied {
        recorded: RecordedActions,
        as_of: Date,
    },
}

/// The lowest and highest price of one quoted side of a quote, over the rows that quote it
/// (DEC-116).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extent {
    pub low: DecStr,
    pub high: DecStr,
    pub rows: u64,
}

/// The ask minus the bid over the rows quoting both sides, keeping its sign: a crossed quote's
/// spread is negative, so `narrowest` may be below zero (DEC-116). No mean is reported, because
/// the quotient of two exact decimals is not one; `total` and `rows` give it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spread {
    pub narrowest: DecStr,
    pub widest: DecStr,
    pub total: DecStr,
    pub rows: u64,
}

/// Exact extremes and totals of the decimal columns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Values {
    Bars {
        low: DecStr,
        high: DecStr,
        volume: DecStr,
        trade_count: u64,
    },
    Trades {
        low: DecStr,
        high: DecStr,
        /// Over every stored trade. A venue's official open or close that the tape reports again
        /// under another condition counts each time, so a stock's total can exceed its bars'
        /// volume; the trading domain spec defines no trade conditions to exclude.
        size: DecStr,
    },
    /// A side whose price is zero holds no order, so it is left out of `bid`, `ask`, and `spread`,
    /// and counted instead (DEC-116); [`Stats::rows`] still counts every stored row.
    Quotes {
        /// `None` when no row quotes a bid.
        bid: Option<Extent>,
        /// `None` when no row quotes an ask.
        ask: Option<Extent>,
        /// `None` when no row quotes both sides.
        spread: Option<Spread>,
        /// Rows quoting both sides at the same price.
        locked: u64,
        /// Rows quoting both sides with the bid above the ask.
        crossed: u64,
        /// Rows quoting exactly one side.
        one_sided: u64,
        /// Rows quoting neither side.
        unquoted: u64,
    },
}

/// Two consecutive bars further apart than one timeframe (for `1Day`, a UTC calendar day is
/// skipped): no bar starts strictly between `previous` and `next`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Gap {
    pub previous: UtcNanos,
    pub next: UtcNanos,
}

/// Why an expected bar slot has no bar (brief interpretation 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GapClass {
    /// The feed's venue is closed at the slot's start.
    SessionClosure,
    /// The venue is open and the slot's day was fetched cleanly: nothing traded.
    NoTrade,
    /// The venue is open and the slot's day was never fetched or cannot be trusted.
    TrueGap,
    /// The venue's published hours do not say whether it is open.
    Unclassified,
}

impl GapClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SessionClosure => "session closure",
            Self::NoTrade => "no trade",
            Self::TrueGap => "true gap",
            Self::Unclassified => "unclassified",
        }
    }
}

/// A run of consecutive missing slots of one class: the starts of the first and last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stretch {
    pub class: GapClass,
    pub first: UtcNanos,
    pub last: UtcNanos,
    pub slots: u64,
}

/// A gap and its missing slots, in time order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifiedGap {
    pub gap: Gap,
    pub stretches: Vec<Stretch>,
}

/// Records sharing a key: a bar's start, a trade's time, ID, exchange, and tape, or a quote's
/// time and two exchange codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Duplicate {
    pub time: UtcNanos,
    /// The shared trade ID; `None` for bars.
    pub trade_id: Option<u64>,
    pub count: u64,
    /// Whether every record with the key is the same in every field.
    pub identical: bool,
}

/// A partition that cannot be trusted; its records are left out of everything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Missing {
        day: Date,
    },
    /// Its size or SHA-256 is not what the manifest records.
    Altered {
        day: Date,
    },
    Unreadable {
        day: Date,
        reason: String,
    },
    RowCount {
        day: Date,
        listed: u64,
        read: u64,
    },
    OutsideDay {
        day: Date,
        records: u64,
    },
    /// A `.parquet` file in the directory that the manifest does not list.
    Unlisted {
        file: String,
    },
    /// Bars whose open or close lies outside their low-to-high range, which no bar of trades can
    /// have (trading domain spec §4.1).
    InconsistentBars {
        day: Date,
        bars: u64,
    },
}

/// Inspects the dataset stored in `dir`. A missing or invalid manifest is an error; a bad
/// partition is a [`Problem`] in the result.
pub fn inspect(dir: &Path) -> Result<Inspection, InspectError> {
    let (dataset, days) = dataset::read_manifest(dir)?;
    let venue = Venue::of(dataset.feed())?;
    let coverage = coverage(&days, &venue)?;
    let corporate_actions = actions_report(dir, &dataset, coverage.span)?;
    let adjuster = match &corporate_actions {
        ActionsReport::Applied { recorded, as_of } => {
            Some(recorded.actions.price_adjuster(*as_of)?)
        }
        ActionsReport::NotApplicable
        | ActionsReport::NotRecorded
        | ActionsReport::Incomplete(_) => None,
    };
    let mut totals = Totals::new(dataset.kind(), adjuster);
    let mut problems = Vec::new();
    let mut states = venue.states();
    let daily = matches!(dataset.kind(), Kind::Bars(t) if t.unit() == TimeUnit::Day);
    for listed in days.iter().filter(|d| d.file.is_some()) {
        let records = match load(dir, listed, dataset.kind()) {
            Ok(records) => records,
            Err(problem) => {
                problems.push(problem);
                continue;
            }
        };
        match inconsistent_bars(&records)? {
            0 => {}
            bars => {
                problems.push(Problem::InconsistentBars {
                    day: listed.day,
                    bars,
                });
                continue;
            }
        }
        for at in records.times() {
            if closed_at(&venue, &mut states, daily, at)? {
                totals.quality.closed_period.add(at);
            }
        }
        totals.add(&records)?;
    }
    problems.extend(unlisted(dir, &days)?);
    let untrusted: BTreeSet<Date> = problems.iter().filter_map(Problem::day).collect();
    let clean: BTreeSet<Date> = days
        .iter()
        .map(|d| d.day)
        .filter(|day| !untrusted.contains(day))
        .collect();
    let mut gaps = Vec::new();
    if let (Kind::Bars(timeframe), Some(finder)) = (dataset.kind(), totals.gaps.take()) {
        for gap in finder.gaps {
            gaps.push(ClassifiedGap {
                gap,
                stretches: classify(timeframe, gap, &venue, &clean)?,
            });
        }
    }
    Ok(Inspection {
        coverage,
        stats: totals.stats()?,
        corporate_actions,
        gaps,
        duplicates: totals.duplicates,
        quality: totals.quality,
        problems,
        dataset,
    })
}

/// Whether the venue is closed at a record's time; a daily bar is closed when its whole day is.
fn closed_at(
    venue: &Venue,
    states: &mut States<'_>,
    daily: bool,
    at: UtcNanos,
) -> Result<bool, InspectError> {
    let state = if daily {
        venue.day_state(at.date())?
    } else {
        states.at(at)?
    };
    Ok(state == VenueState::Closed)
}

/// The number of bars whose open or close lies outside their low-to-high range; zero for trades
/// and quotes.
fn inconsistent_bars(records: &Records) -> Result<u64, InspectError> {
    let bars = match records {
        Records::Bars(bars) => bars,
        Records::Trades(_) | Records::Quotes(_) => return Ok(0),
    };
    let mut count = 0_u64;
    for bar in bars {
        let low = units("low", &bar.low, BAR_SCALE)?;
        let high = units("high", &bar.high, BAR_SCALE)?;
        let within = |column, value| -> Result<bool, InspectError> {
            let value = units(column, value, BAR_SCALE)?;
            Ok(low <= value && value <= high)
        };
        if !(within("open", &bar.open)? && within("close", &bar.close)?) {
            count = count.saturating_add(1);
        }
    }
    Ok(count)
}

/// The missing slots of `gap` between bars of `timeframe`, in runs of one class: a slot is a
/// session closure or unclassified by the venue's state at its start, and otherwise no trade when
/// its UTC day is in `clean` (listed and without problems) and a true gap when not. For `1Day`,
/// the slots are the calendar days strictly between the bars' days, classified by the whole day.
pub fn classify(
    timeframe: Timeframe,
    gap: Gap,
    venue: &Venue,
    clean: &BTreeSet<Date>,
) -> Result<Vec<Stretch>, InspectError> {
    let daily = timeframe.unit() == TimeUnit::Day;
    let mut states = venue.states();
    let mut stretches: Vec<Stretch> = Vec::new();
    let mut slot = one_bar_later(timeframe, gap.previous)?;
    loop {
        let missing = if daily {
            slot.date() < gap.next.date()
        } else {
            slot < gap.next
        };
        if !missing {
            return Ok(stretches);
        }
        let state = if daily {
            venue.day_state(slot.date())?
        } else {
            states.at(slot)?
        };
        let class = match state {
            VenueState::Closed => GapClass::SessionClosure,
            VenueState::Unclassified => GapClass::Unclassified,
            VenueState::Open if clean.contains(&slot.date()) => GapClass::NoTrade,
            VenueState::Open => GapClass::TrueGap,
        };
        match stretches.last_mut() {
            Some(run) if run.class == class => {
                run.last = slot;
                run.slots = run.slots.saturating_add(1);
            }
            _ => stretches.push(Stretch {
                class,
                first: slot,
                last: slot,
                slots: 1,
            }),
        }
        slot = one_bar_later(timeframe, slot)?;
    }
}

fn actions_report(
    dir: &Path,
    dataset: &DatasetId,
    span: Option<DayRange>,
) -> Result<ActionsReport, InspectError> {
    match dataset.asset_class() {
        AssetClass::Crypto => return Ok(ActionsReport::NotApplicable),
        AssetClass::UsEquity => {}
    }
    let Some(recorded) = read_actions(dir, dataset.symbol())? else {
        return Ok(ActionsReport::NotRecorded);
    };
    Ok(match span {
        Some(span)
            if recorded.range.first() <= span.first() && span.last() <= recorded.range.last() =>
        {
            ActionsReport::Applied {
                recorded,
                as_of: span.last(),
            }
        }
        _ => ActionsReport::Incomplete(recorded),
    })
}

/// The gaps between bars of `timeframe` starting at `starts`, given in any order; repeated
/// starts count once.
pub fn gaps(timeframe: Timeframe, starts: &[UtcNanos]) -> Result<Vec<Gap>, InspectError> {
    let mut finder = GapFinder::new(timeframe);
    for start in starts.iter().copied().collect::<BTreeSet<_>>() {
        finder.push(start)?;
    }
    Ok(finder.gaps)
}

impl Problem {
    /// The listed day the problem is with; `None` for an unlisted file.
    fn day(&self) -> Option<Date> {
        match self {
            Self::Missing { day }
            | Self::Altered { day }
            | Self::Unreadable { day, .. }
            | Self::RowCount { day, .. }
            | Self::OutsideDay { day, .. }
            | Self::InconsistentBars { day, .. } => Some(*day),
            Self::Unlisted { .. } => None,
        }
    }
}

fn load(dir: &Path, listed: &ListedDay, kind: Kind) -> Result<Records, Problem> {
    let day = listed.day;
    let path = dir.join(dataset::partition_name(day));
    let bytes = match fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == ErrorKind::NotFound => return Err(Problem::Missing { day }),
        Err(e) => {
            return Err(Problem::Unreadable {
                day,
                reason: e.to_string(),
            });
        }
    };
    let file = u64::try_from(bytes.len())
        .ok()
        .map(|size| (size, Digest::of(&bytes)));
    if file != listed.file {
        return Err(Problem::Altered { day });
    }
    let records = dataset::read(&path, kind).map_err(|e| Problem::Unreadable {
        day,
        reason: e.to_string(),
    })?;
    let read = u64::try_from(records.len()).unwrap_or(u64::MAX);
    if read != listed.rows {
        return Err(Problem::RowCount {
            day,
            listed: listed.rows,
            read,
        });
    }
    let outside = records.times().iter().filter(|t| t.date() != day).count();
    if outside > 0 {
        return Err(Problem::OutsideDay {
            day,
            records: u64::try_from(outside).unwrap_or(u64::MAX),
        });
    }
    Ok(records)
}

fn unlisted(dir: &Path, days: &[ListedDay]) -> Result<Vec<Problem>, InspectError> {
    let listed: BTreeSet<String> = days
        .iter()
        .filter(|d| d.file.is_some())
        .map(|d| dataset::partition_name(d.day))
        .collect();
    let io = |source| DatasetError::Io {
        path: dir.to_path_buf(),
        source,
    };
    let mut files = BTreeSet::new();
    for entry in fs::read_dir(dir).map_err(io)? {
        let name = entry
            .map_err(io)?
            .file_name()
            .to_string_lossy()
            .into_owned();
        if name.ends_with(".parquet") && !listed.contains(&name) {
            files.insert(name);
        }
    }
    Ok(files
        .into_iter()
        .map(|file| Problem::Unlisted { file })
        .collect())
}

fn coverage(days: &[ListedDay], venue: &Venue) -> Result<Coverage, InspectError> {
    let (Some(first), Some(last)) = (days.first(), days.last()) else {
        return Ok(Coverage {
            span: None,
            listed: 0,
            empty: Vec::new(),
            closed: 0,
            missing: Vec::new(),
        });
    };
    let span = DayRange::new(first.day, last.day)?;
    let rows: BTreeMap<Date, u64> = days.iter().map(|d| (d.day, d.rows)).collect();
    let (mut empty, mut missing) = (Vec::new(), Vec::new());
    let mut closed = 0_u64;
    for date in span.days()? {
        match rows.get(&date) {
            None => missing.push(date),
            Some(0) if venue.day_state(date)? == VenueState::Closed => {
                closed = closed.saturating_add(1);
            }
            Some(0) => empty.push(date),
            Some(_) => {}
        }
    }
    Ok(Coverage {
        span: Some(span),
        listed: u64::try_from(days.len()).unwrap_or(u64::MAX),
        empty: runs(empty)?,
        closed,
        missing: runs(missing)?,
    })
}

/// Ascending dates grouped into runs of consecutive days.
fn runs(dates: Vec<Date>) -> Result<Vec<DayRange>, InspectError> {
    let mut runs: Vec<(Date, Date)> = Vec::new();
    for date in dates {
        match runs.last_mut() {
            Some((_, last)) if last.next()? == date => *last = date,
            _ => runs.push((date, date)),
        }
    }
    runs.into_iter()
        .map(|(first, last)| DayRange::new(first, last).map_err(InspectError::from))
        .collect()
}

struct GapFinder {
    timeframe: Timeframe,
    previous: Option<UtcNanos>,
    gaps: Vec<Gap>,
}

impl GapFinder {
    fn new(timeframe: Timeframe) -> Self {
        Self {
            timeframe,
            previous: None,
            gaps: Vec::new(),
        }
    }

    /// `start` must be later than every start pushed before.
    fn push(&mut self, start: UtcNanos) -> Result<(), InspectError> {
        if let Some(previous) = self.previous
            && skips_a_slot(self.timeframe, previous, start)?
        {
            self.gaps.push(Gap {
                previous,
                next: start,
            });
        }
        self.previous = Some(start);
        Ok(())
    }
}

fn skips_a_slot(
    timeframe: Timeframe,
    previous: UtcNanos,
    next: UtcNanos,
) -> Result<bool, InspectError> {
    if timeframe.unit() == TimeUnit::Day {
        return Ok(next.date() > previous.date().next()?);
    }
    Ok(next > one_bar_later(timeframe, previous)?)
}

/// The start of the bar slot after the one starting at `start`.
fn one_bar_later(timeframe: Timeframe, start: UtcNanos) -> Result<UtcNanos, InspectError> {
    let unit_secs: i64 = match timeframe.unit() {
        TimeUnit::Minute => 60,
        TimeUnit::Hour => 3_600,
        TimeUnit::Day => 86_400,
    };
    let secs = i64::from(timeframe.amount())
        .checked_mul(unit_secs)
        .and_then(|step| start.secs().checked_add(step))
        .ok_or(TimeError::OutOfRange)?;
    Ok(UtcNanos::from_parts(secs, start.nanos())?)
}

/// What makes two records the same record: a bar's start, a trade's time, ID, exchange, and tape,
/// or a quote's time and its bid and ask exchange codes.
type Key<'a> = (UtcNanos, Option<u64>, Option<&'a str>, Option<&'a str>);

fn duplicates<'a, T: PartialEq>(
    records: &'a [T],
    key: impl Fn(&'a T) -> Key<'a>,
) -> Vec<Duplicate> {
    let mut sorted: Vec<&'a T> = records.iter().collect();
    sorted.sort_by(|a, b| key(a).cmp(&key(b)));
    sorted
        .chunk_by(|a, b| key(a) == key(b))
        .filter_map(|group| match group {
            [first, _, ..] => {
                let (time, trade_id, _, _) = key(first);
                Some(Duplicate {
                    time,
                    trade_id,
                    count: u64::try_from(group.len()).unwrap_or(u64::MAX),
                    identical: group.iter().all(|r| r == first),
                })
            }
            _ => None,
        })
        .collect()
}

/// Whether a bar's prices are not all equal. Only bars inside their own range are summarized, and
/// such a bar's prices are all equal exactly when its low equals its high.
fn prices_differ(bar: &Bar) -> Result<bool, InspectError> {
    Ok(units("low", &bar.low, BAR_SCALE)? != units("high", &bar.high, BAR_SCALE)?)
}

fn units(column: &'static str, value: &DecStr, scale: u8) -> Result<i128, InspectError> {
    number::to_units(value, scale).map_err(|source| InspectError::Number { column, source })
}

fn decimal(column: &'static str, units: i128, scale: u8) -> Result<DecStr, InspectError> {
    number::from_units(units, scale).map_err(|source| InspectError::Number { column, source })
}

fn sum(column: &'static str, total: i128, units: i128) -> Result<i128, InspectError> {
    total
        .checked_add(units)
        .ok_or(InspectError::Overflow { column })
}

/// The lowest and highest of some prices, in units of one scale.
#[derive(Debug, Default)]
struct Extremes {
    low: Option<i128>,
    high: Option<i128>,
}

impl Extremes {
    fn include(&mut self, low: i128, high: i128) {
        self.low = Some(self.low.map_or(low, |l| l.min(low)));
        self.high = Some(self.high.map_or(high, |h| h.max(high)));
    }
}

/// Whether a side of a quote holds an order. Alpaca sends a zero price, a zero size, and a blank
/// exchange for the side with none, so a zero is an absent order and not a price of zero; it is
/// counted, never reported as an extreme or a spread (DEC-116). Any other value, a negative one
/// included, is summarized as stored, because storage keeps what the vendor sent (DEC-89).
fn quoted(units: i128) -> bool {
    units != 0
}

/// The sides, spreads, and classes of a quotes dataset, in units of [`PRICE_SCALE`] (DEC-116).
#[derive(Debug, Default)]
struct QuoteTotals {
    bids: Extremes,
    bid_rows: u64,
    asks: Extremes,
    ask_rows: u64,
    /// The narrowest and widest ask minus bid, which a crossed quote makes negative.
    spreads: Extremes,
    spread_total: i128,
    two_sided: u64,
    locked: u64,
    crossed: u64,
    one_sided: u64,
    unquoted: u64,
}

impl QuoteTotals {
    fn add(&mut self, bid: i128, ask: i128) -> Result<(), InspectError> {
        if quoted(bid) {
            self.bids.include(bid, bid);
            self.bid_rows = self.bid_rows.saturating_add(1);
        }
        if quoted(ask) {
            self.asks.include(ask, ask);
            self.ask_rows = self.ask_rows.saturating_add(1);
        }
        match (quoted(bid), quoted(ask)) {
            (true, true) => {
                let spread = ask
                    .checked_sub(bid)
                    .ok_or(InspectError::Overflow { column: "spread" })?;
                self.spreads.include(spread, spread);
                self.spread_total = sum("spread", self.spread_total, spread)?;
                self.two_sided = self.two_sided.saturating_add(1);
                if bid == ask {
                    self.locked = self.locked.saturating_add(1);
                } else if bid > ask {
                    self.crossed = self.crossed.saturating_add(1);
                }
            }
            (true, false) | (false, true) => self.one_sided = self.one_sided.saturating_add(1),
            (false, false) => self.unquoted = self.unquoted.saturating_add(1),
        }
        Ok(())
    }

    fn values(&self) -> Result<Values, InspectError> {
        Ok(Values::Quotes {
            bid: extent("bid_price", &self.bids, self.bid_rows)?,
            ask: extent("ask_price", &self.asks, self.ask_rows)?,
            spread: self.spread()?,
            locked: self.locked,
            crossed: self.crossed,
            one_sided: self.one_sided,
            unquoted: self.unquoted,
        })
    }

    fn spread(&self) -> Result<Option<Spread>, InspectError> {
        let Some((narrowest, widest)) = self.spreads.low.zip(self.spreads.high) else {
            return Ok(None);
        };
        Ok(Some(Spread {
            narrowest: decimal("spread", narrowest, PRICE_SCALE)?,
            widest: decimal("spread", widest, PRICE_SCALE)?,
            total: decimal("spread", self.spread_total, PRICE_SCALE)?,
            rows: self.two_sided,
        }))
    }
}

/// One side's extremes as exact decimals; `None` when no row quotes the side.
fn extent(
    column: &'static str,
    seen: &Extremes,
    rows: u64,
) -> Result<Option<Extent>, InspectError> {
    let Some((low, high)) = seen.low.zip(seen.high) else {
        return Ok(None);
    };
    Ok(Some(Extent {
        low: decimal(column, low, PRICE_SCALE)?,
        high: decimal(column, high, PRICE_SCALE)?,
        rows,
    }))
}

/// Running statistics, gaps, and duplicates over partitions added in date order.
struct Totals {
    kind: Kind,
    rows: u64,
    first: Option<UtcNanos>,
    last: Option<UtcNanos>,
    raw: Extremes,
    /// At [`BAR_SCALE`] for bars and trades alike: adjusted prices carry up to
    /// [`crate::model::ADJUSTED_PRICE_SCALE`] places.
    adjusted: Extremes,
    adjuster: Option<PriceAdjuster>,
    /// Bar volume or trade size, in units of its column's scale.
    quantity: i128,
    trade_count: u64,
    /// Only a quotes dataset fills it; quote sizes are not totalled, because the top of the book
    /// is a standing offer, not traded volume (DEC-116).
    quotes: QuoteTotals,
    gaps: Option<GapFinder>,
    duplicates: Vec<Duplicate>,
    quality: Quality,
}

impl Totals {
    fn new(kind: Kind, adjuster: Option<PriceAdjuster>) -> Self {
        Self {
            kind,
            rows: 0,
            first: None,
            last: None,
            raw: Extremes::default(),
            adjusted: Extremes::default(),
            adjuster,
            quantity: 0,
            trade_count: 0,
            quotes: QuoteTotals::default(),
            gaps: match kind {
                Kind::Bars(timeframe) => Some(GapFinder::new(timeframe)),
                Kind::Trades => None,
                Kind::Quotes => None,
            },
            duplicates: Vec::new(),
            quality: Quality::default(),
        }
    }

    /// Counts one record and widens the span; every kind calls it once per row.
    fn row(&mut self, time: UtcNanos) {
        self.rows = self.rows.saturating_add(1);
        self.first = Some(self.first.map_or(time, |t| t.min(time)));
        self.last = Some(self.last.map_or(time, |t| t.max(time)));
    }

    fn record(
        &mut self,
        time: UtcNanos,
        (low_column, low): (&'static str, &DecStr),
        (high_column, high): (&'static str, &DecStr),
        scale: u8,
    ) -> Result<(), InspectError> {
        self.row(time);
        self.raw.include(
            units(low_column, low, scale)?,
            units(high_column, high, scale)?,
        );
        if let Some(adjuster) = &self.adjuster {
            let low = units(low_column, &adjuster.adjust(low, time)?, BAR_SCALE)?;
            let high = units(high_column, &adjuster.adjust(high, time)?, BAR_SCALE)?;
            self.adjusted.include(low, high);
        }
        Ok(())
    }

    /// One quoted price of a quote, split-adjusted into [`Stats::adjusted`]. A quotes dataset
    /// keeps its sides apart in [`Values::Quotes`], so only the adjusted envelope is shared.
    fn quoted_price(
        &mut self,
        column: &'static str,
        price: &DecStr,
        time: UtcNanos,
    ) -> Result<(), InspectError> {
        if let Some(adjuster) = &self.adjuster {
            let adjusted = units(column, &adjuster.adjust(price, time)?, BAR_SCALE)?;
            self.adjusted.include(adjusted, adjusted);
        }
        Ok(())
    }

    fn add(&mut self, records: &Records) -> Result<(), InspectError> {
        match records {
            Records::Bars(bars) => {
                for bar in bars {
                    self.record(bar.start, ("low", &bar.low), ("high", &bar.high), BAR_SCALE)?;
                    let volume = units("volume", &bar.volume, BAR_SCALE)?;
                    self.quantity = sum("volume", self.quantity, volume)?;
                    if volume == 0 {
                        self.quality.zero_volume.add(bar.start);
                    }
                    if bar.trade_count == 1 && prices_differ(bar)? {
                        self.quality.single_trade_spread.add(bar.start);
                    }
                    self.trade_count = self.trade_count.checked_add(bar.trade_count).ok_or(
                        InspectError::Overflow {
                            column: "trade_count",
                        },
                    )?;
                }
                if let Some(finder) = &mut self.gaps {
                    for start in bars.iter().map(|b| b.start).collect::<BTreeSet<_>>() {
                        finder.push(start)?;
                    }
                }
                self.duplicates
                    .extend(duplicates(bars, |b| (b.start, None, None, None)));
            }
            Records::Trades(trades) => {
                for trade in trades {
                    let price = ("price", &trade.price);
                    self.record(trade.time, price, price, PRICE_SCALE)?;
                    let size = units("size", &trade.size, SIZE_SCALE)?;
                    self.quantity = sum("size", self.quantity, size)?;
                }
                self.duplicates.extend(duplicates(trades, |t| {
                    (
                        t.time,
                        Some(t.trade_id),
                        t.exchange.as_deref(),
                        t.tape.as_deref(),
                    )
                }));
            }
            Records::Quotes(quotes) => {
                for quote in quotes {
                    self.row(quote.time);
                    let bid = units("bid_price", &quote.bid_price, PRICE_SCALE)?;
                    let ask = units("ask_price", &quote.ask_price, PRICE_SCALE)?;
                    self.quotes.add(bid, ask)?;
                    for (column, price, side) in [
                        ("bid_price", &quote.bid_price, bid),
                        ("ask_price", &quote.ask_price, ask),
                    ] {
                        if quoted(side) {
                            self.quoted_price(column, price, quote.time)?;
                        }
                    }
                }
                self.duplicates.extend(duplicates(quotes, |q| {
                    (
                        q.time,
                        None,
                        q.bid_exchange.as_deref(),
                        q.ask_exchange.as_deref(),
                    )
                }));
            }
        }
        Ok(())
    }

    fn stats(&self) -> Result<Option<Stats>, InspectError> {
        let (Some(first), Some(last)) = (self.first, self.last) else {
            return Ok(None);
        };
        let values = match (self.kind, self.raw.low.zip(self.raw.high)) {
            (Kind::Bars(_), Some((low, high))) => Values::Bars {
                low: decimal("low", low, BAR_SCALE)?,
                high: decimal("high", high, BAR_SCALE)?,
                volume: decimal("volume", self.quantity, BAR_SCALE)?,
                trade_count: self.trade_count,
            },
            (Kind::Trades, Some((low, high))) => Values::Trades {
                low: decimal("price", low, PRICE_SCALE)?,
                high: decimal("price", high, PRICE_SCALE)?,
                size: decimal("size", self.quantity, SIZE_SCALE)?,
            },
            (Kind::Quotes, _) => self.quotes.values()?,
            (Kind::Bars(_) | Kind::Trades, None) => return Ok(None),
        };
        let adjusted = match (self.adjusted.low, self.adjusted.high) {
            (Some(low), Some(high)) => Some(AdjustedPrices {
                low: decimal("low", low, BAR_SCALE)?,
                high: decimal("high", high, BAR_SCALE)?,
            }),
            _ => None,
        };
        Ok(Some(Stats {
            rows: self.rows,
            first,
            last,
            values,
            adjusted,
        }))
    }
}
