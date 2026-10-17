//! The research loop in the core (agent harness spec §5.3, §5.4, §6.2, §6.5, §7): when a run
//! starts and ends, what is journaled before the model reads anything, and how a model's answer is
//! checked and judged. Each case names the rule it holds.

mod common;

use common::research::{
    AAPL, CUT_OFF, INTERVAL_S, Loop, MSFT, RESEARCH_MODEL, RESPONSE_AAPL, RecordedGateway,
    RecordedPlane, World, full_run, pin, production_preconditions, recorded, reply_with,
    worker_run,
};
use common::{
    ACCOUNT_STREAM, AllowGate, FixedPlan, TestIds, clock, event, ports, text, universe, with_clock,
};
use mandate_canon::Value;
use mandate_research::{AssetId, WorkingUniverse};
use mandate_research_run::{CallFailure, CallOutcome, ErrorKind, ModelPin, ResearchCall};
use mandate_runtime::{
    ActiveThesis, Command, Effect, Input, Mode, ResearchInput, ResearchRequest, RiskClock,
    RuntimeError, WriterEpoch, handle,
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

fn failing(kind: ErrorKind) -> RecordedGateway<impl Fn(&ResearchCall) -> CallOutcome> {
    RecordedGateway {
        answer: move |request: &ResearchCall| {
            Err(CallFailure {
                call_id: request.call_id.clone(),
                kind,
                completed_at: request.as_of,
            })
        },
        calls: std::cell::RefCell::new(Vec::new()),
    }
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

#[test]
fn no_run_starts_while_the_journal_cannot_record_the_verdicts() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
    world.preconditions = production_preconditions();
    assert!(
        !world.preconditions.hold(),
        "mandate-journal registers neither record yet"
    );
    let step = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
    assert!(
        step.requests.is_empty(),
        "§6.2 preconditions: no run starts"
    );
    assert!(step.effects.is_empty());
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
    let gateway = failing(ErrorKind::ProviderUnavailable);
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

#[test]
fn exits_only_starts_no_run_until_an_invalidation_can_be_journaled() {
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
    }
    let calls = gateway.calls.borrow();
    assert_eq!(calls.len(), 1, "HI-8: one call per run");
    assert_eq!(calls[0].model, pin());
    assert!(
        calls[0]
            .inputs
            .items
            .iter()
            .all(|item| item.item_id.as_str() != "nw-20261017-aapl-late"),
        "HI-13: the item known after the cut-off is never sent"
    );
    assert!(
        calls[0]
            .inputs
            .items
            .iter()
            .all(|item| item.source.as_str() != "src.anonymous_blog"),
        "DEC-101: the source off the allowlist is never sent"
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
    for kind in [
        ErrorKind::BudgetExhausted,
        ErrorKind::DeadlineExceeded,
        ErrorKind::SchemaInvalid,
        ErrorKind::IdentityMismatch,
        ErrorKind::PolicyDenied,
    ] {
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
        assert_eq!(records.len(), 1, "{}", kind.as_str());
        assert_eq!(
            payload_text(&records[0].payload, "outcome"),
            Some(kind.as_str())
        );
        assert_eq!(
            payload_text(&records[0].payload, "purpose"),
            Some("research")
        );
        assert!(
            lp.events("ModelOutputRecorded").is_empty(),
            "HI-6: {}",
            kind.as_str()
        );
        assert!(lp.events("ThesisProposed").is_empty());
        assert!(lp.events("CandidateVerdictsRecorded").is_empty());
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
        &failing(ErrorKind::PolicyDenied),
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
    assert_eq!(alerts[0].message_key, "research_call_refused");
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
    let gateway = failing(ErrorKind::DeadlineExceeded);
    let mut alerts = Vec::new();
    for run in 0..4 {
        let now = CUT_OFF + run * (i64::from(INTERVAL_S) + 100);
        full_run(&mut lp, clock(now), &ports, &world, &plane, &gateway);
        let called = &lp.steps[lp.steps.len() - 2];
        alerts.push(
            called
                .effects
                .iter()
                .filter(|effect| matches!(effect, Effect::Notify(r) if r.message_key == "research_deadline_streak"))
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
        &failing(ErrorKind::ModelWithdrawn),
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
fn an_admitted_thesis_is_judged_only_after_its_call_record_commits() {
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
    let drafted: Vec<String> = called
        .effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::Journal(draft) => Some(draft.event_type.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        drafted,
        vec!["ModelInvocationRecorded"],
        "§6.2 step 4 alone"
    );
    lp.run(ResearchInput::Poll(clock(CUT_OFF + 31)), &ports, &world);
    let types = lp.types();
    let record = types
        .iter()
        .position(|t| t == "ModelInvocationRecorded")
        .unwrap_or_else(|| panic!("recorded"));
    let thesis = types
        .iter()
        .position(|t| t == "ThesisProposed")
        .unwrap_or_else(|| panic!("judged"));
    assert!(
        record < thesis,
        "HI-2: the response is committed before anything reads it"
    );
    assert_eq!(
        &types[thesis..],
        &[
            "ThesisProposed".to_owned(),
            "ModelOutputRecorded".to_owned(),
            "CandidateVerdictsRecorded".to_owned()
        ]
    );
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
    let owned = output.to_owned();
    let gateway = RecordedGateway {
        answer: move |request: &ResearchCall| Ok(reply_with(request, owned.as_bytes())),
        calls: std::cell::RefCell::new(Vec::new()),
    };
    full_run(&mut lp, clock(CUT_OFF), &ports, world, &plane, &gateway);
    lp
}

fn verdicts(lp: &Loop) -> Vec<String> {
    lp.events("CandidateVerdictsRecorded")
        .first()
        .and_then(|event| event.payload.get("verdicts"))
        .and_then(Value::as_array)
        .unwrap_or_default()
        .iter()
        .filter_map(|verdict| payload_text(verdict, "verdict").map(str::to_owned))
        .collect()
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
        (
            thesis("AAPL", &[SERVICES, SUPPLY], "x").replace(
                r#""evidence""#,
                r#""predecessor_thesis_id":"01PRED","evidence""#,
            ),
            "revisions_not_enabled",
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
fn an_invalidation_of_no_active_thesis_is_unknown_and_one_of_an_active_thesis_is_accepted() {
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
            r#"{{"theses":[],"invalidations":[{{"thesis_id":"01NOPE","evidence":[]}},{{"thesis_id":"01ACTIVE","evidence":["{SERVICES}"]}}]}}"#
        ),
        &world,
    );
    assert_eq!(
        verdicts(&lp),
        vec!["unknown_thesis".to_owned(), "accepted".to_owned()]
    );
    assert!(
        lp.events("ModelOutputRecorded").is_empty(),
        "an invalidation never adds an output"
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
fn an_output_the_harness_cannot_parse_is_journaled_as_rejected_with_no_candidates() {
    let world = World::open();
    let lp = judged_with(
        r#"{"theses":[{"instrument":"AAPL"}],"invalidations":[]}"#,
        &world,
    );
    let judged = lp.events("CandidateVerdictsRecorded");
    assert_eq!(judged.len(), 1);
    assert_eq!(
        payload_text(&judged[0].payload, "outcome"),
        Some("output_rejected")
    );
    assert!(verdicts(&lp).is_empty());
    assert!(lp.events("ThesisProposed").is_empty());
}

#[test]
fn an_output_judged_under_another_pin_is_not_pinned() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let mut world = World::open();
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
    if let Some(config) = world.config.as_mut() {
        config.pin = ModelPin {
            model_version: "0.2.0".to_owned(),
            ..pin()
        };
    }
    let polled = lp.run(ResearchInput::Poll(clock(CUT_OFF + 31)), &ports, &world);
    assert!(starts(&polled).is_empty());
    let judged = lp.events("CandidateVerdictsRecorded");
    assert_eq!(judged.len(), 1, "HI-22: the candidates are still journaled");
    assert_eq!(
        payload_text(&judged[0].payload, "outcome"),
        Some("not_pinned")
    );
    assert!(
        lp.events("ThesisProposed").is_empty(),
        "HI-17: the version in effect judges"
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
    let plane = RecordedPlane::load();
    let gateway = recorded(RESPONSE_AAPL);
    let requests = {
        let started = lp.run(ResearchInput::Poll(clock(CUT_OFF)), &ports, &world);
        started.requests
    };
    let Some(ResearchRequest::Start { run_id, cut_off }) = requests.first().cloned() else {
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
        &ports,
        &world,
    );
    let Some(ResearchRequest::Call(plan)) = retrieved.requests.first().cloned() else {
        panic!("a call is requested");
    };
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
    let call = ResearchCall::build(
        &plan,
        &retrieval,
        world.call_context(),
        mandate_time::UtcNanos::EPOCH,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let outcome = mandate_research_run::settle(
        &call,
        mandate_research_run::ModelGateway::call(&gateway, &call),
    );
    lp.run(
        ResearchInput::Called {
            run_id,
            at: clock(CUT_OFF + 600),
            outcome,
        },
        &ports,
        &world,
    );
    let records = lp.events("ModelInvocationRecorded");
    assert_eq!(records.len(), 1, "§5.3: the record of the call is complete");
    assert_eq!(records[0].payload.get("used"), Some(&Value::Bool(false)));
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
            outcome: Err(CallFailure {
                call_id: "x".to_owned(),
                kind: ErrorKind::ProviderUnavailable,
                completed_at: mandate_time::UtcNanos::EPOCH,
            }),
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
    assert!(
        restarted.events("ThesisProposed").is_empty(),
        "§5.4: not judged after the restart"
    );
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
    let gateway = failing(ErrorKind::ProviderUnavailable);
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

fn rejection(lp: &Loop) -> (Option<String>, Option<String>) {
    let judged = lp.events("CandidateVerdictsRecorded");
    assert_eq!(
        judged.len(),
        1,
        "HI-22: one verdict record per committed call"
    );
    (
        payload_text(&judged[0].payload, "outcome").map(str::to_owned),
        payload_text(&judged[0].payload, "rejection").map(str::to_owned),
    )
}

#[test]
fn a_blank_invalidation_rejects_the_output_with_a_verdict_record() {
    let world = World::open();
    let blank = thesis("AAPL", &[SERVICES, SUPPLY], "x").replace("It stops.", "   ");
    let lp = judged_with(
        &format!(r#"{{"theses":[{blank}],"invalidations":[]}}"#),
        &world,
    );
    assert_eq!(
        rejection(&lp),
        (Some("output_rejected".to_owned()), Some("range".to_owned()))
    );
    assert!(lp.events("ThesisProposed").is_empty());
}

#[test]
fn a_candidate_the_platform_cannot_fill_rejects_the_output_with_a_verdict_record() {
    let mut world = World::open();
    if let Some(config) = world.config.as_mut() {
        config.pin.model_id = String::new();
    }
    let lp = judged_with(RESPONSE_AAPL, &world);
    assert_eq!(
        rejection(&lp),
        (
            Some("output_rejected".to_owned()),
            Some("unfillable_candidate".to_owned())
        )
    );
    assert!(lp.events("ThesisProposed").is_empty());
    assert!(lp.events("ModelOutputRecorded").is_empty());
}

#[test]
fn a_snapshot_the_platform_cannot_read_rejects_the_output_with_a_verdict_record() {
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
    assert_eq!(
        rejection(&lp),
        (
            Some("output_rejected".to_owned()),
            Some("unreadable_snapshot".to_owned())
        )
    );
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
fn called_unfolded(
    lp: &mut Loop,
    ports: &mandate_runtime::Ports<'_>,
    world: &World,
    outcome: impl Fn(&ResearchCall) -> CallOutcome,
) -> (String, Vec<mandate_runtime::EventDraft>) {
    let plane = RecordedPlane::load();
    let started = starts(&lp.run(ResearchInput::Poll(clock(CUT_OFF)), ports, world));
    let run_id = started[0].0.clone();
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
    let call = ResearchCall::build(
        &plan,
        &retrieval,
        world.call_context(),
        mandate_time::UtcNanos::EPOCH,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let answer = outcome(&call);
    let step = mandate_runtime::handle_research(
        &mut lp.shell.state,
        &mut lp.research,
        ResearchInput::Called {
            run_id: run_id.clone(),
            at: clock(CUT_OFF + 30),
            outcome: answer,
        },
        ports,
        &world.context(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    let drafts = step
        .effects
        .into_iter()
        .filter_map(|effect| match effect {
            Effect::Journal(draft) => Some(draft),
            _ => None,
        })
        .collect();
    (run_id, drafts)
}

#[test]
fn the_core_re_checks_the_answer_s_call_and_identity_whatever_the_worker_did() {
    for change in 0..2 {
        let f = Fixture::new();
        let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
        let mut lp = Loop::new(1);
        boot(&mut lp, &ports);
        let world = World::open();
        let (_, drafts) = called_unfolded(&mut lp, &ports, &world, |call| {
            let mut reply = reply_with(call, RESPONSE_AAPL.as_bytes());
            if change == 0 {
                reply.reported.model_version = "9.9.9".to_owned();
            } else {
                reply.call_id = "01SOMEOTHERCALL".to_owned();
            }
            Ok(reply)
        });
        assert_eq!(drafts.len(), 1, "the call record alone");
        assert_eq!(
            payload_text(&drafts[0].payload, "outcome"),
            Some("identity_mismatch"),
            "change {change}"
        );
        assert_eq!(
            lp.research.in_flight(),
            None,
            "a substituted answer ends the run unjudged"
        );
    }
}

#[test]
fn the_schedule_counts_from_an_answer_before_its_record_folds_back() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    called_unfolded(&mut lp, &ports, &world, |call| {
        Err(CallFailure {
            call_id: call.call_id.clone(),
            kind: ErrorKind::ProviderUnavailable,
            completed_at: call.as_of,
        })
    });
    assert_eq!(lp.research.last_call(), Some(clock(CUT_OFF + 30)));
}

#[test]
fn a_call_record_that_never_folds_back_frees_the_loop_after_the_bound() {
    let f = Fixture::new();
    let ports = ports(&f.ids, &AllowGate, &f.plan, &f.view);
    let mut lp = Loop::new(1);
    boot(&mut lp, &ports);
    let world = World::open();
    let (run_id, drafts) = called_unfolded(&mut lp, &ports, &world, |call| {
        Ok(reply_with(call, RESPONSE_AAPL.as_bytes()))
    });
    for event in drafts {
        let seq = lp.shell.head().0 + 1;
        let stored = mandate_runtime::FoldedEvent {
            stream: common::AGENT_STREAM.to_owned(),
            seq: mandate_runtime::Seq(seq),
            event_id: event.event_id.clone(),
            event_type: event.event_type.clone(),
            causation_id: event.causation_id.clone(),
            actor: mandate_runtime::ActorKind::Agent,
            payload: event.payload.clone(),
        };
        lp.shell.agent_journal.push(stored.clone());
        mandate_runtime::fold(&mut lp.shell.state, &stored).unwrap_or_else(|e| panic!("{e}"));
    }
    let bound = CUT_OFF + 60 + 120;
    lp.run(ResearchInput::Poll(clock(bound)), &ports, &world);
    assert_eq!(
        lp.research.in_flight(),
        Some(run_id.as_str()),
        "within the bound it waits"
    );
    lp.run(ResearchInput::Poll(clock(bound + 1)), &ports, &world);
    assert_eq!(
        lp.research.in_flight(),
        None,
        "past the bound the unfolded record is given up"
    );
    assert!(lp.events("ThesisProposed").is_empty());
}
