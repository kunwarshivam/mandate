//! A cash dividend inside a mandate-version run (DEC-2245 item 2, DEC-2253; trading spec §8.5;
//! mandate spec §5.2). Bars are stored raw (DEC-89), so from the ex-date the bars are lower by the
//! dividend. At 20:00 New York time on the last trading day before the ex-date the run books
//! round(Q × per_share, 2) as a receivable in its ledger, its risk state and the benchmark's ledger,
//! which pays it as cash on the pay date; the ex-date's decision reads the last close less the
//! dividend. The opening of 100.01 fills on day 20 (`version.rs`); the ex-date is day 25.

mod common;

use common::*;
use mandate_num::Price;
use mandate_risk::Purpose;
use mandate_sim::SimBar;
use mandate_time::Date;
use mandate_version_backtest::{
    DecisionOutcome, DecisionRow, RecordedAction, RecordedOutput, VersionRun,
};

const DAYS: usize = 30;
const EX: usize = 25;
const PAY: usize = 28;

/// `cents` hundredths in canonical decimal text.
fn cents_text(cents: i128) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

/// Flat bars at 100 with `volume` until day `ex` and at 100 less `dividend_cents` from it, each
/// with a 1% range either side.
fn ex_bars(ex: usize, dividend_cents: i128, volume: u64) -> Vec<SimBar> {
    trading_days(DAYS)
        .iter()
        .enumerate()
        .map(|(index, date)| {
            let close = if index < ex {
                10_000
            } else {
                10_000 - dividend_cents
            };
            daily_bar(
                *date,
                &cents_text(close),
                &cents_text(close + close / 100),
                &cents_text(close - close / 100),
                &cents_text(close),
                &volume.to_string(),
            )
        })
        .collect()
}

fn dividend(ex: usize, pay_date: Date, dividend_cents: i128) -> RecordedAction {
    RecordedAction::CashDividend {
        ex_date: trading_days(DAYS)[ex],
        pay_date,
        per_share: price(&cents_text(dividend_cents)),
    }
}

fn run_dividend(ex: usize, pay_date: Date, dividend_cents: i128) -> VersionRun {
    let days = trading_days(DAYS);
    run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &ex_bars(ex, dividend_cents, 1_000_000),
        &daily_outputs(&days, "0.5"),
        &[],
        &[dividend(ex, pay_date, dividend_cents)],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()))
}

fn equity_on(series: &[mandate_backtest::Observation], date: Date) -> mandate_num::Usd {
    series
        .iter()
        .find(|observation| observation.date == date)
        .unwrap_or_else(|| panic!("an observation on {date}"))
        .equity
}

/// A dividend moves no USD measure: the price falls by the dividend on the ex-date, the dividend
/// is a receivable from 20:00 the night before and cash from the pay date, so the version's and
/// the benchmark's equity at every close from the day before the ex-date to the last bar equal
/// the close before. The control without the action shows the fall the receivable offsets: its
/// version equity at the ex-date's close is lower by Q × 0.25, Q the shares the run filled.
#[test]
fn both_equities_are_flat_across_an_ex_date_and_a_pay_date() {
    let days = trading_days(DAYS);
    let run = run_dividend(EX, days[PAY], 25);
    for (name, series) in [
        ("strategy", &run.report.strategy_equity),
        ("benchmark", &run.report.benchmark_equity),
    ] {
        let before = equity_on(series, days[EX - 1]);
        for date in &days[EX..] {
            assert_eq!(equity_on(series, *date), before, "{name} on {date}");
        }
    }
    let control = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &ex_bars(EX, 25, 1_000_000),
        &daily_outputs(&days, "0.5"),
        &[],
        &[],
    )
    .unwrap_or_else(|e| panic!("the control completes: {}", e.code()));
    let shares: i128 = run
        .fills
        .iter()
        .map(|fill| {
            fill.execution
                .qty_gross
                .to_string()
                .parse::<i128>()
                .unwrap()
        })
        .sum();
    assert!(shares > 0, "the run holds shares across the ex-date");
    let cents = |usd: mandate_num::Usd| -> i128 {
        let text = usd.to_string();
        let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
        whole.parse::<i128>().unwrap() * 100 + format!("{fraction:0<2}").parse::<i128>().unwrap()
    };
    assert_eq!(
        cents(equity_on(&run.report.strategy_equity, days[EX]))
            - cents(equity_on(&control.report.strategy_equity, days[EX])),
        shares * 25,
        "the receivable is Q × 0.25"
    );
}

/// A dividend paid after the last bar stays in equity as a receivable: the version's last
/// equity equals its equity at the close before the ex-date.
#[test]
fn a_dividend_paid_after_the_last_bar_stays_in_equity() {
    let days = trading_days(DAYS);
    let after = (0..10).fold(days[DAYS - 1], |date, _| date.next().unwrap());
    let run = run_dividend(DAYS - 2, after, 25);
    assert_eq!(
        equity_on(&run.report.strategy_equity, days[DAYS - 1]),
        equity_on(&run.report.strategy_equity, days[DAYS - 3])
    );
}

/// The risk state reads the dividend as income, not as a fall: a 10.00 dividend on the 6 shares
/// the run holds is 60 USD, the tight ladder's 0.6% flatten rung on its own, and the run latches
/// nothing and ends no more restricted than a flat walk.
#[test]
fn the_risk_state_reads_no_drawdown_from_a_dividend() {
    let days = trading_days(DAYS);
    let control = run_with(
        &tight_ladder(),
        &settings(),
        &xyz(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
        &[],
    )
    .unwrap_or_else(|e| panic!("the control completes: {}", e.code()));
    assert!(control.report.limits_latched.is_empty());
    let run = run_with(
        &tight_ladder(),
        &settings(),
        &xyz(),
        &ex_bars(EX, 1_000, 1_000_000),
        &daily_outputs(&days, "0.5"),
        &[],
        &[dividend(EX, days[PAY], 1_000)],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(
        run.report.limits_latched.is_empty(),
        "{:?}",
        run.report.limits_latched
    );
    assert_eq!(
        run.report.most_restrictive_mode,
        control.report.most_restrictive_mode
    );
}

/// The ex-date morning's decision row of a run over `bars` with `actions`.
fn ex_date_row(
    bars: &[SimBar],
    outputs: &[RecordedOutput],
    actions: &[RecordedAction],
) -> DecisionRow {
    let ex_date = trading_days(DAYS)[EX];
    let run = run_with(&pinned(), &settings(), &xyz(), bars, outputs, &[], actions)
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    run.report
        .decisions
        .into_iter()
        .find(|row| row.date == ex_date)
        .unwrap_or_else(|| panic!("a decision on the ex-date"))
}

/// The ex-date's decision quotes the last close less the dividend (DEC-2253 item 2): an opening on
/// the ex-date morning after a 5.00 dividend is priced as a run whose bars were at 95 all along
/// prices it, and the gate's prior close agrees, where the cum-dividend close of 100 would set a
/// limit 5 above the market.
#[test]
fn an_opening_on_the_ex_date_reads_the_ex_dividend_close() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days[EX..], "0.5");
    let with = ex_date_row(
        &ex_bars(EX, 500, 1_000_000),
        &outputs,
        &[dividend(EX, days[PAY], 500)],
    );
    let control = ex_date_row(&ex_bars(0, 500, 1_000_000), &outputs, &[]);
    assert_eq!(with.outcome, DecisionOutcome::Submitted, "{with:?}");
    assert_eq!(with.purpose, Some(Purpose::Open), "{with:?}");
    assert_eq!((with.qty, with.limit), (control.qty, control.limit));
    assert!(with.limit < Some(Price::parse("100").unwrap()), "{with:?}");
}

/// A dividend whose ex-date is the first bar's trade date was taken before the run, which holds
/// nothing then: the run completes as it would without it.
#[test]
fn a_dividend_on_the_first_bars_date_changes_nothing() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let with = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &flat_bars(DAYS),
        &outputs,
        &[],
        &[dividend_on(days[0])],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let without = run(&pinned(), &flat_bars(DAYS), &outputs, &[])
        .unwrap_or_else(|e| panic!("the control completes: {}", e.code()));
    assert_eq!(with.report.strategy_equity, without.report.strategy_equity);
    assert_eq!(with.orders.len(), without.orders.len());
}

/// A dividend finer than the tick is rounded down onto it (DEC-2253 item 2): 100 less 0.333 is
/// 99.667, so the ex-date's opening quotes 99.66, as a run whose bars were at 99.66 all along does,
/// and not 99.67, which would lift a buy's limit above the market's.
#[test]
fn a_sub_tick_dividend_lowers_the_close_onto_the_tick_below() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days[EX..], "0.5");
    let action = RecordedAction::CashDividend {
        ex_date: days[EX],
        pay_date: days[PAY],
        per_share: price("0.333"),
    };
    let with = ex_date_row(&ex_bars(EX, 34, 1_000_000), &outputs, &[action]);
    let control = ex_date_row(&ex_bars(0, 34, 1_000_000), &outputs, &[]);
    assert_eq!(with.outcome, DecisionOutcome::Submitted, "{with:?}");
    assert_eq!((with.qty, with.limit), (control.qty, control.limit));
}
