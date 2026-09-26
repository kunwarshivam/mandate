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
use mandate_accounting::Side;
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

/// A variance whose exact value is positive but rounds to zero at 12 places leaves the Sharpe absent
/// for the same reason: 100000 → 100000.000000001 → 100000.000000002 has period returns 1e-14 and
/// 1e-14 rounded to 12 places, so both are 0 and the variance is 0.
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
    assert!(
        metrics.total_return > Ratio::ZERO,
        "the run did make money, even if no period return survives 12 places"
    );
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

    let variance = metrics.variance.unwrap();
    assert_eq!(
        metrics.variance_annualized,
        Some(variance.times_int(252).unwrap())
    );
    let squared = metrics.sharpe_squared.unwrap();
    assert_eq!(
        metrics.sharpe_squared_annualized,
        Some(squared.times_int(252).unwrap())
    );
    assert_eq!(
        metrics.volatility_annualized,
        Some(variance.times_int(252).unwrap().root_ceiling().unwrap())
    );
}

/// The variance divides by n − 1, not n: on the main series the sample variance is 0.000906221436,
/// where the population variance would be two thirds of it.
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

    let sample = metrics.variance.unwrap();
    assert_eq!(sample, ratio("0.000906221436"));
    let population = Ratio::sample_variance(
        metrics.return_sum,
        metrics.return_sum_of_squares,
        metrics.period_count,
    )
    .unwrap();
    assert_eq!(sample, population, "the figure is the crate's own formula");
    assert!(
        sample > ratio("0.000604147624"),
        "a population divisor would give two thirds of the sample variance"
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

/// The report's fee fields come from the fold, and the return is net of them because equity
/// subtracts accrued fees (spec §8.2): a run whose only event is a fee accrual of 10 USD on 100,000
/// closes at 99,990 and reports −0.0001.
#[test]
#[ignore = "pending E4-2"]
fn the_total_return_is_net_of_accrued_and_charged_fees() {
    let totals = Totals {
        fees_total: usd("10"),
        fees_accrued: usd("4"),
        fees_charged: usd("6"),
        ..one_round_trip()
    };
    let metrics = Metrics::of(
        usd("100000"),
        &observations(&["99990"]),
        &totals,
        &daily_equity_metrics(),
    )
    .unwrap();

    assert_eq!(metrics.fees_total, usd("10"));
    assert_eq!(metrics.fees_accrued, usd("4"));
    assert_eq!(metrics.fees_charged, usd("6"));
    assert_eq!(metrics.total_return, ratio("-0.0001"));
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
/// its own: 0.03 − 0.0201 = 0.0099.
#[test]
#[ignore = "pending E4-2"]
fn the_excess_return_is_the_difference_of_the_two_reported_returns() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

    let expected = run
        .report
        .strategy
        .total_return
        .checked_sub(run.report.benchmark.total_return)
        .unwrap();
    assert_eq!(run.report.excess_total_return, expected);
}

/// Two regular bars a day for the trading week of 2026-09-21, closing at 100.00, 101.00, 104.00,
/// 104.50, and 103.00: at day 3's close the fast window (101 + 104) beats the slow one
/// (100 + 101 + 104) by cross-multiplication, 615 > 610, so the strategy goes long from day 4.
fn five_days() -> Vec<mandate_sim::SimBar> {
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
    ]
}

/// The entry is a day limit buy in the regular session, priced a collar above the signal period's
/// close and put on the tick against the order: 104.00 × (1 + 25 bps) = 104.26, already on the penny
/// grid, and 50,000 ÷ 104.26 truncates to 479 whole shares.
#[test]
#[ignore = "pending E4-2"]
fn an_entry_is_a_day_limit_buy_at_the_collar_above_the_close() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

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
/// the signal: with closes 100, 101, 104 the fast sum 205 times 3 beats the slow sum 305 times 2.
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

/// An exit sells the whole position, not a quantity truncated to the increment (§5.3 rule 2).
#[test]
#[ignore = "pending E4-2"]
fn an_exit_sells_the_whole_position() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

    let entry = run.orders.first().unwrap();
    let exit = run
        .orders
        .iter()
        .find(|o| o.order.side == Side::Sell)
        .expect("day 5's fall takes the signal flat");
    let bought: Qty = run
        .fills
        .iter()
        .filter(|f| f.execution.side == Side::Buy)
        .fold(Qty::ZERO, |sum, f| sum.checked_add(f.fill.qty).unwrap());
    assert_eq!(
        exit.order.qty, bought,
        "the exit sells what the entry filled"
    );
    assert!(exit.order.qty <= entry.order.qty);
    assert_eq!(exit.order.side, Side::Sell);
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
        .expect("the crypto run exits too");
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
        "crypto takes GTC"
    );
    assert!(run.report.strategy.fees_asset > Usd::ZERO);
}

/// Five UTC days of continuous bars, two a day, closing at 100, 101, 104, 104.5, 103.
fn crypto_days() -> Vec<mandate_sim::SimBar> {
    vec![
        continuous(
            "2026-09-21",
            "00:01",
            ["100.00", "100.20", "99.80", "100.10", "8000"],
        ),
        continuous(
            "2026-09-21",
            "23:59",
            ["100.10", "100.20", "99.90", "100.00", "9000"],
        ),
        continuous(
            "2026-09-22",
            "00:01",
            ["100.10", "101.20", "100.00", "101.10", "8000"],
        ),
        continuous(
            "2026-09-22",
            "23:59",
            ["101.10", "101.20", "100.90", "101.00", "9000"],
        ),
        continuous(
            "2026-09-23",
            "00:01",
            ["101.10", "104.20", "101.00", "104.10", "8000"],
        ),
        continuous(
            "2026-09-23",
            "23:59",
            ["104.10", "104.20", "103.90", "104.00", "9000"],
        ),
        continuous(
            "2026-09-24",
            "00:01",
            ["104.10", "104.60", "104.00", "104.50", "8000"],
        ),
        continuous(
            "2026-09-24",
            "23:59",
            ["104.50", "104.60", "104.30", "104.50", "9000"],
        ),
        continuous(
            "2026-09-25",
            "00:01",
            ["104.50", "104.60", "102.90", "103.10", "8000"],
        ),
        continuous(
            "2026-09-25",
            "23:59",
            ["103.10", "103.20", "102.90", "103.00", "9000"],
        ),
    ]
}

/// A period's mark is its closing bar's close (spec §8.2), so the last observation of the week is
/// 103.00 times the position plus cash, never the day's open or high.
#[test]
#[ignore = "pending E4-2"]
fn the_period_mark_is_the_last_bars_close() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let run = run(&config, &five_days()).unwrap();

    let held = run
        .fills
        .iter()
        .fold(Qty::ZERO, |sum, f| sum.checked_add(f.fill.qty).unwrap());
    let last = run.equity.last().unwrap();
    let marked_at_close = held.notional(price("103.00")).unwrap();
    let marked_at_open = held.notional(price("103.10")).unwrap();
    assert!(
        last.equity < marked_at_open,
        "the day's open would have flattered the last observation"
    );
    assert!(
        last.equity <= marked_at_close.checked_add(usd("100000")).unwrap(),
        "equity is cash plus the position at the closing bar's close"
    );
    assert_eq!(last.bar, 9, "the week's last regular bar closed the period");
}

/// An after-hours bar never closes an equity period: §8.2's end of day is the official close, so the
/// period still closes at 15:59 and the after-hours bar's mark lands in the next period.
#[test]
#[ignore = "pending E4-2"]
fn an_after_hours_bar_does_not_close_an_equity_period() {
    let mut bars = five_days();
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
    assert_eq!(run.equity.len(), 5, "still five periods");
}

/// A trade date the input covers only outside the regular session is no period at all; its bars are
/// still folded, so their marks reach the next period.
#[test]
#[ignore = "pending E4-2"]
fn a_date_covered_only_outside_the_regular_session_is_no_period() {
    let mut bars = five_days();
    bars.insert(
        2,
        pre_market(
            "2026-09-22",
            "08:00",
            ["100.00", "100.10", "99.90", "100.00", "500"],
        ),
    );
    bars.retain(|b| {
        !(b.trade_date == d("2026-09-22") && b.session == mandate_sim::Session::Regular)
    });
    let config = run_config(equity(), buy_and_hold(), "100000");
    let run = run(&config, &bars).unwrap();

    assert!(
        run.equity.iter().all(|o| o.date != d("2026-09-22")),
        "no regular bar on the 22nd, so no period for it"
    );
    assert_eq!(run.equity.len(), 4);
}

/// A fill in the period's closing bar is inside that period's equity: the observation is taken after
/// the bar's fills, its mark, and its charges, never before.
#[test]
#[ignore = "pending E4-2"]
fn a_fill_in_the_last_bar_of_a_day_is_inside_that_days_equity() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let bars = closing_bar_fill();
    let run = run(&config, &bars).unwrap();

    let fill = run
        .fills
        .first()
        .expect("the benchmark fills on the second bar");
    assert_eq!(fill.fill.bar, 1, "the day's closing bar");
    let first = run.equity.first().unwrap();
    assert_eq!(first.bar, 1);
    assert!(
        first.equity != usd("100000"),
        "the fill and its fees are inside the first observation"
    );
}

/// One trading day of two bars where the benchmark's order can only fill in the second, which is also
/// the bar that closes the period.
fn closing_bar_fill() -> Vec<mandate_sim::SimBar> {
    vec![
        auction(
            "2026-09-21",
            "09:30",
            ["100.00", "100.20", "99.80", "100.00", "8000"],
        ),
        bar(
            "2026-09-21",
            "15:59",
            ["100.05", "100.20", "99.90", "100.00", "9000"],
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
    ]
}

/// A settlement dated before a bar posts before that bar's fills: the sale of day 1 settles on day 2
/// (T+1 on the settlement calendar), and the entry of day 2 sizes from cash that already includes it.
#[test]
#[ignore = "pending E4-2"]
fn a_settlement_posts_before_the_bars_fills() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

    let settled_on = run
        .equity
        .iter()
        .find(|o| o.date == d("2026-09-25"))
        .expect("the week's last period");
    assert!(settled_on.equity > Usd::ZERO);
    assert!(
        run.fills
            .iter()
            .all(|f| f.execution.executed_at.date() <= d("2026-09-25")),
        "no fill is dated after the run"
    );
}

/// Equity fees are charged at 20:00 ET on their trade date (spec §6.2): with a bar after that
/// instant, day 4's accrual is charged and no longer accrued.
#[test]
#[ignore = "pending E4-2"]
fn equity_fees_are_charged_at_twenty_hundred_new_york_on_their_trade_date() {
    let mut bars = five_days();
    bars.insert(
        8,
        after_hours(
            "2026-09-24",
            "20:30",
            ["104.50", "104.60", "104.40", "104.50", "500"],
        ),
    );
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &bars).unwrap();

    assert!(
        run.report.strategy.fees_charged > Usd::ZERO,
        "the 20:30 bar is past 20:00 ET, so day 4's accrual is charged"
    );
}

/// Crypto fees are charged at 00:00 UTC (spec §6.3), so the first bar of the next UTC day charges the
/// day before it.
#[test]
#[ignore = "pending E4-2"]
fn crypto_fees_are_charged_at_midnight_utc() {
    let config = run_config(crypto(), crossover(), "100000");
    let run = run(&config, &crypto_days()).unwrap();

    assert!(
        run.report.strategy.fees_charged > Usd::ZERO,
        "a later UTC day's bar charges the accrual of the day before"
    );
}

/// Nothing is swept at the end of a run: an accrual whose charging instant the bars never reach stays
/// accrued, which is exactly what the fold would hold at that instant, and the report shows it.
#[test]
#[ignore = "pending E4-2"]
fn an_accrual_the_bars_never_reach_stays_accrued() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

    assert!(
        run.report.strategy.fees_accrued > Usd::ZERO,
        "the last day's fees accrue but 20:00 ET is past the last bar"
    );
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

/// A day's fees are charged once, not twice: two bars past 20:00 ET on the same trade date leave one
/// charge.
#[test]
#[ignore = "pending E4-2"]
fn a_days_fees_are_charged_once_after_the_day_ends() {
    let mut bars = five_days();
    bars.insert(
        8,
        after_hours(
            "2026-09-24",
            "21:30",
            ["104.50", "104.60", "104.40", "104.50", "500"],
        ),
    );
    bars.insert(
        8,
        after_hours(
            "2026-09-24",
            "20:30",
            ["104.50", "104.60", "104.40", "104.50", "500"],
        ),
    );
    let config = run_config(equity(), crossover(), "100000");
    let twice = run(&config, &bars).unwrap();

    let mut once_bars = five_days();
    once_bars.insert(
        8,
        after_hours(
            "2026-09-24",
            "20:30",
            ["104.50", "104.60", "104.40", "104.50", "500"],
        ),
    );
    let once = run(&config, &once_bars).unwrap();

    assert_eq!(
        twice.report.strategy.fees_charged, once.report.strategy.fees_charged,
        "a second bar past the instant charges nothing more"
    );
}

/// A signal at a period's close fills no earlier than the next bar: the decision is timed at that
/// bar's start (DEC-127 item 2), so the bar whose close produced it never fills it.
#[test]
#[ignore = "pending E4-2"]
fn a_signal_at_a_days_close_fills_no_earlier_than_the_next_bar() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

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

/// The decision instant is the next bar's start, so a gap in the data delays a fill rather than
/// letting it happen at an instant the input does not cover: with the whole of day 4 missing, the
/// entry decided at day 3's close first becomes eligible on day 5.
#[test]
#[ignore = "pending E4-2"]
fn a_decision_is_timed_at_the_next_bars_start_so_a_gap_delays_it() {
    let bars: Vec<_> = five_days()
        .into_iter()
        .filter(|b| b.trade_date != d("2026-09-24"))
        .collect();
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &bars).unwrap();

    let entry = run.orders.first().expect("day 3's close still goes long");
    assert_eq!(entry.decided_at_bar, 5);
    for fill in &run.fills {
        assert!(fill.execution.executed_at.date() >= d("2026-09-25"));
    }
}

/// An order eligible in the middle of a session caps on the previous bar's volume, not on the
/// 20-session median: the loop passes the whole bar slice, so the model sees the earlier bars
/// (DEC-127 item 18). With no median at all, a mid-session order still fills.
#[test]
#[ignore = "pending E4-2"]
fn an_order_eligible_mid_session_caps_on_the_previous_bars_volume_not_the_median() {
    let config = run_config(equity(), crossover(), "100000");
    let with_median = run(&config, &five_days()).unwrap();
    let without_median = run_without_median(&config, &five_days()).unwrap();

    assert!(
        !without_median.fills.is_empty(),
        "a mid-session bar has a previous bar of its session, so its cap is not the median's"
    );
    assert_eq!(
        with_median.fills.first().map(|f| f.fill.qty),
        without_median.fills.first().map(|f| f.fill.qty),
        "the median never decides a mid-session cap"
    );
}

/// A day order's remainder ends where the fill model says it does: the loop reads `OrderEnd` back
/// rather than recomputing rule 2's last eligible session.
#[test]
#[ignore = "pending E4-2"]
fn a_day_orders_remainder_ends_where_simulate_says_it_does() {
    let config = run_config(equity(), crossover(), "100000");
    let bars = five_days();
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
/// that bar (§5.3 rule 6), so the next submission comes from a later period close.
#[test]
#[ignore = "pending E4-2"]
fn the_bar_that_ends_an_order_submits_no_replacement() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

    for pair in run.orders.windows(2) {
        let (first, second) = (pair[0], pair[1]);
        let last_fill = run
            .fills
            .iter()
            .filter(|f| f.fill.order.index() == usize::try_from(first.index).unwrap())
            .map(|f| f.fill.bar)
            .max();
        if let Some(bar) = last_fill {
            assert!(
                second.decided_at_bar > bar,
                "the bar that ended order {} decided no replacement",
                first.index
            );
        }
    }
}

/// Two executions of one order share its client order ID, so the per-order TAF cap binds across them
/// (spec §6.2, DEC-87), and every fill ID is distinct.
#[test]
#[ignore = "pending E4-2"]
fn two_executions_of_one_order_share_its_client_order_id_and_one_taf_cap() {
    let config = run_config(equity(), crossover(), "100000");
    let run = run(&config, &five_days()).unwrap();

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
    }
}

/// A bar whose `trade_date` is not the one the calendar gives its start fails the run, so a
/// mislabelled bar can never move a period boundary (DEC-127 item 20).
#[test]
#[ignore = "pending E4-2"]
fn a_bar_whose_trade_date_disagrees_with_the_calendar_fails_the_run() {
    let mut bars = five_days();
    bars[2].trade_date = d("2026-09-23");
    let config = run_config(equity(), crossover(), "100000");

    assert_eq!(
        run(&config, &bars).map(|_| ()),
        Err(BacktestError::BarTradeDateMismatch(2))
    );
}

/// An entry the day after an exit sizes from `cash_total`, so the unsettled proceeds of the sale are
/// available in a margin account (§7.2) and the period is not silently sat out.
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
    assert!(
        entries.len() >= 2,
        "the second signal still submits, although the first sale has not settled"
    );
    let second = entries[1];
    assert!(
        second.order.qty > Qty::ZERO,
        "sizing from settled cash alone would have made this order zero"
    );
}

/// A week that goes long, flat, then long again, with a starting cash small enough that a settled-cash
/// denominator would leave nothing to buy with on the re-entry day.
fn round_trip_and_re_entry() -> Vec<mandate_sim::SimBar> {
    let mut bars = five_days();
    bars.extend(vec![
        auction(
            "2026-09-28",
            "09:30",
            ["103.00", "106.00", "103.00", "105.90", "8000"],
        ),
        bar(
            "2026-09-28",
            "15:59",
            ["105.90", "106.00", "105.50", "106.00", "9000"],
        ),
        auction(
            "2026-09-29",
            "09:30",
            ["106.00", "108.00", "106.00", "107.90", "8000"],
        ),
        bar(
            "2026-09-29",
            "15:59",
            ["107.90", "108.00", "107.50", "108.00", "9000"],
        ),
    ]);
    bars
}

/// The benchmark buys at its first eligible bar, not at the run's last price: its fill is on bar 1
/// and its price is inside that bar's range, never the final close.
#[test]
#[ignore = "pending E4-2"]
fn the_benchmark_buys_at_its_first_eligible_bar_not_the_last() {
    let config = run_config(equity(), buy_and_hold(), "100000");
    let bars = five_days();
    let run = run(&config, &bars).unwrap();

    let fill = run.fills.first().expect("the benchmark buys");
    assert_eq!(fill.fill.bar, 1, "the bar after the one that decided it");
    assert!(
        fill.fill.price <= price("100.20"),
        "bar 1's high bounds the price"
    );
    assert!(fill.fill.price >= price("99.90"), "bar 1's low bounds it");
    assert_eq!(
        run.orders.len(),
        1,
        "the benchmark submits exactly one order"
    );
}

/// A benchmark the market never lets fill in full reports its unfilled quantity rather than
/// pretending to hold shares.
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
}

/// The same week with tiny volumes, so a 10% cap fills only a fraction of the benchmark's order.
fn thin_week() -> Vec<mandate_sim::SimBar> {
    five_days()
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
    let bars = five_days();
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
    changed[9].close = price("103.01");
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
    let run = run(&config, &five_days()).unwrap();
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
