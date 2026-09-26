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
//! The backtest loop and its metrics report (backlog E4-2, PRD FR-4.1, FR-4.2, FR-4.5;
//! [task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md), DEC-127): bars in, a signal
//! from a deliberately simple baseline strategy, orders through `mandate-sim`'s §6.4 fill model,
//! fills through `mandate-accounting`'s fold, a mark at every bar's close, and one report an owner
//! can read.
//!
//! [`run`] is pure. It reads no clock, no file, and no randomness, keeps every collection ordered,
//! and returns the same [`BacktestRun`] for the same inputs, so a report's canonical bytes and their
//! SHA-256 are the story's acceptance criterion (FR-4.5, ADR-0001 ES-21).
//!
//! Every figure is an exact decimal or one named rounding of one formula (spec §2.1): the report's
//! exact fields are the *squared* statistics, the variance and the squared Sharpe, so annualizing
//! them is a multiplication by an integer period count, and the only chosen roundings are the
//! 12-place roots, each moved to the side that does not flatter the run — volatility up, Sharpe
//! towards minus infinity, every drawdown rung up. A report that flatters a run misleads the owner
//! who reads it before going live (DEC-97), which is why this crate is safety-critical.
//!
//! The loop re-derives nothing the fill model owns. Each submitted order is simulated over the
//! **whole** bar slice, because a bar with no earlier bar of its session in the slice it is given is
//! that session's first covered bar (DEC-106 item 4), and a day order's cancellation is read back
//! from [`mandate_sim::SimOutcome::end_of`] rather than recomputed (DEC-127 item 18).

mod metrics;
mod report;
mod strategy;

pub use metrics::{Metrics, Totals};
pub use report::{AbsentStatistics, InputDigests, REPORT_VERSION, Report, Sign};
pub use strategy::{Signal, Strategy, StrategyConfig};

use mandate_accounting::{AccountType, AccountingError, Execution, InstrumentId};
use mandate_num::{NumError, Price, Qty, Ratio, TickRule, Usd};
use mandate_sim::{
    FirstBarVolumes, Instrument, OrderEnd, SimBar, SimConfig, SimError, SimFill, SimOrder,
};
use mandate_time::{Date, TimeError, UtcNanos};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BacktestError {
    #[error("a backtest needs at least one bar")]
    NoBars,
    #[error("bar {0} is labelled with a trade date the calendar does not give its start")]
    BarTradeDateMismatch(usize),
    #[error("period {0} opens with an equity that is not positive, so no return is defined")]
    EquityNotPositive(u32),
    #[error("a crossover needs a fast window below its slow window, both above zero")]
    StrategyWindowsCrossed,
    #[error("a year must hold at least one period")]
    PeriodsPerYearNotPositive,
    /// The stub [`run`] of this story's tests PR returns this, so every pending test fails on it
    /// (DEC-77, DEC-83); the implementation PR replaces the stubs and removes the variant.
    #[error("the backtest loop is not implemented yet")]
    Unimplemented,
    #[error(transparent)]
    Sim(#[from] SimError),
    #[error(transparent)]
    Accounting(#[from] AccountingError),
    #[error(transparent)]
    Num(#[from] NumError),
    #[error(transparent)]
    Time(#[from] TimeError),
}

impl BacktestError {
    /// Stable reason code (ADR-0001 ES-09).
    pub fn code(&self) -> &'static str {
        match self {
            Self::NoBars => "no_bars",
            Self::BarTradeDateMismatch(_) => "bar_trade_date_mismatch",
            Self::EquityNotPositive(_) => "equity_not_positive",
            Self::StrategyWindowsCrossed => "strategy_windows_crossed",
            Self::PeriodsPerYearNotPositive => "periods_per_year_not_positive",
            Self::Unimplemented => "unimplemented",
            Self::Sim(e) => e.code(),
            Self::Accounting(e) => e.code(),
            Self::Num(e) => e.code(),
            Self::Time(e) => e.code(),
        }
    }
}

/// How a run turns its period-return series into the report's statistics (DEC-127 items 3, 5, 6,
/// and 7). `periods_per_year` annualizes the squared figures exactly: 252 for equity trade dates,
/// 365 for crypto days. `risk_free_per_period` is subtracted from the mean before the Sharpe and is
/// zero unless a run states otherwise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetricsConfig {
    pub periods_per_year: u32,
    pub risk_free_per_period: Ratio,
}

/// Everything a run is pinned to, hashed into the report so a reader knows which configuration
/// produced it (FR-4.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunConfig {
    pub sim: SimConfig,
    pub fees: mandate_accounting::Config,
    pub account_type: AccountType,
    pub instrument_id: InstrumentId,
    pub instrument: Instrument,
    pub tick: TickRule,
    pub starting_cash: Usd,
    pub strategy: Strategy,
    pub metrics: MetricsConfig,
}

/// One run's inputs. `bars` is one instrument's bars in strictly increasing order, each carrying the
/// session labels `mandate-sim` reads; `coverage_start` and `first_bar_volumes` are passed through to
/// the fill model unchanged (spec §6.4 rule 3, DEC-106 item 4).
pub struct BacktestInput<'a> {
    pub config: &'a RunConfig,
    pub bars: &'a [SimBar],
    pub coverage_start: UtcNanos,
    pub first_bar_volumes: &'a dyn FirstBarVolumes,
}

/// One period's closing equity, at the bar that closed it (DEC-127 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Observation {
    pub period: u32,
    pub date: Date,
    pub bar: usize,
    pub equity: Usd,
}

/// A fill as the loop folded it: the model's fill, and the execution built from it with the
/// identifiers of DEC-127 item 21.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFill {
    pub fill: SimFill,
    pub execution: Execution,
}

/// An order the strategy submitted: its run-wide index, the bar whose close decided it, the order
/// itself, its tick-rounded limit price, and how it ended (read from the fill model, never derived).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmittedOrder {
    pub index: u32,
    pub decided_at_bar: usize,
    pub order: SimOrder,
    pub limit: Price,
    pub end: OrderEnd,
}

/// What [`run`] produces: the report, the equity series it was computed from, every fill, and every
/// submitted order, all in the order the walk produced them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacktestRun {
    pub report: Report,
    pub equity: Vec<Observation>,
    pub fills: Vec<RunFill>,
    pub orders: Vec<SubmittedOrder>,
}

impl BacktestRun {
    /// The quantity every submitted order asked for, and the gross quantity that filled (DEC-127
    /// item 22).
    pub fn quantities(&self) -> Result<(Qty, Qty), BacktestError> {
        Err(BacktestError::Unimplemented)
    }
}

/// Runs the baseline strategy and the buy-and-hold benchmark over the same bars, fill model, fee
/// configuration, and starting cash, and reports both with the strategy's excess return
/// (DEC-127 item 13).
///
/// At each bar, in this order (DEC-127 item 2): the cash movements `Account::due` makes due, the
/// bar's fills, the bar-close mark (spec §8.2), the fee charges spec §6.2 and §6.3 date at 20:00 ET
/// and 00:00 UTC, the period observation when this bar closes a period, and last the strategy's
/// decision, timed at the next bar's start so nothing fills in the bar that decided it (§6.4 rule 1).
///
/// The buy-and-hold benchmark is the one exception to that last step: its single order is decided at
/// **bar 0's** close rather than at the first period close, because a benchmark that sat out its first
/// period would not be the comparison FR-4.2 asks for (DEC-127 item 13). Its decision is timed at bar
/// 1's start like any other, so it still cannot fill in the bar that decided it.
///
/// Errors: no bars; a bar whose `trade_date` is not the one the calendar gives its start; a period
/// whose opening equity is not positive; a crossover whose windows are crossed or zero; a year with
/// no periods; and the arithmetic, fill-model, accounting, and time errors it wraps.
pub fn run(input: &BacktestInput<'_>) -> Result<BacktestRun, BacktestError> {
    let _ = input;
    Err(BacktestError::Unimplemented)
}
