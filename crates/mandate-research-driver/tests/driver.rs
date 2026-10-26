//! The research driver's step machine (agent harness spec §5.3, §10.3; DEC-996 item 2; DEC-2204):
//! each input goes to the core; the step's drafts are appended as one batch and folded back; only
//! then are the worker's requests handed on, in order, and the step's other effects returned. An
//! append whose outcome is unknown holds every other input until the same one is retried (journal
//! spec §5.1), and an append the journal refuses halts research in the process, which never touches
//! trading (HI-7).

use std::cell::RefCell;
use std::rc::Rc;

use mandate_research_driver::{
    AppendFailure, Core, Driver, DriverError, JournalWriter, StepBatch, Stepped, Worker,
};
use mandate_runtime::{
    ActorKind, Effect, EventDraft, EventId, FoldedEvent, NotificationRef, ResearchInput,
    ResearchRequest, ResearchStep, RiskClock, RuntimeError, Seq,
};

type Log = Rc<RefCell<Vec<String>>>;

fn draft(id: &str) -> EventDraft {
    EventDraft {
        event_id: EventId(id.to_owned()),
        event_type: "ObservationRecorded".to_owned(),
        causation_id: None,
        payload: mandate_canon::Value::Null,
    }
}

fn start(run: &str) -> ResearchRequest {
    ResearchRequest::Start {
        run_id: run.to_owned(),
        cut_off: RiskClock::from_secs(100),
    }
}

fn poll(at: i64) -> ResearchInput {
    ResearchInput::Poll(RiskClock::from_secs(at))
}

/// A core that answers each input with the next scripted step, and logs what it is handed.
struct FakeCore {
    log: Log,
    steps: Vec<Result<ResearchStep, RuntimeError>>,
    fold_refuses: bool,
}

impl Core for FakeCore {
    fn step(&mut self, input: ResearchInput) -> Result<ResearchStep, RuntimeError> {
        self.log.borrow_mut().push(format!("core {input:?}"));
        if self.steps.is_empty() {
            return Ok(ResearchStep {
                effects: Vec::new(),
                requests: Vec::new(),
                artifacts: Vec::new(),
                correlation_id: None,
            });
        }
        self.steps.remove(0)
    }

    fn fold(&mut self, event: &FoldedEvent) -> Result<(), RuntimeError> {
        self.log
            .borrow_mut()
            .push(format!("fold {}", event.event_id.0));
        if self.fold_refuses {
            return Err(RuntimeError::NotStarted);
        }
        Ok(())
    }

    fn abandon(&mut self) -> bool {
        self.log.borrow_mut().push("abandon".to_owned());
        true
    }
}

/// A journal that answers each append with the next scripted outcome (committed by default).
struct FakeJournal {
    log: Log,
    seq: u64,
    outcomes: Vec<Option<AppendFailure>>,
    /// Rows the journal drops from the end of its reply, as a broken writer might.
    short: usize,
}

impl JournalWriter for FakeJournal {
    fn append(&mut self, batch: &StepBatch<'_>) -> Result<Vec<FoldedEvent>, AppendFailure> {
        let drafts = batch.drafts;
        if !batch.artifacts.is_empty() || batch.correlation_id.is_some() {
            self.log.borrow_mut().push(format!(
                "store {} artifacts for {:?}",
                batch.artifacts.len(),
                batch.correlation_id
            ));
        }
        let ids: Vec<&str> = drafts.iter().map(|d| d.event_id.0.as_str()).collect();
        self.log
            .borrow_mut()
            .push(format!("append {}", ids.join(",")));
        if !self.outcomes.is_empty()
            && let Some(failure) = self.outcomes.remove(0)
        {
            return Err(failure);
        }
        let kept = drafts.len().saturating_sub(self.short);
        Ok(drafts[..kept]
            .iter()
            .map(|d| {
                self.seq += 1;
                FoldedEvent {
                    stream: "agent:ws_1:agent_a".to_owned(),
                    seq: Seq(self.seq),
                    event_id: d.event_id.clone(),
                    event_type: d.event_type.clone(),
                    causation_id: d.causation_id.clone(),
                    actor: ActorKind::Agent,
                    payload: d.payload.clone(),
                }
            })
            .collect())
    }
}

struct FakeWorker {
    log: Log,
}

impl Worker for FakeWorker {
    fn hand(&mut self, request: &ResearchRequest) {
        self.log.borrow_mut().push(format!("hand {request:?}"));
    }
}

struct Parts {
    log: Log,
    core: FakeCore,
    journal: FakeJournal,
    worker: FakeWorker,
}

fn parts(
    steps: Vec<Result<ResearchStep, RuntimeError>>,
    outcomes: Vec<Option<AppendFailure>>,
) -> Parts {
    let log: Log = Rc::default();
    Parts {
        core: FakeCore {
            log: log.clone(),
            steps,
            fold_refuses: false,
        },
        journal: FakeJournal {
            log: log.clone(),
            seq: 0,
            outcomes,
            short: 0,
        },
        worker: FakeWorker { log: log.clone() },
        log,
    }
}

fn step_with(effects: Vec<Effect>, requests: Vec<ResearchRequest>) -> ResearchStep {
    ResearchStep {
        effects,
        requests,
        artifacts: Vec::new(),
        correlation_id: None,
    }
}

fn notice() -> Effect {
    Effect::Notify(NotificationRef {
        subject_event: EventId("e1".to_owned()),
        kind: mandate_runtime::RuntimeAlert::ResearchCallRefused,
    })
}

impl Parts {
    fn step(&mut self, driver: &mut Driver, input: ResearchInput) -> Result<Stepped, DriverError> {
        driver.step(input, &mut self.core, &mut self.journal, &mut self.worker)
    }

    fn log(&self) -> Vec<String> {
        self.log.borrow().clone()
    }
}

#[test]
fn the_drafts_commit_and_fold_back_before_any_request_is_handed_on() {
    let mut p = parts(
        vec![Ok(step_with(
            vec![Effect::Journal(draft("e1")), Effect::Journal(draft("e2"))],
            vec![start("run_1")],
        ))],
        vec![],
    );
    let mut driver = Driver::new();
    let stepped = p.step(&mut driver, poll(1));
    assert_eq!(
        stepped,
        Ok(Stepped::Handed {
            requests: vec![start("run_1")],
            effects: Vec::new(),
        })
    );
    assert_eq!(
        p.log(),
        vec![
            format!("core {:?}", poll(1)),
            "append e1,e2".to_owned(),
            "fold e1".to_owned(),
            "fold e2".to_owned(),
            format!("hand {:?}", start("run_1")),
        ],
        "one batch, folded back in order, then the request (§5.3)"
    );
}

#[test]
fn a_step_with_no_drafts_appends_nothing() {
    let mut p = parts(vec![Ok(step_with(vec![], vec![start("run_1")]))], vec![]);
    let mut driver = Driver::new();
    assert!(p.step(&mut driver, poll(1)).is_ok());
    assert_eq!(
        p.log(),
        vec![
            format!("core {:?}", poll(1)),
            format!("hand {:?}", start("run_1")),
        ]
    );
}

#[test]
fn an_unknown_append_holds_every_other_input_until_the_same_one_is_retried() {
    let step = step_with(vec![Effect::Journal(draft("e1"))], vec![start("run_1")]);
    let mut p = parts(
        vec![Ok(step.clone()), Ok(step)],
        vec![Some(AppendFailure::Unresolved)],
    );
    let mut driver = Driver::new();
    assert_eq!(p.step(&mut driver, poll(1)), Ok(Stepped::Held));
    assert_eq!(driver.pending(), Some(&poll(1)));
    assert!(
        !driver.halted(),
        "an unknown append holds, it does not halt"
    );
    assert!(
        !p.log().iter().any(|line| line.starts_with("hand")),
        "nothing is handed on before its drafts are known committed"
    );
    assert_eq!(
        p.step(&mut driver, poll(2)),
        Err(DriverError::Held),
        "another input waits"
    );
    assert_eq!(
        p.log().iter().filter(|l| l.starts_with("core")).count(),
        1,
        "and never reaches the core"
    );
    let retried = p.step(&mut driver, poll(1));
    assert!(matches!(retried, Ok(Stepped::Handed { .. })), "{retried:?}");
    assert_eq!(driver.pending(), None);
    let log = p.log();
    assert_eq!(
        log.last(),
        Some(&format!("hand {:?}", start("run_1"))),
        "{log:?}"
    );
}

#[test]
fn a_refused_append_halts_research_in_the_process() {
    let step = step_with(vec![Effect::Journal(draft("e1"))], vec![start("run_1")]);
    let mut p = parts(
        vec![Ok(step)],
        vec![Some(AppendFailure::Refused {
            code: "schema".to_owned(),
        })],
    );
    let mut driver = Driver::new();
    assert_eq!(
        p.step(&mut driver, poll(1)),
        Err(DriverError::Refused {
            code: "schema".to_owned()
        })
    );
    assert!(driver.halted());
    assert_eq!(driver.pending(), None);
    assert!(
        p.log().iter().any(|line| line == "abandon"),
        "the core forgets the batch, so nothing waits behind it"
    );
    for input in [poll(1), poll(2)] {
        assert_eq!(
            p.step(&mut driver, input),
            Err(DriverError::Halted),
            "a batch the journal refuses never commits, so research stays stopped"
        );
    }
    assert!(!p.log().iter().any(|line| line.starts_with("hand")));
}

#[test]
fn a_core_refusal_appends_nothing_and_hands_nothing() {
    let mut p = parts(vec![Err(RuntimeError::NotStarted)], vec![]);
    let mut driver = Driver::new();
    assert_eq!(
        p.step(&mut driver, poll(1)),
        Err(DriverError::Core(RuntimeError::NotStarted))
    );
    assert_eq!(p.log(), vec![format!("core {:?}", poll(1))]);
    assert_eq!(
        driver.pending(),
        None,
        "nothing was drafted, so nothing is in doubt"
    );
}

#[test]
fn the_other_effects_come_back_after_the_commit_in_their_order() {
    let mut p = parts(
        vec![Ok(step_with(
            vec![notice(), Effect::Journal(draft("e1"))],
            vec![],
        ))],
        vec![],
    );
    let mut driver = Driver::new();
    assert_eq!(
        p.step(&mut driver, poll(1)),
        Ok(Stepped::Handed {
            requests: Vec::new(),
            effects: vec![notice()],
        }),
        "a notice names a committed record, so it is the shell's to send after the batch commits"
    );
}

#[test]
fn a_held_step_returns_no_effects_to_run() {
    let mut p = parts(
        vec![Ok(step_with(
            vec![notice(), Effect::Journal(draft("e1"))],
            vec![],
        ))],
        vec![Some(AppendFailure::Unresolved)],
    );
    let mut driver = Driver::new();
    assert_eq!(p.step(&mut driver, poll(1)), Ok(Stepped::Held));
}

#[test]
fn a_committed_event_the_core_cannot_fold_back_halts_research() {
    let step = step_with(
        vec![Effect::Journal(draft("e1")), Effect::Journal(draft("e2"))],
        vec![start("run_1")],
    );
    let mut p = parts(vec![Ok(step)], vec![]);
    p.core.fold_refuses = true;
    let mut driver = Driver::new();
    assert!(
        matches!(
            p.step(&mut driver, poll(1)),
            Err(DriverError::Diverged { .. })
        ),
        "the shell must rebuild the core from the journal"
    );
    assert!(
        !p.log().iter().any(|line| line == "abandon"),
        "a committed batch is never forgotten"
    );
    assert!(
        driver.halted(),
        "the core's state is unknown, so nothing more is judged"
    );
    assert!(!p.log().iter().any(|line| line.starts_with("hand")));
    assert!(
        !p.log().iter().any(|line| line == "fold e2"),
        "folding stops at the first refusal"
    );
    assert_eq!(p.step(&mut driver, poll(2)), Err(DriverError::Halted));
}

#[test]
fn a_reply_that_is_not_the_batch_is_a_divergence_and_nothing_is_folded_or_handed() {
    let step = step_with(
        vec![Effect::Journal(draft("e1")), Effect::Journal(draft("e2"))],
        vec![start("run_1")],
    );
    let mut p = parts(vec![Ok(step)], vec![]);
    p.journal.short = 1;
    let mut driver = Driver::new();
    assert!(matches!(
        p.step(&mut driver, poll(1)),
        Err(DriverError::Diverged { .. })
    ));
    assert!(driver.halted());
    let log = p.log();
    assert!(!log.iter().any(|line| line.starts_with("fold")), "{log:?}");
    assert!(!log.iter().any(|line| line.starts_with("hand")), "{log:?}");
    assert_eq!(p.step(&mut driver, poll(2)), Err(DriverError::Halted));
}

/// DEC-2206 items 4 and 8: the journal writer is handed the step's own artifacts and run with its
/// drafts, in one call before anything is handed on, so the artifacts are stored before the batch
/// that names them and every draft carries its run.
#[test]
fn the_journal_is_handed_the_step_s_artifacts_and_run_with_its_drafts() {
    let mut step = step_with(vec![Effect::Journal(draft("01A"))], vec![start("01RUN")]);
    step.artifacts = vec![b"universe".to_vec(), b"arguments".to_vec()];
    step.correlation_id = Some("01RUN".to_owned());
    let mut p = parts(vec![Ok(step)], Vec::new());
    let mut driver = Driver::new();
    driver
        .step(poll(100), &mut p.core, &mut p.journal, &mut p.worker)
        .unwrap_or_else(|e| panic!("{e}"));
    let log = p.log.borrow();
    let at = |prefix: &str| {
        log.iter()
            .position(|line| line.starts_with(prefix))
            .unwrap_or_else(|| panic!("no `{prefix}` in {log:?}"))
    };
    assert_eq!(log[at("store")], r#"store 2 artifacts for Some("01RUN")"#);
    assert!(at("store") < at("append"), "{log:?}");
    assert!(at("append") < at("hand"), "{log:?}");
}
