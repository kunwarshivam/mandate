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

use mandate_accounting::AssetClass;
use mandate_alpaca::Pause;
use mandate_canon::{Digest, Value};
use mandate_executor::Purpose;
use mandate_num::Price;
use mandate_risk::Verdict;
use mandate_runtime::{
    Autonomy, IntentHandoff, Observation, OrderExecution, RiskClock,
    TimeInForce as RuntimeTimeInForce,
};
use mandate_time::UtcNanos;

use super::doubles::{
    AAPL, AGENT, ConvertingSink, FixedClassifier, FixedGate, FixedReconciler, PaperExecutor, World,
    agent_stream, control_stream, instrument, output, passed_checks, seed, setup,
};
use super::tail_install::{ApprovableMandate, established};
use crate::error::{Cause, ShellError};
use crate::lt10::watch_may_end;
use crate::stages::{Connector, Executor, Reconciler, Sink, Stage, Stages};
use crate::tracer::{AskedCycle, Door, Mark, ProductionCycle, Report, Wake, door};
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
        assert!(
            self.instant.get().secs() < PAUSE_HORIZON_S,
            "the watch paused past its horizon: a loop that never ends"
        );
        if let Ok(step) = i64::try_from(duration.as_secs())
            && let Ok(next) =
                UtcNanos::from_parts(self.instant.get().secs().saturating_add(step), 0)
        {
            self.instant.set(next);
        }
    }
}

/// An hour past the fixture's run clock (2026-09-25T20:00:00Z): no probe's watch pauses that
/// far, so a watch that never ends fails at once rather than running until the harness times out.
const PAUSE_HORIZON_S: i64 = 1_790_370_000;

/// The run clock as the fixture's setup holds it.
fn clock() -> Result<Clock, String> {
    Ok(Clock {
        instant: Rc::new(Cell::new(setup()?.now)),
    })
}

/// The preflight quote's risk mark the run hands the cycle: the fixture's 255.2 at the run
/// clock, which the cycle journals before its request, so the request records it as its
/// reference (DEC-943 item 1).
fn mark() -> Result<Mark, ShellError> {
    marked_at("255.2")
}

/// A risk mark at `price`, read at the run clock.
fn marked_at(price: &str) -> Result<Mark, ShellError> {
    let at = setup().map_err(|_| ShellError::ProposalInvalid)?.now;
    let price = Price::parse(price).map_err(|_| ShellError::ProposalInvalid)?;
    Ok(Mark { price, at })
}

/// The paper double executor, answering the watch's protection question covered: the facts the
/// LT-10 question reads are the caller's to judge here, never this double's.
struct Covered(PaperExecutor);

impl Executor for Covered {
    fn reset(&mut self) -> Result<(), Cause> {
        self.0.reset()
    }

    fn use_profile(&mut self, profile: mandate_domain::CapabilityProfile) -> Result<(), Cause> {
        self.0.use_profile(profile)
    }

    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        self.0.step(input)
    }

    fn protected(&mut self, _: &mandate_accounting::InstrumentId) -> Result<bool, Cause> {
        Ok(true)
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        self.0.committed(event)
    }
}

/// The doubles' stages with the classifier answering `autonomy`, the mandate naming the founder
/// as its approver, the observation's closes stored, the instrument marked, and the doubles' stock
/// sink, which refuses an intent with no execution policy as the production sink does: a granted
/// intent carries the plan its request bound (DEC-942).
fn cycle(world: &World, autonomy: Autonomy, place: bool) -> Result<ProductionCycle, String> {
    assembled(
        world,
        autonomy,
        place,
        Box::new(ConvertingSink(world.clone())),
    )
}

fn assembled(
    world: &World,
    autonomy: Autonomy,
    place: bool,
    sink: Box<dyn Sink>,
) -> Result<ProductionCycle, String> {
    let (stages, setup) = parts(world, autonomy, place, sink)?;
    Ok(ProductionCycle::new(stages, setup))
}

/// [`assembled`]'s stages and setup, for a probe that puts its own double in one stage's place.
fn parts(
    world: &World,
    autonomy: Autonomy,
    place: bool,
    sink: Box<dyn Sink>,
) -> Result<(Stages, crate::tracer::Setup), String> {
    let mut stages = world.stages();
    stages.sink = sink;
    stages.executor = Box::new(Covered(PaperExecutor {
        world: world.clone(),
        seen: Default::default(),
        inverted: false,
        last_client_order_id: None,
    }));
    stages.classifier = Box::new(FixedClassifier {
        world: world.clone(),
        autonomy,
    });
    stages.mandate = Box::new(ApprovableMandate {
        world: world.clone(),
    });
    established(world)?;
    let artifacts: Arc<dyn mandate_journal::ArtifactSource + Send + Sync> =
        Arc::new(BTreeMap::from([(Digest::of(CLOSES), CLOSES.to_vec())]));
    stages.artifacts = Some(artifacts);
    let mut setup = setup()?;
    setup.place_one_order = place;
    Ok((stages, setup))
}

fn observation() -> Result<Observation, String> {
    Ok(Observation {
        source: "alpaca_iex_daily_bars".to_owned(),
        instrument_id: Some(instrument().map_err(|cause| cause.to_string())?),
        as_of: RiskClock::from_secs(setup()?.now.secs()),
        data_ref: Digest::of(CLOSES),
    })
}

/// The watch the runner hands in: bound ten seconds before the fixture session's 16:00 end, so
/// the shell's own LT-10 question caps at the first wake past the run clock, and the caller's
/// question, which ends every run it is asked about, tells the two apart.
fn watch(clock: &Clock) -> Result<Watch<Clock>, String> {
    Ok(Watch {
        pause: clock.clone(),
        bound: UtcNanos::parse("2026-09-25T19:59:50.000000000Z").map_err(|e| e.to_string())?,
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
        (None, _) | (Some(_), None) => Err("the request names no id or hash".to_owned()),
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
        2,
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
            &mark().map_err(|error| error.to_string())?,
            |asked: &mut dyn AskedCycle| {
                let agent = asked.agent_rows()?;
                log.borrow_mut().push(types(&agent));
                let quiet = asked.wake(now, &mark()?)?;
                log.borrow_mut().push(types(&quiet.agent));
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                let Wake { agent, control } = asked.wake(now, &mark()?)?;
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
            &mark().map_err(|error| error.to_string())?,
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
        "the shell's own watch fails closed at the cap, where the caller's would have ended: \
         {report:?}"
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
            &mark().map_err(|error| error.to_string())?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                asked.wake(now, &mark()?)?;
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
            &mark().map_err(|error| error.to_string())?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                asked.wake(now, &mark()?).map(|_| ())
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
            &mark().map_err(|error| error.to_string())?,
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
            &mark().map_err(|error| error.to_string())?,
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
            &mark().map_err(|error| error.to_string())?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                let read = asked.wake(now, &mark()?)?;
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
    assert!(agent > 0 && control == 2, "{:?}", lengths.borrow());
    assert_eq!(
        (agent, control),
        (agent_ledger, control_ledger),
        "the wake reads both streams whole"
    );
    Ok(())
}

/// The production sink behind a recorder of the runtime's handoff, so a probe reads both what the
/// runtime handed and what the production conversion made of it.
struct RecordedExecutorSink {
    sink: crate::adapters::ExecutorSink,
    handed: Rc<RefCell<Vec<IntentHandoff>>>,
    converted: Rc<RefCell<Vec<mandate_executor::IntentHandoff>>>,
}

impl Sink for RecordedExecutorSink {
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<mandate_executor::IntentHandoff, Cause> {
        self.handed.borrow_mut().push(handoff.clone());
        let converted = self.sink.hand(handoff)?;
        self.converted.borrow_mut().push(converted.clone());
        Ok(converted)
    }
}

/// DEC-940 item 6, gap 2, closed by DEC-942: the request is journaled at `ApprovalRequested`
/// schema version 2 with the sizing's plan in its content, and the grant's intent reaches the production `ExecutorSink` with exactly the plan the
/// sizing produced for the asked proposal, so the held opening is sent once on the release.
#[test]
fn a_granted_intent_reaches_the_executor_sink_with_its_asked_plan() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let w = world.clone();
    let now = clock.now();
    let handed = Rc::new(RefCell::new(Vec::new()));
    let converted = Rc::new(RefCell::new(Vec::new()));
    let sink = RecordedExecutorSink {
        sink: crate::adapters::ExecutorSink {
            agent: mandate_runtime::AgentId(AGENT.to_owned()),
        },
        handed: Rc::clone(&handed),
        converted: Rc::clone(&converted),
    };
    let report: Result<Report, ShellError> =
        assembled(&world, Autonomy::Ask, true, Box::new(sink))?.run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            &mark().map_err(|error| error.to_string())?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                asked.wake(now, &mark()?)?;
                asked.submit()
            },
            &end,
        );
    report.map_err(|e| format!("the granted opening is sent: {e:?}"))?;
    let rows: Vec<Vec<u8>> = world
        .ledger
        .borrow()
        .bodies()
        .into_iter()
        .filter(|body| {
            mandate_canon::parse(body).is_ok_and(|v| {
                v.get("event_type").and_then(Value::as_str) == Some("ApprovalRequested")
            })
        })
        .collect();
    let [request] = &rows[..] else {
        return Err(format!("{} requests, not one", rows.len()));
    };
    let envelope = mandate_canon::parse(request).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        envelope.get("schema_version").and_then(Value::as_int),
        Some(2),
        "a request that binds a plan is version 2"
    );
    let stated = envelope
        .get("payload")
        .and_then(|payload| payload.get("content"))
        .and_then(|content| content.get("execution"))
        .map(mandate_canon::to_canonical);
    assert_eq!(
        stated.as_deref(),
        Some(
            &br#"{"asset_class":"us_equity","protection":null,"protection_required":false,"tif":"day"}"#[..]
        ),
        "the sizing's plan, as the content binds it"
    );
    let planned = Some(OrderExecution {
        asset_class: AssetClass::UsEquity,
        tif: RuntimeTimeInForce::Day,
        protection_required: false,
        protection: None,
    });
    let handed = handed.borrow();
    let [granted] = &handed[..] else {
        return Err(format!("{} handoffs, not one", handed.len()));
    };
    assert_eq!(granted.execution, planned, "the sizing's plan, unchanged");
    let converted = converted.borrow();
    let [sent] = &converted[..] else {
        return Err(format!("{} conversions, not one", converted.len()));
    };
    assert_eq!(sent.tif, Some(mandate_executor::TimeInForce::Day));
    assert_eq!(submissions(&world), 1, "the released opening is sent once");
    Ok(())
}

/// One asked cycle whose wake hands the runtime `wake_mark`, the founder's grant committed
/// before it: the re-validation's result and reason, the account stream's `MarkUpdated` bodies,
/// the request's and the re-validation's bodies, and the submissions sent.
struct Revalidated {
    result: Option<String>,
    reason: Option<String>,
    marks: Vec<Value>,
    requested: Option<Value>,
    revalidated: Option<Value>,
    sent: u32,
}

fn revalidated_at(wake_mark: &str) -> Result<Revalidated, String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0));
    let end = ending(&asks);
    let w = world.clone();
    let now = clock.now();
    let at_wake = marked_at(wake_mark).map_err(|error| error.to_string())?;
    let _: Result<Report, ShellError> = cycle(&world, Autonomy::Ask, true)?
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            &mark().map_err(|error| error.to_string())?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                asked.wake(now, &at_wake)?;
                asked.submit()
            },
            &end,
        );
    let ledger = world.ledger.borrow();
    let payload = |event_type: &str| {
        ledger
            .parsed(event_type)
            .first()
            .and_then(|row| row.get("payload"))
            .cloned()
    };
    let member = |value: &Option<Value>, name: &str| {
        value
            .as_ref()
            .and_then(|payload| payload.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let revalidated = payload("ApprovalRevalidated");
    Ok(Revalidated {
        result: member(&revalidated, "result"),
        reason: member(&revalidated, "reason"),
        marks: ledger
            .parsed("MarkUpdated")
            .iter()
            .filter_map(|row| row.get("payload").cloned())
            .collect(),
        requested: payload("ApprovalRequested"),
        revalidated,
        sent: submissions(&world),
    })
}

/// DEC-943 item 1 (gap 1 of DEC-940 item 6): the run journals its own marks — the preflight
/// quote's at the cycle's open, before the request, and each wake's before the answers reach
/// the runtime — as the registered `MarkUpdated` (`instrument_id`, `price`, `source`, `feed`,
/// `risk_clock`), so the request records the open's mark as its reference and the
/// re-validation compares the wake's mark with it, and a grant inside the band acts.
#[test]
fn the_open_and_each_wake_journal_the_runs_marks_before_the_runtime_reads_them()
-> Result<(), String> {
    let run = revalidated_at("255.3")?;
    let tick = "2026-09-25T20:00:00.000000000Z";
    let expected = |price: &str| {
        let text = format!(
            r#"{{"feed":"iex","instrument_id":"{AAPL}","price":"{price}","risk_clock":"{tick}","source":"quote"}}"#
        );
        mandate_canon::parse(text.as_bytes()).map_err(|error| error.to_string())
    };
    assert_eq!(
        run.marks,
        vec![expected("255.2")?, expected("255.3")?],
        "the open's mark, then the wake's, as the registered record"
    );
    let reference = run
        .requested
        .as_ref()
        .and_then(|payload| payload.get("reference_mark"))
        .and_then(|mark| mark.get("price"))
        .and_then(Value::as_str);
    assert_eq!(
        reference,
        Some("255.2"),
        "the request records the open's mark"
    );
    let compared = |name: &str| {
        run.revalidated
            .as_ref()
            .and_then(|payload| payload.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    assert_eq!(
        (compared("m_req"), compared("m_now")),
        (Some("255.2".to_owned()), Some("255.3".to_owned())),
        "the re-validation compares the wake's mark with the request's"
    );
    assert_eq!(run.result.as_deref(), Some("act"), "{:?}", run.reason);
    assert_eq!(run.sent, 1, "the granted opening is sent once");
    Ok(())
}

/// Mandate spec §6.4 check 12 (MI-22), with the run's own marks: a wake's mark at the band's
/// edge either side of the request's — 1% of 255.2 is 2.552 — acts, and one a thousandth of a
/// dollar beyond it skips the grant as `drift`, with nothing sent.
#[test]
fn a_wake_mark_beyond_the_band_skips_the_grant_as_drift() -> Result<(), String> {
    for (price, acts) in [
        ("257.752", true),
        ("252.648", true),
        ("257.753", false),
        ("252.647", false),
    ] {
        let run = revalidated_at(price)?;
        let (result, reason, sent) = match acts {
            true => ("act", None, 1),
            false => ("skip", Some("drift"), 0),
        };
        assert_eq!(run.result.as_deref(), Some(result), "{price}");
        assert_eq!(run.reason.as_deref(), reason, "{price}");
        assert_eq!(run.sent, sent, "{price}");
    }
    Ok(())
}

/// Rule 13 and FT-1 at the broker door (DEC-940 item 2), over every purpose and every state:
/// only while the wait holds, and only a risk-adding purpose, is a submission held; a second
/// one then refuses; an exit, a protective order or a flatten always passes, held or not.
#[test]
fn the_door_holds_only_a_risk_adding_submission_while_the_wait_holds() {
    let all = [
        Purpose::Open,
        Purpose::Increase,
        Purpose::RiskExit,
        Purpose::OwnerExit,
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::Flatten,
    ];
    for purpose in all {
        let adds = matches!(purpose, Purpose::Open | Purpose::Increase);
        for held in [false, true] {
            for submitted in [false, true] {
                assert_eq!(
                    door(false, held, submitted, purpose),
                    Door::Pass,
                    "{purpose:?}: nothing is held outside a wait"
                );
                let expected = match (adds, held || submitted) {
                    (false, false) | (false, true) => Door::Pass,
                    (true, false) => Door::Hold,
                    (true, true) => Door::Second,
                };
                assert_eq!(
                    door(true, held, submitted, purpose),
                    expected,
                    "{purpose:?} held={held} submitted={submitted}"
                );
            }
        }
    }
}

/// Only the classifier's `ask` keeps the session open: any other refusal of the cycle — here
/// the gate's denial — stops the run with its own refusal, and the wait is never called.
#[test]
fn a_cycle_refused_otherwise_never_waits() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let waited = Rc::new(Cell::new(false));
    let flag = Rc::clone(&waited);
    let mut stages = world.stages();
    stages.gate = Box::new(FixedGate {
        world: world.clone(),
        verdict: Verdict::Deny,
        checks: passed_checks(),
    });
    let artifacts: Arc<dyn mandate_journal::ArtifactSource + Send + Sync> =
        Arc::new(BTreeMap::from([(Digest::of(CLOSES), CLOSES.to_vec())]));
    stages.artifacts = Some(artifacts);
    let report: Result<Report, ShellError> = ProductionCycle::new(stages, setup()?)
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            &mark().map_err(|error| error.to_string())?,
            |_: &mut dyn AskedCycle| {
                flag.set(true);
                Ok(())
            },
            &watch_may_end,
        );
    assert_eq!(
        report.as_ref().err().map(ShellError::code),
        Some("gate_refused"),
        "{report:?}"
    );
    assert!(!waited.get(), "a refusal other than the ask never waits");
    assert_eq!(submissions(&world), 0);
    Ok(())
}

/// The four reads one reconciliation gathers (trading spec §11).
fn reconciliation_reads() -> [mandate_executor::Effect; 4] {
    [
        mandate_executor::BrokerRequest::ListOpenOrders,
        mandate_executor::BrokerRequest::ListPositions,
        mandate_executor::BrokerRequest::GetAccount,
        mandate_executor::BrokerRequest::ListActivities {
            since: mandate_executor::ActivityCursor(String::new()),
        },
    ]
    .map(mandate_executor::Effect::Broker)
}

/// The covered paper double, asking for a reconciliation on every command the watch hands it,
/// as the real executor does when a read finds the entry in a state it cannot account for.
struct ReconcilesOnCommand(Covered);

impl Executor for ReconcilesOnCommand {
    fn reset(&mut self) -> Result<(), Cause> {
        self.0.reset()
    }

    fn use_profile(&mut self, profile: mandate_domain::CapabilityProfile) -> Result<(), Cause> {
        self.0.use_profile(profile)
    }

    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        let commanded = matches!(input, mandate_executor::Input::Command(_));
        let mut effects = self.0.step(input)?;
        if commanded {
            effects.extend(reconciliation_reads());
        }
        Ok(effects)
    }

    fn protected(&mut self, instrument: &mandate_accounting::InstrumentId) -> Result<bool, Cause> {
        self.0.protected(instrument)
    }

    fn committed(&mut self, event: &mandate_executor::FoldedEvent) -> Result<(), Cause> {
        self.0.committed(event)
    }
}

/// A reconciler whose rounds before `mismatch_from` are clean and whose later rounds mismatch,
/// each mismatched round asking for another as one that ingested a late fill does.
struct MismatchesLater {
    inner: FixedReconciler,
    mismatch_from: usize,
    rounds: Rc<Cell<usize>>,
}

impl Reconciler for MismatchesLater {
    fn snapshot(
        &mut self,
        connector: &mut dyn Connector,
        requests: &[mandate_executor::BrokerRequest],
    ) -> Result<mandate_executor::BrokerSnapshot, Cause> {
        self.inner.snapshot(connector, requests)
    }

    fn reconcile(
        &mut self,
        snapshot: &mandate_executor::BrokerSnapshot,
    ) -> Result<mandate_executor::Reconciliation, Cause> {
        let round = self.rounds.get();
        self.rounds.set(round.saturating_add(1));
        self.inner.clean = round < self.mismatch_from;
        let mut reconciled = self.inner.reconcile(snapshot)?;
        if !self.inner.clean {
            reconciled.effects.extend(reconciliation_reads());
        }
        Ok(reconciled)
    }
}

/// #1369 review: a reconciliation the watch asks for that finds a mismatch stops the run, as the
/// startup's does (trading spec §11, `AGENTS.md` rule 3): the run answers
/// `reconciliation_mismatch`, the watch reads nothing more and the end question is never asked
/// again, nothing further is sent, and the round the mismatch asked for is never run.
#[test]
fn a_mismatch_in_the_watch_stops_the_run() -> Result<(), String> {
    let world = World::default();
    let clock = clock()?;
    let asks = Rc::new(Cell::new(0_u32));
    let counted = Rc::clone(&asks);
    let end = move |_: Option<&str>, _: bool, _: &[&'static str], _: UtcNanos, _: UtcNanos| {
        counted.set(counted.get().saturating_add(1));
        Ok(counted.get() > 4)
    };
    let rounds = Rc::new(Cell::new(0));
    let (mut stages, setup) = parts(
        &world,
        Autonomy::Ask,
        true,
        Box::new(ConvertingSink(world.clone())),
    )?;
    stages.executor = Box::new(ReconcilesOnCommand(Covered(PaperExecutor {
        world: world.clone(),
        seen: Default::default(),
        inverted: false,
        last_client_order_id: None,
    })));
    stages.reconciler = Box::new(MismatchesLater {
        inner: FixedReconciler {
            world: world.clone(),
            clean: true,
        },
        mismatch_from: 1,
        rounds: Rc::clone(&rounds),
    });
    let w = world.clone();
    let now = clock.now();
    let report: Result<Report, ShellError> = ProductionCycle::new(stages, setup)
        .run_observed_asked_watched(
            observation()?,
            output().map_err(|cause| cause.to_string())?,
            &watch(&clock)?,
            &mark().map_err(|error| error.to_string())?,
            |asked: &mut dyn AskedCycle| {
                answer(&w).map_err(|_| ShellError::ProposalInvalid)?;
                asked.wake(now, &mark()?)?;
                asked.submit()
            },
            &end,
        );
    assert_eq!(
        report.as_ref().err().map(ShellError::code),
        Some("reconciliation_mismatch"),
        "the watch's mismatch stops the run: {report:?}"
    );
    assert_eq!(
        rounds.get(),
        2,
        "the startup's round and the watch's mismatched one; the round it asked for never runs"
    );
    assert_eq!(asks.get(), 1, "the watch asks nothing after the mismatch");
    assert_eq!(submissions(&world), 1, "nothing further is sent");
    let results: Vec<String> = world
        .ledger
        .borrow()
        .parsed("ReconciliationRun")
        .iter()
        .filter_map(|run| {
            run.get("payload")
                .and_then(|payload| payload.get("result"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(
        results,
        ["clean", "mismatch"],
        "the mismatch is on the record"
    );
    Ok(())
}
