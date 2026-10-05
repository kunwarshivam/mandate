#![deny(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::float_arithmetic,
    clippy::float_cmp,
    clippy::as_conversions
)]
//! The pure liquidity figures the gate's liquidity floor and participation caps read
//! (trading-domain spec §3.2 item 5 and §9.6), each computed by one stated rule from typed bars
//! the caller has already judged fit to decide on (DEC-470 item 4, DEC-471).
//!
//! Every figure errs toward the tighter limit: the median is the lower of the two middle values,
//! the average is truncated to whole shares, and the trailing volume counts only bars that are
//! complete at the clock. A figure that cannot be computed by its rule is an error, never a
//! default (`AGENTS.md` rule 3).

use std::error::Error;
use std::fmt;

use mandate_num::{Fraction, NumError, Price, Qty, ShareIncrement, Usd};
use mandate_time::{TimeError, UtcNanos};

/// The sessions the daily figures span (spec §3.2 item 5, §9.6).
pub const SESSIONS: usize = 20;
/// The lower of the two middle positions of [`SESSIONS`] sorted values, which is never above the
/// true median, so a floor that compares against it reads it conservatively.
const LOWER_MEDIAN: usize = 9;
/// One [`SESSIONS`]th, so a sum of twenty daily volumes times it is their mean.
const ONE_PER_SESSION: &str = "0.05";
/// §9.6's trailing window for the order-size participation cap.
pub const TRAILING_WINDOW_S: i64 = 300;
/// One bar's span: the trailing volume is read from one-minute bars.
pub const BAR_S: i64 = 60;

/// One trusted daily bar's inputs to the liquidity rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DailyBar {
    pub close: Price,
    pub volume: Qty,
}

/// The daily figures, from the last [`SESSIONS`] sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DailyLiquidity {
    /// The close of the last session given.
    pub prior_close: Price,
    /// The lower median of the sessions' `volume × close`.
    pub median_dollar_volume_20d: Usd,
    /// The mean of the sessions' volumes, truncated to whole shares.
    pub adv_20d: Qty,
}

/// One one-minute bar's start and volume, all the trailing volume reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MinuteVolume {
    pub start: UtcNanos,
    pub volume: Qty,
}

/// Why a liquidity figure could not be computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiquidityError {
    FewerSessions,
    AheadOfClock,
    Unordered,
    EmptyWindow,
    Num(NumError),
    Time(TimeError),
}

impl LiquidityError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::FewerSessions => "fewer_sessions",
            Self::AheadOfClock => "ahead_of_clock",
            Self::Unordered => "unordered",
            Self::EmptyWindow => "empty_window",
            Self::Num(error) => error.code(),
            Self::Time(error) => error.code(),
        }
    }
}

impl fmt::Display for LiquidityError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FewerSessions => formatter.write_str("fewer than 20 sessions were given"),
            Self::AheadOfClock => formatter.write_str(
                "a minute bar ends after the clock, so it cannot have been seen complete",
            ),
            Self::Unordered => {
                formatter.write_str("the minute bars do not start in strictly increasing order")
            }
            Self::EmptyWindow => {
                formatter.write_str("no complete minute bar lies in the trailing window")
            }
            Self::Num(error) => fmt::Display::fmt(error, formatter),
            Self::Time(error) => fmt::Display::fmt(error, formatter),
        }
    }
}

impl Error for LiquidityError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Num(error) => Some(error),
            Self::Time(error) => Some(error),
            Self::FewerSessions | Self::AheadOfClock | Self::Unordered | Self::EmptyWindow => None,
        }
    }
}

impl From<NumError> for LiquidityError {
    fn from(error: NumError) -> Self {
        Self::Num(error)
    }
}

impl From<TimeError> for LiquidityError {
    fn from(error: TimeError) -> Self {
        Self::Time(error)
    }
}

/// The daily figures from the last [`SESSIONS`] of `bars`, which are in session order: the prior
/// close is the last close, the median dollar volume is the lower median of `volume × close`, and
/// the average daily volume is the mean volume truncated to whole shares.
///
/// # Errors
/// [`LiquidityError::FewerSessions`] for fewer than [`SESSIONS`] bars, and the number's own error
/// when an intermediate value cannot be represented.
pub fn daily_liquidity(bars: &[DailyBar]) -> Result<DailyLiquidity, LiquidityError> {
    let first = bars
        .len()
        .checked_sub(SESSIONS)
        .ok_or(LiquidityError::FewerSessions)?;
    let window = bars.get(first..).ok_or(LiquidityError::FewerSessions)?;
    let last = window.last().ok_or(LiquidityError::FewerSessions)?;
    let mut dollar_volumes = window
        .iter()
        .map(|bar| bar.volume.notional(bar.close))
        .collect::<Result<Vec<Usd>, NumError>>()?;
    dollar_volumes.sort_unstable();
    let median_dollar_volume_20d = dollar_volumes
        .get(LOWER_MEDIAN)
        .copied()
        .ok_or(LiquidityError::FewerSessions)?;
    let total = window
        .iter()
        .try_fold(Qty::ZERO, |sum, bar| sum.checked_add(bar.volume))?;
    Ok(DailyLiquidity {
        prior_close: last.close,
        median_dollar_volume_20d,
        adv_20d: total.portion(Fraction::parse(ONE_PER_SESSION)?, ShareIncrement::Whole)?,
    })
}

/// The volume of the one-minute `bars` that start no earlier than [`TRAILING_WINDOW_S`] before
/// `now` (§9.6). Every bar must end by `now`, since a bar still open has not traded its volume yet,
/// and starts must strictly increase, so no minute is counted twice.
///
/// # Errors
/// [`LiquidityError::AheadOfClock`] for a bar ending after `now`, [`LiquidityError::Unordered`] for
/// a start at or before the one before it, and [`LiquidityError::EmptyWindow`] when no bar starts
/// inside the window.
pub fn trailing_volume(bars: &[MinuteVolume], now: UtcNanos) -> Result<Qty, LiquidityError> {
    let from = shifted(
        now,
        TRAILING_WINDOW_S
            .checked_neg()
            .ok_or(TimeError::OutOfRange)?,
    )?;
    let mut previous: Option<UtcNanos> = None;
    let mut counted: Option<Qty> = None;
    for bar in bars {
        if previous.is_some_and(|before| bar.start <= before) {
            return Err(LiquidityError::Unordered);
        }
        previous = Some(bar.start);
        if shifted(bar.start, BAR_S)? > now {
            return Err(LiquidityError::AheadOfClock);
        }
        if bar.start >= from {
            counted = Some(counted.unwrap_or(Qty::ZERO).checked_add(bar.volume)?);
        }
    }
    counted.ok_or(LiquidityError::EmptyWindow)
}

/// `at` moved by `secs` whole seconds.
fn shifted(at: UtcNanos, secs: i64) -> Result<UtcNanos, TimeError> {
    let moved = at.secs().checked_add(secs).ok_or(TimeError::OutOfRange)?;
    UtcNanos::from_parts(moved, at.nanos())
}

#[cfg(test)]
mod tests {
    use std::error::Error as _;

    use mandate_num::{Price, Qty, Usd};
    use mandate_time::UtcNanos;

    use super::{
        DailyBar, DailyLiquidity, LiquidityError, MinuteVolume, SESSIONS, daily_liquidity,
        trailing_volume,
    };

    type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

    fn bar(close: &str, volume: &str) -> Result<DailyBar> {
        Ok(DailyBar {
            close: Price::parse(close)?,
            volume: Qty::parse(volume)?,
        })
    }

    fn at(text: &str) -> Result<UtcNanos> {
        Ok(UtcNanos::parse_rfc3339(text)?)
    }

    fn minute(start: &str, volume: &str) -> Result<MinuteVolume> {
        Ok(MinuteVolume {
            start: at(start)?,
            volume: Qty::parse(volume)?,
        })
    }

    /// Twenty sessions whose dollar volumes, hand-sorted, put 1,000 × 10 at the tenth place and
    /// 1,100 × 10 at the eleventh: the lower median is 10,000, not the 10,500 a midpoint would give.
    /// Volumes sum to 9 × 900 + 1,000 + 1,100 + 9 × 1,201 = 21,009, so the mean 1,050.45 truncates
    /// to 1,050 whole shares. An older, twenty-first session with an outsized volume and close is
    /// outside the window.
    #[test]
    fn the_daily_figures_read_the_last_twenty_sessions_by_their_stated_rules() -> Result<()> {
        let mut bars = vec![bar("999", "999999")?];
        for _ in 0..9 {
            bars.push(bar("10", "900")?);
        }
        bars.push(bar("10", "1000")?);
        bars.push(bar("10", "1100")?);
        for _ in 0..9 {
            bars.push(bar("10", "1201")?);
        }
        bars.swap(5, 15);
        assert_eq!(bars.len(), SESSIONS + 1);
        assert_eq!(bars.last().map(|bar| bar.close), Some(Price::parse("10")?));
        assert_eq!(
            daily_liquidity(&bars)?,
            DailyLiquidity {
                prior_close: Price::parse("10")?,
                median_dollar_volume_20d: Usd::parse("10000")?,
                adv_20d: Qty::parse("1050")?,
            }
        );
        Ok(())
    }

    /// The prior close is the last session's, whatever the others closed at, and the median is of
    /// `volume × close`, not of volume alone.
    #[test]
    fn the_prior_close_is_the_last_close_and_the_median_weighs_price() -> Result<()> {
        let mut bars = Vec::new();
        for _ in 0..19 {
            bars.push(bar("2", "100")?);
        }
        bars.push(bar("3.5", "100")?);
        let figures = daily_liquidity(&bars)?;
        assert_eq!(figures.prior_close, Price::parse("3.5")?);
        assert_eq!(figures.median_dollar_volume_20d, Usd::parse("200")?);
        assert_eq!(figures.adv_20d, Qty::parse("100")?);
        Ok(())
    }

    #[test]
    fn fewer_than_twenty_sessions_compute_nothing() -> Result<()> {
        let bars = (0..19)
            .map(|_| bar("10", "100"))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(daily_liquidity(&bars), Err(LiquidityError::FewerSessions));
        assert_eq!(daily_liquidity(&[]), Err(LiquidityError::FewerSessions));
        Ok(())
    }

    /// Ten bars from 16:50 to 16:59 with volumes 1 to 10 (hand-summed windows below).
    fn ten_minutes() -> Result<Vec<MinuteVolume>> {
        (0_u32..10)
            .map(|index| {
                let volume = index.checked_add(1).ok_or("minute index overflow")?;
                minute(&format!("2026-09-28T16:5{index}:00Z"), &volume.to_string())
            })
            .collect()
    }

    /// At 17:00 exactly the window starts at 16:55, which counts (6 + 7 + 8 + 9 + 10 = 40), and the
    /// 16:59 bar ends exactly at the clock, which counts too. One nanosecond later 16:55 drops out
    /// (34). At 17:04 only 16:59 is left (10); a nanosecond later nothing is.
    #[test]
    fn the_trailing_volume_counts_complete_bars_starting_inside_the_window() -> Result<()> {
        let bars = ten_minutes()?;
        let cases = [
            ("2026-09-28T17:00:00Z", Ok("40")),
            ("2026-09-28T17:00:00.000000001Z", Ok("34")),
            ("2026-09-28T17:04:00Z", Ok("10")),
            (
                "2026-09-28T17:04:00.000000001Z",
                Err(LiquidityError::EmptyWindow),
            ),
            (
                "2026-09-28T16:59:59.999999999Z",
                Err(LiquidityError::AheadOfClock),
            ),
        ];
        for (now, expected) in cases {
            let expected = match expected {
                Ok(volume) => Ok(Qty::parse(volume)?),
                Err(error) => Err(error),
            };
            assert_eq!(trailing_volume(&bars, at(now)?), expected, "at {now}");
        }
        Ok(())
    }

    /// A window holding only a zero-volume bar is a window with a bar in it: zero, not a refusal.
    #[test]
    fn a_window_with_a_zero_volume_bar_reads_zero() -> Result<()> {
        let bars = [minute("2026-09-28T16:59:00Z", "0")?];
        assert_eq!(
            trailing_volume(&bars, at("2026-09-28T17:00:00Z")?),
            Ok(Qty::ZERO)
        );
        assert_eq!(
            trailing_volume(&[], at("2026-09-28T17:00:00Z")?),
            Err(LiquidityError::EmptyWindow)
        );
        Ok(())
    }

    /// A repeated or backward start would count a minute twice or hide a gap.
    #[test]
    fn starts_must_strictly_increase() -> Result<()> {
        let now = at("2026-09-28T17:00:00Z")?;
        let repeated = [
            minute("2026-09-28T16:58:00Z", "5")?,
            minute("2026-09-28T16:58:00Z", "5")?,
        ];
        assert_eq!(
            trailing_volume(&repeated, now),
            Err(LiquidityError::Unordered)
        );
        let backward = [
            minute("2026-09-28T16:59:00Z", "5")?,
            minute("2026-09-28T16:58:00Z", "5")?,
        ];
        assert_eq!(
            trailing_volume(&backward, now),
            Err(LiquidityError::Unordered)
        );
        Ok(())
    }

    #[test]
    fn every_code_is_stable() -> Result<()> {
        let simple = [
            (
                LiquidityError::FewerSessions,
                "fewer_sessions",
                "fewer than 20 sessions were given",
            ),
            (
                LiquidityError::AheadOfClock,
                "ahead_of_clock",
                "a minute bar ends after the clock, so it cannot have been seen complete",
            ),
            (
                LiquidityError::Unordered,
                "unordered",
                "the minute bars do not start in strictly increasing order",
            ),
            (
                LiquidityError::EmptyWindow,
                "empty_window",
                "no complete minute bar lies in the trailing window",
            ),
        ];
        for (error, code, display) in simple {
            assert_eq!(error.code(), code);
            assert_eq!(error.to_string(), display);
            assert!(error.source().is_none());
        }
        let num = Qty::parse("-1").err().ok_or("a negative quantity parses")?;
        let num_error = LiquidityError::Num(num);
        assert_eq!(num_error.code(), num.code());
        assert_eq!(num_error.to_string(), num.to_string());
        assert!(num_error.source().is_some());
        let time = UtcNanos::from_parts(-1, 0)
            .err()
            .ok_or("a negative instant exists")?;
        let time_error = LiquidityError::Time(time);
        assert_eq!(time_error.code(), time.code());
        assert_eq!(time_error.to_string(), time.to_string());
        assert!(time_error.source().is_some());
        Ok(())
    }
}
