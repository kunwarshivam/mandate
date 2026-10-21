//! Hand-calculated cases for the version backtest (E4-4, DEC-1000) and its report and record
//! (E4-5, DEC-1001). Each case names the decision item it pins.

mod common;

use common::*;
use mandate_accounting::{AssetClass, Side};
use mandate_canon::{Digest, Value};
use mandate_risk::Purpose;
use mandate_sim::{OrderEnd, OrderKind};
use mandate_version_backtest::{
    DecisionOutcome, RunRecord, VersionDigests, VersionError, VersionObjects,
};
use serde_json::Value as Json;

/// The period count every case runs: 20 to open the liquidity window, then ten more.
const DAYS: usize = 30;

fn rows_with(run: &mandate_version_backtest::VersionRun, outcome: DecisionOutcome) -> usize {
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
        protection.decided_at,
        at(&format!("{}T20:00:00Z", trading_days(DAYS)[20])),
        "placed at the fill bar's regular-session close, 16:00 New York"
    );
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

fn refusal(result: Result<mandate_version_backtest::VersionRun, VersionError>) -> &'static str {
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
    let invalidated_early = vec![mandate_version_backtest::RecordedThesis {
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
    let ahead_of_receipt = vec![mandate_version_backtest::RecordedThesis {
        as_of: at("2026-09-02T12:00:01Z"),
        ..thesis(at("2026-09-02T12:00:00Z"), at("2026-10-30T12:00:00Z"), true)
    }];
    assert_eq!(
        refusal(run(
            &research(),
            &flat_bars(DAYS),
            &daily_outputs(&days, "0.5"),
            &ahead_of_receipt
        )),
        "record_before_its_data"
    );
}

/// The receipt check's edges: a thesis made from earlier data, and one invalidated at the instant
/// it was received, are records that can exist, so the run completes.
#[test]
fn a_thesis_from_earlier_data_or_invalidated_on_receipt_is_accepted() {
    let days = trading_days(DAYS);
    let received = at("2026-09-02T12:00:00Z");
    let expires = at("2026-10-30T12:00:00Z");
    let from_earlier_data = mandate_version_backtest::RecordedThesis {
        as_of: at("2026-09-01T20:00:00Z"),
        ..thesis(received, expires, true)
    };
    let invalidated_on_receipt = mandate_version_backtest::RecordedThesis {
        thesis_id: "th_02".to_owned(),
        invalidated_at: Some(received),
        ..thesis(received, expires, true)
    };
    for theses in [[from_earlier_data], [invalidated_on_receipt]] {
        assert!(
            run(
                &research(),
                &flat_bars(DAYS),
                &daily_outputs(&days, "0.5"),
                &theses
            )
            .is_ok()
        );
    }
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

/// DEC-2245 item 6: a split inside the bars, on either edge too, is applied, not refused.
#[test]
fn a_split_in_range_is_not_refused() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    for date in [days[10], days[0], days[DAYS - 1]] {
        let split = run_with(
            &pinned(),
            &settings(),
            &xyz(),
            &bars,
            &outputs,
            &[],
            &[split_on(date)],
        );
        assert_ne!(
            split.err().map(|e| e.code()),
            Some("corporate_action_in_range"),
            "a split on {date} is applied, not refused (DEC-2245 item 6)"
        );
    }
}

/// DEC-2245 item 6 slice 3: a cash dividend inside the bars, on either edge too, is applied and
/// the run completes.
#[test]
fn a_dividend_in_range_completes_the_run() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    for date in [days[10], days[0], days[DAYS - 1]] {
        run_with(
            &pinned(),
            &settings(),
            &xyz(),
            &bars,
            &outputs,
            &[],
            &[dividend_on(date)],
        )
        .unwrap_or_else(|e| panic!("a dividend on {date}: the run completes: {}", e.code()));
    }
}

/// An action the day before the first bar, or the day after the last, holds nothing the run
/// walks: the run completes.
#[test]
fn an_action_past_either_edge_completes_the_run() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let after_the_last = days[DAYS - 1]
        .next()
        .unwrap_or_else(|e| panic!("{}", e.code()));
    for date in [day("2026-08-31"), after_the_last] {
        run_with(
            &pinned(),
            &settings(),
            &xyz(),
            &bars,
            &outputs,
            &[],
            &[dividend_on(date)],
        )
        .unwrap_or_else(|e| panic!("a dividend on {date}: the run completes: {}", e.code()));
    }
}

/// DEC-1000 item 8: a corporate action outside the bars does not refuse; the run completes, and
/// its configuration names the action (DEC-2245 item 4).
#[test]
fn a_corporate_action_outside_the_bars_completes_the_run() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let outside = run_with(
        &pinned(),
        &settings(),
        &xyz(),
        &bars,
        &outputs,
        &[],
        &[split_on(day("2026-08-14"))],
    );
    outside.unwrap_or_else(|e| panic!("the run completes: {e:?}"));
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
    let numbered: Vec<_> = run
        .fills
        .iter()
        .take(2)
        .map(|fill| fill.fill.order)
        .collect();
    assert_eq!(
        numbered,
        [mandate_sim::OrderRef::new(0), mandate_sim::OrderRef::new(1)],
        "each fill names its own order's run-wide index"
    );
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
    let input = mandate_version_backtest::VersionInput {
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
        mandate_version_backtest::run_version(&input).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(completed.report.inputs, inputs);
    let build = Digest::of(b"build");
    let record = mandate_version_backtest::backtest_run_recorded(&RunRecord {
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
    let error = VersionError::AssetClassNotSupported;
    let refused = mandate_version_backtest::backtest_run_recorded(&RunRecord {
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
        Some(&Value::Str("asset_class_not_supported".to_owned()))
    );
    assert_eq!(
        refused.get("result"),
        Some(&Value::Str("refused".to_owned()))
    );
}

/// Journal spec §6.3 and §11 check 6: each digest a record names is SHA-256 of the canonical
/// bytes of the object `VersionObjects` hands the runner to store, so every reference resolves.
/// The oracle hashes the objects itself, and reads one member of each, so four objects that were
/// one, or empty, would fail it.
#[test]
fn each_digest_is_of_the_object_a_runner_stores() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let mandate = pinned();
    let settings = settings();
    let instrument = xyz();
    let medians = Median(qty("10000000"));
    let theses = [thesis(
        at("2026-09-01T13:00:00Z"),
        at("2026-12-01T00:00:00Z"),
        true,
    )];
    let input = mandate_version_backtest::VersionInput {
        mandate: &mandate,
        settings: &settings,
        instrument: &instrument,
        bars: &bars,
        coverage_start: bars[0].start,
        first_bar_volumes: &medians,
        outputs: &outputs,
        theses: &theses,
        corporate_actions: &[],
    };
    let objects = VersionObjects::of(&input).unwrap_or_else(|e| panic!("{}", e.code()));
    let hashed = |value: &Value| Digest::of(&mandate_canon::to_canonical(value));
    let digests = VersionDigests::of(&input).unwrap_or_else(|e| panic!("{}", e.code()));
    assert_eq!(objects.digests(), digests);
    assert_eq!(hashed(&objects.bars), digests.bars);
    assert_eq!(hashed(&objects.model_outputs), digests.model_outputs);
    assert_eq!(hashed(&objects.theses), digests.theses);
    assert_eq!(hashed(&objects.config), digests.config);
    let rows = objects.bars.get("rows").and_then(Value::as_array);
    assert_eq!(rows.map(<[Value]>::len), Some(DAYS));
    let count = |value: &Value| value.as_array().map_or(0, <[Value]>::len);
    assert_eq!(count(&objects.model_outputs), outputs.len());
    assert_eq!(count(&objects.theses), 1);
    assert!(objects.config.get("fees").is_some(), "{:?}", objects.config);
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

fn set(document: &mut Json, pointer: &str, value: Json) {
    *document
        .pointer_mut(pointer)
        .unwrap_or_else(|| panic!("the document has {pointer}")) = value;
}

/// Mandate spec §5.4: the daily loss resets at each New York midnight. A position of 6 shares that
/// loses 2 a day loses 12 USD a day, below a 0.15% limit of 15 USD, so nothing latches; folded
/// without the day's reset it would lose 108 in all and latch.
#[test]
fn the_daily_loss_resets_at_each_risk_day() {
    let days = trading_days(DAYS);
    let mut document = base("research_equity_pinned");
    set(
        &mut document,
        "/risk/max_daily_loss",
        Json::String("0.0015".to_owned()),
    );
    set(&mut document, "/risk/daily_breach_min_s", Json::from(0));
    set(
        &mut document,
        "/protection/stop_distance",
        Json::String("0.2".to_owned()),
    );
    let mut bars = flat_bars(DAYS);
    for (step, index) in (21..DAYS).enumerate() {
        let close = 98 - 2 * i64::try_from(step).unwrap_or_else(|e| panic!("{e}"));
        let (close, low) = (close.to_string(), (close - 1).to_string());
        bars[index] = daily_bar(days[index], &close, &close, &low, &close, "1000000");
    }
    let run = run(
        &validate(&document),
        &bars,
        &daily_outputs(&days, "0.5"),
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(!run.fills.is_empty());
    assert!(
        run.report.limits_latched.is_empty(),
        "{:?}",
        run.report.limits_latched
    );
    assert_eq!(
        run.report.most_restrictive_mode,
        mandate_domain::AgentMode::Normal
    );
}

/// Mandate spec §5.5's `trim_to_target`: 9 shares bought at 100 fall to 96, the scale rung at
/// 0.2% activates, and the trim sells 2 at the bid, a risk exit the gate allows.
#[test]
fn an_active_trim_rung_sells_down_to_its_target() {
    let days = trading_days(DAYS);
    let mut document = base("research_equity_pinned");
    set(
        &mut document,
        "/risk/scale_action",
        Json::String("trim_to_target".to_owned()),
    );
    set(
        &mut document,
        "/risk/drawdown_ladder",
        serde_json::json!([
            {"at": "0.002", "action": "scale_sizes", "factor": "0.5"},
            {"at": "0.05", "action": "exits_only", "factor": null},
            {"at": "0.06", "action": "flatten_and_pause", "factor": null}
        ]),
    );
    set(
        &mut document,
        "/risk/max_drawdown",
        Json::String("0.06".to_owned()),
    );
    set(
        &mut document,
        "/risk/hysteresis",
        Json::String("0.001".to_owned()),
    );
    set(
        &mut document,
        "/risk/max_daily_loss",
        Json::String("0.5".to_owned()),
    );
    set(
        &mut document,
        "/protection/stop_distance",
        Json::String("0.2".to_owned()),
    );
    set(
        &mut document,
        "/autonomy/rules/0/when/value",
        Json::String("2000".to_owned()),
    );
    let mut bars = flat_bars(DAYS);
    for (index, date) in days.iter().enumerate().skip(22) {
        bars[index] = daily_bar(*date, "96", "97", "95", "96", "1000000");
    }
    let run = run(
        &validate(&document),
        &bars,
        &daily_outputs(&days, "0.8"),
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let trim = run
        .report
        .decisions
        .iter()
        .find(|row| row.purpose == Some(Purpose::RiskExit))
        .unwrap_or_else(|| panic!("a trim was proposed"));
    assert_eq!(trim.outcome, DecisionOutcome::Submitted);
    assert_eq!(trim.qty, Some(qty("2")));
    assert_eq!(trim.limit, Some(price("96")));
    assert_eq!(run.report.risk_exits, 1);
}

/// DEC-1000 item 1.7: a profile whose equity stop-limit cell offers the resting stop-limit protects
/// a mandate with no take-profit by one: stop 95.01, limit 95.01 × (1 − 0.005) = 94.53495.
#[test]
fn a_stop_limit_protects_when_the_profile_offers_one() {
    let days = trading_days(DAYS);
    let mut document = base("research_equity_pinned");
    set(
        &mut document,
        "/protection/take_profit_distance",
        Json::Null,
    );
    set(
        &mut document,
        "/protection/crypto_stop_limit_offset",
        Json::String("0.005".to_owned()),
    );
    let mut settings = settings();
    settings.profile = stop_limit_profile();
    let run = run_with(
        &validate(&document),
        &settings,
        &xyz(),
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let protection = run
        .orders
        .iter()
        .find(|order| order.protective)
        .unwrap_or_else(|| panic!("protected"));
    assert_eq!(
        protection.order.kind,
        OrderKind::StopLimit {
            stop: price("95.01"),
            limit: price("94.53495"),
        }
    );
}

/// Trading spec §9.6's opposite-fill interval, read from the run's own fills: after the stop sells
/// on day 23, the six reopenings inside a ten-day interval are denied, and the first after it opens.
#[test]
fn the_opposite_fill_interval_is_read_from_the_runs_fills() {
    let days = trading_days(DAYS);
    let mut settings = settings();
    settings.gate.opposite_fill_interval_s = 864_000;
    let mut bars = flat_bars(DAYS);
    bars[22] = daily_bar(days[22], "99", "99", "90", "91", "1000000");
    let run = run_with(
        &pinned(),
        &settings,
        &xyz(),
        &bars,
        &daily_outputs(&days, "0.5"),
        &[],
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    let denied = run
        .report
        .decisions
        .iter()
        .filter(|row| row.reason.as_deref() == Some("opposite_fill_interval"))
        .count();
    assert_eq!(denied, 6);
    assert_eq!(
        run.report.decisions.last().map(|row| row.outcome),
        Some(DecisionOutcome::Submitted)
    );
}

/// DEC-1000 item 5's visibility at its edges: a thesis received exactly at the 09:30 decision is
/// seen by it; one expiring or invalidated exactly then is not; a later refused record ends an
/// earlier admission; and a refused record received at the same instant as an admitted one wins
/// (DEC-1140 item 1).
#[test]
fn thesis_visibility_holds_at_its_edges() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let bars = flat_bars(DAYS);
    let decision = |index: usize| at(&format!("{}T13:30:00Z", days[index]));
    let opened = |theses: &[mandate_version_backtest::RecordedThesis]| {
        run(&research(), &bars, &outputs, theses)
            .unwrap_or_else(|e| panic!("the run completes: {}", e.code()))
            .report
            .decisions
            .iter()
            .position(|row| row.outcome == DecisionOutcome::Submitted)
    };
    let late = thesis(decision(25), at("2026-10-30T12:00:00Z"), true);
    assert_eq!(
        opened(&[late]),
        Some(24),
        "received exactly at the day-26 decision"
    );
    let expiring = thesis(at("2026-09-02T12:00:00Z"), decision(20), true);
    assert_eq!(
        opened(&[expiring]),
        None,
        "expired exactly at the first open decision"
    );
    let invalidated = mandate_version_backtest::RecordedThesis {
        invalidated_at: Some(decision(20)),
        ..thesis(at("2026-09-02T12:00:00Z"), at("2026-10-30T12:00:00Z"), true)
    };
    assert_eq!(opened(&[invalidated]), None, "invalidated exactly then");
    let admitted = thesis(at("2026-09-02T12:00:00Z"), at("2026-10-30T12:00:00Z"), true);
    let refused_renewal = mandate_version_backtest::RecordedThesis {
        thesis_id: "th_02".to_owned(),
        ..thesis(
            at("2026-09-03T12:00:00Z"),
            at("2026-10-30T12:00:00Z"),
            false,
        )
    };
    assert_eq!(
        opened(&[admitted, refused_renewal]),
        None,
        "the later refused record ends it"
    );
    let received = at("2026-09-02T12:00:00Z");
    let refused_at_once = mandate_version_backtest::RecordedThesis {
        thesis_id: "th_01".to_owned(),
        ..thesis(received, at("2026-10-30T12:00:00Z"), false)
    };
    let admitted_at_once = mandate_version_backtest::RecordedThesis {
        thesis_id: "th_02".to_owned(),
        ..thesis(received, at("2026-10-30T12:00:00Z"), true)
    };
    assert_eq!(
        opened(&[admitted_at_once, refused_at_once]),
        None,
        "at one receive time the refused record wins, whatever the ids"
    );
}

/// DEC-1000 item 11: a classification made after the last decision (17:00 New York on the last
/// day) is not seen by any; with none before them,
/// the instrument is unclassified and every opening is denied.
#[test]
fn a_classification_made_after_the_decision_is_not_seen() {
    let days = trading_days(DAYS);
    let mut instrument = xyz();
    instrument.etp_classifications = vec![mandate_version_backtest::EtpClassification {
        at: at(&format!("{}T21:00:00Z", days[DAYS - 1])),
        class: mandate_risk::EtpClass::Plain,
    }];
    let run = run_with(
        &pinned(),
        &settings(),
        &instrument,
        &flat_bars(DAYS),
        &daily_outputs(&days, "0.5"),
        &[],
        &[],
    )
    .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(run.orders.is_empty());
}

/// The eligibility floor reads each period's dollar volume: 9,999 shares a day at 100 is 999,900,
/// below the 1,000,000 floor, so no opening is ever allowed.
#[test]
fn a_thin_instrument_never_opens() {
    let days = trading_days(DAYS);
    let bars: Vec<_> = days
        .iter()
        .map(|date| daily_bar(*date, "100", "101", "99", "100", "9999"))
        .collect();
    let run = run(&pinned(), &bars, &daily_outputs(&days, "0.5"), &[])
        .unwrap_or_else(|e| panic!("the run completes: {}", e.code()));
    assert!(run.orders.is_empty());
    assert!(
        run.report
            .decisions
            .iter()
            .all(|row| row.reason.as_deref() == Some("below_liquidity_floor"))
    );
}

/// DEC-1000 items 5 and 8: two theses with one id, and a bar outside the regular session, refuse
/// the run.
#[test]
fn a_repeated_thesis_or_an_extended_hours_bar_refuses_the_run() {
    let days = trading_days(DAYS);
    let outputs = daily_outputs(&days, "0.5");
    let twice = [
        thesis(at("2026-09-02T12:00:00Z"), at("2026-10-30T12:00:00Z"), true),
        thesis(at("2026-09-03T12:00:00Z"), at("2026-10-30T12:00:00Z"), true),
    ];
    assert_eq!(
        refusal(run(&research(), &flat_bars(DAYS), &outputs, &twice)),
        "duplicate_thesis"
    );
    let mut bars = flat_bars(DAYS);
    bars[3].session = mandate_sim::Session::AfterHours;
    assert_eq!(
        refusal(run(&pinned(), &bars, &outputs, &[])),
        "bar_outside_regular_session"
    );
}
