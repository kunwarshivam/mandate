//! The driver over the real core (DEC-2204): a research run handed through [`Driver`] and
//! [`RuntimeCore`] journals exactly what the runtime's own test loop journals for the same run, in
//! the same order, and hands the worker the same requests, so the driver adds no record, drops none,
//! and reorders nothing. The fixtures are the runtime's own (`mandate-runtime/tests/common`).

#[path = "../../mandate-runtime/tests/common/mod.rs"]
mod common;

use common::research::{
    CUT_OFF, Loop, RESPONSE_AAPL, RecordedPlane, World, config, full_run, gateway_for, instant,
    recorded,
};
use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, TestIds, clock, event, ports, text, universe, with_clock,
};
use mandate_research_driver::{
    AppendFailure, Driver, DriverError, JournalWriter, RuntimeCore, StepBatch, Stepped, Worker,
};
use mandate_research_run::{Answer, call, retrieve};
use mandate_runtime::{
    ActorKind, AgentId, Command, FoldedEvent, Initiator, Input, KillScope, Mode, Ports,
    ResearchInput, ResearchRequest, RetrievedObservation, RiskClock, Seq, WriterEpoch,
};

const AGENT_STREAM: &str = common::AGENT_STREAM;

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

/// The agent stream as a list, continuing from the boot's head. It answers its first appends with
/// `failures`, then commits.
struct Stream {
    next_seq: u64,
    stored: Vec<FoldedEvent>,
    failures: Vec<AppendFailure>,
}

impl JournalWriter for Stream {
    fn append(&mut self, batch: &StepBatch<'_>) -> Result<Vec<FoldedEvent>, AppendFailure> {
        let drafts = batch.drafts;
        if !self.failures.is_empty() {
            return Err(self.failures.remove(0));
        }
        let rows: Vec<FoldedEvent> = drafts
            .iter()
            .map(|draft| {
                let seq = self.next_seq;
                self.next_seq += 1;
                FoldedEvent {
                    stream: AGENT_STREAM.to_owned(),
                    seq: Seq(seq),
                    event_id: draft.event_id.clone(),
                    event_type: draft.event_type.clone(),
                    causation_id: draft.causation_id.clone(),
                    actor: ActorKind::Agent,
                    payload: draft.payload.clone(),
                }
            })
            .collect();
        self.stored.extend(rows.iter().cloned());
        Ok(rows)
    }
}

#[derive(Default)]
struct Requests(Vec<ResearchRequest>);

impl Worker for Requests {
    fn hand(&mut self, request: &ResearchRequest) {
        self.0.push(request.clone());
    }
}

fn handed(stepped: Stepped) -> Vec<ResearchRequest> {
    match stepped {
        Stepped::Handed { requests, .. } => requests,
        Stepped::Held => panic!("nothing is held over an in-memory stream"),
    }
}

#[test]
fn a_run_through_the_driver_journals_what_the_core_s_own_loop_journals() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = World::open();
    let plane = RecordedPlane::load();
    let model = recorded(RESPONSE_AAPL);
    let now = clock(CUT_OFF);

    let mut reference = Loop::new(1);
    boot(&mut reference, &ports);
    let booted = reference.shell.agent_journal.len();
    full_run(&mut reference, now, &ports, &world, &plane, &model);
    let want: Vec<(String, String)> = reference.shell.agent_journal[booted..]
        .iter()
        .map(|e| (e.event_id.0.clone(), e.event_type.clone()))
        .collect();
    let want_requests: Vec<ResearchRequest> = reference
        .steps
        .iter()
        .flat_map(|step| step.requests.clone())
        .collect();

    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let context = world.context();
    let mut stream = Stream {
        next_seq: lp.shell.head().0.saturating_add(1),
        stored: Vec::new(),
        failures: Vec::new(),
    };
    let mut worker = Requests::default();
    let mut driver = Driver::new();
    let mut core = RuntimeCore {
        state: &mut lp.shell.state,
        research: &mut lp.research,
        ports: &ports,
        context: &context,
    };
    let mut step = |input: ResearchInput| {
        handed(
            driver
                .step(input, &mut core, &mut stream, &mut worker)
                .unwrap_or_else(|e| panic!("{e}")),
        )
    };

    let started = step(ResearchInput::Poll(now));
    let Some(ResearchRequest::Start { run_id, cut_off }) = started.first().cloned() else {
        panic!("a due run starts: {started:?}");
    };
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
    let calls = step(ResearchInput::Retrieved {
        run_id: run_id.clone(),
        items,
    });
    let Some(ResearchRequest::Call(plan)) = calls.first().cloned() else {
        panic!("the retrieval asks for the call: {calls:?}");
    };
    let mut gateway = gateway_for(&model, instant(CUT_OFF));
    let (_, answer) = call(&mut gateway, &plan, &retrieval, world.call_context())
        .unwrap_or_else(|e| panic!("{e}"));
    let Answer::Recorded(record) = answer else {
        panic!("the recorded model answers");
    };
    step(ResearchInput::Called {
        run_id,
        at: RiskClock::from_secs(CUT_OFF + 30),
        record: Box::new(record),
    });
    step(ResearchInput::Poll(clock(CUT_OFF + 31)));

    let got: Vec<(String, String)> = stream
        .stored
        .iter()
        .map(|e| (e.event_id.0.clone(), e.event_type.clone()))
        .collect();
    assert_eq!(got, want, "the same records, the same ids, the same order");
    assert!(
        got.iter().any(|(_, t)| t == "ThesisProposed"),
        "the run reached admission: {got:?}"
    );
    assert_eq!(worker.0, want_requests, "the same requests, in order");
    assert_eq!(driver.pending(), None);
}

/// The worker's retrieval for a started run, as the core will be handed it.
fn retrieved(world: &World, started: &[ResearchRequest]) -> ResearchInput {
    let Some(ResearchRequest::Start { run_id, cut_off }) = started.first().cloned() else {
        panic!("a due run starts: {started:?}");
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
    ResearchInput::Retrieved { run_id, items }
}

fn kill_switch() -> Input {
    Input::Command(Command::KillSwitch {
        scope: KillScope::Agent(AgentId("agent-a".to_owned())),
        initiator: Initiator::Owner,
        confirmation: None,
    })
}

/// A research batch the journal refuses stores nothing, so no trading input may wait behind it:
/// after the refusal the core takes the next tick and the kill switch at once (`AGENTS.md` rules 3
/// and 13), and research stays halted.
#[test]
fn a_refused_research_batch_never_holds_trading_or_the_kill_switch() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = World::open();
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let context = world.context();
    let mut stream = Stream {
        next_seq: lp.shell.head().0.saturating_add(1),
        stored: Vec::new(),
        failures: vec![AppendFailure::Refused {
            code: "schema".to_owned(),
        }],
    };
    let mut driver = Driver::new();
    {
        let mut core = RuntimeCore {
            state: &mut lp.shell.state,
            research: &mut lp.research,
            ports: &ports,
            context: &context,
        };
        let started = driver
            .step(
                ResearchInput::Poll(clock(CUT_OFF)),
                &mut core,
                &mut stream,
                &mut Requests::default(),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let Stepped::Handed { requests, .. } = started else {
            panic!("nothing to hold yet");
        };
        let refused = driver.step(
            retrieved(&world, &requests),
            &mut core,
            &mut stream,
            &mut Requests::default(),
        );
        assert_eq!(
            refused,
            Err(DriverError::Refused {
                code: "schema".to_owned()
            })
        );
    }
    assert!(stream.stored.is_empty());
    assert!(driver.halted());
    assert!(
        mandate_runtime::handle(&mut lp.shell.state, Input::Tick(clock(CUT_OFF + 1)), &ports)
            .is_ok(),
        "the next tick is taken"
    );
    let killed = mandate_runtime::handle(&mut lp.shell.state, kill_switch(), &ports);
    assert!(killed.is_ok(), "the kill switch is taken: {killed:?}");
}

/// An append whose outcome is unknown holds the run; the same input retried journals exactly the
/// rows and hands exactly the requests a first-time commit would.
#[test]
fn an_unknown_append_retried_journals_what_a_clean_commit_journals() {
    let (ids, plan, view) = (TestIds, FixedPlan::silent(), universe(&[]));
    let ports = ports(&ids, &AllowGate, &plan, &view);
    let world = World::open();
    let context = world.context();
    let first_step = |failures: Vec<AppendFailure>| {
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let mut stream = Stream {
            next_seq: lp.shell.head().0.saturating_add(1),
            stored: Vec::new(),
            failures,
        };
        let mut worker = Requests::default();
        let mut driver = Driver::new();
        let mut core = RuntimeCore {
            state: &mut lp.shell.state,
            research: &mut lp.research,
            ports: &ports,
            context: &context,
        };
        let started = driver
            .step(
                ResearchInput::Poll(clock(CUT_OFF)),
                &mut core,
                &mut stream,
                &mut worker,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        let Stepped::Handed { requests, .. } = started else {
            panic!("nothing to hold yet");
        };
        let input = retrieved(&world, &requests);
        let mut outcomes = Vec::new();
        for _ in 0..2 {
            outcomes.push(
                driver
                    .step(input.clone(), &mut core, &mut stream, &mut worker)
                    .unwrap_or_else(|e| panic!("{e}")),
            );
            if matches!(outcomes.last(), Some(Stepped::Handed { .. })) {
                break;
            }
        }
        let rows: Vec<(String, String)> = stream
            .stored
            .iter()
            .map(|e| (e.event_id.0.clone(), e.event_type.clone()))
            .collect();
        (outcomes, rows, worker.0)
    };
    let (clean, clean_rows, clean_requests) = first_step(Vec::new());
    let (held, held_rows, held_requests) = first_step(vec![AppendFailure::Unresolved]);
    assert_eq!(clean.len(), 1);
    assert_eq!(held.first(), Some(&Stepped::Held));
    assert_eq!(
        held.last(),
        clean.last(),
        "the retry answers as a clean step does"
    );
    assert_eq!(held_rows, clean_rows, "the same rows, the same ids");
    assert_eq!(held_requests, clean_requests);
    assert!(
        clean_requests
            .iter()
            .any(|r| matches!(r, ResearchRequest::Call(_))),
        "the retrieval asked for the call: {clean_requests:?}"
    );
    assert!(!clean_rows.is_empty(), "the retrieval journaled its items");
}
