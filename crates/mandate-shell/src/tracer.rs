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
use mandate_canon::Value;
use mandate_executor::BrokerRequest;
use mandate_journal::{AppendOutcome, Environment, StoredEvent};
use mandate_runtime::{
    ActorKind, Autonomy, Classified, Deployment, DryRunVerdict, Effect, EventDraft, FlattenPlan,
    FlattenPlanner, FlattenRequest, FoldedEvent, GateDryRun, Input, IntentHandoff, MandateView,
    OrderPlan, Ports, Proposal, Purpose, RiskClock, RuntimeState, SignalInputs, WriterEpoch,
};
use mandate_time::UtcNanos;

use crate::envelope::{
    DraftFields, Envelope, IdSpace, Ids, Writer, account_stream, agent_stream, draft_bytes,
};
use crate::error::{Cause, ShellError, refused};
use crate::map;
use crate::stages::{Classifier, ExitPath, Gate, JournalWriter, Sizing, Stage, Stages};

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
    let instrument = pinned(&admitted.view)?;
    let closes = stages
        .bars
        .closes(&admitted.symbol)
        .map_err(refused(Stage::MarketData))?;
    let signal = stages
        .signal
        .signal(&admitted.model, &closes)
        .map_err(refused(Stage::Signal))?;
    let clock = RiskClock::from_secs(setup.now.secs());
    let output = map::model_output(signal, &admitted.model, &instrument, clock)
        .map_err(refused(Stage::Signal))?;
    let mut session = Session::open(stages, setup, &admitted.view)?;
    session.start()?;
    if session.cycle_open && !setup.new_cycle {
        return Err(ShellError::CycleAlreadyOpen);
    }
    session.feed(Input::ModelOutput(output))?;
    session.feed(Input::Tick(clock))?;
    Ok(session.report)
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
    /// Takes both streams and replays them, the agent stream first (journal spec §8).
    pub(crate) fn open(
        stages: &'s mut Stages,
        setup: &'s Setup,
        view: &'s MandateView,
    ) -> Result<Self, ShellError> {
        let deployment = &setup.deployment;
        let agent = agent_stream(&deployment.workspace.0, &deployment.agent.0);
        let account = account_stream(&deployment.workspace.0, &setup.account_ref);
        let mut session = Self {
            state: RuntimeState::new(deployment.clone()),
            stages,
            setup,
            view,
            agent_stream: agent,
            account_stream: account,
            epochs: BTreeMap::new(),
            heads: BTreeMap::new(),
            recorded: BTreeSet::new(),
            submitted_drafts: BTreeSet::new(),
            cycle_open: false,
            report: Report::default(),
        };
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
            session.replay(&stream, &rows)?;
        }
        Ok(session)
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

    /// Recovery, then the startup reconciliation. The runtime holds every opening until a clean
    /// `ReconciliationRun` has been folded (DEC-131 item 13), and a mismatch keeps it held.
    pub(crate) fn start(&mut self) -> Result<(), ShellError> {
        let agent_epoch = self.epoch(&self.agent_stream.clone());
        self.feed(Input::Started(WriterEpoch(agent_epoch)))?;
        let account_epoch = self.epoch(&self.account_stream.clone());
        let executor_effects = self
            .stages
            .executor
            .step(mandate_executor::Input::Started(
                mandate_executor::WriterEpoch(account_epoch),
            ))
            .map_err(refused(Stage::Executor))?;
        let reconciliation_requests = self.prepare_reconciliation(executor_effects)?;
        let reconciled = self
            .stages
            .reconciler
            .reconcile(&mut *self.stages.connector, &reconciliation_requests)
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
        for effect in effects {
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
                    | BrokerRequest::CancelAll(_)
                    | BrokerRequest::ClosePosition(_, _) => {
                        self.perform_executor(vec![mandate_executor::Effect::Broker(request)])?;
                    }
                },
                mandate_executor::Effect::Journal(_)
                | mandate_executor::Effect::Timer(_)
                | mandate_executor::Effect::Notify(_) => {
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
        for effect in effects {
            match effect {
                Effect::Journal(draft) => self.append_agent(&draft)?,
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
                mandate_executor::Effect::Journal(draft) => self.append_account(&draft)?,
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

    fn append_agent(&mut self, draft: &EventDraft) -> Result<(), ShellError> {
        let stream = self.agent_stream.clone();
        let bytes = draft_bytes(
            &Envelope {
                stream: &stream,
                writer: Writer::Agent,
                actor_id: &self.setup.deployment.agent.0,
                event_time: self.setup.now,
            },
            &DraftFields {
                event_id: &draft.event_id.0,
                event_type: &draft.event_type,
                causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
                payload: &draft.payload,
            },
        )?;
        self.append(&stream, bytes)
    }

    fn append_account(&mut self, draft: &mandate_executor::EventDraft) -> Result<(), ShellError> {
        let stream = self.account_stream.clone();
        let bytes = draft_bytes(
            &Envelope {
                stream: &stream,
                writer: Writer::Executor,
                actor_id: "executor",
                event_time: self.setup.now,
            },
            &DraftFields {
                event_id: &draft.event_id.0,
                event_type: &draft.event_type,
                causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
                payload: &draft.payload,
            },
        )?;
        self.append(&stream, bytes)?;
        if draft.event_type == "OrderSubmitted"
            && let Some(id) = draft.payload.get("client_order_id").and_then(Value::as_str)
        {
            self.submitted_drafts.insert(id.to_owned());
        }
        Ok(())
    }

    /// Appends one draft at the stream's head and folds what committed back into both cores. An
    /// `AlreadyCommitted` answer can name rows this session already folded; those are never folded
    /// twice, since a fold of a `seq` already held is out of order.
    fn append(&mut self, stream: &str, bytes: Vec<u8>) -> Result<(), ShellError> {
        let head = self.head(stream);
        let epoch = self.epoch(stream);
        let rows = match self.stages.journal.append(stream, head, epoch, &[bytes]) {
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

    /// The stage classifier names no `DecidedBy` label, so its answer carries none.
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Classified {
        let answer = self.classifier.classify(view, proposal);
        let autonomy = map::autonomy_of(&answer);
        let cause = match answer {
            Err(cause) => Some(cause),
            Ok(Autonomy::Auto) => None,
            Ok(other @ (Autonomy::Ask | Autonomy::Deny)) => Some(Cause::NotAuto {
                autonomy: map::autonomy_name(other),
            }),
        };
        if let Some(cause) = cause {
            self.record(ShellError::Refused {
                stage: Stage::Classify,
                cause,
            });
        }
        Classified {
            autonomy,
            decided_by: None,
        }
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
