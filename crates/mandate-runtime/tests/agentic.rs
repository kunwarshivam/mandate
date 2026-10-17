//! An agentic research run in the core (agent harness spec v0.2 §6.9 to §6.11, journal spec §9.23's
//! run records, DEC-2206): the kind and menu fixed before anything untrusted is read, `RunStarted`
//! before the seed, each turn's record heading its tool calls, reads dispatched only after their
//! records, results journaled before the next turn, a final output judged once, and the run's end.
//! Each case names the rule it holds.
//!
//! Pending until E19-13's loop bodies (lane AH, `mandate_research_run::agentic`) and the core's
//! paths (DEC-2206) land: the core answers an agentic entry with `Unimplemented`.

mod common;

use std::collections::BTreeSet;

use common::research::{
    AAPL, CUT_OFF, Loop, MSFT, RESPONSE_AAPL, RecordedPlane, World, config, record_for, recorded,
};
use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, TestIds, clock, event, ports, text, universe, with_clock,
};
use mandate_canon::{Digest, Value};
use mandate_gateway::money::Usd6;
use mandate_gateway::tools::ToolCall as GatewayToolCall;
use mandate_gateway::{CallRecord, Outcome};
use mandate_research::{AssetId, WorkingUniverse};
use mandate_research_run::agentic::{LoopBounds, OutputMode, Tool, ToolCallId, ToolOutcome};
use mandate_research_run::{Retrieval, retrieve};
use mandate_runtime::{
    ActiveThesis, AgenticConfig, Command, DispatchRequest, DispatchedResult, FoldedEvent, Input,
    Mode, Ports, ResearchInput, ResearchRequest, ResearchStep, RetrievedObservation, RiskClock,
    TurnRequest, WriterEpoch,
};

const PENDING: &str = "E19-13";

fn agentic() -> AgenticConfig {
    AgenticConfig {
        output: OutputMode::FinalOutput,
        bounds: LoopBounds::v0_2(),
        ideate_menu: BTreeSet::from([
            Tool::MarketBars,
            Tool::NewsSearch,
            Tool::NewsGet,
            Tool::FilingsGet,
            Tool::Note,
        ]),
        monitor_menu: BTreeSet::from([Tool::NewsSearch, Tool::NewsGet, Tool::Note]),
        cost_ceiling: Usd6::from_micros(500_000),
    }
}

/// The open world with an agentic `final_output` entry.
fn world() -> World {
    let mut world = World::open();
    let mut entry = config();
    entry.agentic = Some(agentic());
    world.config = Some(entry);
    world
}

fn boot(lp: &mut Loop, ports: &Ports<'_>) {
    lp.shell.run(Input::Started(WriterEpoch(1)), ports);
    lp.shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            1,
            "ReconciliationRun",
            with_clock(&[("result", text("clean"))], CUT_OFF - 120),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    lp.shell.run(Input::Tick(clock(CUT_OFF - 60)), ports);
    assert_eq!(lp.shell.state.effective_mode(), Mode::Normal);
}

/// One research step; a refusal panics with its `Debug` form, which names the stub's report.
fn go(lp: &mut Loop, input: ResearchInput, ports: &Ports<'_>, world: &World) -> ResearchStep {
    lp.step(input, ports, world)
        .unwrap_or_else(|e| panic!("{PENDING}: {e:?}"))
}

fn payload_str<'a>(event: &'a FoldedEvent, member: &str) -> Option<&'a str> {
    event.payload.get(member).and_then(Value::as_str)
}

fn strs(value: Option<&Value>) -> Vec<&str> {
    value
        .and_then(Value::as_array)
        .map(|items| items.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default()
}

/// A run in flight: its loop, its id and cut-off, and the retrieval its seed read.
struct Run {
    lp: Loop,
    run_id: String,
    cut_off: RiskClock,
    retrieval: Retrieval,
}

/// Starts a due run at the cut-off and seeds it from the recorded plane; returns the run and the
/// seed's step, which asks for turn 1.
fn seeded(ports: &Ports<'_>, world: &World) -> (Run, ResearchStep) {
    let mut lp = Loop::new(1);
    boot(&mut lp, ports);
    let started = go(&mut lp, ResearchInput::Poll(clock(CUT_OFF)), ports, world);
    let Some((run_id, cut_off)) = started.requests.iter().find_map(|r| match r {
        ResearchRequest::Start { run_id, cut_off } => Some((run_id.clone(), *cut_off)),
        _ => None,
    }) else {
        panic!("a due agentic run starts: {started:?}");
    };
    let plane = RecordedPlane::load();
    let retrieval = retrieve(&config().plan, &world.scope(cut_off), plane.readers())
        .unwrap_or_else(|e| panic!("{e}"));
    let items: Vec<RetrievedObservation> = retrieval
        .items
        .iter()
        .map(|item| {
            RetrievedObservation::capture(item, cut_off)
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        })
        .collect();
    let seed = go(
        &mut lp,
        ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items,
        },
        ports,
        world,
    );
    (
        Run {
            lp,
            run_id,
            cut_off,
            retrieval,
        },
        seed,
    )
}

fn turn_of(step: &ResearchStep) -> TurnRequest {
    step.requests
        .iter()
        .find_map(|r| match r {
            ResearchRequest::Turn(turn) => Some((**turn).clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the step asks for a turn: {step:?}"))
}

fn dispatch_of(step: &ResearchStep) -> DispatchRequest {
    step.requests
        .iter()
        .find_map(|r| match r {
            ResearchRequest::Dispatch(dispatch) => Some(dispatch.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the step asks for a dispatch: {step:?}"))
}

fn call(index: u32, name: &str, arguments: &str) -> GatewayToolCall {
    GatewayToolCall {
        index,
        name: name.to_owned(),
        arguments: mandate_canon::parse(arguments.as_bytes()).unwrap_or_else(|e| panic!("{e:?}")),
    }
}

/// The gateway's record of `turn`'s call: a final output when `output` is given, else a tool-call
/// reply asking for `calls`.
fn reply(
    run: &Run,
    turn: &TurnRequest,
    world: &World,
    calls: Vec<GatewayToolCall>,
    output: Option<&str>,
) -> CallRecord {
    let mut record = record_for(
        &recorded(output.unwrap_or(RESPONSE_AAPL)),
        &turn.call,
        &run.retrieval,
        world,
        mandate_time::UtcNanos::from_parts(run.cut_off.secs(), 0).unwrap_or_else(|e| panic!("{e}")),
    );
    if output.is_none() {
        record.output = None;
        record.tool_calls = calls;
    }
    record
}

fn answered(run: &mut Run, record: CallRecord, ports: &Ports<'_>, world: &World) -> ResearchStep {
    let input = ResearchInput::Called {
        run_id: run.run_id.clone(),
        at: clock(run.cut_off.secs() + 20),
        record: Box::new(record),
    };
    go(&mut run.lp, input, ports, world)
}

fn read_result(turn: u32, index: u32, content: &str) -> DispatchedResult {
    DispatchedResult {
        call: ToolCallId { turn, index },
        outcome: ToolOutcome::Ok,
        code: None,
        content: Digest::of(content.as_bytes()),
        bytes: u32::try_from(content.len()).unwrap_or(u32::MAX),
        truncated: false,
        untrusted: true,
        items: Vec::new(),
    }
}

fn fixture() -> (TestIds, FixedPlan, mandate_runtime::MandateView) {
    (TestIds, FixedPlan::silent(), universe(&[]))
}

/// HI-4, §6.9 step 1: the kind and menu are fixed from the trigger and folded state before the
/// seed is read, and `RunStarted` is journaled before any observation, naming the run.
#[test]
#[ignore = "pending E19-13"]
fn a_due_agentic_run_journals_run_started_with_its_kind_and_menu_before_its_seed() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let booted = lp.types().len();
    let step = go(&mut lp, ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    let run_id = step
        .requests
        .iter()
        .find_map(|r| match r {
            ResearchRequest::Start { run_id, .. } => Some(run_id.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("the run starts: {step:?}"));
    assert_eq!(
        lp.types()[booted..],
        ["RunStarted"],
        "nothing else before the seed"
    );
    let started = lp.events("RunStarted")[0];
    assert_eq!(payload_str(started, "kind"), Some("ideate"));
    assert_eq!(payload_str(started, "output"), Some("final_output"));
    assert_eq!(
        started.payload.get("trigger"),
        Some(
            &mandate_canon::parse(br#"{"cause":null,"kind":"schedule"}"#)
                .unwrap_or_else(|e| panic!("{e:?}"))
        ),
    );
    assert_eq!(
        strs(started.payload.get("menu")),
        vec![
            "filings.get",
            "market.bars",
            "news.get",
            "news.search",
            "note"
        ],
        "the ideate menu, ascending by bytes (rule 172)"
    );
    assert_eq!(step.correlation_id.as_deref(), Some(run_id.as_str()));
    let universe_ref = payload_str(started, "data_universe_ref").unwrap_or_default();
    assert!(
        step.artifacts
            .iter()
            .any(|bytes| Digest::of(bytes).to_string() == universe_ref),
        "the core hands the shell the data universe it names (DEC-2206 item 4)"
    );
}

/// §6.9's run kinds: `monitor` in `exits_only` while a thesis is active, with its own menu; nothing
/// at all in `exits_only` with no active thesis.
#[test]
#[ignore = "pending E19-13"]
fn exits_only_runs_a_monitor_with_its_menu_only_while_a_thesis_is_active() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    for active in [false, true] {
        let mut world = world();
        if active {
            world.active = vec![ActiveThesis {
                thesis_id: "01ACTIVE".to_owned(),
                lineage_id: "01ACTIVE".to_owned(),
                revision: 0,
                instrument: AssetId::new(AAPL).unwrap_or_else(|e| panic!("{e}")),
                symbol: mandate_research_run::Symbol::new("AAPL").unwrap_or_else(|e| panic!("{e}")),
                expires_at: clock(CUT_OFF + 86_400),
            }];
        }
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        lp.shell
            .fold_one(&event(
                ACCOUNT_STREAM,
                2,
                "AgentModeApplied",
                with_clock(
                    &[
                        ("to", text("exits_only")),
                        ("restriction", text("daily_loss")),
                    ],
                    CUT_OFF - 30,
                ),
            ))
            .unwrap_or_else(|e| panic!("{e}"));
        go(&mut lp, ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
        let started = lp.events("RunStarted");
        if active {
            assert_eq!(started.len(), 1, "a monitor runs");
            assert_eq!(payload_str(started[0], "kind"), Some("monitor"));
            assert_eq!(
                strs(started[0].payload.get("menu")),
                vec!["news.get", "news.search", "note"]
            );
        } else {
            assert!(started.is_empty(), "no kind is due, so nothing runs");
        }
    }
}

/// §6.9: with the universe full only a `monitor` run is due.
#[test]
#[ignore = "pending E19-13"]
fn a_full_universe_runs_a_monitor() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut world = world();
    world.universe = WorkingUniverse::Known {
        instruments: [
            AAPL,
            MSFT,
            "7b4a1c2e-3333-4a2b-9c3d-000000000003",
            "7b4a1c2e-4444-4a2b-9c3d-000000000004",
            "7b4a1c2e-5555-4a2b-9c3d-000000000005",
        ]
        .iter()
        .map(|id| AssetId::new(id).unwrap_or_else(|e| panic!("{e}")))
        .collect(),
        pinned: false,
    };
    world.active = vec![ActiveThesis {
        thesis_id: "01ACTIVE".to_owned(),
        lineage_id: "01ACTIVE".to_owned(),
        revision: 0,
        instrument: AssetId::new(AAPL).unwrap_or_else(|e| panic!("{e}")),
        symbol: mandate_research_run::Symbol::new("AAPL").unwrap_or_else(|e| panic!("{e}")),
        expires_at: clock(CUT_OFF + 86_400),
    }];
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    go(&mut lp, ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    let started = lp.events("RunStarted");
    assert_eq!(started.len(), 1);
    assert_eq!(payload_str(started[0], "kind"), Some("monitor"));
}

/// HI-2, §6.9 steps 2 and 3: the seed's observations are journaled before turn 1 is asked for, and
/// the turn carries the run's menu, its ceiling, and its time left.
#[test]
#[ignore = "pending E19-13"]
fn turn_one_follows_the_seed_s_observations_with_the_menu_and_the_run_s_ceiling() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    assert_eq!(turn.plan.turn, 1);
    assert_eq!(turn.plan.tools, agentic().ideate_menu);
    assert_eq!(turn.plan.spend_ceiling, agentic().cost_ceiling);
    assert_eq!(
        turn.plan.time_left_ms, 900_000,
        "max_run_s from the cut-off"
    );
    assert!(turn.transcript.is_empty());
    let observed = run.lp.events("ObservationRecorded").len();
    assert!(observed > 0);
    assert_eq!(
        turn.call.observations.len(),
        observed,
        "turn 1 names every observation of its seed"
    );
}

/// §6.9 step 4, rules 175 and 176: the turn's record heads one batch with a `ToolCallRecorded` per
/// call in reply order, and the reads are dispatched only after that batch.
#[test]
#[ignore = "pending E19-13"]
fn a_reply_s_record_heads_its_tool_calls_and_its_reads_are_dispatched_after_them() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let before = run.lp.types().len();
    let record = reply(
        &run,
        &turn,
        &world,
        vec![
            call(0, "news.search", r#"{"terms":["AAPL"]}"#),
            call(1, "note", r#"{"text":"check services"}"#),
        ],
        None,
    );
    let step = answered(&mut run, record, &ports, &world);
    let written: Vec<String> = run.lp.types()[before..].to_vec();
    assert_eq!(
        written,
        vec![
            "ModelInvocationRecorded",
            "ToolCallRecorded",
            "ToolCallRecorded"
        ]
    );
    let calls = run.lp.events("ToolCallRecorded");
    let indices: Vec<Option<u64>> = calls
        .iter()
        .map(|c| c.payload.get("index").and_then(Value::as_int))
        .collect();
    assert_eq!(indices, vec![Some(0), Some(1)]);
    assert_eq!(payload_str(calls[0], "class"), Some("read"));
    assert_eq!(payload_str(calls[1], "class"), Some("think"));
    let dispatch = dispatch_of(&step);
    assert_eq!(dispatch.turn, 1);
    assert_eq!(
        dispatch
            .calls
            .iter()
            .map(|c| (c.index, c.tool))
            .collect::<Vec<_>>(),
        vec![(0, Tool::NewsSearch), (1, Tool::Note)],
        "reads and thinks in reply order"
    );
    assert!(
        step.artifacts.len() >= 2,
        "each call's canonical arguments, for `arguments_ref`"
    );
    assert_eq!(step.correlation_id.as_deref(), Some(run.run_id.as_str()));
}

/// §6.9 step 5, HI-24: a dispatch's results are journaled, each after its new items'
/// observations, before the next turn is asked for, and the next turn carries the transcript.
#[test]
#[ignore = "pending E19-13"]
fn a_dispatch_s_results_are_journaled_before_the_next_turn_which_carries_the_transcript() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![call(0, "news.search", r#"{"terms":["AAPL"]}"#)],
        None,
    );
    let response = record.response_ref;
    answered(&mut run, record, &ports, &world);
    let before = run.lp.types().len();
    let next = go(
        &mut run.lp,
        ResearchInput::Dispatched {
            run_id: run.run_id.clone(),
            at: clock(CUT_OFF + 25),
            results: vec![read_result(1, 0, r#"{"items":[]}"#)],
        },
        &ports,
        &world,
    );
    assert_eq!(run.lp.types()[before..], ["ToolResultRecorded"]);
    let result = run.lp.events("ToolResultRecorded")[0];
    assert_eq!(payload_str(result, "result"), Some("ok"));
    assert_eq!(result.payload.get("code"), Some(&Value::Null));
    let turn = turn_of(&next);
    assert_eq!(turn.plan.turn, 2);
    assert_eq!(turn.transcript.len(), 1);
    assert_eq!(Some(turn.transcript[0].response), response);
    assert_eq!(
        turn.transcript[0].results,
        vec![Digest::of(br#"{"items":[]}"#)]
    );
}

/// §6.9 step 4, `final_output`: a reply with no tool call is judged once, as a single call's output
/// is, invalidations first, and the run ends `done` with its summary in the same batch.
#[test]
#[ignore = "pending E19-13"]
fn a_final_output_is_judged_once_and_ends_the_run_done_with_its_summary() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let before = run.lp.types().len();
    let record = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
    let step = answered(&mut run, record, &ports, &world);
    let written: Vec<String> = run.lp.types()[before..].to_vec();
    assert_eq!(
        written.first().map(String::as_str),
        Some("ModelInvocationRecorded")
    );
    assert!(written.iter().any(|t| t == "ThesisProposed"), "{written:?}");
    assert_eq!(
        written[written.len() - 2..],
        ["RunSummaryRecorded", "RunEnded"],
        "the summary and then the end close the batch"
    );
    let ended = run.lp.events("RunEnded")[0];
    assert_eq!(payload_str(ended, "reason"), Some("done"));
    assert_eq!(ended.payload.get("bound"), Some(&Value::Null));
    assert_eq!(strs(ended.payload.get("discarded")), Vec::<&str>::new());
    assert!(
        !step
            .requests
            .iter()
            .any(|r| matches!(r, ResearchRequest::Turn(_) | ResearchRequest::Dispatch(_))),
        "nothing follows the end"
    );
    assert!(run.lp.research.in_flight().is_none());
}

/// §6.9 step 4: a reply asking for more tool calls than the run has left runs none of them; each is
/// refused `call_budget` in the batch, and the run ends `bound`.
#[test]
#[ignore = "pending E19-13"]
fn a_reply_over_the_tool_call_budget_runs_none_and_ends_the_run() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut world = world();
    if let Some(entry) = world.config.as_mut()
        && let Some(loop_) = entry.agentic.as_mut()
    {
        loop_.bounds.max_tool_calls = 1;
    }
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![
            call(0, "news.search", r#"{"terms":["AAPL"]}"#),
            call(1, "news.search", r#"{"terms":["MSFT"]}"#),
        ],
        None,
    );
    let step = answered(&mut run, record, &ports, &world);
    let results = run.lp.events("ToolResultRecorded");
    assert_eq!(results.len(), 2);
    for result in &results {
        assert_eq!(payload_str(result, "result"), Some("refused"));
        assert_eq!(payload_str(result, "code"), Some("call_budget"));
    }
    let ended = run.lp.events("RunEnded");
    assert_eq!(ended.len(), 1);
    assert_eq!(payload_str(ended[0], "reason"), Some("bound"));
    assert_eq!(payload_str(ended[0], "bound"), Some("max_tool_calls"));
    assert!(
        !step
            .requests
            .iter()
            .any(|r| matches!(r, ResearchRequest::Dispatch(_))),
        "nothing is dispatched"
    );
}

/// HI-6, §6.9 step 4: an error outcome ends the run `error`, with no output from that reply.
#[test]
#[ignore = "pending E19-13"]
fn an_error_outcome_ends_the_run_with_no_output() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let mut record = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
    record.outcome = Outcome::ProviderUnavailable;
    record.output = None;
    answered(&mut run, record, &ports, &world);
    assert!(run.lp.events("ThesisProposed").is_empty());
    assert!(run.lp.events("ModelOutputRecorded").is_empty());
    let ended = run.lp.events("RunEnded");
    assert_eq!(payload_str(ended[0], "reason"), Some("error"));
}

/// §6.9 "at every step": a poll past the run's deadline ends it `deadline` without waiting for the
/// worker, and asks the worker to cancel.
#[test]
#[ignore = "pending E19-13"]
fn the_risk_clock_past_the_deadline_ends_the_run_and_cancels_it() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, _) = seeded(&ports, &world);
    let step = go(
        &mut run.lp,
        ResearchInput::Poll(clock(CUT_OFF + 901)),
        &ports,
        &world,
    );
    let ended = run.lp.events("RunEnded");
    assert_eq!(ended.len(), 1);
    assert_eq!(payload_str(ended[0], "reason"), Some("deadline"));
    assert_eq!(
        run.lp.types()[run.lp.types().len() - 2..],
        ["RunSummaryRecorded", "RunEnded"]
    );
    assert!(
        step.requests
            .iter()
            .any(|r| matches!(r, ResearchRequest::Cancel { run_id } if *run_id == run.run_id))
    );
    assert!(run.lp.research.in_flight().is_none());
}

/// HI-15, §6.9: a pause cancels the run in flight, which ends `mode`.
#[test]
#[ignore = "pending E19-13"]
fn a_pause_ends_the_run_mode() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, _) = seeded(&ports, &world);
    run.lp.shell.run(Input::Command(Command::Pause), &ports);
    go(
        &mut run.lp,
        ResearchInput::Poll(clock(CUT_OFF + 10)),
        &ports,
        &world,
    );
    let ended = run.lp.events("RunEnded");
    assert_eq!(payload_str(ended[0], "reason"), Some("mode"));
}

/// Rule 183, §5.3: a reply arriving after its run ended is journaled with no verdicts and no tool
/// call records, and nothing follows it.
#[test]
#[ignore = "pending E19-13"]
fn a_late_reply_is_journaled_with_no_verdicts_and_no_tool_calls() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    go(
        &mut run.lp,
        ResearchInput::Poll(clock(CUT_OFF + 901)),
        &ports,
        &world,
    );
    let record = reply(
        &run,
        &turn,
        &world,
        vec![call(0, "news.search", r#"{"terms":["AAPL"]}"#)],
        None,
    );
    let step = go(
        &mut run.lp,
        ResearchInput::Called {
            run_id: run.run_id.clone(),
            at: clock(CUT_OFF + 902),
            record: Box::new(record),
        },
        &ports,
        &world,
    );
    let types = run.lp.types();
    assert_eq!(
        types.last().map(String::as_str),
        Some("ModelInvocationRecorded")
    );
    assert!(run.lp.events("ToolCallRecorded").is_empty());
    let late = run.lp.events("ModelInvocationRecorded");
    assert_eq!(
        strs(late[late.len() - 1].payload.get("verdicts")),
        Vec::<&str>::new()
    );
    assert!(step.requests.is_empty());
}

/// §6.10: a tool not on the run's menu can never run, whatever the reply asks; the gateway's
/// validation is not the only wall.
#[test]
#[ignore = "pending E19-13"]
fn a_tool_off_the_menu_is_never_dispatched() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![call(0, "records.query", r#"{"kind":"positions"}"#)],
        None,
    );
    let step = go(
        &mut run.lp,
        ResearchInput::Called {
            run_id: run.run_id.clone(),
            at: clock(CUT_OFF + 20),
            record: Box::new(record),
        },
        &ports,
        &world,
    );
    assert!(
        !step
            .requests
            .iter()
            .any(|r| matches!(r, ResearchRequest::Dispatch(_))),
        "{step:?}"
    );
    assert!(
        run.lp.events("ToolCallRecorded").is_empty(),
        "rule 175 refuses it"
    );
    let ended = run.lp.events("RunEnded");
    assert_eq!(payload_str(ended[0], "reason"), Some("error"));
}

/// HI-3: the same inputs give the same drafts, ids, requests and artifacts.
#[test]
#[ignore = "pending E19-13"]
fn an_agentic_run_replays_identically() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let mut journals = Vec::new();
    for _ in 0..2 {
        let (mut run, seed) = seeded(&ports, &world);
        let turn = turn_of(&seed);
        let record = reply(
            &run,
            &turn,
            &world,
            vec![call(0, "news.search", r#"{"terms":["AAPL"]}"#)],
            None,
        );
        answered(&mut run, record, &ports, &world);
        go(
            &mut run.lp,
            ResearchInput::Dispatched {
                run_id: run.run_id.clone(),
                at: clock(CUT_OFF + 25),
                results: vec![read_result(1, 0, r#"{"items":[]}"#)],
            },
            &ports,
            &world,
        );
        journals.push((run.lp.shell.agent_journal.clone(), run.lp.steps.clone()));
    }
    assert_eq!(journals[0], journals[1]);
}
