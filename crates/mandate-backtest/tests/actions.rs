//! Corporate actions in a run (DEC-2245 items 1 and 2; trading spec §8.5). Bars are stored raw
//! (DEC-89), so after a split the bars are already at post-split prices; the run applies the split
//! to its ledger at 20:00 New York time before the ex-date, which rescales the position and its
//! marks, so a flat price series stays a flat equity series through the split. Without it, the
//! position held at the old quantity would be marked at the new price and equity would fall by the
//! split's whole ratio.

mod common;

use common::*;
use mandate_accounting::{CorporateAction, Split};
use mandate_backtest::{BacktestError, BacktestInput, BacktestRun, RunConfig};
use mandate_num::{ShareIncrement, SplitRatio, Usd};
use mandate_sim::SimBar;
use mandate_time::Date;
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestRunner};

/// Ten trading days, the first five before the ex-date (2026-09-28) and the last five on or
/// after it.
const DAYS: [&str; 10] = [
    "2026-09-21",
    "2026-09-22",
    "2026-09-23",
    "2026-09-24",
    "2026-09-25",
    "2026-09-28",
    "2026-09-29",
    "2026-09-30",
    "2026-10-01",
    "2026-10-02",
];
const EX_DATE: &str = "2026-09-28";

/// A flat day at `close`: its opening auction bar and its closing regular bar, with volume enough
/// that the benchmark's buy fills whole.
fn flat_day(day: &str, close: &str) -> [SimBar; 2] {
    [
        auction(day, "09:30", [close, close, close, close, "10000000"]),
        bar(day, "15:59", [close, close, close, close, "10000000"]),
    ]
}

/// Five days at `before` and five at `after`, the change on the ex-date.
fn split_series(before: &str, after: &str) -> Vec<SimBar> {
    DAYS.iter()
        .flat_map(|day| {
            flat_day(
                day,
                if Date::parse(day).unwrap() < d(EX_DATE) {
                    before
                } else {
                    after
                },
            )
        })
        .collect()
}

fn config() -> RunConfig {
    let mut config = run_config(equity(), buy_and_hold(), "100000");
    config.fees = no_equity_fees();
    config
}

fn split(
    config: &RunConfig,
    (new, old): (u64, u64),
    cash_in_lieu_price: Option<&str>,
) -> CorporateAction {
    split_on(config, d(EX_DATE), (new, old), cash_in_lieu_price)
}

fn split_on(
    config: &RunConfig,
    ex_date: Date,
    (new, old): (u64, u64),
    cash_in_lieu_price: Option<&str>,
) -> CorporateAction {
    CorporateAction::Split(Split {
        instrument: config.instrument_id.clone(),
        ex_date,
        ratio: SplitRatio::new(new, old).unwrap(),
        increment: ShareIncrement::Whole,
        cash_in_lieu_price: cash_in_lieu_price.map(price),
    })
}

fn run_actions(
    config: &RunConfig,
    bars: &[SimBar],
    actions: &[CorporateAction],
) -> Result<BacktestRun, BacktestError> {
    let medians = Median(qty("10000000"));
    mandate_backtest::run(&BacktestInput {
        config,
        bars,
        coverage_start: bars[0].start,
        first_bar_volumes: &medians,
        corporate_actions: actions,
    })
}

/// The equity at each period close from the first close after the buy filled, once the buy is
/// shown to have filled: a run that held nothing would be flat through any split.
fn held_equities(run: &BacktestRun) -> Vec<Usd> {
    assert!(
        !run.report.strategy.filled_qty.is_zero(),
        "the buy fills, so the split has a position to act on"
    );
    equities(&run.equity).into_iter().skip(1).collect()
}

/// A 2:1 split halves the raw price and doubles the shares, so a flat series stays flat: every
/// period close after the buy has the same equity, on both sides of the ex-date.
#[test]
fn buy_and_hold_holds_through_a_two_for_one_split_with_flat_equity() {
    let config = config();
    let bars = split_series("100", "50");
    let run = run_actions(&config, &bars, &[split(&config, (2, 1), None)])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let held = held_equities(&run);
    assert!(held.len() >= 8, "the buy fills early: {held:?}");
    assert!(
        held.windows(2).all(|pair| pair[0] == pair[1]),
        "equity is flat through the split: {held:?}"
    );
}

/// An ex-date on a day with no bars, a weekend or a holiday, applies before the first bar traded
/// after it, as one on a trading day does: the bars change price on the Monday after a Sunday
/// ex-date, and on the day after a holiday whose bars the series leaves out.
#[test]
fn an_ex_date_without_bars_applies_before_the_next_trading_day() {
    let config = config();
    let sunday = split_series("100", "50");
    let holiday: Vec<SimBar> = DAYS
        .iter()
        .filter(|day| **day != EX_DATE)
        .flat_map(|day| flat_day(day, if d(day) < d(EX_DATE) { "100" } else { "50" }))
        .collect();
    for (name, bars, ex_date) in [
        ("sunday", sunday, d("2026-09-27")),
        ("holiday", holiday, d(EX_DATE)),
    ] {
        let run = run_actions(&config, &bars, &[split_on(&config, ex_date, (2, 1), None)])
            .unwrap_or_else(|e| panic!("{name}: the run completes: {}", e.code()));
        let held = held_equities(&run);
        assert!(
            held.windows(2).all(|pair| pair[0] == pair[1]),
            "{name}: equity is flat through the split: {held:?}"
        );
    }
}

/// A 1:3 reverse split of a holding that is not a multiple of 3 leaves a residual, paid in cash at
/// the posted 300; with the raw price tripled to 300 too, equity stays flat, and the residual's
/// cash is the difference between Q × 100 and Q' × 300.
#[test]
fn a_reverse_split_pays_cash_in_lieu_for_its_residual() {
    let config = config();
    let bars = split_series("100", "300");
    let run = run_actions(&config, &bars, &[split(&config, (1, 3), Some("300"))])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert_ne!(
        run.report
            .strategy
            .filled_qty
            .to_string()
            .parse::<u64>()
            .unwrap()
            % 3,
        0,
        "the holding leaves a residual, so cash in lieu is exercised"
    );
    let held = held_equities(&run);
    assert!(
        held.windows(2).all(|pair| pair[0] == pair[1]),
        "equity is flat through the reverse split: {held:?}"
    );
}

/// A buy still working when a split applies would fill against post-split raw prices at its
/// pre-split limit, so the run refuses rather than guess its adjustment: the limit at the first
/// close plus the collar rests below the pre-split bars at 120 and would fill at 60 after a 2:1.
#[test]
fn an_order_working_across_a_split_refuses_the_run() {
    let config = config();
    let mut bars = split_series("120", "60");
    bars[0] = auction(DAYS[0], "09:30", ["100", "100", "100", "100", "10000000"]);
    let refused = run_actions(&config, &bars, &[split(&config, (2, 1), None)]);
    assert_eq!(
        refused.err().map(|e| e.code()),
        Some("order_working_across_split")
    );
}

/// Actions outside the run change nothing: one whose ex-date is the first bar's trade date was
/// taken before the run began, and one after the last bar after it ended.
#[test]
fn actions_outside_the_bars_change_nothing() {
    let config = config();
    let bars = split_series("100", "100");
    let first = bars[0].trade_date;
    let after = d("2026-10-05");
    let outside = |ex_date: Date| {
        CorporateAction::Split(Split {
            instrument: config.instrument_id.clone(),
            ex_date,
            ratio: SplitRatio::new(2, 1).unwrap(),
            increment: ShareIncrement::Whole,
            cash_in_lieu_price: None,
        })
    };
    let plain = run_actions(&config, &bars, &[]).unwrap();
    let with = run_actions(&config, &bars, &[outside(first), outside(after)]).unwrap();
    assert_eq!(held_equities(&with), held_equities(&plain));
    assert_eq!(with.fills.len(), plain.fills.len());
}

/// `text` in billionths, the finest step a price or an amount here carries.
fn billionths(text: &str) -> i128 {
    let (whole, fraction) = text.split_once('.').unwrap_or((text, ""));
    let digits = format!("{whole}{fraction:0<9}");
    digits
        .parse()
        .unwrap_or_else(|_| panic!("{text} is a decimal"))
}

/// Equity is flat through any split of a flat series, whatever the ratio: before the ex-date the
/// price is k × new, after it k × old, and cash in lieu is posted at the post-split price, so
/// Q' × k × old + f × k × old = Q × k × new exactly. The oracle is that constant, the starting cash
/// less what the buys paid plus Q × k × new, summed in integer billionths from the fills; it shares
/// nothing with the fold.
#[test]
fn equity_is_flat_through_any_split_of_a_flat_series() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    let strategy = (1_u64..=10, 1_u64..=10, 5_u64..=50);
    if let Err(failure) = runner.run(&strategy, |(new, old, k)| {
        let config = config();
        let before = (k * new).to_string();
        let after = (k * old).to_string();
        let bars = split_series(&before, &after);
        let run = run_actions(
            &config,
            &bars,
            &[split(&config, (new, old), Some(after.as_str()))],
        )
        .map_err(|e| TestCaseError::fail(format!("{new}:{old} at {k}: {}", e.code())))?;
        let held = held_equities(&run);
        let bought: i128 = run
            .fills
            .iter()
            .map(|fill| {
                let shares = billionths(&fill.execution.qty_gross.to_string());
                shares * billionths(&fill.execution.price.to_string()) / 1_000_000_000
            })
            .sum();
        let shares = billionths(&run.report.strategy.filled_qty.to_string());
        let expected = billionths("100000") - bought + shares * i128::from(k * new);
        prop_assert_eq!(
            billionths(&held[0].to_string()),
            expected,
            "{}:{} at {}: the first held close",
            new,
            old,
            k
        );
        prop_assert!(
            held.windows(2).all(|pair| pair[0] == pair[1]),
            "{}:{} at {}: {:?}",
            new,
            old,
            k,
            held
        );
        Ok(())
    }) {
        panic!("equity through a split: {failure}");
    }
}

fn other_instrument(config: &RunConfig) -> CorporateAction {
    let mut other = config.clone();
    other.instrument_id = id("OTHER");
    split(&other, (2, 1), None)
}

fn out_of_order(config: &RunConfig) -> [CorporateAction; 2] {
    [
        split_on(config, d("2026-10-01"), (2, 1), None),
        split_on(config, d(EX_DATE), (2, 1), None),
    ]
}

/// The walk takes actions in the order given, so it refuses, rather than skip, a slice out of
/// ex-date order or an action for another instrument than the run's (DEC-2245 item 7).
#[test]
fn actions_it_cannot_apply_as_given_refuse_the_run() {
    let config = config();
    let bars = split_series("100", "100");
    assert!(run_actions(&config, &bars, &out_of_order(&config)).is_err());
    assert!(run_actions(&config, &bars, &[other_instrument(&config)]).is_err());
}

#[test]
fn actions_out_of_ex_date_order_refuse_with_their_code() {
    let config = config();
    let bars = split_series("100", "100");
    let refused = run_actions(&config, &bars, &out_of_order(&config));
    assert_eq!(
        refused.err().map(|e| e.code()),
        Some("corporate_actions_unordered")
    );
}

#[test]
fn an_action_for_another_instrument_refuses_with_its_code() {
    let config = config();
    let bars = split_series("100", "100");
    let refused = run_actions(&config, &bars, &[other_instrument(&config)]);
    assert_eq!(
        refused.err().map(|e| e.code()),
        Some("corporate_action_other_instrument")
    );
}
