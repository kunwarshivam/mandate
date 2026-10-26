//! One broker account's executor as a long-running process (E21-30, DEC-2003, DEC-2033).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_canon::{Object, parse};
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountDeclaration, AccountScope, AcknowledgmentHandoff, ActivityCursor, AgentId,
    BindingGateSource, BrokerOrder, BrokerOutcome, BrokerRequest, BrokerSnapshot, BrokerUpdate,
    Command, ConnectorError, Effect, EventDraft, EventId, ExecutorConfig, ExecutorError,
    ExecutorState, IdGen, Initiator, Input, InstrumentSnapshot, IntentBody, IntentHandoff,
    IntentId, KillScope, MandateVersion, MandateView, OrderState, Ports, ProtectionPrices,
    ReconcileReason, RiskClock, Seq, ThesisRow, WriterEpoch, fold, handle, limit_fills,
    recorded_fills,
};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_num::{Fraction, Price};
use mandate_process::{Action, Input as Lifecycled, Kind, Lifecycle, State};
use mandate_store::Store;
use mandate_store::envelope::{Envelope, Fields, draft_bytes, object, text};
use mandate_time::{ExchangeCalendar, UtcNanos};

use crate::follow::{
    Proposal, Switch, Unreadable, agents_of, bounded, folded, proposals, switches, theses,
};
use crate::health::Health;
use crate::marks::{MARK_EVERY, Marked, mark_draft};
use crate::quotes::{QuoteFeed, QuotePoll};
use crate::robinhood::AccountReads;

/// The version the envelope's actor carries.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// How often, in seconds, a process with an order working asks for a reconciliation (X1b-3): four
/// broker reads, well inside Alpaca's limit of 200 requests a minute.
pub const SCHEDULED_RECONCILE_S: i64 = 10;

/// The broker as the process calls it: one request, one answer, in order. The connector is
/// `mandate_executor::BrokerConnector`; an adapter that owns its runtime drives it (X2).
pub trait Broker {
    /// The broker's capability profile, set on the state before `Input::Started` (DEC-838 item 5).
    fn profile(&self) -> Result<CapabilityProfile, ProfileError>;
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError>;
    /// The account in one read, for a broker that publishes no account activity (Robinhood,
    /// DEC-933 item 1, DEC-2386 item 4); `None` for a broker that answers the four reads a
    /// reconciliation asks for, as Alpaca does.
    fn account_reads(&mut self) -> Option<Result<AccountReads, ConnectorError>> {
        None
    }

    /// The account type and day-trading regime the broker's connector declares (trading spec
    /// §7.2, DEC-840), which each `AccountSnapshotRecorded` journals (journal spec §9.35,
    /// DEC-2398). `None`, the default, declares none: the snapshot journals `null`, and a reader
    /// holds what depends on it.
    fn declared(&self) -> Option<AccountDeclaration> {
        None
    }
}

/// The protections port could not answer. The process stops rather than hand an opening without
/// the protection its mandate asks for (TI-4); the restart asks again (DEC-2033 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unplanned;

/// The protective prices an opening is handed with: the mandate's protection for an entry at its
/// limit, as the order builder derives it in one process, or none when the mandate turns
/// protection off. `IntentProposed` does not journal them, so the handoff takes them from the
/// mandate the agent is deployed under (DEC-2033 item 3).
pub trait Protections {
    /// # Errors
    /// [`Unplanned`] when the mandate's protection cannot be read.
    fn protection(
        &self,
        agent: &AgentId,
        instrument: &InstrumentId,
        entry: Price,
    ) -> Result<Option<ProtectionPrices>, Unplanned>;
}

/// The executor core's ports for one account, with the protections an opening is handed with.
pub struct Wiring {
    pub ids: Box<dyn IdGen>,
    pub mandates: Box<dyn MandateView>,
    pub instruments: Box<dyn InstrumentSnapshot>,
    pub config: ExecutorConfig,
    pub fees: mandate_accounting::Config,
    pub gate: Box<dyn BindingGateSource>,
    pub protections: Box<dyn Protections>,
    /// The latest quote of each instrument whose exit waits on a price (E21-30, DEC-2359), and
    /// of each instrument marked.
    pub quotes: Box<dyn QuoteFeed>,
    /// The instruments the executor marks (DEC-2380 item 3).
    pub marked: Box<dyn Marked>,
}

impl Wiring {
    fn ports<'a>(&'a self, mandates: &'a Deployed<'a>) -> Ports<'a> {
        Ports {
            ids: self.ids.as_ref(),
            mandates,
            instruments: self.instruments.as_ref(),
            config: &self.config,
            fees: &self.fees,
        }
    }
}

/// The core's mandate port: a followed agent's version is the one its `AgentDeployed` names, so no
/// exit of a followed agent waits on the wiring's port; the rest is the wiring's (DEC-2034 item 1).
struct Deployed<'a> {
    versions: &'a BTreeMap<AgentId, MandateVersion>,
    wiring: &'a dyn MandateView,
}

impl MandateView for Deployed<'_> {
    fn version(&self, agent: &AgentId) -> Option<MandateVersion> {
        self.versions
            .get(agent)
            .cloned()
            .or_else(|| self.wiring.version(agent))
    }

    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<Fraction> {
        self.wiring.crypto_stop_limit_offset(agent)
    }

    fn covers(&self, agent: &AgentId, instrument: &InstrumentId) -> bool {
        self.wiring.covers(agent, instrument)
    }
}

/// What an executor process runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub scope: AccountScope,
    pub environment: Environment,
    pub liveness: Duration,
    /// How many broker answers deep one input may lead before the process stops, so a broker that
    /// keeps answering with work for the core can never hold the loop (DEC-2003 item 7).
    pub max_chain: usize,
}

/// An owner alert the core asked for, after the draft it names committed: a fixed key and an opaque
/// event id, nothing else (`AGENTS.md` rule 6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub key: &'static str,
    pub subject: Option<String>,
}

/// Why a reconciliation read gave no snapshot.
enum Read {
    /// The answer was lost on the way back: read again on a later pass.
    Lost,
    /// The broker answered something that is not the read's answer, or the request could not be
    /// sent.
    Unusable,
    /// An answer handed before the snapshot did not commit; the pass stops as a handoff does.
    Halted,
}

/// How one input's effects ended.
enum Ran {
    Committed,
    InDoubt,
    /// The journal refused the batch as invalid: nothing of it committed (DEC-2805).
    Refused,
    Stopped,
}

/// One broker account's executor process.
pub struct ExecutorProcess<S, B> {
    store: S,
    broker: B,
    wiring: Wiring,
    state: ExecutorState,
    lifecycle: Lifecycle,
    environment: Environment,
    account: Option<StreamId>,
    control: Option<StreamId>,
    clock: Option<StreamId>,
    epoch: u64,
    /// The input whose drafts' append is in doubt, with the time they were first written at: the
    /// core takes no other input until it is handed again, and the retry offers the journal the
    /// same bytes (journal spec §5.1). Nothing it asks of the broker leaves before it commits.
    pending: Option<(Input, UtcNanos)>,
    /// The mark whose append is in doubt, with the time it was first written at: offered to the
    /// journal again, the same bytes, before any other append, so a mark that committed with its
    /// answer lost is found already committed rather than conflicting with the next (DEC-2380
    /// item 3, journal spec §5.1).
    pending_mark: Option<(EventDraft, UtcNanos)>,
    /// Requests whose drafts committed and which have not been sent, in order: what an answer
    /// whose own batch went in doubt left behind (DEC-2003 item 7).
    outbox: VecDeque<BrokerRequest>,
    /// Whether the core asked for a reconciliation that has not been handed yet.
    reconcile: bool,
    /// When the last scheduled reconciliation was asked for (X1b-3): while an order works, the
    /// process reads the broker every [`SCHEDULED_RECONCILE_S`], so a fill the broker reports
    /// after the order's answer is booked without a restart. The start's own reconciliation
    /// counts as the first.
    scheduled: UtcNanos,
    /// The followed streams that did not read on their last read, each alerted once.
    unreadable: BTreeSet<String>,
    /// The handoffs the core refused for a missing binding-gate input, each alerted once by
    /// [`held`]; asked again every pass, and never a stop, so the exits behind them still go
    /// (`AGENTS.md` rule 13).
    held: BTreeSet<String>,
    /// Whether the last append was in doubt, so the hold it raises is alerted once per outage and
    /// never floods (E21-26, design §2.1 and §8.2, RB-07). Cleared by the next commit, so the
    /// following outage raises it again.
    journal_down: bool,
    /// Whether a refused batch was already alerted to the operator in this process (DEC-2805).
    journal_refused: bool,
    /// Whether a broker input the core could not apply has alerted the operator, once a process
    /// (DEC-2821).
    input_refused: bool,
    /// Each followed agent's deployed mandate version, from its `AgentDeployed`, which the core's
    /// mandate port answers with (DEC-2034 item 1).
    deployed: BTreeMap<AgentId, MandateVersion>,
    max_chain: usize,
    /// When each instrument whose exit waits on a price was last read, so each is read at most
    /// once per `exit_step_s` (DEC-2359).
    polled: QuotePoll,
    /// When each marked instrument was last marked, so each is marked at most once per
    /// [`MARK_EVERY`] (DEC-2380 item 3).
    marked_at: QuotePoll,
    /// The US-equities calendar a mark's session is read from; `None` when it cannot load, and then
    /// nothing is marked, so the gate holds (`AGENTS.md` rule 3).
    calendar: Option<ExchangeCalendar>,
    written_at: UtcNanos,
    /// The connection's health as this process's broker answers show it (DEC-2832): each pass
    /// tells the core of every trouble it holds, which restricts the account first (DEC-2826).
    health: Health,
}

/// What one pass reads from the agents' streams: the kill switches this stream has not applied,
/// the proposals the core does not hold yet, each with its agent, whether a row did not read, and
/// whether nothing the journal shows still wants the executor ([`Following::released`]).
struct Followed {
    switches: Vec<(AgentId, Switch)>,
    proposals: Vec<(AgentId, Proposal)>,
    /// Each followed agent's thesis records, in stream order, which the core copies onto the
    /// account stream as its working universe (DEC-2823).
    theses: Vec<(AgentId, Vec<ThesisRow>)>,
    unreadable: bool,
    released: bool,
    /// Whether every followed agent's stream parsed and read: one that did not may hold a switch,
    /// so it holds the exit (DEC-2347 item 3.3).
    all_read: bool,
    /// The owner's acknowledgments and requests the account stream holds no copy of yet, in the
    /// control stream's order (journal spec §9.28, DEC-2828, DEC-2830).
    acknowledgments: Vec<AcknowledgmentHandoff>,
}

impl<S: Store, B: Broker> ExecutorProcess<S, B> {
    /// Starts at `now`: refuses any environment but paper before taking anything, takes a newer
    /// writer epoch on the account stream (fencing every earlier process), folds it, and opens it
    /// if it is empty. `Input::Started` waits for the first pass that reads a tick, since every
    /// record it leads to carries the risk clock (mandate spec §5.2).
    pub fn start(store: S, broker: B, wiring: Wiring, config: &Config, now: UtcNanos) -> Self {
        let scope = &config.scope;
        let workspace = scope.workspace.0.as_str();
        let wiring_step = wiring.config.exit_step_s;
        let stream = |id: String| StreamId::parse(&id);
        let mut this = Self {
            store,
            broker,
            wiring,
            state: ExecutorState::new(scope.clone()),
            lifecycle: Lifecycle::new(Kind::Executor, config.liveness, now),
            environment: config.environment,
            account: stream(format!("acct:{workspace}:{}", scope.account.0)),
            control: stream(format!("ctl:{workspace}")),
            clock: stream(format!("clock:{workspace}")),
            epoch: 0,
            pending: None,
            pending_mark: None,
            outbox: VecDeque::new(),
            reconcile: false,
            scheduled: now,
            unreadable: BTreeSet::new(),
            held: BTreeSet::new(),
            journal_down: false,
            journal_refused: false,
            input_refused: false,
            deployed: BTreeMap::new(),
            max_chain: config.max_chain,
            written_at: now,
            health: Health::default(),
            polled: QuotePoll::new(Duration::from_secs(
                u64::try_from(wiring_step).unwrap_or_default(),
            )),
            marked_at: QuotePoll::new(MARK_EVERY),
            calendar: ExchangeCalendar::us_equities().ok(),
        };
        if config.environment != Environment::Paper {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        }
        let profile = this.broker.profile();
        let (Some(account), Ok(profile)) = (this.account.clone(), profile) else {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        };
        this.state = this.state.clone().with_profile(profile);
        this.lifecycle.observe(now, Lifecycled::StartupPassed);
        let Some(epoch) = this.store.take_ownership(&account) else {
            this.lifecycle.observe(now, Lifecycled::Failed);
            return this;
        };
        this.epoch = epoch;
        this.lifecycle.observe(now, Lifecycled::EpochTaken);
        let replayed = this
            .store
            .rows(&account)
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
        if head == 0 && !matches!(this.open(now), Ran::Committed) {
            this.lifecycle.observe(now, Lifecycled::Failed);
        }
        this
    }

    /// Journal spec §2's stream lifecycle: the process that owns a new account stream writes its
    /// `StreamOpened` at sequence 1 before the core receives `Input::Started`.
    fn open(&mut self, now: UtcNanos) -> Ran {
        let scope = self.state.scope().clone();
        let payload = object(vec![
            ("account_ref", text(&scope.account.0)),
            ("broker", text("alpaca")),
            ("stream_type", text("account")),
            ("workspace_id", text(&scope.workspace.0)),
        ]);
        let Some(payload) = payload else {
            return Ran::Stopped;
        };
        let opened = EventDraft {
            event_id: self.wiring.ids.event_id(WriterEpoch(self.epoch), Seq(0), 0),
            event_type: "StreamOpened".to_owned(),
            schema_version: 1,
            config_refs: Object::new(),
            causation_id: None,
            payload,
        };
        self.written_at = now;
        self.journal(now, &[opened])
    }

    /// One pass of the loop at the injected time `now`: an input in doubt first, then a mark in
    /// doubt, then requests left unsent and a due reconciliation, then (once started) every kill
    /// switch the account's agents' streams hold that the fold has not applied, then the latest
    /// tick, then every order they propose that the fold has not taken, an opening or an increase
    /// only while ready (DEC-2033 item 1), then the marks due at the tick, so a mark never holds
    /// the tick's exit ladder or an exit. Before the start,
    /// the pass hands the first tick it reads and `Input::Started`. Returns the alerts whose drafts
    /// committed.
    pub fn step(&mut self, now: UtcNanos) -> Vec<Alert> {
        let mut alerts = Vec::new();
        if self.exited() {
            return alerts;
        }
        self.lifecycle.observe(now, Lifecycled::Heartbeat);
        if let Some((input, at)) = self.pending.take()
            && !self.hand(now, at, input, &mut alerts, 0)
        {
            return alerts;
        }
        if let Some((draft, at)) = self.pending_mark.take()
            && !self.append_mark(now, at, draft, &mut alerts)
        {
            return alerts;
        }
        if !self.flush(now, &mut alerts, 0) {
            return alerts;
        }
        let tick = self.tick(&mut alerts);
        if !self.state.started() {
            let Some(tick) = tick else {
                return alerts;
            };
            if !self.hand(now, now, Input::Tick(tick), &mut alerts, 0) {
                return alerts;
            }
            let started = Input::Started(WriterEpoch(self.epoch));
            if !self.hand(now, now, started, &mut alerts, 0) {
                return alerts;
            }
        }
        let Some(Followed {
            switches,
            proposals: proposed,
            theses: researched,
            unreadable,
            released,
            all_read,
            acknowledgments,
        }) = self.switches(now, &mut alerts)
        else {
            return alerts;
        };
        let nothing_read = switches.is_empty() && proposed.is_empty();
        for (agent, switch) in switches {
            if !self.apply(now, agent, switch, &mut alerts) {
                return alerts;
            }
        }
        if unreadable {
            self.lifecycle.observe(now, Lifecycled::Failed);
            return alerts;
        }
        if !self.report_troubles(now, &mut alerts) {
            return alerts;
        }
        if !self.read_quotes(now, &mut alerts).unwrap_or(true) {
            return alerts;
        }
        if let Some(tick) = tick
            && !self.hand(now, now, Input::Tick(tick), &mut alerts, 0)
        {
            return alerts;
        }
        for handoff in acknowledgments {
            if !self.hand(now, now, Input::Acknowledgment(handoff), &mut alerts, 0) {
                return alerts;
            }
        }
        for (agent, proposal) in proposed {
            let action = if proposal.purpose.adds_risk() {
                Action::Open
            } else {
                Action::RiskExit
            };
            if !self.report_troubles(now, &mut alerts) {
                return alerts;
            }
            if self.lifecycle.permits(action) && !self.take(now, agent, proposal, &mut alerts) {
                return alerts;
            }
        }
        for (agent, records) in researched {
            if !self.hand(now, now, Input::Theses { agent, records }, &mut alerts, 0) {
                return alerts;
            }
        }
        if let Some(tick) = tick
            && !self.write_marks(now, tick, &mut alerts)
        {
            return alerts;
        }
        if nothing_read && all_read && released && self.owes_nothing() {
            self.lifecycle.observe(now, Lifecycled::Completed);
        }
        self.reconcile_on_schedule(now, &mut alerts);
        alerts
    }

    /// Journals `copy`'s establishment copy on the account stream at `now`, under this process's
    /// writer epoch, as [`crate::establish::copy_draft`] writes it with the next event id, and
    /// folds it (DEC-2341 item 2).
    ///
    /// # Errors
    /// `copy_refused` when the draft cannot be made or the journal does not commit it.
    pub fn copy_establishment(
        &mut self,
        now: UtcNanos,
        copy: &crate::establish::ToCopy,
    ) -> Result<(), &'static str> {
        let head = self.account_head();
        let id = self
            .wiring
            .ids
            .event_id(WriterEpoch(self.epoch), Seq(head), 0);
        let draft = crate::establish::copy_draft(copy, id, now)?;
        match self.journal(now, &[draft]) {
            Ran::Committed => Ok(()),
            _ => Err("copy_refused"),
        }
    }

    /// Whether the account owes nothing more (DEC-2347): the process is ready, no reconciliation
    /// is due, every order the account stream holds is terminal, `Unknown` included, and no agent
    /// followed holds lots its kill switch or exits would still sell. A pass that reads a switch
    /// or a proposal, or reaches here with anything in doubt or unsent, never asks: it returned
    /// earlier, or reads again on the next pass. An account switch still closing an instrument the
    /// account holds owes its close (DEC-2364).
    fn owes_nothing(&mut self) -> bool {
        self.ready()
            && !self.reconcile
            && !self.state.account_closing()
            && self
                .state
                .orders()
                .values()
                .all(|order| order.state.is_terminal())
            && self
                .deployed
                .keys()
                .all(|agent| self.state.holds_lots(agent) == Ok(false))
            && self.broker_quiet()
    }

    /// Whether the broker, read now, lists no order on the account that may still work, ours or
    /// one the fold never held (an order the platform did not place is external activity, never
    /// one of the fold's orders): every status it reports is final. A read that fails, or a status
    /// it does not name, keeps the executor running (DEC-2347 item 3.5).
    fn broker_quiet(&mut self) -> bool {
        let listed = match self.account_reads() {
            Some(read) => read
                .map(|reads| BrokerOutcome::OpenOrders(reads.open_orders))
                .map_err(read_error),
            None => self.read(&BrokerRequest::ListOpenOrders),
        };
        match listed {
            Ok(BrokerOutcome::OpenOrders(orders)) => orders.iter().all(|order| {
                matches!(
                    order.status.as_str(),
                    "filled" | "canceled" | "expired" | "rejected" | "replaced"
                )
            }),
            _ => false,
        }
    }

    /// Hands the core, before the pass's tick, the latest quote of each instrument whose exit waits
    /// on a price and is due a read (DEC-2359), so the tick's ladder step prices from it. A quote
    /// the feed cannot give hands nothing. `Ok(false)` when a handoff stopped the pass.
    ///
    /// # Errors
    /// The core's refusal to name the instruments it wants; the pass then reads no quote and goes
    /// on, as it would with the feed down (`AGENTS.md` rule 3).
    fn read_quotes(
        &mut self,
        now: UtcNanos,
        alerts: &mut Vec<Alert>,
    ) -> Result<bool, ExecutorError> {
        let wanted = self.state.quote_wanted()?;
        let Ok(due) = self.polled.due(&wanted, now) else {
            return Ok(true);
        };
        for instrument in due {
            let Ok(observation) = self.wiring.quotes.observe(&instrument, now) else {
                continue;
            };
            if !observation.sane {
                continue;
            }
            if !self.hand(now, now, Input::Market(observation), alerts, 0) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    /// Journals, at the tick `at`, a `MarkUpdated` version 2 for each instrument the wiring marks
    /// that is due one (DEC-2380 item 3): at most once per [`MARK_EVERY`] of the risk clock, from
    /// the quote the feed hands at `now`. An instrument the feed has no quote for, the snapshot no
    /// asset class for, or no session names, is not marked. `false` when the journal stopped the
    /// pass; an append in doubt raises the journal's one opaque alert, and the instruments after
    /// it stay due.
    fn write_marks(&mut self, now: UtcNanos, at: RiskClock, alerts: &mut Vec<Alert>) -> bool {
        let Some(calendar) = self.calendar.clone() else {
            return true;
        };
        let Ok(instant) = UtcNanos::from_parts(at.secs(), 0) else {
            return true;
        };
        let wanted = self.wiring.marked.instruments();
        let Ok(due) = self.marked_at.due(&wanted, instant) else {
            return true;
        };
        let mut due = due.into_iter();
        while let Some(instrument) = due.next() {
            let Some(class) = self.wiring.instruments.asset_class(&instrument) else {
                continue;
            };
            let Ok(observation) = self.wiring.quotes.observe(&instrument, now) else {
                continue;
            };
            let id = self
                .wiring
                .ids
                .event_id(WriterEpoch(self.epoch), Seq(self.account_head()), 0);
            let feed = self.wiring.quotes.feed();
            let Some(draft) = mark_draft(&observation, class, &calendar, feed, id, at) else {
                continue;
            };
            if !self.append_mark(now, now, draft, alerts) {
                for unmarked in due {
                    self.marked_at.forget(&unmarked);
                }
                return false;
            }
        }
        true
    }

    /// Journals the mark `draft` with the event time `at`; whether the pass may go on. An append in
    /// doubt keeps the draft, unchanged, to be offered again first on the next pass, and raises the
    /// journal's one opaque alert for the outage. A mark the journal refuses is dropped, alerted to
    /// the operator once, and the pass goes on (DEC-2805 item 3): a missing mark only leaves the
    /// risk mark stale, which holds openings at the gate, and never stops an exit.
    fn append_mark(
        &mut self,
        now: UtcNanos,
        at: UtcNanos,
        draft: EventDraft,
        alerts: &mut Vec<Alert>,
    ) -> bool {
        self.written_at = at;
        match self.journal_refusable(now, std::slice::from_ref(&draft)) {
            Ran::Committed => {
                self.journal_down = false;
                true
            }
            Ran::Refused => {
                if !self.journal_refused {
                    self.journal_refused = true;
                    alerts.push(Alert {
                        key: "journal_refused",
                        subject: None,
                    });
                }
                true
            }
            Ran::InDoubt => {
                if !self.journal_down {
                    self.journal_down = true;
                    alerts.push(journal_unavailable());
                }
                self.pending_mark = Some((draft, at));
                false
            }
            Ran::Stopped => false,
        }
    }

    /// Drains at `now`: no new input; an input in doubt is handed once more, then the process exits
    /// `Drained`, or `DrainIncomplete` if it is still in doubt.
    pub fn drain(&mut self, now: UtcNanos) {
        if self.exited() {
            return;
        }
        self.lifecycle.observe(now, Lifecycled::DrainRequested);
        let settled = match self.pending.take() {
            Some((input, at)) => self.hand(now, at, input, &mut Vec::new(), 0),
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

    pub fn state(&self) -> &ExecutorState {
        &self.state
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    pub fn broker(&self) -> &B {
        &self.broker
    }

    /// Whether the process may take an opening: its epoch held, its stream replayed, and its
    /// startup reconciliation passed (design §3.4).
    pub fn ready(&self) -> bool {
        self.lifecycle.state() == State::Ready
    }

    fn exited(&self) -> bool {
        matches!(self.lifecycle.state(), State::Exited(_))
    }

    /// The scheduler's latest tick, as the core's risk clock (journal spec §9.20). `None` when
    /// the clock stream holds none or cannot be read.
    fn tick(&mut self, alerts: &mut Vec<Alert>) -> Option<RiskClock> {
        let clock = self.clock.clone()?;
        let rows = self.follow(&clock, alerts)?;
        rows.iter()
            .filter(|row| row.event_type == "ClockAdvanced")
            .filter_map(tick_of)
            .max()
            .map(|at| RiskClock::from_secs(at.secs()))
    }

    /// The rows of a stream the process follows, or `None` when the store cannot read it. A stream
    /// that stops reading raises one opaque alert, and another only after it has read again
    /// (`AGENTS.md` rule 6: a fixed key and no subject), so a persistent failure is never silent.
    fn follow(&mut self, stream: &StreamId, alerts: &mut Vec<Alert>) -> Option<Vec<StoredEvent>> {
        let rows = self.store.rows(stream);
        let name = stream.as_str().to_owned();
        match rows {
            Some(_) => {
                self.unreadable.remove(&name);
            }
            None => {
                if self.unreadable.insert(name) {
                    alerts.push(Alert {
                        key: "stream_unreadable",
                        subject: None,
                    });
                }
            }
        }
        rows
    }

    /// The kill switches of the account's agents that this stream has not applied, read from the
    /// switch's own record (`ExecutorState::applied_copy`, DEC-2003 item 4), never from a mode
    /// another restriction may also have set; and whether a row on an agent's stream could not be
    /// read. An unreadable control stream yields nothing this pass, and so does an agent stream
    /// that cannot be read. A control record that does not read stops the process at once, since
    /// the agents it names are unknown; a row on one agent's stream that does not read stops it only
    /// after the other agents' switches are applied (DEC-85, `AGENTS.md` rule 13). An owner's
    /// acknowledgment, its request, or a record that spends a step-up that does not read holds
    /// every opening and hands no acknowledgment, and every exit still goes (journal spec §9.28,
    /// DEC-2830).
    fn switches(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>) -> Option<Followed> {
        let control = self.control.clone()?;
        let rows = self.follow(&control, alerts)?;
        let Ok(following) = agents_of(&rows, self.state.scope(), self.environment) else {
            self.lifecycle.observe(now, Lifecycled::Failed);
            return None;
        };
        self.deployed.clone_from(&following.agents);
        let workspace = self.state.scope().workspace.0.clone();
        let mut pending = Vec::new();
        let mut proposed = Vec::new();
        let mut researched = Vec::new();
        let mut unreadable = false;
        let mut all_read = true;
        for agent in following.agents.keys().cloned() {
            let Some(stream) = StreamId::parse(&format!("agent:{workspace}:{}", agent.0)) else {
                all_read = false;
                continue;
            };
            let Some(rows) = self.follow(&stream, alerts) else {
                all_read = false;
                continue;
            };
            let read: Result<Vec<_>, _> = rows
                .iter()
                .filter(|row| row.environment == self.environment.as_str())
                .map(folded)
                .collect();
            let Ok((held, asked, records)) =
                read.and_then(|rows| Ok((switches(&rows)?, proposals(&rows)?, theses(&rows))))
            else {
                unreadable = true;
                continue;
            };
            if !records.is_empty() {
                researched.push((agent.clone(), records));
            }
            let openings_held = following.held.contains(&agent);
            proposed.extend(
                asked
                    .into_iter()
                    .filter(|proposal| !(openings_held && proposal.purpose.adds_risk()))
                    .filter(|proposal| {
                        self.state
                            .intent(&IntentId(proposal.intent.clone()))
                            .is_none()
                    })
                    .map(|proposal| (agent.clone(), proposal)),
            );
            let applied = |source: &EventId| self.state.applied_for(&agent, source);
            let copied: Vec<&EventId> = held
                .iter()
                .filter(|switch| self.state.applied_copy(&switch.switch))
                .filter_map(|switch| switch.cause.as_ref())
                .collect();
            let direct: Vec<Switch> = following
                .commands_for(&agent)
                .into_iter()
                .filter(|command| !applied(command) && !copied.contains(&command))
                .map(|command| Switch {
                    switch: command,
                    cause: None,
                    initiator: Initiator::Owner,
                    confirmation: None,
                })
                .collect();
            let read: Vec<EventId> = direct.iter().map(|switch| switch.switch.clone()).collect();
            let copies: Vec<Switch> = held
                .into_iter()
                .filter(|switch| !self.state.applied_copy(&switch.switch))
                .filter(|switch| {
                    switch
                        .cause
                        .as_ref()
                        .is_none_or(|cause| !applied(cause) && !read.contains(cause))
                })
                .collect();
            pending.extend(
                direct
                    .into_iter()
                    .chain(copies)
                    .map(|switch| (agent.clone(), switch)),
            );
        }
        let agents: BTreeSet<AgentId> = following.agents.keys().cloned().collect();
        let processed_at = RiskClock::from_secs(now.secs());
        let acknowledgments = crate::acknowledgments::acknowledgments(
            &rows,
            &agents,
            self.environment,
            processed_at,
            |original| self.state.copied_acknowledgment(original),
        )
        .unwrap_or_else(|Unreadable| {
            proposed.retain(|(_, proposal)| !proposal.purpose.adds_risk());
            Vec::new()
        });
        Some(Followed {
            switches: pending,
            proposals: proposed,
            theses: researched,
            unreadable,
            released: following.released(),
            all_read,
            acknowledgments,
        })
    }

    /// Applies one switch as the core's own kill switch of that agent alone, never an account-wide
    /// one (`AGENTS.md` rule 13), naming the record it copies so the journal says it was applied.
    fn apply(
        &mut self,
        now: UtcNanos,
        agent: AgentId,
        switch: Switch,
        alerts: &mut Vec<Alert>,
    ) -> bool {
        let command = Command::KillSwitch {
            scope: KillScope::Agent(agent),
            initiator: switch.initiator,
            confirmation: switch.confirmation,
            cause: Some(switch.switch),
        };
        self.hand(now, now, Input::Command(command), alerts, 0)
    }

    /// Hands one proposal as the handoff the runtime would make in one process, an opening or an
    /// increase with its mandate's protection. A protections port that cannot answer stops the
    /// process rather than hand an opening unprotected (DEC-2033 item 3). An exit at the market is
    /// handed with no limit, which the core sends at the market or prices by the ladder; an
    /// owner's with a confirmed floor carries the floor as its limit (DEC-2037 items 1 and 7).
    fn take(
        &mut self,
        now: UtcNanos,
        agent: AgentId,
        proposal: Proposal,
        alerts: &mut Vec<Alert>,
    ) -> bool {
        let opening = proposal.limit.filter(|_| proposal.purpose.adds_risk());
        let protection = match opening {
            Some(limit) => {
                match self
                    .wiring
                    .protections
                    .protection(&agent, &proposal.instrument, limit)
                {
                    Ok(protection) => protection,
                    Err(Unplanned) => {
                        self.lifecycle.observe(now, Lifecycled::Failed);
                        return false;
                    }
                }
            }
            None => None,
        };
        let handoff = IntentHandoff {
            intent_id: IntentId(proposal.intent),
            agent,
            tif: Some(proposal.tif),
            body: IntentBody::Order {
                instrument: proposal.instrument,
                side: proposal.side,
                qty: proposal.qty,
                limit: proposal.limit,
                purpose: proposal.purpose,
                protection,
            },
            confirmation: proposal.confirmation.map(|handed| bounded(handed, now)),
        };
        self.hand(now, now, Input::Intent(handoff), alerts, 0)
    }

    /// Hands `input` to the core, journals its drafts as one batch, and only then sends what it
    /// asks of the broker, handing each answer back; whether all of it committed. An input whose
    /// append is in doubt is kept to be handed again; a refusal or a fence stops the process.
    fn hand(
        &mut self,
        now: UtcNanos,
        at: UtcNanos,
        input: Input,
        alerts: &mut Vec<Alert>,
        depth: usize,
    ) -> bool {
        if depth > self.max_chain {
            self.lifecycle.observe(now, Lifecycled::Failed);
            return false;
        }
        self.written_at = at;
        let reconciled = matches!(input, Input::BrokerSnapshot(_));
        let effects = match handle(
            &mut self.state,
            input.clone(),
            &self.wiring.ports(&Deployed {
                versions: &self.deployed,
                wiring: self.wiring.mandates.as_ref(),
            }),
            self.wiring.gate.as_ref(),
        ) {
            Ok(effects) => effects,
            Err(error) => {
                if let Some((key, intent)) = held(&error, &input) {
                    if self.held.insert(intent.clone()) {
                        alerts.push(Alert {
                            key,
                            subject: Some(intent),
                        });
                    }
                    return true;
                }
                if !held_on_core_error(&input) {
                    self.lifecycle.observe(now, Lifecycled::Failed);
                    return false;
                }
                if !self.input_refused {
                    self.input_refused = true;
                    alerts.push(Alert {
                        key: "input_refused",
                        subject: None,
                    });
                }
                if reconciled {
                    self.lifecycle
                        .observe(now, Lifecycled::Reconciled { passed: false });
                }
                return self.hand(now, at, Input::InputFailed, alerts, depth.saturating_add(1));
            }
        };
        let mut drafts = Vec::new();
        let mut asked = Vec::new();
        let mut requests = Vec::new();
        for effect in effects {
            match effect {
                Effect::Journal(draft) => drafts.push(draft),
                Effect::Broker(request) => requests.push(request),
                Effect::Notify(notice) => asked.push(Alert {
                    key: notice.kind.key(),
                    subject: Some(notice.subject_event.0),
                }),
                Effect::Timer(_) => {}
            }
        }
        if !drafts.is_empty() {
            match self.journal_refusable(now, &drafts) {
                Ran::Committed => {
                    self.journal_down = false;
                }
                Ran::Refused => return self.refused(now, at, input, alerts, depth),
                Ran::InDoubt => {
                    if !self.journal_down {
                        self.journal_down = true;
                        alerts.push(journal_unavailable());
                    }
                    self.pending = Some((input, at));
                    return false;
                }
                Ran::Stopped => return false,
            }
        }
        alerts.extend(asked);
        if reconciled {
            let passed =
                self.state.mismatched().is_empty() && self.state.reconciled_through().is_some();
            self.lifecycle
                .observe(now, Lifecycled::Reconciled { passed });
        }
        let unsent = std::mem::take(&mut self.outbox);
        for request in requests {
            if is_reconciliation_read(&request) {
                self.reconcile = true;
            } else {
                self.outbox.push_back(request);
            }
        }
        self.outbox.extend(unsent);
        self.flush(now, alerts, depth)
    }

    /// Asks for a reconciliation once [`SCHEDULED_RECONCILE_S`] have passed since the last one this
    /// asked for, while any order the account stream holds may still work (X1b-3). The broker
    /// reports a fill only on a read, so without it an order accepted and filled later is booked
    /// only by the next start's reconciliation. It runs last in a pass that went through, so no
    /// read goes ahead of what the pass sent, and none while the journal is down; a run that does
    /// not commit has already told the lifecycle, and the next pass picks up what it left.
    fn reconcile_on_schedule(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>) {
        let working = self
            .state
            .orders()
            .values()
            .any(|order| !order.state.is_terminal());
        let due = now.secs().saturating_sub(self.scheduled.secs()) >= SCHEDULED_RECONCILE_S;
        if working && due {
            self.reconcile = true;
            self.scheduled = now;
            self.flush(now, alerts, 0);
        }
    }

    /// Sends the requests left in the outbox in order, handing each answer to the core, then the
    /// reconciliation if one is due. A request that cannot be sent, or an answer that cannot be
    /// read, stops the process (DEC-85); whether everything handed committed.
    fn flush(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>, depth: usize) -> bool {
        let next = depth.saturating_add(1);
        while let Some(request) = self.outbox.pop_front() {
            let answer = match self.call(&request) {
                Ok(outcome) => Input::Broker(Ok(outcome)),
                Err(ConnectorError::Unknown(unknown)) => Input::Broker(Err(unknown)),
                Err(_) => {
                    self.lifecycle.observe(now, Lifecycled::Failed);
                    return false;
                }
            };
            if !self.hand(now, self.written_at, answer, alerts, next) {
                return false;
            }
        }
        if self.reconcile {
            return self.reconciliation(now, alerts, next);
        }
        true
    }

    /// The reconciliation the core asked for (trading spec §11): the broker's reads as one
    /// snapshot (its four reads, or a one-read broker's account read after its unlisted orders
    /// are listed, [`Self::snapshot`]), its account reported first, as the shell's start does. The snapshot keeps the
    /// head it was taken at, before its own report, so the reconciliation reads that report as
    /// the snapshot's own and counts every fill since the base it replaced once (DEC-948 item 5);
    /// a fill the broker already booked in the reported cash is never subtracted twice. A read
    /// whose answer is lost leaves it due for the next pass, since a read changes nothing at the broker, and the pass goes on, so a kill switch
    /// never waits on it (`AGENTS.md` rule 13). It stays due until its snapshot is handed.
    fn reconciliation(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>, depth: usize) -> bool {
        self.reconcile = false;
        let snapshot = match self.snapshot(now, alerts, depth) {
            Ok(snapshot) => snapshot,
            Err(Read::Lost) => {
                self.reconcile = true;
                return true;
            }
            Err(Read::Halted) => {
                self.reconcile = true;
                return false;
            }
            Err(Read::Unusable) => {
                self.reconcile = true;
                self.lifecycle.observe(now, Lifecycled::Failed);
                return false;
            }
        };
        let reported = Input::BrokerUpdate(BrokerUpdate::Account(snapshot.account.clone()));
        if !self.hand(now, self.written_at, reported, alerts, depth) {
            self.reconcile = true;
            return false;
        }
        self.hand(
            now,
            self.written_at,
            Input::BrokerSnapshot(snapshot),
            alerts,
            depth,
        )
    }

    /// The broker's open orders, positions, account and activity since the executor's cursor. A
    /// broker that publishes no activity is read once instead, and its fills are the ones its
    /// order records and positions imply, an entry's (`recorded_fills`, DEC-1151) and a
    /// risk-reducing sell's at its limit (`limit_fills`, DEC-2300), with the cursor left where it
    /// was (DEC-2386 item 4), as the shell's reconciler takes them.
    fn snapshot(
        &mut self,
        now: UtcNanos,
        alerts: &mut Vec<Alert>,
        depth: usize,
    ) -> Result<BrokerSnapshot, Read> {
        let since = self
            .state
            .checkpoint()
            .cloned()
            .unwrap_or_else(|| ActivityCursor(String::new()));
        if let Some(read) = self.account_reads() {
            let mut reads = read.map_err(read_error)?;
            if self.list_unlisted(&reads.open_orders, now, alerts, depth)? {
                reads = self
                    .account_reads()
                    .ok_or(Read::Unusable)?
                    .map_err(read_error)?;
            }
            let mut fills = recorded_fills(&self.state, &reads.positions);
            fills.extend(limit_fills(&self.state, &reads.positions).map_err(|_| Read::Unusable)?);
            return Ok(BrokerSnapshot {
                open_orders: reads.open_orders,
                positions: reads.positions,
                account: reads.account,
                fills,
                cursor: since,
                reason: ReconcileReason::Startup,
                declared: self.broker.declared(),
                taken_at_head: Seq(self.account_head()),
            });
        }
        let BrokerOutcome::OpenOrders(open_orders) = self.read(&BrokerRequest::ListOpenOrders)?
        else {
            return Err(Read::Unusable);
        };
        let BrokerOutcome::Positions(positions) = self.read(&BrokerRequest::ListPositions)? else {
            return Err(Read::Unusable);
        };
        let BrokerOutcome::Account(account) = self.read(&BrokerRequest::GetAccount)? else {
            return Err(Read::Unusable);
        };
        let BrokerOutcome::Activities { fills, cursor } =
            self.read(&BrokerRequest::ListActivities { since })?
        else {
            return Err(Read::Unusable);
        };
        Ok(BrokerSnapshot {
            open_orders,
            positions,
            account,
            fills,
            cursor,
            reason: ReconcileReason::Startup,
            declared: self.broker.declared(),
            taken_at_head: Seq(self.account_head()),
        })
    }

    /// Before a one-read broker's snapshot is reconciled, each of our sent orders that may still
    /// work but that the read no longer lists as working is listed, and the answer handed to the
    /// core, which adopts the one record naming it at our prices (DEC-862, DEC-1150 item 2,
    /// DEC-2395 item 2). Robinhood reports a fill only on the order's record, so this is what
    /// books an order that filled between two reads before its position is compared: without it,
    /// every fill would read as an unexplained position and pause the agent (DEC-2800).
    ///
    /// Whether an order may still work is read as each is reached, so one an earlier answer's
    /// effects settled (a nested reconciliation that booked it) is skipped, and a terminal order
    /// is never listed.
    ///
    /// Returns whether any answer was handed: its effects may have journaled and sent more (a
    /// protective order placed on the fill), so the snapshot then reads the account again, and its
    /// reads are never older than the head it is taken at.
    ///
    /// # Errors
    /// [`Read::Lost`] for a listing whose answer was lost, [`Read::Unusable`] for one that could
    /// not be sent, and [`Read::Halted`] when a handed answer did not commit.
    fn list_unlisted(
        &mut self,
        working: &[BrokerOrder],
        now: UtcNanos,
        alerts: &mut Vec<Alert>,
        depth: usize,
    ) -> Result<bool, Read> {
        let listed: BTreeSet<&str> = working
            .iter()
            .filter_map(|order| order.client_order_id.as_deref())
            .collect();
        let unlisted: Vec<_> = self
            .state
            .orders()
            .values()
            .filter(|order| order.state != OrderState::Intent)
            .filter(|order| !listed.contains(order.client_order_id.as_str()))
            .map(|order| order.client_order_id.clone())
            .collect();
        let mut handed = false;
        for id in unlisted {
            let open = self
                .state
                .orders()
                .get(&id)
                .is_some_and(|order| !order.state.is_terminal());
            let Some(listing) = open.then(|| self.state.listing(&id)).flatten() else {
                continue;
            };
            let answer = self.read(&BrokerRequest::ListOrders(listing))?;
            if !self.hand(
                now,
                self.written_at,
                Input::Broker(Ok(answer)),
                alerts,
                depth,
            ) {
                return Err(Read::Halted);
            }
            handed = true;
        }
        Ok(handed)
    }

    /// A read of the broker. One whose credential the broker refused is an unknown outcome
    /// (`Unauthorized`, connections spec §5.4), so it is read again on a later pass like a lost
    /// one, and counted until the connection is `suspended` and the account restricted
    /// (DEC-2832), never a stop that a restart would repeat with its count lost.
    fn read(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, Read> {
        self.call(request).map_err(read_error)
    }

    /// Every request to the broker, its answer counted toward the connection's health (DEC-2832).
    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let answer = self.broker.call(request);
        self.health.observe(&answer);
        answer
    }

    /// Tells the core of every trouble the connection's health holds (DEC-2832): after the pass's
    /// kill switches, so none waits on it, and again before each proposal, so a count that crossed
    /// its threshold earlier in the pass restricts the account before the next opening is taken.
    /// The core journals each restriction and state change once (DEC-2826). `false` when a handoff
    /// stopped the pass.
    fn report_troubles(&mut self, now: UtcNanos, alerts: &mut Vec<Alert>) -> bool {
        self.health
            .troubles()
            .into_iter()
            .all(|trouble| self.hand(now, now, Input::Connection(trouble), alerts, 0))
    }

    /// A one-read broker's account reads, counted as one answer (DEC-2832).
    fn account_reads(&mut self) -> Option<Result<AccountReads, ConnectorError>> {
        let read = self.broker.account_reads()?;
        match &read {
            Ok(_) => self.health.observe_answered(),
            Err(error) => self.health.observe_error(error),
        }
        Some(read)
    }

    fn account_head(&self) -> u64 {
        self.account
            .as_ref()
            .and_then(|a| self.state.head(a.as_str()))
            .map_or(0, |seq| seq.0)
    }

    /// DEC-2805: the journal refused the batch `input` drafted. Nothing of it committed and none of
    /// its requests is sent; the core forgets it and holds every agent's openings, alerting the
    /// owner once, and the pass goes on, so exits, protection and the kill switch never stop on a
    /// record the journal cannot take (`AGENTS.md` rule 13). The operator is alerted once per
    /// process. A refused reconciliation is one that did not pass. Where [`held_on_refusal`] says
    /// the refusal cannot be held, the process stops.
    fn refused(
        &mut self,
        now: UtcNanos,
        at: UtcNanos,
        input: Input,
        alerts: &mut Vec<Alert>,
        depth: usize,
    ) -> bool {
        if !held_on_refusal(&input) {
            self.lifecycle.observe(now, Lifecycled::Failed);
            return false;
        }
        if !self.journal_refused {
            self.journal_refused = true;
            alerts.push(Alert {
                key: "journal_refused",
                subject: None,
            });
        }
        if matches!(input, Input::BrokerSnapshot(_)) {
            self.lifecycle
                .observe(now, Lifecycled::Reconciled { passed: false });
        }
        self.hand(
            now,
            at,
            Input::BatchRefused,
            alerts,
            depth.saturating_add(1),
        )
    }

    /// Journals `drafts` as one batch on the account stream and folds what committed; a refused
    /// batch stops the process.
    fn journal(&mut self, now: UtcNanos, drafts: &[EventDraft]) -> Ran {
        match self.journal_refusable(now, drafts) {
            Ran::Refused => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                Ran::Stopped
            }
            ran => ran,
        }
    }

    /// Journals `drafts` as one batch on the account stream and folds what committed, answering
    /// [`Ran::Refused`] for a batch the journal refused as invalid.
    fn journal_refusable(&mut self, now: UtcNanos, drafts: &[EventDraft]) -> Ran {
        let Some(account) = self.account.clone() else {
            return Ran::Stopped;
        };
        let envelope = Envelope {
            stream: account.as_str(),
            environment: self.environment,
            actor_kind: "system",
            actor_id: "executor",
            build: concat!("mandate-executord/", env!("CARGO_PKG_VERSION")),
            version: VERSION,
            event_time: self.written_at,
        };
        let bytes: Option<Vec<Vec<u8>>> = drafts
            .iter()
            .map(|draft| {
                draft_bytes(
                    &envelope,
                    &Fields {
                        event_id: &draft.event_id.0,
                        event_type: &draft.event_type,
                        schema_version: draft.schema_version,
                        causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
                        correlation_id: None,
                        config_refs: &draft.config_refs,
                        payload: &draft.payload,
                    },
                )
            })
            .collect();
        let Some(bytes) = bytes else {
            self.lifecycle.observe(now, Lifecycled::Failed);
            return Ran::Stopped;
        };
        let head = self.account_head();
        match self.store.append(&account, head, self.epoch, now, &bytes) {
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
            AppendOutcome::Invalid { .. } => Ran::Refused,
            AppendOutcome::HeadMismatch { .. } | AppendOutcome::IdempotencyConflict { .. } => {
                self.lifecycle.observe(now, Lifecycled::Failed);
                Ran::Stopped
            }
        }
    }

    /// Folds `rows` of the account stream; the stream's head after them, or `None` for a row the
    /// fold refuses or one journaled for another environment (`AGENTS.md` rule 8).
    fn fold_rows(&mut self, rows: &[StoredEvent]) -> Option<u64> {
        for row in rows {
            if row.environment != self.environment.as_str() {
                return None;
            }
            let event = folded(row).ok()?;
            fold(&mut self.state, &event).ok()?;
        }
        Some(self.account_head())
    }
}

/// A broker read that did not answer: a lost answer may be retried, anything else is unusable.
fn read_error(error: ConnectorError) -> Read {
    match error {
        ConnectorError::Unknown(_) => Read::Lost,
        _ => Read::Unusable,
    }
}

/// The alert an append going in doubt raises: one per outage, with a fixed key and no subject, so
/// it discloses nothing but the fact that the executor is holding (`AGENTS.md` rule 6, E21-26,
/// design §2.1 and §8.2, RB-07). `stream_unreadable` is the same shape.
fn journal_unavailable() -> Alert {
    Alert {
        key: "journal_unavailable",
        subject: None,
    }
}

/// One of the four reads the core asks for a reconciliation (`request_reconciliation`), which the
/// process gathers into one snapshot rather than send one by one.
fn is_reconciliation_read(request: &BrokerRequest) -> bool {
    matches!(
        request,
        BrokerRequest::ListOpenOrders
            | BrokerRequest::ListPositions
            | BrokerRequest::GetAccount
            | BrokerRequest::ListActivities { .. }
    )
}

/// The tick a `ClockAdvanced` row carries (journal spec §9.20).
fn tick_of(row: &StoredEvent) -> Option<UtcNanos> {
    let body = parse(&row.body).ok()?;
    let clock = body.get("payload")?.get("clock")?.as_str()?;
    UtcNanos::parse(clock).ok()
}

/// The alert and intent for a handoff the core refused for a missing binding-gate input, which the
/// process holds rather than stop on (DEC-2033 item 5): `opening_held` for an opening or an
/// increase, and `exit_held` for an exit, an alarm that must never sound once the mandate port reads
/// each followed agent's deployed version, since an exit needs no other gate fact (rule 13); the
/// binary's fixture port reads none yet. Any other
/// refusal, and this one for any input but a handoff, is `None`: it stops the process.
fn held(error: &ExecutorError, input: &Input) -> Option<(&'static str, String)> {
    let (ExecutorError::BindingGateInputMissing, Input::Intent(handoff)) = (error, input) else {
        return None;
    };
    let opens = match &handoff.body {
        IntentBody::Order { purpose, .. } => purpose.adds_risk(),
        _ => false,
    };
    let key = if opens { "opening_held" } else { "exit_held" };
    Some((key, handoff.intent_id.0.0.clone()))
}

/// Whether a core error on `input` is held and the process goes on (DEC-2821): the broker's
/// answers, updates and snapshots, the quotes its feed sends, an agent's thesis records (DEC-2823),
/// and the connection's troubles (DEC-2832), which the restart would read or count again, so
/// stopping on one would be a crash loop that also stops every exit, protective order and kill
/// switch. Any other input's error, the hold's own included, stops the process as before.
fn held_on_core_error(input: &Input) -> bool {
    matches!(
        input,
        Input::Broker(_)
            | Input::BrokerUpdate(_)
            | Input::BrokerSnapshot(_)
            | Input::Market(_)
            | Input::Theses { .. }
            | Input::Connection(_)
    )
}

/// Whether a refusal of the batch `input` drafted is held and the process goes on (DEC-2805 item
/// 1), or stops it (item 4). A refused hold leaves nothing safe to record. A refused start has
/// already marked the core started and lost its recovery, the lookups of orders in doubt, the
/// resent cancels and the startup reconciliation, which `Input::Started` cannot be handed again
/// to redo; the restart redoes them, before any trading.
fn held_on_refusal(input: &Input) -> bool {
    !matches!(input, Input::BatchRefused | Input::Started(_))
}

#[cfg(test)]
mod tests {
    use mandate_accounting::Side;
    use mandate_executor::{EventId, Purpose, TimeInForce};
    use mandate_num::Qty;

    use super::*;

    type Error = Box<dyn std::error::Error>;

    fn handoff(purpose: Purpose) -> Result<Input, Error> {
        Ok(Input::Intent(IntentHandoff {
            intent_id: IntentId(EventId("i1".to_owned())),
            agent: AgentId("agenta".to_owned()),
            tif: Some(TimeInForce::Day),
            body: IntentBody::Order {
                instrument: InstrumentId::new("AAPL")?,
                side: Side::Sell,
                qty: Qty::parse("10")?,
                limit: Some(Price::parse("150")?),
                purpose,
                protection: None,
            },
            confirmation: None,
        }))
    }

    #[test]
    fn only_a_handoff_refused_for_a_missing_gate_input_is_held() -> Result<(), Error> {
        let missing = ExecutorError::BindingGateInputMissing;
        let held_as = |purpose| -> Result<_, Error> {
            Ok(held(&missing, &handoff(purpose)?).map(|(key, _)| key))
        };
        assert_eq!(held_as(Purpose::Open)?, Some("opening_held"));
        assert_eq!(held_as(Purpose::Increase)?, Some("opening_held"));
        for exit in [
            Purpose::RiskExit,
            Purpose::DiscretionaryExit,
            Purpose::OwnerExit,
        ] {
            assert_eq!(held_as(exit)?, Some("exit_held"));
        }
        assert_eq!(
            held(&missing, &handoff(Purpose::Open)?),
            Some(("opening_held", "i1".to_owned()))
        );
        let tick = Input::Tick(RiskClock::from_secs(0));
        assert_eq!(held(&missing, &tick), None);
        assert_eq!(held(&missing, &Input::Started(WriterEpoch(1))), None);
        assert_eq!(
            held(&ExecutorError::NotStarted, &handoff(Purpose::Open)?),
            None
        );
        Ok(())
    }

    /// A wiring port that knows `agentb` and covers only MSFT, with a 1 % crypto offset.
    struct Wired;

    impl MandateView for Wired {
        fn version(&self, agent: &AgentId) -> Option<MandateVersion> {
            (agent.0 == "agentb").then(|| MandateVersion("sha256:wired".to_owned()))
        }

        fn crypto_stop_limit_offset(&self, _agent: &AgentId) -> Option<Fraction> {
            Fraction::parse("0.01").ok()
        }

        fn covers(&self, _agent: &AgentId, instrument: &InstrumentId) -> bool {
            instrument.as_str() == "MSFT"
        }
    }

    #[test]
    fn a_followed_agents_deployed_version_answers_first_and_the_rest_is_the_wirings()
    -> Result<(), Error> {
        let agent = |id: &str| AgentId(id.to_owned());
        let versions = BTreeMap::from([(
            agent("agenta"),
            MandateVersion("sha256:deployed".to_owned()),
        )]);
        let port = Deployed {
            versions: &versions,
            wiring: &Wired,
        };
        let version = |id: &str| port.version(&agent(id)).map(|version| version.0);
        assert_eq!(version("agenta").as_deref(), Some("sha256:deployed"));
        assert_eq!(version("agentb").as_deref(), Some("sha256:wired"));
        assert_eq!(version("agentc"), None);
        assert_eq!(
            port.crypto_stop_limit_offset(&agent("agenta")),
            Some(Fraction::parse("0.01")?)
        );
        assert!(port.covers(&agent("agenta"), &InstrumentId::new("MSFT")?));
        assert!(!port.covers(&agent("agenta"), &InstrumentId::new("AAPL")?));
        Ok(())
    }

    /// DEC-2821: a core error is held for a broker answer, and for no other input, the hold's
    /// own among them.
    #[test]
    fn a_core_error_is_held_for_a_broker_input_or_thesis_records_alone() -> Result<(), Error> {
        assert!(held_on_core_error(&Input::Broker(Ok(
            mandate_executor::BrokerOutcome::AccountWideAccepted
        ))));
        assert!(held_on_core_error(&Input::Theses {
            agent: AgentId("agenta".to_owned()),
            records: Vec::new(),
        }));
        for other in [
            Input::Tick(RiskClock::from_secs(0)),
            Input::Started(WriterEpoch(1)),
            Input::InputFailed,
            Input::BatchRefused,
            handoff(Purpose::Open)?,
        ] {
            assert!(!held_on_core_error(&other), "{other:?}");
        }
        Ok(())
    }

    /// DEC-2805: a refusal is held for every input but a refused hold and a refused start, which
    /// stop the process (item 4).
    #[test]
    fn a_refusal_is_held_unless_it_is_of_the_hold_or_the_start() -> Result<(), Error> {
        assert!(!held_on_refusal(&Input::BatchRefused));
        assert!(!held_on_refusal(&Input::Started(WriterEpoch(1))));
        assert!(held_on_refusal(&Input::Tick(RiskClock::from_secs(0))));
        assert!(held_on_refusal(&handoff(Purpose::Open)?));
        Ok(())
    }
}
