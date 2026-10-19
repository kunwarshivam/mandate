//! A dispatch's reads and thinks (agent harness spec v0.2 §6.10 and §6.11; DEC-2203, DEC-2209;
//! backlog E19-13 slice 3b): each read takes its scope from the harness and is filtered again on
//! the cut-off and the symbol, the byte caps drop the oldest items first and set `truncated`, the
//! result is spotlighted canonical JSON, a refused or failed read returns only `{"error": code}`,
//! and a think has no effect. `news.*` follows in the next pair of PRs.

mod common;

use std::collections::BTreeSet;

use common::{CUT_OFF, DAY, Fixture, at, bar, bars, scope};
use mandate_canon::{Value, to_canonical};
use mandate_research_run::agentic::{Code, LoopBounds, Tool, ToolCall, ToolCallId, ToolOutcome};
use mandate_research_run::{
    Bar, BarReader, Dispatch, DispatchScope, Dispatched, ReadError, Readers, ResearchRunError,
    RetrievedItem, Symbol, dispatch,
};
use mandate_time::UtcNanos;
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
    readers: Readers<'_>,
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
    dispatch(TURN, calls, within, readers)
}

fn run(fixture: &Fixture, calls: &[ToolCall], bounds: LoopBounds, left: u32) -> Dispatch {
    try_run(fixture.readers(), calls, bounds, left).unwrap_or_else(|e| panic!("{e:?}"))
}

fn one(fixture: &Fixture, tool: Tool, arguments: &str) -> Dispatched {
    let calls = [call(0, tool, arguments)];
    let mut dispatched = run(fixture, &calls, LoopBounds::v0_2(), 96_000);
    assert_eq!(dispatched.results.len(), 1, "one call, one result");
    dispatched.results.remove(0)
}

fn with_bars(rows: Vec<Bar>) -> Fixture {
    Fixture {
        bars: rows,
        ..Fixture::default()
    }
}

fn verdict(result: &Dispatched) -> (ToolOutcome, Option<Code>) {
    (result.outcome, result.code)
}

fn parsed(result: &Dispatched) -> Value {
    mandate_canon::parse(&result.content).unwrap_or_else(|e| panic!("JSON: {e:?}"))
}

fn keys(value: &Value) -> Vec<&str> {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("an object: {value:?}"));
    object.keys().map(|key| key.as_str()).collect()
}

/// The `item_id` of every item the result shows the model, in its order.
fn shown(result: &Dispatched) -> Vec<String> {
    let value = parsed(result);
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

/// The ids of the items the result will have observed, in their order.
fn observed(result: &Dispatched) -> Vec<String> {
    let id = |item: &RetrievedItem| item.item_id().map(|id| id.as_str().to_owned());
    result
        .items
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

/// A bar reader that answers every query with all its rows, whatever the symbol or cut-off.
struct Leaky(Vec<Bar>);

impl BarReader for Leaky {
    fn daily_bars(&self, _: &Symbol, _: UtcNanos, _: u32) -> Result<Vec<Bar>, ReadError> {
        Ok(self.0.clone())
    }
}

#[test]
fn bars_are_newest_first_up_to_count_and_never_after_the_cut_off() {
    let mut rows = bars("AAPL", 30);
    rows.push(bar("AAPL", CUT_OFF + 60, "999"));
    let result = one(
        &with_bars(rows),
        Tool::MarketBars,
        r#"{"count":5,"symbol":"AAPL"}"#,
    );
    assert_eq!(verdict(&result), OK);
    assert_eq!(
        shown(&result),
        newest(5),
        "newest first, none after the cut-off"
    );
    assert_eq!(
        observed(&result),
        newest(5),
        "`items` is what was shown, in its order"
    );
    assert!(
        !result.truncated,
        "the model's own count is not a cap (rule 179)"
    );
    assert!(
        !result.untrusted,
        "bars are platform data, not third-party text"
    );
    assert!(
        result
            .items
            .iter()
            .all(|item| item.knowledge_at() <= at(CUT_OFF))
    );
}

#[test]
fn the_result_is_spotlighted_canonical_json() {
    let result = one(&with_bars(bars("AAPL", 3)), Tool::MarketBars, AAPL_BARS);
    let value = parsed(&result);
    assert_eq!(
        result.content,
        to_canonical(&value),
        "canonical: it is its own re-serialisation"
    );
    assert_eq!(
        keys(&value),
        ["items", "truncated"],
        "exactly the two members (DEC-2209 item 4)"
    );
    assert_eq!(value.get("truncated"), Some(&Value::Bool(false)));
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_default();
    assert_eq!(items.len(), 3);
    for item in items {
        assert_eq!(
            keys(item),
            ["item_id", "source", "untrusted"],
            "every item is wrapped as untrusted data, bars included (§6.10)"
        );
        assert_eq!(
            item.get("source").and_then(Value::as_str),
            Some("platform.bars")
        );
        let data = item.get("untrusted").unwrap_or_else(|| panic!("untrusted"));
        let fields = [
            "close",
            "high",
            "knowledge_at",
            "low",
            "open",
            "symbol",
            "volume",
        ];
        assert_eq!(keys(data), fields);
        assert_eq!(data.get("symbol").and_then(Value::as_str), Some("AAPL"));
    }
}

#[test]
fn a_reader_that_leaks_shows_only_the_asked_symbol_at_or_before_the_cut_off() {
    let mut rows = bars("MSFT", 10);
    rows.extend(bars("AAPL", 3));
    rows.push(bar("AAPL", CUT_OFF + DAY, "999"));
    let (fixture, leaky) = (Fixture::default(), Leaky(rows));
    let readers = Readers {
        bars: &leaky,
        ..fixture.readers()
    };
    let calls = [call(0, Tool::MarketBars, AAPL_BARS)];
    let dispatched = try_run(readers, &calls, LoopBounds::v0_2(), 96_000);
    let dispatched = dispatched.unwrap_or_else(|e| panic!("{e:?}"));
    let result = dispatched
        .results
        .first()
        .unwrap_or_else(|| panic!("one result"));
    assert_eq!(
        shown(result),
        newest(3),
        "another symbol's bars and a later bar are dropped"
    );
    assert_eq!(observed(result), newest(3));
}

#[test]
fn two_answers_for_one_instant_are_one_item_the_first_answered() {
    for (first, second) in [("100", "101"), ("101", "100")] {
        let rows = vec![
            bar("AAPL", CUT_OFF - DAY, first),
            bar("AAPL", CUT_OFF - DAY, second),
        ];
        let result = one(&with_bars(rows), Tool::MarketBars, AAPL_BARS);
        assert_eq!(
            shown(&result),
            newest(1),
            "one id, so one item (DEC-2209 item 12)"
        );
        let Some(RetrievedItem::Bar(kept)) = result.items.first() else {
            panic!("one bar: {:?}", result.items);
        };
        assert_eq!(
            kept.close.to_string(),
            first,
            "the first answered is kept, as the seed does"
        );
    }
}

#[test]
fn the_widest_count_shows_the_newest_260_of_a_longer_answer() {
    let bounds = LoopBounds {
        max_result_bytes: 200_000,
        ..LoopBounds::v0_2()
    };
    let calls = [call(0, Tool::MarketBars, AAPL_BARS)];
    let dispatched = run(&with_bars(bars("AAPL", 300)), &calls, bounds, 200_000);
    let result = dispatched
        .results
        .first()
        .unwrap_or_else(|| panic!("one result"));
    assert_eq!(
        shown(result),
        newest(260),
        "the 260-bar cap and the widest count coincide"
    );
    assert!(
        !result.truncated,
        "what the model asked for is what it got (rule 179)"
    );
}

#[test]
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
        observed(result),
        kept,
        "only what the model is shown is observed"
    );
}

#[test]
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
    assert_eq!(
        second.content,
        br#"{"items":[],"truncated":true}"#.to_vec(),
        "§6.11 at zero"
    );
    assert!(second.items.is_empty() && second.truncated);
    let spent = u32::try_from(first.content.len() + second.content.len()).unwrap_or(u32::MAX);
    assert_eq!(
        dispatched.result_bytes_left,
        1_500_u32.saturating_sub(spent)
    );
}

#[test]
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
fn bars_arguments_outside_the_pinned_schema_read_nothing() {
    let fixture = with_bars(bars("AAPL", 5));
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
fn a_read_outside_the_data_universe_is_refused_whole() {
    let fixture = with_bars(bars("TSLA", 5));
    let calls = [call(0, Tool::MarketBars, r#"{"count":5,"symbol":"TSLA"}"#)];
    let dispatched = run(&fixture, &calls, LoopBounds::v0_2(), 96_000);
    let result = dispatched
        .results
        .first()
        .unwrap_or_else(|| panic!("one result"));
    assert_eq!(
        verdict(result),
        (ToolOutcome::Refused, Some(Code::OutOfUniverse))
    );
    assert_eq!(result.content, br#"{"error":"out_of_universe"}"#.to_vec());
    assert!(result.items.is_empty() && !result.truncated && !result.untrusted);
    assert_eq!(
        dispatched.result_bytes_left,
        96_000 - 27,
        "a refusal counts as the read's result"
    );
    assert_eq!(
        fixture.calls.get(),
        0,
        "the core screens first; the worker never reads past it"
    );
}

#[test]
fn reads_not_served_yet_read_nothing_and_answer_unavailable() {
    let fixture = with_bars(bars("AAPL", 5));
    for (tool, arguments) in [
        (Tool::MemoryPositions, "{}"),
        (Tool::MemoryTheses, r#"{"status":"active"}"#),
        (Tool::MemoryRuns, r#"{"limit":5}"#),
        (Tool::RecordsQuery, r#"{"kind":"refusal","limit":5}"#),
        (Tool::MandateRead, "{}"),
    ] {
        let result = one(&fixture, tool, arguments);
        assert_eq!(
            verdict(&result),
            FAILED,
            "{} (DEC-2209 item 11)",
            tool.name()
        );
        assert_eq!(result.content, br#"{"error":"unavailable"}"#.to_vec());
    }
    assert_eq!(fixture.calls.get(), 0);
}

#[test]
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
fn an_action_is_never_dispatched_and_nothing_of_its_reply_is_read() {
    for action in [Tool::ProposeThesis, Tool::Invalidate, Tool::ProposeExit] {
        let fixture = with_bars(bars("AAPL", 5));
        let calls = [call(0, Tool::MarketBars, AAPL_BARS), call(1, action, "{}")];
        let refused = try_run(fixture.readers(), &calls, LoopBounds::v0_2(), 96_000).err();
        let loop_order = matches!(refused, Some(ResearchRunError::LoopOrder { .. }));
        assert!(loop_order, "{}: {refused:?}", action.name());
        assert_eq!(
            fixture.calls.get(),
            0,
            "{}: the read before it is not run",
            action.name()
        );
    }
}

/// The oracle the property checks a bar read with, computed from the inputs alone: the result
/// stays in its budget, shows the newest eligible sessions newest first, observes what it shows,
/// sets `truncated` exactly when it shows fewer than the model's count allows, and lowers the run's
/// bytes left by exactly its length. `None` when it holds, or the first breach.
fn breach(days: &BTreeSet<i64>, count: u32, cap: u32, left: u32, got: &Dispatch) -> Option<String> {
    let [result] = got.results.as_slice() else {
        return Some(format!("{} results for one call", got.results.len()));
    };
    let kept = shown(result);
    let budget = usize::try_from(cap.min(left)).unwrap_or(usize::MAX);
    let eligible: Vec<String> = days
        .iter()
        .rev()
        .filter(|day| **day <= 0)
        .take(usize::try_from(count).unwrap_or(usize::MAX))
        .map(|day| format!("bar:AAPL:{}", CUT_OFF + DAY * day))
        .collect();
    let spent = u32::try_from(result.content.len()).unwrap_or(u32::MAX);
    if !kept.is_empty() && result.content.len() > budget {
        Some(format!(
            "{} bytes over a budget of {budget}",
            result.content.len()
        ))
    } else if kept.as_slice() != eligible.get(..kept.len()).unwrap_or_default() {
        Some(format!("{kept:?} is not the newest of {eligible:?}"))
    } else if observed(result) != kept {
        Some("observes other than it shows".to_owned())
    } else if !wrapped(result) {
        Some("an item is not exactly item_id, source, and its data under untrusted".to_owned())
    } else if result.truncated != (kept.len() < eligible.len()) {
        Some(format!(
            "truncated {} with {} of {}",
            result.truncated,
            kept.len(),
            eligible.len()
        ))
    } else if got.result_bytes_left != left.saturating_sub(spent) {
        Some(format!(
            "{} bytes left, not {}",
            got.result_bytes_left,
            left.saturating_sub(spent)
        ))
    } else {
        None
    }
}

/// Whether every item the result shows is exactly `item_id`, `source`, and its fields under
/// `untrusted` (§6.10, DEC-2209 item 4).
fn wrapped(result: &Dispatched) -> bool {
    let value = parsed(result);
    let items = value
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_default();
    items.iter().all(|item| {
        item.as_object().is_some()
            && keys(item) == ["item_id", "source", "untrusted"]
            && item.get("untrusted").and_then(Value::as_object).is_some()
    })
}

/// The same result with every item's `untrusted` section renamed `data`.
fn unwrapped(got: &Dispatch) -> Dispatch {
    let mut got = got.clone();
    for result in &mut got.results {
        let text = String::from_utf8(result.content.clone()).unwrap_or_default();
        result.content = text.replace("\"untrusted\"", "\"data\"").into_bytes();
    }
    got
}

/// Random bar answers, counts, and byte budgets against [`breach`]'s oracle.
#[test]
fn random_reads_hold_the_caps_and_the_cut_off() {
    let strategy = (
        proptest::collection::btree_set(-320_i64..5, 0..300),
        1_u32..=260,
        64_u32..30_000,
        0_u32..40_000,
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
        let calls = [call(0, Tool::MarketBars, &arguments)];
        let got = run(&with_bars(rows), &calls, bounds, left);
        assert_eq!(
            breach(&days, count, cap, left, &got),
            None,
            "{days:?} {count} {cap} {left}"
        );
    }
}

/// A hand-built result: bars of the given days, with the given `truncated` and bytes left.
fn seeded(days: &[i64], truncated: bool, left: u32) -> Dispatch {
    let bars: Vec<Bar> = days
        .iter()
        .map(|day| bar("AAPL", CUT_OFF + DAY * day, "100"))
        .collect();
    let item = |bar: &Bar| {
        let id = format!("bar:AAPL:{}", bar.knowledge_at.secs());
        format!(r#"{{"item_id":"{id}","source":"platform.bars","untrusted":{{}}}}"#)
    };
    let items: Vec<String> = bars.iter().map(item).collect();
    let content = format!(
        r#"{{"items":[{}],"truncated":{truncated}}}"#,
        items.join(",")
    );
    let result = Dispatched {
        call: ToolCallId {
            turn: TURN,
            index: 0,
        },
        outcome: ToolOutcome::Ok,
        code: None,
        content: content.into_bytes(),
        truncated,
        untrusted: false,
        items: bars.into_iter().map(RetrievedItem::Bar).collect(),
    };
    Dispatch {
        results: vec![result],
        result_bytes_left: left,
    }
}

fn twice(got: &Dispatch) -> Dispatch {
    let results = [got.results.clone(), got.results.clone()].concat();
    Dispatch {
        results,
        result_bytes_left: got.result_bytes_left,
    }
}

#[test]
fn the_oracle_catches_seeded_breaches() {
    let days: BTreeSet<i64> = [-3, -2, -1, 1].into_iter().collect();
    let honest = seeded(&[-1, -2, -3], false, 1_000);
    let length = honest
        .results
        .first()
        .map_or(0, |result| result.content.len());
    let left = 1_000 + u32::try_from(length).unwrap_or(0);
    assert_eq!(
        breach(&days, 3, 30_000, left, &honest),
        None,
        "the honest result passes"
    );
    for (what, got, count, cap) in [
        (
            "after the cut-off",
            seeded(&[1, -1, -2], false, 1_000),
            3,
            30_000,
        ),
        (
            "oldest first",
            seeded(&[-3, -2, -1], false, 1_000),
            3,
            30_000,
        ),
        ("not the newest", seeded(&[-2, -3], true, 1_000), 3, 30_000),
        (
            "truncated unset",
            seeded(&[-1, -2], false, 1_000),
            3,
            30_000,
        ),
        (
            "truncated by the model's count",
            seeded(&[-1, -2], true, 1_000),
            2,
            30_000,
        ),
        (
            "bytes left wrong",
            seeded(&[-1, -2, -3], false, 999),
            3,
            30_000,
        ),
        (
            "over its budget",
            seeded(&[-1, -2, -3], false, 1_000),
            3,
            64,
        ),
        ("two results", twice(&honest), 3, 30_000),
        (
            "its data not under untrusted",
            unwrapped(&honest),
            3,
            30_000,
        ),
    ] {
        assert!(
            breach(&days, count, cap, left, &got).is_some(),
            "the oracle misses: {what}"
        );
    }
}
