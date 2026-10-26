//! Research in the runtime process (DEC-2208): the research driver behind a deny-all default, over
//! the real in-memory journal, with a worker that records what it is handed and answers what a
//! test queues. The control and clock streams are rows a test lays down, as in `runtime.rs`.

mod research_rig;

use std::cell::Cell;
use std::collections::BTreeSet;
use std::path::Path;
use std::rc::Rc;

use mandate_canon::{Digest, Value, parse};
use mandate_daemon::cell;
use mandate_daemon::recorded::Bundle;
use mandate_daemon::worker::{RecordedWorker, WorkerFor};
use mandate_daemon::{ResearchParts, ResearchSource, ResearchWiring, RuntimeProcess, fixture};
use mandate_journal::StreamId;
use mandate_process::{Exit, State};
use mandate_research::AssetId;
use mandate_research_run::{ItemId, ItemKind, NewsItem, RetrievedItem, SourceId, Symbol};
use mandate_runtime::{
    ActiveThesis, EventTriggers, Mode, ResearchInput, ResearchRequest, RiskClock, RuntimeState,
};
use mandate_store::Store;

use research_rig::*;

/// DEC-2208 item 1: the binary's fixture wiring runs no research.
#[test]
fn the_fixture_wiring_runs_no_research() {
    assert!(
        fixture::wiring("user-owner", "agent:ws1:agent-a")
            .research
            .is_none()
    );
}

/// DEC-2208 item 1: with no research wiring, the process journals exactly what it journals without
/// one, ticks and the kill switch included.
#[test]
fn no_research_wiring_journals_what_a_process_without_research_journals() {
    let plain = Rig::default();
    let wired = Rig::default();
    let wires = Wired::default();
    let mut without = start(&plain, None);
    let mut with = start(&wired, Some(wires.wiring()));
    for rig in [&plain, &wired] {
        tick(rig, NOW + 1);
        kill_switch(rig);
        tick(rig, NOW + 2);
    }
    without.step(at(NOW + 1));
    with.step(at(NOW + 1));
    without.step(at(NOW + 2));
    with.step(at(NOW + 2));
    assert_eq!(kinds(&plain), kinds(&wired));
    assert_eq!(count(&wired, "KillSwitchActivated"), 1);
}

/// DEC-2208 item 3, DEC-1012 item 4: until the core's risk clock is set, the research pass asks
/// the worker nothing and hands nothing.
#[test]
fn no_tick_asks_the_worker_nothing() {
    let rig = Rig::default();
    let wires = Wired::default();
    let mut process = start(&rig, Some(wires.wiring()));
    wires.worker.queued.borrow_mut().push(stray());
    let alerts = process.step(at(NOW + 1));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert_eq!(wires.worker.asked.get(), 0);
    assert_eq!(wires.source.asked.get(), 0);
    assert_eq!(wires.worker.queued.borrow().len(), 1, "the answer waits");
}

/// The core's own gate: a runtime that has folded no clean reconciliation is paused, so a due tick
/// starts no research and the worker is handed nothing (research spec §5.3: research runs in
/// `normal` only, or `exits_only` for an agentic monitor).
#[test]
fn a_paused_runtime_starts_no_research_however_often_it_ticks() {
    let rig = Rig::default();
    let wires = Wired::default();
    let mut process = start(&rig, Some(wires.wiring()));
    let before = kinds(&rig);
    for second in 1..=3 {
        tick(&rig, NOW + second * 3_600);
        let alerts = process.step(at(NOW + second * 3_600));
        assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    }
    assert_eq!(process.state().effective_mode(), Mode::Paused);
    assert!(wires.source.asked.get() >= 3, "each pass reads the context");
    assert!(wires.worker.handed.borrow().is_empty());
    assert!(wires.shelf.put.borrow().is_empty());
    assert_eq!(kinds(&rig), before, "nothing journaled");
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 7: an answer the core refuses is dropped with an alert, the next pass does not
/// hand it again, and it stops nothing: the kill switch after it is journaled.
#[test]
fn a_refused_worker_answer_is_dropped_with_an_alert_and_stops_nothing() {
    let rig = Rig::default();
    let wires = Wired::default();
    let mut process = start(&rig, Some(wires.wiring()));
    tick(&rig, NOW + 1);
    wires.worker.queued.borrow_mut().extend([stray(), stray()]);
    let alerts = process.step(at(NOW + 1));
    assert_eq!(
        keys(&alerts),
        ["research_input_refused", "research_input_refused"],
        "each answer once, in arrival order"
    );
    assert_eq!(wires.worker.asked.get(), 1);
    let alerts = process.step(at(NOW + 2));
    assert!(alerts.is_empty(), "dropped, not kept: {:?}", keys(&alerts));
    kill_switch(&rig);
    process.step(at(NOW + 3));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// A source with nothing to give runs no research on that pass, and the worker's answers wait for
/// one that has: the answer is handed once the context is back, and only then.
#[test]
fn a_source_with_nothing_to_give_keeps_the_worker_s_answers_waiting() {
    struct Sometimes(Rc<Cell<bool>>);
    impl ResearchSource for Sometimes {
        fn parts(&self, state: &RuntimeState) -> Option<ResearchParts> {
            if self.0.get() {
                Open::default().parts(state)
            } else {
                None
            }
        }
    }
    let rig = Rig::default();
    let wires = Wired::default();
    let given = Rc::new(Cell::new(false));
    let mut research = wires.wiring();
    research.source = Box::new(Sometimes(Rc::clone(&given)));
    let mut process = start(&rig, Some(research));
    tick(&rig, NOW + 1);
    wires.worker.queued.borrow_mut().push(stray());
    for second in 1..=2 {
        let alerts = process.step(at(NOW + second));
        assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    }
    given.set(true);
    let alerts = process.step(at(NOW + 3));
    assert_eq!(keys(&alerts), ["research_input_refused"]);
    let alerts = process.step(at(NOW + 4));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 items 3 and 5, DEC-2206 items 3 and 4: a reconciled runtime's due tick starts an
/// agentic run. The data universe is stored before the batch that names it, `RunStarted` carries the
/// run as its `correlation_id`, and only then is the worker handed the start.
#[test]
fn a_due_tick_journals_run_started_under_its_run_with_the_universe_stored_first() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    rig.log.borrow_mut().clear();
    let alerts = process.step(at(NOW + 60));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    let started = run_started(&rig);
    assert_eq!(started.len(), 1, "{:?}", kinds(&rig));
    let run_ids = starts(&wired);
    assert_eq!(run_ids.len(), 1);
    assert_eq!(
        started[0].get("correlation_id").and_then(Value::as_str),
        Some(run_ids[0].as_str())
    );
    let universe_ref = started[0]
        .get("payload")
        .and_then(|p| p.get("data_universe_ref"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    assert!(
        wired
            .shelf
            .put
            .borrow()
            .iter()
            .any(|bytes| format!("sha256:{}", Digest::of(bytes)) == universe_ref),
        "the universe the record names was stored"
    );
    let log = rig.log.borrow().clone();
    let put = log.iter().position(|l| l == "put").unwrap();
    let append = log.iter().position(|l| l == "append research").unwrap();
    assert!(put < append, "{log:?}");
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 5: an artifact the sink cannot store appends nothing and halts research with an
/// alert, and holds nothing else: the kill switch laid in the same pass is journaled.
#[test]
fn an_artifact_the_sink_cannot_store_halts_research_and_holds_nothing_else() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    wired.shelf.refuse.set(true);
    tick(&rig, NOW + 60);
    let alerts = process.step(at(NOW + 60));
    assert_eq!(keys(&alerts), ["research_halted"]);
    assert!(run_started(&rig).is_empty());
    assert!(starts(&wired).is_empty());
    wired.shelf.refuse.set(false);
    kill_switch(&rig);
    tick(&rig, NOW + 7_200);
    process.step(at(NOW + 7_200));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert!(run_started(&rig).is_empty(), "research stays halted");
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 4, journal spec §5.1: a research batch whose answer was lost is in doubt; the next
/// pass offers it again before reading the control stream, it is written once, the worker is handed
/// the start only then, and the kill switch follows in the same pass.
#[test]
fn a_research_batch_in_doubt_is_retried_first_and_written_once() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert_eq!(run_started(&rig).len(), 1, "committed");
    assert!(starts(&wired).is_empty(), "but not known to be");
    kill_switch(&rig);
    rig.log.borrow_mut().clear();
    process.step(at(NOW + 61));
    let log = rig.log.borrow().clone();
    let retried = log.iter().position(|l| l == "append research").unwrap();
    let control = log.iter().position(|l| l == "read ctl:ws1").unwrap();
    assert!(retried < control, "{log:?}");
    assert_eq!(run_started(&rig).len(), 1, "written once");
    assert_eq!(starts(&wired).len(), 1);
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
}

/// DEC-2208 item 6: a research batch the journal refuses as invalid halts research with an alert,
/// and trading goes on: the kill switch is journaled, and no later tick starts a run.
#[test]
fn an_invalid_research_batch_halts_research_and_the_kill_switch_goes_on() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    reconciled(&rig);
    let mut process = running_with(&rig, &wired, Box::new(MandateRef));
    tick(&rig, NOW + 60);
    let alerts = process.step(at(NOW + 60));
    assert_eq!(keys(&alerts), ["research_halted"], "{:?}", kinds(&rig));
    kill_switch(&rig);
    tick(&rig, NOW + 7_200);
    process.step(at(NOW + 7_200));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert!(run_started(&rig).is_empty());
    assert!(starts(&wired).is_empty());
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 6: a research append fenced by a newer writer stops the process `Fenced`.
#[test]
fn a_fenced_research_append_stops_the_process_fenced() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::Fenced));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert_eq!(process.lifecycle().state(), State::Exited(Exit::Fenced));
    assert!(starts(&wired).is_empty());
}

/// DEC-2208 item 6: a research append whose head no longer matches stops the process `Failed`.
#[test]
fn a_research_append_at_a_stale_head_stops_the_process() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::HeadMismatch));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    assert!(starts(&wired).is_empty());
}

/// DEC-2208 item 3: the worker's seed for the run reaches the core on a later pass, and the core's
/// next request, the first turn, is handed after its batch.
#[test]
fn the_worker_s_seed_reaches_the_core_and_the_first_turn_follows() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    let run_id = starts(&wired).pop().unwrap();
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Retrieved {
            run_id,
            items: Vec::new(),
        });
    let alerts = process.step(at(NOW + 61));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert!(
        wired
            .worker
            .handed
            .borrow()
            .iter()
            .any(|request| matches!(request, ResearchRequest::Turn(_))),
        "{:?}",
        wired.worker.handed.borrow()
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 4: the retry of a research batch in doubt offers the core the same input, which it
/// answers from the batch it remembers, so a source with nothing to give on that pass holds nothing:
/// the batch is settled and the kill switch is journaled in the same pass.
#[test]
fn a_source_with_nothing_never_holds_the_retry_of_a_batch_in_doubt() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert!(starts(&wired).is_empty(), "in doubt");
    wired.source.off.set(true);
    kill_switch(&rig);
    process.step(at(NOW + 61));
    assert_eq!(starts(&wired).len(), 1, "settled");
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(run_started(&rig).len(), 1);
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 5: an artifact the sink cannot store on the retry of a batch that may already be
/// committed leaves the batch in doubt rather than abandoning it, so the core's head never goes
/// stale; once the sink is back the batch settles, written once, and the process stays ready.
#[test]
fn an_unstored_artifact_on_a_retry_keeps_the_batch_in_doubt() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    wired.shelf.refuse.set(true);
    let alerts = process.step(at(NOW + 61));
    assert!(alerts.is_empty(), "not halted: {:?}", keys(&alerts));
    assert!(starts(&wired).is_empty(), "still in doubt");
    wired.shelf.refuse.set(false);
    kill_switch(&rig);
    process.step(at(NOW + 62));
    assert_eq!(starts(&wired).len(), 1, "settled");
    assert_eq!(run_started(&rig).len(), 1, "written once");
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 3: an answer queued behind a batch that goes into doubt waits in the inbox, and is
/// handed once the batch settles, on the next pass, never dropped.
#[test]
fn an_answer_queued_behind_a_batch_in_doubt_waits_for_it() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Retrieved {
            run_id: "01J9ZZZZZZZZZZZZZZZZZZZZZY".to_owned(),
            items: Vec::new(),
        });
    tick(&rig, NOW + 60);
    let alerts = process.step(at(NOW + 60));
    assert!(alerts.is_empty(), "behind the doubt: {:?}", keys(&alerts));
    let alerts = process.step(at(NOW + 61));
    assert_eq!(
        keys(&alerts),
        ["research_input_refused"],
        "handed once settled"
    );
    assert_eq!(starts(&wired).len(), 1);
    let alerts = process.step(at(NOW + 62));
    assert!(alerts.is_empty(), "handed once: {:?}", keys(&alerts));
}

/// DEC-2208 item 4, journal spec §5.1: a research batch still in doubt after its retry holds every
/// other input, the kill switch among them, since the agent stream has one writer; the process stays
/// ready, and once the journal answers the batch settles and the kill switch follows.
#[test]
fn a_research_batch_still_in_doubt_holds_the_pass_until_it_settles() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    rig.fault.set(Some(Fault::Down));
    kill_switch(&rig);
    process.step(at(NOW + 61));
    assert_eq!(
        count(&rig, "KillSwitchActivated"),
        0,
        "held behind the doubt"
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
    process.step(at(NOW + 62));
    assert_eq!(starts(&wired).len(), 1);
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 3: an answer whose own batch goes into doubt is the driver's to retry, never the
/// inbox's to hand again: once its retry settles, the run has ended once and nothing is refused.
#[test]
fn an_answer_whose_batch_goes_into_doubt_is_retried_never_handed_twice() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    let run_id = starts(&wired).pop().unwrap();
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items: Vec::new(),
        });
    process.step(at(NOW + 61));
    let turn = wired
        .worker
        .handed
        .borrow()
        .iter()
        .find_map(|request| match request {
            ResearchRequest::Turn(turn) => Some(turn.clone()),
            _ => None,
        })
        .unwrap();
    rig.fault.set(Some(Fault::LoseAnswer));
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Called {
            run_id,
            at: RiskClock::from_secs(NOW + 60),
            record: Box::new(unavailable(&turn)),
        });
    let alerts = process.step(at(NOW + 62));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert_eq!(count(&rig, "RunEnded"), 1, "committed: {:?}", kinds(&rig));
    for second in 63..66 {
        let alerts = process.step(at(NOW + second));
        assert!(alerts.is_empty(), "handed once: {:?}", keys(&alerts));
    }
    assert_eq!(count(&rig, "RunEnded"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2210 item 3 and `AGENTS.md` rule 4: the cell's worker, wired into the process, takes a run
/// through the recorded fake to its judgment, and nothing the model said reaches the order path. The
/// run journals its call and the model's thesis as a proposal, and no stream holds an order event.
#[test]
fn the_recorded_worker_s_answers_never_reach_the_order_path() {
    let rig = Rig::default();
    reconciled(&rig);
    established(&rig);
    let bundle = Bundle::load(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/research"
    )))
    .unwrap();
    let pin = bundle.entry.content().model_ref();
    let mut entry = config_entry();
    entry.pin = pin.clone();
    let clock = Rc::new(Cell::new(NOW));
    let read = Rc::clone(&clock);
    let who = WorkerFor {
        workspace: WORKSPACE.to_owned(),
        agent: AGENT.to_owned(),
        description: entry.description.clone(),
    };
    let worker = RecordedWorker::new(
        &bundle,
        who,
        Box::new(Shelf::default()),
        Box::new(move || at(read.get())),
    )
    .unwrap();
    let research = ResearchWiring {
        config: entry,
        source: Box::new(Open::default()),
        worker: Box::new(worker),
        artifacts: Box::new(Shelf::default()),
    };
    let mut process = RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(research), Box::new(BundleRefs(pin.content_hash))),
        &config(),
        at(NOW),
    );
    for second in 60..70 {
        tick(&rig, NOW + second);
        clock.set(NOW + second);
        process.step(at(NOW + second));
    }
    assert_eq!(
        count(&rig, "ModelInvocationRecorded"),
        1,
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(count(&rig, "ThesisProposed"), 1, "{:?}", kinds(&rig));
    for stream in [AGENT_STREAM, ACCOUNT_STREAM, CONTROL] {
        let rows = rig.journal.rows(&StreamId::parse(stream).unwrap()).unwrap();
        let orders: Vec<_> = rows
            .iter()
            .map(|row| row.event_type.as_str())
            .filter(|kind| kind.starts_with("Order"))
            .collect();
        assert!(orders.is_empty(), "{stream}: {orders:?}");
    }
}

/// DEC-2268 item 5, agent harness spec §6.9 and HI-24, and `AGENTS.md` rule 4: the cell's worker
/// over an agentic bundle, wired into the process with the bundle's loop, runs the scripted turns
/// to the final output. Every turn is journaled: each reply's `ModelInvocationRecorded` heads its
/// calls' `ToolCallRecorded`, each call's `ToolResultRecorded` follows before the next turn, and the
/// run ends `done` after the final reply is judged. No stream holds an order event.
#[test]
fn an_agentic_run_journals_every_turn_through_to_its_end() {
    let rig = Rig::default();
    reconciled(&rig);
    established(&rig);
    let bundle = Bundle::load(Path::new(AGENTIC_BUNDLE)).unwrap();
    let pin = bundle.entry.content().model_ref();
    let mut entry = config_entry();
    entry.pin = pin.clone();
    entry.agentic = Some(cell::agentic_config(&bundle).unwrap_or_else(|e| panic!("{e:?}")));
    let clock = Rc::new(Cell::new(NOW));
    let read = Rc::clone(&clock);
    let who = WorkerFor {
        workspace: WORKSPACE.to_owned(),
        agent: AGENT.to_owned(),
        description: entry.description.clone(),
    };
    let shelf = Shelf::default();
    let worker = RecordedWorker::new(
        &bundle,
        who,
        Box::new(shelf.clone()),
        Box::new(move || at(read.get())),
    )
    .unwrap();
    let research = ResearchWiring {
        config: entry,
        source: Box::new(Open::default()),
        worker: Box::new(worker),
        artifacts: Box::new(shelf),
    };
    let mut process = RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(research), Box::new(BundleRefs(pin.content_hash))),
        &config(),
        at(NOW),
    );
    for second in 60..90 {
        tick(&rig, NOW + second);
        clock.set(NOW + second);
        process.step(at(NOW + second));
    }
    let (scripted, calls) = scripted_calls();
    let journaled = kinds(&rig);
    let run: Vec<&str> = journaled
        .iter()
        .map(String::as_str)
        .filter(|kind| {
            matches!(
                *kind,
                "RunStarted"
                    | "ModelInvocationRecorded"
                    | "ToolCallRecorded"
                    | "ToolResultRecorded"
                    | "RunSummaryRecorded"
                    | "RunEnded"
            )
        })
        .collect();
    assert_eq!(run.first(), Some(&"RunStarted"), "{journaled:?}");
    assert_eq!(
        count(&rig, "ModelInvocationRecorded"),
        scripted + 1,
        "every scripted turn and the final one: {journaled:?}"
    );
    assert_eq!(count(&rig, "ToolCallRecorded"), calls, "{journaled:?}");
    assert_eq!(count(&rig, "ToolResultRecorded"), calls, "{journaled:?}");
    let invocations: Vec<usize> = run
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == "ModelInvocationRecorded")
        .map(|(index, _)| index)
        .collect();
    for pair in invocations.windows(2) {
        let between = &run[pair[0] + 1..pair[1]];
        let asked = between.iter().filter(|k| **k == "ToolCallRecorded").count();
        let answered = between
            .iter()
            .filter(|k| **k == "ToolResultRecorded")
            .count();
        assert!(asked > 0, "a scripted turn asks for calls: {run:?}");
        assert_eq!(
            asked, answered,
            "every call answered before the next turn: {run:?}"
        );
    }
    assert_eq!(
        run[run.len() - 2..],
        ["RunSummaryRecorded", "RunEnded"],
        "{run:?}"
    );
    let ended = rig
        .journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .into_iter()
        .rfind(|row| row.event_type == "RunEnded")
        .unwrap();
    let body = parse(&ended.body).unwrap();
    assert_eq!(
        body.get("payload")
            .and_then(|payload| payload.get("reason"))
            .and_then(Value::as_str),
        Some("done"),
        "{body:?}"
    );
    for stream in [AGENT_STREAM, ACCOUNT_STREAM, CONTROL] {
        let rows = rig.journal.rows(&StreamId::parse(stream).unwrap()).unwrap();
        let orders: Vec<_> = rows
            .iter()
            .map(|row| row.event_type.as_str())
            .filter(|kind| kind.starts_with("Order"))
            .collect();
        assert!(orders.is_empty(), "{stream}: {orders:?}");
    }
}

/// DEC-2246 item 4: the flag only makes the recorded bundle available; the confirmed envelope
/// decides research. `demo-spy`'s mandate pins its universe and has no `behavior.research`, so
/// opted in over the demo bundle it starts no run and journals no research record, so nothing is
/// called, metered or spent, however long it runs. The same opt-in under an envelope that
/// researches (unpinned, a `behavior.research`, and one `llm.` model admitting instruments, as
/// V-036 asks) starts runs and journals their records, so the first answer is the envelope's, not
/// the harness's.
#[test]
fn the_demo_agent_opted_in_runs_no_research_and_spends_nothing() {
    let rig = Rig::default();
    let research = opted_in(&rig, "spy", |_| {});
    assert_eq!(
        a_day_of_research(&rig, research),
        (0, 0),
        "{:?}",
        kinds(&rig)
    );
    let researching = Rig::default();
    let research = opted_in(&researching, "researching", |document| {
        document["universe"]["pinned"] = serde_json::json!(false);
        document["universe"]["pinned_instruments"] = serde_json::json!([]);
        document["behavior"]["signal_models"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "llm.research_agent",
                "version": "0.1.0",
                "content_hash": format!("sha256:{}", "4".repeat(64)),
                "params": [],
                "weight": "0.5",
                "max_output_age_s": 3600,
                "admits_instruments": true
            }));
        document["behavior"]["research"] = serde_json::json!({
            "interval_s": 3600,
            "cost_cap_usd_per_day": "5",
            "max_revisions_per_lineage": 3
        });
    });
    let (starts, records) = a_day_of_research(&researching, research);
    assert!(
        starts > 0 && records > 0,
        "{starts} {records} {:?}",
        kinds(&researching)
    );
}

/// DEC-2316 item 1: a run may take every turn its loop allows. The cell's worker over the agentic
/// bundle with seven scripted turns, each one note, and the final reply runs the `v0_2` loop's eight
/// turns within one minute, each answered `ok`, and ends `done`. The cell's gateway rate cap is
/// derived from the pinned loop, so it never cuts a run inside its own bounds.
#[test]
fn an_agentic_run_takes_every_turn_its_loop_allows_and_ends_done() {
    let rig = Rig::default();
    reconciled(&rig);
    established(&rig);
    let mut bundle = Bundle::load(Path::new(AGENTIC_BUNDLE)).unwrap();
    let scripted = usize::try_from(bundle.entry.agentic.unwrap().max_turns - 1).unwrap();
    let note = bundle.turns[1].clone();
    while bundle.turns.len() < scripted {
        let mut next = note.clone();
        next.calls = vec![(
            "note".to_owned(),
            format!(r#"{{"text":"turn {} reads on"}}"#, bundle.turns.len() + 1),
        )];
        bundle.turns.push(next);
    }
    let pin = bundle.entry.content().model_ref();
    let mut entry = config_entry();
    entry.pin = pin.clone();
    entry.agentic = Some(cell::agentic_config(&bundle).unwrap_or_else(|e| panic!("{e:?}")));
    let clock = Rc::new(Cell::new(NOW));
    let read = Rc::clone(&clock);
    let who = WorkerFor {
        workspace: WORKSPACE.to_owned(),
        agent: AGENT.to_owned(),
        description: entry.description.clone(),
    };
    let shelf = Shelf::default();
    let worker = RecordedWorker::new(
        &bundle,
        who,
        Box::new(shelf.clone()),
        Box::new(move || at(read.get())),
    )
    .unwrap();
    let research = ResearchWiring {
        config: entry,
        source: Box::new(Open::default()),
        worker: Box::new(worker),
        artifacts: Box::new(shelf),
    };
    let mut process = RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(research), Box::new(BundleRefs(pin.content_hash))),
        &config(),
        at(NOW),
    );
    for second in 60..120 {
        tick(&rig, NOW + second);
        clock.set(NOW + second);
        process.step(at(NOW + second));
    }
    let rows = rig
        .journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap();
    let payloads = |kind: &str| -> Vec<Value> {
        rows.iter()
            .filter(|row| row.event_type == kind)
            .map(|row| parse(&row.body).unwrap())
            .map(|body| body.get("payload").unwrap().clone())
            .collect()
    };
    let outcomes: Vec<String> = payloads("ModelInvocationRecorded")
        .iter()
        .map(|payload| format!("{:?}", payload.get("outcome")))
        .collect();
    assert_eq!(outcomes.len(), scripted + 1, "every turn: {outcomes:?}");
    assert!(
        outcomes.iter().all(|outcome| outcome.contains("\"ok\"")),
        "{outcomes:?}"
    );
    let ends = payloads("RunEnded");
    let [end] = ends.as_slice() else {
        panic!("one end: {ends:?}");
    };
    assert_eq!(
        format!("{:?}", end.get("reason")),
        format!("{:?}", Some(&Value::Str("done".to_owned()))),
        "{end:?}"
    );
}

/// A news item about AAPL, arrived at `arrived`.
fn aapl_news(arrived: i64) -> RetrievedItem {
    RetrievedItem::News(NewsItem {
        item_id: ItemId::new("nw-feed-aapl").unwrap(),
        source: SourceId::new("src.newswire").unwrap(),
        symbols: BTreeSet::from([Symbol::new("AAPL").unwrap()]),
        knowledge_at: at(arrived),
        published_at: at(arrived),
        headline: "AAPL guidance withdrawn".to_owned(),
        summary: "The company withdrew its full-year guidance.".to_owned(),
        link: "https://news.example/nw-feed-aapl".to_owned(),
        content_hash: Digest::of(b"AAPL guidance withdrawn"),
        licensed: false,
    })
}

/// News triggers, the AAPL item arrived a minute before `NOW`, and an active thesis on AAPL.
fn evented(wired: &Wired) {
    *wired.source.evented.borrow_mut() = Some(Evented {
        triggers: Some(EventTriggers {
            kinds: BTreeSet::from([ItemKind::News]),
            debounce_s: 900,
            max_runs_per_day: 4,
        }),
        events: vec![aapl_news(NOW - 60)],
        active: vec![ActiveThesis {
            thesis_id: "01ACTIVE".to_owned(),
            lineage_id: "01ACTIVE".to_owned(),
            revision: 0,
            instrument: AssetId::new(AAPL).unwrap(),
            symbol: Symbol::new("AAPL").unwrap(),
            expires_at: RiskClock::from_secs(NOW + 86_400 * 30),
        }],
    });
}

fn stored(rig: &Rig, event_type: &str) -> Vec<Value> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .iter()
        .filter(|row| row.event_type == event_type)
        .map(|row| parse(&row.body).unwrap())
        .collect()
}

/// E19-12 in the cell (DEC-2392): an arrived item on a thesis's instrument, offered by the pass
/// before its scheduled poll, is stored, journaled as the observation, and brings a `monitor` run
/// forward whose `RunStarted` names it; the worker is handed that one start, and a later pass
/// offers the item no more.
#[test]
fn an_arrived_item_on_a_thesis_brings_a_monitor_run_forward() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    evented(&wired);
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    rig.log.borrow_mut().clear();
    let alerts = process.step(at(NOW + 60));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    let observed = stored(&rig, "ObservationRecorded");
    assert_eq!(observed.len(), 1, "{:?}", kinds(&rig));
    let data_ref = observed[0]
        .get("payload")
        .and_then(|p| p.get("data_ref"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    assert!(
        wired
            .shelf
            .put
            .borrow()
            .iter()
            .any(|bytes| format!("sha256:{}", Digest::of(bytes)) == data_ref),
        "the item's record was stored"
    );
    let log = rig.log.borrow().clone();
    let put = log.iter().position(|l| l == "put").unwrap();
    let append = log.iter().position(|l| l == "append research").unwrap();
    assert!(put < append, "{log:?}");
    let started = run_started(&rig);
    assert_eq!(started.len(), 1, "{:?}", kinds(&rig));
    let payload = started[0].get("payload").unwrap();
    assert_eq!(payload.get("kind").and_then(Value::as_str), Some("monitor"));
    let trigger = payload.get("trigger").unwrap();
    assert_eq!(trigger.get("kind").and_then(Value::as_str), Some("event"));
    assert_eq!(
        trigger.get("cause").and_then(Value::as_str),
        observed[0].get("event_id").and_then(Value::as_str)
    );
    assert_eq!(starts(&wired).len(), 1);
    tick(&rig, NOW + 120);
    let puts = wired.shelf.put.borrow().len();
    assert!(process.step(at(NOW + 120)).is_empty());
    assert_eq!(stored(&rig, "ObservationRecorded").len(), 1);
    assert_eq!(
        wired.shelf.put.borrow().len(),
        puts,
        "offered once a process"
    );
    assert_eq!(starts(&wired).len(), 1);
}

/// Without event triggers in the version, the same arrived item is neither stored nor journaled,
/// and the pass goes on to its scheduled run.
#[test]
fn without_event_triggers_an_arrived_item_is_neither_stored_nor_journaled() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    evented(&wired);
    if let Some(evented) = wired.source.evented.borrow_mut().as_mut() {
        evented.triggers = None;
    }
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    assert!(process.step(at(NOW + 60)).is_empty());
    assert!(stored(&rig, "ObservationRecorded").is_empty());
    let started = run_started(&rig);
    assert_eq!(started.len(), 1, "{:?}", kinds(&rig));
    assert_eq!(
        started[0]
            .get("payload")
            .and_then(|p| p.get("trigger"))
            .and_then(|t| t.get("kind"))
            .and_then(Value::as_str),
        Some("schedule")
    );
}
