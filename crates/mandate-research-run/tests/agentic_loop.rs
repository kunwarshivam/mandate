//! The agentic loop's pure state (agent harness spec v0.2 §6.9 to §6.11; DEC-2203; backlog E19-13
//! slice 3): which tools each turn offers, how a reply's calls are ordered, screened, and bounded,
//! what a dispatch's results count, how the run ends, and that folding the run's own records
//! rebuilds the same state (HI-3).

use std::collections::{BTreeMap, BTreeSet};

use mandate_gateway::money::Usd6;
use mandate_research_run::agentic::{
    Bound, Code, EndReason, FoldedEvent, LoopBounds, Next, OutputMode, ReplyInput, RunKind,
    RunStart, RunState, Tool, ToolCall, ToolCallId, ToolOutcome, ToolResult,
};
use mandate_research_run::{ItemId, ItemKind, ResearchRunError, Symbol};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const READS_AND_THINKS: [Tool; 13] = [
    Tool::MarketBars,
    Tool::MarketQuote,
    Tool::NewsSearch,
    Tool::NewsGet,
    Tool::FilingsSearch,
    Tool::FilingsGet,
    Tool::MemoryPositions,
    Tool::MemoryTheses,
    Tool::MemoryRuns,
    Tool::RecordsQuery,
    Tool::MandateRead,
    Tool::Note,
    Tool::Plan,
];

fn sym(text: &str) -> Symbol {
    Symbol::new(text).expect("a test symbol")
}

fn item(text: &str) -> ItemId {
    ItemId::new(text).expect("a test item id")
}

fn final_output_menu() -> BTreeSet<Tool> {
    READS_AND_THINKS.into_iter().collect()
}

fn action_menu() -> BTreeSet<Tool> {
    let mut menu = final_output_menu();
    menu.insert(Tool::ProposeThesis);
    menu.insert(Tool::Invalidate);
    menu
}

fn start_with(output: OutputMode, menu: BTreeSet<Tool>, bounds: LoopBounds) -> RunStart {
    RunStart {
        kind: RunKind::Ideate,
        output,
        menu,
        bounds,
        cost_ceiling: Usd6::from_micros(500_000),
        cut_off: UtcNanos::from_parts(1_760_000_000, 0).expect("a test instant"),
        deadline: UtcNanos::from_parts(1_760_000_900, 0).expect("a test instant"),
        allowlist_version: 3,
        data_universe: [sym("AAPL"), sym("MSFT")].into_iter().collect(),
        memory: Vec::new(),
    }
}

fn final_output_run(bounds: LoopBounds) -> RunState {
    RunState::start(start_with(
        OutputMode::FinalOutput,
        final_output_menu(),
        bounds,
    ))
}

fn call(index: u32, tool: Tool, arguments: &str) -> ToolCall {
    ToolCall {
        index,
        tool,
        arguments: arguments.to_owned(),
    }
}

fn reply(turn: u32, cost: u64, tool_calls: Vec<ToolCall>) -> ReplyInput {
    ReplyInput {
        turn,
        ok: true,
        cost: Usd6::from_micros(cost),
        tool_calls,
        final_output: None,
    }
}

fn ok_result(turn: u32, index: u32, bytes: u32, items: Vec<ItemId>) -> ToolResult {
    ToolResult {
        call: ToolCallId { turn, index },
        outcome: ToolOutcome::Ok,
        code: None,
        bytes,
        truncated: false,
        untrusted: true,
        items,
    }
}

fn failed_result(turn: u32, index: u32) -> ToolResult {
    ToolResult {
        call: ToolCallId { turn, index },
        outcome: ToolOutcome::Failed,
        code: Some(Code::Unavailable),
        bytes: 25,
        truncated: false,
        untrusted: false,
        items: Vec::new(),
    }
}

fn refusals(state: &[(ToolCallId, Code)]) -> BTreeMap<u32, Code> {
    state.iter().map(|(id, code)| (id.index, *code)).collect()
}

fn indexes(calls: &[ToolCall]) -> Vec<u32> {
    calls.iter().map(|call| call.index).collect()
}

fn bars(symbol: &str) -> String {
    format!(r#"{{"count":20,"symbol":"{symbol}"}}"#)
}

#[test]
fn turn_one_offers_the_menu_the_run_s_ceiling_and_every_bound_left() {
    let state = final_output_run(LoopBounds::v0_2());
    let plan = state.turn(600_000).expect("turn 1");
    assert_eq!(plan.turn, 1);
    assert_eq!(plan.tools, final_output_menu());
    assert_eq!(plan.spend_ceiling, Usd6::from_micros(500_000));
    assert_eq!(
        plan.time_left_ms, 600_000,
        "the risk clock's time left, as given"
    );
    assert_eq!(plan.turns_left, 8, "this turn included");
    assert_eq!(plan.tool_calls_left, 24);
}

#[test]
fn each_turn_s_ceiling_is_the_run_s_less_what_earlier_turns_cost() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .on_reply(&reply(
            1,
            120_000,
            vec![call(0, Tool::Note, r#"{"text":"a"}"#)],
        ))
        .expect("turn 1");
    state
        .on_results(&[ok_result(1, 0, 4, Vec::new())])
        .expect("its result");
    let plan = state.turn(1).expect("turn 2");
    assert_eq!(plan.turn, 2);
    assert_eq!(plan.spend_ceiling, Usd6::from_micros(380_000));
    assert_eq!(plan.turns_left, 7);
    assert_eq!(plan.tool_calls_left, 23);
}

#[test]
fn a_ceiling_already_spent_offers_a_zero_ceiling_never_a_wrapped_one() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .on_reply(&reply(
            1,
            900_000,
            vec![call(0, Tool::Note, r#"{"text":"a"}"#)],
        ))
        .expect("turn 1");
    state
        .on_results(&[ok_result(1, 0, 4, Vec::new())])
        .expect("its result");
    assert_eq!(state.turn(1).expect("turn 2").spend_ceiling, Usd6::ZERO);
}

#[test]
fn a_reply_with_no_tool_call_ends_the_run_done_and_commits_nothing() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let step = state
        .on_reply(&reply(1, 10, Vec::new()))
        .expect("the final reply");
    assert_eq!(step.ends, Some(EndReason::Done(None)));
    assert!(step.ordered.is_empty() && step.refused.is_empty() && step.dispatch.is_empty());
    assert!(state.turn(1).is_err(), "a stopped run offers no turn");
    let plan = state.end(EndReason::Done(None)).expect("the end");
    assert_eq!(plan.reason, EndReason::Done(None));
    assert!(plan.commit.is_empty() && plan.discarded.is_empty());
    assert!(
        matches!(
            state.end(EndReason::Done(None)),
            Err(ResearchRunError::LoopOrder { .. })
        ),
        "a run ends once"
    );
}

#[test]
fn an_error_reply_ends_the_run_with_nothing_ordered_or_dispatched() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let mut failed = reply(1, 0, vec![call(0, Tool::MarketBars, &bars("AAPL"))]);
    failed.ok = false;
    let step = state.on_reply(&failed).expect("the failed reply");
    assert_eq!(step.ends, Some(EndReason::Error));
    assert!(step.dispatch.is_empty() && step.refused.is_empty());
}

#[test]
fn reads_and_thinks_are_dispatched_in_reply_order_with_nothing_refused() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::Note, r#"{"text":"look at bars first"}"#),
                call(
                    1,
                    Tool::NewsSearch,
                    r#"{"limit":5,"symbols":["MSFT"],"window_h":24}"#,
                ),
                call(2, Tool::MarketBars, &bars("AAPL")),
                call(3, Tool::MandateRead, "{}"),
            ],
        ))
        .expect("turn 1");
    assert_eq!(indexes(&step.ordered), vec![0, 1, 2, 3]);
    assert_eq!(indexes(&step.dispatch), vec![0, 1, 2, 3]);
    assert!(step.refused.is_empty());
    assert_eq!(step.ends, None);
}

#[test]
fn a_symbol_outside_the_run_s_scope_refuses_the_whole_call_out_of_universe() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("TSLA")),
                call(1, Tool::MarketQuote, r#"{"symbols":["AAPL","GME"]}"#),
                call(
                    2,
                    Tool::FilingsSearch,
                    r#"{"limit":3,"symbols":["MSFT"],"window_d":30}"#,
                ),
                call(3, Tool::MarketBars, &bars("not a symbol")),
            ],
        ))
        .expect("turn 1");
    assert_eq!(
        refusals(&step.refused),
        BTreeMap::from([
            (0, Code::OutOfUniverse),
            (1, Code::OutOfUniverse),
            (3, Code::OutOfUniverse),
        ])
    );
    assert_eq!(indexes(&step.dispatch), vec![2]);
    assert_eq!(step.ends, None);
}

#[test]
fn an_item_the_run_never_received_or_of_the_wrong_kind_is_unknown_item() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .on_seed(&[
            (ItemKind::News, item("news-1")),
            (ItemKind::Filing, item("filing-1")),
        ])
        .expect("the seed");
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::NewsGet, r#"{"item":"news-1"}"#),
                call(1, Tool::NewsGet, r#"{"item":"news-2"}"#),
                call(2, Tool::NewsGet, r#"{"item":"filing-1"}"#),
                call(
                    3,
                    Tool::FilingsGet,
                    r#"{"item":"filing-1","length":100,"offset":0}"#,
                ),
            ],
        ))
        .expect("turn 1");
    assert_eq!(
        refusals(&step.refused),
        BTreeMap::from([(1, Code::UnknownItem), (2, Code::UnknownItem)])
    );
    assert_eq!(indexes(&step.dispatch), vec![0, 3]);
}

#[test]
fn items_a_search_returned_may_be_read_in_a_later_turn() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state.on_seed(&[]).expect("an empty seed");
    state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(
                    0,
                    Tool::NewsSearch,
                    r#"{"limit":5,"symbols":["AAPL"],"window_h":24}"#,
                ),
                call(
                    1,
                    Tool::FilingsSearch,
                    r#"{"limit":3,"symbols":["AAPL"],"window_d":30}"#,
                ),
            ],
        ))
        .expect("turn 1");
    state
        .on_results(&[
            ok_result(1, 0, 300, vec![item("news-9")]),
            ok_result(1, 1, 300, vec![item("filing-9")]),
        ])
        .expect("the searches");
    let step = state
        .on_reply(&reply(
            2,
            10,
            vec![
                call(0, Tool::NewsGet, r#"{"item":"news-9"}"#),
                call(
                    1,
                    Tool::FilingsGet,
                    r#"{"item":"filing-9","length":100,"offset":0}"#,
                ),
                call(
                    2,
                    Tool::FilingsGet,
                    r#"{"item":"news-9","length":100,"offset":0}"#,
                ),
            ],
        ))
        .expect("turn 2");
    assert_eq!(
        refusals(&step.refused),
        BTreeMap::from([(2, Code::UnknownItem)])
    );
    assert_eq!(indexes(&step.dispatch), vec![0, 1]);
}

#[test]
fn a_reply_over_the_tool_calls_left_runs_none_and_ends_the_run_bound() {
    let bounds = LoopBounds {
        max_tool_calls: 3,
        ..LoopBounds::v0_2()
    };
    let mut state = final_output_run(bounds);
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("AAPL")),
                call(1, Tool::MarketBars, &bars("MSFT")),
                call(2, Tool::MandateRead, "{}"),
                call(3, Tool::Note, r#"{"text":"x"}"#),
            ],
        ))
        .expect("turn 1");
    assert_eq!(
        refusals(&step.refused),
        (0..4).map(|index| (index, Code::CallBudget)).collect()
    );
    assert!(step.dispatch.is_empty());
    assert_eq!(step.ends, Some(EndReason::Bound(Bound::MaxToolCalls)));
}

#[test]
fn tool_calls_spent_exactly_offer_no_tool_and_the_final_reply_ends_done_max_tool_calls() {
    let bounds = LoopBounds {
        max_tool_calls: 2,
        ..LoopBounds::v0_2()
    };
    let mut state = final_output_run(bounds);
    state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("AAPL")),
                call(1, Tool::MarketBars, &bars("MSFT")),
            ],
        ))
        .expect("turn 1");
    let next = state
        .on_results(&[
            ok_result(1, 0, 100, Vec::new()),
            ok_result(1, 1, 100, Vec::new()),
        ])
        .expect("the reads");
    assert!(matches!(next, Next::Turn { .. }));
    let plan = state.turn(1).expect("turn 2");
    assert!(
        plan.tools.is_empty(),
        "no call is left, so no tool is offered"
    );
    assert_eq!(plan.tool_calls_left, 0);
    let step = state
        .on_reply(&reply(2, 10, Vec::new()))
        .expect("the final reply");
    assert_eq!(step.ends, Some(EndReason::Done(Some(Bound::MaxToolCalls))));
}

#[test]
fn the_last_turn_of_a_final_output_run_offers_no_tool_and_ends_done_max_turns() {
    let bounds = LoopBounds {
        max_turns: 2,
        ..LoopBounds::v0_2()
    };
    let mut state = final_output_run(bounds);
    state
        .on_reply(&reply(
            1,
            10,
            vec![call(0, Tool::Plan, r#"{"steps":["a"]}"#)],
        ))
        .expect("turn 1");
    state
        .on_results(&[ok_result(1, 0, 10, Vec::new())])
        .expect("its result");
    let plan = state.turn(1).expect("turn 2");
    assert_eq!(plan.turns_left, 1);
    assert!(plan.tools.is_empty());
    let step = state
        .on_reply(&reply(2, 10, Vec::new()))
        .expect("the final reply");
    assert_eq!(step.ends, Some(EndReason::Done(Some(Bound::MaxTurns))));
}

#[test]
fn the_last_turn_of_an_action_tools_run_offers_its_action_tools_only() {
    let bounds = LoopBounds {
        max_turns: 1,
        ..LoopBounds::v0_2()
    };
    let state = RunState::start(start_with(OutputMode::ActionTools, action_menu(), bounds));
    let plan = state.turn(1).expect("turn 1");
    assert_eq!(
        plan.tools,
        [Tool::ProposeThesis, Tool::Invalidate]
            .into_iter()
            .collect()
    );
}

#[test]
fn a_last_turn_that_held_only_actions_ends_done_max_turns_with_no_dispatch() {
    let bounds = LoopBounds {
        max_turns: 1,
        ..LoopBounds::v0_2()
    };
    let mut state = RunState::start(start_with(OutputMode::ActionTools, action_menu(), bounds));
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![call(
                0,
                Tool::Invalidate,
                r#"{"evidence":[],"reason":"r","thesis_id":"t"}"#,
            )],
        ))
        .expect("turn 1");
    assert!(step.dispatch.is_empty());
    assert_eq!(step.ends, Some(EndReason::Done(Some(Bound::MaxTurns))));
}

#[test]
fn a_reply_s_calls_are_taken_invalidations_then_theses_then_reads_and_thinks() {
    let mut state = RunState::start(start_with(
        OutputMode::ActionTools,
        action_menu(),
        LoopBounds::v0_2(),
    ));
    let thesis = r#"{"direction":"long","instrument":"AAPL"}"#;
    let invalidation = r#"{"evidence":[],"reason":"r","thesis_id":"t"}"#;
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("AAPL")),
                call(1, Tool::ProposeThesis, thesis),
                call(2, Tool::Note, r#"{"text":"x"}"#),
                call(3, Tool::Invalidate, invalidation),
            ],
        ))
        .expect("turn 1");
    assert_eq!(indexes(&step.ordered), vec![3, 1, 0, 2]);
    assert_eq!(
        indexes(&step.dispatch),
        vec![0, 2],
        "actions are the core's to mediate"
    );
    assert!(step.refused.is_empty());
}

#[test]
fn a_thesis_past_max_theses_is_refused_and_every_call_without_a_result_is_run_ended() {
    let bounds = LoopBounds {
        max_theses: 1,
        ..LoopBounds::v0_2()
    };
    let mut state = RunState::start(start_with(OutputMode::ActionTools, action_menu(), bounds));
    let thesis = r#"{"direction":"long","instrument":"AAPL"}"#;
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("TSLA")),
                call(1, Tool::ProposeThesis, thesis),
                call(2, Tool::ProposeThesis, thesis),
                call(3, Tool::Note, r#"{"text":"x"}"#),
            ],
        ))
        .expect("turn 1");
    assert_eq!(
        refusals(&step.refused),
        BTreeMap::from([
            (0, Code::RunEnded),
            (2, Code::MaxTheses),
            (3, Code::RunEnded),
        ]),
        "the first thesis is mediated; the read had no result yet, so it is run_ended"
    );
    assert!(step.dispatch.is_empty());
    assert_eq!(step.ends, Some(EndReason::Bound(Bound::MaxTheses)));
}

#[test]
fn a_second_identical_refusal_in_one_reply_ends_the_run_max_repeat_denials() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("TSLA")),
                call(1, Tool::MarketBars, &bars("AAPL")),
                call(2, Tool::MarketBars, &bars("TSLA")),
                call(3, Tool::MarketBars, &bars("GME")),
            ],
        ))
        .expect("turn 1");
    assert_eq!(
        refusals(&step.refused),
        BTreeMap::from([
            (0, Code::OutOfUniverse),
            (1, Code::RunEnded),
            (2, Code::MaxRepeatDenials),
            (3, Code::RunEnded),
        ])
    );
    assert!(step.dispatch.is_empty());
    assert_eq!(step.ends, Some(EndReason::Bound(Bound::MaxRepeatDenials)));
}

#[test]
fn a_refusal_repeated_in_a_later_turn_ends_the_run_max_repeat_denials() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("TSLA")),
                call(1, Tool::Note, r#"{"text":"x"}"#),
            ],
        ))
        .expect("turn 1");
    state
        .on_results(&[ok_result(1, 1, 4, Vec::new())])
        .expect("the note");
    let step = state
        .on_reply(&reply(
            2,
            10,
            vec![call(0, Tool::MarketBars, &bars("TSLA"))],
        ))
        .expect("turn 2");
    assert_eq!(
        refusals(&step.refused),
        BTreeMap::from([(0, Code::MaxRepeatDenials)])
    );
    assert_eq!(step.ends, Some(EndReason::Bound(Bound::MaxRepeatDenials)));
}

#[test]
fn different_arguments_or_a_different_tool_are_not_a_repeated_refusal() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("TSLA")),
                call(1, Tool::MarketBars, &bars("GME")),
                call(2, Tool::MarketQuote, r#"{"symbols":["TSLA"]}"#),
            ],
        ))
        .expect("turn 1");
    assert_eq!(step.ends, None);
    assert_eq!(
        refusals(&step.refused),
        (0..3).map(|index| (index, Code::OutOfUniverse)).collect()
    );
}

#[test]
fn a_failed_read_repeated_at_dispatch_ends_the_run_after_its_batch() {
    let mut state = final_output_run(LoopBounds::v0_2());
    for turn in 1..=2 {
        state
            .on_reply(&reply(
                turn,
                10,
                vec![call(0, Tool::MarketBars, &bars("AAPL"))],
            ))
            .expect("the turn");
        let next = state
            .on_results(&[failed_result(turn, 0)])
            .expect("its failure");
        if turn == 1 {
            assert!(matches!(next, Next::Turn { .. }), "one failure goes on");
        } else {
            assert_eq!(next, Next::End(EndReason::Bound(Bound::MaxRepeatDenials)));
        }
    }
}

#[test]
fn only_read_results_lower_the_run_s_result_bytes() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("AAPL")),
                call(1, Tool::Note, r#"{"text":"x"}"#),
            ],
        ))
        .expect("turn 1");
    let next = state
        .on_results(&[
            ok_result(1, 0, 4_000, Vec::new()),
            ok_result(1, 1, 50, Vec::new()),
        ])
        .expect("the results");
    assert_eq!(
        next,
        Next::Turn {
            result_bytes_left: 92_000
        }
    );
}

#[test]
fn results_that_are_not_the_dispatch_s_calls_in_order_are_refused() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("AAPL")),
                call(1, Tool::Note, r#"{"text":"x"}"#),
            ],
        ))
        .expect("turn 1");
    for wrong in [
        vec![
            ok_result(1, 1, 4, Vec::new()),
            ok_result(1, 0, 4, Vec::new()),
        ],
        vec![ok_result(1, 0, 4, Vec::new())],
        vec![
            ok_result(2, 0, 4, Vec::new()),
            ok_result(2, 1, 4, Vec::new()),
        ],
    ] {
        assert!(matches!(
            state.clone().on_results(&wrong),
            Err(ResearchRunError::LoopOrder { .. })
        ));
    }
}

#[test]
fn a_reply_for_a_turn_not_asked_for_is_refused() {
    let mut state = final_output_run(LoopBounds::v0_2());
    assert!(matches!(
        state.on_reply(&reply(2, 10, Vec::new())),
        Err(ResearchRunError::LoopOrder { .. })
    ));
    state
        .on_reply(&reply(
            1,
            10,
            vec![call(0, Tool::MarketBars, &bars("AAPL"))],
        ))
        .expect("turn 1");
    assert!(
        matches!(
            state.on_reply(&reply(2, 10, Vec::new())),
            Err(ResearchRunError::LoopOrder { .. })
        ),
        "turn 2 waits for turn 1's dispatch"
    );
    assert!(state.turn(1).is_err(), "no turn while a dispatch is out");
}

#[test]
fn a_tool_off_the_turn_s_offer_ends_the_run_error_and_runs_nothing() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("AAPL")),
                call(1, Tool::ProposeThesis, r#"{"instrument":"AAPL"}"#),
            ],
        ))
        .expect("turn 1");
    assert_eq!(step.ends, Some(EndReason::Error));
    assert!(step.dispatch.is_empty());
    assert_eq!(
        refusals(&step.refused),
        BTreeMap::from([(0, Code::RunEnded), (1, Code::RunEnded)]),
        "every call still has exactly one result"
    );
}

#[test]
fn more_calls_than_a_turn_may_hold_end_the_run_error() {
    let bounds = LoopBounds {
        max_tool_calls_per_turn: 1,
        ..LoopBounds::v0_2()
    };
    let mut state = final_output_run(bounds);
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("AAPL")),
                call(1, Tool::MandateRead, "{}"),
            ],
        ))
        .expect("turn 1");
    assert_eq!(step.ends, Some(EndReason::Error));
    assert!(step.dispatch.is_empty());
}

#[test]
fn a_run_ended_early_discards_nothing_it_never_held() {
    let mut state = final_output_run(LoopBounds::v0_2());
    let plan = state.end(EndReason::Deadline).expect("a cancelled run");
    assert_eq!(plan.reason, EndReason::Deadline);
    assert!(plan.commit.is_empty() && plan.discarded.is_empty());
}

/// One step a fuzzed run takes: a reply of these calls, then, for each dispatched read, ok with
/// these bytes or failed.
#[derive(Debug, Clone)]
struct FuzzTurn {
    calls: Vec<(Tool, String)>,
    outcomes: Vec<Option<u32>>,
}

fn fuzz_args(tool: Tool) -> BoxedStrategy<String> {
    let symbols = prop::sample::select(vec!["AAPL", "MSFT", "TSLA"]);
    let items = prop::sample::select(vec!["news-1", "filing-1", "news-x"]);
    match tool {
        Tool::MarketBars => symbols.prop_map(bars).boxed(),
        Tool::MarketQuote | Tool::NewsSearch | Tool::FilingsSearch => symbols
            .prop_map(|s| format!(r#"{{"limit":3,"symbols":["{s}"],"window_d":5,"window_h":5}}"#))
            .boxed(),
        Tool::NewsGet => items.prop_map(|i| format!(r#"{{"item":"{i}"}}"#)).boxed(),
        Tool::FilingsGet => items
            .prop_map(|i| format!(r#"{{"item":"{i}","length":10,"offset":0}}"#))
            .boxed(),
        _ => Just("{}".to_owned()).boxed(),
    }
}

fn fuzz_turn() -> impl Strategy<Value = FuzzTurn> {
    prop::collection::vec(prop::sample::select(READS_AND_THINKS.to_vec()), 0..=5)
        .prop_flat_map(|tools| {
            let args: Vec<_> = tools.iter().map(|tool| fuzz_args(*tool)).collect();
            (
                Just(tools),
                args,
                prop::collection::vec(prop::option::of(0u32..30_000), 5),
            )
        })
        .prop_map(|(tools, args, outcomes)| FuzzTurn {
            calls: tools.into_iter().zip(args).collect(),
            outcomes,
        })
}

/// Drives a run through fuzzed turns, checking each step's invariants against the oracle's own
/// counts, and returns the records it would journal.
fn drive(turns: &[FuzzTurn], bounds: LoopBounds) -> (RunState, Vec<FoldedEvent>) {
    let start = start_with(OutputMode::FinalOutput, final_output_menu(), bounds);
    let mut state = RunState::start(start.clone());
    let seed = vec![
        (ItemKind::News, item("news-1")),
        (ItemKind::Filing, item("filing-1")),
    ];
    state.on_seed(&seed).expect("the seed");
    let mut records = vec![FoldedEvent::Started(start), FoldedEvent::Seeded(seed)];
    let mut calls_made: u32 = 0;
    let mut read_bytes: u32 = 0;
    for (k, fuzz) in turns.iter().enumerate() {
        let turn = u32::try_from(k).expect("few turns") + 1;
        let Ok(plan) = state.turn(1_000) else {
            break;
        };
        assert_eq!(plan.turn, turn);
        assert!(turn <= bounds.max_turns, "never a turn past max_turns");
        assert_eq!(plan.tool_calls_left, bounds.max_tool_calls - calls_made);
        let offered: Vec<(Tool, String)> = fuzz
            .calls
            .iter()
            .filter(|(tool, _)| plan.tools.contains(tool))
            .cloned()
            .collect();
        let input = reply(
            turn,
            7,
            offered
                .iter()
                .enumerate()
                .map(|(i, (tool, args))| call(u32::try_from(i).expect("few calls"), *tool, args))
                .collect(),
        );
        let step = state.on_reply(&input).expect("a reply in order");
        records.push(FoldedEvent::Turn(input.clone()));
        let mut answered: BTreeSet<u32> = step.refused.iter().map(|(id, _)| id.index).collect();
        assert_eq!(answered.len(), step.refused.len(), "one refusal per call");
        for (id, code) in &step.refused {
            let refused = input
                .tool_calls
                .iter()
                .find(|c| c.index == id.index)
                .expect("a refused call is the reply's");
            records.push(FoldedEvent::Result(ToolResult {
                call: *id,
                outcome: ToolOutcome::Refused,
                code: Some(*code),
                bytes: 16,
                truncated: false,
                untrusted: false,
                items: Vec::new(),
            }));
            assert_eq!(refused.index, id.index);
        }
        for dispatched in &step.dispatch {
            assert!(
                answered.insert(dispatched.index),
                "a dispatched call was refused too"
            );
        }
        let all: BTreeSet<u32> = input.tool_calls.iter().map(|c| c.index).collect();
        assert_eq!(answered, all, "every call has exactly one result (HI-24)");
        calls_made += u32::try_from(input.tool_calls.len()).expect("few calls");
        if let Some(reason) = step.ends {
            assert!(
                step.dispatch.is_empty(),
                "an ending reply dispatches nothing"
            );
            let plan = state.end(reason).expect("the end");
            records.push(FoldedEvent::Ended(plan.reason));
            return (state, records);
        }
        if step.dispatch.is_empty() {
            continue;
        }
        let results: Vec<ToolResult> = step
            .dispatch
            .iter()
            .zip(&fuzz.outcomes)
            .map(|(c, outcome)| match outcome {
                Some(bytes) => ok_result(turn, c.index, *bytes, Vec::new()),
                None => failed_result(turn, c.index),
            })
            .collect();
        for (c, r) in step.dispatch.iter().zip(&results) {
            if c.tool.class() == mandate_research_run::agentic::ToolClass::Read {
                read_bytes = read_bytes.saturating_add(r.bytes);
            }
        }
        let next = state.on_results(&results).expect("the dispatch's results");
        records.extend(results.into_iter().map(FoldedEvent::Result));
        match next {
            Next::Turn { result_bytes_left } => assert_eq!(
                result_bytes_left,
                bounds.max_run_result_bytes.saturating_sub(read_bytes)
            ),
            Next::End(reason) => {
                let plan = state.end(reason).expect("the end");
                records.push(FoldedEvent::Ended(plan.reason));
                return (state, records);
            }
        }
    }
    (state, records)
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, ..ProptestConfig::default() })]

    #[test]
    fn every_call_has_one_result_and_no_bound_is_ever_passed(
        turns in prop::collection::vec(fuzz_turn(), 1..10),
        max_turns in 1u32..5,
        max_tool_calls in 1u32..10,
    ) {
        let bounds = LoopBounds { max_turns, max_tool_calls, ..LoopBounds::v0_2() };
        let _ = drive(&turns, bounds);
    }

    #[test]
    fn folding_a_run_s_records_rebuilds_the_same_state(
        turns in prop::collection::vec(fuzz_turn(), 1..10),
        max_turns in 1u32..5,
    ) {
        let bounds = LoopBounds { max_turns, ..LoopBounds::v0_2() };
        let (driven, records) = drive(&turns, bounds);
        let Some(FoldedEvent::Started(start)) = records.first() else {
            panic!("a run starts with its start");
        };
        let mut folded = RunState::start(start.clone());
        for record in &records[1..] {
            folded.fold(record).expect("each record folds");
        }
        prop_assert_eq!(folded, driven);
    }
}

#[test]
fn a_seed_after_the_first_turn_or_the_end_is_refused() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .on_reply(&reply(
            1,
            10,
            vec![call(0, Tool::MarketBars, &bars("AAPL"))],
        ))
        .expect("turn 1");
    assert!(matches!(
        state.on_seed(&[(ItemKind::News, item("news-1"))]),
        Err(ResearchRunError::LoopOrder { .. })
    ));
    let mut ended = final_output_run(LoopBounds::v0_2());
    ended.end(EndReason::Mode).expect("a cancelled run");
    assert!(matches!(
        ended.on_seed(&[]),
        Err(ResearchRunError::LoopOrder { .. })
    ));
}

#[test]
fn only_propose_thesis_counts_toward_max_theses() {
    let bounds = LoopBounds {
        max_theses: 1,
        ..LoopBounds::v0_2()
    };
    let mut state = RunState::start(start_with(OutputMode::ActionTools, action_menu(), bounds));
    let invalidation = r#"{"evidence":[],"reason":"r","thesis_id":"t"}"#;
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::Invalidate, invalidation),
                call(1, Tool::Invalidate, invalidation),
                call(2, Tool::ProposeThesis, r#"{"instrument":"AAPL"}"#),
            ],
        ))
        .expect("turn 1");
    assert_eq!(step.ends, None);
    assert!(step.refused.is_empty());
}

#[test]
fn exit_requests_are_taken_after_invalidations_and_before_theses() {
    let mut menu = action_menu();
    menu.insert(Tool::ProposeExit);
    let mut state = RunState::start(start_with(
        OutputMode::ActionTools,
        menu,
        LoopBounds::v0_2(),
    ));
    let step = state
        .on_reply(&reply(
            1,
            10,
            vec![
                call(0, Tool::ProposeThesis, r#"{"instrument":"AAPL"}"#),
                call(
                    1,
                    Tool::ProposeExit,
                    r#"{"instrument":"MSFT","reason":"r"}"#,
                ),
                call(
                    2,
                    Tool::Invalidate,
                    r#"{"evidence":[],"reason":"r","thesis_id":"t"}"#,
                ),
            ],
        ))
        .expect("turn 1");
    assert_eq!(indexes(&step.ordered), vec![2, 1, 0]);
}

#[test]
fn results_with_no_dispatch_out_or_after_the_end_are_refused() {
    let mut state = final_output_run(LoopBounds::v0_2());
    assert!(matches!(
        state.on_results(&[]),
        Err(ResearchRunError::LoopOrder { .. })
    ));
    state
        .on_reply(&reply(
            1,
            10,
            vec![call(0, Tool::MarketBars, &bars("AAPL"))],
        ))
        .expect("turn 1");
    state.end(EndReason::Deadline).expect("a cancelled run");
    assert!(matches!(
        state.on_results(&[ok_result(1, 0, 10, Vec::new())]),
        Err(ResearchRunError::LoopOrder { .. })
    ));
}

#[test]
fn a_second_start_folded_after_a_turn_or_the_end_is_refused() {
    let start = start_with(
        OutputMode::FinalOutput,
        final_output_menu(),
        LoopBounds::v0_2(),
    );
    let mut fresh = RunState::start(start.clone());
    fresh
        .fold(&FoldedEvent::Started(start.clone()))
        .expect("a start before anything else folds");
    let mut turned = RunState::start(start.clone());
    turned
        .fold(&FoldedEvent::Turn(reply(
            1,
            10,
            vec![call(0, Tool::Note, r#"{"text":"x"}"#)],
        )))
        .expect("turn 1");
    assert!(matches!(
        turned.fold(&FoldedEvent::Started(start.clone())),
        Err(ResearchRunError::LoopOrder { .. })
    ));
    let mut ended = RunState::start(start.clone());
    ended
        .fold(&FoldedEvent::Ended(EndReason::Mode))
        .expect("an end");
    assert!(matches!(
        ended.fold(&FoldedEvent::Started(start)),
        Err(ResearchRunError::LoopOrder { .. })
    ));
}

#[test]
fn a_folded_refusal_must_be_the_one_the_reply_produced() {
    let mut state = final_output_run(LoopBounds::v0_2());
    state
        .fold(&FoldedEvent::Turn(reply(
            1,
            10,
            vec![
                call(0, Tool::MarketBars, &bars("TSLA")),
                call(1, Tool::Note, r#"{"text":"x"}"#),
            ],
        )))
        .expect("turn 1");
    let refusal = |outcome, code| {
        FoldedEvent::Result(ToolResult {
            call: ToolCallId { turn: 1, index: 0 },
            outcome,
            code: Some(code),
            bytes: 16,
            truncated: false,
            untrusted: false,
            items: Vec::new(),
        })
    };
    for wrong in [
        refusal(ToolOutcome::Refused, Code::UnknownItem),
        refusal(ToolOutcome::Failed, Code::OutOfUniverse),
    ] {
        assert!(matches!(
            state.clone().fold(&wrong),
            Err(ResearchRunError::LoopOrder { .. })
        ));
    }
    state
        .fold(&refusal(ToolOutcome::Refused, Code::OutOfUniverse))
        .expect("the reply's own refusal");
    assert!(
        matches!(
            state.fold(&FoldedEvent::Result(ok_result(1, 5, 4, Vec::new()))),
            Err(ResearchRunError::LoopOrder { .. })
        ),
        "a result for a call that was never out"
    );
}
