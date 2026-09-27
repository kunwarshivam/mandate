//! Property tests for the backtest loop and its metrics, each against an oracle that computes the
//! answer its own way ([task brief](../../../docs/project/tasks/E4-2-backtest-baseline.md), DEC-127).
//!
//! Two oracles, neither sharing code with the crate:
//!
//! * [`oracle`] holds the statistics as `i128` integers — equity and money at 10⁻¹², ratios at 10⁻¹²
//!   and their squares at 10⁻²⁴ — with its own half-even and ceiling division and its own integer
//!   square roots. Every figure the report shows is recomputed from the equity series and the
//!   totals, so a formula the crate gets wrong disagrees with an independent number rather than with
//!   itself.
//! * [`sequencer`] rebuilds the expected input sequence of each bar from the brief's order — the
//!   movements `Account::due` makes due, the bar's fills, the bar-close mark, the fee charges spec
//!   §6.2 and §6.3 date — and folds it through `mandate_accounting::Account` to get the equity
//!   series a run must report. The fold itself is E3-1's and is not what these properties test; the
//!   selection and the order of its inputs are.
//!
//! Every property first checks the number of observations against the oracle's, so none of them can
//! pass on an empty run.

mod common;

use common::*;
use mandate_accounting::{Account, AccountType, FeeFamily, Input, Side};
use mandate_backtest::{BacktestRun, Metrics, Observation, Sign};
use mandate_num::{Price, Qty, Ratio, Usd};
use mandate_sim::SimBar;
use mandate_time::{Date, new_york_date_and_hour};
use proptest::prelude::*;

/// Fixed-point helpers and the statistics, in `i128` units.
mod oracle {
    /// Money, equity, and ratios are held at 10⁻¹²; squares of ratios at 10⁻²⁴.
    pub const SCALE: i128 = 1_000_000_000_000;

    /// Canonical decimal text (what `Display` produces) as integer units at `scale`.
    pub fn units(text: &str, scale: u32) -> i128 {
        let (sign, digits) = match text.strip_prefix('-') {
            Some(rest) => (-1, rest),
            None => (1, text),
        };
        let (whole, fraction) = match digits.split_once('.') {
            Some((w, f)) => (w, f.to_string()),
            None => (digits, String::new()),
        };
        let mut fraction = fraction;
        assert!(
            fraction.len() <= scale as usize,
            "{text} holds more than {scale} fractional digits"
        );
        while fraction.len() < scale as usize {
            fraction.push('0');
        }
        let whole: i128 = whole.parse().expect("whole part");
        let fraction: i128 = if fraction.is_empty() {
            0
        } else {
            fraction.parse().expect("fractional part")
        };
        let mut power = 1i128;
        for _ in 0..scale {
            power *= 10;
        }
        sign * (whole * power + fraction)
    }

    /// `round(numerator ÷ denominator)` to the nearest integer, ties to the even neighbour.
    pub fn half_even(numerator: i128, denominator: i128) -> i128 {
        assert!(
            denominator > 0,
            "the oracle divides by positive numbers only"
        );
        let negative = numerator < 0;
        let magnitude = numerator.abs();
        let quotient = magnitude / denominator;
        let twice = (magnitude % denominator) * 2;
        let rounded = if twice > denominator || (twice == denominator && quotient % 2 == 1) {
            quotient + 1
        } else {
            quotient
        };
        if negative { -rounded } else { rounded }
    }

    /// `round(numerator ÷ denominator)` towards positive infinity.
    pub fn ceiling(numerator: i128, denominator: i128) -> i128 {
        assert!(
            denominator > 0,
            "the oracle divides by positive numbers only"
        );
        let quotient = numerator / denominator;
        if numerator % denominator > 0 {
            quotient + 1
        } else {
            quotient
        }
    }

    /// The greatest integer whose square is at or below `value`.
    pub fn isqrt(value: i128) -> i128 {
        assert!(value >= 0, "no root of a negative value");
        if value < 2 {
            return value;
        }
        let mut guess = 1i128 << ((128 - value.leading_zeros()) / 2 + 1);
        loop {
            let next = (guess + value / guess) / 2;
            if next >= guess {
                break;
            }
            guess = next;
        }
        while guess * guess > value {
            guess -= 1;
        }
        while (guess + 1) * (guess + 1) <= value {
            guess += 1;
        }
        guess
    }

    /// The least 12-place value whose square reaches `value`, in 10⁻¹² units: the report's volatility
    /// and the magnitude of a negative Sharpe.
    pub fn root_ceiling(value: i128) -> i128 {
        let scaled = value * SCALE;
        let floor = isqrt(scaled);
        if floor * floor == scaled {
            floor
        } else {
            floor + 1
        }
    }

    /// The greatest 12-place value whose square stays at or below `value`, in 10⁻¹² units: a
    /// non-negative Sharpe.
    pub fn root_floor(value: i128) -> i128 {
        isqrt(value * SCALE)
    }

    /// The period returns of an equity series, each `round((Eₖ − Eₖ₋₁) ÷ Eₖ₋₁, 12, half_even)` in
    /// 10⁻¹² units.
    pub fn returns(starting: i128, closes: &[i128]) -> Vec<i128> {
        let mut previous = starting;
        let mut out = Vec::new();
        for close in closes {
            assert!(
                previous > 0,
                "a period opening at or below zero has no return"
            );
            out.push(half_even((close - previous) * SCALE, previous));
            previous = *close;
        }
        out
    }

    /// `Σ r` in 10⁻¹² units and `Σ r²` in 10⁻²⁴ units.
    pub fn sums(returns: &[i128]) -> (i128, i128) {
        let sum: i128 = returns.iter().sum();
        let squares: i128 = returns.iter().map(|r| r * r).sum();
        (sum, squares)
    }

    /// The sample variance in 10⁻¹² units, or `None` below two periods.
    pub fn variance(returns: &[i128]) -> Option<i128> {
        let count = i128::try_from(returns.len()).expect("a small series");
        if count < 2 {
            return None;
        }
        let (sum, squares) = sums(returns);
        let numerator = count * squares - sum * sum;
        assert!(numerator >= 0, "a variance is never negative");
        Some(half_even(numerator, count * (count - 1) * SCALE))
    }

    /// The squared Sharpe in 10⁻¹² units, or `None` without a positive variance.
    pub fn sharpe_squared(mean: i128, risk_free: i128, variance: i128) -> Option<i128> {
        if variance == 0 {
            return None;
        }
        let excess = mean - risk_free;
        Some(half_even(excess * excess, variance))
    }

    /// The largest drawdown in 10⁻¹² units, with the periods of its peak and its trough, over the
    /// closes and the starting equity as the first peak.
    pub fn drawdown(starting: i128, closes: &[i128]) -> (i128, i128, u32, u32) {
        let mut peak = starting;
        let mut peak_period = 0u32;
        let mut worst = (0i128, 0i128, 0u32, 0u32);
        for (index, close) in closes.iter().enumerate() {
            let period = u32::try_from(index + 1).expect("a small series");
            if *close > peak {
                peak = *close;
                peak_period = period;
            }
            assert!(peak > 0, "a peak is positive");
            let rung = ceiling((peak - close) * SCALE, peak);
            if rung > worst.0 {
                worst = (rung, peak - close, peak_period, period);
            }
        }
        worst
    }

    /// `round(min(buys, sells) ÷ starting, 12, half_even)` in 10⁻¹² units.
    pub fn turnover(buys: i128, sells: i128, starting: i128) -> i128 {
        assert!(starting > 0, "a run starts with positive equity");
        half_even(buys.min(sells) * SCALE, starting)
    }
}

/// The expected input sequence and equity series of a run, rebuilt from the brief's per-bar order.
mod sequencer {
    use super::*;

    /// The period-closing bars of `bars`: the last regular-session bar of each equity trade date, or
    /// the last bar of each UTC date for a continuous instrument (DEC-127 item 3).
    pub fn closing_bars(bars: &[SimBar]) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for (index, bar) in bars.iter().enumerate() {
            let counts = matches!(
                bar.session,
                mandate_sim::Session::Regular | mandate_sim::Session::Continuous
            );
            if !counts {
                continue;
            }
            match out.last() {
                Some(last) if bars[*last].trade_date == bar.trade_date => {
                    let len = out.len();
                    out[len - 1] = index;
                }
                _ => out.push(index),
            }
        }
        out
    }

    /// The equity series a run must report, from the fills it produced: the fold applied in the
    /// brief's order, with the observation taken at each period's closing bar.
    pub fn equity_series(
        run: &BacktestRun,
        bars: &[SimBar],
        config: &mandate_backtest::RunConfig,
    ) -> Vec<Usd> {
        let mut account = Account::opening(AccountType::Margin, config.starting_cash, []);
        let closings = closing_bars(bars);
        let mut charged: Vec<Date> = Vec::new();
        let mut out = Vec::new();
        for (index, bar) in bars.iter().enumerate() {
            for input in account.due(bar.start).expect("due movements") {
                account = account
                    .apply(&input, &config.fees)
                    .expect("a due movement folds")
                    .account;
            }
            for fill in run.fills.iter().filter(|f| f.fill.bar == index) {
                account = account
                    .apply(&Input::Fill(fill.execution.clone()), &config.fees)
                    .expect("a fill folds")
                    .account;
            }
            account = account
                .apply(
                    &Input::Mark {
                        instrument: config.instrument_id.clone(),
                        price: bar.close,
                    },
                    &config.fees,
                )
                .expect("a mark folds")
                .account;
            for day in charges_due(bars, index, &mut charged) {
                account = account
                    .apply(
                        &Input::FeesCharged {
                            family: FeeFamily::Equities,
                            day,
                        },
                        &config.fees,
                    )
                    .expect("a charge folds")
                    .account;
            }
            if closings.contains(&index) {
                out.push(account.equity().expect("equity after a closing bar"));
            }
        }
        out
    }

    /// The equity trade dates whose 20:00 ET instant this bar is the first to reach (spec §6.2), which
    /// is the rule the loop must follow: a date is charged once, at the first bar whose New York date
    /// and hour are at or past (date, 20).
    pub fn charges_due(bars: &[SimBar], index: usize, charged: &mut Vec<Date>) -> Vec<Date> {
        let (today, hour) = new_york_date_and_hour(bars[index].start).expect("a New York instant");
        let mut due = Vec::new();
        for bar in bars.iter().take(index + 1) {
            let date = bar.trade_date;
            if charged.contains(&date) || due.contains(&date) {
                continue;
            }
            let reached = date < today || (date == today && hour >= 20);
            if reached {
                due.push(date);
            }
        }
        due.sort_unstable();
        charged.extend(due.iter().copied());
        due
    }

    /// The signal the crossover must give at each period close, by cross-multiplying the window sums
    /// as integers at 10⁻⁹ (a price holds nine places).
    pub fn signals(closes: &[Price], fast: usize, slow: usize) -> Vec<mandate_backtest::Signal> {
        let units: Vec<i128> = closes
            .iter()
            .map(|p| oracle::units(&p.to_string(), 9))
            .collect();
        let mut out = Vec::new();
        for end in 1..=units.len() {
            if end < slow {
                out.push(mandate_backtest::Signal::Undecided);
                continue;
            }
            let fast_sum: i128 = units[end - fast..end].iter().sum();
            let slow_sum: i128 = units[end - slow..end].iter().sum();
            let long =
                fast_sum * i128::try_from(slow).unwrap() > slow_sum * i128::try_from(fast).unwrap();
            out.push(if long {
                mandate_backtest::Signal::Long
            } else {
                mandate_backtest::Signal::Flat
            });
        }
        out
    }
}

/// A close between 50 and 200 dollars, on the penny.
fn close() -> impl Strategy<Value = u32> {
    5_000u32..20_000
}

/// Canonical decimal text for a whole number of cents, which is the only form `Price::parse` takes:
/// the grammar has no trailing fractional zero, so 5,000 cents is `50` and 5,010 cents is `50.1`.
fn dollars(cents: u32) -> String {
    let text = format!("{}.{:02}", cents / 100, cents % 100);
    text.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// Two regular bars for each of `days` trading days from 2026-09-21, built around generated closes.
fn generated_bars() -> impl Strategy<Value = Vec<SimBar>> {
    proptest::collection::vec((close(), 1_000u32..20_000), 2..7).prop_map(|days| {
        let dates = [
            "2026-09-21",
            "2026-09-22",
            "2026-09-23",
            "2026-09-24",
            "2026-09-25",
            "2026-09-28",
            "2026-09-29",
        ];
        let mut bars = Vec::new();
        for (index, (cents, volume)) in days.into_iter().enumerate() {
            let day = dates[index];
            let close = dollars(cents);
            let high = dollars(cents + 50);
            let low = dollars(cents - 50);
            let volume = volume.to_string();
            bars.push(auction(
                day,
                "09:30",
                [&close, &high, &low, &close, &volume],
            ));
            bars.push(bar(day, "15:59", [&close, &high, &low, &close, &volume]));
        }
        bars
    })
}

/// A run of the crossover over generated bars, or the error it failed with.
fn generated_run(bars: &[SimBar]) -> BacktestRun {
    let config = run_config(equity(), crossover(), "100000");
    run(&config, bars).expect("a generated run folds")
}

/// Every property runs through this, so a run with no periods can never satisfy one.
fn checked(run: &BacktestRun, bars: &[SimBar]) -> Vec<Observation> {
    let expected = sequencer::closing_bars(bars);
    assert_eq!(
        run.equity.len(),
        expected.len(),
        "a run observes one period per closing bar"
    );
    assert!(!run.equity.is_empty(), "a run observes at least one period");
    run.equity.clone()
}

proptest! {
    /// Every period return is one half-even rounding of the exact quotient, as the integer oracle
    /// computes it.
    #[test]
    #[ignore = "pending E4-2"]
    fn returns_match_the_integer_oracle(bars in generated_bars()) {
        let run = generated_run(&bars);
        let observations = checked(&run, &bars);
        let starting = oracle::units(&run.report.strategy.starting_equity.to_string(), 12);
        let closes: Vec<i128> = observations
            .iter()
            .map(|o| oracle::units(&o.equity.to_string(), 12))
            .collect();
        let expected = oracle::returns(starting, &closes);
        let computed = Metrics::period_returns(run.report.strategy.starting_equity, &observations).unwrap();
        let computed: Vec<i128> = computed.iter().map(|r| oracle::units(&r.to_string(), 12)).collect();
        prop_assert_eq!(computed, expected);
    }

    /// The variance's root never understates the variance: v² ≥ variance > (v − 10⁻¹²)².
    #[test]
    #[ignore = "pending E4-2"]
    fn volatility_brackets_the_variance(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let metrics = &run.report.strategy;
        if let (Some(variance), Some(volatility)) = (metrics.variance, metrics.volatility) {
            let variance = oracle::units(&variance.to_string(), 12);
            let volatility = oracle::units(&volatility.to_string(), 12);
            prop_assert_eq!(volatility, oracle::root_ceiling(variance));
            prop_assert!(volatility * volatility >= variance * oracle::SCALE);
            if volatility > 0 {
                prop_assert!((volatility - 1) * (volatility - 1) < variance * oracle::SCALE);
            }
        }
    }

    /// The Sharpe's sign is the sign of the excess mean, and its magnitude never exceeds the root of
    /// the squared figure.
    #[test]
    #[ignore = "pending E4-2"]
    fn the_sharpe_sign_matches_the_excess_mean(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let metrics = &run.report.strategy;
        let mean = oracle::units(&metrics.mean_return.to_string(), 12);
        let risk_free = oracle::units(&metrics.risk_free_per_period.to_string(), 12);
        let excess = mean - risk_free;
        let expected = match excess.signum() {
            1 => Sign::Positive,
            -1 => Sign::Negative,
            _ => Sign::Zero,
        };
        prop_assert_eq!(metrics.sharpe_sign, expected);
        if let (Some(squared), Some(sharpe)) = (metrics.sharpe_squared, metrics.sharpe) {
            let squared = oracle::units(&squared.to_string(), 12);
            let sharpe = oracle::units(&sharpe.to_string(), 12);
            let magnitude = sharpe.abs();
            let rounded = if excess < 0 {
                oracle::root_ceiling(squared)
            } else {
                oracle::root_floor(squared)
            };
            prop_assert_eq!(magnitude, rounded);
            prop_assert_eq!(sharpe < 0, excess < 0);
        }
    }

    /// The maximum drawdown, its amount, and both periods match a running-peak oracle that includes
    /// the starting equity as the first peak.
    #[test]
    #[ignore = "pending E4-2"]
    fn max_drawdown_matches_the_running_peak_oracle(bars in generated_bars()) {
        let run = generated_run(&bars);
        let observations = checked(&run, &bars);
        let metrics = &run.report.strategy;
        let starting = oracle::units(&metrics.starting_equity.to_string(), 12);
        let closes: Vec<i128> = observations
            .iter()
            .map(|o| oracle::units(&o.equity.to_string(), 12))
            .collect();
        let (rung, amount, peak, trough) = oracle::drawdown(starting, &closes);
        prop_assert_eq!(oracle::units(&metrics.max_drawdown.to_string(), 12), rung);
        prop_assert_eq!(oracle::units(&metrics.max_drawdown_usd.to_string(), 12), amount);
        prop_assert_eq!(metrics.max_drawdown_peak_period, peak);
        prop_assert_eq!(metrics.max_drawdown_trough_period, trough);
        prop_assert!(metrics.max_drawdown >= Ratio::ZERO);
    }

    /// Turnover is the smaller side over the starting equity, never the sum and never the half-sum.
    #[test]
    #[ignore = "pending E4-2"]
    fn turnover_matches_the_min_of_the_two_sides(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let metrics = &run.report.strategy;
        let buys = oracle::units(&metrics.buy_notional.to_string(), 12);
        let sells = oracle::units(&metrics.sell_notional.to_string(), 12);
        let starting = oracle::units(&metrics.starting_equity.to_string(), 12);
        prop_assert_eq!(
            oracle::units(&metrics.turnover.to_string(), 12),
            oracle::turnover(buys, sells, starting)
        );
        prop_assert_eq!(
            metrics.traded_notional,
            metrics.buy_notional.checked_add(metrics.sell_notional).unwrap()
        );
    }

    /// Every figure recomputes from the figures the report shows: the sums, the count, the mean, the
    /// variance, and the period count are enough to rebuild the whole block (DEC-127 item 15).
    #[test]
    #[ignore = "pending E4-2"]
    fn every_reported_statistic_recomputes_from_the_reported_inputs(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let metrics = &run.report.strategy;
        let returns = oracle::returns(
            oracle::units(&metrics.starting_equity.to_string(), 12),
            &run.equity
                .iter()
                .map(|o| oracle::units(&o.equity.to_string(), 12))
                .collect::<Vec<_>>(),
        );
        let (sum, squares) = oracle::sums(&returns);
        prop_assert_eq!(oracle::units(&metrics.return_sum.to_string(), 12), sum);
        prop_assert_eq!(oracle::units(&metrics.return_sum_of_squares.to_string(), 24), squares);
        let count = i128::try_from(returns.len()).unwrap();
        prop_assert_eq!(
            oracle::units(&metrics.mean_return.to_string(), 12),
            oracle::half_even(sum, count)
        );
        match (oracle::variance(&returns), metrics.variance) {
            (Some(expected), Some(reported)) => {
                prop_assert_eq!(oracle::units(&reported.to_string(), 12), expected);
            }
            (None, None) => {}
            (expected, reported) => prop_assert!(false, "{expected:?} against {reported:?}"),
        }
        if let (Some(variance), Some(reported)) = (metrics.variance, metrics.sharpe_squared) {
            let variance = oracle::units(&variance.to_string(), 12);
            let mean = oracle::half_even(sum, count);
            let risk_free = oracle::units(&metrics.risk_free_per_period.to_string(), 12);
            prop_assert_eq!(
                Some(oracle::units(&reported.to_string(), 12)),
                oracle::sharpe_squared(mean, risk_free, variance)
            );
        }
    }

    /// The annualized figures are the period figures times the period count, exactly.
    #[test]
    #[ignore = "pending E4-2"]
    fn annualized_figures_scale_the_squares(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let metrics = &run.report.strategy;
        if let (Some(variance), Some(annual)) = (metrics.variance, metrics.variance_annualized) {
            prop_assert_eq!(annual, variance.times_int(metrics.periods_per_year).unwrap());
        }
        if let (Some(squared), Some(annual)) = (metrics.sharpe_squared, metrics.sharpe_squared_annualized) {
            prop_assert_eq!(annual, squared.times_int(metrics.periods_per_year).unwrap());
        }
    }

    /// Each equity observation matches the ledger the sequencer folds independently, and each is
    /// taken at its period's closing bar.
    #[test]
    #[ignore = "pending E4-2"]
    fn every_equity_observation_matches_the_independent_ledger(bars in generated_bars()) {
        let config = run_config(equity(), crossover(), "100000");
        let run = run(&config, &bars).expect("a generated run folds");
        let observations = checked(&run, &bars);
        let expected = sequencer::equity_series(&run, &bars, &config);
        prop_assert_eq!(observations.iter().map(|o| o.equity).collect::<Vec<_>>(), expected);
    }

    /// A period is observed at its closing bar, and that bar's close is the mark behind it.
    #[test]
    #[ignore = "pending E4-2"]
    fn every_period_marks_at_its_last_bars_close(bars in generated_bars()) {
        let run = generated_run(&bars);
        let observations = checked(&run, &bars);
        let closings = sequencer::closing_bars(&bars);
        prop_assert_eq!(
            observations.iter().map(|o| o.bar).collect::<Vec<_>>(),
            closings.clone()
        );
        for (observation, bar) in observations.iter().zip(closings) {
            prop_assert_eq!(observation.date, bars[bar].trade_date);
        }
    }

    /// No order fills in the bar whose close decided it (§6.4 rule 1, DEC-127 item 2).
    #[test]
    #[ignore = "pending E4-2"]
    fn no_order_fills_in_the_bar_that_decided_it(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        for order in &run.orders {
            for fill in run.fills.iter().filter(|f| f.fill.order.index() == usize::try_from(order.index).unwrap()) {
                prop_assert!(fill.fill.bar > order.decided_at_bar);
            }
        }
    }

    /// At most one order works at a time (§5.3 rule 6): each order's decision bar is past the bar
    /// where the previous order stopped working.
    #[test]
    #[ignore = "pending E4-2"]
    fn at_most_one_order_ever_works(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        for pair in run.orders.windows(2) {
            let (first, second) = (pair[0], pair[1]);
            prop_assert!(second.decided_at_bar > first.decided_at_bar);
            let index = usize::try_from(first.index).unwrap();
            let last_fill = run
                .fills
                .iter()
                .filter(|f| f.fill.order.index() == index)
                .map(|f| f.fill.bar)
                .max();
            let stopped = match first.end {
                mandate_sim::OrderEnd::Filled => last_fill,
                mandate_sim::OrderEnd::Expired { at_bar } => Some(at_bar.max(last_fill.unwrap_or(at_bar))),
                mandate_sim::OrderEnd::Open => None,
            };
            prop_assert!(
                first.end != mandate_sim::OrderEnd::Open,
                "an order still working leaves no room for a later one"
            );
            if let Some(bar) = stopped {
                prop_assert!(
                    second.decided_at_bar > bar,
                    "the bar that ended an order decides no replacement"
                );
            }
        }
    }

    /// The run's fills are exactly what the fill model returns for the same order over the same whole
    /// bar slice: the loop re-derives nothing (DEC-127 item 18).
    #[test]
    #[ignore = "pending E4-2"]
    fn the_runs_fills_are_exactly_what_simulate_returned(bars in generated_bars()) {
        let config = run_config(equity(), crossover(), "100000");
        let run = run(&config, &bars).expect("a generated run folds");
        checked(&run, &bars);
        for order in &run.orders {
            let outcome = simulate_directly(&config, &bars, &[order.order]).expect("the model runs");
            let expected: Vec<(usize, Qty, Price)> = outcome
                .fills
                .iter()
                .map(|f| (f.bar, f.qty, f.price))
                .collect();
            let got: Vec<(usize, Qty, Price)> = run
                .fills
                .iter()
                .filter(|f| f.fill.order.index() == usize::try_from(order.index).unwrap())
                .map(|f| (f.fill.bar, f.fill.qty, f.fill.price))
                .collect();
            prop_assert_eq!(got, expected);
            prop_assert_eq!(Some(order.end), outcome.end_of(mandate_sim::OrderRef::new(0)));
        }
    }

    /// No fill ID is ever reused, so the fold never rejects a fill as a duplicate (DEC-127 item 21).
    #[test]
    #[ignore = "pending E4-2"]
    fn no_fill_id_is_ever_reused(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let mut ids: Vec<&str> = run.fills.iter().map(|f| f.execution.fill_id.as_str()).collect();
        let count = ids.len();
        ids.sort_unstable();
        ids.dedup();
        prop_assert_eq!(ids.len(), count);
    }

    /// The signal matches a rational comparison oracle that cross-multiplies the window sums as
    /// integers, and it never reads a close later than the period it is asked about.
    #[test]
    #[ignore = "pending E4-2"]
    fn the_signal_matches_the_rational_comparison_oracle(bars in generated_bars()) {
        let closes: Vec<Price> = sequencer::closing_bars(&bars)
            .into_iter()
            .map(|index| bars[index].close)
            .collect();
        let expected = sequencer::signals(&closes, 2, 3);
        let strategy = crossover();
        for end in 1..=closes.len() {
            let signal = strategy.signal(&closes[..end]).expect("a signal");
            prop_assert_eq!(signal, expected[end - 1]);
        }
    }

    /// The strategy never reads a later close: raising the **closes** of the last trade date by 500 basis
    /// points leaves every order decided before that date, and every fill before it, exactly as they
    /// were. A loop that handed the signal a later close would decide differently on the earlier
    /// periods, which is the bug this discriminates.
    #[test]
    #[ignore = "pending E4-2"]
    fn the_signal_never_reads_a_later_close(bars in generated_bars()) {
        let config = run_config(equity(), crossover(), "100000");
        let first = run(&config, &bars).expect("a generated run folds");
        checked(&first, &bars);

        let last_date = bars.last().expect("bars").trade_date;
        let cut = bars.iter().position(|b| b.trade_date == last_date).expect("its first bar");
        let mut changed = bars.clone();
        for bar in changed.iter_mut().skip(cut) {
            let raised = bar
                .close
                .slipped(mandate_num::Bps::parse("500").unwrap(), mandate_num::Adverse::Up)
                .unwrap();
            bar.close = raised;
            bar.high = bar.high.max(raised);
        }
        let other = run(&config, &changed).expect("a generated run folds");

        let before: Vec<_> = first.orders.iter().filter(|o| o.decided_at_bar < cut).collect();
        let after: Vec<_> = other.orders.iter().filter(|o| o.decided_at_bar < cut).collect();
        prop_assert_eq!(
            before.iter().map(|o| (o.index, o.decided_at_bar, o.limit, o.order.qty)).collect::<Vec<_>>(),
            after.iter().map(|o| (o.index, o.decided_at_bar, o.limit, o.order.qty)).collect::<Vec<_>>()
        );
        prop_assert_eq!(
            first.fills.iter().filter(|f| f.fill.bar < cut).map(|f| f.fill.clone()).collect::<Vec<_>>(),
            other.fills.iter().filter(|f| f.fill.bar < cut).map(|f| f.fill.clone()).collect::<Vec<_>>()
        );
    }

    /// The strategy is long only and never crosses zero: no sell exceeds what is held, and no
    /// position ever goes short (DEC-32, §5.3 rule 3).
    #[test]
    #[ignore = "pending E4-2"]
    fn the_strategy_never_crosses_zero_and_never_goes_short(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let mut held = Qty::ZERO;
        for fill in &run.fills {
            match fill.execution.side {
                Side::Buy => held = held.checked_add(fill.fill.qty).unwrap(),
                Side::Sell => {
                    prop_assert!(fill.fill.qty <= held, "a sell never exceeds the position");
                    held = held.checked_sub(fill.fill.qty).unwrap();
                }
            }
        }
    }

    /// The benchmark submits one order and never trades again.
    #[test]
    #[ignore = "pending E4-2"]
    fn the_benchmark_never_trades_after_its_first_order(bars in generated_bars()) {
        let config = run_config(equity(), buy_and_hold(), "100000");
        let run = run(&config, &bars).expect("a generated run folds");
        checked(&run, &bars);
        prop_assert_eq!(run.orders.len(), 1);
        prop_assert!(run.fills.iter().all(|f| f.fill.order.index() == 0));
        prop_assert!(run.fills.iter().all(|f| f.execution.side == Side::Buy));
    }

    /// A higher fee never raises the return: fees are inside equity (§8.2), so raising a rate can only
    /// leave the return the same or lower it.
    #[test]
    #[ignore = "pending E4-2"]
    fn a_higher_fee_never_raises_the_return(bars in generated_bars()) {
        let mut cheap = run_config(equity(), crossover(), "100000");
        cheap.fees = no_equity_fees();
        let dear = run_config(equity(), crossover(), "100000");
        let cheap_run = run(&cheap, &bars).expect("a generated run folds");
        let dear_run = run(&dear, &bars).expect("a generated run folds");
        checked(&cheap_run, &bars);
        checked(&dear_run, &bars);
        prop_assert_eq!(cheap_run.report.strategy.fees_total, Usd::ZERO);
        prop_assert!(dear_run.report.strategy.fees_total >= cheap_run.report.strategy.fees_total);
        if dear_run.report.strategy.fees_total > Usd::ZERO {
            prop_assert!(
                dear_run.report.strategy.total_return < cheap_run.report.strategy.total_return,
                "a fee that exists must lower the return, not leave it equal"
            );
            prop_assert!(
                dear_run.report.strategy.ending_equity < cheap_run.report.strategy.ending_equity
            );
        } else {
            prop_assert_eq!(
                dear_run.report.strategy.total_return,
                cheap_run.report.strategy.total_return
            );
        }
    }

    /// Every total recomputes from the run's own fills and orders: the notionals are the gross
    /// quantity times the price by side, the counts are the lengths, and the quantities are the sums of
    /// what was submitted and what filled (DEC-127 items 10 and 22). A loop that summed net quantities,
    /// limit prices, or one side twice would disagree here.
    #[test]
    #[ignore = "pending E4-2"]
    fn the_totals_recompute_from_the_runs_fills_and_orders(bars in generated_bars()) {
        let run = generated_run(&bars);
        checked(&run, &bars);
        let metrics = &run.report.strategy;

        let mut buys = 0i128;
        let mut sells = 0i128;
        let mut filled = 0i128;
        for fill in &run.fills {
            let qty = oracle::units(&fill.fill.qty.to_string(), 9);
            let price = oracle::units(&fill.fill.price.to_string(), 9);
            let notional = qty * price;
            match fill.execution.side {
                Side::Buy => buys += notional,
                Side::Sell => sells += notional,
            }
            filled += qty;
        }
        let submitted: i128 = run
            .orders
            .iter()
            .map(|o| oracle::units(&o.order.qty.to_string(), 9))
            .sum();

        prop_assert_eq!(oracle::units(&metrics.buy_notional.to_string(), 18), buys);
        prop_assert_eq!(oracle::units(&metrics.sell_notional.to_string(), 18), sells);
        prop_assert_eq!(oracle::units(&metrics.traded_notional.to_string(), 18), buys + sells);
        prop_assert_eq!(usize::try_from(metrics.fill_count).unwrap(), run.fills.len());
        prop_assert_eq!(oracle::units(&metrics.filled_qty.to_string(), 9), filled);
        prop_assert_eq!(oracle::units(&metrics.submitted_qty.to_string(), 9), submitted);
        prop_assert!(metrics.filled_qty <= metrics.submitted_qty);
    }

    /// Identical inputs give byte-identical reports and the same digest, which is the story's
    /// acceptance criterion.
    #[test]
    #[ignore = "pending E4-2"]
    fn identical_inputs_give_byte_identical_reports(bars in generated_bars()) {
        let config = run_config(equity(), crossover(), "100000");
        let first = run(&config, &bars).expect("a generated run folds");
        let again = run(&config, &bars).expect("a generated run folds");
        checked(&first, &bars);
        prop_assert_eq!(
            first.report.canonical_bytes().unwrap(),
            again.report.canonical_bytes().unwrap()
        );
        prop_assert_eq!(first.report.digest().unwrap(), again.report.digest().unwrap());
    }

    /// A run repeated in the same process is equal in every field, not only in its bytes: no
    /// collection's order and no hash's seed reaches a figure (ES-21).
    #[test]
    #[ignore = "pending E4-2"]
    fn a_run_repeated_in_the_same_process_is_equal(bars in generated_bars()) {
        let config = run_config(equity(), crossover(), "100000");
        let first = run(&config, &bars).expect("a generated run folds");
        let again = run(&config, &bars).expect("a generated run folds");
        checked(&first, &bars);
        prop_assert_eq!(first.report, again.report);
        prop_assert_eq!(first.equity, again.equity);
        prop_assert_eq!(first.fills, again.fills);
        prop_assert_eq!(first.orders, again.orders);
    }
}
