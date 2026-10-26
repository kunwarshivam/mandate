//! The minute bars a trailing-volume figure rests on (trading spec §9.6, DEC-2385 item 6): the
//! complete one-minute bars of a stored minute dataset that started inside the trailing window
//! before the clock, as `mandate_liquidity::trailing_volume` sums them. A bar is complete only once
//! it has ended by the clock, so a stored bar for a later minute is never handed out.

use std::path::Path;

use mandate_liquidity::{BAR_S, MinuteVolume, TRAILING_WINDOW_S};
use mandate_marketdata::dataset;
use mandate_marketdata::inspect;
use mandate_marketdata::model::{Kind, Records, TimeUnit, Timeframe};
use mandate_num::Qty;
use mandate_time::UtcNanos;

use crate::trusted::BarsError;

/// The complete one-minute bars of `symbol` in the minute dataset at `dir` that start inside the
/// [`TRAILING_WINDOW_S`] before `now` and end by it, oldest first, each with its volume. Empty when
/// none do, which the trailing-volume rule refuses (`EmptyWindow`).
///
/// # Errors
/// [`BarsError::Untrusted`] when the dataset is another symbol's, is not one-minute bars, or has a
/// problem or a duplicate under `inspect`; the dataset's own error when it cannot be read.
pub fn trailing_minutes(
    dir: &Path,
    symbol: &str,
    now: UtcNanos,
) -> Result<Vec<MinuteVolume>, BarsError> {
    let minute = Kind::Bars(Timeframe::new(1, TimeUnit::Minute)?);
    let inspection = inspect::inspect(dir)?;
    let refusal = if inspection.dataset.symbol().as_str() != symbol {
        Some("the minute bars are another instrument's")
    } else if inspection.dataset.kind() != minute {
        Some("the dataset is not one-minute bars")
    } else if !inspection.problems.is_empty() {
        Some("a minute partition cannot be trusted")
    } else if !inspection.duplicates.is_empty() {
        Some("two minute bars share a start")
    } else {
        None
    };
    if let Some(what) = refusal {
        return Err(BarsError::Untrusted { what });
    }
    let from = now
        .secs()
        .checked_sub(TRAILING_WINDOW_S)
        .ok_or(BarsError::Untrusted {
            what: "a clock inside the calendar",
        })?;
    let (_, days) = dataset::read_manifest(dir)?;
    let Some(listed) = days
        .iter()
        .find(|d| d.day == now.date() && d.file.is_some())
    else {
        return Ok(Vec::new());
    };
    let path = dir.join(dataset::partition_name(listed.day));
    let Records::Bars(bars) = dataset::read(&path, minute)? else {
        return Err(BarsError::Untrusted {
            what: "the dataset holds no bars",
        });
    };
    bars.into_iter()
        .filter(|bar| {
            bar.start.secs() >= from
                && bar
                    .start
                    .secs()
                    .checked_add(BAR_S)
                    .is_some_and(|end| end <= now.secs())
        })
        .map(|bar| {
            Ok(MinuteVolume {
                start: bar.start,
                volume: Qty::parse(bar.volume.as_str())?,
            })
        })
        .collect()
}
