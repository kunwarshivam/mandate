//! The liquidity figures the gate's liquidity floor and participation caps read (trading-domain spec
//! §3.2 item 5 and §9.6), each computed by one stated rule from bars the caller has already judged
//! fit to decide on (DEC-468 item 4, DEC-469).
//!
//! Every figure errs toward the tighter limit: the median is the lower of the two middle values, the
//! average is truncated to whole shares, and the trailing volume counts only bars that are complete
//! at the clock. A figure that cannot be computed by its rule is an error, never a default
//! (`AGENTS.md` rule 3).

use mandate_num::{Fraction, NumError, Price, Qty, ShareIncrement, Usd};
use mandate_time::{TimeError, UtcNanos};

use crate::model::Bar;

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
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LiquidityError {
    #[error("fewer than 20 sessions were given")]
    FewerSessions,
    #[error("a minute bar ends after the clock, so it cannot have been seen complete")]
    AheadOfClock,
    #[error("the minute bars do not start in strictly increasing order")]
    Unordered,
    #[error("no complete minute bar lies in the trailing window")]
    EmptyWindow,
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
}

impl LiquidityError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::FewerSessions => "fewer_sessions",
            Self::AheadOfClock => "ahead_of_clock",
            Self::Unordered => "unordered",
            Self::EmptyWindow => "empty_window",
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
        }
    }
}

/// The daily figures from the last [`SESSIONS`] of `bars`, which are in session order: the prior
/// close is the last close, the median dollar volume is the lower median of `volume × close`, and
/// the average daily volume is the mean volume truncated to whole shares.
///
/// # Errors
/// [`LiquidityError::FewerSessions`] for fewer than [`SESSIONS`] bars, and the number's own error
/// for a close or volume `mandate-num` cannot hold.
pub fn daily_liquidity(bars: &[Bar]) -> Result<DailyLiquidity, LiquidityError> {
    let first = bars
        .len()
        .checked_sub(SESSIONS)
        .ok_or(LiquidityError::FewerSessions)?;
    let window = bars.get(first..).ok_or(LiquidityError::FewerSessions)?;
    let last = window.last().ok_or(LiquidityError::FewerSessions)?;
    let mut dollar_volumes = window
        .iter()
        .map(|bar| volume(bar)?.notional(close(bar)?))
        .collect::<Result<Vec<Usd>, NumError>>()?;
    dollar_volumes.sort_unstable();
    let median_dollar_volume_20d = dollar_volumes
        .get(LOWER_MEDIAN)
        .copied()
        .ok_or(LiquidityError::FewerSessions)?;
    let total = window
        .iter()
        .try_fold(Qty::ZERO, |sum, bar| sum.checked_add(volume(bar)?))?;
    Ok(DailyLiquidity {
        prior_close: close(last)?,
        median_dollar_volume_20d,
        adv_20d: total.portion(Fraction::parse(ONE_PER_SESSION)?, ShareIncrement::Whole)?,
    })
}

fn close(bar: &Bar) -> Result<Price, NumError> {
    Price::parse(bar.close.as_str())
}

fn volume(bar: &Bar) -> Result<Qty, NumError> {
    Qty::parse(bar.volume.as_str())
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
    use mandate_canon::DecStr;
    use mandate_num::{Price, Qty, Usd};
    use mandate_time::UtcNanos;

    use super::{
        DailyLiquidity, LiquidityError, MinuteVolume, SESSIONS, daily_liquidity, trailing_volume,
    };
    use crate::model::Bar;

    type Result<T> = core::result::Result<T, Box<dyn std::error::Error>>;

    fn bar(day: u32, close: &str, volume: &str) -> Result<Bar> {
        let decimal = |text: &str| DecStr::parse(text).map_err(|e| format!("{text}: {e:?}"));
        Ok(Bar {
            start: UtcNanos::from_parts(i64::from(day) * 86_400, 0)?,
            open: decimal(close)?,
            high: decimal(close)?,
            low: decimal(close)?,
            close: decimal(close)?,
            volume: decimal(volume)?,
            vwap: decimal(close)?,
            trade_count: 1,
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
        let mut bars = vec![bar(1, "999", "999999")?];
        for day in 0..9 {
            bars.push(bar(2 + day, "10", "900")?);
        }
        bars.push(bar(11, "10", "1000")?);
        bars.push(bar(12, "10", "1100")?);
        for day in 0..9 {
            bars.push(bar(13 + day, "10", "1201")?);
        }
        bars.swap(5, 15);
        assert_eq!(bars.len(), SESSIONS + 1);
        let last_close = bars.last().map(|b| b.close.as_str().to_owned());
        assert_eq!(last_close.as_deref(), Some("10"));
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
        for day in 0..19 {
            bars.push(bar(day, "2", "100")?);
        }
        bars.push(bar(19, "3.5", "100")?);
        let figures = daily_liquidity(&bars)?;
        assert_eq!(figures.prior_close, Price::parse("3.5")?);
        assert_eq!(figures.median_dollar_volume_20d, Usd::parse("200")?);
        assert_eq!(figures.adv_20d, Qty::parse("100")?);
        Ok(())
    }

    #[test]
    fn fewer_than_twenty_sessions_compute_nothing() -> Result<()> {
        let bars = (0..19)
            .map(|day| bar(day, "10", "100"))
            .collect::<Result<Vec<_>>>()?;
        assert_eq!(daily_liquidity(&bars), Err(LiquidityError::FewerSessions));
        assert_eq!(daily_liquidity(&[]), Err(LiquidityError::FewerSessions));
        Ok(())
    }

    /// Ten bars from 16:50 to 16:59 with volumes 1 to 10 (hand-summed windows below).
    fn ten_minutes() -> Result<Vec<MinuteVolume>> {
        (0..10)
            .map(|index| {
                minute(
                    &format!("2026-09-28T16:5{index}:00Z"),
                    &(index + 1).to_string(),
                )
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
        assert_eq!(LiquidityError::FewerSessions.code(), "fewer_sessions");
        assert_eq!(LiquidityError::AheadOfClock.code(), "ahead_of_clock");
        assert_eq!(LiquidityError::Unordered.code(), "unordered");
        assert_eq!(LiquidityError::EmptyWindow.code(), "empty_window");
        let num = Qty::parse("-1").err().ok_or("a negative quantity parses")?;
        assert_eq!(LiquidityError::Num(num).code(), num.code());
        let time = UtcNanos::from_parts(-1, 0)
            .err()
            .ok_or("a negative instant exists")?;
        assert_eq!(LiquidityError::Time(time).code(), time.code());
        Ok(())
    }
}
