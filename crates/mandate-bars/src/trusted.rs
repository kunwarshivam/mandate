//! The trust check on a stored daily-bar dataset, and the bars and closes it admits (DEC-846 item
//! 2, DEC-470 item 4, DEC-138 item 3), moved unchanged from `mandate-shell` so the shell, the paper
//! runner, the runtime process and the executor read one rule (DEC-2385). A dataset is admitted
//! only when it is the pinned symbol's daily bars through the last completed equity session, every
//! partition intact, no trading day without its bar, and no split inside the span; everything else
//! is [`BarsError::Untrusted`], which denies the opening it was read for (`AGENTS.md` rule 3).

use std::path::Path;

use mandate_marketdata::dataset::{self, DatasetError};
use mandate_marketdata::inspect::{self, ActionsReport, GapClass, InspectError, Inspection};
use mandate_marketdata::model::{Bar, Kind, ModelError, Records, TimeUnit, Timeframe};
use mandate_num::{NumError, Price};
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

/// Why a dataset's bars were not handed out.
#[derive(Debug, thiserror::Error)]
pub enum BarsError {
    /// The dataset is not one the run may decide on; `what` names the condition that failed.
    #[error("untrusted market data: {what}")]
    Untrusted { what: &'static str },
    #[error(transparent)]
    Inspect(#[from] InspectError),
    #[error(transparent)]
    Dataset(#[from] DatasetError),
    #[error(transparent)]
    Model(#[from] ModelError),
    #[error(transparent)]
    Num(#[from] NumError),
}

fn untrusted(what: &'static str) -> BarsError {
    BarsError::Untrusted { what }
}

/// The daily bars the run trusts, exactly as stored and only once `trusted` admits the dataset at
/// `now`, so the paper assembly's liquidity facts read the same span as the model host's closes.
pub fn trusted_daily_bars(dir: &Path, symbol: &str, now: UtcNanos) -> Result<Vec<Bar>, BarsError> {
    let inspection = inspect::inspect(dir)?;
    trusted(&inspection, symbol, now)?;
    listed_bars(dir, inspection.dataset.kind())
}

/// The pinned symbol's closes, each with its session, exactly as stored and only once `trusted`
/// admits the dataset at `now`: the span the liquidity facts read, handed to the model host
/// (DEC-846 item 2).
pub fn trusted_daily_closes(
    dir: &Path,
    symbol: &str,
    now: UtcNanos,
) -> Result<Vec<(Date, Price)>, BarsError> {
    let inspection = inspect::inspect(dir)?;
    trusted(&inspection, symbol, now)?;
    listed_days(dir, inspection.dataset.kind())?
        .into_iter()
        .map(|(day, bar)| Ok((day, Price::parse(bar.close.as_str())?)))
        .collect()
}

fn listed_bars(dir: &Path, kind: Kind) -> Result<Vec<Bar>, BarsError> {
    Ok(listed_days(dir, kind)?
        .into_iter()
        .map(|(_, bar)| bar)
        .collect())
}

/// One bar per day the manifest lists with a file, in its date order, with that day.
fn listed_days(dir: &Path, kind: Kind) -> Result<Vec<(Date, Bar)>, BarsError> {
    let (_, days) = dataset::read_manifest(dir)?;
    let mut listed_bars = Vec::new();
    for listed in days.iter().filter(|listed| listed.file.is_some()) {
        let path = dir.join(dataset::partition_name(listed.day));
        match dataset::read(&path, kind)? {
            Records::Bars(bars) => match <[Bar; 1]>::try_from(bars) {
                Ok([bar]) => listed_bars.push((listed.day, bar)),
                Err(_) => return Err(untrusted("a listed day does not hold exactly one bar")),
            },
            Records::Trades(_) | Records::Quotes(_) => {
                return Err(untrusted("the dataset holds no bars"));
            }
        }
    }
    Ok(listed_bars)
}

/// Whether `inspect` found the dataset fit to decide on: the pinned symbol's daily bars through
/// the last completed equity session, every partition intact, no trading day without its bar, and
/// no split inside the span, whose prices the shell may not adjust (DEC-138 item 3). `inspect`'s
/// quality warnings are the vendor's records as stored (DEC-89) and are not refusals.
fn trusted(inspection: &Inspection, symbol: &str, now: UtcNanos) -> Result<(), BarsError> {
    let daily = Kind::Bars(Timeframe::new(1, TimeUnit::Day)?);
    let latest = latest_completed_equity_day(now)?;
    let refusal = if inspection.dataset.symbol().as_str() != symbol {
        Some("the dataset is another instrument's")
    } else if inspection.dataset.kind() != daily {
        Some("the dataset is not daily bars")
    } else if !inspection.problems.is_empty() {
        Some("a partition cannot be trusted")
    } else if !inspection.duplicates.is_empty() {
        Some("two bars share a start")
    } else if !inspection.coverage.empty.is_empty() {
        Some("a trading day has no bar")
    } else if inspection.gaps.iter().any(|gap| {
        gap.stretches
            .iter()
            .any(|stretch| matches!(stretch.class, GapClass::TrueGap | GapClass::Unclassified))
    }) {
        Some("a trading day was never fetched")
    } else if inspection.coverage.span.map(|span| span.last()) != Some(latest) {
        Some("the dataset does not end on the last completed equity session")
    } else if split_inside(inspection) {
        Some("a split lies inside the span, or the corporate actions do not cover it")
    } else {
        None
    };
    match refusal {
        Some(what) => Err(untrusted(what)),
        None => Ok(()),
    }
}

/// The newest US-equity regular session whose end is not after the injected run clock. Calendar
/// data, session hours, holidays, and early closes all come from `mandate-time`; the shell invents
/// none of them, and the session is `mandate-time`'s
/// [`ExchangeCalendar::last_completed_regular_session`], the one the model host names too. A clock
/// before the calendar's first completed session, or past its range (DEC-517), is untrusted.
fn latest_completed_equity_day(now: UtcNanos) -> Result<Date, BarsError> {
    let cannot_name = || untrusted("the equity calendar cannot name the last completed session");
    ExchangeCalendar::us_equities()
        .map_err(|_| cannot_name())?
        .last_completed_regular_session(now)
        .map_err(|_| cannot_name())?
        .ok_or_else(cannot_name)
}

/// Whether a split could have moved a close inside the span. Actions recorded for only part of the
/// span say nothing about the rest, so they count as one.
fn split_inside(inspection: &Inspection) -> bool {
    match &inspection.corporate_actions {
        ActionsReport::NotApplicable | ActionsReport::NotRecorded => false,
        ActionsReport::Incomplete(_) => true,
        ActionsReport::Applied { recorded, .. } => match inspection.coverage.span {
            Some(span) => recorded
                .actions
                .splits
                .iter()
                .any(|split| split.ex_date > span.first() && split.ex_date <= span.last()),
            None => false,
        },
    }
}

#[cfg(test)]
mod tests;
