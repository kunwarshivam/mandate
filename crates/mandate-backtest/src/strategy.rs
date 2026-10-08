//! The baseline strategy and the buy-and-hold benchmark
//! ([task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md) "The baseline strategy",
//! DEC-127 items 12 and 13).

mod ma_crossover;

pub use ma_crossover::StrategyConfig;

use crate::BacktestError;
use mandate_accounting::AssetClass;
use mandate_num::{Bps, Price, Usd};
use mandate_sim::TimeInForce;

/// Which strategy a run drives.
///
/// [`Strategy::MovingAverageCrossover`] is long when the fast window's average is strictly above the
/// slow window's and flat otherwise, a tie resolving flat because ambiguity never adds risk
/// (AGENTS.md rule 3). It compares the two averages **without dividing**, by cross-multiplying the
/// window sums, so the signal introduces no rounding of its own.
///
/// [`Strategy::BuyAndHold`] submits one GTC limit buy at the **first bar's** close plus the collar and
/// nothing afterwards: the benchmark FR-4.2 asks for, paying the same slippage and the same fees as
/// the strategy it is compared with. It is the one exception to the loop's rule that decisions are
/// taken at period closes (DEC-127 items 2 and 13): a benchmark decided at the first *period* close
/// would sit out that whole period, so its order is decided at bar 0's close, timed at bar 1's start,
/// and eligible from bar 1.
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
    /// buy-and-hold: `Long` when `closes` holds exactly one price, the close of bar 0, and `Undecided`
    /// afterwards, since the benchmark submits exactly one order. The loop passes it the first bar's
    /// close at bar 0, rather than waiting for a period to close.
    ///
    /// Errors: `strategy_windows_crossed` for a fast window at or above the slow one, or either
    /// zero; and the arithmetic errors of the sums.
    pub fn signal(&self, closes: &[Price]) -> Result<Signal, BacktestError> {
        match self {
            Self::MovingAverageCrossover(config) => config.signal(closes),
            Self::BuyAndHold { .. } => Ok(match closes.len() {
                1 => Signal::Long,
                _ => Signal::Undecided,
            }),
        }
    }

    /// How far from the signal period's close a limit is priced before it is put on the tick.
    pub(crate) fn collar(self) -> Bps {
        match self {
            Self::MovingAverageCrossover(config) => config.collar,
            Self::BuyAndHold { collar } => collar,
        }
    }

    /// The money an entry sizes from: the crossover's target notional or the whole cash balance,
    /// whichever is smaller, and for buy-and-hold the whole balance, which at bar 0 is the starting
    /// cash (DEC-127 items 12 and 13). `cash_total` is settled plus every unsettled bucket, so a
    /// margin account's unsettled sale proceeds are buying power as spec §7.2 makes them.
    pub(crate) fn entry_budget(self, cash_total: Usd) -> Usd {
        match self {
            Self::MovingAverageCrossover(config) => cash_total.min(config.target_notional),
            Self::BuyAndHold { .. } => cash_total,
        }
    }

    /// Spec §5.2's time in force: `Day` on an equity, and `Gtc` on a continuous instrument, whose
    /// bars have no session to cancel a day order at. A buy-and-hold benchmark is always `Gtc`, so
    /// a remainder the volume cap left keeps working (DEC-127 item 13).
    pub(crate) fn time_in_force(self, asset_class: AssetClass) -> TimeInForce {
        match (self, asset_class) {
            (Self::BuyAndHold { .. }, _) | (_, AssetClass::Crypto) => TimeInForce::Gtc,
            (Self::MovingAverageCrossover(_), AssetClass::UsEquity) => TimeInForce::Day,
        }
    }
}
