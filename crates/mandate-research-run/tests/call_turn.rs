//! The worker's turn through the gateway (agent harness spec v0.2 §6.9 step 3; DEC-2256; backlog
//! E19-13 slice 3c): `call_turn` builds the turn's call at the gateway's own clock and sends it, as
//! the single `call` does, over the recorded fake with an agentic research entry registered; the
//! gateway's record carries the reply's validated tool calls, a request the entry could not have
//! made is refused with no record, and a turn that cannot be built sends nothing.

mod common;

use std::collections::BTreeMap;

use common::{
    AAPL_ID, AGENT, CUT_OFF, Fixture, NEWSWIRE, REPORTED_PROVIDER, ResearchGateway, SLUG, UPSTREAM,
    WORKSPACE, at, bars, budget, endpoint, entry, instrument, news, parts, plan, registry, scope,
    symbol, usage,
};
use mandate_canon::{Value, encode_ulid, parse};
use mandate_domain::AssetClass;
use mandate_gateway::fake::{Recorded, RecordedTransport, openrouter_reply};
use mandate_gateway::money::Usd6;
use mandate_gateway::provider::{Delivery, NoKeys};
use mandate_gateway::registry::{Mode, ModelRef, PinnedContent, Reported, Status};
use mandate_gateway::tools::{
    Agentic, LoopBounds, OutputKind, ToolClass as DeclaredClass, ToolDeclaration,
};
use mandate_gateway::{Outcome, RequestError};
use mandate_research_run::agentic::{RunKind, Tool, TurnPlan};
use mandate_research_run::{
    Answer, CallContext, CallPlan, EarlierTurn, NoHostedTransport, ResearchRunError, Retrieval,
    TurnParts, call_turn, research_gateway, retrieve,
};

const BARS_ARGS: &str = r#"{"type":"object","properties":{"symbol":{"type":"string","max_length":12},"count":{"type":"integer","minimum":1,"maximum":260}}}"#;
const NOTE_ARGS: &str =
    r#"{"type":"object","properties":{"text":{"type":"string","max_length":2000}}}"#;
const MONITOR_OUTPUT: &str = r#"{"type":"object","properties":{"invalidations":{"type":"array","max_items":3,"items":{"type":"object","properties":{"thesis_id":{"type":"string","max_length":26}}}}}}"#;
const BARS_CALL: (&str, &str) = ("market.bars", r#"{"count":30,"symbol":"AAPL"}"#);
const NOTE_CALL: (&str, &str) = ("note", r#"{"text":"check the 10-Q"}"#);

fn json(text: &str) -> Value {
    parse(text.as_bytes()).unwrap_or_else(|e| panic!("{e:?}"))
}

fn declared(name: &str, class: DeclaredClass, arguments: &str) -> ToolDeclaration {
    ToolDeclaration {
        name: name.to_owned(),
        class,
        description: format!("The {name} tool."),
        arguments: json(arguments),
        result_cap_bytes: 24_000,
        menu: vec!["monitor".to_owned()],
    }
}

/// The research entry as an agentic one: two tools on the `monitor` menu, the pinned bounds, and
/// the monitor run's final output schema.
fn agentic_content() -> PinnedContent {
    PinnedContent {
        model_version: "0.2.0".to_owned(),
        mode: Mode::Agentic(Agentic {
            output: OutputKind::FinalOutput,
            tools: vec![
                declared("market.bars", DeclaredClass::Read, BARS_ARGS),
                declared("note", DeclaredClass::Think, NOTE_ARGS),
            ],
            loop_bounds: LoopBounds {
                max_turns: 8,
                max_tool_calls: 24,
                max_tool_calls_per_turn: 4,
                max_theses: 3,
                max_result_bytes: 24_000,
                max_run_result_bytes: 96_000,
                max_repeat_denials: 2,
                max_run_s: 900,
                tool_timeout_s: 10,
            },
            output_schemas: BTreeMap::from([("monitor".to_owned(), json(MONITOR_OUTPUT))]),
        }),
        ..entry().content()
    }
}

fn reported() -> Reported {
    Reported {
        model: Some(SLUG.to_owned()),
        provider: Some(REPORTED_PROVIDER.to_owned()),
    }
}

/// The recorded gateway with the agentic entry registered beside the single-call one, `replies`
/// scripted in order.
fn agentic_gateway(replies: Vec<Vec<u8>>) -> (ResearchGateway, ModelRef) {
    let (mut registry, _) = registry();
    let pinned = agentic_content();
    let model = pinned.model_ref();
    registry
        .register(pinned)
        .unwrap_or_else(|e| panic!("the agentic entry registers: {e}"));
    registry
        .add_endpoint(&model, endpoint(), &[reported()])
        .unwrap_or_else(|e| panic!("{e}"));
    registry
        .set_status(&model, Status::Offered)
        .unwrap_or_else(|e| panic!("{e}"));
    let mut recorded = RecordedTransport::new();
    recorded.script(
        replies
            .into_iter()
            .map(|body| Recorded {
                latency_ms: 30_000,
                delivery: Delivery::Response {
                    status: 200,
                    body,
                    retry_after_ms: None,
                },
            })
            .collect(),
    );
    let mut pieces = parts(budget());
    pieces.registry = registry;
    let (gateway, _) = research_gateway(
        pieces,
        Box::new(NoKeys),
        None::<NoHostedTransport>,
        recorded,
        UPSTREAM,
    );
    (gateway, model)
}

fn calls(calls: &[(&str, &str)]) -> Vec<u8> {
    openrouter_reply(None, calls, &reported(), usage())
}

fn seed() -> Retrieval {
    let fixture = Fixture {
        instruments: vec![instrument("AAPL", AAPL_ID)],
        bars: bars("AAPL", 2),
        news: vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)],
        ..Fixture::default()
    };
    retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"))
}

fn call_plan(model: &ModelRef, observations: usize) -> CallPlan {
    CallPlan {
        run_id: encode_ulid((1 << 100) + 3),
        call_id: encode_ulid((1 << 100) + 7),
        workspace_id: WORKSPACE.to_owned(),
        agent_id: AGENT.to_owned(),
        model: model.clone(),
        as_of: at(CUT_OFF),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        observations: (0..observations)
            .map(|n| encode_ulid((1 << 101) + u128::try_from(n).unwrap_or(0)))
            .collect(),
    }
}

fn context() -> CallContext {
    CallContext {
        description: "Long-only US large caps on public news and filings.".to_owned(),
        data_universe: vec![symbol("AAPL"), symbol("MSFT")],
        asset_classes: vec![AssetClass::UsEquity],
        max_instruments: 5,
        working_universe: vec![symbol("AAPL")],
        held: Vec::new(),
        theses: Vec::new(),
    }
}

fn turn_plan(turn: u32, tools: &[Tool]) -> TurnPlan {
    TurnPlan {
        turn,
        tools: tools.iter().copied().collect(),
        spend_ceiling: Usd6::from_micros(1_500_000),
        time_left_ms: 3_600_000,
        turns_left: 9 - turn,
        tool_calls_left: 20,
    }
}

#[test]
fn a_turn_through_the_recorded_fake_answers_with_the_reply_s_validated_tool_calls() {
    let (mut gateway, model) = agentic_gateway(vec![calls(&[BARS_CALL, NOTE_CALL])]);
    let retrieved = seed();
    let call = call_plan(&model, retrieved.items.len());
    let plan = turn_plan(1, &[Tool::MarketBars, Tool::Note]);
    let (turn, answer) = call_turn(
        &mut gateway,
        TurnParts {
            call: &call,
            plan: &plan,
            kind: RunKind::Monitor,
            seed: &retrieved,
            context: context(),
            transcript: Vec::new(),
        },
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        turn.deadline,
        at(CUT_OFF + 5 + 120),
        "the deadline counts from the gateway's clock at dispatch"
    );
    let Answer::Recorded(record) = answer else {
        panic!("the gateway's record: {answer:?}")
    };
    assert_eq!(record.outcome, Outcome::Ok);
    assert_eq!(record.output, None, "a tool-call reply is not an output");
    let named: Vec<(String, Value)> = record
        .tool_calls
        .iter()
        .map(|call| (call.name.clone(), call.arguments.clone()))
        .collect();
    assert_eq!(
        named,
        [
            ("market.bars".to_owned(), json(BARS_CALL.1)),
            ("note".to_owned(), json(NOTE_CALL.1)),
        ]
    );
}

#[test]
fn a_later_turn_carries_its_transcript_through_the_gateway_s_check() {
    let (mut gateway, model) = agentic_gateway(vec![calls(&[NOTE_CALL])]);
    let retrieved = seed();
    let call = call_plan(&model, retrieved.items.len());
    let plan = turn_plan(2, &[Tool::MarketBars, Tool::Note]);
    let earlier = EarlierTurn {
        response: calls(&[BARS_CALL]),
        results: vec![br#"{"items":[],"truncated":false}"#.to_vec()],
    };
    let (_, answer) = call_turn(
        &mut gateway,
        TurnParts {
            call: &call,
            plan: &plan,
            kind: RunKind::Monitor,
            seed: &retrieved,
            context: context(),
            transcript: vec![earlier],
        },
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    let Answer::Recorded(record) = answer else {
        panic!("the gateway's record: {answer:?}")
    };
    assert_eq!(
        record.outcome,
        Outcome::Ok,
        "the gateway re-validated turn 1"
    );
    assert_eq!(record.tool_calls.len(), 1);
}

#[test]
fn a_turn_the_entry_could_not_have_made_is_refused_with_no_record() {
    let (mut gateway, model) = agentic_gateway(vec![calls(&[NOTE_CALL])]);
    let retrieved = seed();
    let call = call_plan(&model, retrieved.items.len());
    let plan = turn_plan(1, &[Tool::NewsSearch]);
    let (_, answer) = call_turn(
        &mut gateway,
        TurnParts {
            call: &call,
            plan: &plan,
            kind: RunKind::Monitor,
            seed: &retrieved,
            context: context(),
            transcript: Vec::new(),
        },
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(
        matches!(answer, Answer::Refused(RequestError::Tools)),
        "a tool off the entry's monitor menu: {answer:?}"
    );
}

#[test]
fn a_turn_that_cannot_be_built_sends_nothing() {
    let (mut gateway, model) = agentic_gateway(vec![calls(&[NOTE_CALL])]);
    let retrieved = seed();
    let call = call_plan(&model, retrieved.items.len());
    let fitting = turn_plan(1, &[Tool::Note]);
    let first = call_turn(
        &mut gateway,
        TurnParts {
            call: &call,
            plan: &fitting,
            kind: RunKind::Monitor,
            seed: &retrieved,
            context: context(),
            transcript: Vec::new(),
        },
    );
    first.unwrap_or_else(|e| panic!("the fitting turn is sent: {e:?}"));

    let (mut gateway, model) = agentic_gateway(vec![calls(&[NOTE_CALL])]);
    let call = call_plan(&model, retrieved.items.len());
    let plan = turn_plan(2, &[Tool::Note]);
    let built = call_turn(
        &mut gateway,
        TurnParts {
            call: &call,
            plan: &plan,
            kind: RunKind::Monitor,
            seed: &retrieved,
            context: context(),
            transcript: Vec::new(),
        },
    );
    assert!(
        matches!(built, Err(ResearchRunError::LoopOrder { .. })),
        "turn 2 with no transcript is refused before the gateway: {built:?}"
    );
    assert!(
        gateway.meter().store().records().is_empty(),
        "nothing reserved, so nothing was sent"
    );
}
