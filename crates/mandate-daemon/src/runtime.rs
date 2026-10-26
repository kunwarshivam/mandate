//! One agent deployment's runtime as a long-running process (E21-30 slice 1, DEC-1012).

mod research;

pub use research::{ArtifactSink, ResearchParts, ResearchSource, ResearchWiring, ResearchWorker};

use std::time::Duration;

use mandate_canon::{Object, parse};

use crate::production::Facts;
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_process::{Input as Lifecycled, Kind, Lifecycle, State};
use mandate_runtime::{
    Deployment, Effect, EventDraft, FlattenPlanner, GateDryRun, IdGen, Input, MandateView,
    OrderPlan, Ports, RiskClock, RuntimeState, Seq, WriterEpoch, fold, handle, tailed,
};
use mandate_store::Store;
use mandate_store::envelope::{Envelope, Fields, draft_bytes, object, text};
use mandate_time::UtcNanos;

/// The version the envelope's actor carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The schema version and configuration references a draft is journaled with: the deployment's
/// registered configuration, which only the production wiring knows. `None` refuses the draft, and
/// the process stops rather than journal a record without the references it needs.
pub trait Refs {
    fn refs(&self, draft: &EventDraft) -> Option<(u64, Object)>;
}

/// The runtime core's ports for one deployment (`mandate_runtime::Ports`), with [`Refs`].
pub struct Wiring {
    pub ids: Box<dyn IdGen>,
    pub gate: Box<dyn GateDryRun>,
    pub plan: Box<dyn OrderPlan>,
    pub flatten: Box<dyn FlattenPlanner>,
    pub refs: Box<dyn Refs>,
    pub view: MandateView,
    /// The deployment's research (DEC-2208); `None`, the default, runs none.
    pub research: Option<ResearchWiring>,
    /// The fold the production ports read (DEC-2378 S2), fed every row the process reads from the
    /// control, account and clock streams; `None` for the fixture wiring, which reads nothing.
    pub facts: Option<Facts>,
}

impl Wiring {
    fn ports(&self) -> Ports<'_> {
        Ports {
            ids: self.ids.as_ref(),
            gate: self.gate.as_ref(),
            plan: self.plan.as_ref(),
            flatten: self.flatten.as_ref(),
            view: &self.view,
        }
    }
}

/// What a runtime process runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub deployment: Deployment,
    pub environment: Environment,
    pub liveness: Duration,
}

/// An owner alert the core asked for, after the draft it names committed: a fixed key and an opaque
/// event id, nothing else (`AGENTS.md` rule 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub key: &'static str,
    pub subject: Option<String>,
}

/// The operator alert a pass raises when the production flatten left a held instrument unplanned
/// (DEC-2378 item 8): a fixed key, nothing about the instrument (`AGENTS.md` rule 6).
pub const FLATTEN_UNPLANNED: &str = "flatten_unplanned";

/// How one input's effects ended.
enum Ran {
    Committed,
    InDoubt,
    Stopped,
}

/// How a read of the account stream ended.
enum Account {
    Folded,
    Unreadable,
    Unfoldable,
}

/// One agent deployment's runtime process.
pub struct RuntimeProcess<S> {
    store: S,
    wiring: Wiring,
    state: RuntimeState,
    lifecycle: Lifecycle,
    environment: Environment,
    agent: Option<StreamId>,
    control: Option<StreamId>,
    clock: Option<StreamId>,
    /// The account stream the deployment's connection writes through, once the control stream's
    /// `ConnectionEstablished` names it (DEC-1014). It never changes while the process runs.
    account: Option<StreamId>,
    epoch: u64,
    /// The control stream's head when it was last handed whole while ready.
    control_handed: Option<u64>,
    /// The input whose drafts' append is in doubt, with the time they were first written at: the
    /// core takes no other input until it is handed again, and the retry offers the journal the same
    /// bytes (journal spec §5.1).
    pending: Option<(Input, UtcNanos)>,
    /// The event time the drafts being written carry.
    written_at: UtcNanos,
    /// The deployment's research, when its wiring has one (DEC-2208).
    research: Option<research::Research>,
}

impl<S: Store> RuntimeProcess<S> {
    /// Starts at `now`: refuses any environment but paper before taking anything, takes a newer
    /// writer epoch on the agent stream (fencing every earlier process), folds the stream, and hands
    /// the core `Input::Started`. Only once that batch commits is the process ready.
    pub fn start(store: S, mut wiring: Wiring, config: &Config, now: UtcNanos) -> Self {
        let research = wiring.research.take().map(research::Research::new);
        let deployment = &config.deployment;
        let stream = |kind: &str, rest: &str| StreamId::parse(&format!("{kind}:{rest}"));
        let workspace = deployment.workspace.0.as_str();
        let mut this = Self {
            store,
            wiring,
            state: RuntimeState::new(deployment.clone()),
            lifecycle: Lifecycle::new(Kind::Runtime, config.liveness, now),
            environment: config.environment,
            agent: stream("agent", &format!("{workspace}:{}", deployment.agent.0)),
            control: stream("ctl", workspace),
            clock: stream("clock", workspace),
            account: None,
            epoch: 0,
            control_handed: None,
            pending: None,
            written_at: now,
            research,
        };
        if config.environment != Environment::Paper {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        }
        this.lifecycle.observe(now, Lifecycled::StartupPassed);
        let Some(agent) = this.agent.clone() else {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        };
        let Some(epoch) = this.store.take_ownership(&agent) else {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        };
        this.epoch = epoch;
        this.lifecycle.observe(now, Lifecycled::EpochTaken);
        let replayed = this
            .store
            .rows(&agent)
            .and_then(|rows| this.fold_rows(&rows));
        let Some(head) = replayed else {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        };
        this.lifecycle.observe(
            now,
            Lifecycled::Replayed {
                position: head,
                head,
            },
        );
        if head == 0 {
            match this.open(now) {
                Ran::Committed => {}
                Ran::InDoubt => {
                    this.lifecycle.observe(now, Lifecycled::Failed);
                    return this;
                }
                Ran::Stopped => return this,
            }
        }
        if !this.replay_account() {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        }
        this.hand(
            now,
            now,
            Input::Started(WriterEpoch(epoch)),
            &mut Vec::new(),
        );
        this
    }

    /// Folds the account stream before `Input::Started`, as the shell's open replays it, so the
    /// startup hold is taken on the whole fold (DEC-1014 item 2). Whether the control stream, and
    /// the account stream it names if any, were read and folded whole.
    fn replay_account(&mut self) -> bool {
        let Some(control) = self.control.as_ref().and_then(|c| self.store.rows(c)) else {
            return false;
        };
        self.account = account_of(&control, self.state.deployment(), self.environment);
        if let (Some(facts), Some(account)) = (&self.wiring.facts, &self.account) {
            facts.follow(account.as_str());
        }
        self.feed(&control);
        match self.account.clone() {
            Some(account) => matches!(self.fold_account(&account), Account::Folded),
            None => true,
        }
    }

    /// Journal spec §2's stream lifecycle: the process that owns a new agent stream writes its
    /// `StreamOpened` at sequence 1 before the core receives `Input::Started` (§9.1).
    fn open(&mut self, now: UtcNanos) -> Ran {
        let deployment = self.state.deployment().clone();
        let payload = object(vec![
            ("agent_id", text(&deployment.agent.0)),
            ("stream_type", text("agent")),
            ("workspace_id", text(&deployment.workspace.0)),
        ]);
        let Some(payload) = payload else {
            self.lifecycle.observe(now, Lifecycled::Failed);
            return Ran::Stopped;
        };
        let opened = EventDraft {
            event_id: self.wiring.ids.event_id(WriterEpoch(self.epoch), Seq(0), 0),
            event_type: "StreamOpened".to_owned(),
            causation_id: None,
            payload,
        };
        self.written_at = now;
        self.journal_with(now, &[opened], |_| Some((1, Object::new())))
    }

    /// One pass of the loop at the injected time `now`: an input in doubt first, then the control
    /// stream, then the account stream, then the clock stream's latest tick. Returns the alerts
    /// whose drafts committed.
    pub fn step(&mut self, now: UtcNanos) -> Vec<Alert> {
        let mut alerts = Vec::new();
        if self.exited() {
            return alerts;
        }
        self.lifecycle.observe(now, Lifecycled::Heartbeat);
        if let Some((input, at)) = self.pending.take()
            && !self.hand(now, at, input, &mut alerts)
        {
            return alerts;
        }
        if !self.research_retry(now, &mut alerts) || self.exited() {
            return alerts;
        }
        if let Some(control) = self.control_pass(now, &mut alerts)
            && self.account_pass(now, &control)
            && self.clock_pass(now, &mut alerts)
        {
            self.research_pass(now, &mut alerts);
        }
        if self
            .wiring
            .facts
            .as_ref()
            .is_some_and(|facts| facts.take_unplanned() > 0)
        {
            alerts.push(Alert {
                key: FLATTEN_UNPLANNED,
                subject: None,
            });
        }
        alerts
    }

    /// Drains at `now`: no new input; an input in doubt is handed once more, then the process exits
    /// `Drained`, or `DrainIncomplete` if it is still in doubt.
    pub fn drain(&mut self, now: UtcNanos) {
        if self.exited() {
            return;
        }
        self.lifecycle.observe(now, Lifecycled::DrainRequested);
        let settled = match self.pending.take() {
            Some((input, at)) => self.hand(now, at, input, &mut Vec::new()),
            None => true,
        };
        let last = if settled {
            Lifecycled::Settled
        } else {
            Lifecycled::Failed
        };
        self.lifecycle.observe(now, last);
    }

    pub fn lifecycle(&self) -> &Lifecycle {
        &self.lifecycle
    }

    pub fn state(&self) -> &RuntimeState {
        &self.state
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    fn exited(&self) -> bool {
        matches!(self.lifecycle.state(), State::Exited(_))
    }

    /// The control stream, handed whole each time it has grown (DEC-914 item 2: the core's own
    /// copy fold makes a row handed again inert). It is read before anything else in a pass, so
    /// the owner's kill switch is never queued behind a tick (ES-06). The rows read, if the pass
    /// ended with nothing in doubt.
    fn control_pass(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>) -> Option<Vec<StoredEvent>> {
        let control = self.control.clone()?;
        let rows = self.store.rows(&control)?;
        self.feed(&rows);
        let events = tailed(&rows).ok()?;
        let head = events.last().map_or(0, |e| e.seq.0);
        if self.control_handed == Some(head) {
            return Some(rows);
        }
        for event in events {
            if !self.hand(now, now, Input::Journal(event), alerts) {
                return None;
            }
        }
        self.control_handed = Some(head);
        Some(rows)
    }

    /// The account stream's rows past the core's head, folded into the core as the shell's copy
    /// fold folds them, so a clean `ReconciliationRun` lifts the startup hold and the executor's
    /// `AgentModeApplied` reaches the runtime on the next input (DEC-1014 item 1). The account is
    /// the one the control stream's `control` rows name; a process that follows one account and
    /// is told of another stops, and the next process follows the new one from its start. Whether
    /// the clock may be read: an account that cannot be read folds nothing this pass, and the tick
    /// is still handed, so no risk exit or deadline waits on it (`AGENTS.md` rule 13); an opening
    /// decided on a mode that may be stale still meets the executor's gate (rule 1).
    fn account_pass(&mut self, now: UtcNanos, control: &[StoredEvent]) -> bool {
        let named = account_of(control, self.state.deployment(), self.environment);
        match (&self.account, named) {
            (Some(followed), Some(named)) if *followed != named => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                return false;
            }
            (None, named) => self.account = named,
            _ => {}
        }
        let Some(account) = self.account.clone() else {
            return true;
        };
        match self.fold_account(&account) {
            Account::Folded | Account::Unreadable => true,
            Account::Unfoldable => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                false
            }
        }
    }

    /// Folds the rows of `account` past the core's head for it. A row the core cannot fold, or one
    /// journaled for another environment, is never skipped (DEC-85, `AGENTS.md` rule 8).
    fn fold_account(&mut self, account: &StreamId) -> Account {
        let Some(rows) = self.store.rows(account) else {
            return Account::Unreadable;
        };
        if let Some(facts) = &self.wiring.facts {
            facts.follow(account.as_str());
        }
        self.feed(&rows);
        let head = self.state.head(account.as_str()).map_or(0, |seq| seq.0);
        let fresh: Vec<StoredEvent> = rows.into_iter().filter(|r| r.seq > head).collect();
        if fresh
            .iter()
            .any(|r| r.environment != self.environment.as_str())
        {
            return Account::Unfoldable;
        }
        let Ok(events) = tailed(&fresh) else {
            return Account::Unfoldable;
        };
        for event in events {
            if fold(&mut self.state, &event).is_err() {
                return Account::Unfoldable;
            }
        }
        Account::Folded
    }

    /// The scheduler's latest tick, as the core's risk clock. It is handed on every pass: the core
    /// evaluates each risk-clock second once and takes a tick it has seen as no input at all, so a
    /// repeat costs nothing and no bookkeeping of what was handed can drift from the journal. A clock
    /// stream that cannot be read hands no tick and holds nothing else. Whether the pass ended with
    /// a readable clock and nothing in doubt, so the research pass may follow.
    fn clock_pass(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>) -> bool {
        let Some(clock) = self.clock.clone() else {
            return false;
        };
        let Some(rows) = self.store.rows(&clock) else {
            return false;
        };
        self.feed(&rows);
        let latest = rows
            .iter()
            .filter(|r| r.event_type == "ClockAdvanced")
            .filter_map(tick_of)
            .max();
        match latest {
            Some(tick) => self.hand(
                now,
                now,
                Input::Tick(RiskClock::from_secs(tick.secs())),
                alerts,
            ),
            None => true,
        }
    }

    /// Feeds `rows` to the production ports' fold, which takes each row once (DEC-2378 S2).
    fn feed(&self, rows: &[StoredEvent]) {
        if let Some(facts) = &self.wiring.facts {
            facts.feed(rows);
        }
    }

    /// Hands `input` to the core and runs its effects; whether they all committed. An input whose
    /// append is in doubt is kept to be handed again; a refusal or a fence stops the process.
    fn hand(&mut self, now: UtcNanos, at: UtcNanos, input: Input, alerts: &mut Vec<Alert>) -> bool {
        self.written_at = at;
        let started = matches!(input, Input::Started(_));
        let effects = match handle(&mut self.state, input.clone(), &self.wiring.ports()) {
            Ok(effects) => effects,
            Err(_) => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                return false;
            }
        };
        match self.run(now, effects, alerts) {
            Ran::Committed => {
                if started {
                    self.lifecycle
                        .observe(now, Lifecycled::Reconciled { passed: true });
                }
                true
            }
            Ran::InDoubt => {
                self.pending = Some((input, at));
                false
            }
            Ran::Stopped => false,
        }
    }

    /// Journals the effects' drafts as one batch on the agent stream, folds what committed, and
    /// only then reports their alerts. Intents and timers are not run here: the journal is the only
    /// channel to the executor (design §3.1), and a deadline is folded state.
    fn run(&mut self, now: UtcNanos, effects: Vec<Effect>, alerts: &mut Vec<Alert>) -> Ran {
        let mut drafts = Vec::new();
        let mut asked = Vec::new();
        for effect in effects {
            match effect {
                Effect::Journal(draft) => drafts.push(draft),
                Effect::Notify(notice) => asked.push(Alert {
                    key: notice.message_key,
                    subject: Some(notice.subject_event.0),
                }),
                Effect::NotifyApproval(_) => asked.push(Alert {
                    key: "approval_needed",
                    subject: None,
                }),
                Effect::Intent(_) | Effect::Timer(_) => {}
            }
        }
        if !drafts.is_empty() {
            match self.journal(now, &drafts) {
                Ran::Committed => {}
                other => return other,
            }
        }
        alerts.extend(asked);
        Ran::Committed
    }

    fn journal(&mut self, now: UtcNanos, drafts: &[EventDraft]) -> Ran {
        let refs = |draft: &EventDraft| self.wiring.refs.refs(draft);
        let refs: Option<Vec<(u64, Object)>> = drafts.iter().map(refs).collect();
        match refs {
            Some(refs) => {
                let mut given = refs.into_iter();
                self.journal_with(now, drafts, |_| given.next())
            }
            None => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                Ran::Stopped
            }
        }
    }

    /// Journals `drafts` as one batch, each with the schema version and references `refs` gives it.
    fn journal_with(
        &mut self,
        now: UtcNanos,
        drafts: &[EventDraft],
        mut refs: impl FnMut(&EventDraft) -> Option<(u64, Object)>,
    ) -> Ran {
        let Some(agent) = self.agent.clone() else {
            return Ran::Stopped;
        };
        let deployment = self.state.deployment();
        let envelope = Envelope {
            stream: agent.as_str(),
            environment: self.environment,
            actor_kind: "agent",
            actor_id: &deployment.agent.0,
            build: concat!("mandate-daemon/", env!("CARGO_PKG_VERSION")),
            version: VERSION,
            event_time: self.written_at,
        };
        let bytes: Option<Vec<Vec<u8>>> = drafts
            .iter()
            .map(|draft| {
                let (schema_version, config_refs) = refs(draft)?;
                draft_bytes(
                    &envelope,
                    &Fields {
                        event_id: &draft.event_id.0,
                        event_type: &draft.event_type,
                        schema_version,
                        causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
                        correlation_id: None,
                        config_refs: &config_refs,
                        payload: &draft.payload,
                    },
                )
            })
            .collect();
        let Some(bytes) = bytes else {
            self.lifecycle.observe(now, Lifecycled::Failed);
            return Ran::Stopped;
        };
        let head = self.state.head(agent.as_str()).map_or(0, |seq| seq.0);
        match self.store.append(&agent, head, self.epoch, now, &bytes) {
            AppendOutcome::Committed(stored) | AppendOutcome::AlreadyCommitted(stored) => {
                if self.fold_rows(&stored).is_some() {
                    Ran::Committed
                } else {
                    self.lifecycle.observe(now, Lifecycled::Failed);
                    Ran::Stopped
                }
            }
            AppendOutcome::Unavailable | AppendOutcome::Ambiguous => Ran::InDoubt,
            AppendOutcome::Fenced { .. } => {
                self.lifecycle.observe(now, Lifecycled::Fenced);
                Ran::Stopped
            }
            AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::IdempotencyConflict { .. }
            | AppendOutcome::Invalid { .. } => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                Ran::Stopped
            }
        }
    }

    /// Folds `rows` of the agent stream into the core; the stream's head after them, or `None` for
    /// a row the core cannot fold.
    fn fold_rows(&mut self, rows: &[StoredEvent]) -> Option<u64> {
        for event in tailed(rows).ok()? {
            fold(&mut self.state, &event).ok()?;
        }
        let agent = self.agent.as_ref()?;
        Some(self.state.head(agent.as_str()).map_or(0, |seq| seq.0))
    }
}

/// The account stream the latest version-2 `ConnectionEstablished` of the deployment's connection
/// in `environment` names on the control stream (journal spec §9.8: an account stream belongs to
/// the connection whose establishment names its `account_ref`). `None` until one does.
fn account_of(
    control: &[StoredEvent],
    deployment: &Deployment,
    environment: Environment,
) -> Option<StreamId> {
    let reference = control.iter().rev().find_map(|row| {
        if row.event_type != "ConnectionEstablished" || row.schema_version != 2 {
            return None;
        }
        let payload = parse(&row.body).ok()?.get("payload")?.clone();
        if payload.get("connection_id")?.as_str()? != deployment.connection.0
            || payload.get("environment")?.as_str()? != environment.as_str()
        {
            return None;
        }
        payload.get("account_ref")?.as_str().map(str::to_owned)
    })?;
    StreamId::parse(&format!("acct:{}:{reference}", deployment.workspace.0))
}

/// The tick a `ClockAdvanced` row carries (journal spec §9.20).
fn tick_of(row: &StoredEvent) -> Option<UtcNanos> {
    let body = parse(&row.body).ok()?;
    let clock = body.get("payload")?.get("clock")?.as_str()?;
    UtcNanos::parse(clock).ok()
}
