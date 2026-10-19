//! A split inside a mandate-version run (DEC-2245 items 2 and 3; trading spec §8.5; mandate spec
//! §5.2). Bars are stored raw (DEC-89), so from the ex-date the bars are at post-split prices. At
//! 20:00 New York time on the last trading day before the ex-date the run applies the split to its
//! ledger, its risk state and the benchmark's ledger, cancels the resting protection, and re-places
//! it in the same step on the post-split position, its stop and limit carried by old ÷ new and
//! rounded toward more protection. The opening of 100.01 fills on day 20 and rests an OCO at stop
//! 95.01 and take-profit 110.02 (`version.rs`); the split's ex-date is day 25.

mod common;

use common::*;
use mandate_num::{Price, SplitRatio};
use mandate_sim::{OrderEnd, OrderKind, SimBar};
use mandate_time::Date;
use mandate_version_backtest::{RecordedAction, VersionOrder, VersionRun};
use proptest::prelude::*;
use proptest::test_runner::{Config as ProptestConfig, TestRunner};

const DAYS: usize = 30;
const EX: usize = 25;

/// `cents` hundredths in canonical decimal text.
fn cents_text(cents: i128) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}

/// A price's value in cents; every price these tests read is on the cent.
fn cents_of(value: Price) -> i128 {
    let text = value.to_string();
    let (whole, fraction) = text.split_once('.').unwrap_or((&text, ""));
    assert!(fraction.len() <= 2, "{text} is on the cent");
    whole.parse::<i128>().unwrap() * 100 + format!("{fraction:0<2}").parse::<i128>().unwrap()
}

/// Flat bars at 100 until the ex-date and at 100 × old ÷ new, to the cent, from it, each with a 1%
/// range either side.
fn split_bars(new: u64, old: u64) -> Vec<SimBar> {
    let days = trading_days(DAYS);
    let after = 10_000 * i128::from(old) / i128::from(new);
    days.iter()
        .enumerate()
        .map(|(index, date)| {
            let close = if index < EX { 10_000 } else { after };
            daily_bar(
                *date,
                &cents_text(close),
                &cents_text(close + close / 100),
                &cents_text(close - close / 100),
                &cents_text(close),
                "1000000",
            )
        })
        .collect()
}

fn split_at(new: u64, old: u64, cash_in_lieu_price: Option<Price>) -> RecordedAction {
    RecordedAction::Split {
        ex_date: trading_days(DAYS)[EX],
        ratio: SplitRatio::new(new, old).unwrap(),
        cash_in_lieu_price,
    }
}

fn run_split(new: u64, old: u64, cash_in_lieu_price: Option<Price>) -> VersionRun {
    let days = trading_days(DAYS);
    run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &split_bars(new, old),
        &daily_outputs(&days, "0.5"),
        &[],
        &[split_at(new, old, cash_in_lieu_price)],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()))
}

/// The split's instant: 20:00 New York time (EDT) on the last trading day before the ex-date.
fn split_instant() -> mandate_time::UtcNanos {
    let before: Date = trading_days(DAYS)[EX - 1];
    at(&format!("{}T00:00:00Z", before.next().unwrap()))
}

/// The protection resting before the split and the one re-placed at it.
fn protections(run: &VersionRun) -> (&VersionOrder, &VersionOrder) {
    let mut protective = run.orders.iter().filter(|order| order.protective);
    let before = protective.next().unwrap_or_else(|| panic!("protected"));
    let after = protective
        .find(|order| order.decided_at == split_instant())
        .unwrap_or_else(|| panic!("re-placed at the split: {:?}", run.orders));
    (before, after)
}

/// A split whose ex-date is the first bar's trade date was taken before the run: it applies before
/// any position exists and the run completes as it would without it.
#[test]
fn a_split_on_the_first_bars_date_completes_the_run() {
    let days = trading_days(DAYS);
    let action = RecordedAction::Split {
        ex_date: days[0],
        ratio: SplitRatio::new(2, 1).unwrap(),
        cash_in_lieu_price: None,
    };
    let with = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
        &[action],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let without = run(
        &pinned(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
    )
    .unwrap_or_else(|e| panic!("the control completes: {}", e.code()));
    assert_eq!(with.report.strategy_equity, without.report.strategy_equity);
    assert_eq!(with.orders.len(), without.orders.len());
}

/// A 2:1 split moves no USD measure: the version's and the benchmark's equity at the close before
/// the ex-date equal those at the ex-date's close, the position held at twice the shares and half
/// the price. Without the split the strategy's would halve its stock and the benchmark's too.
#[test]
fn both_equities_are_flat_across_a_two_for_one_split() {
    let run = run_split(2, 1, None);
    let days = trading_days(DAYS);
    for (name, series) in [
        ("strategy", &run.report.strategy_equity),
        ("benchmark", &run.report.benchmark_equity),
    ] {
        let at_date = |date: Date| {
            series
                .iter()
                .find(|observation| observation.date == date)
                .unwrap_or_else(|| panic!("{name} observes {date}"))
                .equity
        };
        assert_eq!(
            at_date(days[EX - 1]),
            at_date(days[EX]),
            "{name}'s equity is flat across the split"
        );
    }
    assert_eq!(run.report.risk_exits, 0);
}

/// The resting OCO is canceled at the split and re-placed in the same step on twice the shares:
/// stop 95.01 ÷ 2 = 47.505 up to 47.51, take-profit 110.02 ÷ 2 = 55.01 (DEC-2245 item 3).
#[test]
fn the_protection_is_re_placed_at_the_split_on_the_new_shares() {
    let run = run_split(2, 1, None);
    let (before, after) = protections(&run);
    assert_eq!(
        before.end,
        OrderEnd::Expired { at_bar: EX - 1 },
        "canceled after the last bar it worked through"
    );
    assert_eq!(after.order.qty.to_string(), {
        let held: u64 = before.order.qty.to_string().parse().unwrap();
        (held * 2).to_string()
    });
    assert_eq!(
        after.order.kind,
        OrderKind::Oco {
            limit: price("55.01"),
            stop: price("47.51"),
        }
    );
    assert_eq!(after.order.tif, mandate_sim::TimeInForce::Gtc);
}

/// A 1:3 reverse split re-places the protection on the whole shares left, the residual paid in cash
/// at the posted 300: stop 95.01 × 3 = 285.03 and take-profit 110.02 × 3 = 330.06, both exact.
#[test]
fn a_reverse_split_re_places_the_protection_on_the_whole_shares() {
    let run = run_split(1, 3, Some(price("300")));
    let (before, after) = protections(&run);
    let held: u64 = before.order.qty.to_string().parse().unwrap();
    assert_eq!(after.order.qty.to_string(), (held / 3).to_string());
    assert_eq!(
        after.order.kind,
        OrderKind::Oco {
            limit: price("330.06"),
            stop: price("285.03"),
        }
    );
}

/// The risk state takes the split too, so the post-split marks do not read as a 50% fall: under a
/// ladder that scales at 0.2% of equity, the same flat walk latches nothing with the split as
/// without it.
#[test]
fn the_risk_state_reads_no_drawdown_from_a_split() {
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
    assert!(
        control.report.limits_latched.is_empty(),
        "the flat walk latches nothing, so the comparison means something"
    );
    let split = run_with(
        &tight_ladder(),
        &settings(),
        &xyz(),
        &split_bars(2, 1),
        &daily_outputs(&days, "0.5"),
        &[],
        &[split_at(2, 1, None)],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(
        split.report.limits_latched.is_empty(),
        "{:?}",
        split.report.limits_latched
    );
    assert_eq!(
        split.report.most_restrictive_mode,
        control.report.most_restrictive_mode
    );
}

/// The re-placed protection never covers fewer shares than the post-split position and never
/// loosens a stop, for any ratio up to 6:6 (the coordinator's review of the plan). The oracle works
/// in integer cents from the pre-split OCO the run rested, 95.01 and 110.02: the stop is the
/// smallest cent at or above 95.01 × old ÷ new, the take-profit the largest at or below
/// 110.02 × old ÷ new, and the quantity Q × new ÷ old truncated to a whole share. It shares nothing
/// with `SplitRatio::price` or the fold.
#[test]
fn re_placed_protection_covers_the_position_and_never_loosens() {
    let mut runner = TestRunner::new(ProptestConfig {
        cases: 24,
        failure_persistence: None,
        ..ProptestConfig::default()
    });
    if let Err(failure) = runner.run(&(1_u64..=6, 1_u64..=6), |(new, old)| {
        let post = 10_000 * i128::from(old) / i128::from(new);
        let run = run_split(new, old, Some(price(&cents_text(post))));
        let (before, after) = protections(&run);
        let (n, o) = (i128::from(new), i128::from(old));
        let held: i128 = before.order.qty.to_string().parse().unwrap();
        prop_assert_eq!(
            after.order.qty.to_string(),
            (held * n / o).to_string(),
            "{}:{} covers the whole post-split position",
            new,
            old
        );
        let OrderKind::Oco { limit, stop } = after.order.kind else {
            return Err(TestCaseError::fail(format!(
                "{new}:{old}: {:?}",
                after.order.kind
            )));
        };
        let (stop, limit) = (cents_of(stop), cents_of(limit));
        prop_assert!(
            stop * n >= 9_501 * o && (stop - 1) * n < 9_501 * o,
            "{}:{} stop {} is the first cent at or above 95.01 × {} ÷ {}",
            new,
            old,
            stop,
            old,
            new
        );
        prop_assert!(
            limit * n <= 11_002 * o && (limit + 1) * n > 11_002 * o,
            "{}:{} take-profit {} is the last cent at or below 110.02 × {} ÷ {}",
            new,
            old,
            limit,
            old,
            new
        );
        Ok(())
    }) {
        panic!("re-placed protection: {failure}");
    }
}

/// The carried protection is live from the split's instant, not from the old protection's: a fall
/// to 45 on day 27 triggers the carried stop at 47.51 and sells all the post-split shares. Were
/// the re-placed OCO eligible from the old placement, its take-profit at 55.01 would have filled
/// against a pre-split bar at 100 and ended there, leaving nothing to trigger.
#[test]
fn the_carried_stop_triggers_on_a_post_split_fall() {
    let days = trading_days(DAYS);
    let mut bars = split_bars(2, 1);
    bars[EX + 2] = daily_bar(days[EX + 2], "50", "50.5", "45", "46", "1000000");
    let run = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &bars,
        &daily_outputs(&days, "0.5"),
        &[],
        &[split_at(2, 1, None)],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let (before, after) = protections(&run);
    let held: u64 = before.order.qty.to_string().parse().unwrap();
    let stopped = run
        .fills
        .iter()
        .find(|fill| fill.execution.executed_at.date() == days[EX + 2])
        .unwrap_or_else(|| panic!("the carried stop triggers on day 27: {:?}", run.fills));
    assert_eq!(stopped.execution.side, mandate_accounting::Side::Sell);
    assert_eq!(
        stopped.execution.qty_gross.to_string(),
        (held * 2).to_string()
    );
    assert_eq!(after.end, OrderEnd::Filled);
}

/// A stop-limit protection is carried the same way, each price on its protective side: stop
/// 95.01 ÷ 2 = 47.505 up to 47.51, limit 94.53495 ÷ 2 = 47.267475 down to 47.26.
#[test]
fn a_stop_limit_protection_is_carried_on_its_protective_sides() {
    let days = trading_days(DAYS);
    let mut document = base("research_equity_pinned");
    for (path, value) in [
        ("/protection/take_profit_distance", serde_json::Value::Null),
        (
            "/protection/crypto_stop_limit_offset",
            serde_json::Value::String("0.005".to_owned()),
        ),
    ] {
        *document
            .pointer_mut(path)
            .unwrap_or_else(|| panic!("{path}")) = value;
    }
    let mut settings = settings();
    settings.profile = stop_limit_profile();
    let run = run_with(
        &validate(&document),
        &settings,
        &xyz(),
        &split_bars(2, 1),
        &daily_outputs(&days, "0.5"),
        &[],
        &[split_at(2, 1, None)],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let (_, after) = protections(&run);
    assert_eq!(
        after.order.kind,
        OrderKind::StopLimit {
            stop: price("47.51"),
            limit: price("47.26"),
        }
    );
}

/// Splits out of ex-date order refuse the run, as the benchmark's walk refuses them (DEC-2245 item
/// 7), rather than skip one.
#[test]
#[ignore = "pending E4-4"]
fn splits_out_of_ex_date_order_refuse_the_run() {
    let days = trading_days(DAYS);
    let earlier = RecordedAction::Split {
        ex_date: days[EX - 5],
        ratio: SplitRatio::new(2, 1).unwrap(),
        cash_in_lieu_price: None,
    };
    let refused = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &split_bars(2, 1),
        &daily_outputs(&days, "0.5"),
        &[],
        &[split_at(2, 1, None), earlier],
    );
    assert_eq!(
        refused.err().map(|e| e.code()),
        Some("corporate_actions_unordered")
    );
}
