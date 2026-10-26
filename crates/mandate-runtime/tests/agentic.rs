//! An agentic research run in the core (agent harness spec v0.2 §6.9 to §6.11, journal spec §9.23's
//! run records, DEC-2206): the kind and menu fixed before anything untrusted is read, `RunStarted`
//! before the seed, each turn's record heading its tool calls, reads dispatched only after their
//! records, results journaled before the next turn, a final output judged once, and the run's end.
//! Each case names the rule it holds.

mod common;

use std::collections::BTreeSet;

use common::research::{
    AAPL, CUT_OFF, Loop, MSFT, RESPONSE_AAPL, RecordedPlane, World, config, record_for, recorded,
};
use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, TestIds, clock, event, ports, text, universe, with_clock,
};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_gateway::money::Usd6;
use mandate_gateway::tools::ToolCall as GatewayToolCall;
use mandate_gateway::{CallRecord, Outcome};
use mandate_journal::{Draft, Invalid};
use mandate_research::{AssetId, WorkingUniverse};
use mandate_research_run::agentic::{
    LoopBounds, OutputMode, RunKind, Tool, ToolCallId, ToolOutcome,
};
use mandate_research_run::{Retrieval, retrieve};
use mandate_runtime::{
    ActiveThesis, AgenticConfig, Command, DispatchRequest, DispatchedResult, Effect, EventDraft,
    FoldedEvent, Input, Mode, Ports, ResearchInput, ResearchRequest, ResearchStep,
    RetrievedObservation, RiskClock, TurnRequest, WriterEpoch,
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
    seeded_after(ports, world, |_| {})
}

/// The agent's mode restricted to `exits_only` before the run is due, as the account stream
/// applies a daily-loss restriction.
fn exits_only(lp: &mut Loop) {
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
}

/// An active thesis on AAPL, so a `monitor` run is due.
fn active_aapl() -> ActiveThesis {
    ActiveThesis {
        thesis_id: "01ACTIVE".to_owned(),
        lineage_id: "01ACTIVE".to_owned(),
        revision: 0,
        instrument: AssetId::new(AAPL).unwrap_or_else(|e| panic!("{e}")),
        symbol: mandate_research_run::Symbol::new("AAPL").unwrap_or_else(|e| panic!("{e}")),
        expires_at: clock(CUT_OFF + 86_400),
    }
}

/// [`seeded`], with `before` applied to the booted loop before the run is due.
fn seeded_after(
    ports: &Ports<'_>,
    world: &World,
    before: impl FnOnce(&mut Loop),
) -> (Run, ResearchStep) {
    let mut lp = Loop::new(1);
    boot(&mut lp, ports);
    before(&mut lp);
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

/// The gateway's record of `turn`'s call carrying `output` as the gateway hands it on: a monitor
/// run's call is checked there against the monitor schema, `invalidations` alone (the gateway's
/// own `tests/agentic.rs` holds that check), and the core reads only the record's output. The test
/// gateway here checks single calls only, so the record is made from an ideate-shaped reply and
/// carries `output` as the monitor run's gateway would.
fn monitor_reply(run: &Run, turn: &TurnRequest, world: &World, output: &str) -> CallRecord {
    let mut record = reply(
        run,
        turn,
        world,
        Vec::new(),
        Some(r#"{"theses":[],"invalidations":[]}"#),
    );
    assert_eq!(record.outcome, Outcome::Ok);
    record.output =
        Some(mandate_canon::parse(output.as_bytes()).unwrap_or_else(|e| panic!("{e:?}")));
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
            .any(|bytes| format!("sha256:{}", Digest::of(bytes)) == universe_ref),
        "the core hands the shell the data universe it names (DEC-2206 item 4)"
    );
}

/// §6.9's run kinds: `monitor` in `exits_only` while a thesis is active, with its own menu; nothing
/// at all in `exits_only` with no active thesis.
#[test]
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
fn turn_one_follows_the_seed_s_observations_with_the_menu_and_the_run_s_ceiling() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    assert_eq!(
        turn.kind,
        RunKind::Ideate,
        "the run's kind, for the turn's call"
    );
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
            call(
                0,
                "news.search",
                r#"{"symbols":["AAPL"],"terms":["services"]}"#,
            ),
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
        vec![call(
            0,
            "news.search",
            r#"{"symbols":["AAPL"],"terms":["services"]}"#,
        )],
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

/// §6.9: a `monitor` run never proposes a thesis. The gateway pins its output schema without
/// `theses`; should a reply carry one anyway, the core uses nothing of it, as the gateway's
/// `schema_invalid` would: no thesis, no invalidation, no output, and the run ends `error`.
#[test]
fn a_monitor_reply_carrying_a_thesis_uses_nothing_and_ends_the_run_error() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut world = world();
    world.active = vec![active_aapl()];
    let (mut run, seed) = seeded_after(&ports, &world, exits_only);
    assert_eq!(
        payload_str(run.lp.events("RunStarted")[0], "kind"),
        Some("monitor")
    );
    let turn = turn_of(&seed);
    assert_eq!(
        turn.kind,
        RunKind::Monitor,
        "the run's kind, for the turn's call"
    );
    let before = run.lp.types().len();
    let record = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
    answered(&mut run, record, &ports, &world);
    let written: Vec<String> = run.lp.types()[before..].to_vec();
    assert_eq!(
        written,
        ["ModelInvocationRecorded", "RunSummaryRecorded", "RunEnded"],
        "the turn's record and the end, nothing judged"
    );
    let invocation = run.lp.events("ModelInvocationRecorded");
    let last = invocation[invocation.len() - 1];
    assert_eq!(
        last.payload
            .get("verdicts")
            .and_then(Value::as_array)
            .map(|verdicts| verdicts.len()),
        Some(0)
    );
    assert_eq!(
        payload_str(run.lp.events("RunEnded")[0], "reason"),
        Some("error")
    );
}

/// §6.9: a `monitor` run's output is `invalidations` alone, the gateway's schema for the kind, and
/// an invalidation of an active thesis is judged and journaled as `ThesisInvalidated`, so an event
/// run can end the position its thesis holds (DEC-2391, DEC-2392). The run ends `done`.
#[test]
fn a_monitor_reply_of_invalidations_alone_invalidates_the_thesis() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut world = world();
    world.active = vec![active_aapl()];
    let (mut run, seed) = seeded_after(&ports, &world, exits_only);
    let turn = turn_of(&seed);
    assert_eq!(turn.kind, RunKind::Monitor);
    let record = monitor_reply(
        &run,
        &turn,
        &world,
        r#"{"invalidations":[{"thesis_id":"01ACTIVE","evidence":[]}]}"#,
    );
    answered(&mut run, record, &ports, &world);
    let invalidated = run.lp.events("ThesisInvalidated");
    assert_eq!(invalidated.len(), 1, "{:?}", run.lp.types());
    assert_eq!(payload_str(invalidated[0], "thesis_id"), Some("01ACTIVE"));
    let invocation = run.lp.events("ModelInvocationRecorded");
    let verdicts = invocation[invocation.len() - 1]
        .payload
        .get("verdicts")
        .and_then(Value::as_array)
        .map(<[Value]>::to_vec)
        .unwrap_or_default();
    assert_eq!(verdicts.len(), 1);
    assert_eq!(
        verdicts[0].get("kind").and_then(Value::as_str),
        Some("invalidation")
    );
    assert_eq!(
        verdicts[0].get("result").and_then(Value::as_str),
        Some("judged")
    );
    assert_eq!(
        payload_str(run.lp.events("RunEnded")[0], "reason"),
        Some("done")
    );
}

/// §6.9 step 4: a reply asking for more tool calls than the run has left runs none of them; each is
/// refused `call_budget` in the batch, and the run ends `bound`.
#[test]
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
            call(
                0,
                "news.search",
                r#"{"symbols":["AAPL"],"terms":["services"]}"#,
            ),
            call(
                1,
                "news.search",
                r#"{"symbols":["MSFT"],"terms":["services"]}"#,
            ),
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
        vec![call(
            0,
            "news.search",
            r#"{"symbols":["AAPL"],"terms":["services"]}"#,
        )],
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
            vec![call(
                0,
                "news.search",
                r#"{"symbols":["AAPL"],"terms":["services"]}"#,
            )],
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

/// The step's result, refused or not, for the cases that must be refused.
fn tried(
    lp: &mut Loop,
    input: ResearchInput,
    ports: &Ports<'_>,
    world: &World,
) -> Result<ResearchStep, mandate_runtime::RuntimeError> {
    lp.step(input, ports, world)
}

/// DEC-2206 item 8: a `single_call` step that journals carries its run as the correlation id, so
/// the shell can stamp the run on every draft; one that journals nothing carries none.
#[test]
fn a_single_call_step_that_journals_names_its_run() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = World::open();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let started = go(&mut lp, ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert_eq!(started.correlation_id, None, "the start journals nothing");
    let Some((run_id, cut_off)) = started.requests.iter().find_map(|r| match r {
        ResearchRequest::Start { run_id, cut_off } => Some((run_id.clone(), *cut_off)),
        _ => None,
    }) else {
        panic!("a due run starts");
    };
    let plane = RecordedPlane::load();
    let retrieval = retrieve(&config().plan, &world.scope(cut_off), plane.readers())
        .unwrap_or_else(|e| panic!("{e}"));
    let items = retrieval
        .items
        .iter()
        .map(|item| {
            RetrievedObservation::capture(item, cut_off)
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        })
        .collect();
    let seeded = go(
        &mut lp,
        ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items,
        },
        &ports,
        &world,
    );
    assert_eq!(seeded.correlation_id.as_deref(), Some(run_id.as_str()));
}

/// HI-2: a seed off the run's cut-off, or with an item twice, is refused and journals nothing.
#[test]
fn an_agentic_seed_off_its_cut_off_or_with_an_item_twice_is_refused() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    for broken in ["off_cut_off", "twice"] {
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let started = go(&mut lp, ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
        let Some((run_id, cut_off)) = started.requests.iter().find_map(|r| match r {
            ResearchRequest::Start { run_id, cut_off } => Some((run_id.clone(), *cut_off)),
            _ => None,
        }) else {
            panic!("a due run starts");
        };
        let plane = RecordedPlane::load();
        let retrieval = retrieve(&config().plan, &world.scope(cut_off), plane.readers())
            .unwrap_or_else(|e| panic!("{e}"));
        let mut items: Vec<RetrievedObservation> = retrieval
            .items
            .iter()
            .map(|item| {
                RetrievedObservation::capture(item, cut_off)
                    .unwrap_or_else(|e| panic!("{e}"))
                    .0
            })
            .collect();
        if broken == "twice" {
            let first = items[0].clone();
            items.push(first);
        } else {
            items[0].observation.as_of = clock(CUT_OFF - 1);
        }
        let before = lp.types().len();
        let refused = tried(
            &mut lp,
            ResearchInput::Retrieved { run_id, items },
            &ports,
            &world,
        );
        assert!(refused.is_err(), "{broken}: {refused:?}");
        assert_eq!(lp.types().len(), before, "{broken}: nothing journaled");
        assert!(lp.research.in_flight().is_some(), "{broken}: the run stays");
    }
}

/// §5.3: a run cancelled before its first turn has no call out, so a refused call for it is not
/// its and is refused.
#[test]
fn a_run_cancelled_before_its_first_turn_has_no_call_to_answer() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let started = go(&mut lp, ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    let Some(run_id) = started.requests.iter().find_map(|r| match r {
        ResearchRequest::Start { run_id, .. } => Some(run_id.clone()),
        _ => None,
    }) else {
        panic!("a due run starts");
    };
    lp.shell.run(Input::Command(Command::Pause), &ports);
    go(
        &mut lp,
        ResearchInput::Poll(clock(CUT_OFF + 1)),
        &ports,
        &world,
    );
    let refused = tried(
        &mut lp,
        ResearchInput::Refused {
            run_id,
            at: clock(CUT_OFF + 2),
            error: mandate_gateway::RequestError::UnknownModel,
        },
        &ports,
        &world,
    );
    assert!(refused.is_err(), "{refused:?}");
}

/// A turn's reply is the run's only if it is the record of the call out: a record of another call,
/// or one arriving while the reply's reads are out, is refused and journals nothing.
#[test]
fn a_reply_that_is_not_the_turn_s_call_is_refused() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let mut other = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
    other.call_id = "01J9ZZZZZZZZZZZZZZZZZZZZZZ".to_owned();
    let before = run.lp.types().len();
    let refused = tried(
        &mut run.lp,
        ResearchInput::Called {
            run_id: run.run_id.clone(),
            at: clock(CUT_OFF + 20),
            record: Box::new(other),
        },
        &ports,
        &world,
    );
    assert!(refused.is_err(), "another call's record: {refused:?}");
    assert_eq!(run.lp.types().len(), before);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![call(
            0,
            "news.search",
            r#"{"symbols":["AAPL"],"terms":["services"]}"#,
        )],
        None,
    );
    answered(&mut run, record.clone(), &ports, &world);
    let before = run.lp.types().len();
    let again = tried(
        &mut run.lp,
        ResearchInput::Called {
            run_id: run.run_id.clone(),
            at: clock(CUT_OFF + 21),
            record: Box::new(record),
        },
        &ports,
        &world,
    );
    assert!(again.is_err(), "a reply while the reads are out: {again:?}");
    assert_eq!(run.lp.types().len(), before);
}

/// §5.3: a reply that arrives once research may no longer run is journaled with nothing used, and
/// the run ends `mode`; one completed after the run's deadline ends it `deadline`.
#[test]
fn a_reply_research_may_no_longer_use_ends_the_run_mode_or_deadline() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    for late in [false, true] {
        let mut world = world();
        let (mut run, seed) = seeded(&ports, &world);
        let turn = turn_of(&seed);
        let mut record = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
        if late {
            record.completed_at = mandate_time::UtcNanos::from_parts(CUT_OFF + 901, 0)
                .unwrap_or_else(|e| panic!("{e}"));
            record.deadline = record.completed_at;
        } else {
            world.preconditions =
                mandate_runtime::Preconditions::read(&common::research::Catalog(false));
        }
        answered(&mut run, record, &ports, &world);
        assert!(
            run.lp.events("ThesisProposed").is_empty(),
            "nothing is used"
        );
        let ended = run.lp.events("RunEnded");
        assert_eq!(
            payload_str(ended[0], "reason"),
            Some(if late { "deadline" } else { "mode" })
        );
        assert!(run.lp.research.in_flight().is_none(), "the run is over");
        let summary = run.lp.events("RunSummaryRecorded");
        assert_eq!(summary.len(), 1);
    }
}

/// A dispatch's results are the run's only if they answer exactly its calls, of its turn, in order.
#[test]
fn results_that_are_not_the_dispatch_s_calls_are_refused() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    for broken in ["index", "turn", "run"] {
        let (mut run, seed) = seeded(&ports, &world);
        let turn = turn_of(&seed);
        let record = reply(
            &run,
            &turn,
            &world,
            vec![call(
                0,
                "news.search",
                r#"{"symbols":["AAPL"],"terms":["services"]}"#,
            )],
            None,
        );
        answered(&mut run, record, &ports, &world);
        let result = match broken {
            "index" => read_result(1, 1, "{}"),
            _ => read_result(if broken == "turn" { 2 } else { 1 }, 0, "{}"),
        };
        let run_id = if broken == "run" {
            "01J9ZZZZZZZZZZZZZZZZZZZZZY".to_owned()
        } else {
            run.run_id.clone()
        };
        let before = run.lp.types().len();
        let refused = tried(
            &mut run.lp,
            ResearchInput::Dispatched {
                run_id,
                at: clock(CUT_OFF + 25),
                results: vec![result],
            },
            &ports,
            &world,
        );
        let refusal = format!("{refused:?}");
        let ours = if broken == "run" {
            "not in flight"
        } else {
            "not exactly the dispatch's calls"
        };
        assert!(
            refusal.contains(ours),
            "{broken}: the core refuses before the loop sees them: {refusal}"
        );
        assert_eq!(run.lp.types().len(), before, "{broken}");
    }
}

/// Results that arrive once research may no longer run, or after the deadline, journal nothing they
/// hold and end the run (`mode`, `deadline`); a pause exactly at the deadline is still `mode`.
#[test]
fn results_research_may_no_longer_use_end_the_run() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    for (case, at, paused, reason) in [
        ("paused", CUT_OFF + 25, true, "mode"),
        ("paused_at_the_deadline", CUT_OFF + 900, true, "mode"),
        ("late", CUT_OFF + 901, false, "deadline"),
    ] {
        let world = world();
        let (mut run, seed) = seeded(&ports, &world);
        let turn = turn_of(&seed);
        let record = reply(
            &run,
            &turn,
            &world,
            vec![call(
                0,
                "news.search",
                r#"{"symbols":["AAPL"],"terms":["services"]}"#,
            )],
            None,
        );
        answered(&mut run, record, &ports, &world);
        if paused {
            run.lp.shell.run(Input::Command(Command::Pause), &ports);
        }
        go(
            &mut run.lp,
            ResearchInput::Dispatched {
                run_id: run.run_id.clone(),
                at: clock(at),
                results: vec![read_result(1, 0, "{}")],
            },
            &ports,
            &world,
        );
        assert!(run.lp.events("ToolResultRecorded").is_empty(), "{case}");
        let ended = run.lp.events("RunEnded");
        assert_eq!(payload_str(ended[0], "reason"), Some(reason), "{case}");
    }
}

/// A poll that cancels a run exactly at its deadline, because the owner paused, ends it `mode`,
/// not `deadline`: only the risk clock past the deadline is a deadline end.
#[test]
fn a_pause_seen_at_the_deadline_ends_the_run_mode() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, _) = seeded(&ports, &world);
    run.lp.shell.run(Input::Command(Command::Pause), &ports);
    go(
        &mut run.lp,
        ResearchInput::Poll(clock(CUT_OFF + 900)),
        &ports,
        &world,
    );
    let ended = run.lp.events("RunEnded");
    assert_eq!(payload_str(ended[0], "reason"), Some("mode"));
}

/// Every turn's call id is its own: none is the id of a record the run journaled.
#[test]
fn a_turn_s_call_id_is_never_a_journaled_record_s() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    assert!(
        run.lp
            .shell
            .agent_journal
            .iter()
            .all(|event| event.event_id.0 != turn.call.call_id),
        "{}",
        turn.call.call_id
    );
}

/// `RunSummaryRecorded` counts the run's theses proposed and admitted from its thesis records: an
/// admitted thesis counts once, an ignored one only as proposed.
#[test]
fn the_summary_counts_the_theses_proposed_and_admitted() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let short = RESPONSE_AAPL.replace(r#""long""#, r#""short""#);
    for (output, admitted) in [(RESPONSE_AAPL.to_owned(), 1), (short, 0)] {
        let world = world();
        let (mut run, seed) = seeded(&ports, &world);
        let turn = turn_of(&seed);
        let record = reply(&run, &turn, &world, Vec::new(), Some(&output));
        answered(&mut run, record, &ports, &world);
        let summary = run.lp.events("RunSummaryRecorded")[0];
        assert_eq!(
            summary.payload.get("proposed").and_then(Value::as_int),
            Some(1),
            "{output}"
        );
        assert_eq!(
            summary.payload.get("admitted").and_then(Value::as_int),
            Some(admitted),
            "{output}"
        );
    }
}

/// `RunEnded.result_bytes` counts the run's read results only (§6.9 step 5): a note's bytes do not.
#[test]
fn the_end_counts_only_read_results_bytes() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![
            call(
                0,
                "news.search",
                r#"{"symbols":["AAPL"],"terms":["services"]}"#,
            ),
            call(1, "note", r#"{"text":"check services"}"#),
        ],
        None,
    );
    answered(&mut run, record, &ports, &world);
    let next = go(
        &mut run.lp,
        ResearchInput::Dispatched {
            run_id: run.run_id.clone(),
            at: clock(CUT_OFF + 25),
            results: vec![read_result(1, 0, "0123456789"), read_result(1, 1, "abc")],
        },
        &ports,
        &world,
    );
    let turn = turn_of(&next);
    let record = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
    answered(&mut run, record, &ports, &world);
    let ended = run.lp.events("RunEnded")[0];
    assert_eq!(
        ended.payload.get("result_bytes").and_then(Value::as_int),
        Some(10)
    );
}

/// §6.9 step 6: a turn ended `budget_exhausted` ends the run `cost`, one ended
/// `deadline_exceeded` ends it `deadline`, and any other error `error`.
#[test]
fn an_error_outcome_names_its_end() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    for (outcome, reason) in [
        (Outcome::BudgetExhausted, "cost"),
        (Outcome::DeadlineExceeded, "deadline"),
        (Outcome::RateLimitedLocal, "error"),
    ] {
        let world = world();
        let (mut run, seed) = seeded(&ports, &world);
        let turn = turn_of(&seed);
        let mut record = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
        record.outcome = outcome;
        record.output = None;
        answered(&mut run, record, &ports, &world);
        let ended = run.lp.events("RunEnded");
        assert_eq!(payload_str(ended[0], "reason"), Some(reason), "{outcome:?}");
    }
}

fn digests(value: &Value, out: &mut BTreeSet<String>) {
    match value {
        Value::Str(text) if text.len() == 71 && text.starts_with("sha256:") => {
            out.insert(text.clone());
        }
        Value::Array(items) => items.iter().for_each(|v| digests(v, out)),
        Value::Object(members) => members.values().for_each(|v| digests(v, out)),
        _ => {}
    }
}

/// The draft in an agent-stream envelope of this file's own making, with the step's run as its
/// `correlation_id`, both references a research record names (the model's, the pinned entry's
/// content hash), and the payload's references listed
/// (journal spec §3), parsed by the journal's registered schemas, which share no code with the core.
fn parse_as_journal_draft(draft: &EventDraft, run: Option<&str>) -> Result<Draft, Invalid> {
    let quoted =
        |text: Option<&str>| text.map_or_else(|| "null".to_owned(), |t| format!("\"{t}\""));
    let mut listed = BTreeSet::new();
    digests(&draft.payload, &mut listed);
    let refs: Vec<String> = listed.iter().map(|r| format!("\"{r}\"")).collect();
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}",
        "stream_id":"agent:ws1:agent-a","event_type":"{}","schema_version":1,
        "event_time":"2026-10-16T20:00:00.000000000Z","clock_source":"scheduler",
        "causation_id":{},"correlation_id":{},
        "actor":{{"kind":"agent","id":"agent-a","version":"0.1.0","build":"sha256:{}"}},
        "config_refs":{{"mandate_version":"sha256:{}","model_version":"sha256:{}"}},
        "payload":{},"artifact_refs":[{}],"pii_refs":[]}}"#,
        draft.event_id.0,
        draft.event_type,
        quoted(draft.causation_id.as_ref().map(|id| id.0.as_str())),
        quoted(run),
        "c".repeat(64),
        "1".repeat(64),
        common::research::pin().content_hash,
        String::from_utf8_lossy(&to_canonical(&draft.payload)),
        refs.join(",")
    );
    Draft::parse(body.as_bytes())
}

/// Journal spec §9.23, §3: every draft of a whole agentic run, from its start through a read, its
/// result, and a final output to its end, is one the journal's own schemas accept, with the step's
/// run as its `correlation_id`: the references it writes are `sha256:` digests, as `ref` requires.
#[test]
fn every_record_of_an_agentic_run_passes_the_journal_s_check() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![call(
            0,
            "news.search",
            r#"{"symbols":["AAPL"],"terms":["services"]}"#,
        )],
        None,
    );
    answered(&mut run, record, &ports, &world);
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
    let turn = turn_of(&next);
    let record = reply(&run, &turn, &world, Vec::new(), Some(RESPONSE_AAPL));
    answered(&mut run, record, &ports, &world);
    let mut seen = BTreeSet::new();
    let mut failed = Vec::new();
    for step in &run.lp.steps {
        for effect in &step.effects {
            if let Effect::Journal(draft) = effect {
                seen.insert(draft.event_type.clone());
                if let Err(refusal) = parse_as_journal_draft(draft, step.correlation_id.as_deref())
                {
                    failed.push(format!("{}: {refusal:?}", draft.event_type));
                }
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
    for kind in [
        "RunStarted",
        "ModelInvocationRecorded",
        "ToolCallRecorded",
        "ToolResultRecorded",
        "RunSummaryRecorded",
        "RunEnded",
    ] {
        assert!(seen.contains(kind), "{kind} was checked: {seen:?}");
    }
}

fn refusal(run: &Run, secs: i64) -> ResearchInput {
    ResearchInput::Refused {
        run_id: run.run_id.clone(),
        at: clock(secs),
        error: mandate_gateway::RequestError::Tools,
    }
}

fn starts_a_run(step: &ResearchStep) -> bool {
    step.requests
        .iter()
        .any(|r| matches!(r, ResearchRequest::Start { .. }))
}

/// DEC-2261 item 1: the gateway's refusal of a turn ends the run `error` at once, with no record of
/// the refused call, and asks the worker for nothing more.
#[test]
fn a_refused_turn_ends_the_run_error_with_no_record() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    turn_of(&seed);
    let before = run.lp.types().len();
    let input = refusal(&run, CUT_OFF + 20);
    let step = go(&mut run.lp, input, &ports, &world);
    assert_eq!(
        run.lp.types()[before..],
        ["RunSummaryRecorded", "RunEnded"],
        "the end, and no ModelInvocationRecorded for a call the gateway never made"
    );
    assert_eq!(
        payload_str(run.lp.events("RunEnded")[0], "reason"),
        Some("error")
    );
    assert!(step.requests.is_empty(), "no turn follows: {step:?}");
    assert!(run.lp.research.in_flight().is_none());
}

/// DEC-2261 item 2: a refused turn withdraws its pin, as a refused single call does: no run starts
/// under it again, and a new version re-enables research.
#[test]
fn a_refused_turn_withdraws_its_pin_until_a_new_version() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let mut world = world();
    let (mut run, _) = seeded(&ports, &world);
    let input = refusal(&run, CUT_OFF + 20);
    go(&mut run.lp, input, &ports, &world);
    let later = clock(CUT_OFF + 2 * i64::from(common::research::INTERVAL_S));
    let withdrawn = go(&mut run.lp, ResearchInput::Poll(later), &ports, &world);
    assert!(!starts_a_run(&withdrawn), "{withdrawn:?}");
    if let Some(config) = world.config.as_mut() {
        config.pin.model_version = "0.2.0".to_owned();
    }
    let repinned = go(
        &mut run.lp,
        ResearchInput::Poll(clock(later.secs() + 1)),
        &ports,
        &world,
    );
    assert!(starts_a_run(&repinned), "a new version re-enables research");
}

/// DEC-2261 item 3: a refusal while the run's reads are out, not its turn, is not interpreted, and
/// the run stays as it was.
#[test]
fn a_refusal_while_the_reads_are_out_is_not_interpreted() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![call(
            0,
            "news.search",
            r#"{"symbols":["AAPL"],"terms":["services"]}"#,
        )],
        None,
    );
    let step = answered(&mut run, record, &ports, &world);
    dispatch_of(&step);
    let before = run.lp.types().len();
    let input = refusal(&run, CUT_OFF + 25);
    let refused = tried(&mut run.lp, input, &ports, &world);
    assert!(
        matches!(
            refused,
            Err(mandate_runtime::RuntimeError::NotInterpreted { .. })
        ),
        "{refused:?}"
    );
    assert_eq!(run.lp.types().len(), before, "nothing journaled");
    assert_eq!(run.lp.research.in_flight(), Some(run.run_id.as_str()));
}

/// A refusal naming another run is not this run's, whatever phase this run is in.
#[test]
fn a_refusal_for_another_run_leaves_the_agentic_run_in_flight() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, _) = seeded(&ports, &world);
    let before = run.lp.types().len();
    let refused = tried(
        &mut run.lp,
        ResearchInput::Refused {
            run_id: "01NOTTHISRUN".to_owned(),
            at: clock(CUT_OFF + 20),
            error: mandate_gateway::RequestError::Tools,
        },
        &ports,
        &world,
    );
    assert!(
        matches!(
            refused,
            Err(mandate_runtime::RuntimeError::NotInterpreted { .. })
        ),
        "{refused:?}"
    );
    assert_eq!(run.lp.types().len(), before);
    assert_eq!(run.lp.research.in_flight(), Some(run.run_id.as_str()));
}

/// A `market.bars` read's result for turn 1, its items AAPL's recorded bar and a copy of it under
/// each of `others`, captured as the worker captures them; and the items themselves.
fn bars_result(
    run: &Run,
    others: &[&str],
) -> (DispatchedResult, Vec<mandate_research_run::RetrievedItem>) {
    let plane = RecordedPlane::load();
    let aapl = plane
        .bars
        .iter()
        .find(|bar| bar.symbol.as_str() == "AAPL")
        .cloned()
        .unwrap_or_else(|| panic!("the plane records an AAPL bar"));
    let mut bars = vec![aapl.clone()];
    for symbol in others {
        let mut other = aapl.clone();
        other.symbol = mandate_research_run::Symbol::new(symbol).unwrap_or_else(|e| panic!("{e}"));
        bars.push(other);
    }
    let retrieved: Vec<mandate_research_run::RetrievedItem> = bars
        .into_iter()
        .map(mandate_research_run::RetrievedItem::Bar)
        .collect();
    let items = retrieved
        .iter()
        .map(|item| {
            mandate_runtime::RetrievedObservation::capture(item, run.cut_off)
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        })
        .collect();
    (
        DispatchedResult {
            items,
            ..read_result(1, 0, r#"{"items":[],"truncated":false}"#)
        },
        retrieved,
    )
}

/// A run that reads `result` at turn 1, then gives the final output over the seed and the read's
/// items: its loop, every step taken.
fn read_then_final(
    result: impl FnOnce(&Run) -> (DispatchedResult, Vec<mandate_research_run::RetrievedItem>),
) -> Run {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    let turn = turn_of(&seed);
    let record = reply(
        &run,
        &turn,
        &world,
        vec![call(0, "market.bars", r#"{"count":5,"symbol":"AAPL"}"#)],
        None,
    );
    answered(&mut run, record, &ports, &world);
    let (dispatched, read) = result(&run);
    let next = go(
        &mut run.lp,
        ResearchInput::Dispatched {
            run_id: run.run_id.clone(),
            at: clock(CUT_OFF + 25),
            results: vec![dispatched],
        },
        &ports,
        &world,
    );
    let turn = turn_of(&next);
    let mut seen = run.retrieval.clone();
    seen.items.extend(read);
    let record = record_for(
        &recorded(RESPONSE_AAPL),
        &turn.call,
        &seen,
        &world,
        mandate_time::UtcNanos::from_parts(run.cut_off.secs(), 0).unwrap_or_else(|e| panic!("{e}")),
    );
    answered(&mut run, record, &ports, &world);
    run
}

fn instruments_read(run: &Run) -> Vec<String> {
    let summary = run.lp.events("RunSummaryRecorded");
    assert_eq!(summary.len(), 1, "one summary");
    strs(summary[0].payload.get("instruments_read"))
        .into_iter()
        .map(str::to_owned)
        .collect()
}

/// The run summary draft the core journaled, as its step wrote it.
fn summary_draft(run: &Run) -> (EventDraft, Option<String>) {
    run.lp
        .steps
        .iter()
        .flat_map(|step| {
            step.effects.iter().filter_map(move |effect| match effect {
                Effect::Journal(draft) if draft.event_type == "RunSummaryRecorded" => {
                    Some((draft.clone(), step.correlation_id.clone()))
                }
                _ => None,
            })
        })
        .next()
        .unwrap_or_else(|| panic!("the run journaled its summary"))
}

/// DEC-2267 item 1, journal spec §9.23: the run summary names the instruments its reads returned
/// items about by asset ID, the seed's instrument fact for each item's symbol.
#[test]
fn a_run_summary_names_its_reads_instruments_by_asset_id() {
    let run = read_then_final(|run| bars_result(run, &[]));
    assert_eq!(instruments_read(&run), [AAPL]);
}

/// DEC-2267 item 2: a read item's symbol the seed has no instrument fact for is left out of the
/// summary, mapped to nothing, while its `ObservationRecorded` keeps the symbol-derived id.
#[test]
fn a_symbol_the_seed_does_not_map_is_left_out_and_its_observation_keeps_it() {
    let run = read_then_final(|run| bars_result(run, &["IBM"]));
    assert_eq!(
        instruments_read(&run),
        [AAPL],
        "IBM is left out, not mapped"
    );
    let observed: Vec<Option<&str>> = run
        .lp
        .events("ObservationRecorded")
        .iter()
        .map(|event| payload_str(event, "instrument_id"))
        .collect();
    assert!(
        observed.contains(&Some("IBM")),
        "IBM's read is still observed: {observed:?}"
    );
}

/// DEC-2267 item 3, journal spec §9.23 and rule 181: every record of a run whose read returned
/// items about instruments is one the journal's own schemas accept.
#[test]
fn every_record_of_a_run_that_read_instruments_passes_the_journal_s_check() {
    let run = read_then_final(|run| bars_result(run, &["IBM"]));
    let mut failed = Vec::new();
    for step in &run.lp.steps {
        for effect in &step.effects {
            if let Effect::Journal(draft) = effect
                && let Err(refusal) = parse_as_journal_draft(draft, step.correlation_id.as_deref())
            {
                failed.push(format!("{}: {refusal:?}", draft.event_type));
            }
        }
    }
    assert!(failed.is_empty(), "{failed:#?}");
    let (mut summary, correlation) = summary_draft(&run);
    if let Value::Object(members) = &mut summary.payload {
        members.insert(
            mandate_canon::Key::new("instruments_read").unwrap_or_else(|e| panic!("{e:?}")),
            Value::Array(vec![Value::Str("AAPL".to_owned())]),
        );
    }
    assert!(
        parse_as_journal_draft(&summary, correlation.as_deref()).is_err(),
        "the check refuses the summary mapped back to the symbol"
    );
}

/// The journal check this file trusts refuses a run summary naming a symbol where an asset ID
/// belongs, and accepts the same summary naming the asset ID: the oracle fails on the bug F fixes.
#[test]
fn the_journal_check_refuses_a_summary_that_names_a_symbol() {
    let summary = |read: &str| {
        EventDraft {
        event_id: mandate_runtime::EventId("01J8Z3MC000000000000000099".to_owned()),
        event_type: "RunSummaryRecorded".to_owned(),
        causation_id: None,
        payload: mandate_canon::parse(
            format!(
                r#"{{"admitted":0,"instruments_read":["{read}"],"invalidated":0,"proposed":0,"watch":[]}}"#
            )
            .as_bytes(),
        )
        .unwrap_or_else(|e| panic!("{e:?}")),
    }
    };
    let run = Some("01J8Z3MC0000000000000000RN");
    assert!(parse_as_journal_draft(&summary(AAPL), run).is_ok());
    assert!(parse_as_journal_draft(&summary("AAPL"), run).is_err());
}

/// DEC-2397 item 3: a turn the model-gateway service never answered whole ends the run `error` as a
/// refused turn does, alerts the operator once naming its `RunEnded`, and keeps the pin, so the
/// next due run starts.
#[test]
fn a_lost_turn_ends_the_run_error_alerts_and_keeps_the_pin() {
    let (ids, plan, view) = fixture();
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = world();
    let (mut run, seed) = seeded(&ports, &world);
    turn_of(&seed);
    let before = run.lp.types().len();
    let lost = ResearchInput::Refused {
        run_id: run.run_id.clone(),
        at: clock(CUT_OFF + 20),
        error: mandate_gateway::RequestError::GatewayUnreachable,
    };
    let step = go(&mut run.lp, lost, &ports, &world);
    assert_eq!(run.lp.types()[before..], ["RunSummaryRecorded", "RunEnded"]);
    let ended = run.lp.events("RunEnded")[0].event_id.clone();
    let alerts: Vec<_> = step
        .effects
        .iter()
        .filter_map(|effect| match effect {
            mandate_runtime::Effect::Notify(reference) => {
                Some((reference.kind.key(), reference.subject_event.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(alerts, [("research_gateway_unreachable", ended)]);
    assert!(step.requests.is_empty(), "no turn follows: {step:?}");
    let later = clock(CUT_OFF + 2 * i64::from(common::research::INTERVAL_S));
    let next = go(&mut run.lp, ResearchInput::Poll(later), &ports, &world);
    assert!(starts_a_run(&next), "the pin is kept: {next:?}");
}
