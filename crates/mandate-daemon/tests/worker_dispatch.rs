//! The cell's worker runs the core's dispatch (DEC-2260; agent harness spec v0.2 §6.9 step 5;
//! backlog E19-13 slice 3c): only with the loop's bounds and for a run it started, within the run's
//! frozen scope, each result's content and each item's record stored before the answer, and any
//! failure answering nothing. The oracle is the harness's own `dispatch` and `capture` over the same
//! bundle, not the worker. A bundle naming the agentic loop registers it and gives the worker its
//! bounds; one naming a loop no registry may pin is refused before any call (DEC-2263).

mod cell_research;

use std::collections::BTreeSet;

use cell_research::{CUT_OFF, RUN, SOURCES, Shelf, agentic_bundle, bundle, parts, who};
use mandate_canon::Digest;
use mandate_daemon::recorded::BundleError;
use mandate_daemon::worker::{DispatchError, RecordedWorker, WorkerError, WorkerFor};
use mandate_daemon::{ResearchParts, ResearchWorker};
use mandate_gateway::registry::Mode;
use mandate_research::AllowlistVersion;
use mandate_research_run::agentic::{LoopBounds, Tool, ToolCall};
use mandate_research_run::{
    Allowlist, Dispatch, DispatchScope, OutputBounds, Readers, ResearchRunError, Scope,
    SourceId as RunSource, agentic_loop, dispatch,
};
use mandate_runtime::{
    DispatchRequest, DispatchedResult, ResearchInput, ResearchRequest, RetrievedObservation,
    RiskClock,
};
use mandate_time::UtcNanos;

const TURN: u32 = 1;
const LEFT: u32 = 96_000;

fn bare(shelf: &Shelf) -> RecordedWorker {
    let who = WorkerFor {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        description: "Long-only US large caps, on public news and filings.".to_owned(),
    };
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    RecordedWorker::new(&bundle(), who, Box::new(shelf.clone()), Box::new(now)).unwrap()
}

/// The worker with the loop's bounds and the run started, and what it has stored so far.
fn started(shelf: &Shelf) -> (RecordedWorker, usize) {
    let mut worker = bare(shelf).with_loop(LoopBounds::v0_2());
    let start = ResearchRequest::Start {
        run_id: RUN.to_owned(),
        cut_off: RiskClock::from_secs(CUT_OFF),
    };
    worker.hand(&start, &parts());
    worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let stored = shelf.put.borrow().len();
    (worker, stored)
}

fn calls() -> Vec<ToolCall> {
    vec![
        ToolCall {
            index: 0,
            tool: Tool::MarketBars,
            arguments: r#"{"count":5,"symbol":"AAPL"}"#.to_owned(),
        },
        ToolCall {
            index: 1,
            tool: Tool::Note,
            arguments: r#"{"text":"check the 10-Q"}"#.to_owned(),
        },
    ]
}

fn request(calls: Vec<ToolCall>, allowlist_version: u32) -> DispatchRequest {
    DispatchRequest {
        run_id: RUN.to_owned(),
        turn: TURN,
        calls,
        result_bytes_left: LEFT,
        cut_off: RiskClock::from_secs(CUT_OFF),
        allowlist_version,
    }
}

/// The harness's own dispatch over the bundle's plane in the run's scope.
fn oracle(calls: &[ToolCall]) -> Dispatch {
    let bundle = bundle();
    let scope = Scope {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        cut_off: UtcNanos::from_parts(CUT_OFF, 0).unwrap(),
        data_universe: parts().data_symbols,
        held: BTreeSet::new(),
        allowlist: Allowlist {
            version: 3,
            sources: SOURCES.iter().map(|s| RunSource::new(s).unwrap()).collect(),
        },
    };
    let readers = Readers {
        bars: &bundle.plane,
        news: &bundle.plane,
        filings: &bundle.plane,
        instruments: &bundle.plane,
    };
    let within = DispatchScope {
        scope: &scope,
        bounds: &LoopBounds::v0_2(),
        result_bytes_left: LEFT,
    };
    dispatch(TURN, calls, within, readers).unwrap()
}

/// What the worker must store and answer for `calls`, computed from the oracle.
fn expected(calls: &[ToolCall]) -> (Vec<Vec<u8>>, Vec<DispatchedResult>) {
    let mut stored = Vec::new();
    let mut results = Vec::new();
    for result in oracle(calls).results {
        stored.push(result.content.clone());
        let mut items = Vec::new();
        for item in &result.items {
            let (observation, record) =
                RetrievedObservation::capture(item, RiskClock::from_secs(CUT_OFF)).unwrap();
            stored.push(record);
            items.push(observation);
        }
        results.push(DispatchedResult {
            call: result.call,
            outcome: result.outcome,
            code: result.code,
            content: Digest::of(&result.content),
            bytes: u32::try_from(result.content.len()).unwrap(),
            truncated: result.truncated,
            untrusted: result.untrusted,
            items,
        });
    }
    (stored, results)
}

#[test]
fn a_dispatch_stores_every_content_and_record_before_its_answer() {
    let shelf = Shelf::default();
    let (mut worker, before) = started(&shelf);
    worker.dispatch(&request(calls(), 3), &parts()).unwrap();
    let (stored, results) = expected(&calls());
    assert!(
        results.first().is_some_and(|bars| !bars.items.is_empty()),
        "the fixture reads AAPL's bars"
    );
    assert_eq!(
        shelf.put.borrow()[before..],
        stored,
        "each content, then its items' records, in call order"
    );
    assert_eq!(
        worker.answers(RiskClock::from_secs(CUT_OFF + 30)),
        [ResearchInput::Dispatched {
            run_id: RUN.to_owned(),
            at: RiskClock::from_secs(CUT_OFF + 30),
            results,
        }],
        "stamped at the pass it is handed in"
    );
    assert!(
        worker
            .answers(RiskClock::from_secs(CUT_OFF + 40))
            .is_empty()
    );
}

#[test]
fn the_core_s_dispatch_request_is_run_when_handed() {
    let handed_shelf = Shelf::default();
    let (mut handed, _) = started(&handed_shelf);
    let called_shelf = Shelf::default();
    let (mut called, _) = started(&called_shelf);
    called.dispatch(&request(calls(), 3), &parts()).unwrap();
    handed.hand(&ResearchRequest::Dispatch(request(calls(), 3)), &parts());
    let at = RiskClock::from_secs(CUT_OFF + 30);
    assert_eq!(handed.answers(at), called.answers(at));
    assert_eq!(*handed_shelf.put.borrow(), *called_shelf.put.borrow());
}

/// Each way a dispatch answers nothing, and the reason the worker gives.
fn unanswered(
    worker: &mut RecordedWorker,
    request: &DispatchRequest,
    parts: &ResearchParts,
) -> DispatchError {
    let outcome = worker.dispatch(request, parts);
    assert!(
        worker
            .answers(RiskClock::from_secs(CUT_OFF + 30))
            .is_empty(),
        "nothing is answered: {outcome:?}"
    );
    match outcome {
        Err(error) => error,
        Ok(()) => panic!("a dispatch that must answer nothing answered"),
    }
}

#[test]
fn without_the_loop_s_bounds_nothing_is_run() {
    let shelf = Shelf::default();
    let mut worker = bare(&shelf);
    worker.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        &parts(),
    );
    worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let before = shelf.put.borrow().len();
    let error = unanswered(&mut worker, &request(calls(), 3), &parts());
    assert_eq!(error, DispatchError::NoLoop);
    assert_eq!(shelf.put.borrow().len(), before, "nothing stored");
}

#[test]
fn a_run_not_started_or_cancelled_is_not_run() {
    let shelf = Shelf::default();
    let mut fresh = bare(&shelf).with_loop(LoopBounds::v0_2());
    let error = unanswered(&mut fresh, &request(calls(), 3), &parts());
    assert_eq!(error, DispatchError::NotStarted, "never started");

    let (mut worker, _) = started(&shelf);
    worker.hand(
        &ResearchRequest::Cancel {
            run_id: RUN.to_owned(),
        },
        &parts(),
    );
    worker.answers(RiskClock::from_secs(CUT_OFF + 2));
    let error = unanswered(&mut worker, &request(calls(), 3), &parts());
    assert_eq!(error, DispatchError::NotStarted, "cancelled");
}

#[test]
fn a_moved_allowlist_reads_nothing() {
    let shelf = Shelf::default();
    let (mut worker, before) = started(&shelf);
    let error = unanswered(&mut worker, &request(calls(), 4), &parts());
    assert_eq!(error, DispatchError::ScopeMoved, "the run froze version 4");
    let mut moved = parts();
    moved.facts.allowlist.version = AllowlistVersion(4);
    let error = unanswered(&mut worker, &request(calls(), 3), &moved);
    assert_eq!(error, DispatchError::ScopeMoved, "the pass moved to 4");
    assert_eq!(shelf.put.borrow().len(), before, "nothing stored");
}

#[test]
fn an_action_among_the_calls_is_refused_whole() {
    let shelf = Shelf::default();
    let (mut worker, before) = started(&shelf);
    let mut with_action = calls();
    with_action.push(ToolCall {
        index: 2,
        tool: Tool::ProposeExit,
        arguments: "{}".to_owned(),
    });
    let error = unanswered(&mut worker, &request(with_action, 3), &parts());
    assert!(
        matches!(
            error,
            DispatchError::Refused(ResearchRunError::LoopOrder { .. })
        ),
        "{error:?}"
    );
    assert_eq!(shelf.put.borrow().len(), before, "nothing stored");
}

#[test]
fn a_refused_store_answers_nothing() {
    let shelf = Shelf::default();
    let (mut worker, _) = started(&shelf);
    shelf.refuse.set(true);
    let error = unanswered(&mut worker, &request(calls(), 3), &parts());
    assert_eq!(error, DispatchError::Unstored);
}

#[test]
fn an_agentic_bundle_registers_its_pinned_loop() {
    let bundle = agentic_bundle();
    let registry = bundle.registry().unwrap_or_else(|e| panic!("{e:?}"));
    let pin = bundle.entry.content().model_ref();
    let entry = registry.entry(&pin).unwrap_or_else(|e| panic!("{e:?}"));
    let pinned = agentic_loop(&LoopBounds::v0_2(), &OutputBounds::v0_1()).unwrap();
    assert_eq!(entry.content().mode, Mode::Agentic(pinned));
}

#[test]
fn the_worker_takes_the_loop_s_bounds_from_an_agentic_bundle() {
    let shelf = Shelf::default();
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let mut worker = RecordedWorker::new(
        &agentic_bundle(),
        who(),
        Box::new(shelf.clone()),
        Box::new(now),
    )
    .unwrap();
    worker.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        &parts(),
    );
    worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    worker
        .dispatch(&request(calls(), 3), &parts())
        .unwrap_or_else(|e| panic!("no with_loop, the bundle's bounds: {e:?}"));
    let (_, results) = expected(&calls());
    assert_eq!(
        worker.answers(RiskClock::from_secs(CUT_OFF + 30)),
        [ResearchInput::Dispatched {
            run_id: RUN.to_owned(),
            at: RiskClock::from_secs(CUT_OFF + 30),
            results,
        }]
    );
}

#[test]
fn a_bundle_naming_an_unpinnable_loop_is_refused_before_any_call() {
    let mut bundle = agentic_bundle();
    bundle.entry.agentic = Some(LoopBounds {
        max_turns: 0,
        ..LoopBounds::v0_2()
    });
    assert_eq!(
        bundle.registry().map(|_| ()),
        Err(BundleError::Unregistered)
    );
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let built = RecordedWorker::new(&bundle, who(), Box::new(Shelf::default()), Box::new(now));
    assert!(
        matches!(built, Err(WorkerError::Bundle(BundleError::Unregistered))),
        "no worker, so no call: {:?}",
        built.err()
    );
}
