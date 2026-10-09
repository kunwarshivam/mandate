//! The tracer's one pass (task brief, "The path, step by step") and the effect runner that performs
//! what the two cores describe.
//!
//! The runner is where `AGENTS.md` rule 5 becomes mechanical: it appends each `Effect::Journal`
//! and stops at the first append that is neither `Committed` nor `AlreadyCommitted`, it hands an
//! intent only when the draft recording it is known committed, and it lets a submission reach the
//! connector only when the `OrderSubmitted` recording it committed **in this run** (TI-1).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mandate_accounting::InstrumentId;
use mandate_alpaca::Pause;
use mandate_canon::{Digest, Key, Object, Value};
use mandate_executor::BrokerRequest;
use mandate_journal::{AppendOutcome, ArtifactRef, Environment, StoredEvent, get_artifact};
use mandate_runtime::{
    ActorKind, Autonomy, Classified, Deployment, DryRunVerdict, Effect, EventDraft, FlattenPlan,
    FlattenPlanner, FlattenRequest, FoldedEvent, GateDryRun, Input, IntentHandoff, MandateView,
    Observation, OrderPlan, Ports, Proposal, Purpose, RiskClock, RuntimeState, SignalInputs,
    WriterEpoch,
};
use mandate_time::UtcNanos;

use crate::envelope::{
    DraftFields, Envelope, IdSpace, Ids, Writer, account_stream, agent_stream, draft_bytes,
};
use crate::error::{Cause, ShellError, refused};
use crate::map;
use crate::stages::{
    Admitted, Classifier, ExitPath, Gate, GovernedRefs, JournalWriter, Sizing, Stage, Stages,
};
use crate::watch::Watch;

/// What a run is for, besides its stages. Nothing here is secret: every id is opaque (TI-8).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub deployment: Deployment,
    /// The account stream's subject: an opaque internal id, never the broker's account number.
    pub account_ref: String,
    /// The injected clock. The shell reads no wall clock (ES-21), so one setup replays exactly.
    pub now: UtcNanos,
    /// Without it the run stops before the submission and reports the order it would place
    /// (`AGENTS.md` rule 3): the flag is the only way to reach a broker.
    pub place_one_order: bool,
    /// Lets a run start a new cycle on an agent stream that already carries an intent (TI-12).
    pub new_cycle: bool,
}

/// What a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// The `client_order_id` of every submission handed to the connector.
    pub submitted: Vec<String>,
    /// The submission a run without `place_one_order` stopped before.
    pub would_place: Option<mandate_executor::SubmitOrder>,
    /// The message key of every alert the cores raised, in order. Keys only (`AGENTS.md` rule 6).
    pub alerts: Vec<&'static str>,
}

/// One pass of the tracer. Step 17 runs first: the exit path is probed before anything is armed,
/// so the tracer never opens a position whose exit it could not have planned (TI-4).
///
/// # Errors
/// Every [`ShellError`] is a stop after which nothing further is sent.
pub fn run(stages: &mut Stages, setup: &Setup) -> Result<Report, ShellError> {
    let admitted = admit(stages)?;
    let instrument = pinned(&admitted.view)?;
    let closes = stages
        .bars
        .closes(&admitted.symbol, setup.now)
        .map_err(refused(Stage::MarketData))?;
    let signal = stages
        .signal
        .signal(&admitted.model, &closes)
        .map_err(refused(Stage::Signal))?;
    let clock = RiskClock::from_secs(setup.now.secs());
    let output = map::model_output(signal, &admitted.model, &instrument, clock)
        .map_err(refused(Stage::Signal))?;
    execute_cycle(stages, setup, &admitted, None, output)
}

/// One pass over the model host's observation and its output (E15-13, the brief's slice H3,
/// DEC-503 item 6): the paper adapter stored the observation's data under its `data_ref` first,
/// and the runtime journals `ObservationRecorded` before `ModelOutputRecorded`, both before any
/// decision (FT-6). An observation whose artifact is not in the run's store stops the run before
/// either record and before any order, refused as `market_data_untrusted`. The shell reads no bars
/// and computes no output: the output it decides on is the one handed in.
///
/// # Errors
/// Every [`ShellError`] is a stop after which nothing further is sent.
pub fn run_observed(
    stages: &mut Stages,
    setup: &Setup,
    observation: Observation,
    output: mandate_runtime::ModelOutput,
) -> Result<Report, ShellError> {
    let admitted = admit(stages)?;
    execute_cycle(stages, setup, &admitted, Some(observation), output)
}

/// [`run_observed`], then E1b's bounded watch over the one submission (DEC-853 items 5 and 6,
/// FT-11): a run that submitted nothing watches nothing. Each wake pauses `watch.interval`, reads
/// the entry back by its client order id and ticks the executor at the clock's time; the first
/// wake at or past `watch.bound` hands the executor `CancelOpenings` instead of the read. The watch
/// ends once the broker reports the entry terminal.
///
/// # Errors
/// Every [`ShellError`] is a stop after which nothing further is sent.
pub fn run_observed_watched<P: Pause>(
    stages: &mut Stages,
    setup: &Setup,
    observation: Observation,
    output: mandate_runtime::ModelOutput,
    watch: &Watch<P>,
) -> Result<Report, ShellError> {
    let _ = (stages, setup, observation, output, watch);
    Err(ShellError::Refused {
        stage: Stage::Executor,
        cause: Cause::Unimplemented { story: "E7-19" },
    })
}

fn admit(stages: &mut Stages) -> Result<Admitted, ShellError> {
    stages.exit.probe().map_err(refused(Stage::FlattenProbe))?;
    stages
        .protection
        .probe()
        .map_err(refused(Stage::ProtectionProbe))?;
    let admitted = stages
        .mandate
        .admitted()
        .map_err(refused(Stage::Validate))?;
    if admitted.environment != Environment::Paper {
        return Err(ShellError::NonPaperEnvironment);
    }
    pinned(&admitted.view)?;
    Ok(admitted)
}

/// One cycle over `output`, after `observation` when the host handed one. The observation is
/// offered only once the replayed stream shows no open cycle, so a restart never journals it
/// again (FT-12), and always before the output, so `ObservationRecorded` precedes
/// `ModelOutputRecorded` (FT-6).
fn execute_cycle(
    stages: &mut Stages,
    setup: &Setup,
    admitted: &Admitted,
    observation: Option<Observation>,
    output: mandate_runtime::ModelOutput,
) -> Result<Report, ShellError> {
    let clock = RiskClock::from_secs(setup.now.secs());
    let mut session = Session::open_governed(stages, setup, &admitted.view, admitted.governed)?;
    session.start()?;
    if session.cycle_open && !setup.new_cycle {
        return Err(ShellError::CycleAlreadyOpen);
    }
    if let Some(observation) = observation {
        session.observe(observation)?;
    }
    session.feed(Input::ModelOutput(output))?;
    session.feed(Input::Tick(clock))?;
    Ok(session.report)
}

/// One production deployment cycle. Its assembled stages and setup are private, so callers can
/// supply model opinions but cannot replace the builder, gate, journal or executor (DEC-475).
pub struct ProductionCycle {
    stages: Stages,
    setup: Setup,
}

impl ProductionCycle {
    pub(crate) fn new(stages: Stages, setup: Setup) -> Self {
        Self { stages, setup }
    }

    /// Runs one cycle from a model output supplied by the model gateway.
    ///
    /// # Errors
    /// Every [`ShellError`] is a fail-closed stop after which nothing further is sent.
    pub fn run(&mut self, output: mandate_runtime::ModelOutput) -> Result<Report, ShellError> {
        let admitted = admit(&mut self.stages)?;
        execute_cycle(&mut self.stages, &self.setup, &admitted, None, output)
    }

    /// Runs one cycle and its watch, as [`run_observed_watched`] does (E1b, DEC-853).
    ///
    /// # Errors
    /// Every [`ShellError`] is a fail-closed stop after which nothing further is sent.
    pub fn run_observed_watched<P: Pause>(
        &mut self,
        observation: Observation,
        output: mandate_runtime::ModelOutput,
        watch: &Watch<P>,
    ) -> Result<Report, ShellError> {
        let _ = (observation, output, watch);
        Err(ShellError::Refused {
            stage: Stage::Executor,
            cause: Cause::Unimplemented { story: "E7-19" },
        })
    }

    /// Runs one cycle from the model host's observation and its output, as [`run_observed`] does
    /// (E15-13, the brief's slice H3).
    ///
    /// # Errors
    /// Every [`ShellError`] is a fail-closed stop after which nothing further is sent.
    pub fn run_observed(
        &mut self,
        observation: Observation,
        output: mandate_runtime::ModelOutput,
    ) -> Result<Report, ShellError> {
        let admitted = admit(&mut self.stages)?;
        execute_cycle(
            &mut self.stages,
            &self.setup,
            &admitted,
            Some(observation),
            output,
        )
    }
}

/// The one instrument the mandate pins. The tracer trades exactly one (DEC-138).
fn pinned(view: &MandateView) -> Result<InstrumentId, ShellError> {
    let mut universe = view.working_universe.iter();
    match (universe.next(), universe.next()) {
        (Some(only), None) => Ok(only.clone()),
        (None, _) | (Some(_), Some(_)) => Err(ShellError::UniverseNotPinned),
    }
}

/// One process's run over the two streams it writes.
pub(crate) struct Session<'s> {
    stages: &'s mut Stages,
    setup: &'s Setup,
    view: &'s MandateView,
    /// The registered objects a governed run's version-2 agent records reference (DEC-484).
    governed: Option<GovernedRefs>,
    state: RuntimeState,
    agent_stream: String,
    account_stream: String,
    epochs: BTreeMap<String, u64>,
    heads: BTreeMap<String, u64>,
    /// Every event id known committed: replayed, or appended in this run.
    recorded: BTreeSet<String>,
    /// The `client_order_id` of every `OrderSubmitted` committed **in this run** (TI-1).
    submitted_drafts: BTreeSet<String>,
    /// Whether the replayed agent stream already carries an intent (TI-12).
    pub(crate) cycle_open: bool,
    pub(crate) report: Report,
}

impl<'s> Session<'s> {
    /// Takes both streams and replays them, the agent stream first (journal spec §8). A run that
    /// `governed` governs writes its `ModelOutputRecorded` and `DecisionMade` at schema version 2
    /// with their registered references.
    pub(crate) fn open_governed(
        stages: &'s mut Stages,
        setup: &'s Setup,
        view: &'s MandateView,
        governed: Option<GovernedRefs>,
    ) -> Result<Self, ShellError> {
        let deployment = &setup.deployment;
        let agent = agent_stream(&deployment.workspace.0, &deployment.agent.0);
        let account = account_stream(&deployment.workspace.0, &setup.account_ref);
        let mut session = Self {
            state: RuntimeState::new(deployment.clone()),
            stages,
            setup,
            view,
            governed,
            agent_stream: agent,
            account_stream: account,
            epochs: BTreeMap::new(),
            heads: BTreeMap::new(),
            recorded: BTreeSet::new(),
            submitted_drafts: BTreeSet::new(),
            cycle_open: false,
            report: Report::default(),
        };
        session
            .stages
            .executor
            .reset()
            .map_err(refused(Stage::Executor))?;
        for stream in [session.agent_stream.clone(), session.account_stream.clone()] {
            let epoch = session
                .stages
                .journal
                .take_ownership(&stream)
                .map_err(refused(Stage::Journal))?;
            session.epochs.insert(stream.clone(), epoch);
            let rows = session
                .stages
                .journal
                .read(&stream)
                .map_err(refused(Stage::Journal))?;
            if rows.is_empty() {
                session.open_stream(&stream)?;
            } else {
                session.replay(&stream, &rows)?;
            }
        }
        Ok(session)
    }

    /// Hands the runtime `observation` once its data is in the run's store under its `data_ref`
    /// and re-hashes there; otherwise refuses before the runtime sees it, so no batch goes into
    /// doubt and a later input, the kill switch included, still steps (E15-13, H3; rule 13).
    ///
    /// The check is a checked read ([`get_artifact`]), which re-hashes the stored bytes, so other
    /// bytes under the digest are refused as missing ones are. It runs here, never inside the
    /// journal append: an append refused there would leave the runtime's batch in doubt (B8).
    pub(crate) fn observe(&mut self, observation: Observation) -> Result<(), ShellError> {
        let reference = ArtifactRef::from_digest(observation.data_ref);
        let stored = self
            .stages
            .artifacts
            .as_deref()
            .is_some_and(|store| get_artifact(store, &reference).is_ok());
        if !stored {
            return Err(ShellError::Refused {
                stage: Stage::MarketData,
                cause: Cause::Absent {
                    what: "the observation's stored data",
                },
            });
        }
        self.feed(Input::Observation(observation))
    }

    /// Journal §2's stream lifecycle: the shell that owns a new stream writes its
    /// `StreamOpened` at sequence 1 before either core receives `Started`. A replayed stream
    /// already has this record and never passes through here.
    fn open_stream(&mut self, stream: &str) -> Result<(), ShellError> {
        let workspace = self.setup.deployment.workspace.0.clone();
        let (writer, actor_id, space, members) = if stream == self.agent_stream {
            (
                Writer::Agent,
                self.setup.deployment.agent.0.as_str(),
                IdSpace::Agent,
                vec![
                    ("stream_type", "agent".to_owned()),
                    ("workspace_id", workspace),
                    ("agent_id", self.setup.deployment.agent.0.clone()),
                ],
            )
        } else {
            (
                Writer::Executor,
                "executor",
                IdSpace::Account,
                vec![
                    ("stream_type", "account".to_owned()),
                    ("workspace_id", workspace),
                    ("broker", "alpaca".to_owned()),
                    ("account_ref", self.setup.account_ref.clone()),
                ],
            )
        };
        let mut object = Object::new();
        for (name, value) in members {
            object.insert(
                Key::new(name).map_err(|_| ShellError::Envelope { field: name })?,
                Value::Str(value),
            );
        }
        let event_id = Ids { space }.derive(self.epoch(stream), 0, 0);
        let config_refs = Object::new();
        let bytes = draft_bytes(
            &Envelope {
                stream,
                writer,
                actor_id,
                event_time: self.setup.now,
            },
            &DraftFields {
                event_id: &event_id,
                event_type: "StreamOpened",
                schema_version: 1,
                causation_id: None,
                config_refs: &config_refs,
                payload: &Value::Object(object),
            },
        )?;
        self.append(stream, vec![bytes])
    }

    fn replay(&mut self, stream: &str, rows: &[StoredEvent]) -> Result<(), ShellError> {
        for row in rows {
            if stream == self.agent_stream && row.event_type == "IntentProposed" {
                self.cycle_open = true;
            }
            self.fold(stream, row)?;
        }
        Ok(())
    }

    /// Recovery, then the startup account report and reconciliation. The account report commits
    /// first; the runtime holds every opening until it and a clean `ReconciliationRun` have both
    /// folded (DEC-458), and a mismatch keeps it held.
    pub(crate) fn start(&mut self) -> Result<(), ShellError> {
        let agent_epoch = self.epoch(&self.agent_stream.clone());
        self.feed(Input::Started(WriterEpoch(agent_epoch)))?;
        let account_epoch = self.epoch(&self.account_stream.clone());
        let profile = self.stages.connector.profile().map_err(|_| {
            refused(Stage::Connector)(Cause::Untrusted {
                what: "the connector's capability profile",
            })
        })?;
        self.stages
            .executor
            .use_profile(profile)
            .map_err(refused(Stage::Executor))?;
        let executor_effects = self
            .stages
            .executor
            .step(mandate_executor::Input::Started(
                mandate_executor::WriterEpoch(account_epoch),
            ))
            .map_err(refused(Stage::Executor))?;
        let reconciliation_requests = self.prepare_reconciliation(executor_effects)?;
        let snapshot = self
            .stages
            .reconciler
            .snapshot(&mut *self.stages.connector, &reconciliation_requests)
            .map_err(refused(Stage::Reconcile))?;
        let account_effects = self
            .stages
            .executor
            .step(mandate_executor::Input::BrokerUpdate(
                mandate_executor::BrokerUpdate::Account(snapshot.account.clone()),
            ))
            .map_err(refused(Stage::Executor))?;
        self.perform_executor(account_effects)?;
        let reconciled = self
            .stages
            .reconciler
            .reconcile(&snapshot)
            .map_err(refused(Stage::Reconcile))?;
        self.perform_executor(reconciled.effects)?;
        match reconciled.verdict {
            mandate_executor::ReconciliationVerdict::Clean
            | mandate_executor::ReconciliationVerdict::Adopted => Ok(()),
            mandate_executor::ReconciliationVerdict::Mismatch => {
                self.report.alerts.push("reconciliation_mismatch");
                Err(ShellError::ReconciliationMismatch)
            }
        }
    }

    fn prepare_reconciliation(
        &mut self,
        effects: Vec<mandate_executor::Effect>,
    ) -> Result<Vec<BrokerRequest>, ShellError> {
        let mut requests = Vec::new();
        let mut effects = VecDeque::from(effects);
        while let Some(effect) = effects.pop_front() {
            match effect {
                mandate_executor::Effect::Broker(request) => match request {
                    BrokerRequest::ListOpenOrders
                    | BrokerRequest::ListPositions
                    | BrokerRequest::GetAccount
                    | BrokerRequest::ListActivities { .. } => requests.push(request),
                    BrokerRequest::Submit(_)
                    | BrokerRequest::Cancel { .. }
                    | BrokerRequest::AcknowledgeReplace { .. }
                    | BrokerRequest::GetOrderByClientId(_)
                    | BrokerRequest::ListOrders(_)
                    | BrokerRequest::CancelAll(_)
                    | BrokerRequest::ClosePosition(_, _) => {
                        self.perform_executor(vec![mandate_executor::Effect::Broker(request)])?;
                    }
                },
                mandate_executor::Effect::Journal(draft) => {
                    let drafts = consecutive_executor_journals(draft, &mut effects);
                    self.append_account(&drafts)?;
                }
                mandate_executor::Effect::Timer(_) | mandate_executor::Effect::Notify(_) => {
                    self.perform_executor(vec![effect])?;
                }
            }
        }
        Ok(requests)
    }

    /// One runtime step and every effect it describes, in order. A refusal a stage recorded during
    /// the step is reported after the step's effects ran, so what the runtime journaled about the
    /// refusal (a `DecisionMade` denial, an `ApprovalRequested`) is on the record.
    pub(crate) fn feed(&mut self, input: Input) -> Result<(), ShellError> {
        let (effects, refusal) = self.step(input)?;
        self.perform(effects)?;
        match refusal {
            Some(refusal) => Err(refusal),
            None => Ok(()),
        }
    }

    fn step(&mut self, input: Input) -> Result<(Vec<Effect>, Option<ShellError>), ShellError> {
        let bridge = Bridge {
            sizing: &*self.stages.sizing,
            classifier: &*self.stages.classifier,
            gate: &*self.stages.gate,
            exit: &*self.stages.exit,
            journal: &*self.stages.journal,
            stream: &self.account_stream,
            clock: self.state.risk_clock(),
            findings: RefCell::new(Findings::default()),
        };
        let ids = Ids {
            space: IdSpace::Agent,
        };
        let ports = Ports {
            ids: &ids,
            gate: &bridge,
            plan: &bridge,
            flatten: &bridge,
            view: self.view,
        };
        let effects = mandate_runtime::handle(&mut self.state, input, &ports)?;
        let findings = bridge.findings.into_inner();
        if let Some(poison) = findings.poison {
            return Err(poison);
        }
        let first = findings
            .refusals
            .into_iter()
            .min_by_key(|refusal| refusal.stage().map(Stage::position));
        Ok((effects, first))
    }

    pub(crate) fn perform(&mut self, effects: Vec<Effect>) -> Result<(), ShellError> {
        let mut effects = VecDeque::from(effects);
        while let Some(effect) = effects.pop_front() {
            match effect {
                Effect::Journal(draft) => {
                    let mut drafts = vec![draft];
                    while matches!(effects.front(), Some(Effect::Journal(_))) {
                        if let Some(Effect::Journal(next)) = effects.pop_front() {
                            drafts.push(next);
                        }
                    }
                    self.append_agent(&drafts)?;
                }
                Effect::Intent(handoff) => self.hand(&handoff)?,
                Effect::Timer(_) => {}
                Effect::Notify(notification) => self.report.alerts.push(notification.message_key),
                Effect::NotifyApproval(_) => self.report.alerts.push("approval_needed"),
            }
        }
        Ok(())
    }

    /// Step 9. The handoff's intent must be known committed — appended in this run, or replayed —
    /// which is the runtime's own rule for `Input::Started`'s re-hands (DEC-131 item 7).
    fn hand(&mut self, handoff: &IntentHandoff) -> Result<(), ShellError> {
        if !self.recorded.contains(&handoff.intent_id.0) {
            return Err(ShellError::WriteAheadViolated {
                effect: "intent handoff",
            });
        }
        let converted = self
            .stages
            .sink
            .hand(handoff)
            .map_err(refused(Stage::Sink))?;
        let effects = self
            .stages
            .executor
            .step(mandate_executor::Input::Intent(converted))
            .map_err(refused(Stage::Executor))?;
        self.perform_executor(effects)
    }

    fn perform_executor(
        &mut self,
        effects: Vec<mandate_executor::Effect>,
    ) -> Result<(), ShellError> {
        let mut queue = VecDeque::from(effects);
        while let Some(effect) = queue.pop_front() {
            match effect {
                mandate_executor::Effect::Journal(draft) => {
                    let drafts = consecutive_executor_journals(draft, &mut queue);
                    self.append_account(&drafts)?;
                }
                mandate_executor::Effect::Broker(request) => {
                    if let Some(input) = self.broker(&request)? {
                        let more = self
                            .stages
                            .executor
                            .step(input)
                            .map_err(refused(Stage::Executor))?;
                        queue.extend(more);
                    }
                }
                mandate_executor::Effect::Timer(_) => {}
                mandate_executor::Effect::Notify(notification) => {
                    self.report.alerts.push(notification.message_key);
                }
            }
        }
        Ok(())
    }

    /// Step 13. A submission goes out only when its `OrderSubmitted` committed in this run, only
    /// when the operator asked for an order, and only once per run.
    fn broker(
        &mut self,
        request: &BrokerRequest,
    ) -> Result<Option<mandate_executor::Input>, ShellError> {
        if let BrokerRequest::Submit(order) = request {
            if !self
                .submitted_drafts
                .contains(order.client_order_id.as_str())
            {
                return Err(ShellError::WriteAheadViolated {
                    effect: "submission",
                });
            }
            if !self.setup.place_one_order {
                self.report.would_place = Some(order.clone());
                return Ok(None);
            }
            if !self.report.submitted.is_empty() {
                return Err(ShellError::SecondSubmission);
            }
            self.report
                .submitted
                .push(order.client_order_id.as_str().to_owned());
        }
        let answer = self.stages.connector.call(request);
        map::broker_input(answer)
            .map(Some)
            .map_err(refused(Stage::Connector))
    }

    /// A governed run writes `ModelOutputRecorded` with `model_registry` and `DecisionMade` with
    /// `model_registry` and `policy_set` beside `mandate_version`, at schema version 2 (journal
    /// spec v0.16, DEC-484 item 4). Every other record, and every record of an ungoverned run,
    /// stays at version 1 with `mandate_version` alone.
    fn agent_refs(&self, event_type: &str) -> Result<(u64, Object), ShellError> {
        let mut refs = vec![("mandate_version", Value::Str(self.view.version.clone()))];
        let reference = |digest: Digest| Value::Str(format!("sha256:{digest}"));
        let mut version = 1;
        if let Some(governed) = self.governed {
            let decision = event_type == "DecisionMade";
            if decision || event_type == "ModelOutputRecorded" {
                refs.push(("model_registry", reference(governed.model_registry)));
                version = 2;
            }
            if decision {
                refs.push(("policy_set", reference(governed.policy_set)));
            }
        }
        let mut config_refs = Object::new();
        for (name, value) in refs {
            config_refs.insert(
                Key::new(name).map_err(|_| ShellError::Envelope { field: name })?,
                value,
            );
        }
        Ok((version, config_refs))
    }

    fn append_agent(&mut self, drafts: &[EventDraft]) -> Result<(), ShellError> {
        let stream = self.agent_stream.clone();
        let bytes = drafts
            .iter()
            .map(|draft| {
                let (schema_version, config_refs) = self.agent_refs(&draft.event_type)?;
                draft_bytes(
                    &Envelope {
                        stream: &stream,
                        writer: Writer::Agent,
                        actor_id: &self.setup.deployment.agent.0,
                        event_time: self.setup.now,
                    },
                    &DraftFields {
                        event_id: &draft.event_id.0,
                        event_type: &draft.event_type,
                        schema_version,
                        causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
                        config_refs: &config_refs,
                        payload: &draft.payload,
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.append(&stream, bytes)
    }

    fn append_account(
        &mut self,
        drafts: &[mandate_executor::EventDraft],
    ) -> Result<(), ShellError> {
        let stream = self.account_stream.clone();
        let bytes = drafts
            .iter()
            .map(|draft| {
                draft_bytes(
                    &Envelope {
                        stream: &stream,
                        writer: Writer::Executor,
                        actor_id: "executor",
                        event_time: self.setup.now,
                    },
                    &DraftFields {
                        event_id: &draft.event_id.0,
                        event_type: &draft.event_type,
                        schema_version: draft.schema_version,
                        causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
                        config_refs: &draft.config_refs,
                        payload: &draft.payload,
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.append(&stream, bytes)?;
        for draft in drafts {
            if draft.event_type == "OrderSubmitted"
                && let Some(id) = draft.payload.get("client_order_id").and_then(Value::as_str)
            {
                self.submitted_drafts.insert(id.to_owned());
            }
        }
        Ok(())
    }

    /// Appends one batch at the stream's head and folds what committed back into both cores. An
    /// `AlreadyCommitted` answer can name rows this session already folded; those are never folded
    /// twice, since a fold of a `seq` already held is out of order.
    fn append(&mut self, stream: &str, bytes: Vec<Vec<u8>>) -> Result<(), ShellError> {
        let head = self.head(stream);
        let epoch = self.epoch(stream);
        let outcome = self.stages.journal.append(stream, head, epoch, &bytes);
        let rows = match outcome {
            AppendOutcome::Committed(rows) | AppendOutcome::AlreadyCommitted(rows) => rows,
            refusal @ (AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::IdempotencyConflict { .. }
            | AppendOutcome::Fenced { .. }
            | AppendOutcome::Invalid { .. }
            | AppendOutcome::Unavailable
            | AppendOutcome::Ambiguous) => {
                return Err(ShellError::Refused {
                    stage: Stage::Journal,
                    cause: Cause::Append {
                        outcome: refusal.name(),
                    },
                });
            }
        };
        for row in rows {
            if !self.recorded.contains(&row.event_id) {
                self.fold(stream, &row)?;
            }
        }
        Ok(())
    }

    /// Folds one committed event into the runtime, and an account-stream event into the executor.
    fn fold(&mut self, stream: &str, row: &StoredEvent) -> Result<(), ShellError> {
        let (payload, causation) = body_of(row)?;
        mandate_runtime::fold(
            &mut self.state,
            &FoldedEvent {
                stream: stream.to_owned(),
                seq: mandate_runtime::Seq(row.seq),
                event_id: mandate_runtime::EventId(row.event_id.clone()),
                event_type: row.event_type.clone(),
                causation_id: causation.clone().map(mandate_runtime::EventId),
                actor: TRACER_ACTOR,
                payload: payload.clone(),
            },
        )?;
        if stream == self.account_stream {
            self.stages
                .executor
                .committed(&mandate_executor::FoldedEvent {
                    stream: stream.to_owned(),
                    seq: mandate_executor::Seq(row.seq),
                    event_id: mandate_executor::EventId(row.event_id.clone()),
                    event_type: row.event_type.clone(),
                    causation_id: causation.map(mandate_executor::EventId),
                    payload,
                })
                .map_err(refused(Stage::Executor))?;
        }
        self.recorded.insert(row.event_id.clone());
        self.heads.insert(stream.to_owned(), row.seq);
        Ok(())
    }

    fn head(&self, stream: &str) -> u64 {
        match self.heads.get(stream) {
            Some(seq) => *seq,
            None => 0,
        }
    }

    fn epoch(&self, stream: &str) -> u64 {
        match self.epochs.get(stream) {
            Some(epoch) => *epoch,
            None => 0,
        }
    }
}

fn consecutive_executor_journals(
    first: mandate_executor::EventDraft,
    effects: &mut VecDeque<mandate_executor::Effect>,
) -> Vec<mandate_executor::EventDraft> {
    let mut drafts = vec![first];
    while matches!(effects.front(), Some(mandate_executor::Effect::Journal(_))) {
        if let Some(mandate_executor::Effect::Journal(next)) = effects.pop_front() {
            drafts.push(next);
        }
    }
    drafts
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use mandate_canon::{Object, Value};
    use mandate_executor::{Effect, EventDraft, EventId, NotificationRef};

    use super::consecutive_executor_journals;

    fn draft(id: &str, event_type: &str, schema_version: u64) -> EventDraft {
        EventDraft {
            event_id: EventId(id.to_owned()),
            event_type: event_type.to_owned(),
            schema_version,
            config_refs: Object::new(),
            causation_id: None,
            payload: Value::Null,
        }
    }

    #[test]
    fn consecutive_executor_drafts_stay_in_one_append_run() {
        let companion = draft("companion", "OrderRequestRecorded", 1);
        let submitted = draft("submitted", "OrderSubmitted", 2);
        let boundary = Effect::Notify(NotificationRef {
            subject_event: EventId("submitted".to_owned()),
            message_key: "submitted",
        });
        let mut effects = VecDeque::from([Effect::Journal(submitted), boundary]);
        let drafts = consecutive_executor_journals(companion, &mut effects);
        assert_eq!(
            drafts
                .iter()
                .map(|draft| (draft.event_type.as_str(), draft.schema_version))
                .collect::<Vec<_>>(),
            [("OrderRequestRecorded", 1), ("OrderSubmitted", 2)]
        );
        assert!(matches!(effects.front(), Some(Effect::Notify(_))));
        assert_eq!(effects.len(), 1);
    }
}

/// The actor every event the tracer folds is read as. The tracer folds only its agent and account
/// streams, where no response is admitted from anyone, and reads no control stream; `system` is
/// what admission never admits (EI-10), so the control-stream tail that maps the envelope's
/// `actor.kind` is stream L's, with the tail itself (M7 brief, Decisions needed 5).
const TRACER_ACTOR: ActorKind = ActorKind::System;

/// The payload and causation a stored body carries, read from its canonical bytes rather than from
/// anything the shell remembers, so what is folded is what was committed.
fn body_of(row: &StoredEvent) -> Result<(Value, Option<String>), ShellError> {
    let body = mandate_canon::parse(&row.body).map_err(|_| ShellError::Envelope {
        field: "stored body",
    })?;
    let payload = body
        .get("payload")
        .cloned()
        .ok_or(ShellError::Envelope { field: "payload" })?;
    let causation = body
        .get("causation_id")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Ok((payload, causation))
}

/// What a step's ports found, read by the runner before it performs a single effect.
#[derive(Default)]
struct Findings {
    refusals: Vec<ShellError>,
    poison: Option<ShellError>,
}

/// The runtime's pure ports over the shell's stages, applying [`map`]'s total mappings and
/// recording why a stage did not answer.
struct Bridge<'a> {
    sizing: &'a dyn Sizing,
    classifier: &'a dyn Classifier,
    gate: &'a dyn Gate,
    exit: &'a dyn ExitPath,
    journal: &'a dyn JournalWriter,
    stream: &'a str,
    clock: Option<RiskClock>,
    findings: RefCell<Findings>,
}

impl Bridge<'_> {
    fn record(&self, refusal: ShellError) {
        self.findings.borrow_mut().refusals.push(refusal);
    }
}

impl OrderPlan for Bridge<'_> {
    fn plan(&self, view: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        let (proposal, refusal) = map::proposal_of(self.sizing.size(view, inputs));
        if let Some(refusal) = refusal {
            self.record(refusal);
        }
        proposal
    }

    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Classified {
        let answer = self.classifier.classify(view, proposal);
        let (classified, cause) = match answer {
            Err(cause) => (
                Classified {
                    autonomy: Autonomy::Deny,
                    decided_by: None,
                },
                Some(cause),
            ),
            Ok(classified) if classified.autonomy == Autonomy::Auto => (classified, None),
            Ok(classified) => {
                let cause = Cause::NotAuto {
                    autonomy: map::autonomy_name(classified.autonomy),
                };
                (classified, Some(cause))
            }
        };
        if let Some(cause) = cause {
            self.record(ShellError::Refused {
                stage: Stage::Classify,
                cause,
            });
        }
        classified
    }
}

impl GateDryRun for Bridge<'_> {
    fn check(&self, proposal: &Proposal) -> DryRunVerdict {
        let answer = self.gate.evaluate(proposal);
        let verdict = map::verdict_of(&answer, proposal.purpose);
        if let DryRunVerdict::Deny { reason_code } = &verdict {
            let cause = match answer {
                Err(cause) => cause,
                Ok(_) => Cause::GateAnswered {
                    reason_code: reason_code.clone(),
                },
            };
            self.record(ShellError::Refused {
                stage: Stage::GateDryRun,
                cause,
            });
        }
        verdict
    }
}

/// The poisoned adapter of the coordinator's ruling (task brief, Decisions needed 3).
/// `FlattenPlanner::plan` is infallible, so when the planner cannot answer, this sets the poison
/// flag and returns a plan that is **never acted on**: [`Session::step`] reads the flag before the
/// runner performs a single effect of the step, and halts. The position keeps whatever protection
/// already rests at the broker; nothing is invented and nothing empty is sent (TI-4, PB-8).
impl FlattenPlanner for Bridge<'_> {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        match self
            .exit
            .plan(request, self.journal, self.stream, self.clock)
        {
            Ok(plan) => plan,
            Err(cause) => {
                self.findings.borrow_mut().poison = Some(ShellError::Refused {
                    stage: Stage::FlattenProbe,
                    cause: Cause::Poisoned(cause.to_string()),
                });
                FlattenPlan {
                    cancel_client_order_ids: Vec::new(),
                    sells: Vec::new(),
                    purpose: Purpose::Flatten,
                    confirmation: request.confirmation.clone(),
                }
            }
        }
    }
}
