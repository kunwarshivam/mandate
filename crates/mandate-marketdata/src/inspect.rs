//! `inspect` (backlog E2-2, E2-4): what a stored dataset covers and whether to trust it. It reads
//! the manifest and every partition it lists, and reports the days listed, empty, and never
//! fetched; exact statistics, with prices also split-adjusted by the corporate actions stored
//! with a stock dataset; gaps between consecutive bars, each missing bar slot classified as a
//! session closure, no trade, a true gap, or unclassified (trading domain spec §4.2); records
//! that share a key; and every partition that is missing, altered, unreadable, or not listed.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use mandate_canon::{DecStr, Digest};
use mandate_time::{Date, TimeError, UtcNanos};

use crate::actions::{ActionsError, RecordedActions};
use crate::dataset::{self, BAR_SCALE, DatasetError, ListedDay, PRICE_SCALE, SIZE_SCALE};
use crate::model::{
    AdjustmentError, DatasetId, DayRange, Kind, ModelError, Records, TimeUnit, Timeframe,
};
use crate::number::{self, NumberError};
use crate::venue::{Venue, VenueError};

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
            Self::Actions(_) | Self::Venue(_) | Self::Adjustment(_) => "",
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
    /// Listed days first, in date order, then unlisted files by name.
    pub problems: Vec<Problem>,
}

/// The days the manifest lists, from its first listed day to its last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// `None` when the manifest lists no day.
    pub span: Option<DayRange>,
    pub listed: u64,
    /// Runs of listed days without records.
    pub empty: Vec<DayRange>,
    /// Runs of days inside the span that the manifest does not list: never fetched.
    pub missing: Vec<DayRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stats {
    pub rows: u64,
    pub first: UtcNanos,
    pub last: UtcNanos,
    /// Raw, as stored (DEC-89).
    pub values: Values,
    /// Split-adjusted as of the span's last day; `Some` exactly when the corporate actions are
    /// [`ActionsReport::Applied`].
    pub adjusted: Option<AdjustedPrices>,
}

/// The extremes of a dataset's prices (bar lows and highs, or trade prices), each split-adjusted
/// point in time to the terms of one date (spec §4.5, §8.5).
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
        size: DecStr,
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
        ""
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

/// Records sharing a key: a bar's start, or a trade's time, ID, exchange, and tape.
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
}

/// Inspects the dataset stored in `dir`. A missing or invalid manifest is an error; a bad
/// partition is a [`Problem`] in the result.
pub fn inspect(dir: &Path) -> Result<Inspection, InspectError> {
    let (dataset, days) = dataset::read_manifest(dir)?;
    let mut totals = Totals::new(dataset.kind());
    let mut problems = Vec::new();
    for listed in days.iter().filter(|d| d.file.is_some()) {
        match load(dir, listed, dataset.kind()) {
            Ok(records) => totals.add(&records)?,
            Err(problem) => problems.push(problem),
        }
    }
    problems.extend(unlisted(dir, &days)?);
    let gaps = totals
        .gaps
        .take()
        .map(|finder| finder.gaps)
        .unwrap_or_default()
        .into_iter()
        .map(|gap| ClassifiedGap {
            gap,
            stretches: Vec::new(),
        })
        .collect();
    Ok(Inspection {
        coverage: coverage(&days)?,
        stats: totals.stats()?,
        corporate_actions: ActionsReport::NotRecorded,
        gaps,
        duplicates: totals.duplicates,
        problems,
        dataset,
    })
}

/// The missing slots of `gap` between bars of `timeframe`, in runs of one class: a slot is a
/// session closure or unclassified by the venue's state at its start, and otherwise no trade when
/// its UTC day is in `clean` (listed and without problems) and a true gap when not. For `1Day`,
/// the slots are the calendar days strictly between the bars' days, classified by the whole day.
pub fn classify(
    _timeframe: Timeframe,
    _gap: Gap,
    _venue: &Venue,
    _clean: &BTreeSet<Date>,
) -> Result<Vec<Stretch>, InspectError> {
    Ok(Vec::new())
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

fn coverage(days: &[ListedDay]) -> Result<Coverage, InspectError> {
    let (Some(first), Some(last)) = (days.first(), days.last()) else {
        return Ok(Coverage {
            span: None,
            listed: 0,
            empty: Vec::new(),
            missing: Vec::new(),
        });
    };
    let span = DayRange::new(first.day, last.day)?;
    let rows: BTreeMap<Date, u64> = days.iter().map(|d| (d.day, d.rows)).collect();
    let (mut empty, mut missing) = (Vec::new(), Vec::new());
    for date in span.days()? {
        match rows.get(&date) {
            None => missing.push(date),
            Some(0) => empty.push(date),
            Some(_) => {}
        }
    }
    Ok(Coverage {
        span: Some(span),
        listed: u64::try_from(days.len()).unwrap_or(u64::MAX),
        empty: runs(empty)?,
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
    let unit_secs: i64 = match timeframe.unit() {
        TimeUnit::Minute => 60,
        TimeUnit::Hour => 3_600,
        TimeUnit::Day => return Ok(next.date() > previous.date().next()?),
    };
    let expected = i64::from(timeframe.amount())
        .checked_mul(unit_secs)
        .and_then(|step| previous.secs().checked_add(step))
        .ok_or(TimeError::OutOfRange)?;
    Ok(next > UtcNanos::from_parts(expected, previous.nanos())?)
}

/// What makes two records the same record: a bar's start, or a trade's time, ID, exchange, and
/// tape.
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

/// Running statistics, gaps, and duplicates over partitions added in date order.
struct Totals {
    kind: Kind,
    rows: u64,
    first: Option<UtcNanos>,
    last: Option<UtcNanos>,
    low: Option<i128>,
    high: Option<i128>,
    /// Bar volume or trade size, in units of its column's scale.
    quantity: i128,
    trade_count: u64,
    gaps: Option<GapFinder>,
    duplicates: Vec<Duplicate>,
}

impl Totals {
    fn new(kind: Kind) -> Self {
        Self {
            kind,
            rows: 0,
            first: None,
            last: None,
            low: None,
            high: None,
            quantity: 0,
            trade_count: 0,
            gaps: match kind {
                Kind::Bars(timeframe) => Some(GapFinder::new(timeframe)),
                Kind::Trades => None,
            },
            duplicates: Vec::new(),
        }
    }

    fn record(&mut self, time: UtcNanos, low: i128, high: i128) {
        self.rows = self.rows.saturating_add(1);
        self.first = Some(self.first.map_or(time, |t| t.min(time)));
        self.last = Some(self.last.map_or(time, |t| t.max(time)));
        self.low = Some(self.low.map_or(low, |l| l.min(low)));
        self.high = Some(self.high.map_or(high, |h| h.max(high)));
    }

    fn add(&mut self, records: &Records) -> Result<(), InspectError> {
        match records {
            Records::Bars(bars) => {
                for bar in bars {
                    let low = units("low", &bar.low, BAR_SCALE)?;
                    let high = units("high", &bar.high, BAR_SCALE)?;
                    self.record(bar.start, low, high);
                    let volume = units("volume", &bar.volume, BAR_SCALE)?;
                    self.quantity = sum("volume", self.quantity, volume)?;
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
                    let price = units("price", &trade.price, PRICE_SCALE)?;
                    self.record(trade.time, price, price);
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
        }
        Ok(())
    }

    fn stats(&self) -> Result<Option<Stats>, InspectError> {
        let (Some(first), Some(last), Some(low), Some(high)) =
            (self.first, self.last, self.low, self.high)
        else {
            return Ok(None);
        };
        let values = match self.kind {
            Kind::Bars(_) => Values::Bars {
                low: decimal("low", low, BAR_SCALE)?,
                high: decimal("high", high, BAR_SCALE)?,
                volume: decimal("volume", self.quantity, BAR_SCALE)?,
                trade_count: self.trade_count,
            },
            Kind::Trades => Values::Trades {
                low: decimal("price", low, PRICE_SCALE)?,
                high: decimal("price", high, PRICE_SCALE)?,
                size: decimal("size", self.quantity, SIZE_SCALE)?,
            },
        };
        Ok(Some(Stats {
            rows: self.rows,
            first,
            last,
            values,
            adjusted: None,
        }))
    }
}
