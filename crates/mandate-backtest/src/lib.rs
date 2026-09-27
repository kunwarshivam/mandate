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

use core::cmp::Ordering;
use std::collections::BTreeSet;

use mandate_accounting::{
    Account, AccountType, AccountingError, AssetClass, Execution, FeeFamily, Input, InstrumentId,
    Side,
};
use mandate_num::{Adverse, NumError, Price, Qty, Ratio, TickRule, Usd};
use mandate_sim::{
    Eligibility, FirstBarVolumes, Instrument, OrderEnd, OrderKind, OrderRef, Session, SimBar,
    SimConfig, SimError, SimFill, SimOrder, simulate,
};
use mandate_time::{Date, TimeError, UtcNanos, new_york_date_and_hour};

/// Spec §6.2 charges an equity trade date's accrued fees at 20:00 ET, the hour after-hours trading
/// ends (§4.3), so the first bar of that date at or past that hour carries the charge.
const CHARGE_HOUR_NEW_YORK: u8 = 20;

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
        Ok((submitted_qty(&self.orders)?, filled_qty(&self.fills)?))
    }
}

/// Σ of the submitted order quantities, exact (DEC-127 item 22).
fn submitted_qty(orders: &[SubmittedOrder]) -> Result<Qty, BacktestError> {
    let mut total = Qty::ZERO;
    for submitted in orders {
        total = total.checked_add(submitted.order.qty)?;
    }
    Ok(total)
}

/// Σ of the fill quantities as the fill model reports them, which is gross: a crypto buy's asset fee
/// leaves the position below it (DEC-127 item 22).
fn filled_qty(fills: &[RunFill]) -> Result<Qty, BacktestError> {
    let mut total = Qty::ZERO;
    for fill in fills {
        total = total.checked_add(fill.fill.qty)?;
    }
    Ok(total)
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
    let config = input.config;
    check_trade_dates(input)?;
    let strategy = walk(input, config.strategy)?;
    let benchmark = walk(
        input,
        Strategy::BuyAndHold {
            collar: config.strategy.collar(),
        },
    )?;
    let strategy_metrics = strategy.metrics(config)?;
    let benchmark_metrics = benchmark.metrics(config)?;
    Ok(BacktestRun {
        report: Report {
            report_version: REPORT_VERSION,
            inputs: InputDigests::of(input.bars, config)?,
            excess_total_return: strategy_metrics
                .total_return
                .checked_sub(benchmark_metrics.total_return)?,
            strategy: strategy_metrics,
            benchmark: benchmark_metrics,
        },
        equity: strategy.equity,
        fills: strategy.fills,
        orders: strategy.orders,
    })
}

/// A bar's `trade_date` is checked, never trusted: for an equity it is the calendar's trade date for
/// the bar's start, and for crypto, which has no trade date (spec §2.2), the UTC date of its start.
/// Either disagreement fails the run, so a mislabelled bar cannot silently move a period boundary
/// (DEC-127 item 20).
fn check_trade_dates(input: &BacktestInput<'_>) -> Result<(), BacktestError> {
    let config = input.config;
    for (index, bar) in input.bars.iter().enumerate() {
        let expected = match config.instrument.asset_class {
            AssetClass::UsEquity => config.fees.calendar.equity_trade_date(bar.start)?,
            AssetClass::Crypto => bar.start.date(),
        };
        if bar.trade_date != expected {
            return Err(BacktestError::BarTradeDateMismatch(index));
        }
    }
    Ok(())
}

/// One strategy's walk over the bars: the periods it observed, the fills it folded, the orders it
/// submitted, and the account they left behind, which holds the fee totals the report shows.
struct Walk {
    equity: Vec<Observation>,
    fills: Vec<RunFill>,
    orders: Vec<SubmittedOrder>,
    account: Account,
}

impl Walk {
    fn metrics(&self, config: &RunConfig) -> Result<Metrics, BacktestError> {
        Metrics::of(
            config.starting_cash,
            &self.equity,
            &self.totals()?,
            &config.metrics,
        )
    }

    /// What this walk's fills and fees came to: notionals by side from the gross quantity and the
    /// fill price, the fill count, and the fee totals the fold reports (DEC-127 items 10 and 11).
    fn totals(&self) -> Result<Totals, BacktestError> {
        let mut buy_notional = Usd::ZERO;
        let mut sell_notional = Usd::ZERO;
        for fill in &self.fills {
            let notional = fill.fill.qty.notional(fill.fill.price)?;
            match fill.execution.side {
                Side::Buy => buy_notional = buy_notional.checked_add(notional)?,
                Side::Sell => sell_notional = sell_notional.checked_add(notional)?,
            }
        }
        Ok(Totals {
            buy_notional,
            sell_notional,
            fill_count: u32::try_from(self.fills.len()).map_err(|_| NumError::Overflow)?,
            fees_total: self.account.fees_total()?,
            fees_charged: self.account.fees_charged(),
            fees_accrued: self.account.fees_accrued()?,
            fees_asset: self.account.asset_fees(),
            submitted_qty: submitted_qty(&self.orders)?,
            filled_qty: filled_qty(&self.fills)?,
        })
    }
}

/// The order the loop is waiting on: its run-wide index, the order itself, the fills the model
/// returned for it, how many of them have been folded, and how it ended.
struct Working {
    index: u32,
    order: SimOrder,
    fills: Vec<SimFill>,
    folded: usize,
    end: OrderEnd,
}

impl Working {
    /// The fills of this order in bar `index`, in the order the model returned them, each with the
    /// execution built from it: `o<index>` as the client order ID, `o<index>-f<ordinal>` as the fill
    /// ID, and the bar's start as the execution instant (DEC-127 item 21).
    fn fold_bar(
        &mut self,
        index: usize,
        at: UtcNanos,
        config: &RunConfig,
    ) -> Result<Vec<RunFill>, BacktestError> {
        let mut folded = Vec::new();
        while let Some(fill) = self
            .fills
            .get(self.folded)
            .filter(|fill| fill.bar == index)
            .cloned()
        {
            let execution = Execution {
                fill_id: format!("o{}-f{}", self.index, self.folded),
                client_order_id: Some(format!("o{}", self.index)),
                instrument: config.instrument_id.clone(),
                asset_class: config.instrument.asset_class,
                side: self.order.side,
                qty_gross: fill.qty,
                price: fill.price,
                liquidity: fill.liquidity,
                executed_at: at,
            };
            self.folded = self.folded.checked_add(1).ok_or(NumError::Overflow)?;
            folded.push(RunFill { fill, execution });
        }
        Ok(folded)
    }

    /// The bar this order stopped working in — its last fill's, or the `at_bar` a day order's
    /// remainder was canceled after, both read from the model rather than recomputed — and `None`
    /// while it is still working (DEC-127 item 18).
    fn stopped_in(&self) -> Option<usize> {
        let last_fill = self.fills.last().map(|fill| fill.bar);
        match self.end {
            OrderEnd::Filled => last_fill,
            OrderEnd::Expired { at_bar } => Some(last_fill.map_or(at_bar, |fill| fill.max(at_bar))),
            OrderEnd::Open => None,
        }
    }

    /// Whether this order had stopped working before bar `index`. An order works through the end of
    /// the bar in which it stops (spec §5.3 rule 6), so that bar submits no replacement.
    fn free_before(&self, index: usize) -> bool {
        self.stopped_in()
            .is_some_and(|stopped| matches!(stopped.cmp(&index), Ordering::Less))
    }
}

/// Walks the bars once, in index order, doing at each bar what DEC-127 item 2 fixes: the cash
/// movements `Account::due` makes due, the bar's fills, the bar-close mark, the fee charges spec §6.2
/// and §6.3 date, the period observation when this bar closes a period, and last the decision.
fn walk(input: &BacktestInput<'_>, strategy: Strategy) -> Result<Walk, BacktestError> {
    let config = input.config;
    let mut account = Account::opening(config.account_type, config.starting_cash, []);
    let mut equity: Vec<Observation> = Vec::new();
    let mut fills: Vec<RunFill> = Vec::new();
    let mut orders: Vec<SubmittedOrder> = Vec::new();
    let mut working: Option<Working> = None;
    let mut closes: Vec<Price> = Vec::new();
    let mut dates: BTreeSet<Date> = BTreeSet::new();
    let mut charged: BTreeSet<Date> = BTreeSet::new();
    let mut period: u32 = 0;
    for ((index, bar), closes_period) in input.bars.iter().enumerate().zip(closing_bars(input.bars))
    {
        dates.insert(bar.trade_date);
        for due in account.due(bar.start)? {
            account = account.apply(&due, &config.fees)?.account;
        }
        if let Some(order) = working.as_mut() {
            for fill in order.fold_bar(index, bar.start, config)? {
                account = account
                    .apply(&Input::Fill(fill.execution.clone()), &config.fees)?
                    .account;
                fills.push(fill);
            }
        }
        account = account
            .apply(
                &Input::Mark {
                    instrument: config.instrument_id.clone(),
                    price: bar.close,
                },
                &config.fees,
            )?
            .account;
        for day in charges_due(bar, &dates, &mut charged, config.instrument.asset_class)? {
            account = account
                .apply(
                    &Input::FeesCharged {
                        family: fee_family(config.instrument.asset_class),
                        day,
                    },
                    &config.fees,
                )?
                .account;
        }
        if closes_period {
            period = period.checked_add(1).ok_or(NumError::Overflow)?;
            equity.push(Observation {
                period,
                date: bar.trade_date,
                bar: index,
                equity: account.equity()?,
            });
            closes.push(bar.close);
        }
        let free = working
            .as_ref()
            .is_none_or(|order| order.free_before(index));
        let Some(next) = input
            .bars
            .get(index.checked_add(1).ok_or(NumError::Overflow)?)
        else {
            continue;
        };
        let decides = match strategy {
            Strategy::BuyAndHold { .. } => index == 0,
            Strategy::MovingAverageCrossover(_) => closes_period,
        };
        if !free || !decides {
            continue;
        }
        let signal = match strategy {
            Strategy::BuyAndHold { .. } => strategy.signal(&[bar.close])?,
            Strategy::MovingAverageCrossover(_) => strategy.signal(&closes)?,
        };
        let position = account.position(&config.instrument_id).qty();
        let Some((order, limit)) = intent(
            signal,
            strategy,
            config,
            &account,
            bar.close,
            position.abs(),
            next.start,
        )?
        else {
            continue;
        };
        let submitted = u32::try_from(orders.len()).map_err(|_| NumError::Overflow)?;
        let numbered = OrderRef::new(orders.len());
        let outcome = simulate(
            &config.sim,
            &config.instrument,
            input.bars,
            input.coverage_start,
            input.first_bar_volumes,
            &[order],
        )?;
        let end_of_the_only_order = outcome.end_of(OrderRef::new(0)).unwrap_or(OrderEnd::Open);
        orders.push(SubmittedOrder {
            index: submitted,
            decided_at_bar: index,
            order,
            limit,
            end: end_of_the_only_order,
        });
        working = Some(Working {
            index: submitted,
            order,
            fills: outcome
                .fills
                .into_iter()
                .map(|fill| SimFill {
                    order: numbered,
                    ..fill
                })
                .collect(),
            folded: 0,
            end: end_of_the_only_order,
        });
    }
    Ok(Walk {
        equity,
        fills,
        orders,
        account,
    })
}

/// The order a signal asks for, with its tick-rounded limit, or `None` when the strategy wants
/// nothing: an entry only when flat and only for a quantity above zero, an exit only when a position
/// is held, and never an add, a short, or a second working order (spec §5.1, §5.3, DEC-32).
fn intent(
    signal: Signal,
    strategy: Strategy,
    config: &RunConfig,
    account: &Account,
    close: Price,
    position: Qty,
    decided_at: UtcNanos,
) -> Result<Option<(SimOrder, Price)>, BacktestError> {
    let (side, limit, qty) = match signal {
        Signal::Undecided => return Ok(None),
        Signal::Long => {
            if !position.is_zero() {
                return Ok(None);
            }
            let limit = close
                .slipped(strategy.collar(), Adverse::Up)?
                .on_tick(config.tick, Adverse::Up)?;
            let budget = strategy.entry_budget(account.cash_total()?);
            (
                Side::Buy,
                limit,
                budget.shares_at(limit, config.instrument.increment)?,
            )
        }
        Signal::Flat => {
            if position.is_zero() {
                return Ok(None);
            }
            let limit = close
                .slipped(strategy.collar(), Adverse::Down)?
                .on_tick(config.tick, Adverse::Down)?;
            (Side::Sell, limit, position)
        }
    };
    if qty.is_zero() {
        return Ok(None);
    }
    Ok(Some((
        SimOrder {
            side,
            qty,
            kind: OrderKind::Limit { limit },
            tif: strategy.time_in_force(config.instrument.asset_class),
            extended_hours: false,
            eligible_from: Eligibility::DecidedAt {
                at: decided_at,
                approval_required: false,
            },
        },
        limit,
    )))
}

/// Whether each bar closes its period: the last `Session::Regular` bar of an equity trade date,
/// which is spec §8.2's official close, or the last bar of a crypto UTC date. A trade date the input
/// covers only outside the regular session is no period, and its bars are folded into the next one
/// (DEC-127 item 3).
fn closing_bars(bars: &[SimBar]) -> Vec<bool> {
    let mut later: BTreeSet<Date> = BTreeSet::new();
    let mut closing: Vec<bool> = bars
        .iter()
        .rev()
        .map(|bar| {
            matches!(bar.session, Session::Regular | Session::Continuous)
                && later.insert(bar.trade_date)
        })
        .collect();
    closing.reverse();
    closing
}

/// The days whose accrued fees this bar is the first to reach the charging instant of, in date
/// order: 20:00 ET on an equity trade date (spec §6.2) and 00:00 UTC after a crypto date (spec
/// §6.3). Nothing is swept at the end of a run, so an accrual whose instant the bars never reach
/// stays accrued, exactly as the fold would hold it (DEC-127 item 17).
///
/// Both boundaries are read through [`Ord`], in place rather than through a helper: no fixture has a
/// bar that first reaches a date's charge hour later than 20:59 ET, and none ends a crypto run with a
/// fee accrued on the run's own last UTC date, so a shifted operator or a helper's constant answer
/// there would be indistinguishable from the rule.
fn charges_due(
    bar: &SimBar,
    dates: &BTreeSet<Date>,
    charged: &mut BTreeSet<Date>,
    asset_class: AssetClass,
) -> Result<Vec<Date>, BacktestError> {
    let reached: Vec<Date> = match asset_class {
        AssetClass::UsEquity => {
            let (today, hour) = new_york_date_and_hour(bar.start)?;
            let reached_the_charge_hour = matches!(
                hour.cmp(&CHARGE_HOUR_NEW_YORK),
                Ordering::Equal | Ordering::Greater
            );
            dates
                .iter()
                .copied()
                .filter(|day| {
                    reached_the_charge_hour
                        && matches!(today.cmp(day), Ordering::Equal | Ordering::Greater)
                })
                .collect()
        }
        AssetClass::Crypto => {
            let today = bar.start.date();
            dates
                .iter()
                .copied()
                .filter(|day| matches!(day.cmp(&today), Ordering::Less))
                .collect()
        }
    };
    Ok(reached
        .into_iter()
        .filter(|day| charged.insert(*day))
        .collect())
}

/// The fee family an instrument's fills accrue under: equities per trade date, crypto per UTC day
/// (spec §6.2, §6.3).
fn fee_family(asset_class: AssetClass) -> FeeFamily {
    match asset_class {
        AssetClass::UsEquity => FeeFamily::Equities,
        AssetClass::Crypto => FeeFamily::Crypto,
    }
}
