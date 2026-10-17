//! Hand-calculated cases for the version backtest (E4-4, DEC-1000) and its report and record
//! (E4-5, DEC-1001). Each case names the decision item it pins.

mod version_common;

use mandate_accounting::{AssetClass, Side};
use mandate_backtest::{BacktestError, DecisionOutcome, RunRecord, VersionDigests};
use mandate_canon::{Digest, Value};
use mandate_risk::Purpose;
use mandate_sim::{OrderEnd, OrderKind};
use serde_json::Value as Json;
use version_common::*;

/// The period count every case runs: 20 to open the liquidity window, then ten more.
const DAYS: usize = 30;

fn rows_with(run: &mandate_backtest::VersionRun, outcome: DecisionOutcome) -> usize {
    run.report
        .decisions
        .iter()
        .filter(|row| row.outcome == outcome)
        .count()
}

/// DEC-1000 item 7: the eligibility floor's lower median needs 20 closed periods, so every opening
/// before them fails closed, and the first decision after the twentieth close submits.
#[test]
fn openings_fail_closed_until_twenty_periods_have_closed() {
    let days = trading_days(DAYS);
    let run = run(
        &pinned(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let rows = &run.report.decisions;
    assert_eq!(
        rows.len(),
        DAYS - 1,
        "one decision after each period but the last"
    );
    for row in rows.iter().take(19) {
        assert_eq!(row.outcome, DecisionOutcome::GateDeny, "{row:?}");
        assert_eq!(row.reason.as_deref(), Some("below_liquidity_floor"));
        assert_eq!(row.purpose, Some(Purpose::Open));
    }
    let first_order = &rows[19];
    assert_eq!(first_order.outcome, DecisionOutcome::Submitted);
    assert_eq!(first_order.decided_by.as_deref(), Some("rule:routine"));
}

/// DEC-1000 items 1.6 and 1.7: the opening is a day limit order at the ask (the close of 100
/// moved up 1 bps onto the cent: 100.01), and once it fills the executor's protection rests on the
/// whole position as one GTC OCO: stop 100.01 × 0.95 = 95.0095, up to 95.01; take-profit
/// 100.01 × 1.1 = 110.011, up to 110.02 (both sell prices round up, DEC-470 item 3).
#[test]
fn an_opening_fills_then_rests_the_executors_oco() {
    let days = trading_days(DAYS);
    let run = run(
        &pinned(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let opening = &run.orders[0];
    assert!(!opening.protective);
    assert_eq!(opening.order.side, Side::Buy);
    assert_eq!(
        opening.order.kind,
        OrderKind::Limit {
            limit: price("100.01")
        }
    );
    assert_eq!(opening.end, OrderEnd::Filled);
    let protection = &run.orders[1];
    assert!(protection.protective);
    assert_eq!(protection.order.side, Side::Sell);
    assert_eq!(protection.order.qty, opening.order.qty);
    assert_eq!(
        protection.order.kind,
        OrderKind::Oco {
            limit: price("110.02"),
            stop: price("95.01"),
        }
    );
    assert_eq!(protection.order.tif, mandate_sim::TimeInForce::Gtc);
    assert_eq!(
        run.fills.len(),
        1,
        "the protection never triggers on flat bars"
    );
}

/// DEC-1000 item 2: with no rule that answers AUTO, every opening asks, nobody answers, and the
/// ask resolves as `on_timeout: skip`: no order at all.
#[test]
fn an_unanswered_ask_never_becomes_an_order() {
    let days = trading_days(DAYS);
    let run = run(
        &ask_only(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(run.orders.is_empty());
    assert!(run.fills.is_empty());
    assert_eq!(
        rows_with(&run, DecisionOutcome::AskUnanswered),
        DAYS - 1 - 19
    );
    let ask = run
        .report
        .decisions
        .iter()
        .find(|row| row.outcome == DecisionOutcome::AskUnanswered)
        .unwrap_or_else(|| panic!("an ask row"));
    assert_eq!(ask.decided_by.as_deref(), Some("default"));
}

/// DEC-1000 item 3: an output received one minute after the 09:30 decision is not seen by it, and
/// by the next morning's decision it is past both models' ages, so no decision ever has a fresh
/// output and nothing is ordered. The same outputs received at 09:21 open a position.
#[test]
fn an_output_received_after_the_decision_is_never_used() {
    let days = trading_days(DAYS);
    let late: Vec<_> = daily_outputs(&days, "0.5")
        .into_iter()
        .map(|mut recorded| {
            recorded.received_at = at(&format!("{}T13:31:00Z", recorded.output.as_of.date()));
            recorded
        })
        .collect();
    let run = run(&pinned(), &flat_bars(DAYS), &late, &[])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(run.orders.is_empty());
    for row in run.report.decisions.iter().skip(19) {
        assert_eq!(row.outcome, DecisionOutcome::BuilderHold, "{row:?}");
        assert_eq!(row.reason.as_deref(), Some("no_fresh_outputs"));
        assert_eq!(row.outputs_used, 0);
    }
}

fn refusal(result: Result<mandate_backtest::VersionRun, BacktestError>) -> &'static str {
    match result {
        Ok(_) => panic!("the run should refuse"),
        Err(error) => error.code(),
    }
}

/// DEC-1000 item 3: a record whose `as_of` is after its own receive time cannot exist.
#[test]
fn a_record_received_before_its_data_refuses_the_run() {
    let days = trading_days(DAYS);
    let mut outputs = daily_outputs(&days, "0.5");
    outputs[3].received_at = at(&format!("{}T13:19:00Z", days[1]));
    assert_eq!(
        refusal(run(&pinned(), &flat_bars(DAYS), &outputs, &[])),
        "record_before_its_data"
    );
    let invalidated_early = vec![mandate_backtest::RecordedThesis {
        invalidated_at: Some(at("2026-09-01T12:00:00Z")),
        ..thesis(at("2026-09-02T12:00:00Z"), at("2026-10-30T12:00:00Z"), true)
    }];
    assert_eq!(
        refusal(run(
            &research(),
            &flat_bars(DAYS),
            &daily_outputs(&days, "0.5"),
            &invalidated_early
        )),
        "record_before_its_data"
    );
}

/// DEC-1000 item 6: an output from a model version the mandate does not pin refuses the run, as
/// the shell refuses it live; so does one for another instrument.
#[test]
fn an_unpinned_or_foreign_output_refuses_the_run() {
    let days = trading_days(DAYS);
    let mut unpinned = daily_outputs(&days, "0.5");
    unpinned[0].output.model_version =
        mandate_builder::ModelVersion::parse("1.0.1").unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        refusal(run(&pinned(), &flat_bars(DAYS), &unpinned, &[])),
        "output_not_pinned"
    );
    let mut foreign = daily_outputs(&days, "0.5");
    foreign[0].output.instrument = asset("7b4a1c2e-3333-4a2b-9c3d-000000000003");
    assert_eq!(
        refusal(run(&pinned(), &flat_bars(DAYS), &foreign, &[])),
        "record_other_instrument"
    );
}

/// DEC-1000 item 8: a corporate action inside the bars refuses the run; one outside does not.
#[test]
fn a_corporate_action_in_range_refuses_the_run() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let inside = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &bars,
        &outputs,
        &[],
        &[days[10]],
    );
    assert_eq!(refusal(inside), "corporate_action_in_range");
    let outside = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &bars,
        &outputs,
        &[],
        &[day("2026-08-14")],
    );
    assert!(outside.is_ok());
}

/// DEC-1000 item 8: crypto is slice 2's.
#[test]
fn a_crypto_instrument_is_not_supported() {
    let days = trading_days(DAYS);
    let mut instrument = xyz();
    instrument.instrument.asset_class = AssetClass::Crypto;
    let result = run_with(
        &pinned(),
        &settings(),
        &instrument,
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
        &[],
    );
    assert_eq!(refusal(result), "asset_class_not_supported");
}

/// DEC-1000 item 1.7: with no take-profit, Alpaca's equity profile offers no protective form the
/// executor can place on a position (no OCO without a take-profit, no equity stop-limit), and the
/// live executor refuses such an opening; so the run refuses rather than invent one.
#[test]
fn protection_the_executor_cannot_place_refuses_the_run() {
    let days = trading_days(DAYS);
    let mut document = base("research_equity_pinned");
    *document
        .pointer_mut("/protection/take_profit_distance")
        .unwrap_or_else(|| panic!("the base has protection")) = Json::Null;
    let result = run(
        &validate(&document),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
    );
    assert_eq!(refusal(result), "protection_not_placeable");
}

/// DEC-1000 item 5: a research-agent version may open an instrument only while an admitted thesis
/// is active; a refused thesis, or none, keeps it out of the universe.
#[test]
fn a_research_version_opens_only_on_an_active_admitted_thesis() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let none = run(&research(), &bars, &outputs, &[])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(none.orders.is_empty());
    let refused = run(
        &research(),
        &bars,
        &outputs,
        &[thesis(
            at("2026-09-02T12:00:00Z"),
            at("2026-10-30T12:00:00Z"),
            false,
        )],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(refused.orders.is_empty());
    let admitted = run(
        &research(),
        &bars,
        &outputs,
        &[thesis(
            at("2026-09-02T12:00:00Z"),
            at("2026-10-30T12:00:00Z"),
            true,
        )],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(!admitted.orders.is_empty());
    assert!(admitted.report.research_agent);
}

/// DEC-1000 items 1.4 and 1.7 with a stop: a fall to 90 on day 25 triggers the protective stop
/// inside the bar, at a price inside the bar's range, and the position is flat afterwards.
#[test]
fn the_protective_stop_fills_inside_its_bar() {
    let days = trading_days(DAYS);
    let mut bars = flat_bars(DAYS);
    bars[24] = daily_bar(days[24], "99", "99", "90", "91", "1000000");
    let run = run(&pinned(), &bars, &daily_outputs(&days, "0.5"), &[])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let stop_fill = run
        .fills
        .iter()
        .find(|fill| fill.execution.side == Side::Sell)
        .unwrap_or_else(|| panic!("the stop filled"));
    assert_eq!(stop_fill.fill.bar, 24);
    assert!(stop_fill.fill.price >= bars[24].low && stop_fill.fill.price <= bars[24].high);
}

/// DEC-1000 item 1.4 and `AGENTS.md` rule 13: a bearish output proposes a discretionary exit; the
/// run cancels the protection before it, and the exit is submitted, never denied.
#[test]
fn a_discretionary_exit_cancels_the_protection_then_sells() {
    let days = trading_days(DAYS);
    let mut outputs = daily_outputs(&days[..24], "0.5");
    outputs.extend(daily_outputs(&days[24..], "-0.9"));
    let run = run(&pinned(), &flat_bars(DAYS), &outputs, &[])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let exit = run
        .report
        .decisions
        .iter()
        .find(|row| row.purpose == Some(Purpose::DiscretionaryExit))
        .unwrap_or_else(|| panic!("an exit was proposed"));
    assert_eq!(exit.outcome, DecisionOutcome::Submitted, "{exit:?}");
    let protection = run
        .orders
        .iter()
        .find(|order| order.protective)
        .unwrap_or_else(|| panic!("the protection was placed"));
    assert!(
        matches!(protection.end, OrderEnd::Expired { .. }),
        "{protection:?}"
    );
    assert!(
        run.fills
            .iter()
            .any(|fill| fill.execution.side == Side::Sell)
    );
}

/// FR-4.5: the same inputs give the same report bytes, and every input digest moves with its
/// input.
#[test]
fn the_report_is_reproducible_and_names_its_inputs() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let first = run(&pinned(), &bars, &outputs, &[]).unwrap_or_else(|e| panic!("{}", e.code()));
    let second = run(&pinned(), &bars, &outputs, &[]).unwrap_or_else(|e| panic!("{}", e.code()));
    let bytes = first
        .report
        .canonical_bytes()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(
        bytes,
        second
            .report
            .canonical_bytes()
            .unwrap_or_else(|e| panic!("{}", e.code()))
    );
    let other_outputs = daily_outputs(&days, "0.6");
    let third =
        run(&pinned(), &bars, &other_outputs, &[]).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(first.report.inputs.bars, third.report.inputs.bars);
    assert_ne!(
        first.report.inputs.model_outputs,
        third.report.inputs.model_outputs
    );
}

fn keys(value: &Value) -> Vec<String> {
    match value {
        Value::Object(members) => members.keys().map(|key| key.as_str().to_owned()).collect(),
        _ => panic!("an object"),
    }
}

/// DEC-1001 item 2: the report's members are exactly the schema's, with no thesis-quality
/// figure, and a research version carries the mechanics limitation and its placeholder name.
#[test]
fn the_report_has_exactly_the_schemas_members() {
    let days = trading_days(DAYS);
    let research_run = run(
        &research(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[thesis(
            at("2026-09-02T12:00:00Z"),
            at("2026-10-30T12:00:00Z"),
            true,
        )],
    )
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let canonical = research_run
        .report
        .canonical()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(
        keys(&canonical),
        [
            "benchmark",
            "decisions",
            "disclosures",
            "equity",
            "excess_total_return",
            "inputs",
            "instrument",
            "kind",
            "limitations",
            "mandate_version",
            "report_version",
            "research_agent",
            "risk",
            "strategy",
            "subject",
        ]
    );
    let get = |key: &str| canonical.get(key).unwrap_or_else(|| panic!("{key}"));
    assert_eq!(
        get("report_version"),
        &Value::Int(mandate_canon::Int::new(2).unwrap_or_else(|| panic!()))
    );
    let strings = |value: &Value| -> Vec<String> {
        match value {
            Value::Array(items) => items
                .iter()
                .map(|item| match item {
                    Value::Str(text) => text.clone(),
                    _ => panic!("a string"),
                })
                .collect(),
            _ => panic!("an array"),
        }
    };
    assert!(strings(get("limitations")).contains(&"mechanics_not_thesis_quality".to_owned()));
    assert_eq!(
        strings(get("disclosures")),
        [
            "LEGEND-HYPOTHETICAL",
            "NOTE-BACKTEST-LIMITS",
            "NOTE-BACKTEST-MECHANICS"
        ]
    );
    let pinned_run = run(
        &pinned(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
    )
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let pinned_canonical = pinned_run
        .report
        .canonical()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    let pinned_get = |key: &str| {
        pinned_canonical
            .get(key)
            .unwrap_or_else(|| panic!("{key}"))
            .clone()
    };
    assert!(
        !strings(&pinned_get("limitations")).contains(&"mechanics_not_thesis_quality".to_owned())
    );
    assert_eq!(
        strings(&pinned_get("disclosures")),
        ["LEGEND-HYPOTHETICAL", "NOTE-BACKTEST-LIMITS"]
    );
}

/// The committed golden report: the bytes a fixed input produces. They were checked against
/// `schemas/workspace-api/read-models/backtest.schema.json`'s `Report` when written (DEC-1001);
/// any change to them is a change to the report, which raises `report_version`.
#[test]
fn the_golden_report_bytes_are_stable() {
    let days = trading_days(DAYS);
    let run = run(
        &pinned(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
    )
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let bytes = run
        .report
        .canonical_bytes()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    let golden = include_bytes!("golden/version-report.json");
    if golden.is_empty() {
        panic!(
            "the golden file is empty; its bytes are:\n{}",
            String::from_utf8_lossy(&bytes)
        );
    }
    assert_eq!(bytes.as_slice(), golden.as_slice());
}

/// DEC-1001 item 4 and journal spec §9.18: a completed run's record names the report by its
/// digest and has no refusal; a refused run's record has no report and names the code (rule
/// 143). The marker is always `backtest`.
#[test]
fn the_record_names_the_report_or_the_refusal() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let mandate = pinned();
    let settings = settings();
    let instrument = xyz();
    let medians = Median(qty("10000000"));
    let input = mandate_backtest::VersionInput {
        mandate: &mandate,
        settings: &settings,
        instrument: &instrument,
        bars: &bars,
        coverage_start: bars[0].start,
        first_bar_volumes: &medians,
        outputs: &outputs,
        theses: &[],
        corporate_actions: &[],
    };
    let inputs = VersionDigests::of(&input).unwrap_or_else(|e| panic!("{}", e.code()));
    let completed =
        mandate_backtest::run_version(&input).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(completed.report.inputs, inputs);
    let build = Digest::of(b"build");
    let record = mandate_backtest::backtest_run_recorded(&RunRecord {
        backtest_id: "bt_01",
        mandate_version: completed.report.mandate_version,
        inputs,
        code_build: build,
        outcome: Ok(&completed.report),
    })
    .unwrap_or_else(|e| panic!("{}", e.code()));
    let text = |key: &str| match record.get(key) {
        Some(Value::Str(text)) => text.clone(),
        other => panic!("{key}: {other:?}"),
    };
    let report_ref = format!(
        "sha256:{}",
        completed
            .report
            .digest()
            .unwrap_or_else(|e| panic!("{}", e.code()))
            .to_hex()
    );
    assert_eq!(text("report"), report_ref);
    assert_eq!(text("marker"), "backtest");
    assert_eq!(text("result"), "completed");
    assert_eq!(record.get("refusal"), Some(&Value::Null));
    assert_eq!(
        text("data_snapshot"),
        format!("sha256:{}", inputs.bars.to_hex())
    );
    assert_eq!(
        keys(&record),
        [
            "backtest_id",
            "code_build",
            "configuration",
            "data_snapshot",
            "mandate_version",
            "marker",
            "model_outputs",
            "refusal",
            "report",
            "report_version",
            "result",
            "theses",
        ]
    );
    let error = BacktestError::CorporateActionInRange(days[3]);
    let refused = mandate_backtest::backtest_run_recorded(&RunRecord {
        backtest_id: "bt_02",
        mandate_version: completed.report.mandate_version,
        inputs,
        code_build: build,
        outcome: Err(&error),
    })
    .unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(refused.get("report"), Some(&Value::Null));
    assert_eq!(
        refused.get("refusal"),
        Some(&Value::Str("corporate_action_in_range".to_owned()))
    );
    assert_eq!(
        refused.get("result"),
        Some(&Value::Str("refused".to_owned()))
    );
}

/// DEC-1000 item 1.2 and `AGENTS.md` rule 13: a fall to 90 puts the 600 USD position's loss of 60
/// at 0.6% of the 10,000 allocation, the `flatten_and_pause` rung. It confirms over the next
/// regular session, and the next decision is the agent-scoped flatten of the whole position. It is
/// the automated kill switch, which the paused mode does not hold, so it is submitted; openings
/// are then held by the pause.
#[test]
fn a_flatten_rung_sells_the_whole_position_as_the_automated_kill_switch() {
    let days = trading_days(DAYS);
    let mut bars = flat_bars(DAYS);
    for (index, date) in days.iter().enumerate().skip(21) {
        bars[index] = daily_bar(*date, "90", "91", "89", "90", "1000000");
    }
    let run = run(&tight_ladder(), &bars, &daily_outputs(&days, "0.5"), &[])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let flatten = run
        .report
        .decisions
        .iter()
        .find(|row| row.purpose == Some(Purpose::RiskExit))
        .unwrap_or_else(|| panic!("a risk exit was proposed"));
    assert_eq!(flatten.outcome, DecisionOutcome::Submitted, "{flatten:?}");
    assert_eq!(flatten.qty, Some(qty("6")), "the whole sub-ledger position");
    assert_eq!(run.report.risk_exits, 1);
    assert_eq!(
        run.report.most_restrictive_mode,
        mandate_domain::AgentMode::Paused
    );
    assert!(
        run.report
            .limits_latched
            .contains(&"drawdown_ladder[2]".to_owned())
    );
    let after: Vec<_> = run
        .report
        .decisions
        .iter()
        .skip_while(|row| row.purpose != Some(Purpose::RiskExit))
        .skip(1)
        .collect();
    assert!(!after.is_empty());
    for row in after {
        assert_eq!(row.outcome, DecisionOutcome::GateHold, "{row:?}");
        assert_eq!(row.reason.as_deref(), Some("agent_paused"));
    }
    let position_after = run
        .fills
        .iter()
        .fold(0i64, |held, fill| match fill.execution.side {
            Side::Buy => held + 6,
            Side::Sell => held - 6,
        });
    assert_eq!(position_after, 0);
}
