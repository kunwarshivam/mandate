//! The liquidity figures the liquidity floor and participation read (DEC-2274): the prior close,
//! the 20-day median dollar volume and ADV from an instrument's trusted daily bars, and the
//! trailing five-minute volume from its complete one-minute bars, each through
//! `mandate-liquidity`'s one rule. The runtime's dry run and the executor's binding gate both read
//! them here, each at its own clock, so they rest on one reading; the store's partitions never
//! change, so the same partitions at the same clock give the same figures.

use std::path::{Path, PathBuf};

use mandate_liquidity::{DailyBar, DailyLiquidity, daily_liquidity, trailing_volume};
use mandate_marketdata::dataset::Store;
use mandate_marketdata::model::{AssetClass, DatasetId, Feed, Kind, Symbol, TimeUnit, Timeframe};
use mandate_num::{Price, Qty, Usd};
use mandate_time::UtcNanos;

use crate::minutes::trailing_minutes;
use crate::trusted::{BarsError, trusted_daily_bars};

/// The four figures, as the gate input carries them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiquidityFacts {
    /// The last completed session's close (§3.2 item 4's price floor).
    pub prior_close: Price,
    /// The lower median of the last 20 sessions' `volume × close` (§3.2 item 5).
    pub median_dollar_volume_20d: Usd,
    /// The last 20 sessions' mean volume, in whole shares (§9.6's daily participation).
    pub adv_20d: Qty,
    /// The complete one-minute bars' volume in the five minutes before the clock (§9.6's order
    /// size participation).
    pub trailing_5m_volume: Qty,
}

fn dataset_dir(root: &Path, symbol: &str, timeframe: Timeframe) -> Result<PathBuf, BarsError> {
    let id = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Iex,
        Kind::Bars(timeframe),
        Symbol::parse(symbol)?,
    )?;
    Ok(Store::new(root).dataset_dir(&id))
}

/// Where `symbol`'s daily bars are under `root`, by `mandate-marketdata`'s layout: a US equity's
/// IEX bars, as the cell's data process stores them (DEC-2385 item 3).
///
/// # Errors
/// [`BarsError::Model`] for a symbol the dataset refuses.
pub fn daily_dir(root: &Path, symbol: &str) -> Result<PathBuf, BarsError> {
    dataset_dir(root, symbol, Timeframe::new(1, TimeUnit::Day)?)
}

/// Where `symbol`'s one-minute bars are under `root`, as [`daily_dir`].
///
/// # Errors
/// [`BarsError::Model`] for a symbol the dataset refuses.
pub fn minute_dir(root: &Path, symbol: &str) -> Result<PathBuf, BarsError> {
    dataset_dir(root, symbol, Timeframe::new(1, TimeUnit::Minute)?)
}

/// The daily figures of the daily dataset at `dir`, only once the trust check admits it at `now`.
///
/// # Errors
/// [`trusted_daily_bars`]' errors, a bar whose close or volume is not a number, and
/// [`BarsError::Liquidity`] for fewer sessions than the figures span.
pub fn trusted_daily_liquidity(
    dir: &Path,
    symbol: &str,
    now: UtcNanos,
) -> Result<DailyLiquidity, BarsError> {
    let bars = trusted_daily_bars(dir, symbol, now)?
        .iter()
        .map(|bar| {
            Ok(DailyBar {
                close: Price::parse(bar.close.as_str())?,
                volume: Qty::parse(bar.volume.as_str())?,
            })
        })
        .collect::<Result<Vec<DailyBar>, BarsError>>()?;
    Ok(daily_liquidity(&bars)?)
}

/// `symbol`'s four figures from the datasets under `root` at `now`, the reader's own clock.
///
/// # Errors
/// Any refusal of [`trusted_daily_liquidity`] or [`trailing_minutes`], a dataset missing under
/// `root`, and [`BarsError::Liquidity`] for an empty trailing window: never a figure.
pub fn liquidity(root: &Path, symbol: &str, now: UtcNanos) -> Result<LiquidityFacts, BarsError> {
    let DailyLiquidity {
        prior_close,
        median_dollar_volume_20d,
        adv_20d,
    } = trusted_daily_liquidity(&daily_dir(root, symbol)?, symbol, now)?;
    let minutes = trailing_minutes(&minute_dir(root, symbol)?, symbol, now)?;
    Ok(LiquidityFacts {
        prior_close,
        median_dollar_volume_20d,
        adv_20d,
        trailing_5m_volume: trailing_volume(&minutes, now)?,
    })
}
