//! A dispatch's reads and thinks (agent harness spec v0.2 §6.10 and §6.11; DEC-2203, DEC-2209;
//! backlog E19-13 slice 3b): each read takes its scope from the harness and is filtered again on
//! the cut-off, the byte caps drop the oldest items first and set `truncated`, a refused or failed
//! read returns only `{"error": code}`, and a think has no effect. `news.*` and `filings.*` follow.

mod common;

use common::{CUT_OFF, DAY, Fixture, at, bar, bars, scope};
use mandate_canon::Value;
use mandate_research_run::agentic::{Code, LoopBounds, Tool, ToolCall, ToolCallId, ToolOutcome};
use mandate_research_run::{Dispatch, DispatchScope, Dispatched, ResearchRunError, dispatch};
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;

const TURN: u32 = 2;
const AAPL_BARS: &str = r#"{"count":260,"symbol":"AAPL"}"#;
const FAILED: (ToolOutcome, Option<Code>) = (ToolOutcome::Failed, Some(Code::Unavailable));
const OK: (ToolOutcome, Option<Code>) = (ToolOutcome::Ok, None);

fn call(index: u32, tool: Tool, arguments: &str) -> ToolCall {
    let arguments = arguments.to_owned();
    ToolCall {
        index,
        tool,
        arguments,
    }
}

fn try_run(
    fixture: &Fixture,
    calls: &[ToolCall],
    bounds: LoopBounds,
    left: u32,
) -> Result<Dispatch, ResearchRunError> {
    let scope = scope();
    let within = DispatchScope {
        scope: &scope,
        bounds: &bounds,
        result_bytes_left: left,
    };
    dispatch(TURN, calls, within, fixture.readers())
}

fn run(fixture: &Fixture, calls: &[ToolCall], bounds: LoopBounds, left: u32) -> Dispatch {
    try_run(fixture, calls, bounds, left).unwrap_or_else(|e| panic!("{e:?}"))
}

fn one(fixture: &Fixture, tool: Tool, arguments: &str) -> Dispatched {
    let mut dispatched = run(
        fixture,
        &[call(0, tool, arguments)],
        LoopBounds::v0_2(),
        96_000,
    );
    assert_eq!(dispatched.results.len(), 1, "one call, one result");
    dispatched.results.remove(0)
}

fn with_bars(rows: Vec<mandate_research_run::Bar>) -> Fixture {
    Fixture {
        bars: rows,
        ..Fixture::default()
    }
}

fn verdict(result: &Dispatched) -> (ToolOutcome, Option<Code>) {
    (result.outcome, result.code)
}

/// The `item_id` of every item the result shows the model, in its order.
fn shown(result: &Dispatched) -> Vec<String> {
    let value = mandate_canon::parse(&result.content);
    let value = value.unwrap_or_else(|e| panic!("canonical JSON: {e:?}"));
    let items = value.get("items").and_then(Value::as_array);
    let items = items.unwrap_or_else(|| panic!("a read's result has `items`"));
    let id = |item: &Value| {
        item.get("item_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    items
        .iter()
        .map(|item| id(item).unwrap_or_default())
        .collect()
}

/// The `bar:AAPL:<secs>` ids of the `count` sessions before the cut-off, newest first.
fn newest(count: i64) -> Vec<String> {
    (1..=count)
        .map(|n| format!("bar:AAPL:{}", CUT_OFF - DAY * n))
        .collect()
}

#[test]
#[ignore = "pending E19-13"]
fn bars_are_newest_first_up_to_count_and_never_after_the_cut_off() {
    let mut rows = bars("AAPL", 30);
    rows.push(bar("AAPL", CUT_OFF + 60, "999"));
    rows.push(bar("AAPL", CUT_OFF - DAY, "101"));
    let result = one(
        &with_bars(rows),
        Tool::MarketBars,
        r#"{"count":5,"symbol":"AAPL"}"#,
    );
    assert_eq!(verdict(&result), OK);
    assert_eq!(
        shown(&result),
        newest(5),
        "newest first, none after the cut-off, one per session"
    );
    assert!(
        !result.truncated,
        "the model's own count is not a cap (rule 179)"
    );
    assert!(
        !result.untrusted,
        "bars are platform data, not third-party text"
    );
    assert_eq!(result.items.len(), 5);
    assert!(
        result
            .items
            .iter()
            .all(|item| item.knowledge_at() <= at(CUT_OFF))
    );
}

#[test]
#[ignore = "pending E19-13"]
fn max_result_bytes_drops_the_oldest_bars_first() {
    let bounds = LoopBounds {
        max_result_bytes: 2_000,
        ..LoopBounds::v0_2()
    };
    let calls = [call(0, Tool::MarketBars, AAPL_BARS)];
    let dispatched = run(&with_bars(bars("AAPL", 30)), &calls, bounds, 96_000);
    let result = dispatched
        .results
        .first()
        .unwrap_or_else(|| panic!("one result"));
    assert!(
        result.content.len() <= 2_000,
        "{} bytes",
        result.content.len()
    );
    assert!(result.truncated);
    let kept = shown(result);
    assert!(!kept.is_empty() && kept.len() < 30, "{kept:?}");
    let count = i64::try_from(kept.len()).unwrap_or(i64::MAX);
    assert_eq!(kept, newest(count), "the newest are kept, newest first");
    assert_eq!(
        result.items.len(),
        kept.len(),
        "only what the model is shown is observed"
    );
}

#[test]
#[ignore = "pending E19-13"]
fn each_read_is_cut_to_what_the_run_has_left_and_lowers_it() {
    let calls = [
        call(0, Tool::MarketBars, AAPL_BARS),
        call(1, Tool::MarketBars, AAPL_BARS),
    ];
    let dispatched = run(
        &with_bars(bars("AAPL", 30)),
        &calls,
        LoopBounds::v0_2(),
        1_500,
    );
    let [first, second] = dispatched.results.as_slice() else {
        panic!("two results: {:?}", dispatched.results);
    };
    assert!(first.content.len() <= 1_500 && first.truncated);
    assert!(
        shown(second).is_empty(),
        "nothing is left for the second read"
    );
    assert!(
        second.truncated,
        "at zero a read is empty and truncated (§6.11)"
    );
    let spent = u32::try_from(first.content.len() + second.content.len()).unwrap_or(u32::MAX);
    assert_eq!(
        dispatched.result_bytes_left,
        1_500_u32.saturating_sub(spent)
    );
}

#[test]
#[ignore = "pending E19-13"]
fn a_failed_read_returns_only_its_error() {
    let fixture = Fixture {
        failing: true,
        ..Fixture::default()
    };
    let calls = [call(3, Tool::MarketBars, r#"{"count":20,"symbol":"AAPL"}"#)];
    let dispatched = run(&fixture, &calls, LoopBounds::v0_2(), 96_000);
    let result = dispatched
        .results
        .first()
        .unwrap_or_else(|| panic!("one result"));
    assert_eq!(
        result.call,
        ToolCallId {
            turn: TURN,
            index: 3
        }
    );
    assert_eq!(verdict(result), FAILED);
    assert_eq!(result.content, br#"{"error":"unavailable"}"#.to_vec());
    assert!(
        result.items.is_empty() && !result.truncated && !result.untrusted,
        "rule 179"
    );
    assert_eq!(
        dispatched.result_bytes_left,
        96_000 - 23,
        "an error counts as the read's result"
    );
}

#[test]
#[ignore = "pending E19-13"]
fn bars_arguments_outside_the_pinned_schema_read_nothing() {
    let fixture = Fixture {
        bars: bars("AAPL", 5),
        ..Fixture::default()
    };
    for arguments in [
        r#"{"count":0,"symbol":"AAPL"}"#,
        r#"{"count":261,"symbol":"AAPL"}"#,
        "{",
    ] {
        let result = one(&fixture, Tool::MarketBars, arguments);
        assert_eq!(verdict(&result), FAILED, "{arguments}");
        assert!(result.items.is_empty(), "{arguments}");
    }
    assert_eq!(
        fixture.calls.get(),
        0,
        "no reader is asked for what the schema forbids"
    );
}

#[test]
#[ignore = "pending E19-13"]
fn a_read_outside_the_data_universe_is_refused_whole() {
    let fixture = Fixture {
        bars: bars("TSLA", 5),
        ..Fixture::default()
    };
    let result = one(&fixture, Tool::MarketBars, r#"{"count":5,"symbol":"TSLA"}"#);
    assert_eq!(
        verdict(&result),
        (ToolOutcome::Refused, Some(Code::OutOfUniverse))
    );
    assert_eq!(result.content, br#"{"error":"out_of_universe"}"#.to_vec());
    assert!(result.items.is_empty() && !result.truncated && !result.untrusted);
    assert_eq!(
        fixture.calls.get(),
        0,
        "the core screens first; the worker never reads past it"
    );
}

#[test]
#[ignore = "pending E19-13"]
fn thinks_run_with_an_empty_result_and_read_nothing() {
    let fixture = Fixture::default();
    let calls = [
        call(0, Tool::Note, r#"{"text":"AAPL looks extended"}"#),
        call(1, Tool::Plan, r#"{"steps":["read bars","read news"]}"#),
    ];
    let dispatched = run(&fixture, &calls, LoopBounds::v0_2(), 96_000);
    for (n, result) in (0..).zip(&dispatched.results) {
        assert_eq!((result.call.index, verdict(result)), (n, OK));
        assert_eq!(result.content, b"{}".to_vec());
        assert!(!result.untrusted && !result.truncated && result.items.is_empty());
    }
    assert_eq!(dispatched.results.len(), 2);
    assert_eq!(dispatched.result_bytes_left, 96_000, "a think is no read");
    assert_eq!(fixture.calls.get(), 0);
}

#[test]
#[ignore = "pending E19-13"]
fn an_action_is_never_dispatched() {
    let fixture = Fixture::default();
    let calls = [call(0, Tool::ProposeThesis, "{}")];
    let refused = try_run(&fixture, &calls, LoopBounds::v0_2(), 96_000).err();
    assert!(
        matches!(refused, Some(ResearchRunError::LoopOrder { .. })),
        "{refused:?}"
    );
    assert_eq!(fixture.calls.get(), 0);
}

/// Random bar answers and byte budgets against an oracle that keeps its own count: the result never
/// passes its cap, shows exactly the newest sessions at or before the cut-off, and lowers the run's
/// bytes left by exactly its length.
#[test]
#[ignore = "pending E19-13"]
fn random_reads_hold_the_caps_and_the_cut_off() {
    let strategy = (
        proptest::collection::btree_set(-40_i64..5, 0..40),
        1_u32..=260,
        64_u32..12_000,
        0_u32..16_000,
    );
    let mut runner = TestRunner::deterministic();
    for _ in 0..64 {
        let tree = strategy
            .new_tree(&mut runner)
            .unwrap_or_else(|e| panic!("{e}"));
        let (days, count, cap, left) = tree.current();
        let rows = days
            .iter()
            .map(|day| bar("AAPL", CUT_OFF + DAY * day, "100"))
            .collect();
        let bounds = LoopBounds {
            max_result_bytes: cap,
            ..LoopBounds::v0_2()
        };
        let arguments = format!(r#"{{"count":{count},"symbol":"AAPL"}}"#);
        let dispatched = run(
            &with_bars(rows),
            &[call(0, Tool::MarketBars, &arguments)],
            bounds,
            left,
        );
        let result = dispatched
            .results
            .first()
            .unwrap_or_else(|| panic!("one result"));
        let kept = shown(result);
        let budget = usize::try_from(cap.min(left)).unwrap_or(usize::MAX);
        assert!(
            kept.is_empty() || result.content.len() <= budget,
            "over {budget}"
        );
        let eligible: Vec<String> = days
            .iter()
            .rev()
            .filter(|day| **day <= 0)
            .take(usize::try_from(count).unwrap_or(usize::MAX))
            .map(|day| format!("bar:AAPL:{}", CUT_OFF + DAY * day))
            .collect();
        assert_eq!(
            kept.as_slice(),
            eligible.get(..kept.len()).unwrap_or_default()
        );
        assert_eq!(
            result.truncated,
            kept.len() < eligible.len(),
            "{kept:?} of {eligible:?}"
        );
        let spent = u32::try_from(result.content.len()).unwrap_or(u32::MAX);
        assert_eq!(dispatched.result_bytes_left, left.saturating_sub(spent));
    }
}
