//! The effect runner that performs what the two cores describe, over one production cycle
//! (`mandate_runtime::Input::ModelOutput` is the strategy boundary, DEC-475 item 1).
//!
//! The runner is where `AGENTS.md` rule 5 becomes mechanical: it appends each `Effect::Journal`
//! and stops at the first append that is neither `Committed` nor `AlreadyCommitted`, it hands an
//! intent only when the draft recording it is known committed, and it lets a submission reach the
//! connector only when the `OrderSubmitted` recording it committed **in this run** (TI-1).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

use mandate_accounting::InstrumentId;
use mandate_alpaca::{LatestQuote, Pause};
use mandate_canon::{Digest, Key, Object, Value};
use mandate_executor::{BrokerOutcome, BrokerRequest, ReconciliationVerdict};
use mandate_journal::{AppendOutcome, ArtifactRef, StoredEvent, get_artifact};
use mandate_num::Price;
use mandate_runtime::{
    ActorKind, Autonomy, Classified, Deployment, DryRunVerdict, Effect, EventDraft, FlattenPlan,
    FlattenPlanner, FlattenRequest, FoldedEvent, GateDryRun, Input, IntentHandoff, MandateView,
    Observation, OrderPlan, Ports, Proposal, Purpose, RiskClock, RuntimeState, SignalInputs,
    WriterEpoch,
};
use mandate_time::UtcNanos;

use crate::approvers::ApproverSource;
use crate::control::ControlRecord;
use crate::envelope::{
    DraftFields, Envelope, IdSpace, Ids, Writer, account_stream, agent_stream, control_stream,
    draft_bytes, journal_environment,
};
use crate::error::{Cause, ShellError, refused};
use crate::lt10::watch_may_end;
use crate::map;
use crate::stages::{
    Admitted, Classifier, ExitPath, Gate, GovernedRefs, JournalWriter, Sizing, Stage, Stages,
};
use crate::tail::tailed;
use crate::watch::Watch;

mod own_fills;

pub use own_fills::Booked;

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
    /// The environment of the connection the run chose (the live door's choice): the one
    /// environment the admitted mandate may name and every draft of the run carries (TI-7 as
    /// DEC-965 reads it).
    pub connection_environment: mandate_domain::Environment,
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
    let admitted = admit(stages, setup)?;
    execute_cycle(stages, setup, &admitted, Some(observation), output)
}

/// [`run_observed`], then E1b's bounded watch over the one submission (DEC-853 items 5 and 6,
/// FT-11): a run that submitted nothing watches nothing. Each wake pauses `watch.interval`, reads
/// the entry back by its client order id and ticks the executor at the clock's time; the first
/// wake at or past `watch.bound` hands the executor `CancelOpenings` instead of the read. The
/// watch ends when the run's LT-10 holds ([`watch_may_end`], DEC-912 item 3): the entry terminal
/// in DEC-858 item 4's four with any filled quantity covered, or the uncovered interval's own
/// alert key reported, past which it fails closed at the session's end.
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
    let admitted = admit(stages, setup)?;
    let instrument = pinned(&admitted.view)?;
    let mut session = open_cycle(stages, setup, &admitted, Some(observation), output)?;
    session.watch(&instrument, watch, &watch_may_end)?;
    Ok(session.report)
}

fn admit(stages: &mut Stages, setup: &Setup) -> Result<Admitted, ShellError> {
    stages.exit.probe().map_err(refused(Stage::FlattenProbe))?;
    stages
        .protection
        .probe()
        .map_err(refused(Stage::ProtectionProbe))?;
    let admitted = stages
        .mandate
        .admitted()
        .map_err(refused(Stage::Validate))?;
    if admitted.environment != journal_environment(setup.connection_environment) {
        return Err(ShellError::EnvironmentMismatch);
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
    open_cycle(stages, setup, admitted, observation, output).map(|session| session.report)
}

/// [`execute_cycle`]'s cycle, handing back the session so a watch can keep it (DEC-858 item 3).
fn open_cycle<'s>(
    stages: &'s mut Stages,
    setup: &'s Setup,
    admitted: &'s Admitted,
    observation: Option<Observation>,
    output: mandate_runtime::ModelOutput,
) -> Result<Session<'s>, ShellError> {
    match open_asking(stages, setup, admitted, None, observation, output)? {
        (session, None) => Ok(session),
        (_, Some(asked)) => Err(asked),
    }
}

/// [`open_cycle`]'s cycle, handing back the session beside the classifier's refusal when the
/// cycle asked (DEC-940 item 1): the ask's records are committed by then (the step's effects
/// run before its refusal is reported), so the session can wait for the answer. Every other
/// refusal stops the cycle as before. A `mark` the run read is journaled after the control
/// stream's tail and before the observation and the output, so the request the cycle may
/// journal records it as its reference (mandate spec §6.4; DEC-943 item 1).
fn open_asking<'s>(
    stages: &'s mut Stages,
    setup: &'s Setup,
    admitted: &'s Admitted,
    mark: Option<&Mark>,
    observation: Option<Observation>,
    output: mandate_runtime::ModelOutput,
) -> Result<(Session<'s>, Option<ShellError>), ShellError> {
    let clock = RiskClock::from_secs(setup.now.secs());
    let mut session = Session::open_governed(stages, setup, admitted)?;
    session.start()?;
    if session.cycle_open && !setup.new_cycle {
        return Err(ShellError::CycleAlreadyOpen);
    }
    session.tail_the_control_stream()?;
    if let Some(mark) = mark {
        session.mark(mark)?;
    }
    if let Some(observation) = observation {
        session.observe(observation)?;
    }
    session.tick_executor()?;
    let fed = session
        .feed(Input::ModelOutput(output))
        .and_then(|()| session.feed(Input::Tick(clock)));
    match fed {
        Ok(()) => Ok((session, None)),
        Err(refusal) if asked(&refusal) => Ok((session, Some(refusal))),
        Err(refusal) => Err(refusal),
    }
}

/// Whether `refusal` is the classifier's `ask`, the one refusal after which the cycle's request
/// is committed and waits for the owner's answer (mandate spec §6.4; DEC-940 item 1).
fn asked(refusal: &ShellError) -> bool {
    matches!(
        refusal,
        ShellError::Refused {
            stage: Stage::Classify,
            cause: Cause::NotAuto { autonomy: "ask" },
        }
    )
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
        let admitted = admit(&mut self.stages, &self.setup)?;
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
        run_observed_watched(&mut self.stages, &self.setup, observation, output, watch)
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
        let admitted = admit(&mut self.stages, &self.setup)?;
        execute_cycle(
            &mut self.stages,
            &self.setup,
            &admitted,
            Some(observation),
            output,
        )
    }
}

/// One wake's read of the two streams an asked cycle's wait folds (DEC-920 item 1; DEC-940): the
/// journal's own `ORDER BY seq` read of the agent stream the run appends to, where the runtime's
/// `ApprovalResponded` and `ApprovalRevalidated` land, and of the workspace control stream, where
/// the owner's answers land, both taken after the wake handed the runtime its answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wake {
    /// The agent stream's rows.
    pub agent: Vec<StoredEvent>,
    /// The workspace control stream's rows.
    pub control: Vec<StoredEvent>,
}

/// The open session of a cycle that asked (G1b, DEC-940), as the runner's grant wait drives it:
/// the session stays the shell's (DEC-475), and the runner reads, wakes and releases through
/// these four calls only.
pub trait AskedCycle {
    /// The agent stream's rows as the journal reads them now: the ask the cycle journaled is the
    /// youngest `ApprovalRequested` among them (DEC-914 item 1).
    ///
    /// # Errors
    /// The journal's refusal.
    fn agent_rows(&mut self) -> Result<Vec<StoredEvent>, ShellError>;

    /// One wake at `now`: `mark`, the risk mark of the IEX quote the runner read for this wake,
    /// journaled as the account stream's `MarkUpdated` first, so the re-validation this wake may
    /// run compares the request's reference with it (DEC-943 item 1); then the control stream
    /// tailed whole and handed to the runtime as its owner input, then the risk clock ticked to
    /// `now`, then both streams read back. An opening the runtime releases is held, never sent,
    /// until [`AskedCycle::submit`].
    ///
    /// # Errors
    /// Every [`ShellError`] is a stop after which nothing is sent.
    fn wake(&mut self, now: UtcNanos, mark: &Mark) -> Result<Wake, ShellError>;

    /// One wake at or past the ask's deadline, `now` (DEC-949): the risk clock ticked to `now`
    /// alone, so the runtime closes the ask with its own `ApprovalTimedOut` (journal spec §9.7,
    /// rule 135; mandate spec §6.4's `on_timeout: skip`), then both streams read back. No mark is
    /// journaled and no answer is handed: an answer the runtime has not yet read is late at
    /// `now`, so nothing it holds can release the opening, and the deny needs no quote. An
    /// opening the runtime releases is still held, never sent; an exit or a protective order
    /// passes the door as at every wake (rule 13).
    ///
    /// # Errors
    /// Every [`ShellError`] is a stop after which nothing is sent.
    fn expire(&mut self, now: UtcNanos) -> Result<Wake, ShellError>;

    /// Sends the one opening the runtime released and the wait let leave.
    ///
    /// # Errors
    /// Every [`ShellError`] is a stop; with no held opening, nothing is sent.
    fn submit(&mut self) -> Result<(), ShellError>;
}

/// The watch's end question an asked run's caller hands in (LT-10, DEC-914 items 4 and 5,
/// DEC-940): over the wake's broker read, the executor's uncovered answer, the report's alert
/// keys, the wake's clock and the watch's bound, `Ok(true)` ends the run, `Ok(false)` reads on,
/// and an error stops it, as [`watch_may_end`] answers.
pub type MayEnd<'e> =
    &'e dyn Fn(Option<&str>, bool, &[&'static str], UtcNanos, UtcNanos) -> Result<bool, ShellError>;

/// One risk mark the run read (trading-domain spec §8.2; mandate spec §5.2's `MarkUpdated`):
/// the bid of an IEX quote, a long's risk mark, and the quote's own stamp (DEC-943 item 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mark {
    /// The quote's bid.
    pub price: Price,
    /// The quote's own stamp, the instant the mark is of.
    pub at: UtcNanos,
}

impl Mark {
    /// The risk mark of an IEX quote the run read and judged: its bid, at its own stamp.
    #[must_use]
    pub fn of_iex(quote: &LatestQuote) -> Self {
        Self {
            price: quote.bid,
            at: quote.at,
        }
    }
}

impl ProductionCycle {
    /// [`run_observed_asked_watched`] over this cycle's own stages and setup (G1b, DEC-940).
    ///
    /// # Errors
    /// Every error is a stop after which nothing further is sent.
    pub fn run_observed_asked_watched<P, E, W>(
        &mut self,
        observation: Observation,
        output: mandate_runtime::ModelOutput,
        watch: &Watch<P>,
        mark: &Mark,
        wait: W,
        may_end: MayEnd<'_>,
    ) -> Result<Report, E>
    where
        P: Pause,
        E: From<ShellError>,
        W: FnOnce(&mut dyn AskedCycle) -> Result<(), E>,
    {
        run_observed_asked_watched(
            &mut self.stages,
            &self.setup,
            observation,
            output,
            watch,
            mark,
            wait,
            may_end,
        )
    }
}

/// [`run_observed_watched`] for a run that may ask (G1b, DEC-940). `mark` is the risk mark of
/// the preflight's IEX quote, journaled on the account stream before the observation, so a
/// request the cycle journals records it as its reference (DEC-943 item 1). A cycle that never
/// asks is exactly that run: `wait` is never called and the watch asks the shell's own
/// [`watch_may_end`]. A cycle that asks keeps its session open: a dry run stops at the ask as
/// before, because it appends to no journal an answer could reach; a placing run hands the
/// session to `wait` as an [`AskedCycle`], whose every wake journals the runner's fresh mark,
/// hands the control stream's answers to the runtime and reads both streams back, and whose
/// opening the runtime releases is held until `wait` submits it — so nothing is sent before the
/// runner's own release, and a `wait` that stops, or returns with the opening still held, sends
/// nothing (`AGENTS.md` rule 3). Only a risk-adding submission is held: an exit or a protective
/// order is sent as it always was (rule 13). After the submit the watch ends through `may_end`,
/// the caller's LT-10 question (DEC-914 items 4 and 5).
///
/// # Errors
/// Every error is a stop after which nothing further is sent.
#[allow(
    clippy::too_many_arguments,
    reason = "the asked run's door: the cycle's inputs, its watch, its mark, and the runner's two frames"
)]
pub fn run_observed_asked_watched<P, E, W>(
    stages: &mut Stages,
    setup: &Setup,
    observation: Observation,
    output: mandate_runtime::ModelOutput,
    watch: &Watch<P>,
    mark: &Mark,
    wait: W,
    may_end: MayEnd<'_>,
) -> Result<Report, E>
where
    P: Pause,
    E: From<ShellError>,
    W: FnOnce(&mut dyn AskedCycle) -> Result<(), E>,
{
    let admitted = admit(stages, setup)?;
    let instrument = pinned(&admitted.view)?;
    let (mut session, asked) = open_asking(
        stages,
        setup,
        &admitted,
        Some(mark),
        Some(observation),
        output,
    )?;
    let Some(asked) = asked else {
        session.watch(&instrument, watch, &watch_may_end)?;
        return Ok(session.report);
    };
    if !setup.place_one_order {
        return Err(E::from(asked));
    }
    session.holding = true;
    wait(&mut session)?;
    session.holding = false;
    if session.held.is_some() {
        return Err(E::from(nothing_released()));
    }
    session.watch(&instrument, watch, may_end)?;
    Ok(session.report)
}

/// What the broker door does with a submission while an asked cycle's wait may hold one
/// (DEC-940 item 2).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Door {
    /// Not held: the door's own one-order rule decides as it always did.
    Pass,
    /// A risk-adding submission while the wait holds: kept until [`AskedCycle::submit`].
    Hold,
    /// A second risk-adding submission while the wait holds, after one already held or sent:
    /// refused, nothing sent (FT-1).
    Second,
}

/// The broker door's hold (DEC-940 item 2): only while `holding`, and only a `purpose` that adds
/// risk. An exit, a protective order or a flatten always passes (`AGENTS.md` rule 13).
pub(crate) fn door(
    holding: bool,
    held: bool,
    opened: bool,
    purpose: mandate_executor::Purpose,
) -> Door {
    if !(holding && purpose.adds_risk()) {
        return Door::Pass;
    }
    if held || opened {
        return Door::Second;
    }
    Door::Hold
}

/// The stop of a held opening the wait never released, and of a submit with none held
/// (DEC-940 item 2): nothing is sent.
fn nothing_released() -> ShellError {
    ShellError::Refused {
        stage: Stage::Executor,
        cause: Cause::Absent {
            what: "the held opening the grant wait released",
        },
    }
}

impl AskedCycle for Session<'_> {
    fn agent_rows(&mut self) -> Result<Vec<StoredEvent>, ShellError> {
        self.stages
            .journal
            .read(&self.agent_stream)
            .map_err(refused(Stage::Journal))
    }

    fn wake(&mut self, now: UtcNanos, mark: &Mark) -> Result<Wake, ShellError> {
        self.event_time = now;
        self.mark(mark)?;
        let stream = control_stream(&self.setup.deployment.workspace.0);
        let control = self
            .stages
            .journal
            .read(&stream)
            .map_err(refused(Stage::Journal))?;
        self.reresolve(&control);
        for event in tailed(&control)? {
            self.reresolve_at(&control, event.seq.0);
            self.feed_asked(Input::Journal(event))?;
        }
        self.feed_asked(Input::Tick(RiskClock::from_secs(now.secs())))?;
        Ok(Wake {
            agent: self.agent_rows()?,
            control,
        })
    }

    fn expire(&mut self, now: UtcNanos) -> Result<Wake, ShellError> {
        self.event_time = now;
        self.feed_asked(Input::Tick(RiskClock::from_secs(now.secs())))?;
        let stream = control_stream(&self.setup.deployment.workspace.0);
        let control = self
            .stages
            .journal
            .read(&stream)
            .map_err(refused(Stage::Journal))?;
        Ok(Wake {
            agent: self.agent_rows()?,
            control,
        })
    }

    fn submit(&mut self) -> Result<(), ShellError> {
        let order = self.held.take().ok_or_else(nothing_released)?;
        self.holding = false;
        if let Some(answer) = self.broker(&BrokerRequest::Submit(order))? {
            let effects = self
                .stages
                .executor
                .step(answer)
                .map_err(refused(Stage::Executor))?;
            self.perform_executor(effects)?;
        }
        Ok(())
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
    /// What `view`'s approvers are resolved from again at each control-stream read (DEC-2384
    /// item 6).
    approvers: &'s ApproverSource,
    /// `view` with its approvers narrowed to the latest control-stream read's, once one was
    /// read: the view every step hands the runtime.
    current: Option<MandateView>,
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
    /// Whether an asked cycle's wait holds its released opening (DEC-940 item 2).
    holding: bool,
    /// The risk-adding submission held while `holding`, sent only by [`AskedCycle::submit`].
    held: Option<mandate_executor::SubmitOrder>,
    /// The one risk-adding submission this run sent, which the watch watches (FT-1): a
    /// protective order or an exit beside it is never counted against it (`AGENTS.md` rule 13).
    opening: Option<mandate_executor::ClientOrderId>,
    pub(crate) report: Report,
    /// The time every record this session appends carries: the run's clock, then each wake's
    /// during a watch (DEC-858 item 3).
    event_time: UtcNanos,
}

impl<'s> Session<'s> {
    /// Takes both streams and replays them, the agent stream first (journal spec §8). A run that
    /// `governed` governs writes its `ModelOutputRecorded` and `DecisionMade` at schema version 2
    /// with their registered references.
    pub(crate) fn open_governed(
        stages: &'s mut Stages,
        setup: &'s Setup,
        admitted: &'s Admitted,
    ) -> Result<Self, ShellError> {
        let (view, governed) = (&admitted.view, admitted.governed);
        let deployment = &setup.deployment;
        let agent = agent_stream(&deployment.workspace.0, &deployment.agent.0);
        let account = account_stream(&deployment.workspace.0, &setup.account_ref);
        let mut session = Self {
            state: RuntimeState::new(deployment.clone()),
            stages,
            setup,
            view,
            approvers: &admitted.approvers,
            current: None,
            governed,
            agent_stream: agent,
            account_stream: account,
            epochs: BTreeMap::new(),
            heads: BTreeMap::new(),
            recorded: BTreeSet::new(),
            submitted_drafts: BTreeSet::new(),
            cycle_open: false,
            holding: false,
            held: None,
            opening: None,
            report: Report::default(),
            event_time: setup.now,
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

    /// The control-stream tail's install at the open (E7-26, DEC-155 items 2 and 5): one read of
    /// the workspace control stream — `ctl:{workspace}`, whose single writer is the workspace
    /// control services, in Phase 1 the founder's CLI (journal spec §2), so unlike the two owned
    /// streams no ownership is taken and nothing here fences the owner — folded by [`tailed`] and
    /// handed to the runtime one event per row, each as the `Input::Journal` its owner-input
    /// routing reads, in the journal's own order ("The owner's answer arrives only as a
    /// control-stream `ApprovalResponseSubmitted`, tailed and handed in as `Input::Journal`",
    /// `crates/mandate-runtime/src/escalation.rs`). Every row is handed and the runtime's own arms
    /// decide what each acts on (DEC-138 item 3, DEC-914 item 2: never re-sorted, never filtered).
    ///
    /// The read is the **whole stream**, taken after `Input::Started`'s recovery and the cycle's
    /// one-order guard, before the cycle proposes anything, so an answer committed while no run was
    /// open is still judged — "If the runtime is down, the event waits in the journal and is judged
    /// when it is read" (the M7 brief) — against the replayed state, as the same brief's restart
    /// row reads: "A control-stream response committed before the crash is re-tailed and judged at
    /// `max(submitted_at, folded clock)`". No cursor is kept beside the journal's own
    /// `ORDER BY seq`: the runtime's copy fold is the idempotence ("copied ... at most once: a
    /// control-stream event this runtime has already copied is inert, however often it is tailed",
    /// EI-3, DEC-155 item 2), and a cursor that handed only the rows after a remembered head would
    /// be the filter DEC-914 item 2 forbids. The guard in front keeps the answers from opening a
    /// second cycle behind an already-open intent; the wait between runs is the owner's, never the
    /// shell's (DEC-912 item 1: the runner's live wait releases, it never submits, and its
    /// per-wake re-read is the runner's own call site).
    ///
    /// Nothing here folds the control rows into the runtime's replay state: `Input::Journal` is the
    /// answer's whole path, and the runtime's own fold reserves what it does not interpret as
    /// `NotInterpreted` (DEC-85) — the deployment and registration records journal spec §9.2 lays
    /// on the same stream among them, which the shell reads through the validation context, never
    /// through the runtime — so a state fold of the read would refuse a workspace whose control
    /// stream this tail can read whole. The call order is this row's to write and its review's to
    /// judge (DEC-914 item 7).
    fn tail_the_control_stream(&mut self) -> Result<(), ShellError> {
        let stream = control_stream(&self.setup.deployment.workspace.0);
        let rows = self
            .stages
            .journal
            .read(&stream)
            .map_err(refused(Stage::Journal))?;
        self.reresolve(&rows);
        for event in tailed(&rows)? {
            self.reresolve_at(&rows, event.seq.0);
            self.feed(Input::Journal(event))?;
        }
        Ok(())
    }

    /// Narrows the approvers every later step hands the runtime to those `rows`, a fresh read
    /// of the control stream, still resolve to (DEC-2384 item 6): a row the run cannot read
    /// leaves no approver, so every grant is refused (`AGENTS.md` rule 3).
    fn reresolve(&mut self, rows: &[StoredEvent]) {
        let records: Option<Vec<ControlRecord>> = rows.iter().map(ControlRecord::of_row).collect();
        let mut view = self.view.clone();
        view.approval.approvers = match records {
            Some(records) => self
                .approvers
                .current(&self.view.approval.approvers, &records),
            None => BTreeSet::new(),
        };
        self.current = Some(view);
    }

    /// [`Self::reresolve`] over the rows of `rows`, read in `seq` order, up to and including `seq`:
    /// each tailed event is judged against the roster as of its own record, so a membership record
    /// later in the same read neither lends a grant a role its member lacked when it was made nor
    /// takes one away (DEC-2384 item 6). A split that cannot be made reads no row, which resolves
    /// no approver (`AGENTS.md` rule 3).
    fn reresolve_at(&mut self, rows: &[StoredEvent], seq: u64) {
        match rows.split_at_checked(rows.partition_point(|row| row.seq <= seq)) {
            Some((judged, _)) => self.reresolve(judged),
            None => self.reresolve(&[]),
        }
    }

    /// Journals `mark` as the account stream's `MarkUpdated` for the pinned instrument (journal
    /// spec §9's registered version 1: `instrument_id`, `price`, `source`, `feed` and
    /// `risk_clock`), and folds it into both cores, so the runtime's latest mark is the one the
    /// run read (mandate spec §6.4's reference and drift; DEC-943 item 1). The envelope's
    /// `event_time` is the quote's own stamp; the `risk_clock` is the session's second, the tick
    /// the record is folded at (mandate spec §5.2: `event_time` is never used for risk timing).
    /// The executor is the account stream's writer, so the record is written as its own, with an
    /// id derived from the stream's head as the executor derives its own.
    fn mark(&mut self, mark: &Mark) -> Result<(), ShellError> {
        let instrument = pinned(self.view)?;
        let stream = self.account_stream.clone();
        let tick =
            UtcNanos::from_parts(self.event_time.secs(), 0).map_err(|_| ShellError::Envelope {
                field: "risk_clock",
            })?;
        let mut object = Object::new();
        for (name, value) in [
            ("instrument_id", instrument.as_str().to_owned()),
            ("price", mark.price.to_string()),
            ("source", MARK_SOURCE.to_owned()),
            ("feed", MARK_FEED.to_owned()),
            ("risk_clock", tick.to_string()),
        ] {
            object.insert(
                Key::new(name).map_err(|_| ShellError::Envelope { field: name })?,
                Value::Str(value),
            );
        }
        let event_id = Ids {
            space: IdSpace::Account,
        }
        .derive(self.epoch(&stream), self.head(&stream), 0);
        let config_refs = Object::new();
        let bytes = draft_bytes(
            &Envelope {
                stream: &stream,
                writer: Writer::Executor,
                actor_id: "executor",
                event_time: mark.at,
                environment: journal_environment(self.setup.connection_environment),
            },
            &DraftFields {
                event_id: &event_id,
                event_type: "MarkUpdated",
                schema_version: 1,
                causation_id: None,
                config_refs: &config_refs,
                payload: &Value::Object(object),
            },
        )?;
        self.append(&stream, vec![bytes])
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
                event_time: self.event_time,
                environment: journal_environment(self.setup.connection_environment),
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

    /// The executor learns the run's injected clock (ES-21) before the cycle's output reaches
    /// the runtime, so the intent's `OrderSubmitted` carries the run's time and a restart's
    /// listing of it starts five minutes before that (DEC-941 item 5). It follows the startup
    /// reconciliation and the control stream's tail, so a held release, an overdue opening or a
    /// ladder step it starts runs only after a pause or kill the control stream holds.
    fn tick_executor(&mut self) -> Result<(), ShellError> {
        let ticked = self
            .stages
            .executor
            .step(mandate_executor::Input::Tick(
                mandate_executor::RiskClock::from_secs(self.setup.now.secs()),
            ))
            .map_err(refused(Stage::Executor))?;
        self.perform_executor(ticked)
    }

    /// Recovery, then the startup account report and reconciliation. The executor is ticked at the
    /// run clock's second before its start, so every record the start leads to carries the run's
    /// risk clock, never zero (mandate spec §5.2, DEC-948 item 4); that tick only sets the clock,
    /// and evaluates nothing until [`Session::tick_executor`]. The account report commits
    /// first; the runtime holds every opening until it and a clean `ReconciliationRun` have both
    /// folded (DEC-458), and a mismatch keeps it held.
    pub(crate) fn start(&mut self) -> Result<(), ShellError> {
        let agent_epoch = self.epoch(&self.agent_stream.clone());
        self.feed(Input::Started(WriterEpoch(agent_epoch)))?;
        let account_epoch = self.epoch(&self.account_stream.clone());
        let run_clock = mandate_executor::RiskClock::from_secs(self.setup.now.secs());
        let ticked = self
            .stages
            .executor
            .step(mandate_executor::Input::Tick(run_clock))
            .map_err(refused(Stage::Executor))?;
        self.perform_executor(ticked)?;
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
        let (verdict, more) = self.reconcile_once(&reconciliation_requests)?;
        self.stop_on(verdict)?;
        self.settle(more)
    }

    /// A mismatch stops the run fail-safe, wherever the reconciliation ran (trading spec §11,
    /// `AGENTS.md` rule 3): its alert key goes into the report, opaque as rule 6 requires, and
    /// the run answers [`ShellError::ReconciliationMismatch`], after which nothing is sent.
    fn stop_on(&mut self, verdict: ReconciliationVerdict) -> Result<(), ShellError> {
        match verdict {
            ReconciliationVerdict::Clean | ReconciliationVerdict::Adopted => Ok(()),
            ReconciliationVerdict::Mismatch => {
                self.report.alerts.push("reconciliation_mismatch");
                Err(ShellError::ReconciliationMismatch)
            }
        }
    }

    /// One reconciliation over `requests` (trading spec §11): the snapshot gathered through the
    /// run's connector, its account report stepped and performed first, then the reconciler's
    /// effects performed, gathering any reconciliation they ask for rather than sending its reads
    /// one by one. Answers the verdict and the requests of the reconciliation asked for next.
    fn reconcile_once(
        &mut self,
        requests: &[BrokerRequest],
    ) -> Result<(ReconciliationVerdict, Vec<BrokerRequest>), ShellError> {
        let snapshot = self
            .stages
            .reconciler
            .snapshot(&mut *self.stages.connector, requests)
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
        let next = self.prepare_reconciliation(reconciled.effects)?;
        Ok((reconciled.verdict, next))
    }

    /// Reconciles while a reconciliation asks for another, as a `LateFillApplied` it ingested
    /// does (trading spec §5.7, the executor's reconciliation step 2): each is gathered and run
    /// as the first was (DEC-858 item 3, DEC-938 item 5), never stepped answer by answer. A fill
    /// is ingested once, so the reconciliation after a late fill asks for none; one still asking
    /// after [`RECONCILIATION_ROUNDS`] rounds stops the run at Reconcile, so a broker that keeps
    /// reporting new fills can never hold the run in a loop. A round that mismatches stops the
    /// run there ([`Session::stop_on`]), so a round it asked for never runs.
    fn settle(&mut self, mut requests: Vec<BrokerRequest>) -> Result<(), ShellError> {
        let mut rounds = 0_usize;
        while !requests.is_empty() {
            if rounds >= RECONCILIATION_ROUNDS {
                return Err(refused(Stage::Reconcile)(Cause::Absent {
                    what: "a reconciliation that settles",
                }));
            }
            let (verdict, next) = self.reconcile_once(&requests)?;
            rounds = rounds.saturating_add(1);
            self.stop_on(verdict)?;
            requests = next;
        }
        Ok(())
    }

    /// E1b's watch over the run's one risk-adding submission (DEC-858 items 3 to 5). Each wake
    /// pauses, moves the event time to the clock, and reads the entry back ([`entry_read`]: by its client order
    /// id, or by the profile's listing where it cannot query by it, DEC-1150 item 3), then ticks the
    /// executor; the first wake at or past the bound hands the executor `CancelOpenings` in place
    /// of the read. Every wake asks the run's LT-10 question ([`watch_may_end`], DEC-912 item 3),
    /// in place of the bare terminal read the watch kept before E7-26 (DEC-858 item 4, DEC-914
    /// item 7): the run may end only when the entry is terminal in DEC-858 item 4's four and any
    /// filled quantity is covered, or the unprotected interval's own alert key is in the run's
    /// report. A run that submitted nothing watches nothing.
    ///
    /// The cap (DEC-877 item 1): the first wake at or past the end of the bound's regular session
    /// stops the run with [`ShellError::CancelUnconfirmed`] before any read there, so nothing is
    /// sent at or after the session's end. The entry's last state stays the executor's journaled
    /// `pending_cancel`, for the next start's reconciliation.
    fn watch<P: Pause>(
        &mut self,
        instrument: &InstrumentId,
        watch: &Watch<P>,
        end: MayEnd<'_>,
    ) -> Result<(), ShellError> {
        let Some(entry) = self.opening.clone() else {
            return Ok(());
        };
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|_| {
                refused(Stage::Executor)(Cause::Absent {
                    what: "a runtime for the watch",
                })
            })?;
        let agent = mandate_executor::AgentId(self.setup.deployment.agent.0.clone());
        let mut cancel_handed = false;
        let mut read: Option<String> = None;
        let queryable = match self.stages.connector.profile() {
            Ok(profile) => profile.idempotency().query_by_client_order_id,
            Err(..) => true,
        };
        loop {
            runtime.block_on(watch.pause.pause(watch.interval));
            let now = watch.pause.now();
            if self.lt10_holds(instrument, read.as_deref(), now, watch.bound, end)? {
                return Ok(());
            }
            self.event_time = now;
            if !cancel_handed && now >= watch.bound {
                cancel_handed = true;
                self.step_watched(mandate_executor::Input::Command(
                    mandate_executor::Command::CancelOpenings {
                        agent: agent.clone(),
                        instrument: instrument.clone(),
                    },
                ))?;
                continue;
            }
            let request = entry_read(
                queryable,
                &entry,
                instrument,
                mandate_executor::RiskClock::from_secs(self.setup.now.secs()),
            );
            let answer = self.stages.connector.call(&request);
            read = entry_status(&answer, &entry);
            self.step_watched(map::broker_input(answer).map_err(refused(Stage::Connector))?)?;
            self.step_watched(mandate_executor::Input::Tick(
                mandate_executor::RiskClock::from_secs(now.secs()),
            ))?;
            if self.lt10_holds(instrument, read.as_deref(), now, watch.bound, end)? {
                return Ok(());
            }
        }
    }

    /// One wake's LT-10 question (DEC-912 item 3): may the run end at `now`? The facts are the
    /// wake's own — the entry's last broker read, the executor's protection answer for the
    /// instrument (`crate::stages::Executor::protected`, never a guess at covered) and the alert
    /// keys the run's own report holds — and the answer is [`watch_may_end`]'s: the run ends only
    /// on a settled LT-10, reads on inside its bounds, and past the bound's session end fails
    /// closed with the shell's `cancel_unconfirmed` refusal, never reading again.
    fn lt10_holds(
        &mut self,
        instrument: &InstrumentId,
        read: Option<&str>,
        now: UtcNanos,
        bound: UtcNanos,
        end: MayEnd<'_>,
    ) -> Result<bool, ShellError> {
        let covered = self
            .stages
            .executor
            .protected(instrument)
            .map_err(refused(Stage::Executor))?;
        end(read, !covered, &self.report.alerts, now, bound)
    }

    /// One executor step during the watch and everything it asks for. A reconciliation it asks
    /// for is gathered and run through the reconciler, as at the start, never stepped answer by
    /// answer (DEC-858 item 3). A mismatch there stops the run as the startup's does.
    fn step_watched(&mut self, input: mandate_executor::Input) -> Result<(), ShellError> {
        let effects = self
            .stages
            .executor
            .step(input)
            .map_err(refused(Stage::Executor))?;
        let requests = self.prepare_reconciliation(effects)?;
        self.settle(requests)
    }

    /// Runs `effects` and everything they lead to, gathering the reconciliation reads they ask
    /// for rather than sending them one by one: a request the executor asks for is sent and its
    /// answer stepped, and the effects that answer leads to join the queue, so a reconciliation
    /// asked for by an answer is gathered too (DEC-877 item 2). Journal drafts are appended in
    /// order.
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
                        if let Some(answer) = self.broker(&request)? {
                            let more = self
                                .stages
                                .executor
                                .step(answer)
                                .map_err(refused(Stage::Executor))?;
                            effects.extend(more);
                        }
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

    /// [`Session::feed`] during an asked cycle's wait (DEC-940 item 3): the classifier's `ask`
    /// is the request's own classification — re-validation re-classifies the bound order, and
    /// an `ask` by the same trigger is what the grant answered — so it stops nothing here; the
    /// runtime's own `ApprovalRevalidated` decides, and the runner's fold reads it. Every other
    /// refusal stops the wait.
    fn feed_asked(&mut self, input: Input) -> Result<(), ShellError> {
        match self.feed(input) {
            Err(refusal) if asked(&refusal) => Ok(()),
            fed => fed,
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
            view: match &self.current {
                Some(current) => current,
                None => self.view,
            },
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

    /// Step 13. A submission goes out only when its `OrderSubmitted` committed in this run, and
    /// only when the operator asked for an order. One that adds risk goes out once per run; a
    /// protective order or an exit always goes out (`AGENTS.md` rule 13, DEC-1151 item 5).
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
            match door(
                self.holding,
                self.held.is_some(),
                self.opening.is_some(),
                order.purpose,
            ) {
                Door::Second => return Err(ShellError::SecondSubmission),
                Door::Hold => {
                    self.held = Some(order.clone());
                    return Ok(None);
                }
                Door::Pass => {}
            }
            if order.purpose.adds_risk() {
                if self.opening.is_some() {
                    return Err(ShellError::SecondSubmission);
                }
                self.opening = Some(order.client_order_id.clone());
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
    /// stays at version 1 with `mandate_version` alone, except an `ApprovalRequested` whose
    /// content binds the builder's plan, which is version 2 ([`planned_request_version`]).
    fn agent_refs(&self, draft: &EventDraft) -> Result<(u64, Object), ShellError> {
        let event_type = draft.event_type.as_str();
        let mut refs = vec![("mandate_version", Value::Str(self.view.version.clone()))];
        let reference = |digest: Digest| Value::Str(format!("sha256:{digest}"));
        let mut version = planned_request_version(draft);
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
                let (schema_version, config_refs) = self.agent_refs(draft)?;
                draft_bytes(
                    &Envelope {
                        stream: &stream,
                        writer: Writer::Agent,
                        actor_id: &self.setup.deployment.agent.0,
                        event_time: self.event_time,
                        environment: journal_environment(self.setup.connection_environment),
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
                        event_time: self.event_time,
                        environment: journal_environment(self.setup.connection_environment),
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
                    payload: with_environment(row, payload)?,
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

/// The watch's read of its entry (DEC-1150 item 3): by its client order id where the connector's
/// profile can query by it, and otherwise by the profile's own listing of the agent's orders in the
/// instrument created since `since` less DEC-862 item 2's margin, so the watch never sends a query
/// the connector refuses unsent.
fn entry_read(
    queryable: bool,
    entry: &mandate_executor::ClientOrderId,
    instrument: &InstrumentId,
    since: mandate_executor::RiskClock,
) -> BrokerRequest {
    if queryable {
        return BrokerRequest::GetOrderByClientId(entry.clone());
    }
    BrokerRequest::ListOrders(mandate_executor::OrderListing {
        client_order_id: entry.clone(),
        instrument: instrument.clone(),
        origin: mandate_executor::OrderOrigin::Agentic,
        created_since: mandate_executor::RiskClock::from_secs(
            since.secs().saturating_sub(WATCH_LISTING_MARGIN_S),
        ),
    })
}

/// DEC-862 item 2's margin: a listing starts this long before the order could have been placed.
const WATCH_LISTING_MARGIN_S: i64 = 300;

/// The entry's broker status in `answer`, which the run's LT-10 question reads (DEC-912 item 3):
/// the order an order query answered, or, from a listing, the one record that names the entry by
/// its own id. A listing with none or several such records reads nothing, so the watch never
/// takes another order's status for the entry's.
fn entry_status(
    answer: &Result<BrokerOutcome, mandate_executor::ConnectorError>,
    entry: &mandate_executor::ClientOrderId,
) -> Option<String> {
    match answer {
        Ok(BrokerOutcome::Order(order)) => Some(order.status.clone()),
        Ok(BrokerOutcome::Listed { orders, .. }) => {
            let mut named = orders
                .iter()
                .filter(|record| record.order.client_order_id.as_deref() == Some(entry.as_str()));
            match (named.next(), named.next()) {
                (Some(one), None) => Some(one.order.status.clone()),
                (Some(_), Some(_)) | (None, Some(_)) | (None, None) => None,
            }
        }
        Ok(BrokerOutcome::Submitted(..))
        | Ok(BrokerOutcome::DuplicateClientOrderId { .. })
        | Ok(BrokerOutcome::Rejected(..))
        | Ok(BrokerOutcome::Absent { .. })
        | Ok(BrokerOutcome::OpenOrders(..))
        | Ok(BrokerOutcome::Positions(..))
        | Ok(BrokerOutcome::Account(..))
        | Ok(BrokerOutcome::Activities { .. })
        | Ok(BrokerOutcome::CancelAccepted { .. })
        | Ok(BrokerOutcome::AccountWideAccepted) => None,
        Err(..) => None,
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

/// `ApprovalRequested`'s schema version for the content it carries: 2 when the content binds the
/// order builder's plan, which version 1's closed content refuses, and 1 otherwise (DEC-942
/// item 2). Every other agent-stream draft starts at version 1.
fn planned_request_version(draft: &EventDraft) -> u64 {
    let planned = draft.event_type == "ApprovalRequested"
        && draft
            .payload
            .get("content")
            .is_some_and(|content| content.get("execution").is_some());
    if planned { 2 } else { 1 }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use mandate_canon::{Digest, Key, Object, Value};
    use mandate_executor::{Effect, EventDraft, EventId, NotificationRef};
    use mandate_journal::StoredEvent;

    use mandate_accounting::{InstrumentId, Side};
    use mandate_executor::{
        BrokerOrder, BrokerOutcome, BrokerRequest, ClientOrderId, ListedOrder, OrderListing,
        OrderOrigin, OrderType, RiskClock, TimeInForce,
    };
    use mandate_num::Qty;

    use super::{
        consecutive_executor_journals, entry_read, entry_status, planned_request_version,
        with_environment,
    };

    fn entry() -> Result<ClientOrderId, String> {
        ClientOrderId::parse("md-01JABCDEFGHJKMNPQRSTVWXYZ0").map_err(|e| e.to_string())
    }

    fn instrument() -> Result<InstrumentId, String> {
        InstrumentId::new("5f0c3c1e-7d1a-4c8e-9b0a-2e6f1d3a9c41").map_err(|e| e.to_string())
    }

    /// DEC-1150 item 3: a profile that queries by client order id is asked by it; one that cannot
    /// is asked by its listing of the agent's orders in the instrument from five minutes before
    /// the run, the margin DEC-862 item 2 sets.
    #[test]
    fn the_watch_reads_its_entry_the_way_the_profile_offers() -> Result<(), String> {
        let (entry, instrument) = (entry()?, instrument()?);
        let since = RiskClock::from_secs(1_000);
        assert_eq!(
            entry_read(true, &entry, &instrument, since),
            BrokerRequest::GetOrderByClientId(entry.clone())
        );
        assert_eq!(
            entry_read(false, &entry, &instrument, since),
            BrokerRequest::ListOrders(OrderListing {
                client_order_id: entry.clone(),
                instrument,
                origin: OrderOrigin::Agentic,
                created_since: RiskClock::from_secs(700),
            })
        );
        Ok(())
    }

    fn record(client_order_id: Option<&str>, status: &str) -> Result<ListedOrder, String> {
        Ok(ListedOrder {
            order: BrokerOrder {
                broker_order_id: "rh-sim-000001".to_owned(),
                client_order_id: client_order_id.map(str::to_owned),
                instrument: instrument()?,
                side: Side::Buy,
                qty: Qty::parse("1").map_err(|e| e.to_string())?,
                filled_qty: Qty::ZERO,
                limit_price: None,
                stop_price: None,
                status: status.to_owned(),
                reject_code: None,
                replaced_by_broker_order_id: None,
                legs: Vec::new(),
                created_on: None,
            },
            order_type: OrderType::Limit,
            tif: TimeInForce::Day,
        })
    }

    /// The status the LT-10 question reads comes only from the one listed record naming the
    /// entry by its own id: never from a look-alike, another order, or a listing naming it twice.
    #[test]
    fn the_watch_reads_the_status_of_the_one_record_naming_its_entry() -> Result<(), String> {
        let entry = entry()?;
        let listed = |orders: Vec<ListedOrder>| {
            Ok(BrokerOutcome::Listed {
                client_order_id: entry.as_str().to_owned(),
                orders,
            })
        };
        let ours = entry.as_str();
        let answer = listed(vec![
            record(None, "canceled")?,
            record(Some("ref_id someone-else"), "canceled")?,
            record(Some(ours), "filled")?,
        ]);
        assert_eq!(entry_status(&answer, &entry), Some("filled".to_owned()));
        let twice = listed(vec![
            record(Some(ours), "filled")?,
            record(Some(ours), "filled")?,
        ]);
        assert_eq!(entry_status(&twice, &entry), None);
        let none = listed(vec![record(None, "filled")?]);
        assert_eq!(entry_status(&none, &entry), None);
        let order = Ok(BrokerOutcome::Order(record(Some(ours), "accepted")?.order));
        assert_eq!(entry_status(&order, &entry), Some("accepted".to_owned()));
        Ok(())
    }

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

    /// DEC-942 item 2: only an `ApprovalRequested` whose content binds a plan is version 2; a
    /// request without one, and any other record carrying the same member, stay at version 1.
    #[test]
    fn only_a_request_that_binds_a_plan_is_version_two() -> Result<(), String> {
        let with_plan = mandate_canon::parse(br#"{"content":{"execution":null}}"#)
            .map_err(|e| format!("{e:?}"))?;
        let without = mandate_canon::parse(br#"{"content":{}}"#).map_err(|e| format!("{e:?}"))?;
        let agent = |event_type: &str, payload: &Value| mandate_runtime::EventDraft {
            event_id: mandate_runtime::EventId("00000100000000000000000001".to_owned()),
            event_type: event_type.to_owned(),
            causation_id: None,
            payload: payload.clone(),
        };
        assert_eq!(
            planned_request_version(&agent("ApprovalRequested", &with_plan)),
            2
        );
        assert_eq!(
            planned_request_version(&agent("ApprovalRequested", &without)),
            1
        );
        assert_eq!(
            planned_request_version(&agent("ApprovalDelivered", &with_plan)),
            1
        );
        assert_eq!(
            planned_request_version(&agent("ApprovalRequested", &Value::Null)),
            1
        );
        Ok(())
    }

    fn stored(event_type: &str) -> StoredEvent {
        StoredEvent {
            stream_id: "acct:ws:acct".to_owned(),
            seq: 1,
            event_id: "00000900000000000000000001".to_owned(),
            event_type: event_type.to_owned(),
            schema_version: 1,
            environment: "paper".to_owned(),
            recorded_at: "2026-10-16T00:00:00.000000000Z".to_owned(),
            prev_hash: Digest::of(b""),
            hash: Digest::of(b""),
            body: Vec::new(),
        }
    }

    fn payload(members: &[(&str, &str)]) -> Result<Value, String> {
        let mut object = Object::new();
        for (name, value) in members {
            let key = Key::new(name).map_err(|_| format!("the member name {name}"))?;
            object.insert(key, Value::Str((*value).to_owned()));
        }
        Ok(Value::Object(object))
    }

    fn handed(event_type: &str, payload: Value) -> Result<Value, String> {
        with_environment(&stored(event_type), payload).map_err(|error| error.to_string())
    }

    /// DEC-948 item 2: the executor is handed the envelope's environment on a `StreamOpened`
    /// whose payload names none; a payload's own environment, and every other record, are handed
    /// on exactly as stored.
    #[test]
    fn only_a_stream_opened_without_one_takes_the_envelopes_environment() -> Result<(), String> {
        assert_eq!(
            handed("StreamOpened", payload(&[("stream_type", "account")])?)?,
            payload(&[("environment", "paper"), ("stream_type", "account")])?
        );
        let named = payload(&[("environment", "live"), ("stream_type", "account")])?;
        assert_eq!(
            handed("StreamOpened", named.clone())?,
            named,
            "a payload's own environment is never replaced, so the fold still refuses a mismatch"
        );
        let other = payload(&[("result", "clean")])?;
        assert_eq!(handed("ReconciliationRun", other.clone())?, other);
        Ok(())
    }
}

/// The actor every event the tracer's own fold replays is read as: the agent and account streams
/// it appends and rewinds, where no response is admitted from anyone; `system` is what admission
/// never admits (EI-10), so nothing the tracer folds by hand can pose as an owner. The control
/// stream is not folded through this actor: `Session::tail_the_control_stream` tails it through
/// [`crate::tail::tailed`], whose fold maps the envelope's own `actor.kind` (M7 brief, Decisions
/// needed 5).
const TRACER_ACTOR: ActorKind = ActorKind::System;

/// A [`Mark`]'s `source`: a quote, as the journal's own vectors name one.
const MARK_SOURCE: &str = "quote";

/// A [`Mark`]'s `feed`: the IEX feed the run's quotes are read from (trading-domain spec §4).
const MARK_FEED: &str = "iex";

/// The most reconciliations one [`Session::settle`] runs: the one asked for, and the one a late
/// fill it ingested asks for (DEC-938 item 5).
const RECONCILIATION_ROUNDS: usize = 2;

/// The executor reads its stream's environment from `StreamOpened`'s `environment` member, which
/// journal spec §9's registered schema carries in the envelope instead (ADR-0001 ES-23). The
/// shell hands the executor the environment the journal stored with the record, so a paper
/// stream books paper's simulated fees (trading spec §10, DEC-948 item 2); every other record,
/// and a payload that names its own, is handed on as stored.
fn with_environment(row: &StoredEvent, payload: Value) -> Result<Value, ShellError> {
    const MEMBER: &str = "environment";
    let Value::Object(mut members) = payload else {
        return Ok(payload);
    };
    if row.event_type == "StreamOpened" && !members.contains_key(MEMBER) {
        let key = Key::new(MEMBER).map_err(|_| ShellError::Envelope { field: MEMBER })?;
        members.insert(key, Value::Str(row.environment.clone()));
    }
    Ok(Value::Object(members))
}

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

    /// Mandate spec §6.2's classification of a proposal the classifier answered. A classifier that
    /// cannot answer leaves no classification and so no decider, and a `DecisionMade` that the dry
    /// run allowed names its decider (journal spec §9.1 rule 5) from a closed set with no label
    /// for that: like [`FlattenPlanner::plan`]'s, the refusal poisons the step, which
    /// [`Session::step`] reads before it performs a single effect, so nothing of the step is
    /// journaled or sent and the run stops at Classify (DEC-938 item 4). The fail-safe `deny`
    /// handed back is never acted on.
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Classified {
        let answer = self.classifier.classify(view, proposal);
        let (classified, cause) = match answer {
            Err(cause) => {
                self.findings.borrow_mut().poison = Some(ShellError::Refused {
                    stage: Stage::Classify,
                    cause,
                });
                return Classified {
                    autonomy: Autonomy::Deny,
                    decided_by: None,
                };
            }
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
