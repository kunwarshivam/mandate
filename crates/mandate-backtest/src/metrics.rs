//! The report's figures, every one an exact decimal or one named rounding of one formula
//! ([task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md) "Metric definitions",
//! DEC-127 items 4 to 11 and 22).

use crate::report::{AbsentStatistics, Sign};
use crate::{BacktestError, MetricsConfig, Observation};
use mandate_num::{Qty, Ratio, Usd};

/// One run's figures. A figure that is undefined is `None` and [`Metrics::absent`] says why, so a
/// reader never has to infer the reason from which fields are missing (DEC-127 item 7).
///
/// Everything here recomputes from the fields above it: `mean_return` from `return_sum` and
/// `period_count`, `variance` from both sums and the count, `sharpe_squared` from `mean_return`,
/// `risk_free_per_period`, and `variance`, and every annualized figure from its period figure and
/// `periods_per_year` (DEC-127 item 15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metrics {
    pub period_count: u32,
    pub first_date: mandate_time::Date,
    pub last_date: mandate_time::Date,
    pub starting_equity: Usd,
    pub ending_equity: Usd,
    pub net_pnl: Usd,
    pub total_return: Ratio,
    pub return_sum: Ratio,
    pub return_sum_of_squares: Ratio,
    pub mean_return: Ratio,
    pub variance: Option<Ratio>,
    pub volatility: Option<Ratio>,
    pub periods_per_year: u32,
    pub variance_annualized: Option<Ratio>,
    pub volatility_annualized: Option<Ratio>,
    pub risk_free_per_period: Ratio,
    pub sharpe_squared: Option<Ratio>,
    pub sharpe_sign: Sign,
    pub sharpe: Option<Ratio>,
    pub sharpe_squared_annualized: Option<Ratio>,
    pub sharpe_annualized: Option<Ratio>,
    pub max_drawdown: Ratio,
    pub max_drawdown_usd: Usd,
    pub max_drawdown_peak_period: u32,
    pub max_drawdown_trough_period: u32,
    pub buy_notional: Usd,
    pub sell_notional: Usd,
    pub traded_notional: Usd,
    pub fill_count: u32,
    pub turnover: Ratio,
    pub fees_total: Usd,
    pub fees_charged: Usd,
    pub fees_accrued: Usd,
    pub fees_asset: Usd,
    pub submitted_qty: Qty,
    pub filled_qty: Qty,
    pub absent: Option<AbsentStatistics>,
}

/// What a run's fills and fees came to, which the metrics need and the loop counts as it folds:
/// notionals by side (gross quantity × price, exact), the fill count, the fee totals the fold
/// reports, and the submitted and filled quantities (DEC-127 items 10, 11, and 22).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Totals {
    pub buy_notional: Usd,
    pub sell_notional: Usd,
    pub fill_count: u32,
    pub fees_total: Usd,
    pub fees_charged: Usd,
    pub fees_accrued: Usd,
    pub fees_asset: Usd,
    pub submitted_qty: Qty,
    pub filled_qty: Qty,
}

impl Metrics {
    /// Every figure of one run, from its equity series and totals.
    ///
    /// `starting_equity` is the equity before the first bar and `observations` the period closes, in
    /// order. Period returns are `round((Eₖ − Eₖ₋₁) ÷ Eₖ₋₁, 12, half_even)`; the variance is the
    /// sample variance with divisor n − 1 as one rounding of
    /// `(n × Σr² − (Σr)²) ÷ (n × (n − 1))`; the volatility is the 12-place ceiling root of it; the
    /// Sharpe is reported squared, with its sign, plus a root rounded towards minus infinity; the
    /// maximum drawdown is the largest `round((Pₖ − Eₖ) ÷ Pₖ, 12, ceiling)` over the running peak
    /// including the starting equity; and the turnover is
    /// `round(min(buys, sells) ÷ starting equity, 12, half_even)`.
    ///
    /// Errors: `equity_not_positive` for a period whose opening equity is not positive,
    /// `periods_per_year_not_positive` for a year with no periods, `no_bars` for an empty series, and
    /// the arithmetic errors of the formulas.
    pub fn of(
        starting_equity: Usd,
        observations: &[Observation],
        totals: &Totals,
        config: &MetricsConfig,
    ) -> Result<Self, BacktestError> {
        let _ = (starting_equity, observations, totals, config);
        Err(BacktestError::Unimplemented)
    }

    /// The period returns the figures are computed from, in order: `round((Eₖ − Eₖ₋₁) ÷ Eₖ₋₁, 12,
    /// half_even)`, with `equity_not_positive` when a period opens at or below zero.
    pub fn period_returns(
        starting_equity: Usd,
        observations: &[Observation],
    ) -> Result<Vec<Ratio>, BacktestError> {
        let _ = (starting_equity, observations);
        Err(BacktestError::Unimplemented)
    }
}
