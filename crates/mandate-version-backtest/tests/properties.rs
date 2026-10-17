//! Property tests for the version backtest (DEC-1000 item 10), each with an oracle of its own: it
//! reads the inputs and the run's raw orders and fills, never the code under test's own judgments,
//! and each oracle is first shown to fail on a seeded bug.
//!
//! - **Pure.** The same inputs give byte-identical report bytes, and equal orders and fills.
//! - **Inside the bar.** No fill is better than any price its bar traded, and none lies past the
//!   bar's range except by the fill model's own slippage, against the agent (trading spec §6.4).
//! - **No look-ahead.** No order is decided before the earliest receive time of any output, and
//!   no fill comes before its order's decision.
//! - **Asks never trade.** A version whose every opening asks places no order and has no fill.
//! - **Exits are never denied** by the gate (`AGENTS.md` rule 13).

mod common;

use common::*;
use mandate_num::Price;
use mandate_risk::Purpose;
use mandate_sim::SimBar;
use mandate_time::UtcNanos;
use mandate_version_backtest::{DecisionOutcome, RecordedOutput, VersionRun};
use proptest::prelude::*;

/// Every fill inside its bar, read from the bars themselves, as trading spec §6.4 allows it: never
/// at a price better than any the bar traded (a buy at or above its low, a sell at or below its
/// high), and past the bar's range only on the side against the agent and only by the fill model's
/// slippage `s` (rules 4 and 6: a market or stop fill at `open × (1 ± s)` or `S × (1 − s)`), so a
/// buy at most `high × (1 + s)` and a sell at least `low × (1 − s)` (DEC-1000 item 10).
fn fills_inside_their_bars(run: &VersionRun, bars: &[SimBar]) -> Result<(), String> {
    let slippage = match settings().sim.slippage {
        mandate_sim::Slippage::Fixed {
            half_spread_bps,
            impact_bps,
        } => half_spread_bps
            .checked_add(impact_bps)
            .map_err(|e| e.to_string())?,
        mandate_sim::Slippage::Sqrt { .. } => {
            return Err("the oracle reads a fixed slippage".to_owned());
        }
    };
    for fill in &run.fills {
        let bar = bars
            .get(fill.fill.bar)
            .ok_or_else(|| format!("fill names bar {}, which does not exist", fill.fill.bar))?;
        let price = fill.fill.price;
        let inside = match fill.execution.side {
            mandate_accounting::Side::Buy => {
                let ceiling = bar
                    .high
                    .slipped(slippage, mandate_num::Adverse::Up)
                    .map_err(|e| e.to_string())?;
                price >= bar.low && price <= ceiling
            }
            mandate_accounting::Side::Sell => {
                let floor = bar
                    .low
                    .slipped(slippage, mandate_num::Adverse::Down)
                    .map_err(|e| e.to_string())?;
                price <= bar.high && price >= floor
            }
        };
        if !inside {
            return Err(format!(
                "a {:?} fill at {price} lies outside bar {}'s [{}, {}] beyond the slippage",
                fill.execution.side, fill.fill.bar, bar.low, bar.high
            ));
        }
    }
    Ok(())
}

/// No order decided before the first output could be seen, and no fill before its order's
/// decision, from the recorded receive times and the orders' own instants.
fn nothing_before_its_information(
    run: &VersionRun,
    outputs: &[RecordedOutput],
) -> Result<(), String> {
    let first_seen = outputs.iter().map(|recorded| recorded.received_at).min();
    for order in run.orders.iter().filter(|order| !order.protective) {
        if first_seen.is_none_or(|seen| order.decided_at < seen) {
            return Err(format!(
                "order {} was decided before any output was received",
                order.index
            ));
        }
    }
    for fill in &run.fills {
        let order_index: u32 = fill
            .execution
            .client_order_id
            .as_deref()
            .and_then(|id| id.strip_prefix('o'))
            .and_then(|digits| digits.parse().ok())
            .ok_or_else(|| "a fill names no order".to_owned())?;
        let order = run
            .orders
            .iter()
            .find(|order| order.index == order_index)
            .ok_or_else(|| format!("a fill names order {order_index}, which was never placed"))?;
        if fill.execution.executed_at < order.decided_at {
            return Err(format!("order {order_index} filled before it was decided"));
        }
    }
    Ok(())
}

/// Each decision's combined outputs were all received by its instant: no row combines more models
/// than had an output received at or before it, counted from the recorded outputs themselves.
fn outputs_received_by_each_decision(
    run: &VersionRun,
    outputs: &[RecordedOutput],
) -> Result<(), String> {
    for row in &run.report.decisions {
        let received: std::collections::BTreeSet<&str> = outputs
            .iter()
            .filter(|recorded| recorded.received_at <= row.at)
            .map(|recorded| recorded.output.model_id.as_str())
            .collect();
        let seen = u32::try_from(received.len()).map_err(|e| e.to_string())?;
        if row.outputs_used > seen {
            return Err(format!(
                "a decision at {} combined {} outputs with {seen} received",
                row.at, row.outputs_used
            ));
        }
    }
    Ok(())
}

/// No opening or increase of a research-agent version goes out unless the latest thesis record
/// received by its instant (at one receive time, a refused record before an admitted one, then
/// thesis id; DEC-1140 item 1) is admitted, unexpired and not invalidated, recomputed from the
/// records.
fn openings_only_on_an_active_thesis(
    run: &VersionRun,
    theses: &[mandate_version_backtest::RecordedThesis],
) -> Result<(), String> {
    for row in &run.report.decisions {
        let opening = matches!(row.purpose, Some(Purpose::Open | Purpose::Increase));
        if !opening || row.outcome != DecisionOutcome::Submitted {
            continue;
        }
        let mut latest: Option<&mandate_version_backtest::RecordedThesis> = None;
        for thesis in theses.iter().filter(|thesis| thesis.received_at <= row.at) {
            let later = latest.is_none_or(|kept| {
                let key = |record: &mandate_version_backtest::RecordedThesis| {
                    (
                        record.received_at,
                        !record.admitted,
                        record.thesis_id.clone(),
                    )
                };
                key(thesis) > key(kept)
            });
            if later {
                latest = Some(thesis);
            }
        }
        let active = latest.is_some_and(|thesis| {
            thesis.admitted
                && row.at < thesis.expires_at
                && thesis.invalidated_at.is_none_or(|at| row.at < at)
        });
        if !active {
            return Err(format!(
                "an opening at {} had no active admitted thesis",
                row.at
            ));
        }
    }
    Ok(())
}

/// No exit row the gate denied, and no risk exit it held or deferred: a risk exit is the
/// automated kill switch's or the trim's, which no mode, conduct control or session stops (rule
/// 13).
fn exits_never_denied(run: &VersionRun) -> Result<(), String> {
    for row in &run.report.decisions {
        let exit = matches!(
            row.purpose,
            Some(Purpose::DiscretionaryExit | Purpose::RiskExit)
        );
        if exit && row.outcome == DecisionOutcome::GateDeny {
            return Err(format!("an exit was denied: {row:?}"));
        }
        let stopped = matches!(
            row.outcome,
            DecisionOutcome::GateHold | DecisionOutcome::GateDefer
        );
        if row.purpose == Some(Purpose::RiskExit) && stopped {
            return Err(format!("a risk exit was held: {row:?}"));
        }
    }
    Ok(())
}

/// `count` bars from 100, each close moved by the step in basis points `steps[i]`, with a range
/// that holds the open and the close and some room either side.
fn walk(count: usize, steps: &[i64], ranges: &[u32], volumes: &[u32]) -> Vec<SimBar> {
    let days = trading_days(count);
    let mut close: i64 = 10_000;
    let mut bars = Vec::new();
    for (index, date) in days.into_iter().enumerate() {
        let open = close;
        let step = steps.get(index).copied().unwrap_or(0);
        close = (close + close * step / 10_000).clamp(2_000, 30_000);
        let room = i64::from(ranges.get(index).copied().unwrap_or(50));
        let high = open.max(close) + room;
        let low = (open.min(close) - room).max(1_000);
        let cents = |value: i64| {
            let text = format!("{}.{:02}", value / 100, value % 100);
            text.trim_end_matches('0').trim_end_matches('.').to_owned()
        };
        let volume = format!(
            "{}",
            800_000 + u64::from(volumes.get(index).copied().unwrap_or(0))
        );
        bars.push(daily_bar(
            date,
            &cents(open),
            &cents(high),
            &cents(low),
            &cents(close),
            &volume,
        ));
    }
    bars
}

/// One output per model on each day, conviction `convictions[i]` hundredths, received `delays[i]`
/// minutes after its 09:20 `as_of`: some before the 09:30 decision, some after it.
fn outputs(count: usize, convictions: &[i32], delays: &[u32]) -> Vec<RecordedOutput> {
    let days = trading_days(count);
    days.iter()
        .enumerate()
        .flat_map(|(index, date)| {
            let conviction = convictions.get(index).copied().unwrap_or(50);
            let magnitude = conviction.unsigned_abs().min(99);
            let digits = format!("0.{magnitude:02}");
            let digits = digits.trim_end_matches('0').trim_end_matches('.');
            let text = match (conviction < 0, magnitude) {
                (_, 0) => "0".to_owned(),
                (true, _) => format!("-{digits}"),
                (false, _) => digits.to_owned(),
            };
            let as_of = at(&format!("{date}T13:20:00Z"));
            let delay = i64::from(delays.get(index).copied().unwrap_or(1));
            let received_at = UtcNanos::from_parts(as_of.secs() + delay * 60, 0)
                .unwrap_or_else(|e| panic!("{}", e.code()));
            [
                ("llm.research_agent", "0.1.0", RESEARCH_HASH),
                ("quant.momentum", "1.0.0", MOMENTUM_HASH),
            ]
            .into_iter()
            .map(move |(model, version, hash)| RecordedOutput {
                received_at,
                output: model_output(model, version, hash, as_of, &text),
            })
            .collect::<Vec<_>>()
        })
        .collect()
}

fn scenario() -> impl Strategy<Value = (Vec<SimBar>, Vec<RecordedOutput>)> {
    (24usize..34).prop_flat_map(|count| {
        (
            prop::collection::vec(-400i64..400, count),
            prop::collection::vec(0u32..400, count),
            prop::collection::vec(0u32..1_000_000, count),
            prop::collection::vec(-95i32..95, count),
            prop::collection::vec(0u32..20, count),
        )
            .prop_map(move |(steps, ranges, volumes, convictions, delays)| {
                (
                    walk(count, &steps, &ranges, &volumes),
                    outputs(count, &convictions, &delays),
                )
            })
    })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn a_version_backtest_is_a_pure_function_of_its_inputs((bars, recorded) in scenario()) {
        let first = run(&pinned(), &bars, &recorded, &[]).map_err(|e| TestCaseError::fail(e.code()))?;
        let second = run(&pinned(), &bars, &recorded, &[]).map_err(|e| TestCaseError::fail(e.code()))?;
        let bytes = first.report.canonical_bytes().map_err(|e| TestCaseError::fail(e.code()))?;
        prop_assert_eq!(bytes, second.report.canonical_bytes().map_err(|e| TestCaseError::fail(e.code()))?);
        prop_assert_eq!(&first.orders, &second.orders);
        prop_assert_eq!(&first.fills, &second.fills);
    }

    #[test]
    fn no_fill_lies_outside_its_bar((bars, recorded) in scenario()) {
        let run = run(&pinned(), &bars, &recorded, &[]).map_err(|e| TestCaseError::fail(e.code()))?;
        fills_inside_their_bars(&run, &bars).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn nothing_is_decided_or_filled_before_its_information((bars, recorded) in scenario()) {
        let run = run(&pinned(), &bars, &recorded, &[]).map_err(|e| TestCaseError::fail(e.code()))?;
        nothing_before_its_information(&run, &recorded).map_err(TestCaseError::fail)?;
        outputs_received_by_each_decision(&run, &recorded).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn a_research_version_opens_only_on_an_active_thesis(
        (bars, recorded) in scenario(),
        records in prop::collection::vec(
            (0usize..22, 2usize..14, prop::bool::weighted(0.8), prop::option::weighted(0.3, 1usize..12)),
            0..4,
        ),
        anchor in prop::bool::weighted(0.8),
    ) {
        let days = trading_days(bars.len());
        let theses: Vec<_> = records
            .iter()
            .enumerate()
            .map(|(index, (from, length, admitted, invalidated))| {
                let received = at(&format!("{}T13:{:02}:00Z", days[*from], 25 + index));
                let until = days.get(from + length).copied().unwrap_or(days[days.len() - 1]);
                mandate_version_backtest::RecordedThesis {
                    thesis_id: format!("th_{index}"),
                    invalidated_at: invalidated
                        .and_then(|after| days.get(from + after))
                        .map(|date| at(&format!("{date}T13:30:00Z")))
                        .filter(|at| *at >= received),
                    ..thesis(received, at(&format!("{until}T13:30:00Z")), *admitted)
                }
            })
            .collect();
        let mut theses = theses;
        if anchor {
            theses.insert(0, mandate_version_backtest::RecordedThesis {
                thesis_id: "th_anchor".to_owned(),
                ..thesis(at(&format!("{}T12:00:00Z", days[1])), at("2026-12-01T12:00:00Z"), true)
            });
        }
        let run = run(&research(), &bars, &recorded, &theses).map_err(|e| TestCaseError::fail(e.code()))?;
        openings_only_on_an_active_thesis(&run, &theses).map_err(TestCaseError::fail)?;
        outputs_received_by_each_decision(&run, &recorded).map_err(TestCaseError::fail)?;
    }

    #[test]
    fn an_unanswered_ask_never_trades((bars, recorded) in scenario()) {
        let run = run(&ask_only(), &bars, &recorded, &[]).map_err(|e| TestCaseError::fail(e.code()))?;
        prop_assert!(run.orders.is_empty());
        prop_assert!(run.fills.is_empty());
    }

    #[test]
    fn an_exit_is_never_denied_by_the_gate((bars, recorded) in scenario()) {
        let run = run(&pinned(), &bars, &recorded, &[]).map_err(|e| TestCaseError::fail(e.code()))?;
        exits_never_denied(&run).map_err(TestCaseError::fail)?;
        let tight = run_tight(&bars, &recorded)?;
        exits_never_denied(&tight).map_err(TestCaseError::fail)?;
        fills_inside_their_bars(&tight, &bars).map_err(TestCaseError::fail)?;
    }
}

/// The tight-ladder version over the same scenario, so the risk exits are reached.
fn run_tight(bars: &[SimBar], recorded: &[RecordedOutput]) -> Result<VersionRun, TestCaseError> {
    run(&tight_ladder(), bars, recorded, &[]).map_err(|e| TestCaseError::fail(e.code()))
}

/// A run with fills and exits to seed bugs into: the stop case of the hand tests.
fn seeded_run() -> (VersionRun, Vec<SimBar>, Vec<RecordedOutput>) {
    let days = trading_days(30);
    let mut bars = flat_bars(30);
    bars[24] = daily_bar(days[24], "99", "99", "90", "91", "1000000");
    let recorded = daily_outputs(&days, "0.5");
    let run = run(&pinned(), &bars, &recorded, &[]).unwrap_or_else(|e| panic!("{}", e.code()));
    (run, bars, recorded)
}

/// The bar-range oracle catches a buy filled 1% past its bar's high, a buy below its low, and a
/// sell above its high.
#[test]
fn the_bar_range_oracle_catches_a_fill_outside_its_bar() {
    let (run, bars, _) = seeded_run();
    assert!(fills_inside_their_bars(&run, &bars).is_ok());
    let buy = run
        .fills
        .iter()
        .position(|fill| fill.execution.side == mandate_accounting::Side::Buy)
        .unwrap_or_else(|| panic!("a buy"));
    let sell = run
        .fills
        .iter()
        .position(|fill| fill.execution.side == mandate_accounting::Side::Sell)
        .unwrap_or_else(|| panic!("a sell"));
    let moved = |index: usize, to: fn(&SimBar) -> Price| {
        let mut bad = run.clone();
        let bar = &bars[bad.fills[index].fill.bar];
        bad.fills[index].fill.price = to(bar);
        bad
    };
    let past_high = moved(buy, |bar| {
        bar.high
            .slipped(
                mandate_num::Bps::parse("100").unwrap_or_else(|e| panic!("{e}")),
                mandate_num::Adverse::Up,
            )
            .unwrap_or_else(|e| panic!("{e}"))
    });
    assert!(fills_inside_their_bars(&past_high, &bars).is_err());
    let below_low = moved(buy, |bar| {
        bar.low
            .slipped(
                mandate_num::Bps::parse("1").unwrap_or_else(|e| panic!("{e}")),
                mandate_num::Adverse::Down,
            )
            .unwrap_or_else(|e| panic!("{e}"))
    });
    assert!(fills_inside_their_bars(&below_low, &bars).is_err());
    let above_high = moved(sell, |bar| {
        bar.high
            .slipped(
                mandate_num::Bps::parse("1").unwrap_or_else(|e| panic!("{e}")),
                mandate_num::Adverse::Up,
            )
            .unwrap_or_else(|e| panic!("{e}"))
    });
    assert!(fills_inside_their_bars(&above_high, &bars).is_err());
}

/// The look-ahead oracle catches an order decided before any output was received, and a fill
/// before its order's decision.
#[test]
fn the_lookahead_oracle_catches_an_early_order_and_an_early_fill() {
    let (run, _, recorded) = seeded_run();
    assert!(nothing_before_its_information(&run, &recorded).is_ok());
    let mut late = recorded.clone();
    for output in &mut late {
        output.received_at = at("2026-12-01T00:00:00Z");
    }
    assert!(nothing_before_its_information(&run, &late).is_err());
    let mut early_fill = run.clone();
    let fill = early_fill
        .fills
        .first_mut()
        .unwrap_or_else(|| panic!("a fill"));
    fill.execution.executed_at = at("2026-09-01T00:00:00Z");
    assert!(nothing_before_its_information(&early_fill, &recorded).is_err());
}

/// The per-decision oracle catches a row that combined outputs no one had received yet, and the
/// thesis oracle an opening with no active thesis.
#[test]
fn the_visibility_oracles_catch_an_early_output_and_an_unheld_opening() {
    let (mut run, _, recorded) = seeded_run();
    assert!(outputs_received_by_each_decision(&run, &recorded).is_ok());
    let row = run
        .report
        .decisions
        .first_mut()
        .unwrap_or_else(|| panic!("a row"));
    row.at = at("2026-09-01T00:00:00Z");
    row.outputs_used = 2;
    assert!(outputs_received_by_each_decision(&run, &recorded).is_err());
    let opening = run
        .report
        .decisions
        .iter()
        .position(|row| {
            row.outcome == DecisionOutcome::Submitted && row.purpose == Some(Purpose::Open)
        })
        .unwrap_or_else(|| panic!("an opening"));
    assert!(openings_only_on_an_active_thesis(&run, &[]).is_err());
    let expired = [thesis(
        at("2026-09-01T12:00:00Z"),
        at("2026-09-02T12:00:00Z"),
        true,
    )];
    assert!(openings_only_on_an_active_thesis(&run, &expired).is_err());
    let live = [thesis(
        at("2026-09-01T12:00:00Z"),
        at("2026-12-01T12:00:00Z"),
        true,
    )];
    assert!(
        openings_only_on_an_active_thesis(&run, &live).is_ok(),
        "{opening}"
    );
}

/// The exit oracle catches a denied exit row.
#[test]
fn the_exit_oracle_catches_a_denied_exit() {
    let (mut run, _, _) = seeded_run();
    assert!(exits_never_denied(&run).is_ok());
    let row = run
        .report
        .decisions
        .first_mut()
        .unwrap_or_else(|| panic!("a row"));
    row.purpose = Some(Purpose::DiscretionaryExit);
    row.outcome = DecisionOutcome::GateDeny;
    assert!(exits_never_denied(&run).is_err());
    let row = run
        .report
        .decisions
        .first_mut()
        .unwrap_or_else(|| panic!("a row"));
    row.purpose = Some(Purpose::RiskExit);
    row.outcome = DecisionOutcome::GateHold;
    assert!(exits_never_denied(&run).is_err());
}
