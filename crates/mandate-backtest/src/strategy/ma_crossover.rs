//! The moving-average crossover's signal (`quant.ma_crossover`), in a file of its own: the one
//! implementation the backtest and the model host share, and the source file whose bytes the
//! model's content hash lists ([DEC-504](../../../../docs/project/decisions/DEC-504.md) item 1,
//! the [first paper trade brief](../../../../docs/project/tasks/first-paper-trade.md) slice M0).
//! An edit here is an edit to the model.

use core::cmp::Ordering;

use super::Signal;
use crate::BacktestError;
use mandate_num::{Bps, NumError, Price, Ratio, Usd};

/// The moving-average crossover's parameters. The windows are counted in **periods**, not bars, and
/// `collar` is how far from the signal period's close a limit is priced before it is put on the tick
/// (spec §2.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StrategyConfig {
    pub fast_periods: u32,
    pub slow_periods: u32,
    pub collar: Bps,
    pub target_notional: Usd,
}

impl StrategyConfig {
    /// `Long` when `fast_sum × slow_periods > slow_sum × fast_periods`, the division-free form of
    /// `fast_sum ÷ fast_periods > slow_sum ÷ slow_periods`, and `Flat` on equality or below.
    pub(super) fn signal(&self, closes: &[Price]) -> Result<Signal, BacktestError> {
        let (fast, slow) = self.windows()?;
        if closes.len() < slow {
            return Ok(Signal::Undecided);
        }
        let fast_average = window_sum(closes, fast)?.times_int(self.slow_periods)?;
        let slow_average = window_sum(closes, slow)?.times_int(self.fast_periods)?;
        Ok(match fast_average.cmp(&slow_average) {
            Ordering::Greater => Signal::Long,
            Ordering::Equal | Ordering::Less => Signal::Flat,
        })
    }

    /// The two windows as lengths, or `strategy_windows_crossed` for a fast window at or above the
    /// slow one, or either zero.
    fn windows(&self) -> Result<(usize, usize), BacktestError> {
        let fast_below_slow = matches!(self.fast_periods.cmp(&self.slow_periods), Ordering::Less);
        if self.fast_periods == 0 || !fast_below_slow {
            return Err(BacktestError::StrategyWindowsCrossed);
        }
        let fast = usize::try_from(self.fast_periods).map_err(|_| NumError::Overflow)?;
        let slow = usize::try_from(self.slow_periods).map_err(|_| NumError::Overflow)?;
        Ok((fast, slow))
    }
}

/// Σ of the last `periods` closes, exact: a 9-place price is read as the ratio its canonical text
/// denotes, so the comparison introduces no rounding of its own (DEC-127 item 12).
fn window_sum(closes: &[Price], periods: usize) -> Result<Ratio, BacktestError> {
    let window = closes
        .iter()
        .rev()
        .take(periods)
        .map(|close| Ratio::parse(&close.to_string()))
        .collect::<Result<Vec<Ratio>, NumError>>()?;
    Ok(Ratio::sum(&window)?)
}
