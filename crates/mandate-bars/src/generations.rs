//! An `alpaca-iex` root's generations (data-plane spec §3.8, DEC-2283 items 4 and 7). The workspace
//! data process never appends to a stored dataset: each fetch writes a complete generation, and a
//! reader picks its generation from its own clock, so the trust check reads one immutable span.
//!
//! - The daily generation of session `D` is `<root>/daily/<D>`, read at any clock whose last
//!   completed regular session is `D`.
//! - The minute generation of minute `M` is `<root>/minutes/<YYYY-MM-DDTHHMM>`, read at `M` or up
//!   to [`MINUTE_FALLBACK`] minutes after it.
//!
//! Under each, the datasets keep `mandate-marketdata`'s layout ([`daily_dir`], [`minute_dir`]). A
//! generation that is missing names a dataset that does not read, so the reader refuses: no closes,
//! no figures, never a guess. Any other root, the cell's recording among them, holds one dataset of
//! each kind and is read as before.

use std::path::{Path, PathBuf};

use mandate_marketdata::dataset::MANIFEST;
use mandate_time::{Date, ExchangeCalendar, UtcNanos};

use crate::liquidity::{daily_dir, minute_dir};
use crate::trusted::BarsError;

/// The last component of a bars root that holds generations.
pub const GENERATIONS_ROOT: &str = "alpaca-iex";
/// How many whole minutes before its own a reader may fall back to for its minute generation.
pub const MINUTE_FALLBACK: i64 = 2;
const MINUTE_S: i64 = 60;

/// Whether `root` holds generations.
pub fn is_generational(root: &Path) -> bool {
    root.file_name()
        .is_some_and(|name| name == GENERATIONS_ROOT)
}

/// The daily generation of session `session` under `root`.
pub fn daily_generation(root: &Path, session: Date) -> PathBuf {
    root.join("daily").join(session.to_string())
}

/// The minute generation of the whole minute `at` falls in, under `root`.
///
/// # Errors
/// [`BarsError::Untrusted`] for a clock whose minute leaves the calendar.
pub fn minute_generation(root: &Path, at: UtcNanos) -> Result<PathBuf, BarsError> {
    let minute = whole_minute(at)?;
    let text = minute.to_string();
    let (date, rest) = text.split_once('T').ok_or_else(cannot_name)?;
    let hour = rest.get(0..2).ok_or_else(cannot_name)?;
    let min = rest.get(3..5).ok_or_else(cannot_name)?;
    Ok(root.join("minutes").join(format!("{date}T{hour}{min}")))
}

fn cannot_name() -> BarsError {
    BarsError::Untrusted {
        what: "the clock names no generation",
    }
}

fn whole_minute(at: UtcNanos) -> Result<UtcNanos, BarsError> {
    let secs = at.secs();
    let start = secs
        .checked_sub(secs.rem_euclid(MINUTE_S))
        .ok_or_else(cannot_name)?;
    UtcNanos::from_parts(start, 0).map_err(|_| cannot_name())
}

/// Where a reader at `now` reads `symbol`'s daily bars under `root`: for a generational root, the
/// daily generation of `now`'s last completed regular session; otherwise [`daily_dir`].
///
/// # Errors
/// [`BarsError::Model`] for a symbol the dataset refuses, and [`BarsError::Untrusted`] when the
/// calendar names no completed session at `now`.
pub fn daily_dir_at(root: &Path, symbol: &str, now: UtcNanos) -> Result<PathBuf, BarsError> {
    if !is_generational(root) {
        return daily_dir(root, symbol);
    }
    let no_session = || BarsError::Untrusted {
        what: "the equity calendar cannot name the last completed session",
    };
    let session = ExchangeCalendar::us_equities()
        .map_err(|_| no_session())?
        .last_completed_regular_session(now)
        .map_err(|_| no_session())?
        .ok_or_else(no_session)?;
    daily_dir(&daily_generation(root, session), symbol)
}

/// Where a reader at `now` reads `symbol`'s one-minute bars under `root`: for a generational root,
/// the newest of `now`'s minute generation and the [`MINUTE_FALLBACK`] before it that holds the
/// symbol's dataset, or `now`'s own when none does, which then does not read; otherwise
/// [`minute_dir`].
///
/// A dataset counts as present once its manifest is a file: `Store::put_day` writes a partition
/// before the manifest that lists it, so a manifest never lists a partition not yet written. Which
/// generation is read depends on what the store holds as well as on `now`; a decision cites the
/// closes and figures it read, never a generation (data-plane spec §3.8, DP-14).
///
/// # Errors
/// [`BarsError::Model`] for a symbol the dataset refuses, and [`BarsError::Untrusted`] for a clock
/// that names no generation.
pub fn minute_dir_at(root: &Path, symbol: &str, now: UtcNanos) -> Result<PathBuf, BarsError> {
    if !is_generational(root) {
        return minute_dir(root, symbol);
    }
    let own = minute_dir(&minute_generation(root, now)?, symbol)?;
    let start = whole_minute(now)?.secs();
    for back in 0..=MINUTE_FALLBACK {
        let secs = back
            .checked_mul(MINUTE_S)
            .and_then(|offset| start.checked_sub(offset))
            .ok_or_else(cannot_name)?;
        let minute = UtcNanos::from_parts(secs, 0).map_err(|_| cannot_name())?;
        let dir = minute_dir(&minute_generation(root, minute)?, symbol)?;
        if dir.join(MANIFEST).is_file() {
            return Ok(dir);
        }
    }
    Ok(own)
}
