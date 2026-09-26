//! The baseline strategy and the buy-and-hold benchmark
//! ([task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md) "The baseline strategy",
//! DEC-127 items 12 and 13).

use crate::BacktestError;
use mandate_num::{Bps, Price, Usd};

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

/// Which strategy a run drives.
///
/// [`Strategy::MovingAverageCrossover`] is long when the fast window's average is strictly above the
/// slow window's and flat otherwise, a tie resolving flat because ambiguity never adds risk
/// (AGENTS.md rule 3). It compares the two averages **without dividing**, by cross-multiplying the
/// window sums, so the signal introduces no rounding of its own.
///
/// [`Strategy::BuyAndHold`] submits one GTC limit buy at the first bar's close plus the collar and
/// nothing afterwards: the benchmark FR-4.2 asks for, paying the same slippage and the same fees as
/// the strategy it is compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    MovingAverageCrossover(StrategyConfig),
    BuyAndHold { collar: Bps },
}

/// What the strategy wants at a period's close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    /// Hold a position: enter when flat, and never add.
    Long,
    /// Hold nothing: close the whole position when one is held.
    Flat,
    /// Not enough periods to decide yet, so nothing is submitted.
    Undecided,
}

impl Strategy {
    /// The signal at the close of the last period in `closes`, whose entries are the closing prices
    /// of every period so far, oldest first.
    ///
    /// For a crossover: `Undecided` until `slow_periods` periods have closed, then `Long` when
    /// `fast_sum × slow_periods > slow_sum × fast_periods` — the division-free form of
    /// `fast_sum ÷ fast_periods > slow_sum ÷ slow_periods` — and `Flat` on equality or below. For
    /// buy-and-hold: `Long` at the first period and `Undecided` afterwards, since the benchmark
    /// submits exactly one order.
    ///
    /// Errors: `strategy_windows_crossed` for a fast window at or above the slow one, or either
    /// zero; and the arithmetic errors of the sums.
    pub fn signal(&self, closes: &[Price]) -> Result<Signal, BacktestError> {
        let _ = closes;
        Err(BacktestError::Unimplemented)
    }
}
