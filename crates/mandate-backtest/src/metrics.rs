//! The report's figures, every one an exact decimal or one named rounding of one formula
//! ([task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md) "Metric definitions",
//! DEC-127 items 4 to 11 and 22).

use crate::report::{AbsentStatistics, Sign};
use crate::{BacktestError, MetricsConfig, Observation};
use mandate_num::{NumError, Qty, Ratio, Rounding, Usd};

/// Every figure the report rounds is rounded at 12 fractional digits (spec §2.1, DEC-127).
const REPORT_SCALE: u32 = 12;
/// A run needs two periods before dispersion is defined (DEC-127 items 5 and 7).
const FEWEST_PERIODS_WITH_DISPERSION: u32 = 2;

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
        if config.periods_per_year == 0 {
            return Err(BacktestError::PeriodsPerYearNotPositive);
        }
        let first = observations.first().ok_or(BacktestError::NoBars)?;
        let last = observations.last().ok_or(BacktestError::NoBars)?;
        let returns = Self::period_returns(starting_equity, observations)?;
        let period_count = u32::try_from(returns.len()).map_err(|_| NumError::Overflow)?;
        let net_pnl = last.equity.checked_sub(starting_equity)?;
        let return_sum = Ratio::sum(&returns)?;
        let return_sum_of_squares = Ratio::sum_of_squares(&returns)?;
        let mean_return = Ratio::mean(&returns)?;
        let dispersion = Dispersion::of(
            return_sum,
            return_sum_of_squares,
            period_count,
            mean_return,
            config,
        )?;
        let drawdown = Drawdown::of(starting_equity, observations)?;
        Ok(Self {
            period_count,
            first_date: first.date,
            last_date: last.date,
            starting_equity,
            ending_equity: last.equity,
            net_pnl,
            total_return: net_pnl.ratio_to(starting_equity, REPORT_SCALE, Rounding::HalfEven)?,
            return_sum,
            return_sum_of_squares,
            mean_return,
            variance: dispersion.variance,
            volatility: dispersion.volatility,
            periods_per_year: config.periods_per_year,
            variance_annualized: dispersion.variance_annualized,
            volatility_annualized: dispersion.volatility_annualized,
            risk_free_per_period: config.risk_free_per_period,
            sharpe_squared: dispersion.sharpe_squared,
            sharpe_sign: dispersion.sharpe_sign,
            sharpe: dispersion.sharpe,
            sharpe_squared_annualized: dispersion.sharpe_squared_annualized,
            sharpe_annualized: dispersion.sharpe_annualized,
            max_drawdown: drawdown.rung,
            max_drawdown_usd: drawdown.amount,
            max_drawdown_peak_period: drawdown.peak_period,
            max_drawdown_trough_period: drawdown.trough_period,
            buy_notional: totals.buy_notional,
            sell_notional: totals.sell_notional,
            traded_notional: totals.buy_notional.checked_add(totals.sell_notional)?,
            fill_count: totals.fill_count,
            turnover: totals.buy_notional.min(totals.sell_notional).ratio_to(
                starting_equity,
                REPORT_SCALE,
                Rounding::HalfEven,
            )?,
            fees_total: totals.fees_total,
            fees_charged: totals.fees_charged,
            fees_accrued: totals.fees_accrued,
            fees_asset: totals.fees_asset,
            submitted_qty: totals.submitted_qty,
            filled_qty: totals.filled_qty,
            absent: dispersion.absent,
        })
    }

    /// The period returns the figures are computed from, in order: `round((Eₖ − Eₖ₋₁) ÷ Eₖ₋₁, 12,
    /// half_even)`, with `equity_not_positive` when a period opens at or below zero.
    pub fn period_returns(
        starting_equity: Usd,
        observations: &[Observation],
    ) -> Result<Vec<Ratio>, BacktestError> {
        let mut opening = starting_equity;
        let mut returns = Vec::with_capacity(observations.len());
        for observation in observations {
            if opening.is_negative() || opening.is_zero() {
                return Err(BacktestError::EquityNotPositive(observation.period));
            }
            returns.push(observation.equity.checked_sub(opening)?.ratio_to(
                opening,
                REPORT_SCALE,
                Rounding::HalfEven,
            )?);
            opening = observation.equity;
        }
        Ok(returns)
    }
}

/// The dispersion block: the variance, its root, the squared Sharpe with its sign and root, the
/// annualized forms of each, and the reason any of them is absent (DEC-127 items 5 to 7).
///
/// Annualizing multiplies the **squared** figures by the integer period count, which is exact, and
/// each root is then taken once, in the direction that never flatters the run: the volatility up,
/// the Sharpe towards minus infinity.
struct Dispersion {
    variance: Option<Ratio>,
    volatility: Option<Ratio>,
    variance_annualized: Option<Ratio>,
    volatility_annualized: Option<Ratio>,
    sharpe_squared: Option<Ratio>,
    sharpe_sign: Sign,
    sharpe: Option<Ratio>,
    sharpe_squared_annualized: Option<Ratio>,
    sharpe_annualized: Option<Ratio>,
    absent: Option<AbsentStatistics>,
}

impl Dispersion {
    fn of(
        return_sum: Ratio,
        return_sum_of_squares: Ratio,
        period_count: u32,
        mean_return: Ratio,
        config: &MetricsConfig,
    ) -> Result<Self, BacktestError> {
        let excess = mean_return.checked_sub(config.risk_free_per_period)?;
        let sharpe_sign = sign_of(excess);
        let variance = if period_count < FEWEST_PERIODS_WITH_DISPERSION {
            None
        } else {
            Some(Ratio::sample_variance(
                return_sum,
                return_sum_of_squares,
                period_count,
            )?)
        };
        let volatility = variance.map(Ratio::root_ceiling).transpose()?;
        let variance_annualized = variance
            .map(|value| value.times_int(config.periods_per_year))
            .transpose()?;
        let volatility_annualized = variance_annualized.map(Ratio::root_ceiling).transpose()?;
        let dispersed = variance.filter(|value| *value != Ratio::ZERO);
        let sharpe_squared = dispersed
            .map(|value| Ratio::squared_quotient(excess, value))
            .transpose()?;
        let sharpe_squared_annualized = sharpe_squared
            .map(|value| value.times_int(config.periods_per_year))
            .transpose()?;
        let absent = match (variance, sharpe_squared) {
            (None, _) => Some(AbsentStatistics::FewerThanTwoPeriods),
            (Some(_), None) => Some(AbsentStatistics::ZeroVariance),
            (Some(_), Some(_)) => None,
        };
        Ok(Self {
            variance,
            volatility,
            variance_annualized,
            volatility_annualized,
            sharpe_squared,
            sharpe_sign,
            sharpe: sharpe_squared
                .map(|value| root_towards_negative_infinity(value, sharpe_sign))
                .transpose()?,
            sharpe_squared_annualized,
            sharpe_annualized: sharpe_squared_annualized
                .map(|value| root_towards_negative_infinity(value, sharpe_sign))
                .transpose()?,
            absent,
        })
    }
}

/// The 12-place root of a squared Sharpe, rounded towards minus infinity: the floor root with a
/// non-negative sign, the negated ceiling root with a negative one, so the figure never reads better
/// than the truth (DEC-127 item 7).
fn root_towards_negative_infinity(squared: Ratio, sign: Sign) -> Result<Ratio, NumError> {
    match sign {
        Sign::Negative => squared.root_ceiling().map(Ratio::negated),
        Sign::Zero | Sign::Positive => squared.root_floor(),
    }
}

/// The sign of an excess mean, which squaring loses.
fn sign_of(excess: Ratio) -> Sign {
    if excess.is_negative() {
        Sign::Negative
    } else if excess == Ratio::ZERO {
        Sign::Zero
    } else {
        Sign::Positive
    }
}

/// The largest peak-to-trough fall of the period closes, measured from the running peak including
/// the starting equity, each rung rounded **up** so a drawdown never understates a loss, and
/// reported at the earliest period attaining it (DEC-127 item 9).
struct Drawdown {
    rung: Ratio,
    amount: Usd,
    peak_period: u32,
    trough_period: u32,
}

impl Drawdown {
    fn of(starting_equity: Usd, observations: &[Observation]) -> Result<Self, BacktestError> {
        let mut peak = starting_equity;
        let mut peak_period = 0;
        let mut worst = Self {
            rung: Ratio::ZERO,
            amount: Usd::ZERO,
            peak_period: 0,
            trough_period: 0,
        };
        for observation in observations {
            if observation.equity > peak {
                peak = observation.equity;
                peak_period = observation.period;
            }
            let fall = peak.checked_sub(observation.equity)?;
            let rung = fall.ratio_to(peak, REPORT_SCALE, Rounding::Ceiling)?;
            if rung > worst.rung {
                worst = Self {
                    rung,
                    amount: fall,
                    peak_period,
                    trough_period: observation.period,
                };
            }
        }
        Ok(worst)
    }
}
