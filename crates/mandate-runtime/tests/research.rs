//! The research loop in the core (agent harness spec §5.3, §5.4, §6.2, §6.5, §7): when a run
//! starts and ends, what is journaled before the model reads anything, and how a model's answer is
//! checked and judged. Each case names the rule it holds.

mod common;

use common::research::{
    AAPL, CUT_OFF, INTERVAL_S, Loop, MSFT, Model, RESEARCH_MODEL, RESPONSE_AAPL, RecordedPlane,
    World, failing, full_run, pin, production_preconditions, record_for, recorded, refused_run,
    worker_run,
};
use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, TestIds, clock, event, ports, text, universe, with_clock,
};
use mandate_canon::Value;
use mandate_gateway::{CallRecord, Outcome, RequestError};
use mandate_journal::StreamType;
use mandate_research::{AssetId, WorkingUniverse};
use mandate_research_run::{CallPlan, ModelPin, Retrieval};
use mandate_runtime::{
    ActiveThesis, Command, Effect, FoldedEvent, Input, JournalCatalog, Mode, RecordCatalog,
    ResearchInput, ResearchRecord, ResearchRequest, RiskClock, RuntimeError, WriterEpoch, handle,
};

fn boot(lp: &mut Loop, ports: &mandate_runtime::Ports<'_>) {
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

fn starts(step: &mandate_runtime::ResearchStep) -> Vec<(String, RiskClock)> {
    step.requests
        .iter()
        .filter_map(|request| match request {
            ResearchRequest::Start { run_id, cut_off } => Some((run_id.clone(), *cut_off)),
            _ => None,
        })
        .collect()
}

fn cancels(step: &mandate_runtime::ResearchStep) -> Vec<String> {
    step.requests
        .iter()
        .filter_map(|request| match request {
            ResearchRequest::Cancel { run_id } => Some(run_id.clone()),
            _ => None,
        })
        .collect()
}

fn payload_text<'a>(value: &'a Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(Value::as_str)
}

struct Fixture {
    ids: TestIds,
    plan: FixedPlan,
    view: mandate_runtime::MandateView,
}

impl Fixture {
    fn new() -> Self {
        Self {
            ids: TestIds,
            plan: FixedPlan::silent(),
            view: universe(&[]),
        }
    }
}

/// Agent harness spec §6.2: with both records registered (E15-8, E19-5), the production catalogue
/// lets a due run start; a catalogue missing either still keeps every run from starting.
#[test]
fn a_run_starts_in_production_once_the_journal_records_both_verdicts() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    world.preconditions = production_preconditions();
    assert!(
        world.preconditions.hold(),
        "mandate-journal registers both records"
    );
    let step = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert!(
        matches!(step.requests.first(), Some(ResearchRequest::Start { .. })),
        "§6.2: the due run starts: {:?}",
        step.requests
    );
}

/// The shell's catalogue reads `mandate-journal`'s own registration, never a constant: §9.19 closes
/// the agent stream's `ModelInvocationRecorded` with its `verdicts` (E15-8, DEC-2201), and §9.22
/// closes `ThesisInvalidated` (E19-5, DEC-2202).
#[test]
fn the_journal_catalog_reads_mandate_journal_s_registration() {
    let journal = JournalCatalog::JOURNAL;
    assert!(journal.registers(ResearchRecord::CandidateVerdicts));
    assert!(journal.registers(ResearchRecord::InvalidationVerdict));
    let calls_only = JournalCatalog::over(|event_type, version, stream| {
        (event_type, version, stream) == ("ModelInvocationRecorded", 1, StreamType::Agent)
    });
    assert!(calls_only.registers(ResearchRecord::CandidateVerdicts));
    assert!(!calls_only.registers(ResearchRecord::InvalidationVerdict));
    let invalidations_only = JournalCatalog::over(|event_type, version, stream| {
        (event_type, version, stream) == ("ThesisInvalidated", 1, StreamType::Agent)
    });
    assert!(!invalidations_only.registers(ResearchRecord::CandidateVerdicts));
    assert!(invalidations_only.registers(ResearchRecord::InvalidationVerdict));
}

#[test]
fn preconditions_hold_only_when_both_records_are_registered() {
    struct Only(mandate_runtime::ResearchRecord);
    impl mandate_runtime::RecordCatalog for Only {
        fn registers(&self, record: mandate_runtime::ResearchRecord) -> bool {
            record == self.0
        }
    }
    assert!(common::research::open_preconditions().hold());
    for record in [
        mandate_runtime::ResearchRecord::InvalidationVerdict,
        mandate_runtime::ResearchRecord::CandidateVerdicts,
    ] {
        assert!(!mandate_runtime::Preconditions::read(&Only(record)).hold());
    }
}

#[test]
fn a_due_run_starts_at_the_risk_clock_and_journals_nothing() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let before = lp.shell.agent_journal.len();
    let step = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    let started = starts(&step);
    assert_eq!(started.len(), 1);
    assert_eq!(
        started[0].1,
        clock(CUT_OFF),
        "the cut-off is the start's risk clock"
    );
    assert!(
        step.effects.is_empty(),
        "§6.2 step 1: nothing is journaled yet"
    );
    assert_eq!(lp.shell.agent_journal.len(), before);
    assert_eq!(lp.research.in_flight(), Some(started[0].0.as_str()));
}

#[test]
fn at_most_one_run_is_in_flight() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let first = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert_eq!(starts(&first).len(), 1);
    let second = lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
    assert!(starts(&second).is_empty(), "HI-8: one run in flight");
}

#[test]
fn run_ids_never_repeat_within_a_process() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    let first = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    world.config = None;
    lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
    world.config = Some(common::research::config());
    let second = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF + 2)), &ports, &world));
    assert_eq!(
        second.len(),
        1,
        "a cancelled run that never called does not hold the interval"
    );
    assert_ne!(first[0].0, second[0].0);
}

#[test]
fn the_interval_counts_from_the_last_call() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    let gateway = failing(Outcome::ProviderUnavailable);
    worker_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &gateway,
        clock(CUT_OFF + 30),
    );
    let called = CUT_OFF + 30;
    assert_eq!(lp.research.last_call(), Some(clock(called)));
    let early = lp.run(
        ResearchInput::Poll(clock(called + i64::from(INTERVAL_S) - 1)),
        &ports,
        &world,
    );
    assert!(
        starts(&early).is_empty(),
        "HI-8: not before next_proposal_at"
    );
    let due = lp.run(
        ResearchInput::Poll(clock(called + i64::from(INTERVAL_S))),
        &ports,
        &world,
    );
    assert_eq!(starts(&due).len(), 1);
}

#[test]
fn a_later_reservation_pushes_the_next_run_back() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    world.last_reservation = Some(clock(CUT_OFF - 100));
    let early = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert!(
        starts(&early).is_empty(),
        "§5.3: a crash loop cannot call faster than the interval"
    );
    let due = lp.run(
        ResearchInput::Poll(clock(CUT_OFF - 100 + i64::from(INTERVAL_S))),
        &ports,
        &world,
    );
    assert_eq!(starts(&due).len(), 1);
}

#[test]
fn the_policy_floor_lengthens_the_interval() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    world.overlay.research_interval_s = Some(7_200);
    world.last_reservation = Some(clock(CUT_OFF - 4_000));
    let early = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert!(starts(&early).is_empty());
    let due = lp.run(ResearchInput::Poll(clock(CUT_OFF + 3_200)), &ports, &world);
    assert_eq!(starts(&due).len(), 1);
}

#[test]
fn no_run_starts_unless_the_mode_is_normal() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    lp.shell.run(Input::Started(WriterEpoch(1)), &ports);
    let world = World::open();
    let held = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert!(
        starts(&held).is_empty(),
        "HI-15: the startup hold is paused"
    );
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    lp.shell.run(Input::Command(Command::Pause), &ports);
    let paused = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert!(starts(&paused).is_empty(), "HI-15: paused");
}

/// DEC-2205: until the agentic `monitor` run, no `single_call` run starts in `exits_only`; skipping
/// it only delays a removal.
#[test]
fn exits_only_starts_no_single_call_run() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
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
    assert_eq!(lp.shell.state.effective_mode(), Mode::ExitsOnly);
    let world = World::open();
    let step = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert!(starts(&step).is_empty());
}

#[test]
fn a_pause_cancels_the_run_in_flight_and_no_call_follows() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    lp.shell.run(Input::Command(Command::Pause), &ports);
    let polled = lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
    assert_eq!(cancels(&polled), vec![started[0].0.clone()]);
    assert_eq!(lp.research.in_flight(), None);
    let plane = RecordedPlane::load();
    let retrieval = mandate_research_run::retrieve(
        &common::research::config().plan,
        &world.scope(clock(CUT_OFF)),
        plane.readers(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let items = retrieval
        .items
        .iter()
        .map(|item| {
            mandate_runtime::RetrievedObservation::capture(item, clock(CUT_OFF))
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        })
        .collect();
    let late = lp.run(
        ResearchInput::Retrieved {
            run_id: started[0].0.clone(),
            items,
        },
        &ports,
        &world,
    );
    assert!(
        late.effects.is_empty(),
        "a cancelled run's items are never shown, so never journaled"
    );
    assert!(late.requests.is_empty(), "HI-15: no call while paused");
}

#[test]
fn a_pause_between_retrieval_and_the_call_journals_nothing_and_calls_nothing() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    lp.shell.run(Input::Command(Command::Pause), &ports);
    let plane = RecordedPlane::load();
    let retrieval = mandate_research_run::retrieve(
        &common::research::config().plan,
        &world.scope(clock(CUT_OFF)),
        plane.readers(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let items = retrieval
        .items
        .iter()
        .map(|item| {
            mandate_runtime::RetrievedObservation::capture(item, clock(CUT_OFF))
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        })
        .collect();
    let step = lp.run(
        ResearchInput::Retrieved {
            run_id: started[0].0.clone(),
            items,
        },
        &ports,
        &world,
    );
    assert!(step.effects.is_empty());
    assert_eq!(cancels(&step), vec![started[0].0.clone()]);
}

#[test]
fn every_item_is_journaled_before_the_call_is_requested() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    let gateway = recorded(RESPONSE_AAPL);
    let requests = worker_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &gateway,
        clock(CUT_OFF + 30),
    );
    let observed = lp.events("ObservationRecorded");
    assert!(!observed.is_empty());
    let Some(ResearchRequest::Call(plan)) = requests
        .iter()
        .find(|request| matches!(request, ResearchRequest::Call(_)))
    else {
        panic!("the run calls once");
    };
    let ids: Vec<String> = observed
        .iter()
        .map(|event| event.event_id.0.clone())
        .collect();
    assert_eq!(
        plan.observations, ids,
        "HI-2: the call names exactly the journaled items"
    );
    let retrieved_step = &lp.steps[1];
    let last_draft = retrieved_step
        .effects
        .iter()
        .rposition(|effect| matches!(effect, Effect::Journal(_)));
    assert_eq!(
        last_draft,
        Some(retrieved_step.effects.len() - 1),
        "every draft is in the effect list the call request follows"
    );
    for event in &observed {
        assert_eq!(
            payload_text(&event.payload, "as_of"),
            Some("2026-10-16T20:00:00.000000000Z"),
            "every observation's as_of is the run's cut-off"
        );
        assert_ne!(
            payload_text(&event.payload, "source"),
            Some("src.anonymous_blog"),
            "DEC-101: the source off the allowlist is never journaled or sent"
        );
    }
    let records = lp.events("ModelInvocationRecorded");
    assert_eq!(records.len(), 1, "HI-8: one call per run");
    let model = records[0]
        .payload
        .get("model")
        .unwrap_or_else(|| panic!("the record names its model"));
    assert_eq!(
        payload_text(model, "model_id"),
        Some(pin().model_id.as_str())
    );
    assert_eq!(
        payload_text(model, "content_hash"),
        Some(format!("sha256:{}", pin().content_hash).as_str())
    );
    let named: Vec<&str> = records[0]
        .payload
        .get("observations")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let mut sorted = ids.clone();
    sorted.sort();
    assert_eq!(
        named, sorted,
        "rule 150: the retrieved context is the journaled observations, ascending"
    );
    let retrieval = mandate_research_run::retrieve(
        &common::research::config().plan,
        &world.scope(clock(CUT_OFF)),
        plane.readers(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(retrieval.items.len(), ids.len());
    assert!(
        retrieval.items.iter().all(|item| item
            .item_id()
            .is_ok_and(|id| id.as_str() != "nw-20261017-aapl-late")),
        "HI-13: the item known after the cut-off is never sent"
    );
}

#[test]
fn an_empty_retrieval_ends_the_run_without_a_call() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    let step = lp.run(
        ResearchInput::Retrieved {
            run_id: started[0].0.clone(),
            items: Vec::new(),
        },
        &ports,
        &world,
    );
    assert!(step.requests.is_empty());
    assert!(step.effects.is_empty());
    assert_eq!(lp.research.in_flight(), None);
}

#[test]
fn a_failed_call_is_journaled_and_ends_the_run_with_no_output() {
    for kind in Outcome::ALL.into_iter().filter(|kind| *kind != Outcome::Ok) {
        let f = Fixture::new();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let world = World::open();
        let plane = RecordedPlane::load();
        full_run(
            &mut lp,
            clock(CUT_OFF),
            &ports,
            &world,
            &plane,
            &failing(kind),
        );
        let records = lp.events("ModelInvocationRecorded");
        assert_eq!(records.len(), 1, "{}", kind.code());
        assert_eq!(
            payload_text(&records[0].payload, "outcome"),
            Some(kind.code())
        );
        assert_eq!(
            payload_text(&records[0].payload, "purpose"),
            Some("research")
        );
        assert_eq!(
            records[0].payload.get("verdicts"),
            Some(&Value::Array(Vec::new())),
            "rule 151: no verdicts without an ok"
        );
        assert!(
            lp.events("ModelOutputRecorded").is_empty(),
            "HI-6: {}",
            kind.code()
        );
        assert!(lp.events("ThesisProposed").is_empty());
        assert_eq!(lp.research.in_flight(), None);
    }
}

#[test]
fn an_operator_alert_is_opaque_and_follows_the_call_record() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &failing(Outcome::PolicyDenied),
    );
    let called = &lp.steps[2];
    let record = lp.events("ModelInvocationRecorded")[0].event_id.clone();
    let alerts: Vec<_> = called
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Notify(reference) => Some(reference.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(alerts.len(), 1);
    assert_eq!(alerts[0].kind.key(), "research_call_refused");
    assert_eq!(alerts[0].subject_event, record);
}

#[test]
fn three_deadline_misses_in_a_row_alert_the_operator_once() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    let gateway = failing(Outcome::DeadlineExceeded);
    let mut alerts = Vec::new();
    for run in 0..4 {
        let now = CUT_OFF + run * (i64::from(INTERVAL_S) + 300);
        full_run(&mut lp, clock(now), &ports, &world, &plane, &gateway);
        let called = &lp.steps[lp.steps.len() - 2];
        alerts.push(
            called
                .effects
                .iter()
                .filter(|effect| matches!(effect, Effect::Notify(r) if r.kind.key() == "research_deadline_streak"))
                .count(),
        );
    }
    assert_eq!(alerts, vec![0, 0, 1, 0]);
}

#[test]
fn a_withdrawn_model_disables_research_for_its_pin() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &failing(Outcome::ModelWithdrawn),
    );
    let later = clock(CUT_OFF + 2 * i64::from(INTERVAL_S));
    assert!(starts(&lp.run(ResearchInput::Poll(later), &ports, &world)).is_empty());
    if let Some(config) = world.config.as_mut() {
        config.pin.model_version = "0.2.0".to_owned();
    }
    let repinned = lp.run(ResearchInput::Poll(clock(later.secs() + 1)), &ports, &world);
    assert_eq!(
        starts(&repinned).len(),
        1,
        "a new version re-enables research"
    );
}

#[test]
fn the_call_record_heads_the_judging_batch_and_carries_the_verdicts() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    let gateway = recorded(RESPONSE_AAPL);
    worker_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &gateway,
        clock(CUT_OFF + 30),
    );
    let called = lp
        .steps
        .last()
        .cloned()
        .unwrap_or_else(|| panic!("a step ran"));
    let drafts: Vec<mandate_runtime::EventDraft> = called
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Journal(draft) => Some(draft.clone()),
            _ => None,
        })
        .collect();
    let types: Vec<&str> = drafts.iter().map(|d| d.event_type.as_str()).collect();
    assert_eq!(
        types,
        vec![
            "ModelInvocationRecorded",
            "ThesisProposed",
            "ModelOutputRecorded"
        ],
        "§6.2 steps 4 and 5 in one batch, the call record first (INF-10)"
    );
    assert_eq!(drafts[1].causation_id.as_ref(), Some(&drafts[0].event_id));
    assert_eq!(drafts[2].causation_id.as_ref(), Some(&drafts[1].event_id));
    let verdicts = drafts[0]
        .payload
        .get("verdicts")
        .and_then(Value::as_array)
        .unwrap_or_default();
    assert_eq!(verdicts.len(), 1, "HI-22: one verdict per candidate");
    assert_eq!(
        verdicts[0].get("candidate").and_then(Value::as_int),
        Some(0)
    );
    assert_eq!(payload_text(&verdicts[0], "kind"), Some("thesis"));
    assert_eq!(payload_text(&verdicts[0], "result"), Some("judged"));
    assert_eq!(
        payload_text(&drafts[0].payload, "prompt_ref"),
        payload_text(&drafts[1].payload, "prompt_ref"),
        "the thesis names the call's own prompt"
    );
    assert_eq!(
        payload_text(&drafts[0].payload, "response_ref"),
        payload_text(&drafts[1].payload, "response_ref")
    );
    assert_eq!(lp.research.in_flight(), None, "the run is done");
}

#[test]
fn the_platform_fills_every_fact_of_an_admitted_thesis() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &recorded(RESPONSE_AAPL),
    );
    let thesis = lp.events("ThesisProposed")[0].payload.clone();
    assert_eq!(
        payload_text(&thesis, "instrument_id"),
        Some(AAPL),
        "HI-18: resolved by the snapshot"
    );
    assert_eq!(payload_text(&thesis, "asset_class"), Some("us_equity"));
    assert_eq!(
        payload_text(&thesis, "as_of"),
        Some("2026-10-16T20:00:00.000000000Z")
    );
    assert_eq!(
        payload_text(&thesis, "expires_at"),
        Some("2026-10-21T20:00:00.000000000Z")
    );
    assert_eq!(
        payload_text(&thesis, "corroboration"),
        Some("independent_source")
    );
    assert_eq!(payload_text(&thesis, "model_id"), Some(RESEARCH_MODEL));
    assert_eq!(thesis.get("admitted"), Some(&Value::Bool(true)));
    assert_eq!(thesis.get("reason"), Some(&Value::Null));
    assert_eq!(thesis.get("revision").and_then(Value::as_int), Some(0));
    assert_eq!(
        thesis.get("allowlist_version").and_then(Value::as_int),
        Some(3)
    );
    let sources: Vec<&str> = thesis
        .get("evidence_sources")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(
        sources,
        vec!["src.newswire", "src.wire_two"],
        "the cited text sources in the order cited; market data needs no allowlist entry"
    );
    assert_eq!(
        payload_text(&thesis, "thesis_id"),
        payload_text(&thesis, "lineage_id"),
        "a new thesis starts its own lineage"
    );
    let output = lp.events("ModelOutputRecorded")[0].payload.clone();
    assert_eq!(output.get("ignored"), Some(&Value::Null));
    assert_eq!(payload_text(&output, "instrument_id"), Some(AAPL));
    assert_eq!(payload_text(&output, "conviction"), Some("0.8"));
    assert_eq!(
        payload_text(&output, "thesis_id"),
        payload_text(&thesis, "thesis_id")
    );
    let evidence = output
        .get("evidence")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .len();
    assert_eq!(evidence, 3, "the cited items' observations");
}

fn judged_with(output: &str, world: &World) -> Loop {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        world,
        &plane,
        &recorded(output),
    );
    lp
}

/// Every candidate's verdict, in response order: the harness's result, and for a thesis handed to
/// admission its thesis record's (`admitted`, `renewed` for a lineage it continues, or the §8.5
/// reason), and `judged` for an invalidation handed on.
fn verdicts(lp: &Loop) -> Vec<String> {
    let records = lp.events("ModelInvocationRecorded");
    let Some(record) = records.first() else {
        return Vec::new();
    };
    let mut theses = lp.shell.agent_journal.iter().filter(|event| {
        matches!(
            event.event_type.as_str(),
            "ThesisProposed" | "ThesisRevised"
        )
    });
    record
        .payload
        .get("verdicts")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .map(|verdict| {
            let result = payload_text(verdict, "result").unwrap_or_default();
            if result != "judged" || payload_text(verdict, "kind") != Some("thesis") {
                return result.to_owned();
            }
            let thesis = &theses
                .next()
                .unwrap_or_else(|| panic!("a judged thesis has its record"))
                .payload;
            if thesis.get("admitted") != Some(&Value::Bool(true)) {
                return payload_text(thesis, "reason")
                    .unwrap_or_default()
                    .to_owned();
            }
            if payload_text(thesis, "lineage_id") == payload_text(thesis, "thesis_id") {
                "admitted".to_owned()
            } else {
                "renewed".to_owned()
            }
        })
        .collect()
}

/// The judged call's record: its outcome and how many verdicts it carries.
fn judged_record(lp: &Loop) -> (Option<String>, usize) {
    let records = lp.events("ModelInvocationRecorded");
    assert_eq!(records.len(), 1, "one record per call");
    (
        payload_text(&records[0].payload, "outcome").map(str::to_owned),
        records[0]
            .payload
            .get("verdicts")
            .and_then(Value::as_array)
            .map_or(0, <[Value]>::len),
    )
}

fn thesis(instrument: &str, evidence: &[&str], text: &str) -> String {
    let cited: Vec<String> = evidence.iter().map(|id| format!("\"{id}\"")).collect();
    format!(
        r#"{{"instrument":"{instrument}","direction":"long","conviction":"0.6","confidence":"0.6","horizon_s":86400,"thesis":"{text}","invalidation":"It stops.","evidence":[{}]}}"#,
        cited.join(",")
    )
}

const SERVICES: &str = "nw-20261016-aapl-services";
const SUPPLY: &str = "w2-20261016-aapl-supply";

#[test]
fn each_harness_check_refuses_with_its_own_reason_and_admits_nothing() {
    let world = World::open();
    let cases = [
        (
            thesis("ZZZZ", &[SERVICES, SUPPLY], "x"),
            "unknown_instrument",
        ),
        (
            thesis("AAPL", &[SERVICES, "invented-id"], "x"),
            "unknown_evidence",
        ),
        (
            thesis(
                "AAPL",
                &[SERVICES, SUPPLY],
                "Our price target is 300 dollars.",
            ),
            "forbidden_language",
        ),
    ];
    for (candidate, reason) in cases {
        let lp = judged_with(
            &format!(r#"{{"theses":[{candidate}],"invalidations":[]}}"#),
            &world,
        );
        assert_eq!(verdicts(&lp), vec![reason.to_owned()], "{reason}");
        assert!(
            lp.events("ThesisProposed").is_empty(),
            "{reason}: a harness refusal is not an admission"
        );
        assert!(lp.events("ModelOutputRecorded").is_empty(), "{reason}");
    }
}

#[test]
fn a_revision_a_schema_admits_is_still_refused_while_revisions_are_not_enabled() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    if let Some(config) = world.config.as_mut() {
        config.pin = common::research::revisable_content().model_ref();
    }
    let revision = thesis("AAPL", &[SERVICES, SUPPLY], "x").replace(
        r#""evidence""#,
        r#""predecessor_thesis_id":"01PRED","evidence""#,
    );
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &Model::Revisable(format!(r#"{{"theses":[{revision}],"invalidations":[]}}"#)),
    );
    assert_eq!(
        verdicts(&lp),
        vec!["revisions_not_enabled".to_owned()],
        "§6.5: the harness refuses a revision whatever the schema admits"
    );
    assert!(lp.events("ThesisProposed").is_empty());
}

#[test]
fn a_revision_fails_the_pinned_schema_while_revisions_are_not_enabled() {
    let world = World::open();
    let revision = thesis("AAPL", &[SERVICES, SUPPLY], "x").replace(
        r#""evidence""#,
        r#""predecessor_thesis_id":"01PRED","evidence""#,
    );
    let lp = judged_with(
        &format!(r#"{{"theses":[{revision}],"invalidations":[]}}"#),
        &world,
    );
    assert_eq!(
        judged_record(&lp),
        (Some("schema_invalid".to_owned()), 0),
        "DEC-1120 item 3: §6.5 refuses revisions until §8.6 item 5 allows them"
    );
    assert!(lp.events("ThesisProposed").is_empty());
}

#[test]
fn a_second_thesis_for_the_same_instrument_is_a_duplicate() {
    let world = World::open();
    let one = thesis("AAPL", &[SERVICES, SUPPLY], "x");
    let lp = judged_with(
        &format!(r#"{{"theses":[{one},{one}],"invalidations":[]}}"#),
        &world,
    );
    assert_eq!(
        verdicts(&lp),
        vec!["admitted".to_owned(), "duplicate_instrument".to_owned()]
    );
    assert_eq!(lp.events("ThesisProposed").len(), 1);
}

#[test]
fn an_invalidation_of_no_active_thesis_is_unknown_and_one_of_an_active_thesis_is_judged() {
    let mut world = World::open();
    world.active = vec![ActiveThesis {
        thesis_id: "01ACTIVE".to_owned(),
        lineage_id: "01ACTIVE".to_owned(),
        revision: 0,
        instrument: AssetId::new(MSFT).unwrap_or_else(|e| panic!("{e}")),
        symbol: mandate_research_run::Symbol::new("MSFT").unwrap_or_else(|e| panic!("{e}")),
        expires_at: clock(CUT_OFF + 86_400),
    }];
    let lp = judged_with(
        &format!(
            r#"{{"theses":[],"invalidations":[{{"thesis_id":"01NOPE","evidence":[]}},{{"thesis_id":"01ACTIVE","evidence":["{SERVICES}"]}},{{"thesis_id":"01ACTIVE2","evidence":[]}}]}}"#
        ),
        &world,
    );
    assert_eq!(
        verdicts(&lp),
        vec![
            "unknown_thesis".to_owned(),
            "judged".to_owned(),
            "unknown_thesis".to_owned()
        ]
    );
    let record = &lp.events("ModelInvocationRecorded")[0].payload;
    let kinds: Vec<&str> = record
        .get("verdicts")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| payload_text(v, "kind"))
        .collect();
    assert_eq!(kinds, vec!["invalidation"; 3]);
    assert!(
        lp.events("ModelOutputRecorded").is_empty(),
        "an invalidation never adds an output"
    );
}

#[test]
fn an_invalidation_citing_unretrieved_evidence_is_handed_on_for_e19_5_to_judge() {
    let mut world = World::open();
    world.active = vec![ActiveThesis {
        thesis_id: "01ACTIVE".to_owned(),
        lineage_id: "01ACTIVE".to_owned(),
        revision: 0,
        instrument: AssetId::new(MSFT).unwrap_or_else(|e| panic!("{e}")),
        symbol: mandate_research_run::Symbol::new("MSFT").unwrap_or_else(|e| panic!("{e}")),
        expires_at: clock(CUT_OFF + 86_400),
    }];
    let lp = judged_with(
        r#"{"theses":[],"invalidations":[{"thesis_id":"01ACTIVE","evidence":["invented-id"]}]}"#,
        &world,
    );
    assert_eq!(
        verdicts(&lp),
        vec!["judged".to_owned()],
        "rule 151: unknown_evidence is a thesis's result only (DEC-1120 item 6)"
    );
}

#[test]
fn a_refused_admission_journals_its_thesis_record_with_the_first_failing_check() {
    let world = World::open();
    let one_source = thesis("AAPL", &[SERVICES], "x");
    let lp = judged_with(
        &format!(r#"{{"theses":[{one_source}],"invalidations":[]}}"#),
        &world,
    );
    assert_eq!(verdicts(&lp), vec!["no_corroboration".to_owned()]);
    let record = lp.events("ThesisProposed")[0].payload.clone();
    assert_eq!(record.get("admitted"), Some(&Value::Bool(false)));
    assert_eq!(payload_text(&record, "reason"), Some("no_corroboration"));
    assert_eq!(record.get("corroboration"), Some(&Value::Null));
    assert!(
        lp.events("ModelOutputRecorded").is_empty(),
        "only checks 1 to 3 record an ignored output"
    );
}

#[test]
fn a_non_long_direction_is_journaled_and_ignored() {
    let world = World::open();
    let short = thesis("AAPL", &[SERVICES, SUPPLY], "x").replace(r#""long""#, r#""short""#);
    let lp = judged_with(
        &format!(r#"{{"theses":[{short}],"invalidations":[]}}"#),
        &world,
    );
    assert_eq!(verdicts(&lp), vec!["direction_not_allowed".to_owned()]);
    let output = lp.events("ModelOutputRecorded")[0].payload.clone();
    assert_eq!(
        payload_text(&output, "ignored"),
        Some("direction_not_allowed")
    );
    assert_eq!(payload_text(&output, "direction"), Some("short"));
}

#[test]
fn a_full_universe_refuses_the_admission() {
    let mut world = World::open();
    world.universe = WorkingUniverse::Known {
        instruments: [
            "7b4a1c2e-3333-4a2b-9c3d-000000000003",
            "7b4a1c2e-4444-4a2b-9c3d-000000000004",
            "7b4a1c2e-5555-4a2b-9c3d-000000000005",
            "7b4a1c2e-6666-4a2b-9c3d-000000000006",
            "7b4a1c2e-7777-4a2b-9c3d-000000000007",
        ]
        .iter()
        .map(|id| AssetId::new(id).unwrap_or_else(|e| panic!("{e}")))
        .collect(),
        pinned: false,
    };
    let lp = judged_with(RESPONSE_AAPL, &world);
    assert_eq!(verdicts(&lp), vec!["universe_full".to_owned()]);
}

#[test]
fn an_output_the_harness_cannot_read_is_recorded_with_no_verdicts_and_judges_nothing() {
    let world = World::open();
    for (what, output) in [
        (
            "a conviction outside [-1, 1] the schema language cannot bound",
            thesis("AAPL", &[SERVICES, SUPPLY], "x")
                .replace(r#""0.6","confidence""#, r#""5","confidence""#),
        ),
        (
            "text with a control character",
            thesis("AAPL", &[SERVICES, SUPPLY], "a\\u0007b"),
        ),
    ] {
        let lp = judged_with(
            &format!(r#"{{"theses":[{output}],"invalidations":[]}}"#),
            &world,
        );
        assert_eq!(judged_record(&lp), (Some("ok".to_owned()), 0), "{what}");
        assert!(lp.events("ThesisProposed").is_empty(), "{what}");
        assert!(lp.events("ModelOutputRecorded").is_empty(), "{what}");
    }
    let lp = judged_with(
        r#"{"theses":[{"instrument":"AAPL"}],"invalidations":[]}"#,
        &world,
    );
    assert_eq!(
        judged_record(&lp),
        (Some("schema_invalid".to_owned()), 0),
        "the gateway's schema refuses a partial thesis"
    );
}

/// Drives one run to its call request, folding everything back, and returns the run id, the call
/// plan, and the retrieval.
fn until_call(
    lp: &mut Loop,
    ports: &mandate_runtime::Ports<'_>,
    world: &World,
) -> (String, CallPlan, Retrieval) {
    let plane = RecordedPlane::load();
    let started = lp.run(ResearchInput::Poll(clock(CUT_OFF)), ports, world);
    let Some(ResearchRequest::Start { run_id, cut_off }) = started.requests.first().cloned() else {
        panic!("a run starts");
    };
    let retrieval = mandate_research_run::retrieve(
        &common::research::config().plan,
        &world.scope(cut_off),
        plane.readers(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let items = retrieval
        .items
        .iter()
        .map(|item| {
            mandate_runtime::RetrievedObservation::capture(item, cut_off)
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        })
        .collect();
    let retrieved = lp.run(
        ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items,
        },
        ports,
        world,
    );
    let Some(ResearchRequest::Call(plan)) = retrieved.requests.first().cloned() else {
        panic!("a call is requested");
    };
    (run_id, plan, retrieval)
}

#[test]
fn an_answer_arriving_under_another_pin_is_recorded_and_judged_as_nothing() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    let (run_id, plan, retrieval) = until_call(&mut lp, &ports, &world);
    let record = record_for(
        &recorded(RESPONSE_AAPL),
        &plan,
        &retrieval,
        &world,
        common::research::instant(CUT_OFF),
    );
    if let Some(config) = world.config.as_mut() {
        config.pin = ModelPin {
            model_version: "0.2.0".to_owned(),
            ..pin()
        };
    }
    let step = lp.run(
        ResearchInput::Called {
            run_id,
            at: clock(CUT_OFF + 30),
            record: Box::new(record),
        },
        &ports,
        &world,
    );
    assert!(starts(&step).is_empty());
    assert_eq!(
        judged_record(&lp),
        (Some("ok".to_owned()), 0),
        "the call is recorded, with no candidate judged"
    );
    assert!(
        lp.events("ThesisProposed").is_empty(),
        "HI-17: the version in effect judges, and it no longer runs this pin"
    );
    assert!(lp.events("ModelOutputRecorded").is_empty());
}

#[test]
fn a_version_change_cancels_the_run_in_flight() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    if let Some(config) = world.config.as_mut() {
        config.pin.content_hash = mandate_canon::Digest::of(b"another entry");
    }
    let polled = lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
    assert_eq!(cancels(&polled), vec![started[0].0.clone()]);
}

#[test]
fn clearing_the_research_agent_or_pinning_the_universe_cancels_the_run() {
    for change in 0..2 {
        let f = Fixture::new();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let mut world = World::open();
        let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
        if change == 0 {
            world.config = None;
        } else {
            let mut envelope = common::research::envelope();
            envelope.universe_pinned = true;
            world.mandate = mandate_research::ValidatedMandate::from_validated_envelope(envelope);
        }
        let polled = lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
        assert_eq!(
            cancels(&polled),
            vec![started[0].0.clone()],
            "change {change}"
        );
    }
}

#[test]
fn a_run_past_its_wall_clock_bound_is_cancelled() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    let bound = CUT_OFF + 60 + 120;
    let within = lp.run(ResearchInput::Poll(clock(bound)), &ports, &world);
    assert!(
        cancels(&within).is_empty(),
        "§7.1: retrieval 60 s plus the 120 s deadline"
    );
    let past = lp.run(ResearchInput::Poll(clock(bound + 1)), &ports, &world);
    assert_eq!(cancels(&past), vec![started[0].0.clone()]);
}

#[test]
fn a_late_answer_to_a_cancelled_run_is_journaled_and_never_used() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let (run_id, plan, retrieval) = until_call(&mut lp, &ports, &world);
    let past = lp.run(ResearchInput::Poll(clock(CUT_OFF + 500)), &ports, &world);
    assert_eq!(cancels(&past), vec![run_id.clone()]);
    lp.run(
        ResearchInput::Cancelled {
            run_id: run_id.clone(),
            dispatched: true,
        },
        &ports,
        &world,
    );
    let record = record_for(
        &recorded(RESPONSE_AAPL),
        &plan,
        &retrieval,
        &world,
        common::research::instant(CUT_OFF + 400),
    );
    let mut other = record.clone();
    other.call_id = mandate_canon::encode_ulid(1 << 99);
    let substituted = lp.step(
        ResearchInput::Called {
            run_id: run_id.clone(),
            at: clock(CUT_OFF + 600),
            record: Box::new(other),
        },
        &ports,
        &world,
    );
    assert!(
        matches!(substituted, Err(RuntimeError::NotInterpreted { .. })),
        "a record of another call is not the cancelled call's"
    );
    lp.run(
        ResearchInput::Called {
            run_id,
            at: clock(CUT_OFF + 600),
            record: Box::new(record),
        },
        &ports,
        &world,
    );
    assert_eq!(
        judged_record(&lp),
        (Some("ok".to_owned()), 0),
        "§5.3: the record of the call is complete, and nothing in it is used"
    );
    lp.run(ResearchInput::Poll(clock(CUT_OFF + 601)), &ports, &world);
    assert!(lp.events("ThesisProposed").is_empty());
    assert!(lp.events("ModelOutputRecorded").is_empty());
}

#[test]
fn a_cancellation_before_dispatch_forgets_the_run() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    world.config = None;
    lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
    lp.run(
        ResearchInput::Cancelled {
            run_id: started[0].0.clone(),
            dispatched: false,
        },
        &ports,
        &world,
    );
    let stray = lp.step(
        ResearchInput::Called {
            run_id: started[0].0.clone(),
            at: clock(CUT_OFF + 2),
            record: Box::new(common::research::stray_record(
                &mandate_canon::encode_ulid(1 << 98),
                &world,
            )),
        },
        &ports,
        &world,
    );
    assert!(
        matches!(stray, Err(RuntimeError::NotInterpreted { .. })),
        "an answer to a run that never called is not a call to record"
    );
}

#[test]
fn the_trading_loop_refuses_a_research_input() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let refused = handle(
        &mut lp.shell.state,
        Input::Research(ResearchInput::Poll(clock(CUT_OFF))),
        &ports,
    );
    assert!(matches!(refused, Err(RuntimeError::NotInterpreted { .. })));
}

#[test]
fn a_research_step_before_start_is_refused() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    let world = World::open();
    assert_eq!(
        lp.step(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world)
            .map(|_| ()),
        Err(RuntimeError::NotStarted)
    );
}

#[test]
fn a_restart_restores_the_schedule_and_never_judges_the_lost_run() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    worker_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &recorded(RESPONSE_AAPL),
        clock(CUT_OFF + 30),
    );
    let (shell, _) = lp.shell.restart(&ports);
    let mut research = mandate_runtime::ResearchState::new();
    for event in &shell.agent_journal {
        research.fold(event).unwrap_or_else(|e| panic!("{e}"));
    }
    assert_eq!(research.last_call(), Some(clock(CUT_OFF + 30)));
    assert_eq!(research.in_flight(), None, "§5.4: a run in flight is lost");
    let mut restarted = Loop {
        shell,
        research,
        steps: Vec::new(),
    };
    restarted
        .shell
        .fold_one(&event(
            ACCOUNT_STREAM,
            2,
            "ReconciliationRun",
            with_clock(&[("result", text("clean"))], CUT_OFF + 40),
        ))
        .unwrap_or_else(|e| panic!("{e}"));
    restarted
        .shell
        .run(Input::Tick(clock(CUT_OFF + 41)), &ports);
    let polled = restarted.run(ResearchInput::Poll(clock(CUT_OFF + 42)), &ports, &world);
    assert!(
        starts(&polled).is_empty(),
        "the interval still counts from the call"
    );
    assert_eq!(
        restarted.events("ThesisProposed").len(),
        1,
        "judged once, in the call's own step, and never again after the restart"
    );
}

#[test]
fn a_run_lost_to_a_restart_never_takes_its_late_answer() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let (run_id, plan, retrieval) = until_call(&mut lp, &ports, &world);
    let (shell, _) = lp.shell.restart(&ports);
    let mut restarted = Loop {
        shell,
        research: mandate_runtime::ResearchState::new(),
        steps: Vec::new(),
    };
    let record = record_for(
        &recorded(RESPONSE_AAPL),
        &plan,
        &retrieval,
        &world,
        common::research::instant(CUT_OFF),
    );
    let late = restarted.step(
        ResearchInput::Called {
            run_id,
            at: clock(CUT_OFF + 30),
            record: Box::new(record),
        },
        &ports,
        &world,
    );
    assert!(
        matches!(late, Err(RuntimeError::NotInterpreted { .. })),
        "§5.4: a run in flight is lost with its process"
    );
    assert!(restarted.events("ModelInvocationRecorded").is_empty());
}

#[test]
fn every_research_record_replays_through_the_trading_fold() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &recorded(RESPONSE_AAPL),
    );
    let (restarted, _) = lp.shell.restart(&ports);
    assert_eq!(restarted.agent_journal, lp.shell.agent_journal);
}

#[test]
fn drift_folds_each_text_item_once() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    let gateway = failing(Outcome::ProviderUnavailable);
    full_run(&mut lp, clock(CUT_OFF), &ports, &world, &plane, &gateway);
    let first = lp
        .research
        .drift()
        .report()
        .unwrap_or_else(|e| panic!("{e:?}"))
        .source_count()
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(first, 2, "two allowlisted text sources were read");
    let once = lp.research.drift().clone();
    let mut plane = plane;
    plane
        .news
        .retain(|item| item.item_id.as_str() != "nw-20261017-aapl-late");
    full_run(
        &mut lp,
        clock(CUT_OFF + i64::from(INTERVAL_S) + 100),
        &ports,
        &world,
        &plane,
        &gateway,
    );
    assert_eq!(
        lp.research.drift(),
        &once,
        "re-reading the same items cannot fake DuplicateContent; the item known only after the \
         first cut-off is left out so the second run reads exactly the first run's items"
    );
}

#[test]
fn an_unresolved_append_blocks_research_until_the_same_input_retries() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    let retrieval = mandate_research_run::retrieve(
        &common::research::config().plan,
        &world.scope(clock(CUT_OFF)),
        plane.readers(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let items: Vec<_> = retrieval
        .items
        .iter()
        .map(|item| {
            mandate_runtime::RetrievedObservation::capture(item, clock(CUT_OFF))
                .unwrap_or_else(|e| panic!("{e}"))
                .0
        })
        .collect();
    let input = ResearchInput::Retrieved {
        run_id: started[0].0.clone(),
        items,
    };
    let first = mandate_runtime::handle_research(
        &mut lp.shell.state,
        &mut lp.research,
        input.clone(),
        &ports,
        &world.context(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let blocked = mandate_runtime::handle_research(
        &mut lp.shell.state,
        &mut lp.research,
        ResearchInput::Poll(clock(CUT_OFF + 1)),
        &ports,
        &world.context(),
    );
    assert!(matches!(
        blocked,
        Err(RuntimeError::AppendUnresolved { .. })
    ));
    let retried = mandate_runtime::handle_research(
        &mut lp.shell.state,
        &mut lp.research,
        input,
        &ports,
        &world.context(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(retried, first, "the same drafts and the same call request");
}

fn rejected(lp: &Loop) {
    assert_eq!(
        judged_record(lp),
        (Some("ok".to_owned()), 0),
        "HI-6: the call is recorded and nothing is judged"
    );
    assert!(lp.events("ThesisProposed").is_empty());
    assert!(lp.events("ModelOutputRecorded").is_empty());
}

#[test]
fn a_blank_invalidation_is_judged_as_nothing() {
    let world = World::open();
    let blank = thesis("AAPL", &[SERVICES, SUPPLY], "x").replace("It stops.", "   ");
    let lp = judged_with(
        &format!(r#"{{"theses":[{blank}],"invalidations":[]}}"#),
        &world,
    );
    rejected(&lp);
}

#[test]
fn a_pin_the_gateway_does_not_hold_ends_the_run_and_disables_research_for_it() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    if let Some(config) = world.config.as_mut() {
        config.pin.model_version = "0.9.0".to_owned();
    }
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &recorded(RESPONSE_AAPL),
    );
    assert!(
        lp.events("ModelInvocationRecorded").is_empty(),
        "inference spec §3.1: a request the gateway cannot make has no record"
    );
    assert_eq!(lp.research.in_flight(), None, "HI-6: the run ends");
    let later = clock(CUT_OFF + 2 * i64::from(INTERVAL_S));
    assert!(
        starts(&lp.run(ResearchInput::Poll(later), &ports, &world)).is_empty(),
        "the same refusal would repeat, so research waits for a new version"
    );
    if let Some(config) = world.config.as_mut() {
        config.pin = pin();
    }
    let repinned = lp.run(ResearchInput::Poll(clock(later.secs() + 1)), &ports, &world);
    assert_eq!(starts(&repinned).len(), 1);
}

#[test]
fn a_refusal_for_a_run_not_in_flight_is_refused() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let stray = lp.step(
        ResearchInput::Refused {
            run_id: "01NORUN".to_owned(),
            at: clock(CUT_OFF),
            error: RequestError::UnknownModel,
        },
        &ports,
        &world,
    );
    assert!(matches!(stray, Err(RuntimeError::NotInterpreted { .. })));
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    let early = lp.step(
        ResearchInput::Refused {
            run_id: started[0].0.clone(),
            at: clock(CUT_OFF + 1),
            error: RequestError::UnknownModel,
        },
        &ports,
        &world,
    );
    assert!(
        matches!(early, Err(RuntimeError::NotInterpreted { .. })),
        "a refusal before a call was requested"
    );
    assert_eq!(lp.research.in_flight(), Some(started[0].0.as_str()));
}

#[test]
fn a_snapshot_the_platform_cannot_read_is_judged_as_nothing() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let mut plane = RecordedPlane::load();
    for fact in &mut plane.instruments {
        fact.group = Some(String::new());
    }
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &recorded(RESPONSE_AAPL),
    );
    rejected(&lp);
}

#[test]
fn an_ineligible_or_grouped_instrument_in_the_snapshot_binds_admission() {
    for change in 0..2 {
        let f = Fixture::new();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let mut world = World::open();
        let mut plane = RecordedPlane::load();
        for fact in &mut plane.instruments {
            if change == 0 {
                fact.eligible = false;
            } else {
                fact.group = Some("grp.apple".to_owned());
            }
        }
        if change == 1 {
            world
                .facts
                .claimed_by_other_agents
                .insert(AssetId::new(MSFT).unwrap_or_else(|e| panic!("{e}")));
            world.facts.instrument_groups.insert(
                AssetId::new(MSFT).unwrap_or_else(|e| panic!("{e}")),
                mandate_research::GroupId::new("grp.apple").unwrap_or_else(|e| panic!("{e}")),
            );
        }
        full_run(
            &mut lp,
            clock(CUT_OFF),
            &ports,
            &world,
            &plane,
            &recorded(RESPONSE_AAPL),
        );
        let expected = if change == 0 {
            "eligibility_floor"
        } else {
            "instrument_group_claimed"
        };
        assert_eq!(verdicts(&lp), vec![expected.to_owned()]);
    }
}

#[test]
fn an_empty_retrieval_holds_the_interval_like_a_call() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    lp.run(
        ResearchInput::Retrieved {
            run_id: started[0].0.clone(),
            items: Vec::new(),
        },
        &ports,
        &world,
    );
    let again = lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
    assert!(starts(&again).is_empty(), "HI-8: no re-read on every poll");
    let due = lp.run(
        ResearchInput::Poll(clock(CUT_OFF + i64::from(INTERVAL_S))),
        &ports,
        &world,
    );
    assert_eq!(starts(&due).len(), 1);
}

/// Drives one run through its call by hand, without folding anything back, and returns the run id.
/// Drives one run through its call by hand, without folding the call's step back, and returns the
/// run id and that step's drafts.
fn called_unfolded(
    lp: &mut Loop,
    ports: &mandate_runtime::Ports<'_>,
    world: &World,
    model: &Model,
    change: impl Fn(&mut CallRecord),
) -> (
    String,
    Result<Vec<mandate_runtime::EventDraft>, RuntimeError>,
) {
    let (run_id, plan, retrieval) = until_call(lp, ports, world);
    let mut record = record_for(
        model,
        &plan,
        &retrieval,
        world,
        common::research::instant(CUT_OFF),
    );
    change(&mut record);
    let step = mandate_runtime::handle_research(
        &mut lp.shell.state,
        &mut lp.research,
        ResearchInput::Called {
            run_id: run_id.clone(),
            at: clock(CUT_OFF + 30),
            record: Box::new(record),
        },
        ports,
        &world.context(),
    );
    let drafts = step.map(|step| {
        step.effects
            .into_iter()
            .filter_map(|effect| match effect {
                Effect::Journal(draft) => Some(draft),
                _ => None,
            })
            .collect()
    });
    (run_id, drafts)
}

#[test]
fn the_core_refuses_a_record_of_another_call_identity_or_context() {
    for change in 0..6 {
        let f = Fixture::new();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let world = World::open();
        let (run_id, drafts) = called_unfolded(
            &mut lp,
            &ports,
            &world,
            &recorded(RESPONSE_AAPL),
            |record| match change {
                0 => record.model.model_version = "9.9.9".to_owned(),
                1 => record.call_id = mandate_canon::encode_ulid(1 << 97),
                2 => record.purpose = mandate_gateway::Purpose::LlmSignal,
                3 => record.agent_id = Some("agent-b".to_owned()),
                4 => {
                    record.observations.pop();
                }
                _ => record.model.content_hash = mandate_canon::Digest::of(b"other"),
            },
        );
        assert!(
            matches!(drafts, Err(RuntimeError::NotInterpreted { .. })),
            "change {change}: never journaled as this run's call"
        );
        assert_eq!(
            lp.research.in_flight(),
            Some(run_id.as_str()),
            "change {change}: the run waits for its own call's record, or its bound"
        );
    }
}

#[test]
fn an_ok_record_past_its_deadline_is_recorded_and_never_judged() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let (_, drafts) = called_unfolded(
        &mut lp,
        &ports,
        &world,
        &recorded(RESPONSE_AAPL),
        |record| {
            record.completed_at = mandate_time::UtcNanos::from_parts(
                record.deadline.secs(),
                record.deadline.nanos() + 1,
            )
            .unwrap_or_else(|e| panic!("{e}"));
        },
    );
    let drafts = drafts.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(drafts.len(), 1, "the call record alone (INF-3)");
    assert_eq!(drafts[0].event_type, "ModelInvocationRecorded");
    assert_eq!(
        drafts[0].payload.get("verdicts"),
        Some(&Value::Array(Vec::new()))
    );
    assert_eq!(lp.research.in_flight(), None);
}

#[test]
fn a_record_completed_after_the_run_s_bound_is_never_judged_whatever_its_deadline() {
    for (completed, judged) in [(CUT_OFF + 60 + 120, true), (CUT_OFF + 60 + 120 + 1, false)] {
        let f = Fixture::new();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let world = World::open();
        let (_, drafts) = called_unfolded(
            &mut lp,
            &ports,
            &world,
            &recorded(RESPONSE_AAPL),
            |record| {
                record.deadline = common::research::instant(CUT_OFF + 86_400);
                record.completed_at = common::research::instant(completed);
            },
        );
        let drafts = drafts.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            drafts.len() > 1,
            judged,
            "§7.1: completed at {completed}, the bound is the start plus 60 s plus 120 s"
        );
        assert_eq!(drafts[0].event_type, "ModelInvocationRecorded");
    }
}

#[test]
fn the_schedule_counts_from_an_answer_before_its_record_folds_back() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let (_, drafts) = called_unfolded(
        &mut lp,
        &ports,
        &world,
        &failing(Outcome::ProviderUnavailable),
        |_| {},
    );
    assert!(drafts.is_ok());
    assert_eq!(lp.research.last_call(), Some(clock(CUT_OFF + 30)));
}

#[test]
fn an_ok_record_is_judged_in_its_own_step_before_anything_folds_back() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let (_, drafts) = called_unfolded(&mut lp, &ports, &world, &recorded(RESPONSE_AAPL), |_| {});
    let types: Vec<String> = drafts
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .map(|draft| draft.event_type)
        .collect();
    assert_eq!(
        types,
        vec![
            "ModelInvocationRecorded".to_owned(),
            "ThesisProposed".to_owned(),
            "ModelOutputRecorded".to_owned()
        ]
    );
    assert_eq!(lp.research.in_flight(), None);
}

fn booted() -> (Fixture, World) {
    (Fixture::new(), World::open())
}

fn retrieved_items(world: &World, cut_off: i64) -> Vec<mandate_runtime::RetrievedObservation> {
    let plane = RecordedPlane::load();
    mandate_research_run::retrieve(
        &common::research::config().plan,
        &world.scope(clock(cut_off)),
        plane.readers(),
    )
    .unwrap_or_else(|e| panic!("{e}"))
    .items
    .iter()
    .map(|item| {
        mandate_runtime::RetrievedObservation::capture(item, clock(cut_off))
            .unwrap_or_else(|e| panic!("{e}"))
            .0
    })
    .collect()
}

#[test]
fn a_poll_behind_the_risk_clock_is_refused_and_one_at_it_is_not() {
    let (f, world) = booted();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let last = lp
        .shell
        .state
        .risk_clock()
        .unwrap_or_else(|| panic!("a clock"))
        .secs();
    assert!(matches!(
        lp.step(ResearchInput::Poll(clock(last - 1)), &ports, &world),
        Err(RuntimeError::RiskClockWentBackwards { .. })
    ));
    assert!(
        lp.step(ResearchInput::Poll(clock(last)), &ports, &world)
            .is_ok()
    );
}

#[test]
fn worker_inputs_for_a_run_not_in_its_state_are_refused() {
    let (f, world) = booted();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let items = retrieved_items(&world, CUT_OFF);
    let stray = lp.step(
        ResearchInput::Retrieved {
            run_id: "01NORUN".to_owned(),
            items: items.clone(),
        },
        &ports,
        &world,
    );
    assert!(
        matches!(stray, Err(RuntimeError::NotInterpreted { .. })),
        "no run at all"
    );
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    let run_id = started[0].0.clone();
    let wrong = lp.step(
        ResearchInput::Retrieved {
            run_id: "01OTHERRUN".to_owned(),
            items: items.clone(),
        },
        &ports,
        &world,
    );
    assert!(
        matches!(wrong, Err(RuntimeError::NotInterpreted { .. })),
        "another run's id"
    );
    let early = lp.step(
        ResearchInput::Called {
            run_id: run_id.clone(),
            at: clock(CUT_OFF + 1),
            record: Box::new(common::research::stray_record(
                &mandate_canon::encode_ulid(1 << 98),
                &world,
            )),
        },
        &ports,
        &world,
    );
    assert!(
        matches!(early, Err(RuntimeError::NotInterpreted { .. })),
        "an answer before a call"
    );
    assert_eq!(
        lp.research.in_flight(),
        Some(run_id.as_str()),
        "the run is kept"
    );
    lp.run(
        ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items: items.clone(),
        },
        &ports,
        &world,
    );
    let twice = lp.step(ResearchInput::Retrieved { run_id, items }, &ports, &world);
    assert!(
        matches!(twice, Err(RuntimeError::NotInterpreted { .. })),
        "a second retrieval"
    );
}

#[test]
fn a_retrieval_off_its_cut_off_or_with_an_item_twice_is_refused() {
    for change in 0..2 {
        let (f, world) = booted();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
        let mut items = retrieved_items(&world, CUT_OFF);
        if change == 0 {
            items[0].observation.as_of = clock(CUT_OFF - 1);
        } else {
            let first = items[0].clone();
            items.push(first);
        }
        let refused = lp.step(
            ResearchInput::Retrieved {
                run_id: started[0].0.clone(),
                items,
            },
            &ports,
            &world,
        );
        assert!(
            matches!(refused, Err(RuntimeError::NotInterpreted { .. })),
            "change {change}"
        );
        assert!(lp.events("ObservationRecorded").is_empty());
    }
}

#[test]
fn the_worker_s_own_cancellation_ends_the_run_and_one_for_another_run_does_not() {
    let (f, world) = booted();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    lp.run(
        ResearchInput::Cancelled {
            run_id: "01OTHERRUN".to_owned(),
            dispatched: false,
        },
        &ports,
        &world,
    );
    assert_eq!(lp.research.in_flight(), Some(started[0].0.as_str()));
    lp.run(
        ResearchInput::Cancelled {
            run_id: started[0].0.clone(),
            dispatched: false,
        },
        &ports,
        &world,
    );
    assert_eq!(lp.research.in_flight(), None);
}

#[test]
fn an_acknowledgment_forgets_a_run_unless_its_planned_call_was_dispatched() {
    for (planned, dispatched, remembered) in [
        (false, true, false),
        (true, false, false),
        (true, true, true),
    ] {
        let (f, mut world) = booted();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
        let run_id = started[0].0.clone();
        let mut record =
            common::research::stray_record(&mandate_canon::encode_ulid(1 << 98), &world);
        if planned {
            let retrieval = mandate_research_run::retrieve(
                &common::research::config().plan,
                &world.scope(clock(CUT_OFF)),
                RecordedPlane::load().readers(),
            )
            .unwrap_or_else(|e| panic!("{e}"));
            let step = lp.run(
                ResearchInput::Retrieved {
                    run_id: run_id.clone(),
                    items: retrieved_items(&world, CUT_OFF),
                },
                &ports,
                &world,
            );
            let Some(ResearchRequest::Call(plan)) = step.requests.first().cloned() else {
                panic!("a call is requested");
            };
            record = record_for(
                &failing(Outcome::ProviderUnavailable),
                &plan,
                &retrieval,
                &world,
                common::research::instant(CUT_OFF),
            );
        }
        world.config = None;
        lp.run(ResearchInput::Poll(clock(CUT_OFF + 1)), &ports, &world);
        lp.run(
            ResearchInput::Cancelled {
                run_id: run_id.clone(),
                dispatched,
            },
            &ports,
            &world,
        );
        let answer = lp.step(
            ResearchInput::Called {
                run_id,
                at: clock(CUT_OFF + 40),
                record: Box::new(record),
            },
            &ports,
            &world,
        );
        assert_eq!(
            answer.is_ok(),
            remembered,
            "planned {planned}, dispatched {dispatched}"
        );
    }
}

#[test]
fn a_stale_research_step_is_never_replayed_over_another_unresolved_batch() {
    let (f, world) = booted();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world));
    let input = ResearchInput::Retrieved {
        run_id: started[0].0.clone(),
        items: retrieved_items(&world, CUT_OFF),
    };
    lp.run(input.clone(), &ports, &world);
    lp.shell.next_append = common::AppendOutcome::Unresolved;
    lp.shell.run(Input::Command(Command::Pause), &ports);
    let replayed = mandate_runtime::handle_research(
        &mut lp.shell.state,
        &mut lp.research,
        input,
        &ports,
        &world.context(),
    );
    assert!(matches!(
        replayed,
        Err(RuntimeError::AppendUnresolved { .. })
    ));
}

#[test]
fn the_research_fold_reads_only_research_call_records() {
    let mut research = mandate_runtime::ResearchState::new();
    let other = event(
        common::AGENT_STREAM,
        1,
        "ModelInvocationRecorded",
        common::object(&[("purpose", text("compiler"))]),
    );
    research.fold(&other).unwrap_or_else(|e| panic!("{e}"));
    let unrelated = event(
        common::AGENT_STREAM,
        2,
        "DecisionMade",
        common::object(&[("purpose", text("research"))]),
    );
    research.fold(&unrelated).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(research.last_call(), None);
    let unreadable = event(
        common::AGENT_STREAM,
        3,
        "ModelInvocationRecorded",
        common::object(&[("purpose", text("research"))]),
    );
    assert!(
        research.fold(&unreadable).is_err(),
        "a research record without completed_at"
    );
    let readable = event(
        common::AGENT_STREAM,
        4,
        "ModelInvocationRecorded",
        common::object(&[
            ("purpose", text("research")),
            ("completed_at", text("2026-10-16T20:00:30.500000000Z")),
        ]),
    );
    research.fold(&readable).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(research.last_call(), Some(clock(CUT_OFF + 30)));
    let earlier = event(
        common::AGENT_STREAM,
        5,
        "ModelInvocationRecorded",
        common::object(&[
            ("purpose", text("research")),
            ("completed_at", text("2026-10-16T20:00:10.000000000Z")),
        ]),
    );
    research.fold(&earlier).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        research.last_call(),
        Some(clock(CUT_OFF + 30)),
        "the latest call anchors the schedule"
    );
}

#[test]
fn the_drift_fold_classes_news_and_filings_apart() {
    let (f, world) = booted();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut plane = RecordedPlane::load();
    plane
        .news
        .retain(|item| item.source.as_str() == "src.wire_two");
    plane.filings = vec![mandate_research_run::Filing {
        item_id: mandate_research_run::ItemId::new("f-aapl-8k").unwrap_or_else(|e| panic!("{e}")),
        source: mandate_research_run::SourceId::new("src.edgar").unwrap_or_else(|e| panic!("{e}")),
        symbol: mandate_research_run::Symbol::new("AAPL").unwrap_or_else(|e| panic!("{e}")),
        form: "8-K".to_owned(),
        knowledge_at: mandate_time::UtcNanos::from_parts(CUT_OFF - 600, 0)
            .unwrap_or_else(|e| panic!("{e}")),
        filed_at: mandate_time::UtcNanos::from_parts(CUT_OFF - 600, 0)
            .unwrap_or_else(|e| panic!("{e}")),
        excerpt: "Item 2.02.".to_owned(),
        content_hash: mandate_canon::Digest::of(b"f-aapl-8k"),
        licensed: false,
    }];
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &failing(Outcome::ProviderUnavailable),
    );
    let mut expected = mandate_research::drift::DriftState::new();
    let observe = |source: &str, class, at: i64, id: &str, length: u64| {
        mandate_research::drift::InputObservation {
            source: mandate_research::SourceId::new(source).unwrap_or_else(|e| panic!("{e}")),
            class,
            observed_at: mandate_time::UtcNanos::from_parts(at, 0)
                .unwrap_or_else(|e| panic!("{e}")),
            content: mandate_research::ContentHash::new(mandate_canon::Digest::of(id.as_bytes())),
            length_bytes: length,
        }
    };
    let supply = &plane.news[0];
    expected
        .observe(&observe(
            "src.wire_two",
            mandate_research::drift::InputClass::News,
            supply.knowledge_at.secs(),
            supply.item_id.as_str(),
            u64::try_from(supply.headline.len() + supply.summary.len()).unwrap_or(0),
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    expected
        .observe(&observe(
            "src.edgar",
            mandate_research::drift::InputClass::Filing,
            CUT_OFF - 600,
            "f-aapl-8k",
            10,
        ))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(lp.research.drift(), &expected);
}

#[test]
fn a_thesis_id_is_no_event_s_id() {
    let (f, world) = booted();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let plane = RecordedPlane::load();
    full_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        &recorded(RESPONSE_AAPL),
    );
    let thesis = payload_text(&lp.events("ThesisProposed")[0].payload, "thesis_id")
        .unwrap_or_default()
        .to_owned();
    assert!(
        lp.shell
            .agent_journal
            .iter()
            .all(|event| event.event_id.0 != thesis),
        "a derived thesis id never meets a draft's id"
    );
}

#[test]
fn a_thesis_for_an_instrument_with_an_active_thesis_renews_its_lineage() {
    for revision in [0u32, 2] {
        let (f, mut world) = booted();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        world.admit(AAPL);
        world.active = vec![
            ActiveThesis {
                thesis_id: "01MSFTTHESIS".to_owned(),
                lineage_id: "01MSFTLINEAGE".to_owned(),
                revision: 0,
                instrument: AssetId::new(MSFT).unwrap_or_else(|e| panic!("{e}")),
                symbol: mandate_research_run::Symbol::new("MSFT").unwrap_or_else(|e| panic!("{e}")),
                expires_at: clock(CUT_OFF + 86_400),
            },
            ActiveThesis {
                thesis_id: "01AAPLTHESIS".to_owned(),
                lineage_id: "01AAPLLINEAGE".to_owned(),
                revision,
                instrument: AssetId::new(AAPL).unwrap_or_else(|e| panic!("{e}")),
                symbol: mandate_research_run::Symbol::new("AAPL").unwrap_or_else(|e| panic!("{e}")),
                expires_at: clock(CUT_OFF + 86_400),
            },
        ];
        let plane = RecordedPlane::load();
        full_run(
            &mut lp,
            clock(CUT_OFF),
            &ports,
            &world,
            &plane,
            &recorded(RESPONSE_AAPL),
        );
        let kind = if revision > 0 {
            "ThesisRevised"
        } else {
            "ThesisProposed"
        };
        let records = lp.events(kind);
        assert_eq!(records.len(), 1, "revision {revision}");
        let record = &records[0].payload;
        assert_eq!(payload_text(record, "lineage_id"), Some("01AAPLLINEAGE"));
        assert_eq!(
            record.get("revision").and_then(Value::as_int),
            Some(u64::from(revision))
        );
        let predecessor = record.get("predecessor_thesis_id");
        if revision > 0 {
            assert_eq!(predecessor.and_then(Value::as_str), Some("01AAPLTHESIS"));
        } else {
            assert_eq!(predecessor, Some(&Value::Null));
        }
        assert_eq!(
            verdicts(&lp),
            vec!["renewed".to_owned()],
            "revision {revision}"
        );
    }
}

#[test]
fn a_cited_source_off_the_admission_allowlist_never_corroborates() {
    let mut world = World::open();
    world
        .facts
        .allowlist
        .sources
        .retain(|source| source.as_str() != "src.wire_two");
    let lp = judged_with(RESPONSE_AAPL, &world);
    let record = &lp.events("ThesisProposed")[0].payload;
    assert_eq!(record.get("corroboration"), Some(&Value::Null));
    assert_eq!(
        payload_text(record, "reason"),
        Some("source_not_allowlisted")
    );
}

fn active_on(instrument: &str, symbol: &str, id: &str) -> ActiveThesis {
    ActiveThesis {
        thesis_id: id.to_owned(),
        lineage_id: id.to_owned(),
        revision: 0,
        instrument: AssetId::new(instrument).unwrap_or_else(|e| panic!("{e}")),
        symbol: mandate_research_run::Symbol::new(symbol).unwrap_or_else(|e| panic!("{e}")),
        expires_at: clock(CUT_OFF + 86_400),
    }
}

fn in_universe(world: &mut World, instrument: &str) {
    world.universe = WorkingUniverse::Known {
        instruments: [AssetId::new(instrument).unwrap_or_else(|e| panic!("{e}"))]
            .into_iter()
            .collect(),
        pinned: false,
    };
}

/// Journal spec §9.22 and agent harness spec §6.7: an accepted invalidation is journaled as
/// `ThesisInvalidated` in its call's judging batch, after the call record and before every thesis
/// record, naming its verdict by index (theses first, rule 151) and its thesis as the run's inputs
/// held it; the call record is its cause.
#[test]
fn a_judged_invalidation_is_journaled_after_its_call_record_and_before_any_thesis_record() {
    let mut world = World::open();
    world.active = vec![active_on(MSFT, "MSFT", "01ACTIVE")];
    in_universe(&mut world, MSFT);
    let lp = judged_with(
        &format!(
            r#"{{"theses":[{}],"invalidations":[{{"thesis_id":"01ACTIVE","evidence":["{SERVICES}","{SUPPLY}"]}}]}}"#,
            thesis("AAPL", &[SERVICES, SUPPLY], "x")
        ),
        &world,
    );
    assert_eq!(
        verdicts(&lp),
        vec!["admitted".to_owned(), "judged".to_owned()]
    );
    let judged: Vec<&str> = lp
        .shell
        .agent_journal
        .iter()
        .map(|e| e.event_type.as_str())
        .skip_while(|t| *t != "ModelInvocationRecorded")
        .take(3)
        .collect();
    assert_eq!(
        judged,
        vec![
            "ModelInvocationRecorded",
            "ThesisInvalidated",
            "ThesisProposed"
        ],
        "§9.22: the invalidation records come before the thesis records"
    );
    let call = lp.events("ModelInvocationRecorded")[0];
    let invalidated = lp.events("ThesisInvalidated");
    let [invalidated] = invalidated.as_slice() else {
        panic!("one record: {invalidated:?}");
    };
    assert_eq!(invalidated.causation_id.as_ref(), Some(&call.event_id));
    let p = &invalidated.payload;
    assert_eq!(payload_text(p, "thesis_id"), Some("01ACTIVE"));
    assert_eq!(payload_text(p, "lineage_id"), Some("01ACTIVE"));
    assert_eq!(payload_text(p, "instrument_id"), Some(MSFT));
    assert_eq!(payload_text(p, "cause"), Some("model"));
    assert_eq!(
        p.get("candidate").and_then(Value::as_int),
        Some(1),
        "the theses' verdicts come first, so the invalidation is candidate 1"
    );
    assert_eq!(p.get("source"), Some(&Value::Null));
    let evidence: Vec<&str> = p
        .get("evidence")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(evidence, vec![SERVICES, SUPPLY], "as the output gave them");
}

/// A final output that invalidates one thesis twice is refused by the harness's strict read
/// (`duplicate_invalidation`), so nothing of it is judged and no thesis is invalidated twice (HI-6).
/// §6.7's `unknown_thesis` for a repeat is the agentic loop's, across turns.
#[test]
fn a_final_output_invalidating_a_thesis_twice_is_unreadable_and_invalidates_nothing() {
    let mut world = World::open();
    world.active = vec![active_on(MSFT, "MSFT", "01ACTIVE")];
    let lp = judged_with(
        r#"{"theses":[],"invalidations":[{"thesis_id":"01ACTIVE","evidence":[]},{"thesis_id":"01ACTIVE","evidence":[]}]}"#,
        &world,
    );
    assert!(verdicts(&lp).is_empty());
    assert!(lp.events("ThesisInvalidated").is_empty());
}

/// §6.7's judging order: a response's invalidations are judged first and its theses against what
/// is left, so a thesis on an instrument whose thesis the same response invalidates is judged
/// exactly as it would be with that instrument out of the working universe: a new admission, never
/// a renewal of the invalidated thesis.
#[test]
fn a_thesis_on_an_instrument_the_same_response_invalidates_is_judged_as_a_new_admission() {
    let response = |invalidate: bool| {
        format!(
            r#"{{"theses":[{}],"invalidations":[{}]}}"#,
            thesis("AAPL", &[SERVICES, SUPPLY], "x"),
            if invalidate {
                r#"{"thesis_id":"01ACTIVE","evidence":[]}"#
            } else {
                ""
            }
        )
    };
    let mut held = World::open();
    held.active = vec![active_on(AAPL, "AAPL", "01ACTIVE")];
    in_universe(&mut held, AAPL);
    let lp = judged_with(&response(true), &held);
    let fresh = judged_with(&response(false), &World::open());
    let thesis_of = |lp: &Loop| {
        let records: Vec<&FoldedEvent> = lp
            .shell
            .agent_journal
            .iter()
            .filter(|e| matches!(e.event_type.as_str(), "ThesisProposed" | "ThesisRevised"))
            .collect();
        let [record] = records.as_slice() else {
            panic!("one thesis record: {records:?}");
        };
        let p = &record.payload;
        (
            record.event_type.clone(),
            p.get("revision").cloned(),
            p.get("admitted").cloned(),
            p.get("reason").cloned(),
        )
    };
    assert_eq!(lp.events("ThesisInvalidated").len(), 1);
    assert_eq!(
        thesis_of(&lp),
        thesis_of(&fresh),
        "judged as if the instrument had left the universe"
    );
    let record = lp.events("ThesisProposed");
    assert_ne!(
        record
            .first()
            .and_then(|r| payload_text(&r.payload, "lineage_id")),
        Some("01ACTIVE"),
        "a new lineage, never the invalidated one's"
    );
}

/// An invalidation is never an opinion that adds risk: it journals no output and asks nothing of
/// the order path (HI-1, rule 2).
#[test]
fn an_invalidation_adds_no_output_and_no_intent() {
    let mut world = World::open();
    world.active = vec![active_on(MSFT, "MSFT", "01ACTIVE")];
    let lp = judged_with(
        r#"{"theses":[],"invalidations":[{"thesis_id":"01ACTIVE","evidence":[]}]}"#,
        &world,
    );
    assert_eq!(lp.events("ThesisInvalidated").len(), 1);
    assert!(lp.events("ModelOutputRecorded").is_empty());
    assert!(lp.events("IntentProposed").is_empty());
}

/// Removing an invalidated thesis's instrument frees its slot and nothing more: with the universe
/// at `max_instruments` (5) and the invalidated thesis's instrument outside it, the universe stays
/// full, so a corroborated thesis on AAPL is still refused `universe_full`. Removing more than the
/// invalidated thesis's own instrument would let it in past the limit.
#[test]
fn an_invalidation_frees_only_its_own_instrument_s_slot() {
    let mut world = World::open();
    world.active = vec![active_on(MSFT, "MSFT", "01GONE")];
    world.universe = WorkingUniverse::Known {
        instruments: [
            "7b4a1c2e-3333-4a2b-9c3d-000000000003",
            "7b4a1c2e-4444-4a2b-9c3d-000000000004",
            "7b4a1c2e-5555-4a2b-9c3d-000000000005",
            "7b4a1c2e-6666-4a2b-9c3d-000000000006",
            "7b4a1c2e-7777-4a2b-9c3d-000000000007",
        ]
        .iter()
        .map(|id| AssetId::new(id).unwrap_or_else(|e| panic!("{e}")))
        .collect(),
        pinned: false,
    };
    let lp = judged_with(
        &format!(
            r#"{{"theses":[{}],"invalidations":[{{"thesis_id":"01GONE","evidence":[]}}]}}"#,
            thesis("AAPL", &[SERVICES, SUPPLY], "x")
        ),
        &world,
    );
    assert_eq!(lp.events("ThesisInvalidated").len(), 1);
    assert_eq!(
        verdicts(&lp),
        vec!["universe_full".to_owned(), "judged".to_owned()],
        "the thesis is refused and the invalidation judged, theses first (rule 151)"
    );
}

/// A research batch the journal refused stored nothing (DEC-2204): until it is abandoned the single
/// writer holds trading behind it, and once it is, trading and the kill switch are taken at once,
/// the run in flight is dropped, and nothing is left in doubt.
#[test]
fn a_refused_research_batch_is_abandoned_and_trading_is_not_held_behind_it() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let started = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    let Some(ResearchRequest::Start { run_id, .. }) = started.requests.first().cloned() else {
        panic!("a due run starts: {started:?}");
    };
    let refused = mandate_runtime::handle_research(
        &mut lp.shell.state,
        &mut lp.research,
        ResearchInput::Retrieved {
            run_id,
            items: retrieved_items(&world, CUT_OFF),
        },
        &ports,
        &world.context(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        refused
            .effects
            .iter()
            .any(|e| matches!(e, Effect::Journal(_))),
        "the step has drafts the journal will refuse"
    );
    assert!(
        matches!(
            handle(&mut lp.shell.state, Input::Tick(clock(CUT_OFF + 1)), &ports),
            Err(RuntimeError::AppendUnresolved { .. })
        ),
        "a batch in doubt holds trading until it is settled"
    );
    assert!(mandate_runtime::abandon_refused_research(
        &mut lp.shell.state,
        &mut lp.research
    ));
    assert_eq!(lp.research.in_flight(), None, "the run is dropped");
    assert!(handle(&mut lp.shell.state, Input::Tick(clock(CUT_OFF + 1)), &ports).is_ok());
    let killed = handle(
        &mut lp.shell.state,
        Input::Command(Command::KillSwitch {
            scope: mandate_runtime::KillScope::Agent(mandate_runtime::AgentId(
                "agent-a".to_owned(),
            )),
            initiator: mandate_runtime::Initiator::Owner,
            confirmation: None,
        }),
        &ports,
    );
    assert!(killed.is_ok(), "the kill switch is taken: {killed:?}");
    assert!(
        !mandate_runtime::abandon_refused_research(&mut lp.shell.state, &mut lp.research),
        "nothing research wrote is in doubt any more"
    );
}

/// Only a research input's batch is ever abandoned: a trading batch in doubt keeps the writer held
/// until the shell settles it, as journal spec §5.1 requires.
#[test]
fn a_trading_batch_in_doubt_is_never_abandoned_as_research() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let paused = handle(&mut lp.shell.state, Input::Command(Command::Pause), &ports)
        .unwrap_or_else(|e| panic!("{e}"));
    assert!(
        paused.iter().any(|e| matches!(e, Effect::Journal(_))),
        "the pause journals a batch, left unappended"
    );
    assert!(!mandate_runtime::abandon_refused_research(
        &mut lp.shell.state,
        &mut lp.research
    ));
    assert!(matches!(
        handle(&mut lp.shell.state, Input::Tick(clock(CUT_OFF + 1)), &ports),
        Err(RuntimeError::AppendUnresolved { .. })
    ));
}

/// The alerts `step` raised, by key and subject.
fn alerts_of(step: &mandate_runtime::ResearchStep) -> Vec<(&'static str, String)> {
    step.effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Notify(reference) => {
                Some((reference.kind.key(), reference.subject_event.0.clone()))
            }
            _ => None,
        })
        .collect()
}

/// DEC-2397 item 3: a call the model-gateway service never answered whole ends its run with no
/// output and no record, alerts the operator once naming the run's last observation, and keeps the
/// pin: the next due run starts, where a gateway refusal of the same call withdraws it.
#[test]
#[ignore = "pending E21-62"]
fn a_lost_call_ends_its_run_alerts_and_keeps_the_pin() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    refused_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        RequestError::GatewayUnreachable,
        clock(CUT_OFF + 30),
    );
    assert!(lp.research.in_flight().is_none(), "the run ended");
    assert!(
        lp.events("ModelInvocationRecorded").is_empty(),
        "no record of a lost call"
    );
    let observed = lp.events("ObservationRecorded");
    let last = observed
        .last()
        .unwrap_or_else(|| panic!("the run observed items"));
    let lost = lp
        .steps
        .last()
        .unwrap_or_else(|| panic!("a step took the loss"));
    assert_eq!(
        alerts_of(lost),
        [("research_gateway_unreachable", last.event_id.0.clone())]
    );
    assert!(
        lost.requests.is_empty(),
        "the lost call is never sent again: {lost:?}"
    );
    let later = clock(CUT_OFF + 2 * i64::from(INTERVAL_S));
    assert_eq!(
        starts(&lp.run(ResearchInput::Poll(later), &ports, &world)).len(),
        1,
        "research stays on schedule"
    );

    let mut refused = Loop::new(1);
    boot(&mut refused, &ports);
    refused_run(
        &mut refused,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        RequestError::UnknownModel,
        clock(CUT_OFF + 30),
    );
    assert!(
        alerts_of(refused.steps.last().unwrap()).is_empty(),
        "a gateway refusal alerts no one"
    );
    assert!(
        starts(&refused.run(ResearchInput::Poll(later), &ports, &world)).is_empty(),
        "a gateway refusal withdraws the pin"
    );
}

/// DEC-2397 item 3: a lost call waits for the next due run, as a refused one does, and does not
/// start a run again at once.
#[test]
#[ignore = "pending E21-62"]
fn a_lost_call_waits_for_the_next_due_run() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let plane = RecordedPlane::load();
    refused_run(
        &mut lp,
        clock(CUT_OFF),
        &ports,
        &world,
        &plane,
        RequestError::GatewayUnreachable,
        clock(CUT_OFF + 30),
    );
    let soon = clock(CUT_OFF + 31);
    assert!(starts(&lp.run(ResearchInput::Poll(soon), &ports, &world)).is_empty());
}
