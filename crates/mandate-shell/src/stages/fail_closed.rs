//! The fail-closed suite (task brief, "The test that proves it", TI-1 to TI-12).
//!
//! One case per [`Stage`]: that stage stubbed to its own crate's refusal and **every other stage a
//! permissive double**, so the path reaches the stub and would reach the broker if the stub leaked.
//! The all-doubles case is the keystone: the same harness with nothing stubbed places exactly one
//! order, which is what makes every zero below mean something (review finding 1).
//!
//! These cases pass today and carry no pending marker: today every stage really does refuse.

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_canon::{Digest, Value};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{BrokerOutcome, BrokerRequest, ConnectorError, FoldedEvent};
use mandate_journal::{AppendOutcome, Environment, StoredEvent};
use mandate_risk::{Check, CheckOutcome, Verdict};
use mandate_runtime::{
    AgentId, Autonomy, Command, Effect, EventId, Initiator, Input, IntentBody, IntentHandoff,
    KillScope, Observation, Purpose, RiskClock,
};
use proptest::prelude::*;

use super::doubles::{
    FixedClassifier, FixedGate, FixedReconciler, FixtureMandate, LedgerJournal, OneShare,
    PaperExecutor, PlanlessExit, Script, ScriptedConnector, Stubbed, World, account_stream,
    agent_stream, equities_only, instrument, output, passed_checks, seed, setup, stub,
};
use super::{
    Connector, Executor, ExitPath, JournalWriter, MandateSource, Protection, Reconciler, Stage,
};
use crate::adapters::{Disconnected, Sources, over};
use crate::error::{Cause, ShellError};
use crate::symbols::BrokerSymbols;
use crate::tracer::{ProductionCycle, Report, Session, run_observed};

/// Runs one production cycle over `world`'s ledger with `stages`, handing the cycle the host's
/// model output, which is the only strategy input the shell accepts (DEC-475 item 1).
fn run_with(stages: super::Stages) -> Result<Result<Report, ShellError>, String> {
    let setup = setup()?;
    let output = output().map_err(|cause| format!("the fixture output: {cause}"))?;
    Ok(ProductionCycle::new(stages, setup).run(output))
}

/// [`run_with`] over a caller's `setup`, for the cases that change its flags.
fn run_with_setup(
    stages: super::Stages,
    setup: &crate::tracer::Setup,
) -> Result<Result<Report, ShellError>, String> {
    let output = output().map_err(|cause| format!("the fixture output: {cause}"))?;
    Ok(ProductionCycle::new(stages, setup.clone()).run(output))
}

fn refusal(outcome: Result<Report, ShellError>) -> Result<ShellError, String> {
    match outcome {
        Err(error) => Ok(error),
        Ok(report) => Err(format!("the run placed {report:?}")),
    }
}

/// The per-stage assertions, made at the furthest boundary the stubbed stage could have reached
/// (TI-2): the sink for stages up to it, the journal's `IntentProposed` for stages up to the
/// journal, the `OrderSubmitted` for stages up to the executor, and the broker for all of them.
/// `Stage::MarketData` has no stage double to stub: its refusal is the observation store's own
/// check, so its case hands the cycle an observation whose data is not in the run's store and
/// holds the same boundary over the agent stream's cycle records.
fn assert_fails_closed(stage: Stage) -> Result<(), String> {
    if stage == Stage::MarketData {
        return assert_market_data_refuses();
    }
    let world = World::default();
    let mut stages = world.stages();
    stub(&mut stages, stage, &world);
    let error = refusal(run_with(stages)?)?;
    let tally = world.tally.borrow();
    let ledger = world.ledger.borrow();
    assert_eq!(error.stage(), Some(stage), "{stage}: {error}");
    assert_eq!(error.code(), stage.code(), "{stage}: {error}");
    assert!(
        tally.calls.contains(&stage),
        "{stage}: the path never reached the stub, so its zero proves nothing: {:?}",
        tally.calls
    );
    assert_eq!(tally.submissions, 0, "{stage}");
    assert_eq!(tally.unrecorded_hands, 0, "{stage}");
    assert_eq!(tally.unrecorded_submissions, 0, "{stage}");
    if stage.position() <= Stage::Sink.position() {
        assert_eq!(tally.hands, 0, "{stage}");
    }
    if stage.position() <= Stage::Journal.position() {
        assert_eq!(ledger.count("IntentProposed"), 0, "{stage}");
    }
    if stage.position() <= Stage::Executor.position() {
        assert_eq!(ledger.count("OrderSubmitted"), 0, "{stage}");
    }
    if stage.position() < Stage::MarketData.position() {
        assert!(
            !tally.calls.contains(&Stage::Journal),
            "{stage} refuses before any stream exists"
        );
    }
    for downstream in [Stage::Sink, Stage::Executor, Stage::Connector] {
        if downstream.position() > stage.position() {
            match downstream {
                Stage::Executor => assert_eq!(
                    tally.executor_intents, 0,
                    "{stage}: the executor received an intent after the refusal"
                ),
                Stage::Sink | Stage::Connector => assert!(
                    !tally.calls.contains(&downstream),
                    "{stage}: {downstream} was reached after the refusal"
                ),
                Stage::FlattenProbe
                | Stage::ProtectionProbe
                | Stage::Validate
                | Stage::MarketData
                | Stage::Reconcile
                | Stage::Size
                | Stage::Classify
                | Stage::GateDryRun
                | Stage::Journal => unreachable!("the downstream list is closed"),
            }
        }
    }
    Ok(())
}

/// The market-data boundary over the production door: an observation whose data is not in the
/// run's store is refused as `market_data_untrusted`, with nothing about the cycle journaled —
/// no observation, no model output, no decision, no intent — while the startup's own account
/// records still committed (`AGENTS.md` rule 13, E15-13's slice H3).
fn assert_market_data_refuses() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    let setup = setup()?;
    let observation = Observation {
        source: "alpaca_iex_daily_bars".to_owned(),
        instrument_id: Some(instrument().map_err(|cause| format!("{cause}"))?),
        as_of: RiskClock::from_secs(setup.now.secs()),
        data_ref: Digest::of(b"closes the run never stored"),
    };
    let output = output().map_err(|cause| format!("{cause}"))?;
    let error = refusal(run_observed(&mut stages, &setup, observation, output))?;
    assert_eq!(error.code(), "market_data_untrusted", "{error}");
    assert_eq!(error.stage(), Some(Stage::MarketData), "{error}");
    let tally = world.tally.borrow();
    let ledger = world.ledger.borrow();
    assert_eq!(tally.submissions, 0);
    assert_eq!(tally.hands, 0);
    assert_eq!(tally.unrecorded_hands, 0);
    assert_eq!(tally.unrecorded_submissions, 0);
    for event in [
        "ObservationRecorded",
        "ModelOutputRecorded",
        "DecisionMade",
        "IntentProposed",
        "OrderSubmitted",
    ] {
        assert_eq!(ledger.count(event), 0, "{event}");
    }
    assert!(
        ledger.count("AccountStateObserved") > 0,
        "the startup's own account report committed before the refusal"
    );
    Ok(())
}

/// One named test per stage, and the list the suite covers, which
/// `the_suite_covers_every_stage` compares with [`Stage::ALL`].
macro_rules! fail_closed_cases {
    ($($name:ident => $stage:expr),* $(,)?) => {
        const CASES: &[Stage] = &[$($stage),*];
        $(
            #[test]
            fn $name() -> Result<(), String> {
                assert_fails_closed($stage)
            }
        )*
    };
}

fail_closed_cases! {
    stage_flatten_probe => Stage::FlattenProbe,
    stage_protection_probe => Stage::ProtectionProbe,
    stage_validate => Stage::Validate,
    stage_market_data => Stage::MarketData,
    stage_reconcile => Stage::Reconcile,
    stage_size => Stage::Size,
    stage_classify => Stage::Classify,
    stage_gate_dry_run => Stage::GateDryRun,
    stage_journal => Stage::Journal,
    stage_sink => Stage::Sink,
    stage_executor => Stage::Executor,
    stage_connector => Stage::Connector,
}

#[test]
fn every_stage_is_listed_once_in_path_order() {
    for (index, stage) in Stage::ALL.iter().enumerate() {
        assert_eq!(stage.position(), index, "{stage}");
    }
}

#[test]
fn the_suite_covers_every_stage() {
    assert_eq!(CASES, Stage::ALL.as_slice());
}

#[test]
fn only_the_two_probes_reduce_risk() {
    let reducing: Vec<Stage> = Stage::ALL
        .into_iter()
        .filter(|stage| stage.reduces_risk())
        .collect();
    assert_eq!(reducing, [Stage::FlattenProbe, Stage::ProtectionProbe]);
}

/// The keystone (task brief, "All doubles, no stub"): if this harness could not place an order
/// with everything permissive, every zero in the per-stage cases would be vacuous.
#[test]
fn all_doubles_place_exactly_one_order() -> Result<(), String> {
    let world = World::default();
    let report = run_with(world.stages())?.map_err(|e| e.to_string())?;
    let tally = world.tally.borrow();
    let ledger = world.ledger.borrow();
    assert_eq!(tally.hands, 1);
    assert_eq!(tally.submissions, 1);
    assert_eq!(ledger.count("IntentProposed"), 1);
    assert_eq!(ledger.count("OrderSubmitted"), 1);
    assert_eq!(
        tally.unrecorded_hands, 0,
        "TI-1: an intent handed before its draft"
    );
    assert_eq!(
        tally.unrecorded_submissions, 0,
        "TI-1: a submission before its draft"
    );
    let submitted: Vec<String> = tally
        .submitted
        .iter()
        .map(|order| order.client_order_id.as_str().to_owned())
        .collect();
    assert_eq!(report.submitted, submitted);
    assert!(report.would_place.is_none());
    shadow_book_matches(&ledger, &tally.submitted)?;
    every_draft_is_paper(&ledger)?;
    let account = account_stream();
    let startup_facts: Vec<String> = ledger
        .types(&account)
        .into_iter()
        .filter(|event_type| {
            matches!(
                event_type.as_str(),
                "AccountStateObserved" | "ReconciliationRun"
            )
        })
        .collect();
    assert_eq!(
        startup_facts,
        ["AccountStateObserved", "ReconciliationRun"],
        "the account report commits before reconciliation releases startup"
    );
    let held = ledger.len(&account);
    assert!(held > 0);
    assert_eq!(
        tally.executor_folds,
        vec![account; held],
        "the executor folds every account-stream event once, and nothing else"
    );
    Ok(())
}

/// The shadow order book (TI-1, TI-10): the order the drafts say was intended, rebuilt from the
/// committed bytes, is the order the broker was handed.
fn shadow_book_matches(
    ledger: &super::doubles::Ledger,
    submitted: &[mandate_executor::SubmitOrder],
) -> Result<(), String> {
    let intents = ledger.parsed("IntentProposed");
    assert_eq!(intents.len(), submitted.len());
    for (intent, order) in intents.iter().zip(submitted) {
        let field = |name: &str| {
            intent
                .get("payload")
                .and_then(|payload| payload.get(name))
                .and_then(Value::as_str)
                .map(str::to_owned)
        };
        assert_eq!(
            field("instrument_id").as_deref(),
            Some(order.instrument.as_str())
        );
        assert_eq!(field("qty"), Some(order.qty.to_string()));
        let limit = order.limit_price.map(|price| price.to_string());
        assert_eq!(field("limit_price"), limit);
        let intent_id = intent
            .get("event_id")
            .and_then(Value::as_str)
            .ok_or("an IntentProposed without an event id")?;
        assert_eq!(
            order.client_order_id.as_str(),
            format!("{}{intent_id}", mandate_executor::PREFIX)
        );
    }
    Ok(())
}

/// The environment scanner (TI-7): every committed draft says `paper`, read from the bytes. Not
/// `verify_events`, whose column check passes a consistently `live` journal (finding 5).
fn every_draft_is_paper(ledger: &super::doubles::Ledger) -> Result<(), String> {
    every_draft_says(ledger, Environment::Paper)
}

/// The environment scanner over any connection: every committed draft says `expected`, read from
/// the bytes (TI-7 as DEC-965 reads it).
fn every_draft_says(ledger: &super::doubles::Ledger, expected: Environment) -> Result<(), String> {
    let bodies = ledger.bodies();
    assert!(!bodies.is_empty());
    for body in bodies {
        let parsed = mandate_canon::parse(&body).map_err(|e| format!("{e:?}"))?;
        let environment = parsed.get("environment").and_then(Value::as_str);
        assert_eq!(environment.and_then(Environment::parse), Some(expected));
    }
    Ok(())
}

#[test]
fn all_stubs_at_once_refuse_at_the_first_probe_and_place_nothing() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    for stage in Stage::ALL {
        if stage != Stage::MarketData {
            stub(&mut stages, stage, &world);
        }
    }
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "exit_path_unavailable");
    let tally = world.tally.borrow();
    assert_eq!(tally.calls, [Stage::FlattenProbe]);
    assert_eq!(tally.hands, 0);
    assert_eq!(tally.submissions, 0);
    assert!(world.ledger.borrow().bodies().is_empty());
    Ok(())
}

/// The production stages with no mandate file and no injected contexts refuse before anything
/// could be sent:
/// today at the first probe, and once the adapters are real at the protection probe or validation.
#[test]
fn the_production_stages_refuse_without_their_inputs() -> Result<(), String> {
    let recorded_at = setup()?.now;
    let stages = over(Sources {
        mandate: PathBuf::from("no-such-mandate.json"),
        journal: None,
        recorded_at,
        agent: AgentId("tracer-aapl".to_owned()),
        workspace: "tracer".to_owned(),
        account_ref: "tracer-paper".to_owned(),
        executor: None,
        run: None,
        artifacts: None,
        content_store: None,
        symbols: BrokerSymbols::default(),
        transport: Box::new(Disconnected),
    });
    let error = refusal(run_with(stages)?)?;
    let stage = error.stage().ok_or(format!("{error}"))?;
    assert!(stage.position() <= Stage::Validate.position(), "{error}");
    assert_eq!(error.code(), stage.code());
    Ok(())
}

/// Step 17 (TI-4): the tracer refuses to start while the executor's protective sequence answers
/// E7-4's `Unimplemented`, and nothing past the two probes is asked anything.
#[test]
fn refuses_to_start_while_protection_is_unimplemented() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stub(&mut stages, Stage::ProtectionProbe, &world);
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "protection_unavailable");
    assert!(
        matches!(
            &error,
            ShellError::Refused {
                stage: Stage::ProtectionProbe,
                cause: Cause::Executor(mandate_executor::ExecutorError::Unimplemented {
                    story: "E7-4"
                }),
            }
        ),
        "{error:?}"
    );
    assert_eq!(
        world.tally.borrow().calls,
        [Stage::FlattenProbe, Stage::ProtectionProbe]
    );
    Ok(())
}

/// PB-8: a flatten planner that probes but cannot plan mid-run halts the runner before it performs
/// a single effect of the step: no kill-switch draft, no handoff, nothing empty sent.
#[test]
fn flatten_poison_halts() -> Result<(), String> {
    let world = World::default();
    let setup = setup()?;
    let mut stages = world.stages();
    stages.exit = Box::new(PlanlessExit(world.clone()));
    let admitted = FixtureMandate {
        world: world.clone(),
        environment: Environment::Paper,
    }
    .admitted()
    .map_err(|e| e.to_string())?;
    let mut session =
        Session::open_governed(&mut stages, &setup, &admitted).map_err(|e| e.to_string())?;
    session.start().map_err(|e| e.to_string())?;
    let before = world.ledger.borrow().bodies().len();
    let error = match session.feed(Input::Command(Command::KillSwitch {
        scope: KillScope::Agent(AgentId("tracer-aapl".to_owned())),
        initiator: Initiator::Owner,
        confirmation: None,
    })) {
        Err(error) => error,
        Ok(()) => return Err("a kill switch with no plan did not halt".to_owned()),
    };
    assert_eq!(error.code(), "exit_path_unavailable");
    assert!(matches!(
        &error,
        ShellError::Refused {
            stage: Stage::FlattenProbe,
            cause: Cause::Poisoned(_)
        }
    ));
    assert_eq!(world.ledger.borrow().bodies().len(), before);
    assert_eq!(world.ledger.borrow().count("KillSwitchActivated"), 0);
    assert_eq!(world.tally.borrow().hands, 0);
    Ok(())
}

/// TI-1's shell-side guard on its own: an intent handoff whose `IntentProposed` was never appended
/// is refused before the sink sees it. `mandate_runtime::handle` always orders the draft first, so
/// only an effect list built by hand reaches this guard (review round 1, minor 2).
#[test]
fn an_intent_whose_draft_was_never_appended_never_reaches_the_sink() -> Result<(), String> {
    let world = World::default();
    let setup = setup()?;
    let mut stages = world.stages();
    let admitted = FixtureMandate {
        world: world.clone(),
        environment: Environment::Paper,
    }
    .admitted()
    .map_err(|e| e.to_string())?;
    let instrument = admitted
        .view
        .working_universe
        .first()
        .cloned()
        .ok_or("no instrument")?;
    let mut session =
        Session::open_governed(&mut stages, &setup, &admitted).map_err(|e| e.to_string())?;
    session.start().map_err(|e| e.to_string())?;
    let unrecorded = Effect::Intent(IntentHandoff {
        intent_id: EventId("00000100000000000099000000".to_owned()),
        body: IntentBody::Order {
            instrument,
            side: mandate_accounting::Side::Buy,
            qty: mandate_num::Qty::parse("1").map_err(|e| e.to_string())?,
            limit: mandate_num::Price::parse("255.2").map_err(|e| e.to_string())?,
            purpose: Purpose::Open,
        },
        execution: None,
    });
    let error = match session.perform(vec![unrecorded]) {
        Err(error) => error,
        Ok(()) => return Err("an unrecorded intent was handed".to_owned()),
    };
    assert!(
        matches!(
            error,
            ShellError::WriteAheadViolated {
                effect: "intent handoff"
            }
        ),
        "{error:?}"
    );
    assert_eq!(world.tally.borrow().hands, 0);
    assert!(!world.tally.borrow().calls.contains(&Stage::Sink));
    Ok(())
}

/// Step 12's ordering bug, planted in the executor double: the submission before its
/// `OrderSubmitted`. The runner refuses it before the broker sees it (PB-1).
#[test]
fn a_submission_before_its_draft_never_reaches_the_broker() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.executor = Box::new(PaperExecutor {
        world: world.clone(),
        seen: Default::default(),
        inverted: true,
        last_client_order_id: None,
    });
    let error = refusal(run_with(stages)?)?;
    assert!(
        matches!(
            error,
            ShellError::WriteAheadViolated {
                effect: "submission"
            }
        ),
        "{error:?}"
    );
    assert_eq!(world.tally.borrow().submissions, 0);
    assert_eq!(world.ledger.borrow().count("OrderSubmitted"), 0);
    Ok(())
}

/// Rule 5, review round 1 minor 6: an append answered `Ambiguous` may or may not have committed,
/// so it is never read as committed. Here the ledger really holds the `OrderSubmitted`, and the
/// journal answers `Ambiguous` for it: the run stops at the journal and nothing is submitted.
#[test]
fn an_ambiguous_append_is_never_read_as_committed() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.journal = Box::new(AmbiguousOn {
        inner: LedgerJournal(world.clone()),
        event_type: "OrderSubmitted",
    });
    let error = refusal(run_with(stages)?)?;
    assert!(
        matches!(
            &error,
            ShellError::Refused {
                stage: Stage::Journal,
                cause: Cause::Append {
                    outcome: "Ambiguous"
                },
            }
        ),
        "{error:?}"
    );
    assert_eq!(world.ledger.borrow().count("OrderSubmitted"), 1);
    assert_eq!(world.tally.borrow().submissions, 0);
    Ok(())
}

#[test]
fn the_ledger_double_refuses_a_draft_without_its_schema_version() -> Result<(), String> {
    let world = World::default();
    let mut journal = LedgerJournal(world);
    let stream = account_stream();
    let epoch = journal
        .take_ownership(&stream)
        .map_err(|error| error.to_string())?;
    let draft = br#"{"event_id":"01K6VY6M800000000000000000","event_type":"Unversioned"}"#.to_vec();
    assert_eq!(
        journal.append(&stream, 0, epoch, &[draft]),
        AppendOutcome::Unavailable
    );
    Ok(())
}

#[test]
fn an_ambiguous_account_report_never_runs_reconciliation_or_opens() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.journal = Box::new(AmbiguousOn {
        inner: LedgerJournal(world.clone()),
        event_type: "AccountStateObserved",
    });
    let error = refusal(run_with(stages)?)?;
    assert!(matches!(
        &error,
        ShellError::Refused {
            stage: Stage::Journal,
            cause: Cause::Append {
                outcome: "Ambiguous"
            },
        }
    ));
    let ledger = world.ledger.borrow();
    assert_eq!(ledger.count("AccountStateObserved"), 1);
    assert_eq!(ledger.count("ReconciliationRun"), 0);
    let tally = world.tally.borrow();
    assert_eq!(tally.hands, 0);
    assert_eq!(tally.submissions, 0);
    Ok(())
}

#[test]
fn an_ambiguous_reconciliation_keeps_the_committed_account_and_never_opens() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.journal = Box::new(AmbiguousOn {
        inner: LedgerJournal(world.clone()),
        event_type: "ReconciliationRun",
    });
    let error = refusal(run_with(stages)?)?;
    assert!(matches!(
        &error,
        ShellError::Refused {
            stage: Stage::Journal,
            cause: Cause::Append {
                outcome: "Ambiguous"
            },
        }
    ));
    let ledger = world.ledger.borrow();
    assert_eq!(
        ledger
            .types(&account_stream())
            .into_iter()
            .filter(|event_type| {
                matches!(
                    event_type.as_str(),
                    "AccountStateObserved" | "ReconciliationRun"
                )
            })
            .collect::<Vec<_>>(),
        ["AccountStateObserved", "ReconciliationRun"]
    );
    let tally = world.tally.borrow();
    assert_eq!(tally.hands, 0);
    assert_eq!(tally.submissions, 0);
    Ok(())
}

/// The ledger journal, answering `Ambiguous` for any append carrying `event_type` after the
/// ledger has committed it: the one answer a writer cannot read either way.
struct AmbiguousOn {
    inner: LedgerJournal,
    event_type: &'static str,
}

impl JournalWriter for AmbiguousOn {
    fn take_ownership(&mut self, stream: &str) -> Result<u64, Cause> {
        self.inner.take_ownership(stream)
    }

    fn read(&self, stream: &str) -> Result<Vec<StoredEvent>, Cause> {
        self.inner.read(stream)
    }

    fn append(
        &mut self,
        stream: &str,
        expected_head: u64,
        writer_epoch: u64,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        let outcome = self
            .inner
            .append(stream, expected_head, writer_epoch, drafts);
        let marker = format!("\"{}\"", self.event_type);
        let carries = drafts
            .iter()
            .any(|draft| String::from_utf8_lossy(draft).contains(&marker));
        match outcome {
            AppendOutcome::Committed(_) if carries => AppendOutcome::Ambiguous,
            other => other,
        }
    }
}

/// PB-11: an unknown outcome queries once, and one absence never resubmits.
#[test]
fn broker_unknown_then_absent_queries_and_never_resubmits() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.connector = Box::new(ScriptedConnector {
        world: world.clone(),
        script: Script::UnknownThenAbsent,
    });
    let report = run_with(stages)?.map_err(|e| e.to_string())?;
    let tally = world.tally.borrow();
    assert_eq!(tally.submissions, 1);
    assert_eq!(tally.queries, 1);
    assert_eq!(report.submitted.len(), 1);
    Ok(())
}

/// DEC-85: an answer the connector could not read is not an unknown outcome, so it is not handed
/// to the executor to interpret; the run stops naming it.
#[test]
fn an_uninterpretable_broker_answer_stops_and_is_not_handed_on() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.connector = Box::new(ScriptedConnector {
        world: world.clone(),
        script: Script::Unreadable,
    });
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "broker_answer_uninterpretable");
    assert!(error.to_string().contains("(wire)"), "{error}");
    let tally = world.tally.borrow();
    assert_eq!(tally.submissions, 1);
    assert_eq!(tally.executor_broker_inputs, 0);
    Ok(())
}

/// #241 review round 2, minor 3: a submission the connector refused before it left the process
/// stops the run, is asked once, and is never handed to the executor or asked again.
#[test]
fn a_request_that_was_not_sent_stops_the_run_and_is_never_asked_again() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.connector = Box::new(ScriptedConnector {
        world: world.clone(),
        script: Script::NotSent,
    });
    let error = refusal(run_with(stages)?)?;
    assert_eq!(
        error.code(),
        "broker_request_not_sent",
        "a request that never left is not an answer the broker gave (#248 review, minor 2)"
    );
    assert!(error.to_string().contains("(refused_path)"), "{error}");
    let tally = world.tally.borrow();
    assert_eq!(tally.submissions, 1);
    assert_eq!(tally.executor_broker_inputs, 0);
    Ok(())
}

/// PB-12, TI-12: a reconciliation mismatch pauses and alerts, and nothing in the tracer lifts it:
/// the agent stream's last mode is the startup hold's `paused`.
#[test]
fn reconcile_mismatch_pauses() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.reconciler = Box::new(FixedReconciler {
        world: world.clone(),
        clean: false,
    });
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "reconciliation_mismatch");
    let ledger = world.ledger.borrow();
    assert_eq!(
        ledger.last_type(&agent_stream()).as_deref(),
        Some("AgentModeChanged")
    );
    let modes: Vec<String> = ledger
        .parsed("AgentModeChanged")
        .iter()
        .filter_map(|body| {
            body.get("payload")
                .and_then(|p| p.get("to"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(modes, ["paused"]);
    assert_eq!(world.tally.borrow().hands, 0);
    Ok(())
}

/// A reconciler whose first `asks` reconciliations each ask for another, as one that ingested a
/// late fill does, and whose round `n` is clean unless `mismatched` names it.
struct AsksAgain {
    inner: FixedReconciler,
    asks: usize,
    mismatched: Option<usize>,
    rounds: Rc<RefCell<usize>>,
}

impl Reconciler for AsksAgain {
    fn snapshot(
        &mut self,
        connector: &mut dyn Connector,
        requests: &[BrokerRequest],
    ) -> Result<mandate_executor::BrokerSnapshot, Cause> {
        self.inner.snapshot(connector, requests)
    }

    fn reconcile(
        &mut self,
        snapshot: &mandate_executor::BrokerSnapshot,
    ) -> Result<mandate_executor::Reconciliation, Cause> {
        let round = *self.rounds.borrow();
        *self.rounds.borrow_mut() = round.saturating_add(1);
        self.inner.clean = self.mismatched != Some(round);
        let mut reconciled = self.inner.reconcile(snapshot)?;
        if round < self.asks {
            let since = mandate_executor::ActivityCursor(String::new());
            reconciled.effects.extend(
                [
                    BrokerRequest::ListOpenOrders,
                    BrokerRequest::ListPositions,
                    BrokerRequest::GetAccount,
                    BrokerRequest::ListActivities { since },
                ]
                .map(mandate_executor::Effect::Broker),
            );
        }
        Ok(reconciled)
    }
}

/// The run over `world` with [`AsksAgain`] in the reconciler's place, and the rounds it ran.
fn asking(
    world: &World,
    asks: usize,
    mismatched: Option<usize>,
) -> Result<(Result<Report, ShellError>, usize), String> {
    let rounds = Rc::new(RefCell::new(0));
    let mut stages = world.stages();
    stages.reconciler = Box::new(AsksAgain {
        inner: FixedReconciler {
            world: world.clone(),
            clean: true,
        },
        asks,
        mismatched,
        rounds: rounds.clone(),
    });
    let outcome = run_with(stages)?;
    let ran = *rounds.borrow();
    Ok((outcome, ran))
}

/// #1355, trading spec §5.7, DEC-938 item 5: a reconciliation that ingested a late fill asks for
/// another, which the run gathers and reconciles as it did the first, never stepping its reads
/// one by one; the startup settles once the reconciliation asks for no more, up to the first and
/// two more, and stops at Reconcile with nothing sent when one still asks after that.
#[test]
fn a_reconciliation_asked_for_by_a_reconciliation_is_run_too() -> Result<(), String> {
    for asks in 0..=2 {
        let world = World::default();
        let (outcome, ran) = asking(&world, asks, None)?;
        let report = outcome.map_err(|e| format!("{asks}: {e}"))?;
        assert_eq!(ran, asks + 1, "each asked-for reconciliation runs once");
        assert_eq!(report.submitted.len(), 1, "{asks}: the run goes on");
    }
    let world = World::default();
    let (outcome, ran) = asking(&world, 3, None)?;
    let error = refusal(outcome)?;
    assert_eq!(ran, 3, "no reconciliation past the bound");
    assert_eq!(error.stage(), Some(Stage::Reconcile), "{error}");
    assert!(
        error.to_string().contains("a reconciliation that settles"),
        "{error}"
    );
    assert_eq!(world.tally.borrow().submissions, 0);
    Ok(())
}

/// A mismatch in a reconciliation the first asked for pauses and alerts as the first's would.
#[test]
fn a_mismatch_in_an_asked_for_reconciliation_stops_the_startup() -> Result<(), String> {
    for round in 0..=1 {
        let world = World::default();
        let (outcome, _) = asking(&world, 1, Some(round))?;
        let error = refusal(outcome)?;
        assert_eq!(error.code(), "reconciliation_mismatch", "round {round}");
        assert_eq!(world.tally.borrow().submissions, 0);
    }
    Ok(())
}

/// #1369 review: once a round mismatches, the startup stops there; a round that mismatched and
/// asked for another, as one that ingested a late fill does, never has that next round run.
#[test]
fn a_mismatch_stops_the_reconciliations_it_asked_for() -> Result<(), String> {
    for (round, ran) in [(0, 1), (1, 2)] {
        let world = World::default();
        let (outcome, rounds) = asking(&world, 2, Some(round))?;
        let error = refusal(outcome)?;
        assert_eq!(error.code(), "reconciliation_mismatch", "round {round}");
        assert_eq!(rounds, ran, "no round runs after round {round}'s mismatch");
        assert_eq!(world.tally.borrow().submissions, 0);
    }
    Ok(())
}

/// TI-6, TI-12, PB-3, PB-16: a second run over the same journal replays it, restarts, reconciles,
/// and refuses a new cycle; across both runs there is exactly one submission and one intent.
#[test]
fn a_restart_sends_nothing_and_a_repeat_run_is_refused() -> Result<(), String> {
    let world = World::default();
    run_with(world.stages())?.map_err(|e| e.to_string())?;
    let error = refusal(run_with(world.stages())?)?;
    assert_eq!(error.code(), "cycle_already_open");
    let tally = world.tally.borrow();
    let ledger = world.ledger.borrow();
    assert_eq!(tally.submissions, 1);
    assert_eq!(tally.hands, 1);
    assert_eq!(ledger.count("IntentProposed"), 1);
    assert_eq!(ledger.count("AccountStateObserved"), 2);
    assert_eq!(ledger.count("ReconciliationRun"), 2);
    assert_eq!(
        ledger
            .types(&account_stream())
            .into_iter()
            .filter(|event_type| {
                matches!(
                    event_type.as_str(),
                    "AccountStateObserved" | "ReconciliationRun"
                )
            })
            .collect::<Vec<_>>(),
        [
            "AccountStateObserved",
            "ReconciliationRun",
            "AccountStateObserved",
            "ReconciliationRun"
        ],
        "each restart records its account before its reconciliation run"
    );
    Ok(())
}

/// TI-12 counts intents, not events: a stream whose last run was denied at the gate carries no
/// intent, so the next run is a new decision rather than a refusal.
#[test]
fn a_denied_run_leaves_no_cycle_open() -> Result<(), String> {
    let world = World::default();
    let mut denying = world.stages();
    denying.gate = Box::new(FixedGate {
        world: world.clone(),
        verdict: Verdict::Deny,
        checks: passed_checks(),
    });
    let error = refusal(run_with(denying)?)?;
    assert_eq!(error.code(), "gate_refused");
    assert!(!world.ledger.borrow().bodies().is_empty());
    let report = run_with(world.stages())?.map_err(|e| e.to_string())?;
    assert_eq!(report.submitted.len(), 1);
    Ok(())
}

/// Only `--new-cycle` starts another cycle, and it is a new intent with a new id, never the old
/// one resubmitted. The first cycle's order fills before the second starts: while it works,
/// trading spec §5.3 rule 6 holds another opening in its instrument (DEC-1150 item 5).
#[test]
fn a_new_cycle_is_a_new_intent_with_a_new_id() -> Result<(), String> {
    let world = World::default();
    run_with(world.stages())?.map_err(|e| e.to_string())?;
    let first = world
        .tally
        .borrow()
        .submitted
        .first()
        .map(|order| order.client_order_id.as_str().to_owned())
        .ok_or("the first cycle submitted")?;
    let account = account_stream();
    let held = u64::try_from(world.ledger.borrow().len(&account)).map_err(|e| e.to_string())?;
    let seq = held.saturating_add(1);
    seed(
        &world,
        &account,
        seq,
        "01JFIRSTFILLEDXXXXXXXXXXXX",
        "OrderStateChanged",
        "system",
        &format!(r#"{{"client_order_id":"{first}","state":"filled"}}"#),
    )?;
    let mut setup = setup()?;
    setup.new_cycle = true;
    run_with_setup(world.stages(), &setup)
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    let tally = world.tally.borrow();
    assert_eq!(tally.submissions, 2);
    let ids: Vec<&str> = tally
        .submitted
        .iter()
        .map(|order| order.client_order_id.as_str())
        .collect();
    assert_eq!(ids.len(), 2);
    assert_ne!(ids.first(), ids.get(1));
    Ok(())
}

/// TI-9, PB-6: ASK sends nothing. The decision and its approval request are journaled, then the
/// tracer stops because this harness supplies no approval response.
#[test]
fn ask_journals_the_decision_and_sends_nothing() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.classifier = Box::new(FixedClassifier {
        world: world.clone(),
        autonomy: Autonomy::Ask,
    });
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "autonomy_not_auto");
    assert!(error.to_string().contains("classified ask"), "{error}");
    let ledger = world.ledger.borrow();
    assert_eq!(ledger.count("DecisionMade"), 1);
    assert_eq!(ledger.count("ApprovalRequested"), 1);
    assert_eq!(ledger.count("IntentProposed"), 0);
    assert_eq!(world.tally.borrow().submissions, 0);
    Ok(())
}

/// #1355's second defect, journal spec §9.1 rule 5 and DEC-938: a classifier that cannot answer
/// leaves no classification and so no decider, so the step journals no `DecisionMade` (whose
/// `deny` would need a non-null `decided_by` the spec has no label for) and the run stops at
/// Classify, never as the journal's refusal of an unlabelled deny.
#[test]
fn a_classifier_refusal_journals_no_unlabelled_deny() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stub(&mut stages, Stage::Classify, &world);
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.stage(), Some(Stage::Classify), "{error}");
    let ledger = world.ledger.borrow();
    assert_eq!(
        ledger.count("DecisionMade"),
        0,
        "no decision without its decider"
    );
    assert_eq!(ledger.count("IntentProposed"), 0);
    assert_eq!(world.tally.borrow().submissions, 0);
    Ok(())
}

/// PB-2: the gate's denial is journaled as a decision, alerted, and sends nothing.
#[test]
fn a_gate_denial_is_journaled_and_sends_nothing() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.gate = Box::new(FixedGate {
        world: world.clone(),
        verdict: Verdict::Deny,
        checks: passed_checks(),
    });
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "gate_refused");
    assert!(error.to_string().contains("max_order_size"), "{error}");
    let ledger = world.ledger.borrow();
    assert_eq!(ledger.count("DecisionMade"), 1);
    assert_eq!(ledger.count("IntentProposed"), 0);
    assert_eq!(
        ledger.last_type(&agent_stream()).as_deref(),
        Some("DecisionMade")
    );
    Ok(())
}

/// TI-11, PB-13: an opening `Allow` carrying a check the gate never reached is refused, from the
/// gate's own output.
#[test]
fn gate_allow_with_not_reached_refused() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    let mut checks = passed_checks();
    checks.pop();
    checks.push(CheckOutcome::NotReached(Check::DayTradeBudget));
    stages.gate = Box::new(FixedGate {
        world: world.clone(),
        verdict: Verdict::Allow,
        checks,
    });
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "gate_refused");
    assert!(error.to_string().contains("check_not_reached"), "{error}");
    assert_eq!(world.tally.borrow().hands, 0);
    Ok(())
}

/// PB-14: a proposal of quantity zero is refused by the shell; a cap is the gate's, not the
/// shell's, so nothing else about the proposal is judged here.
///
/// Where PB-7 went: a model that answers `Flat` or `Undecided` never reaches the shell, which
/// decides only on the model host's `Long`, and no cycle can be opened on a not-`Long` answer
/// because no such answer exists below the host (DEC-503 item 1, DEC-475 item 1); the shell's
/// half of the rule is that nothing here could compute one. `mandate-modelhost` pins the host's
/// refusals and `mandate-paper`'s `a_flat_model_stores_and_sends_nothing` ends its run there.
#[test]
fn proposal_sanity() -> Result<(), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.sizing = Box::new(OneShare {
        world: world.clone(),
        qty: "0",
    });
    let error = refusal(run_with(stages)?)?;
    assert_eq!(error.code(), "proposal_invalid");
    assert_eq!(world.ledger.borrow().count("DecisionMade"), 0);
    assert_eq!(world.tally.borrow().hands, 0);
    Ok(())
}

/// One cycle of a mandate in `mandate` over a run whose chosen connection is in `connection`.
fn run_in(
    mandate: Environment,
    connection: mandate_domain::Environment,
) -> Result<(World, Result<Report, ShellError>), String> {
    let world = World::default();
    let mut stages = world.stages();
    stages.mandate = Box::new(FixtureMandate {
        world: world.clone(),
        environment: mandate,
    });
    let mut setup = setup()?;
    setup.connection_environment = connection;
    let outcome = run_with_setup(stages, &setup)?;
    Ok((world, outcome))
}

/// The journal's text for a connection environment, spelled here rather than through the shell's
/// own mapping, so the oracle does not share the code it checks.
fn named(connection: mandate_domain::Environment) -> Environment {
    match connection {
        mandate_domain::Environment::Paper => Environment::Paper,
        mandate_domain::Environment::Live => Environment::Live,
    }
}

/// TI-7 as DEC-965 reads it: a mandate whose environment is not the chosen connection's refuses
/// before any stream exists, and one that is runs with every draft in that environment.
#[test]
fn a_mandate_outside_the_chosen_connections_environment_is_refused() -> Result<(), String> {
    for (mandate, connection) in [
        (Environment::Live, mandate_domain::Environment::Paper),
        (Environment::Backtest, mandate_domain::Environment::Paper),
        (Environment::Paper, mandate_domain::Environment::Live),
        (Environment::Backtest, mandate_domain::Environment::Live),
    ] {
        let (world, outcome) = run_in(mandate, connection)?;
        let error = refusal(outcome)?;
        assert_eq!(error.code(), "environment_mismatch");
        assert!(world.ledger.borrow().bodies().is_empty());
        assert_eq!(world.tally.borrow().hands, 0);
    }
    Ok(())
}

/// R13 behind V-001: a live mandate on the live connection the door chose is admitted, and every
/// draft the run journals says `live`, never the constant `paper`.
#[test]
fn a_live_mandate_on_the_chosen_live_connection_journals_live() -> Result<(), String> {
    let (world, outcome) = run_in(Environment::Live, mandate_domain::Environment::Live)?;
    let report = outcome.map_err(|error| error.to_string())?;
    assert_eq!(report.submitted.len(), 1);
    every_draft_says(&world.ledger.borrow(), Environment::Live)
}

fn mandate_environment() -> impl Strategy<Value = Environment> {
    prop_oneof![
        Just(Environment::Paper),
        Just(Environment::Live),
        Just(Environment::Backtest)
    ]
}

fn connection_environment() -> impl Strategy<Value = mandate_domain::Environment> {
    prop_oneof![
        Just(mandate_domain::Environment::Paper),
        Just(mandate_domain::Environment::Live)
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Over every mandate and connection environment pair: the cycle refuses
    /// `environment_mismatch` with nothing journaled and nothing handed exactly when the two
    /// differ, and otherwise journals every draft in the connection's environment.
    #[test]
    fn the_cycle_admits_exactly_the_chosen_connections_environment(
        mandate in mandate_environment(),
        connection in connection_environment(),
    ) {
        let (world, outcome) = run_in(mandate, connection).map_err(TestCaseError::fail)?;
        if mandate == named(connection) {
            prop_assert!(outcome.is_ok(), "{:?} on {:?}: {:?}", mandate, connection, outcome);
            every_draft_says(&world.ledger.borrow(), named(connection))
                .map_err(TestCaseError::fail)?;
        } else {
            let error = refusal(outcome).map_err(TestCaseError::fail)?;
            prop_assert_eq!(error.code(), "environment_mismatch");
            prop_assert!(world.ledger.borrow().bodies().is_empty());
            prop_assert_eq!(world.tally.borrow().hands, 0);
        }
    }
}

/// Without `place_one_order` the run stops before the submission and reports what it would
/// have placed (`AGENTS.md` rule 3).
#[test]
fn a_planning_run_stops_before_the_submission() -> Result<(), String> {
    let world = World::default();
    let mut setup = setup()?;
    setup.place_one_order = false;
    let report = run_with_setup(world.stages(), &setup)
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?;
    assert!(report.submitted.is_empty());
    let order = report.would_place.ok_or("no order reported")?;
    assert_eq!(order.qty.to_string(), "1");
    assert_eq!(world.tally.borrow().submissions, 0);
    assert!(!world.tally.borrow().calls.contains(&Stage::Connector));
    Ok(())
}

/// TI-10: the same stages, fixtures and injected clock produce byte-identical drafts.
#[test]
fn the_same_inputs_journal_byte_identical_drafts() -> Result<(), String> {
    let first = World::default();
    let second = World::default();
    run_with(first.stages())?.map_err(|e| e.to_string())?;
    run_with(second.stages())?.map_err(|e| e.to_string())?;
    let first = first.ledger.borrow().bodies();
    assert!(!first.is_empty());
    assert_eq!(first, second.ledger.borrow().bodies());
    Ok(())
}

/// The refusal a production stub reports names its story in a form `cargo xtask ci pending`
/// reads as a stub (DEC-110, DEC-137).
#[test]
fn a_stub_refusal_reads_as_a_stub() {
    let error = ShellError::Refused {
        stage: Stage::Validate,
        cause: Cause::Unimplemented { story: "E7-7" },
    };
    assert!(error.to_string().contains("not implemented yet"));
    assert!(format!("{error:?}").contains("Unimplemented"));
}

#[test]
fn a_stubbed_stage_answers_its_own_crates_refusal() {
    let world = World::default();
    let stubbed = Stubbed(world.clone(), Stage::Validate);
    let answer = stubbed.admitted();
    assert!(
        matches!(
            answer,
            Err(Cause::Spec(mandate_spec::SpecError::Unimplemented))
        ),
        "{answer:?}"
    );
    assert_eq!(world.tally.borrow().calls, [Stage::Validate]);
}

/// Rule 13 on the production side: both exit probes answer before anything arms. A probe that
/// cannot answer over the shell's fixture fails here, so the tracer never opens a position whose
/// flatten or protective path is unavailable.
#[test]
fn the_production_exit_probes_over_the_shells_fixture() {
    let flatten = crate::adapters::RiskExitPath::new(
        mandate_runtime::AgentId("agent-a".to_owned()),
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tracer/mandate.json"
        )),
    )
    .probe();
    assert!(
        flatten.is_ok(),
        "the flatten probe answers over the shell's mandate fixture: {flatten:?}"
    );
    let protection = crate::adapters::ExecutorProtection::new(
        std::path::PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/tracer/mandate.json"
        )),
        Some(mandate_executor::ExecutorConfig::PROPOSED),
    )
    .probe();
    assert!(
        protection.is_ok(),
        "the protection probe answers: {protection:?}"
    );
}

/// Every `.rs` file under `src/`, with its text.
fn sources() -> Result<Vec<(PathBuf, String)>, String> {
    fn walk(dir: &Path, out: &mut Vec<(PathBuf, String)>) -> Result<(), String> {
        let entries = fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                walk(&path, out)?;
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let text = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                out.push((path, text));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out)?;
    Ok(out)
}

/// The text a shipping build compiles: files declared as `#[cfg(test)] mod` are dropped, and each
/// remaining file is cut at its first `#[cfg(test)]` line, which is where this crate keeps its
/// test modules.
fn shipping_sources() -> Result<Vec<(PathBuf, String)>, String> {
    let all = sources()?;
    let mut test_files = Vec::new();
    for (path, text) in &all {
        let base = match path.file_stem().and_then(|stem| stem.to_str()) {
            Some("lib" | "main") => path.parent().map(Path::to_path_buf),
            Some(stem) => path.parent().map(|dir| dir.join(stem)),
            None => None,
        };
        let lines: Vec<&str> = text.lines().map(str::trim).collect();
        for pair in lines.windows(2) {
            if let [attribute, declaration] = pair
                && *attribute == "#[cfg(test)]"
                && let Some(name) = declaration
                    .strip_prefix("mod ")
                    .and_then(|rest| rest.strip_suffix(';'))
                && let Some(base) = &base
            {
                test_files.push(base.join(format!("{name}.rs")));
            }
        }
    }
    Ok(all
        .into_iter()
        .filter(|(path, _)| !test_files.contains(path))
        .map(|(path, text)| {
            let shipped = match text.split("\n#[cfg(test)]").next() {
                Some(head) => head.to_owned(),
                None => String::new(),
            };
            (path, shipped)
        })
        .collect())
}

/// TI-5, PB-4: no host literal in anything that ships.
#[test]
fn the_shell_holds_no_host_literal_outside_tests() -> Result<(), String> {
    let shipped = shipping_sources()?;
    assert!(shipped.iter().any(|(path, _)| path.ends_with("tracer.rs")));
    assert!(
        !shipped.iter().any(|(path, _)| path.ends_with("doubles.rs")),
        "the doubles are test-only"
    );
    for (path, text) in shipped {
        for literal in [
            concat!("http", "://"),
            concat!("https", "://"),
            concat!("alpaca", ".markets"),
            concat!("local", "host"),
            concat!("127.0", ".0.1"),
        ] {
            assert!(
                !text.contains(literal),
                "{} holds {literal} outside a test module",
                path.display()
            );
        }
    }
    Ok(())
}

/// TI-5, PB-4: the shell implements no transport at all, test modules included. Its submission
/// counter sits at the `Connector` stage, so it needs none (task brief, Decisions needed 5).
#[test]
fn the_shell_implements_no_trading_transport() -> Result<(), String> {
    for (path, text) in sources()? {
        assert!(
            !text.contains(concat!("Trading", "Transport for")),
            "{}",
            path.display()
        );
    }
    Ok(())
}

/// Rung 1 of the trust ladder for TI-3: no defaulting combinator and no wildcard arm anywhere in
/// the crate, tests included, so no mapping can grow a permitting fallback unseen.
#[test]
fn no_defaulting_combinator_or_wildcard_arm_in_the_crate() -> Result<(), String> {
    let banned = [
        concat!(".unwrap", "_or"),
        concat!(".ok", "()"),
        concat!("_", " =>"),
    ];
    let files = sources()?;
    assert!(files.len() >= 10, "{} files", files.len());
    for (path, text) in files {
        for pattern in banned {
            assert!(
                !text.contains(pattern),
                "{} uses `{pattern}`",
                path.display()
            );
        }
    }
    Ok(())
}

/// The scripted broker, declaring `profile` as its capability profile.
struct Declaring {
    inner: ScriptedConnector,
    profile: CapabilityProfile,
}

impl Connector for Declaring {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        Ok(self.profile.clone())
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        self.inner.call(request)
    }
}

/// The paper executor, logging each reset, fold, profile handed over, and start.
struct Recording {
    inner: PaperExecutor,
    log: Rc<RefCell<Vec<String>>>,
}

impl Executor for Recording {
    fn reset(&mut self) -> Result<(), Cause> {
        self.log.borrow_mut().push("reset".to_owned());
        self.inner.reset()
    }

    fn use_profile(&mut self, profile: CapabilityProfile) -> Result<(), Cause> {
        let entry = format!("profile {}", profile.content_hash().to_hex());
        self.log.borrow_mut().push(entry);
        Ok(())
    }

    fn step(
        &mut self,
        input: mandate_executor::Input,
    ) -> Result<Vec<mandate_executor::Effect>, Cause> {
        if matches!(input, mandate_executor::Input::Started(_)) {
            self.log.borrow_mut().push("started".to_owned());
        }
        self.inner.step(input)
    }

    fn protected(&mut self, instrument: &InstrumentId) -> Result<bool, Cause> {
        self.log.borrow_mut().push("protection".to_owned());
        self.inner.protected(instrument)
    }

    fn committed(&mut self, event: &FoldedEvent) -> Result<(), Cause> {
        self.log.borrow_mut().push("fold".to_owned());
        self.inner.committed(event)
    }
}

/// DEC-838 item 5: every start, the first run's and a restart's alike, hands the executor the
/// connector's own capability profile once, after the replay's fold and before `Started`, so
/// protection never reads a profile the broker did not declare.
#[test]
fn every_start_hands_the_executor_the_connectors_profile_after_the_fold() -> Result<(), String> {
    let world = World::default();
    let profile = equities_only()?;
    let log = Rc::new(RefCell::new(Vec::new()));
    for _ in 0..2 {
        let mut stages = world.stages();
        stages.connector = Box::new(Declaring {
            inner: ScriptedConnector {
                world: world.clone(),
                script: Script::Accept,
            },
            profile: profile.clone(),
        });
        stages.executor = Box::new(Recording {
            inner: PaperExecutor {
                world: world.clone(),
                seen: BTreeSet::new(),
                inverted: false,
                last_client_order_id: None,
            },
            log: Rc::clone(&log),
        });
        let _ = run_with(stages)?;
    }
    let log = log.borrow();
    let handed = format!("profile {}", profile.content_hash().to_hex());
    let starts: Vec<&str> = log
        .iter()
        .map(String::as_str)
        .filter(|entry| *entry != "fold")
        .collect();
    let one_start = ["reset", handed.as_str(), "started"];
    assert_eq!(starts, [one_start, one_start].concat(), "{log:?}");
    let folded_after = log
        .windows(2)
        .any(|pair| matches!(pair, [first, next] if *first == handed && next != "started"));
    assert!(
        !folded_after,
        "nothing is folded between the profile and the start: {log:?}"
    );
    Ok(())
}
