//! Hand-calculated cases for the backtest loop and its metrics
//! ([task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md), DEC-127).
//!
//! Every figure in the metric cases was recomputed from the definitions, by hand and in an
//! independent script at 80 digits, on the series the brief's "Fixture check before any code" names:
//! the main series 100000, 101000, 99000, 103000 and the three degenerate series. The loop cases
//! state the arithmetic in their doc comments; where a value belongs to the fill model, the case
//! compares the loop with `simulate` run directly over the same bars rather than restating §6.4.

mod common;

use common::*;
use mandate_accounting::{Side, TafCapBasis};
use mandate_backtest::{
    AbsentStatistics, BacktestError, Metrics, MetricsConfig, Observation, Sign, Strategy,
    StrategyConfig, Totals,
};
use mandate_num::{Qty, Ratio, Usd};

/// The observation series of a run whose equity closed at each of `equities`, one period per
/// trading day from 2026-09-21.
fn observations(equities: &[&str]) -> Vec<Observation> {
    let days = [
        "2026-09-21",
        "2026-09-22",
        "2026-09-23",
        "2026-09-24",
        "2026-09-25",
    ];
    equities
        .iter()
        .enumerate()
        .map(|(index, equity)| Observation {
            period: u32::try_from(index + 1).unwrap(),
            date: d(days[index]),
            bar: index,
            equity: usd(equity),
        })
        .collect()
}

/// Nothing traded: the totals of a run whose equity moved only by marks.
fn no_trades() -> Totals {
    Totals {
        buy_notional: Usd::ZERO,
        sell_notional: Usd::ZERO,
        fill_count: 0,
        fees_total: Usd::ZERO,
        fees_charged: Usd::ZERO,
        fees_accrued: Usd::ZERO,
        fees_asset: Usd::ZERO,
        submitted_qty: Qty::ZERO,
        filled_qty: Qty::ZERO,
    }
}

/// One round trip: 50,000 USD bought and 50,600 USD sold.
fn one_round_trip() -> Totals {
    Totals {
        buy_notional: usd("50000"),
        sell_notional: usd("50600"),
        fill_count: 2,
        submitted_qty: qty("1000"),
        filled_qty: qty("1000"),
        ..no_trades()
    }
}

/// The brief's main fixture, with every figure recomputed from the definitions:
/// returns 0.01, −0.019801980198, 0.040404040404; `return_sum` 0.030602060206;
/// `return_sum_of_squares` 0.00212460490073004860242; `mean_return` 0.010200686735;
/// `variance` 0.000906221436 (exact 0.000906221435556416174137333…); `volatility` 0.030103512022;
/// `variance_annualized` 0.228367801872; `volatility_annualized` 0.477878438384;
/// `sharpe_squared` 0.11482183684; `sharpe` 0.338853710087;
/// `sharpe_squared_annualized` 28.93510288368; `sharpe_annualized` 5.379135886337;
/// `max_drawdown` 0.019801980199 at peak 1 and trough 2, 2000 USD; `turnover` 0.5.
#[test]
#[ignore = "pending E4-2"]
fn a_report_carries_every_figure_the_prd_names() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["101000", "99000", "103000"]),
        &one_round_trip(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.period_count, 3);
    assert_eq!(metrics.starting_equity, usd("100000"));
    assert_eq!(metrics.ending_equity, usd("103000"));
    assert_eq!(metrics.net_pnl, usd("3000"));
    assert_eq!(metrics.total_return, ratio("0.03"));
    assert_eq!(metrics.return_sum, ratio("0.030602060206"));
    assert_eq!(
        metrics.return_sum_of_squares,
        ratio("0.00212460490073004860242")
    );
    assert_eq!(metrics.mean_return, ratio("0.010200686735"));
    assert_eq!(metrics.variance, Some(ratio("0.000906221436")));
    assert_eq!(metrics.volatility, Some(ratio("0.030103512022")));
    assert_eq!(metrics.variance_annualized, Some(ratio("0.228367801872")));
    assert_eq!(metrics.volatility_annualized, Some(ratio("0.477878438384")));
    assert_eq!(metrics.sharpe_squared, Some(ratio("0.11482183684")));
    assert_eq!(metrics.sharpe_sign, Sign::Positive);
    assert_eq!(metrics.sharpe, Some(ratio("0.338853710087")));
    assert_eq!(
        metrics.sharpe_squared_annualized,
        Some(ratio("28.93510288368"))
    );
    assert_eq!(metrics.sharpe_annualized, Some(ratio("5.379135886337")));
    assert_eq!(metrics.max_drawdown, ratio("0.019801980199"));
    assert_eq!(metrics.max_drawdown_usd, usd("2000"));
    assert_eq!(metrics.max_drawdown_peak_period, 1);
    assert_eq!(metrics.max_drawdown_trough_period, 2);
    assert_eq!(metrics.turnover, ratio("0.5"));
    assert_eq!(metrics.absent, None, "every figure is defined at 3 periods");
}

/// A single period defines a mean but no dispersion: `absent` says which (DEC-127 item 7).
#[test]
#[ignore = "pending E4-2"]
fn one_period_leaves_the_dispersion_fields_absent() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["101000"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.mean_return, ratio("0.01"));
    assert_eq!(metrics.variance, None);
    assert_eq!(metrics.volatility, None);
    assert_eq!(metrics.variance_annualized, None);
    assert_eq!(metrics.volatility_annualized, None);
    assert_eq!(metrics.sharpe_squared, None);
    assert_eq!(metrics.sharpe, None);
    assert_eq!(metrics.sharpe_annualized, None);
    assert_eq!(metrics.absent, Some(AbsentStatistics::FewerThanTwoPeriods));
    assert_eq!(metrics.sharpe_sign, Sign::Positive, "the mean is defined");
}

/// A flat run has a variance of zero, so the Sharpe is absent rather than infinite, and every other
/// figure is zero.
#[test]
#[ignore = "pending E4-2"]
fn a_flat_run_leaves_the_sharpe_absent_with_zero_variance() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["100000", "100000"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.total_return, Ratio::ZERO);
    assert_eq!(metrics.mean_return, Ratio::ZERO);
    assert_eq!(metrics.variance, Some(Ratio::ZERO));
    assert_eq!(metrics.volatility, Some(Ratio::ZERO));
    assert_eq!(metrics.sharpe_squared, None);
    assert_eq!(metrics.sharpe, None);
    assert_eq!(metrics.sharpe_sign, Sign::Zero);
    assert_eq!(metrics.max_drawdown, Ratio::ZERO);
    assert_eq!(metrics.absent, Some(AbsentStatistics::ZeroVariance));
}

/// A run whose equity rises by less than a picopoint a period: 100000 → 100000.000000001 →
/// 100000.000000002 has exact period returns of 1 ÷ 10¹⁴, which round to zero at 12 places, so the
/// variance is zero and the Sharpe is absent as `zero_variance`. The **exact** net P&L is still
/// positive while the total return is zero: a figure below the report's last place is not a figure.
#[test]
#[ignore = "pending E4-2"]
fn a_variance_that_rounds_to_zero_leaves_the_sharpe_absent_as_zero_variance() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["100000.000000001", "100000.000000002"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.variance, Some(Ratio::ZERO));
    assert_eq!(metrics.absent, Some(AbsentStatistics::ZeroVariance));
    assert_eq!(metrics.sharpe_squared, None);
    assert_eq!(
        metrics.net_pnl,
        usd("0.000000002"),
        "the run did make money"
    );
    assert_eq!(
        metrics.total_return,
        Ratio::ZERO,
        "2 ÷ 10¹⁴ rounds to zero at the twelfth place"
    );
    assert_eq!(metrics.return_sum, Ratio::ZERO);
}

/// A constant rise never drops below its peak, so the drawdown is zero and both periods it reports
/// are period 0, the starting equity (the brief's first degenerate series).
#[test]
#[ignore = "pending E4-2"]
fn a_run_that_only_rises_has_no_drawdown() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["101000", "102010"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.max_drawdown, Ratio::ZERO);
    assert_eq!(metrics.max_drawdown_usd, Usd::ZERO);
    assert_eq!(metrics.max_drawdown_peak_period, 0);
    assert_eq!(metrics.max_drawdown_trough_period, 0);
    assert_eq!(metrics.total_return, ratio("0.0201"));
    assert_eq!(
        metrics.variance,
        Some(Ratio::ZERO),
        "two identical returns have no dispersion"
    );
}

/// The drawdown is measured from the running peak, not from the start: 100000 → 120000 → 90000 has
/// round((120000 − 90000) ÷ 120000, 12, ceiling) = 0.25, not 0.1.
#[test]
#[ignore = "pending E4-2"]
fn a_drawdown_after_a_new_peak_is_measured_from_that_peak() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["120000", "90000"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.max_drawdown, ratio("0.25"));
    assert_eq!(metrics.max_drawdown_usd, usd("30000"));
    assert_eq!(metrics.max_drawdown_peak_period, 1);
    assert_eq!(metrics.max_drawdown_trough_period, 2);
}

/// A run that only falls measures from the starting equity, which is the first peak:
/// 100000 → 99000 → 98000 gives 0.02 at peak 0 and trough 2, and a negative Sharpe whose magnitude
/// is the **negated ceiling** root of the squared figure (−140.70763353166 for 19798.638134079871).
#[test]
#[ignore = "pending E4-2"]
fn a_run_that_only_falls_measures_from_the_starting_equity() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["99000", "98000"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.max_drawdown, ratio("0.02"));
    assert_eq!(metrics.max_drawdown_usd, usd("2000"));
    assert_eq!(metrics.max_drawdown_peak_period, 0);
    assert_eq!(metrics.max_drawdown_trough_period, 2);
    assert_eq!(metrics.total_return, ratio("-0.02"));
    assert_eq!(metrics.mean_return, ratio("-0.01005050505"));
    assert_eq!(metrics.variance, Some(ratio("0.000000005102")));
    assert_eq!(metrics.volatility, Some(ratio("0.000071428286")));
}

/// A negative excess mean reports a negative sign and a Sharpe rounded towards minus infinity, so
/// the magnitude comes from the ceiling root and the figure never reads better than the truth.
#[test]
#[ignore = "pending E4-2"]
fn a_negative_sharpe_rounds_away_from_zero() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["99000", "98000"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.sharpe_sign, Sign::Negative);
    assert_eq!(metrics.sharpe_squared, Some(ratio("19798.638134079871")));
    assert_eq!(metrics.sharpe, Some(ratio("-140.70763353166")));
    assert_eq!(metrics.sharpe_annualized, Some(ratio("-2233.66443535911")));
    let squared = metrics.sharpe.unwrap();
    assert!(
        squared < Ratio::ZERO,
        "a negative Sharpe stays negative after rooting"
    );
}

/// Annualizing multiplies the variance and the squared Sharpe by the period count, exactly, and the
/// annualized roots are taken from those products: 0.000906221436 × 252 = 0.228367801872 with a
/// ceiling root of 0.477878438384, and 0.11482183684 × 252 = 28.93510288368 with a floor root of
/// 5.379135886337. Multiplying the period volatility by a root of 252 is not an exact decimal and is
/// not what the report does.
#[test]
#[ignore = "pending E4-2"]
fn annualizing_scales_the_squares_not_the_roots() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["101000", "99000", "103000"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.variance, Some(ratio("0.000906221436")));
    assert_eq!(metrics.variance_annualized, Some(ratio("0.228367801872")));
    assert_eq!(metrics.volatility_annualized, Some(ratio("0.477878438384")));
    assert_eq!(metrics.sharpe_squared, Some(ratio("0.11482183684")));
    assert_eq!(
        metrics.sharpe_squared_annualized,
        Some(ratio("28.93510288368"))
    );
    assert_eq!(metrics.sharpe_annualized, Some(ratio("5.379135886337")));
}

/// The variance divides by n − 1, not n: on the main series the sample variance is 0.000906221436,
/// where a population divisor of 3 rather than 2 would give two thirds of it, 0.000604147624.
#[test]
#[ignore = "pending E4-2"]
fn variance_uses_the_sample_divisor() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["101000", "99000", "103000"]),
        &no_trades(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.period_count, 3, "so the divisor is 2");
    assert_eq!(metrics.variance, Some(ratio("0.000906221436")));
    assert_ne!(
        metrics.variance,
        Some(ratio("0.000604147624")),
        "that is the population variance, with n rather than n − 1"
    );
}

/// One round trip of 1,000 shares turns the position over once: min(50000, 50600) ÷ 100000 = 0.5,
/// where counting both sides would report 1.006 and the half-sum 0.503.
#[test]
#[ignore = "pending E4-2"]
fn a_single_round_trip_turns_over_its_notional_once() {
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["100600"]),
        &one_round_trip(),
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.turnover, ratio("0.5"));
    assert_eq!(metrics.buy_notional, usd("50000"));
    assert_eq!(metrics.sell_notional, usd("50600"));
    assert_eq!(metrics.traded_notional, usd("100600"));
    assert_eq!(metrics.fill_count, 2);
}

/// The total return is net of fees, because equity subtracts accrued fees (spec §8.2): the same run
/// with the fees switched off ends strictly higher, by exactly the fee total.
#[test]
#[ignore = "pending E4-2"]
fn the_total_return_is_net_of_accrued_and_charged_fees() {
    let mut free = run_config(equity(), crossover(), "100000");
    free.fees = no_equity_fees();
    let charged = run_config(equity(), crossover(), "100000");

    let free_run = run(&free, &seven_days()).unwrap();
    let charged_run = run(&charged, &seven_days()).unwrap();

    assert_eq!(free_run.report.strategy.fees_total, Usd::ZERO);
    assert_eq!(charged_run.report.strategy.fees_total, usd("1.585045967"));
    assert!(
        charged_run.report.strategy.total_return < free_run.report.strategy.total_return,
        "fees are inside the return"
    );
    assert_eq!(
        charged_run.report.strategy.ending_equity,
        free_run
            .report
            .strategy
            .ending_equity
            .checked_sub(usd("1.585045967"))
            .unwrap(),
        "the whole difference is the fees"
    );
}

/// A year must hold at least one period.
#[test]
#[ignore = "pending E4-2"]
fn a_year_with_no_periods_is_an_error() {
    let outcome = Metrics::of(
        usd("100000"),
        &observations(&["101000", "99000"]),
        &no_trades(),
        &MetricsConfig {
            periods_per_year: 0,
            risk_free_per_period: Ratio::ZERO,
        },
    );

    assert_eq!(
        outcome.map(|_| ()),
        Err(BacktestError::PeriodsPerYearNotPositive)
    );
    assert_eq!(
        BacktestError::PeriodsPerYearNotPositive.code(),
        "periods_per_year_not_positive"
    );
}

/// A period that opens at zero equity has no return, and the report says so rather than inventing
/// one.
#[test]
#[ignore = "pending E4-2"]
fn a_period_opening_at_zero_equity_has_no_return() {
    let outcome = Metrics::of(
        usd("100000"),
        &observations(&["0", "1000"]),
        &no_trades(),
        &daily_equity_metrics(),
    );

    assert_eq!(
        outcome.map(|_| ()),
        Err(BacktestError::EquityNotPositive(2))
    );
}

/// The excess return is the exact difference of the two reported total returns, with no rounding of
/// its own, and the benchmark it is measured against actually traded.
#[test]
#[ignore = "pending E4-2"]
fn the_excess_return_is_the_difference_of_the_two_reported_returns() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &seven_days()).unwrap();

    let expected = run
        .report
        .strategy
        .total_return
        .checked_sub(run.report.benchmark.total_return)
        .unwrap();
    assert_eq!(run.report.excess_total_return, expected);
    assert!(
        run.report.benchmark.filled_qty > Qty::ZERO,
        "the benchmark is a tradable comparison, not an index line"
    );
}

/// Seven trading days of two regular bars each, from 2026-09-21, closing at 100.00, 101.00, 104.00,
/// 104.50, 103.00, 102.00, 102.00. The crossover goes long at period 3 (205 × 3 = 615 above
/// 305 × 2 = 610) and flat at period 5 (207.50 × 3 = 622.50 at or below 311.50 × 2 = 623), so the
/// entry is decided at bar 5 and fills on bar 6 and the exit is decided at bar 9 and fills on bar 10.
/// Period 7's decision falls on the last bar, which the loop never acts on.
fn seven_days() -> Vec<mandate_sim::SimBar> {
    vec![
        auction(
            "2026-09-21",
            "09:30",
            ["100.00", "100.20", "99.80", "100.10", "8000"],
        ),
        bar(
            "2026-09-21",
            "15:59",
            ["100.10", "100.20", "99.90", "100.00", "9000"],
        ),
        auction(
            "2026-09-22",
            "09:30",
            ["100.10", "101.20", "100.00", "101.10", "8000"],
        ),
        bar(
            "2026-09-22",
            "15:59",
            ["101.10", "101.20", "100.90", "101.00", "9000"],
        ),
        auction(
            "2026-09-23",
            "09:30",
            ["101.10", "104.20", "101.00", "104.10", "8000"],
        ),
        bar(
            "2026-09-23",
            "15:59",
            ["104.10", "104.20", "103.90", "104.00", "9000"],
        ),
        auction(
            "2026-09-24",
            "09:30",
            ["104.10", "104.60", "104.00", "104.50", "8000"],
        ),
        bar(
            "2026-09-24",
            "15:59",
            ["104.50", "104.60", "104.30", "104.50", "9000"],
        ),
        auction(
            "2026-09-25",
            "09:30",
            ["104.50", "104.60", "102.90", "103.10", "8000"],
        ),
        bar(
            "2026-09-25",
            "15:59",
            ["103.10", "103.20", "102.90", "103.00", "9000"],
        ),
        auction(
            "2026-09-28",
            "09:30",
            ["103.00", "103.10", "101.90", "102.00", "8000"],
        ),
        bar(
            "2026-09-28",
            "15:59",
            ["101.90", "102.10", "101.80", "102.00", "9000"],
        ),
        auction(
            "2026-09-29",
            "09:30",
            ["102.00", "102.10", "101.90", "102.00", "8000"],
        ),
        bar(
            "2026-09-29",
            "15:59",
            ["102.00", "102.10", "101.90", "102.00", "9000"],
        ),
    ]
}

/// The first `days` trading days of [`seven_days`], then `extra`: the fee cases end a run just before
/// or just after the instant spec §6.2 charges at.
fn days_then(days: usize, extra: Vec<mandate_sim::SimBar>) -> Vec<mandate_sim::SimBar> {
    let mut bars: Vec<_> = seven_days().into_iter().take(days * 2).collect();
    bars.extend(extra);
    bars
}

/// The entry is a day limit buy in the regular session, priced a collar above the signal period's
/// close and put on the tick against the order: 104.00 × (1 + 25 bps) = 104.26, already on the penny
/// grid, and 50,000 ÷ 104.26 truncates to 479 whole shares.
#[test]
#[ignore = "pending E4-2"]
fn an_entry_is_a_day_limit_buy_at_the_collar_above_the_close() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &seven_days()).unwrap();

    let entry = run
        .orders
        .first()
        .expect("the crossover goes long on day 4");
    assert_eq!(entry.index, 0);
    assert_eq!(entry.decided_at_bar, 5, "day 3's last regular bar");
    assert_eq!(entry.limit, price("104.26"));
    assert_eq!(entry.order.qty, qty("479"));
    assert_eq!(entry.order.side, Side::Buy);
    assert_eq!(entry.order.tif, mandate_sim::TimeInForce::Day);
    assert!(
        !entry.order.extended_hours,
        "an entry trades the regular session only"
    );
}

/// A limit below one dollar rounds on the finer Reg NMS tick: 0.50 × (1 + 25 bps) = 0.50125, which a
/// buy limit rounds down to 0.5012 (spec §2.1).
#[test]
#[ignore = "pending E4-2"]
fn a_collar_below_one_dollar_uses_the_finer_tick() {
    let config = run_config(equity(), crossover(), "10000");
    let bars = penny_stock_week();
    let run = run(&config, &bars).unwrap();

    let entry = run.orders.first().expect("the crossover goes long");
    assert_eq!(entry.limit, price("0.5012"));
}

/// The same week, priced under a dollar: closes 0.40, 0.45, 0.50, 0.52, 0.50.
fn penny_stock_week() -> Vec<mandate_sim::SimBar> {
    vec![
        auction(
            "2026-09-21",
            "09:30",
            ["0.40", "0.41", "0.39", "0.40", "80000"],
        ),
        bar(
            "2026-09-21",
            "15:59",
            ["0.40", "0.41", "0.39", "0.40", "90000"],
        ),
        auction(
            "2026-09-22",
            "09:30",
            ["0.40", "0.46", "0.40", "0.45", "80000"],
        ),
        bar(
            "2026-09-22",
            "15:59",
            ["0.45", "0.46", "0.44", "0.45", "90000"],
        ),
        auction(
            "2026-09-23",
            "09:30",
            ["0.45", "0.51", "0.45", "0.50", "80000"],
        ),
        bar(
            "2026-09-23",
            "15:59",
            ["0.50", "0.51", "0.49", "0.50", "90000"],
        ),
        auction(
            "2026-09-24",
            "09:30",
            ["0.50", "0.53", "0.49", "0.52", "80000"],
        ),
        bar(
            "2026-09-24",
            "15:59",
            ["0.52", "0.53", "0.51", "0.52", "90000"],
        ),
        auction(
            "2026-09-25",
            "09:30",
            ["0.52", "0.53", "0.49", "0.50", "80000"],
        ),
        bar(
            "2026-09-25",
            "15:59",
            ["0.50", "0.51", "0.49", "0.50", "90000"],
        ),
    ]
}

/// The crossover compares window sums by cross-multiplication, so no division and no rounding enter
/// the signal: with closes 100, 101, 104 the fast sum 205 times 3 beats the slow sum 305 times 2, and
/// with 104.50 and 103.00 added the fast sum 207.50 times 3 falls at or below 311.50 times 2.
#[test]
#[ignore = "pending E4-2"]
fn the_crossover_compares_sums_by_cross_multiplication() {
    let strategy = crossover();

    assert_eq!(
        strategy.signal(&[price("100"), price("101")]).unwrap(),
        mandate_backtest::Signal::Undecided,
        "three periods are needed before the slow window is full"
    );
    assert_eq!(
        strategy
            .signal(&[price("100"), price("101"), price("104")])
            .unwrap(),
        mandate_backtest::Signal::Long
    );
    assert_eq!(
        strategy
            .signal(&[price("104"), price("101"), price("100")])
            .unwrap(),
        mandate_backtest::Signal::Flat,
        "a falling series keeps the fast average below the slow one"
    );
    assert_eq!(
        strategy
            .signal(&[
                price("100"),
                price("101"),
                price("104"),
                price("104.50"),
                price("103")
            ])
            .unwrap(),
        mandate_backtest::Signal::Flat,
        "622.50 is below 623, so the fifth period of the fixture is flat"
    );
}

/// Equal averages are a tie, which resolves flat: ambiguity never adds risk (AGENTS.md rule 3).
/// With closes 100, 100, 100 the comparison is 200 × 3 = 300 × 2, so neither side is above.
#[test]
#[ignore = "pending E4-2"]
fn equal_averages_leave_the_strategy_flat() {
    let flat = [price("100"), price("100"), price("100")];
    assert_eq!(
        crossover().signal(&flat).unwrap(),
        mandate_backtest::Signal::Flat
    );
}

/// A crossover whose windows are crossed, or zero, is an error rather than a silent reordering.
#[test]
#[ignore = "pending E4-2"]
fn crossed_windows_are_an_error() {
    let crossed = Strategy::MovingAverageCrossover(StrategyConfig {
        fast_periods: 3,
        slow_periods: 3,
        collar: bps("25"),
        target_notional: usd("50000"),
    });
    assert_eq!(
        crossed.signal(&[price("100")]).map(|_| ()),
        Err(BacktestError::StrategyWindowsCrossed)
    );

    let zero = Strategy::MovingAverageCrossover(StrategyConfig {
        fast_periods: 0,
        slow_periods: 3,
        collar: bps("25"),
        target_notional: usd("50000"),
    });
    assert_eq!(
        zero.signal(&[price("100")]).map(|_| ()),
        Err(BacktestError::StrategyWindowsCrossed)
    );
}

/// An exit sells the whole position, not a quantity truncated to the increment (§5.3 rule 2): the
/// entry filled 479 shares on bar 6, so the exit decided at bar 9 submits 479, priced
/// `on_tick(103.00 × (1 − 25 bps), up)` = `on_tick(102.7425, up)` = 102.75.
#[test]
#[ignore = "pending E4-2"]
fn an_exit_sells_the_whole_position() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &seven_days()).unwrap();

    let exit = run
        .orders
        .iter()
        .find(|o| o.order.side == Side::Sell)
        .expect("period 5 is flat, and it is not the last period");
    assert_eq!(exit.index, 1);
    assert_eq!(exit.decided_at_bar, 9, "day 5's last regular bar");
    assert_eq!(exit.limit, price("102.75"));
    assert_eq!(exit.order.qty, qty("479"));
    let bought = run
        .fills
        .iter()
        .filter(|f| f.execution.side == Side::Buy)
        .fold(Qty::ZERO, |sum, f| sum.checked_add(f.fill.qty).unwrap());
    assert_eq!(
        exit.order.qty, bought,
        "the exit sells what the entry filled"
    );
}

/// A crypto buy pays its fee in the asset, so the position sits off the increment; the exit still
/// sells the whole position, and the gross filled quantity stays above it (DEC-127 item 22).
#[test]
#[ignore = "pending E4-2"]
fn a_crypto_exit_sells_the_part_the_asset_fee_left_off_the_increment() {
    let config = run_config(crypto(), crossover(), "100000");
    let run = run(&config, &crypto_days()).unwrap();

    let exit = run
        .orders
        .iter()
        .find(|o| o.order.side == Side::Sell)
        .expect("the crypto run exits on its sixth UTC day");
    let filled = run
        .fills
        .iter()
        .filter(|f| f.execution.side == Side::Buy)
        .fold(Qty::ZERO, |sum, f| sum.checked_add(f.fill.qty).unwrap());
    assert!(
        exit.order.qty < filled,
        "the asset fee left the position below the gross quantity"
    );
    assert_eq!(
        exit.order.tif,
        mandate_sim::TimeInForce::Gtc,
        "a continuous instrument has no session to cancel a day order at"
    );
    assert!(run.report.strategy.fees_asset > Usd::ZERO);
}

/// Seven UTC days of continuous bars, two a day, with the closes of [`seven_days`], so the entry
/// fills on the fourth day and the exit on the sixth; the seventh day's first bar is past 00:00 UTC
/// of the sixth, which is when spec §6.3 charges the sale's fee.
fn crypto_days() -> Vec<mandate_sim::SimBar> {
    let days = [
        ("2026-09-21", "100.00", "100.10"),
        ("2026-09-22", "100.10", "101.00"),
        ("2026-09-23", "101.00", "104.00"),
        ("2026-09-24", "104.00", "104.50"),
        ("2026-09-25", "104.50", "103.00"),
        ("2026-09-26", "103.00", "102.00"),
        ("2026-09-27", "102.00", "102.00"),
    ];
    let mut bars = Vec::new();
    for (day, open, close) in days {
        bars.push(continuous(
            day,
            "00:01",
            [open, "107.30", "99.50", close, "8000"],
        ));
        bars.push(continuous(
            day,
            "23:59",
            [close, "107.30", "99.50", close, "9000"],
        ));
    }
    bars
}

/// A period's mark is its closing bar's close (spec §8.2). The benchmark buys 996 shares — 800 on
/// bar 1, where the cap is 10% of bar 0's 8,000, and 196 on bar 2 — both at
/// `min(100.35, 100.10 × 1.0003)` = 100.13003, so the last observation is
/// `100000 − buy notional + 996 × 102.00 − fees`; the same figure marked at that bar's open, 102.00
/// against a close of 102.00, is why the fixture's last bar opens at 102.00 and the case compares the
/// two explicitly.
#[test]
#[ignore = "pending E4-2"]
fn the_period_mark_is_the_last_bars_close() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let mut bars = seven_days();
    bars[13] = bar(
        "2026-09-29",
        "15:59",
        ["101.50", "102.10", "101.40", "102.00", "9000"],
    );
    let run = run(&config, &bars).unwrap();

    let metrics = &run.report.strategy;
    let cash = usd("100000").checked_sub(metrics.buy_notional).unwrap();
    let closing = bars.len() - 1;
    let at_close = cash
        .checked_add(metrics.filled_qty.notional(bars[closing].close).unwrap())
        .unwrap()
        .checked_sub(metrics.fees_total)
        .unwrap();
    let at_open = cash
        .checked_add(metrics.filled_qty.notional(bars[closing].open).unwrap())
        .unwrap()
        .checked_sub(metrics.fees_total)
        .unwrap();

    let last = run.equity.last().unwrap();
    assert_eq!(last.bar, closing, "the week's last regular bar");
    assert_eq!(metrics.filled_qty, qty("996"));
    assert_eq!(last.equity, at_close);
    assert_ne!(
        last.equity, at_open,
        "the bar's open would have given a different equity"
    );
}

/// An after-hours bar never closes an equity period: §8.2's end of day is the official close, so the
/// period still closes at 15:59 and the after-hours bar's mark lands in the next period.
#[test]
#[ignore = "pending E4-2"]
fn an_after_hours_bar_does_not_close_an_equity_period() {
    let mut bars = seven_days();
    bars.insert(
        2,
        after_hours(
            "2026-09-21",
            "16:30",
            ["100.00", "110.00", "100.00", "110.00", "500"],
        ),
    );
    let config = run_config(equity(), buy_and_hold(), "100000");
    let run = run(&config, &bars).unwrap();

    let first = run.equity.first().unwrap();
    assert_eq!(first.date, d("2026-09-21"));
    assert_eq!(first.bar, 1, "the 15:59 regular bar, not the 16:30 one");
    assert_eq!(run.equity.len(), 7, "still seven periods");
}

/// A trade date the input covers only outside the regular session is no period at all; its bars are
/// still folded, so their marks reach the next period.
#[test]
#[ignore = "pending E4-2"]
fn a_date_covered_only_outside_the_regular_session_is_no_period() {
    let mut bars: Vec<_> = seven_days()
        .into_iter()
        .filter(|b| b.trade_date != d("2026-09-22"))
        .collect();
    bars.insert(
        2,
        pre_market(
            "2026-09-22",
            "08:00",
            ["100.00", "100.10", "99.90", "100.00", "500"],
        ),
    );
    let config = run_config(equity(), buy_and_hold(), "100000");
    let run = run(&config, &bars).unwrap();

    assert!(
        run.equity.iter().all(|o| o.date != d("2026-09-22")),
        "no regular bar on the 22nd, so no period for it"
    );
    assert_eq!(run.equity.len(), 6);
}

/// A fill in the period's closing bar is inside that period's equity: the observation is taken after
/// the bar's fills, its mark, and its charges, never before. The benchmark's first fill is 800 shares
/// on bar 1, which is also the bar that closes period 1, so the first observation already holds them.
#[test]
#[ignore = "pending E4-2"]
fn a_fill_in_the_last_bar_of_a_day_is_inside_that_days_equity() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let bars = seven_days();
    let run = run(&config, &bars).unwrap();

    let first_fill = run.fills.first().expect("the benchmark fills on bar 1");
    assert_eq!(first_fill.fill.bar, 1, "the bar that closes period 1");
    assert_eq!(first_fill.fill.qty, qty("800"), "10% of bar 0's 8,000");

    let first = run.equity.first().unwrap();
    assert_eq!(first.bar, 1);
    let held = first_fill.fill.qty;
    let cash = usd("100000")
        .checked_sub(held.notional(first_fill.fill.price).unwrap())
        .unwrap();
    let expected = cash
        .checked_add(held.notional(bars[1].close).unwrap())
        .unwrap()
        .checked_sub(usd("0.008"))
        .unwrap();
    assert_eq!(
        first.equity, expected,
        "cash after the fill, the position at bar 1's close, less CAT 0.00001 x 800"
    );
}

/// A settlement moves money between buckets without changing equity (spec §8.3, invariant I4), and it
/// posts at the start of the bar whose instant it falls before. The exit fills on 2026-09-28, so its
/// proceeds settle at 00:00 ET on 2026-09-29; with no fill and no charge left on that day, period 7's
/// equity equals period 6's exactly.
#[test]
#[ignore = "pending E4-2"]
fn a_settlement_posts_before_the_bars_fills() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &seven_days()).unwrap();

    let sixth = run
        .equity
        .iter()
        .find(|o| o.date == d("2026-09-28"))
        .expect("period 6");
    let seventh = run
        .equity
        .iter()
        .find(|o| o.date == d("2026-09-29"))
        .expect("period 7");
    assert_eq!(
        seventh.equity, sixth.equity,
        "a settlement is a bucket transfer, not a gain"
    );
    assert!(
        run.fills
            .iter()
            .all(|f| f.execution.executed_at.date() < d("2026-09-29")),
        "nothing trades on the last day, so only the settlement moved"
    );
}

/// Equity fees are charged at 20:00 ET on their trade date (spec §6.2), not at a midnight of the
/// loop's own. The entry fills 479 shares on 2026-09-24, accruing CAT 0.00001 × 479 = 0.00479; a run
/// that ends at that day's 20:30 bar charges `round(0.00479, 2, ceiling)` = 0.01 and accrues nothing,
/// while the same run ending at 19:30 has charged nothing and still accrues 0.00479. A rule that
/// charged at New York midnight instead would charge neither.
#[test]
#[ignore = "pending E4-2"]
fn equity_fees_are_charged_at_twenty_hundred_new_york_on_their_trade_date() {
    let config = run_config(equity(), crossover(), "100000");

    let after = run(
        &config,
        &days_then(
            4,
            vec![overnight(
                "2026-09-24",
                "20:30",
                ["104.50", "104.60", "104.40", "104.50", "500"],
            )],
        ),
    )
    .unwrap();
    assert_eq!(after.report.strategy.fees_charged, usd("0.01"));
    assert_eq!(after.report.strategy.fees_accrued, Usd::ZERO);

    let before = run(
        &config,
        &days_then(
            4,
            vec![after_hours(
                "2026-09-24",
                "19:30",
                ["104.50", "104.60", "104.40", "104.50", "500"],
            )],
        ),
    )
    .unwrap();
    assert_eq!(before.report.strategy.fees_charged, Usd::ZERO);
    assert_eq!(before.report.strategy.fees_accrued, usd("0.00479"));
}

/// Crypto fees are charged at 00:00 UTC (spec §6.3): the exit of the sixth UTC day accrues a USD fee
/// on that date, and the seventh day's first bar is past its midnight, so the fee is charged. A
/// crypto **buy** pays in the asset instead, which is never accrued, so a run that only bought would
/// have nothing to charge.
#[test]
#[ignore = "pending E4-2"]
fn crypto_fees_are_charged_at_midnight_utc() {
    let config = run_config(crypto(), crossover(), "100000");
    let run = run(&config, &crypto_days()).unwrap();

    assert!(
        run.report.strategy.fees_charged > Usd::ZERO,
        "the sale's USD fee is charged at the next 00:00 UTC"
    );
    assert!(
        run.report.strategy.fees_asset > Usd::ZERO,
        "the buy's fee was taken in the asset"
    );
    assert_eq!(
        run.report.strategy.fees_accrued,
        Usd::ZERO,
        "nothing accrued is left once the charge lands"
    );
}

/// Nothing is swept at the end of a run: [`seven_days`] has no bar past 20:00 ET on any trade date,
/// so every accrual stays accrued, exactly as the fold would hold it. The entry accrues CAT
/// 0.00001 × 479 = 0.00479, and the exit of 479 shares at 102.9691, which is 49,322.1989, accrues SEC
/// 0.00003 × 49322.1989 = 1.479665967, TAF 0.0002 × 479 = 0.0958, and CAT 0.00479, which with the
/// entry's 0.00479 is 1.585045967 in all.
#[test]
#[ignore = "pending E4-2"]
fn an_accrual_the_bars_never_reach_stays_accrued() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &seven_days()).unwrap();

    assert_eq!(run.report.strategy.fees_charged, Usd::ZERO);
    assert_eq!(run.report.strategy.fees_accrued, usd("1.585045967"));
    assert_eq!(
        run.report.strategy.fees_total,
        run.report
            .strategy
            .fees_charged
            .checked_add(run.report.strategy.fees_accrued)
            .unwrap()
            .checked_add(run.report.strategy.fees_asset)
            .unwrap()
    );
}

/// A day's fees are charged once: two bars past 20:00 ET on the same trade date leave one charge of
/// 0.01, not two.
#[test]
#[ignore = "pending E4-2"]
fn a_days_fees_are_charged_once_after_the_day_ends() {
    let config = run_config(equity(), crossover(), "100000");
    let twice = run(
        &config,
        &days_then(
            4,
            vec![
                overnight(
                    "2026-09-24",
                    "20:30",
                    ["104.50", "104.60", "104.40", "104.50", "500"],
                ),
                overnight(
                    "2026-09-24",
                    "21:30",
                    ["104.50", "104.60", "104.40", "104.50", "500"],
                ),
            ],
        ),
    )
    .unwrap();

    assert_eq!(twice.report.strategy.fees_charged, usd("0.01"));
    assert_eq!(twice.report.strategy.fees_accrued, Usd::ZERO);
}

/// A signal at a period's close fills no earlier than the next bar: the decision is timed at that
/// bar's start (DEC-127 item 2), so the bar whose close produced it never fills it.
#[test]
#[ignore = "pending E4-2"]
fn a_signal_at_a_days_close_fills_no_earlier_than_the_next_bar() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &seven_days()).unwrap();

    assert!(!run.fills.is_empty(), "the run does trade");
    for order in &run.orders {
        let index = usize::try_from(order.index).unwrap();
        for fill in run.fills.iter().filter(|f| f.fill.order.index() == index) {
            assert!(
                fill.fill.bar > order.decided_at_bar,
                "bar {} decided the order and must not fill it",
                order.decided_at_bar
            );
        }
    }
}

/// Every fill carries its order's client order ID, a distinct fill ID built from the order's index and
/// the fill's ordinal within it, and the start of the bar it happened in (DEC-127 item 21).
#[test]
#[ignore = "pending E4-2"]
fn every_fill_carries_its_orders_identifiers() {
    let config = run_config(equity(), crossover(), "100000");
    let bars = seven_days();
    let run = run(&config, &bars).unwrap();

    let first = run.fills.first().expect("at least one fill");
    assert_eq!(first.execution.client_order_id.as_deref(), Some("o0"));
    assert_eq!(first.execution.fill_id, "o0-f0");
    let ids: Vec<&str> = run
        .fills
        .iter()
        .map(|f| f.execution.fill_id.as_str())
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "no fill ID repeats");
    for fill in &run.fills {
        let order = fill.fill.order.index();
        assert_eq!(
            fill.execution.client_order_id.as_deref(),
            Some(format!("o{order}").as_str())
        );
        assert_eq!(
            fill.execution.executed_at, bars[fill.fill.bar].start,
            "a fill is timed at its bar's start"
        );
    }
}

/// The decision instant is the next bar's start, so a gap in the data delays a fill rather than
/// letting it happen at an instant the input does not cover: with the whole of day 4 missing, the
/// entry decided at day 3's close first becomes eligible on day 5.
#[test]
#[ignore = "pending E4-2"]
fn a_decision_is_timed_at_the_next_bars_start_so_a_gap_delays_it() {
    let bars: Vec<_> = seven_days()
        .into_iter()
        .filter(|b| b.trade_date != d("2026-09-24"))
        .collect();
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &bars).unwrap();

    let entry = run.orders.first().expect("day 3's close still goes long");
    assert_eq!(entry.decided_at_bar, 5);
    for fill in run.fills.iter().filter(|f| f.fill.order.index() == 0) {
        assert!(
            fill.execution.executed_at.date() >= d("2026-09-25"),
            "day 4 is not in the data, so nothing fills there"
        );
    }
}

/// An order eligible in the middle of a session caps on the previous bar's volume, not on the
/// 20-session median: the loop passes the whole bar slice, so the model still sees the earlier bars
/// (DEC-127 item 18). The benchmark is decided at bar 0's close and is eligible from bar 1, which has
/// bar 0 before it in the same session, so its cap is 10% of 8,000 = 800 even when no median exists at
/// all; a slice trimmed to start at bar 1 would make that bar its session's first covered bar and cap
/// it at 0, filling nothing.
#[test]
#[ignore = "pending E4-2"]
fn an_order_eligible_mid_session_caps_on_the_previous_bars_volume_not_the_median() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let bars = seven_days();
    let without_median = run_without_median(&config, &bars).unwrap();

    let first = without_median
        .fills
        .first()
        .expect("bar 1 is mid-session, so it caps on bar 0's volume");
    assert_eq!(first.fill.bar, 1);
    assert_eq!(first.fill.qty, qty("800"));

    let with_median = run(&config, &bars).unwrap();
    assert_eq!(
        with_median.fills.first().map(|f| f.fill.qty),
        Some(qty("800")),
        "a median never decides a mid-session cap"
    );
}

/// A day order's remainder ends where the fill model says it does: the loop reads `OrderEnd` back
/// rather than recomputing rule 2's last eligible session.
#[test]
#[ignore = "pending E4-2"]
fn a_day_orders_remainder_ends_where_simulate_says_it_does() {
    let config = run_config(equity(), crossover(), "100000");
    let bars = seven_days();
    let run = run(&config, &bars).unwrap();

    let entry = run.orders.first().unwrap();
    let outcome = simulate_directly(&config, &bars, &[entry.order]).unwrap();
    assert_eq!(
        Some(entry.end),
        outcome.end_of(mandate_sim::OrderRef::new(0)),
        "the loop's end state is the model's"
    );
}

/// The bar in which an order stops working submits no replacement: the order works through the end of
/// that bar (§5.3 rule 6), so the next submission comes from a later period close. The entry fills in
/// full on bar 6 and the exit is decided at bar 9.
#[test]
#[ignore = "pending E4-2"]
fn the_bar_that_ends_an_order_submits_no_replacement() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &seven_days()).unwrap();

    assert_eq!(run.orders.len(), 2, "one entry and one exit");
    assert_eq!(run.orders[0].end, mandate_sim::OrderEnd::Filled);
    let last_fill = run
        .fills
        .iter()
        .filter(|f| f.fill.order.index() == 0)
        .map(|f| f.fill.bar)
        .max()
        .expect("the entry fills");
    assert_eq!(last_fill, 6);
    assert!(
        run.orders[1].decided_at_bar > last_fill,
        "the bar that ended the entry decided nothing"
    );
}

/// Two executions of one order share its client order ID, so a per-order TAF cap binds across them
/// (spec §6.2, DEC-87). With TAF alone at 0.0002 a share and a cap of 0.05, an exit of 479 shares that
/// fills 200 then 279 is charged 0.04 and then `max(0, 0.05 − 0.04)` = 0.01, that is 0.05 in all; the
/// same fills under a per-execution cap are charged 0.04 and `min(0.0558, 0.05)` = 0.05, that is 0.09,
/// which is also what a `client_order_id` of `None` would give, because the fold then treats every
/// execution as its own order.
#[test]
#[ignore = "pending E4-2"]
fn two_executions_of_one_order_share_its_client_order_id_and_one_taf_cap() {
    let bars = thin_exit_week();
    let mut per_order = run_config(equity(), crossover(), "100000");
    per_order.fees = taf_only_fees("0.05", TafCapBasis::PerOrder);
    let run = run_with_median(&per_order, &bars, "2000").unwrap();

    let exit_fills: Vec<_> = run
        .fills
        .iter()
        .filter(|f| f.execution.side == Side::Sell)
        .collect();
    assert_eq!(
        exit_fills.len(),
        2,
        "the cap splits the exit across two bars"
    );
    assert_eq!(exit_fills[0].fill.qty, qty("200"));
    assert_eq!(exit_fills[1].fill.qty, qty("279"));
    assert_eq!(exit_fills[0].execution.fill_id, "o1-f0");
    assert_eq!(exit_fills[1].execution.fill_id, "o1-f1");
    for fill in &exit_fills {
        assert_eq!(fill.execution.client_order_id.as_deref(), Some("o1"));
    }
    assert_eq!(
        run.report.strategy.fees_total,
        usd("0.05"),
        "one cap for the order, not one for each execution"
    );

    let mut per_execution = run_config(equity(), crossover(), "100000");
    per_execution.fees = taf_only_fees("0.05", TafCapBasis::PerExecution);
    let other = run_with_median(&per_execution, &bars, "2000").unwrap();
    assert_eq!(other.report.strategy.fees_total, usd("0.09"));
}

/// [`seven_days`] with thin volume on the exit day, so a cap of 10% of the 2,000-share median fills
/// 200 of the 479 on that day's first bar and the remaining 279 on the second, whose reference is the
/// first bar's 3,000.
fn thin_exit_week() -> Vec<mandate_sim::SimBar> {
    seven_days()
        .into_iter()
        .map(|b| {
            if b.trade_date == d("2026-09-28") {
                mandate_sim::SimBar {
                    volume: qty("3000"),
                    ..b
                }
            } else {
                b
            }
        })
        .collect()
}

/// A bar whose `trade_date` is not the one the calendar gives its start fails the run, so a
/// mislabelled bar can never move a period boundary (DEC-127 item 20).
#[test]
#[ignore = "pending E4-2"]
fn a_bar_whose_trade_date_disagrees_with_the_calendar_fails_the_run() {
    let mut bars = seven_days();
    bars[2].trade_date = d("2026-09-23");
    let config = run_config(equity(), crossover(), "100000");

    assert_eq!(
        run(&config, &bars).map(|_| ()),
        Err(BacktestError::BarTradeDateMismatch(2))
    );
}

/// An entry the day after an exit sizes from `cash_total`, so the unsettled proceeds of the sale are
/// available in a margin account (§7.2). Starting from 60,000, the entry of 479 shares at 104.13123
/// leaves 10,121.14083 settled; the exit at 102.9691 adds 49,322.1989 **unsettled** until the next
/// day, so at the sixth period's close `cash_total` is 59,443.33973 and the second entry is
/// `truncate(min(50000, 59443.33973) ÷ 107.26)` = 466 shares, where settled cash alone would have
/// bought `truncate(10121.14083 ÷ 107.26)` = 94.
#[test]
#[ignore = "pending E4-2"]
fn an_entry_the_day_after_an_exit_sizes_from_unsettled_proceeds() {
    let config = run_config(equity(), crossover(), "60000");
    let run = run(&config, &round_trip_and_re_entry()).unwrap();

    let entries: Vec<_> = run
        .orders
        .iter()
        .filter(|o| o.order.side == Side::Buy)
        .collect();
    assert_eq!(entries.len(), 2, "the sixth period goes long again");
    assert_eq!(entries[1].limit, price("107.26"));
    assert_eq!(entries[1].order.qty, qty("466"));
}

/// [`seven_days`] with the sixth day rising to 107.00 instead of falling, so the fifth period is flat,
/// which exits, and the sixth is long again (210 × 3 = 630 above 314.50 × 2 = 629) while the sale is
/// still unsettled.
fn round_trip_and_re_entry() -> Vec<mandate_sim::SimBar> {
    let mut bars = seven_days();
    bars[10] = auction(
        "2026-09-28",
        "09:30",
        ["103.00", "107.20", "102.90", "107.10", "8000"],
    );
    bars[11] = bar(
        "2026-09-28",
        "15:59",
        ["107.10", "107.20", "106.90", "107.00", "9000"],
    );
    bars[12] = auction(
        "2026-09-29",
        "09:30",
        ["107.00", "107.20", "106.90", "107.00", "8000"],
    );
    bars[13] = bar(
        "2026-09-29",
        "15:59",
        ["107.00", "107.20", "106.90", "107.00", "9000"],
    );
    bars
}

/// The benchmark buys at its first eligible bar, not at the run's last price. It is decided at the
/// close of **bar 0** rather than at a period close, the one exception loop step 6 makes (DEC-127 item
/// 13), so with the collar its limit is `on_tick(100.10 × 1.0025, down)` = `on_tick(100.35025, down)`
/// = 100.35, its quantity `truncate(100000 ÷ 100.35)` = 996, and its first fill is 800 shares at
/// `min(100.35, 100.10 × 1.0003)` = 100.13003 on bar 1.
#[test]
#[ignore = "pending E4-2"]
fn the_benchmark_buys_at_its_first_eligible_bar_not_the_last() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let bars = seven_days();
    let run = run(&config, &bars).unwrap();

    assert_eq!(
        run.orders.len(),
        1,
        "the benchmark submits exactly one order"
    );
    assert_eq!(run.orders[0].decided_at_bar, 0, "decided at bar 0's close");
    assert_eq!(run.orders[0].limit, price("100.35"));
    assert_eq!(run.orders[0].order.qty, qty("996"));
    assert_eq!(run.orders[0].order.tif, mandate_sim::TimeInForce::Gtc);

    let fill = run.fills.first().expect("the benchmark buys");
    assert_eq!(fill.fill.bar, 1, "the bar after the one that decided it");
    assert_eq!(fill.fill.price, price("100.13003"));
    assert_eq!(fill.fill.qty, qty("800"));
}

/// A benchmark the volume cap never lets fill in full reports its unfilled quantity rather than
/// pretending to hold shares, and its GTC order is still working when the bars run out.
#[test]
#[ignore = "pending E4-2"]
fn the_benchmark_that_cannot_fill_reports_its_unfilled_quantity() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let bars = thin_week();
    let run = run_without_median(&config, &bars).unwrap();

    let metrics = &run.report.strategy;
    assert!(
        metrics.filled_qty < metrics.submitted_qty,
        "the volume cap left part of the order working"
    );
    assert!(metrics.submitted_qty > Qty::ZERO);
    assert_eq!(run.orders[0].end, mandate_sim::OrderEnd::Open);
}

/// The same week with tiny volumes, so a 10% cap fills only a fraction of the benchmark's order.
fn thin_week() -> Vec<mandate_sim::SimBar> {
    seven_days()
        .into_iter()
        .map(|b| mandate_sim::SimBar {
            volume: qty("10"),
            ..b
        })
        .collect()
}

/// Identical inputs give byte-identical reports, and a changed bar changes the digest that says which
/// snapshot produced them (FR-4.5).
#[test]
#[ignore = "pending E4-2"]
fn a_changed_bar_changes_the_bars_digest() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let bars = seven_days();
    let first = run(&config, &bars).unwrap();
    let again = run(&config, &bars).unwrap();
    assert_eq!(
        first.report.canonical_bytes().unwrap(),
        again.report.canonical_bytes().unwrap()
    );
    assert_eq!(
        first.report.digest().unwrap(),
        again.report.digest().unwrap()
    );

    let mut changed = bars.clone();
    changed[13].close = price("102.01");
    let other = run(&config, &changed).unwrap();
    assert_ne!(first.report.inputs.bars, other.report.inputs.bars);
    assert_eq!(
        first.report.inputs.config, other.report.inputs.config,
        "the configuration did not change"
    );
}

/// The report's canonical bytes hold decimals as text and nothing else: a reader can recompute every
/// figure from them, and the digest is the SHA-256 of exactly those bytes.
#[test]
#[ignore = "pending E4-2"]
fn the_report_serializes_to_the_committed_canonical_bytes() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let run = run(&config, &seven_days()).unwrap();
    let bytes = run.report.canonical_bytes().unwrap();
    let text = String::from_utf8(bytes.clone()).unwrap();

    assert!(text.starts_with('{') && text.ends_with('}'));
    assert!(
        !text.contains("e+") && !text.contains("E+"),
        "no exponent form in canonical text"
    );
    assert!(text.contains("\"report_version\":1"));
    assert!(text.contains("\"sharpe_sign\":"));
    assert_eq!(
        run.report.digest().unwrap(),
        mandate_canon::Digest::of(&bytes)
    );
    assert_eq!(
        run.report.canonical().unwrap(),
        mandate_canon::parse(&bytes).unwrap(),
        "the bytes parse back to the value they came from"
    );
}
