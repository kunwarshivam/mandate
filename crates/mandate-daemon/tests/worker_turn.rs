//! The cell's worker sends the core's turn (DEC-2264; agent harness spec v0.2 §6.9 step 3; backlog
//! E19-13 slice 3c): only for a run it started under an agentic bundle, its transcript loaded
//! through the sink, the turn built over the run's seed and sent through the gateway over the
//! recorded reply, and the record's prompt and response stored before `Called`. A transcript
//! artifact the sink cannot give answers nothing; the gateway's refusal answers `Refused`.

mod cell_research;

use std::collections::BTreeSet;

use cell_research::{CUT_OFF, RUN, Shelf, agentic_bundle, bundle, parts, who};
use mandate_canon::{Digest, encode_ulid};
use mandate_daemon::ResearchWorker;
use mandate_daemon::recorded::Bundle;
use mandate_daemon::worker::{RecordedWorker, TurnError};
use mandate_gateway::fake::openrouter_reply;
use mandate_gateway::money::Usd6;
use mandate_gateway::registry::Reported;
use mandate_gateway::{Outcome, RequestError};
use mandate_research_run::agentic::{RunKind, Tool, TurnPlan};
use mandate_research_run::{CallPlan, ResearchRunError};
use mandate_runtime::{ResearchInput, ResearchRequest, RiskClock, TurnRef, TurnRequest};
use mandate_time::UtcNanos;

const NOTE_CALL: (&str, &str) = ("note", r#"{"text":"check the 10-Q"}"#);
const NOTE_RESULT: &[u8] = br#"{"items":[],"truncated":false}"#;

fn worker_over(bundle: &Bundle, shelf: &Shelf) -> RecordedWorker {
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    RecordedWorker::new(bundle, who(), Box::new(shelf.clone()), Box::new(now)).unwrap()
}

/// The worker over the agentic bundle with the run started, and the seed's item count.
fn started(shelf: &Shelf) -> (RecordedWorker, usize) {
    let mut worker = worker_over(&agentic_bundle(), shelf);
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

fn turn(turn: u32, tools: &[Tool], observations: usize, transcript: Vec<TurnRef>) -> TurnRequest {
    TurnRequest {
        call: CallPlan {
            run_id: RUN.to_owned(),
            call_id: encode_ulid((1 << 100) + u128::from(turn)),
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
            turn,
            tools: tools.iter().copied().collect(),
            spend_ceiling: Usd6::from_micros(1_500_000),
            time_left_ms: 900_000,
            turns_left: 9 - turn,
            tool_calls_left: 24,
        },
        transcript,
    }
}

/// Turn 1's reply asking for a note, and its one result, stored as the core stores them.
fn earlier_turn(shelf: &Shelf) -> TurnRef {
    let bundle = bundle();
    let reported = Reported {
        model: Some(bundle.entry.identity.model_name.clone()),
        provider: Some(bundle.endpoint.route.reported_provider.clone()),
    };
    let response = openrouter_reply(None, &[NOTE_CALL], &reported, bundle.reply.usage);
    shelf.put.borrow_mut().push(response.clone());
    shelf.put.borrow_mut().push(NOTE_RESULT.to_vec());
    TurnRef {
        response: Digest::of(&response),
        results: vec![Digest::of(NOTE_RESULT)],
    }
}

/// The stored digests since `before`.
fn stored_since(shelf: &Shelf, before: usize) -> BTreeSet<Digest> {
    shelf.put.borrow()[before..]
        .iter()
        .map(|bytes| Digest::of(bytes))
        .collect()
}

fn unanswered(worker: &mut RecordedWorker, request: &TurnRequest) -> TurnError {
    let outcome = worker.turn(request);
    assert!(
        worker
            .answers(RiskClock::from_secs(CUT_OFF + 30))
            .is_empty(),
        "nothing is answered: {outcome:?}"
    );
    match outcome {
        Err(error) => error,
        Ok(()) => panic!("a turn that must answer nothing answered"),
    }
}

#[test]
fn a_sink_that_only_stores_gives_nothing_back() {
    struct Store;
    impl mandate_daemon::ArtifactSink for Store {
        fn put(&mut self, _: &[u8]) -> bool {
            true
        }
    }
    assert_eq!(
        mandate_daemon::ArtifactSink::get(&Store, &Digest::of(b"x")),
        None
    );
}

#[test]
fn a_first_turn_answers_the_recorded_reply_with_its_prompt_and_response_stored() {
    let shelf = Shelf::default();
    let (mut worker, items) = started(&shelf);
    let before = shelf.put.borrow().len();
    worker
        .turn(&turn(1, &[Tool::MarketBars, Tool::Note], items, Vec::new()))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let at = RiskClock::from_secs(CUT_OFF + 30);
    let answers = worker.answers(at);
    let [
        ResearchInput::Called {
            run_id,
            at: stamped,
            record,
        },
    ] = answers.as_slice()
    else {
        panic!("one call record: {answers:?}");
    };
    assert_eq!((run_id.as_str(), *stamped), (RUN, at));
    assert_eq!(record.outcome, Outcome::Ok);
    assert!(
        record.output.is_some(),
        "the recorded reply is a final output"
    );
    let stored = stored_since(&shelf, before);
    assert!(record.prompt_ref.is_some_and(|d| stored.contains(&d)));
    assert!(record.response_ref.is_some_and(|d| stored.contains(&d)));
}

#[test]
fn a_later_turn_loads_its_transcript_through_the_sink() {
    let shelf = Shelf::default();
    let (mut worker, items) = started(&shelf);
    let earlier = earlier_turn(&shelf);
    worker
        .turn(&turn(
            2,
            &[Tool::MarketBars, Tool::Note],
            items,
            vec![earlier],
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 30));
    let [ResearchInput::Called { record, .. }] = answers.as_slice() else {
        panic!("one call record: {answers:?}");
    };
    assert_eq!(
        record.outcome,
        Outcome::Ok,
        "the gateway re-validated turn 1 from the loaded bytes"
    );
}

#[test]
fn a_transcript_artifact_the_sink_cannot_give_answers_nothing() {
    let shelf = Shelf::default();
    let (mut worker, items) = started(&shelf);
    let mut earlier = earlier_turn(&shelf);
    earlier.results = vec![Digest::of(b"never stored")];
    let before = shelf.put.borrow().len();
    let error = unanswered(
        &mut worker,
        &turn(2, &[Tool::Note], items, vec![earlier.clone()]),
    );
    assert_eq!(error, TurnError::Missing, "a result never stored");
    earlier.results = vec![Digest::of(NOTE_RESULT)];
    earlier.response = Digest::of(b"no reply");
    let error = unanswered(&mut worker, &turn(2, &[Tool::Note], items, vec![earlier]));
    assert_eq!(error, TurnError::Missing, "a reply never stored");
    assert_eq!(
        shelf.put.borrow().len(),
        before,
        "nothing sent, nothing stored"
    );
}

#[test]
fn a_turn_without_the_loop_its_run_or_a_fitting_transcript_answers_nothing() {
    let shelf = Shelf::default();
    let (mut worker, items) = started(&shelf);
    let error = unanswered(&mut worker, &turn(2, &[Tool::Note], items, Vec::new()));
    assert!(
        matches!(
            error,
            TurnError::Unbuilt(ResearchRunError::LoopOrder { .. })
        ),
        "turn 2 with no transcript: {error:?}"
    );

    let mut fresh = worker_over(&agentic_bundle(), &shelf);
    let error = unanswered(&mut fresh, &turn(1, &[Tool::Note], items, Vec::new()));
    assert_eq!(error, TurnError::NotStarted);

    let mut single = worker_over(&bundle(), &shelf);
    single.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        &parts(),
    );
    single.answers(RiskClock::from_secs(CUT_OFF + 1));
    let error = unanswered(&mut single, &turn(1, &[Tool::Note], items, Vec::new()));
    assert_eq!(error, TurnError::NoLoop, "a single-call bundle");
}

#[test]
fn a_turn_the_gateway_refuses_is_answered_refused() {
    let shelf = Shelf::default();
    let (mut worker, items) = started(&shelf);
    worker
        .turn(&turn(1, &[Tool::MemoryRuns], items, Vec::new()))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        worker.answers(RiskClock::from_secs(CUT_OFF + 30)),
        [ResearchInput::Refused {
            run_id: RUN.to_owned(),
            at: RiskClock::from_secs(CUT_OFF + 30),
            error: RequestError::Tools,
        }],
        "a tool the entry does not declare"
    );
}

#[test]
fn the_core_s_turn_request_is_run_when_handed() {
    let called_shelf = Shelf::default();
    let (mut called, items) = started(&called_shelf);
    let request = turn(1, &[Tool::Note], items, Vec::new());
    called.turn(&request).unwrap_or_else(|e| panic!("{e:?}"));
    let handed_shelf = Shelf::default();
    let (mut handed, _) = started(&handed_shelf);
    handed.hand(&ResearchRequest::Turn(Box::new(request)), &parts());
    let at = RiskClock::from_secs(CUT_OFF + 30);
    assert_eq!(handed.answers(at), called.answers(at));
    assert_eq!(*handed_shelf.put.borrow(), *called_shelf.put.borrow());
}

/// DEC-2316 item 2: the gateway's rate cap still bounds a fault that calls again. Within one
/// minute, the worker over a `v0_2` entry answers the loop's eight turns and the margin's two, and
/// refuses the next `rate_limited_local`. The count is the loop's pinned `max_turns` and the margin,
/// read from the bounds, not from the worker. Each call's transcript holds a different result, so
/// no call is the gateway's cache hit, which reaches no provider and is not counted.
#[test]
#[ignore = "pending E19-6"]
fn a_fault_that_calls_again_within_a_minute_is_refused_past_the_loop_s_turns() {
    let shelf = Shelf::default();
    let (mut worker, items) = started(&shelf);
    let allowed = mandate_research_run::agentic::LoopBounds::v0_2().max_turns + 2;
    let mut outcomes = Vec::new();
    for call in 1..=allowed + 1 {
        let mut earlier = earlier_turn(&shelf);
        let result = format!(r#"{{"items":[],"note":{call},"truncated":false}}"#).into_bytes();
        earlier.results = vec![Digest::of(&result)];
        shelf.put.borrow_mut().push(result);
        let mut request = turn(2, &[Tool::Note], items, vec![earlier]);
        request.call.call_id = encode_ulid((1 << 100) + u128::from(call));
        worker
            .turn(&request)
            .unwrap_or_else(|e| panic!("call {call}: {e:?}"));
        for answer in worker.answers(RiskClock::from_secs(CUT_OFF + 30)) {
            let ResearchInput::Called { record, .. } = answer else {
                panic!("call {call}: a call record: {answer:?}");
            };
            assert!(
                !record.cache_hit,
                "call {call} reached the provider: {record:?}"
            );
            outcomes.push(record.outcome);
        }
    }
    let answered = outcomes
        .iter()
        .take_while(|outcome| **outcome != Outcome::RateLimitedLocal)
        .count();
    assert_eq!(
        (answered, outcomes.last()),
        (
            usize::try_from(allowed).unwrap(),
            Some(&Outcome::RateLimitedLocal)
        ),
        "{outcomes:?}"
    );
}
