//! The asked cycle's install probes (E7-26, G1b's grant wait; DEC-940): the in-module pins of
//! [`crate::tracer::ProductionCycle::run_observed_asked_watched`], DEC-77's 2026-09-27 amendment's
//! place for a fact only the private session shows. A cycle that asks keeps its session open for
//! the runner's wait; every wake hands the control stream's answers to the runtime, which writes
//! its own `ApprovalResponded` and `ApprovalRevalidated` (DEC-920 item 1); the opening the
//! runtime then releases is held, never sent, until the runner's own release; and a cycle that
//! never asks never calls the wait, so a paper `auto` run is the run it was. Each probe drives
//! one whole cycle over the doubles' world and reads its answer off the committed
//! [`crate::stages::doubles::Ledger`] and the connector's tally, never off the session's state.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use mandate_alpaca::Pause;
use mandate_canon::{Digest, Value};
use mandate_runtime::{Autonomy, Observation, RiskClock};
use mandate_time::UtcNanos;

use super::doubles::{
    FixedClassifier, World, agent_stream, control_stream, instrument, output, seed, setup,
};
use super::tail_install::ApprovableMandate;
use crate::error::{Cause, ShellError};
use crate::lt10::watch_may_end;
use crate::stages::Stage;
use crate::tracer::{AskedCycle, ProductionCycle, Report, Wake};
use crate::watch::Watch;

/// The closes the fixture observation carries, stored under their own digest.
const CLOSES: &[u8] = br#"{"closes":[["2026-09-25","255.2"]]}"#;

/// The founder's answer's event id on the control stream.
const ANSWER: &str = "00000100000000000000000002";

/// The watch's clock: an injected cell moved on by each pause.
#[derive(Clone)]
struct Clock {
    instant: Rc<Cell<UtcNanos>>,
}

impl Pause for Clock {
    fn now(&self) -> UtcNanos {
        self.instant.get()
    }

    async fn pause(&self, duration: Duration) {
        if let Ok(step) = i64::try_from(duration.as_secs())
            && let Ok(next) =
                UtcNanos::from_parts(self.instant.get().secs().saturating_add(step), 0)
        {
            self.instant.set(next);
        }
    }
}

/// The run clock as the fixture's setup holds it.
fn clock() -> Result<Clock, String> {
    Ok(Clock {
        instant: Rc::new(Cell::new(setup()?.now)),
    })
}

/// The doubles' stages with the classifier answering `autonomy`, the mandate naming the founder
/// as its approver, and the observation's closes stored.
fn cycle(world: &World, autonomy: Autonomy, place: bool) -> Result<ProductionCycle, String> {
    let mut stages = world.stages();
    stages.classifier = Box::new(FixedClassifier {
        world: world.clone(),
        autonomy,
    });
    stages.mandate = Box::new(ApprovableMandate {
        world: world.clone(),
    });
    let artifacts: Arc<dyn mandate_journal::ArtifactSource + Send + Sync> =
        Arc::new(BTreeMap::from([(Digest::of(CLOSES), CLOSES.to_vec())]));
    stages.artifacts = Some(artifacts);
    let mut setup = setup()?;
    setup.place_one_order = place;
    Ok(ProductionCycle::new(stages, setup))
}

fn observation() -> Result<Observation, String> {
    Ok(Observation {
        source: "alpaca_iex_daily_bars".to_owned(),
        instrument_id: Some(instrument().map_err(|cause| cause.to_string())?),
        as_of: RiskClock::from_secs(setup()?.now.secs()),
        data_ref: Digest::of(CLOSES),
    })
}

/// The watch the runner hands in: bound and interval as the fixtures' watches take them.
fn watch(clock: &Clock) -> Result<Watch<Clock>, String> {
    Ok(Watch {
        pause: clock.clone(),
        bound: setup()?.now,
        interval: Duration::from_secs(30),
    })
}

/// An end question that ends the run at its first ask and counts its asks.
fn ending(
    asks: &Rc<Cell<u32>>,
) -> impl Fn(Option<&str>, bool, &[&'static str], UtcNanos, UtcNanos) -> Result<bool, ShellError> + use<>
{
    let asks = Rc::clone(asks);
    move |_, _, _, _, _| {
        asks.set(asks.get().saturating_add(1));
        Ok(true)
    }
}

/// The ask's event id and content hash, read from the request the cycle committed.
fn the_ask(world: &World) -> Result<(String, String), String> {
    let requests = world.ledger.borrow().parsed("ApprovalRequested");
    let [request] = &requests[..] else {
        return Err(format!("{} requests, not one", requests.len()));
    };
    let id = request.get("event_id").and_then(Value::as_str);
    let hash = request
        .get("payload")
        .and_then(|payload| payload.get("content_hash"))
        .and_then(Value::as_str);
    match (id, hash) {
        (Some(id), Some(hash)) => Ok((id.to_owned(), hash.to_owned())),
        _ => Err("the request names no id or hash".to_owned()),
    }
}

/// The founder's `cli_confirm` grant of the cycle's ask, committed on the control stream.
fn answer(world: &World) -> Result<(), String> {
    let (approval, content_hash) = the_ask(world)?;
    let at = setup()?.now.secs();
    let payload = format!(
        r#"{{"agent":"tracer-aapl","approval":"{approval}","verdict":"approved","content_hash":"{content_hash}","submitted_at":{at},"step_up":{{"assertion_id":"first-gesture","authenticated_at":{at},"method":"cli_confirm"}},"responder":"user-founder","role":"approver"}}"#
    );
    seed(
        world,
        &control_stream(),
        1,
        ANSWER,
        "ApprovalResponseSubmitted",
        "user",
        &payload,
    )
}

fn types(rows: &[mandate_journal::StoredEvent]) -> Vec<String> {
    rows.iter().map(|row| row.event_type.clone()).collect()
}

fn submissions(world: &World) -> u32 {
    world.tally.borrow().submissions
}

/// DEC-920 item 1 and DEC-940: a wake hands the answer committed after the cycle asked to the
/// runtime, which writes its copy and its re-validation on the agent stream; the opening the
/// runtime releases is held — nothing reaches the connector — until the runner submits it, and
/// then exactly once.
#[test]
fn a_wake_hands_the_answer_to_the_runtime_and_holds_the_opening() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let log = Rc::clone(&seen);
    let w = world.clone();
    let now = clock.now();
    let report: Result<Report, ShellError> = cycle(&world, Autonomy::Ask, true)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            |asked: &mut dyn AskedCycle| {
                let agent = asked.agent_rows()?;
                log.borrow_mut().push(types(&agent));
                let quiet = asked.wake(now)?;
                log.borrow_mut().push(types(&quiet.agent));
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                let Wake { agent, control } = asked.wake(now)?;
                log.borrow_mut().push(types(&agent));
                log.borrow_mut().push(types(&control));
                log.borrow_mut()
                    .push(vec![format!("held:{}", w.tally.borrow().submissions)]);
                asked.submit()
            },
            &end,
        );
    let report = report.map_err(|error| error.to_string())?;
    let seen = seen.borrow();
    let has = |at: usize, event_type: &str| {
        seen.get(at)
            .is_some_and(|types| types.iter().any(|t| t == event_type))
    };
    assert!(
        has(0, "ApprovalRequested"),
        "the ask is committed: {seen:?}"
    );
    assert!(
        !has(1, "ApprovalResponded"),
        "an unanswered wake writes no copy: {seen:?}"
    );
    assert!(
        has(2, "ApprovalResponded") && has(2, "ApprovalRevalidated"),
        "the wake after the answer reads the runtime's own records: {seen:?}"
    );
    assert!(
        has(3, "ApprovalResponseSubmitted"),
        "the control stream is read back: {seen:?}"
    );
    assert_eq!(
        seen.get(4).cloned(),
        Some(vec!["held:0".to_owned()]),
        "the released opening is held until the runner submits it"
    );
    let copies = world.ledger.borrow().parsed("ApprovalResponded");
    let [copy] = &copies[..] else {
        return Err(format!("{} copies, not one", copies.len()));
    };
    assert_eq!(
        copy.get("causation_id").and_then(Value::as_str),
        Some(ANSWER),
        "the copy is of the committed answer"
    );
    assert_eq!(submissions(&world), 1, "the opening is sent once");
    assert_eq!(report.submitted.len(), 1, "{report:?}");
    assert_eq!(
        asks.get(),
        1,
        "the asked run's watch ends through the caller's question"
    );
    Ok(())
}

/// A paper `auto` run is unchanged: a cycle that never asks never calls the wait, submits as
/// before, and its watch asks the shell's own LT-10 question, never the caller's.
#[test]
fn a_cycle_that_never_asks_never_waits() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let waited = Rc::new(Cell::new(false));
    let flag = Rc::clone(&waited);
    let report: Result<Report, ShellError> = cycle(&world, Autonomy::Auto, true)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            |_: &mut dyn AskedCycle| {
                flag.set(true);
                Ok(())
            },
            &end,
        );
    assert!(!waited.get(), "an auto cycle never waits");
    assert_eq!(submissions(&world), 1, "the auto opening is sent at once");
    assert_eq!(
        asks.get(),
        0,
        "the auto run's watch asks the shell's own question"
    );
    assert_eq!(
        report.as_ref().err().map(ShellError::code),
        Some("cancel_unconfirmed"),
        "the shell's own watch fails closed at the fixture's cap: {report:?}"
    );
    Ok(())
}

/// Rule 3: a wait that stops sends nothing, and the opening the runtime released stays held.
#[test]
fn a_refused_wait_sends_nothing() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let w = world.clone();
    let now = clock.now();
    let report: Result<Report, ShellError> = cycle(&world, Autonomy::Ask, true)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                asked.wake(now)?;
                Err(ShellError::ProposalInvalid)
            },
            &end,
        );
    assert_eq!(
        report.as_ref().err().map(ShellError::code),
        Some("proposal_invalid"),
        "the wait's own stop is the run's: {report:?}"
    );
    assert_eq!(
        world.ledger.borrow().count("ApprovalRevalidated"),
        1,
        "the runtime released the opening"
    );
    assert_eq!(submissions(&world), 0, "nothing is sent");
    assert_eq!(asks.get(), 0, "a stopped run watches nothing");
    Ok(())
}

/// A wait that returns without the runner's release sends nothing: the held opening is refused,
/// never sent behind the runner's back.
#[test]
fn a_wait_that_never_submits_sends_nothing() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let w = world.clone();
    let now = clock.now();
    let report: Result<Report, ShellError> = cycle(&world, Autonomy::Ask, true)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                asked.wake(now).map(|_| ())
            },
            &end,
        );
    assert!(
        matches!(
            report,
            Err(ShellError::Refused {
                stage: Stage::Executor,
                cause: Cause::Absent { .. },
            })
        ),
        "an unreleased held opening refuses: {report:?}"
    );
    assert_eq!(submissions(&world), 0, "nothing is sent");
    Ok(())
}

/// A submit with no released opening sends nothing and refuses.
#[test]
fn a_submit_with_nothing_held_refuses() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let report: Result<Report, ShellError> = cycle(&world, Autonomy::Ask, true)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            |asked: &mut dyn AskedCycle| asked.submit(),
            &end,
        );
    assert!(
        matches!(
            report,
            Err(ShellError::Refused {
                stage: Stage::Executor,
                cause: Cause::Absent { .. },
            })
        ),
        "nothing held, nothing sent: {report:?}"
    );
    assert_eq!(submissions(&world), 0);
    Ok(())
}

/// A dry run never waits: no answer can reach a run that appends to no journal, so the ask stops
/// it at once with the classifier's refusal, as before.
#[test]
fn a_dry_run_that_asks_never_waits() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let waited = Rc::new(Cell::new(false));
    let flag = Rc::clone(&waited);
    let report: Result<Report, ShellError> = cycle(&world, Autonomy::Ask, false)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            |_: &mut dyn AskedCycle| {
                flag.set(true);
                Ok(())
            },
            &watch_may_end,
        );
    assert_eq!(
        report.as_ref().err().map(ShellError::code),
        Some("autonomy_not_auto"),
        "{report:?}"
    );
    assert!(!waited.get(), "a dry run never waits");
    assert_eq!(world.ledger.borrow().count("ApprovalRequested"), 1);
    assert_eq!(submissions(&world), 0);
    Ok(())
}

/// The agent stream the wake reads is the run's own.
#[test]
fn the_wake_reads_the_runs_own_streams() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let now = clock.now();
    let w = world.clone();
    let lengths = Rc::new(RefCell::new((0, 0, 0, 0)));
    let seen = Rc::clone(&lengths);
    let _: Result<Report, ShellError> = cycle(&world, Autonomy::Ask, true)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                let read = asked.wake(now)?;
                let ledger = w.ledger.borrow();
                *seen.borrow_mut() = (
                    read.agent.len(),
                    ledger.len(&agent_stream()),
                    read.control.len(),
                    ledger.len(&control_stream()),
                );
                Err(ShellError::ProposalInvalid)
            },
            &end,
        );
    let (agent, agent_ledger, control, control_ledger) = *lengths.borrow();
    assert!(agent > 0 && control == 1, "{:?}", lengths.borrow());
    assert_eq!(
        (agent, control),
        (agent_ledger, control_ledger),
        "the wake reads both streams whole"
    );
    Ok(())
}
