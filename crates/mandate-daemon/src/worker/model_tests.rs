//! The worker over a model double (DEC-2269; ADR-0005 §10; agent harness spec v0.2 §6.9): each turn
//! is sent to the double, which is never handed the bundle's recorded reply, and the gateway in
//! front of it still refuses what a hostile model asks for.

use mandate_canon::encode_ulid;
use mandate_gateway::Outcome;
use mandate_gateway::fake::{CompromisedModel, Strategy};
use mandate_gateway::money::Usd6;
use mandate_gateway::registry::Reported;
use mandate_research_run::CallPlan;
use mandate_research_run::agentic::{RunKind, Tool, TurnPlan};
use mandate_runtime::{ResearchInput, ResearchRequest, RiskClock, TurnRequest};
use mandate_time::UtcNanos;

use super::RecordedWorker;
use crate::ResearchWorker;
use crate::cell_research::{CUT_OFF, RUN, Shelf, agentic_bundle, parts, who};

const OFFERED: [Tool; 2] = [Tool::MarketBars, Tool::Note];

fn reported() -> Reported {
    let bundle = agentic_bundle();
    Reported {
        model: Some(bundle.entry.identity.model_name.clone()),
        provider: Some(bundle.endpoint.route.reported_provider.clone()),
    }
}

/// The worker over the agentic bundle sending to `model`, with the run started, and the seed's item
/// count.
fn started(model: CompromisedModel) -> (RecordedWorker<CompromisedModel>, usize) {
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let mut worker = RecordedWorker::with_model(
        &agentic_bundle(),
        who(),
        Box::new(Shelf::default()),
        Box::new(now),
        model,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    worker.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        &parts(),
    );
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let Some(ResearchInput::Retrieved { items, .. }) = answers.first() else {
        panic!("the run retrieved: {answers:?}");
    };
    (worker, items.len())
}

fn first_turn(observations: usize) -> TurnRequest {
    TurnRequest {
        call: CallPlan {
            run_id: RUN.to_owned(),
            call_id: encode_ulid((1 << 100) + 1),
            workspace_id: "ws1".to_owned(),
            agent_id: "agent-a".to_owned(),
            model: agentic_bundle().entry.content().model_ref(),
            as_of: UtcNanos::from_parts(CUT_OFF, 0).unwrap(),
            deadline_ms: 120_000,
            max_output_tokens: 4_096,
            observations: (0..observations)
                .map(|n| encode_ulid((1 << 101) + u128::try_from(n).unwrap()))
                .collect(),
        },
        kind: RunKind::Ideate,
        plan: TurnPlan {
            turn: 1,
            tools: OFFERED.into(),
            spend_ceiling: Usd6::from_micros(1_500_000),
            time_left_ms: 900_000,
            turns_left: 8,
            tool_calls_left: 24,
        },
        transcript: Vec::new(),
    }
}

/// Turn 1's one answer, sent to `model`.
fn answered(model: CompromisedModel) -> (ResearchInput, RecordedWorker<CompromisedModel>) {
    let (mut worker, items) = started(model);
    worker
        .turn(&first_turn(items))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut answers = worker.answers(RiskClock::from_secs(CUT_OFF + 30));
    assert_eq!(answers.len(), 1, "one answer: {answers:?}");
    (answers.remove(0), worker)
}

#[test]
fn each_turn_is_sent_to_the_model_and_answered_with_its_calls() {
    for seed in 0..8 {
        let model = CompromisedModel::new(Strategy::Random, seed, reported());
        let (answer, worker) = answered(model);
        assert_eq!(
            worker.transport().unwrap().sent().len(),
            1,
            "seed {seed}: one send"
        );
        let ResearchInput::Called { record, .. } = answer else {
            panic!("seed {seed}: a call record: {answer:?}");
        };
        assert_eq!(record.outcome, Outcome::Ok, "seed {seed}");
        assert_eq!(
            record.output, None,
            "seed {seed}: the model's tool-call reply, not the bundle's recorded final output"
        );
        let offered: Vec<&str> = OFFERED.iter().map(|tool| tool.name()).collect();
        assert!(!record.tool_calls.is_empty(), "seed {seed}");
        for call in &record.tool_calls {
            assert!(
                offered.contains(&call.name.as_str()),
                "seed {seed}: {} was offered",
                call.name
            );
        }
    }
}

#[test]
fn a_hostile_model_s_calls_are_refused_by_the_gateway_in_front_of_it() {
    for seed in 0..8 {
        let model = CompromisedModel::new(Strategy::Hostile, seed, reported());
        let (answer, worker) = answered(model);
        assert_eq!(
            worker.transport().unwrap().sent().len(),
            1,
            "seed {seed}: one send"
        );
        let ResearchInput::Called { record, .. } = answer else {
            panic!("seed {seed}: a call record: {answer:?}");
        };
        assert_eq!(
            (record.outcome, record.tool_calls.len(), record.output),
            (Outcome::SchemaInvalid, 0, None),
            "seed {seed}: a tool not offered, arguments outside its schema, or too many calls"
        );
    }
}
