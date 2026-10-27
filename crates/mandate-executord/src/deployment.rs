//! The executor's production ports over one account's deployments (E21-30 X1b-3 G2b, DEC-2380
//! items 2, 3 and 5), in place of [`crate::fixture`]'s, which know nothing.
//!
//! Each followed agent's deployment is read as the paper shell reads it: its confirmed version
//! and the registrations on the control stream, with the objects they name from the artifact
//! store, through `mandate-gate-inputs` ([`Reviewed`]). From it come the mandate, instrument and
//! protection ports ([`DeployedView`]), the instrument the executor marks, and the binding gate's
//! input ([`DeployedGate`]) over the journal's account, mark and clock (`mandate-gate-context`),
//! the quote, the stored bars' liquidity (DEC-2274) and the broker's facts.
//!
//! Every port reads the store when it is asked, as the process reads its streams each pass. A
//! deployment is read again whenever the control stream grows, and for an opening whenever the
//! trade date changes; a read that fails is never kept, so the next ask reads again. An opening
//! reads only the deployment as it reads now: one the run cannot read covers nothing, is handed no
//! protection and is held (`AGENTS.md` rule 3). What an exit, a fill or a mark reads (the
//! version, the instrument's class, grid and fill references, the instrument marked) falls back
//! to the agent's last deployment that read, so a later unreadable row or a store outage never
//! takes from a held position what its exit and its fills need (rule 13). The binding gate's
//! input is never answered for an exit, which reads nothing here. The broker's answers are read
//! through [`Named`], which names each deployed instrument by its asset id, as the executor sent it,
//! wherever the broker names it by its ticker (DEC-938 item 1, [DEC-2813](../../../docs/project/decisions/DEC-2813.md)).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, parse};
use mandate_connections::checks::{AccountRead, CheckReport, Occasion};
use mandate_connections::record::ConnectionId;
use mandate_domain::{CapabilityProfile, ProfileError};
use mandate_executor::{
    AccountDeclaration, AccountFillRefs, AccountScope, AgentId, BindingGateInput,
    BindingGateRequest, BindingGateSource, BrokerOutcome, BrokerRequest, BrokerSymbols,
    ConnectorError, ExitTier, FillConfigRefs, InstrumentSnapshot, MandateVersion, MandateView,
    MarketObservation, ProtectionPrices,
};
use mandate_gate_context::{Freshness, Journaled, Row};
use mandate_gate_inputs::control::{
    Admitted, ConfirmedVersion, ControlRecord, LiveAdmission, Pinned, RunFacts,
    admitted_configuration, configuration, confirmed_version,
};
use mandate_gate_inputs::{DeployedView, Reviewed};
use mandate_journal::{ArtifactSource, Environment, StoredEvent, StreamId, TrustedStart};
use mandate_num::{Fraction, Price, Ratio, ShareIncrement, Usd};
use mandate_research::{ReplayError, Replayed, ThesisRecord, replay_theses};
use mandate_risk::{AgentMode, ConductState, DayTradeLedger, RiskSnapshot, SaneQuote};
use mandate_spec::ValidatedMandate;
use mandate_spec::context::Membership;
use mandate_time::{Date, UtcNanos};

use crate::follow::agents_of;
use crate::gate::{AccountRules, AgentRisk, AssetRecord, DeployedGate, Liquidity, OrderFacts};
use crate::marks::Marked;
use crate::quotes::QuoteFeed;
use crate::risk::{GateRisk, RiskOpening, RiskOpenings};
use crate::{Broker, Protections, Unplanned, Wiring};

/// How fresh, in risk-clock seconds, the journal's facts an opening reads must be: a mark within a
/// minute, as the executor marks each deployed instrument at every tick, and the account's
/// snapshot within an hour, as the runtime's dry run reads them (DEC-2378 item 4).
pub const FRESH: Freshness = Freshness {
    mark_max_age_s: 60,
    snapshot_max_age_s: 3600,
};

/// How a port reads one stream: the store's records from `start.from_seq` on, their chain checked
/// from `start` (`TrustedStart::GENESIS` reads the whole stream), `None` when the store cannot
/// answer or the first record does not follow `start`.
pub type Rows = Box<dyn Fn(&StreamId, TrustedStart) -> Option<Vec<StoredEvent>>>;

/// `read`, a reader of whole streams, as [`Rows`]: the records it gives from `start.from_seq` on,
/// `None` when the first of them does not follow `start`. Each read still reads the whole stream;
/// the process's own stores read only what follows (`PgStore::rows_from`).
pub fn reading(read: impl Fn(&StreamId) -> Option<Vec<StoredEvent>> + 'static) -> Rows {
    Box::new(move |stream, start| {
        let since: Vec<StoredEvent> = read(stream)?
            .into_iter()
            .filter(|row| row.seq >= start.from_seq)
            .collect();
        match since.first() {
            Some(first) if first.seq != start.from_seq || first.prev_hash != start.prev_hash => {
                None
            }
            _ => Some(since),
        }
    })
}

/// One stream as far as it was read: what its records folded to, and the seq and hash of the last
/// one, so the next read asks only for what follows it (DEC-2847).
struct Tail<S> {
    head: Option<(u64, Digest)>,
    state: S,
}

impl<S> Tail<S> {
    fn new(state: S) -> Self {
        Self { head: None, state }
    }

    /// Reads what `rows` holds of `stream` past the head and folds each record into the state in
    /// order with `fold`. A read past the head that the store refuses is read again from the first
    /// record onto `fresh` state, so a held head that is not the stream's is never folded on;
    /// `false` when neither reads, and then the state must not be read (`AGENTS.md` rule 3).
    fn follow(
        &mut self,
        rows: &Rows,
        stream: &StreamId,
        fresh: impl Fn() -> S,
        mut fold: impl FnMut(&mut S, &StoredEvent),
    ) -> bool {
        let start = self
            .head
            .map_or(TrustedStart::GENESIS, |(seq, hash)| TrustedStart {
                from_seq: seq.saturating_add(1),
                prev_hash: hash,
            });
        let since = match rows(stream, start) {
            Some(since) => since,
            None if self.head.is_some() => match rows(stream, TrustedStart::GENESIS) {
                Some(all) => {
                    self.head = None;
                    self.state = fresh();
                    all
                }
                None => return false,
            },
            None => return false,
        };
        for row in &since {
            fold(&mut self.state, row);
            self.head = Some((row.seq, row.hash));
        }
        true
    }
}

/// A thesis record of an agent stream as [`replay_theses`] reads it: its type, and its payload, or
/// `None` for a body that does not parse.
type Kept = (String, Option<Value>);

/// What the hot ports read of the streams, each folded only past the head it was last read to, so
/// a pass costs what was appended since and never the streams' whole length (DEC-2847): the
/// control stream's records, the scheduler's clock folded, and each research agent stream's thesis
/// records. The control stream is small; the clock and agent streams keep no other record.
#[derive(Default)]
struct Tails {
    control: Option<Tail<Vec<StoredEvent>>>,
    clock: Option<Tail<Journaled>>,
    theses: BTreeMap<String, Tail<Vec<Kept>>>,
}

/// The stored bars' liquidity for the symbol `symbol` at `now` (trading spec §9.4, §9.5;
/// DEC-2274), `None` when it cannot be read, which holds the opening.
pub trait LiquidityRead {
    fn liquidity(&self, symbol: &str, now: UtcNanos) -> Option<Liquidity>;
}

/// The broker's facts the journal does not hold (trading spec §9.2, §9.4): the instrument's asset
/// record and halt status by its symbol, and the account's day-trade rules; each `None` when it
/// cannot be read, which holds the opening.
pub trait BrokerFacts {
    fn asset(&self, symbol: &str) -> Option<AssetRecord>;
    fn rules(&self) -> Option<AccountRules>;
    fn halted(&self, symbol: &str) -> Option<bool>;
}

/// Nothing read: every fact is unread, so every opening is held.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unread;

impl LiquidityRead for Unread {
    fn liquidity(&self, _symbol: &str, _now: UtcNanos) -> Option<Liquidity> {
        None
    }
}

impl BrokerFacts for Unread {
    fn asset(&self, _symbol: &str) -> Option<AssetRecord> {
        None
    }

    fn rules(&self) -> Option<AccountRules> {
        None
    }

    fn halted(&self, _symbol: &str) -> Option<bool> {
        None
    }
}

/// The stored bars under `root` (`MANDATE_BARS`, the data process's datasets), read by
/// `mandate_bars::liquidity` at the binding clock; any refusal is no figure (DEC-2274).
#[derive(Debug, Clone)]
pub struct StoredBars {
    pub root: PathBuf,
}

impl LiquidityRead for StoredBars {
    fn liquidity(&self, symbol: &str, now: UtcNanos) -> Option<Liquidity> {
        let facts = mandate_bars::liquidity(&self.root, symbol, now).ok()?;
        Some(Liquidity {
            prior_close: facts.prior_close,
            median_dollar_volume_20d: facts.median_dollar_volume_20d,
            adv_20d: facts.adv_20d,
            trailing_5m_volume: facts.trailing_5m_volume,
        })
    }
}

/// What the deployment reads.
pub struct Sources {
    pub rows: Rows,
    pub artifacts: Box<dyn ArtifactSource>,
    pub quotes: Box<dyn QuoteFeed>,
    pub liquidity: Box<dyn LiquidityRead>,
    pub broker: Box<dyn BrokerFacts>,
}

/// What a production run reads its deployments from: the journal's rows, the artifact store and
/// the stored bars. The broker's facts are read by the run's broker once it serves them; until
/// then they are unread and every opening is held.
pub struct Production {
    pub rows: Rows,
    pub artifacts: Box<dyn ArtifactSource>,
    pub liquidity: Box<dyn LiquidityRead>,
}

/// The production a pod reads when it mounts the artifact store, as `var` names it
/// (`MANDATE_ARTIFACTS`, read only): its rows over the reader `rows` opens, the store, and the
/// stored bars under `MANDATE_BARS`, unread without them (DEC-2274). `None` without the store, so
/// the run keeps the fixture's ports, which admit no opening.
///
/// # Errors
/// `artifacts_unavailable` for a store that is named and does not open, and `rows`' refusal.
pub fn production(
    var: impl Fn(&str) -> Option<String>,
    rows: impl FnOnce() -> Result<Rows, &'static str>,
) -> Result<Option<Production>, &'static str> {
    let Some(dir) = var("MANDATE_ARTIFACTS") else {
        return Ok(None);
    };
    let artifacts = FsArtifactStore::open_existing(dir).map_err(|_| "artifacts_unavailable")?;
    let liquidity: Box<dyn LiquidityRead> = match var("MANDATE_BARS") {
        Some(root) => Box::new(StoredBars { root: root.into() }),
        None => Box::new(Unread),
    };
    Ok(Some(Production {
        rows: rows()?,
        artifacts: Box::new(artifacts),
        liquidity,
    }))
}

/// One followed agent's deployment, read.
struct Resolved {
    view: DeployedView,
    gate: DeployedGate,
    version: String,
    allocation: Usd,
    loss_carry: Usd,
    symbol: String,
    instrument: InstrumentId,
    /// The confirmed version, validated on every V-rule but V-002, which its risk fold's opening
    /// validates on the opening's equity (DEC-2804 item 2).
    confirmed: ConfirmedVersion,
    /// The reviewed instrument's asset class and its quote staleness, its fold's opening's.
    asset_class: AssetClass,
    quote_max_age: Duration,
    /// Whether the deployed version pins its universe: the binding gate's hold once the account
    /// has traded is named by it (DEC-2143).
    pinned: bool,
}

/// The deployments read at one control-stream length and trade date, so a port asked again within
/// a pass reads no artifact twice. `read` is whether every followed agent's deployment read from
/// every control row: a read that failed is read again at the next ask. `last` is each agent's
/// last deployment that read, from the control rows that parse, kept across later failures for
/// what an exit, a fill or a mark of an agent still `followed` reads.
///
/// A research agent's deployment names no instrument (DEC-2819 item 2): `research` holds what its
/// confirmed version reads now, `research_last` what it last read, and `admitted` each instrument
/// its admission admitted, resolved, with the control length and trade date it was resolved at.
#[derive(Default)]
struct Cache {
    control: Option<usize>,
    date: Option<Date>,
    read: bool,
    followed: BTreeSet<AgentId>,
    current: BTreeMap<AgentId, Rc<Resolved>>,
    last: BTreeMap<AgentId, Rc<Resolved>>,
    research: BTreeMap<AgentId, Rc<Research>>,
    research_last: BTreeMap<AgentId, Rc<Research>>,
    admitted: BTreeMap<(AgentId, InstrumentId), (usize, Date, Rc<Resolved>)>,
}

/// A research agent's confirmed version as read from the control `records` on `day`, and the
/// registered triple of its model that admits instruments.
struct Research {
    confirmed: ConfirmedVersion,
    records: Vec<ControlRecord>,
    day: Date,
    model_id: String,
    model_version: String,
    content_hash: mandate_canon::Digest,
}

/// One agent's deployment as the control stream reads it: pinned to its one instrument, or a
/// research agent's, whose instruments its admission admits.
enum Read {
    Pinned(Box<Resolved>),
    Research(Box<Research>),
}

struct Inner {
    scope: AccountScope,
    control: Option<StreamId>,
    account: Option<StreamId>,
    clock: Option<StreamId>,
    sources: Sources,
    cache: RefCell<Cache>,
    tails: RefCell<Tails>,
    /// Each agent's risk as the process's risk book last read it (DEC-2804 item 7).
    risk: RefCell<BTreeMap<AgentId, GateRisk>>,
    /// Whether the last hand-off could not be kept: then no agent's risk is answered, so a view
    /// that failed is never read as the one before it.
    risk_stale: Cell<bool>,
}

/// The account's deployments as every production port reads them, shared by all of them.
#[derive(Clone)]
pub struct Deployment(Rc<Inner>);

impl Deployment {
    /// The deployments of `scope`'s account, read from `sources`. The executor runs paper only
    /// (`AGENTS.md` rule 8), so only paper deployments are read.
    pub fn new(scope: &AccountScope, sources: Sources) -> Self {
        let workspace = scope.workspace.0.as_str();
        Self(Rc::new(Inner {
            scope: scope.clone(),
            control: StreamId::parse(&format!("ctl:{workspace}")),
            account: StreamId::parse(&format!("acct:{workspace}:{}", scope.account.0)),
            clock: StreamId::parse(&format!("clock:{workspace}")),
            sources,
            cache: RefCell::new(Cache::default()),
            tails: RefCell::new(Tails::default()),
            risk: RefCell::new(BTreeMap::new()),
            risk_stale: Cell::new(false),
        }))
    }

    /// `wiring` with every production port read from the deployment over `rows`, `artifacts`,
    /// `liquidity` and `broker`; its quote feed is the deployment's too, so the binding gate reads
    /// the quote the process hands the core. The deployment is handed back too, for the broker's
    /// answers to be read through ([`Named`]).
    pub fn wired(
        wiring: Wiring,
        scope: &AccountScope,
        rows: Rows,
        artifacts: Box<dyn ArtifactSource>,
        (liquidity, broker): (Box<dyn LiquidityRead>, Box<dyn BrokerFacts>),
    ) -> (Wiring, Self) {
        let deployment = Self::new(
            scope,
            Sources {
                rows,
                artifacts,
                quotes: wiring.quotes,
                liquidity,
                broker,
            },
        );
        let wired = Wiring {
            mandates: Box::new(deployment.clone()),
            instruments: Box::new(deployment.clone()),
            gate: Box::new(deployment.clone()),
            protections: Box::new(deployment.clone()),
            quotes: Box::new(deployment.clone()),
            marked: Box::new(deployment.clone()),
            risk: Box::new(deployment.clone()),
            ..wiring
        };
        (wired, deployment)
    }

    /// The broker's ticker of each instrument an exit, a fill or a mark reads, mapped to the
    /// asset id the deployment names it by: each followed agent's current deployment, else its
    /// last that read, so a held position's own orders and fills keep their asset id through a
    /// store outage (`AGENTS.md` rule 13). A ticker a current deployment names is that
    /// deployment's, over a stopped agent's last: the current ones are registered last.
    pub fn symbols(&self) -> BrokerSymbols {
        BrokerSymbols::registered(self.lasting_all().into_iter().rev().filter_map(|resolved| {
            Some((
                InstrumentId::new(&resolved.symbol).ok()?,
                resolved.instrument.clone(),
            ))
        }))
    }

    /// `stream`'s whole records, read from its first: what the binding gate's fold of an opening
    /// reads, once per opening.
    fn rows(&self, stream: Option<&StreamId>) -> Option<Vec<StoredEvent>> {
        (self.0.sources.rows)(stream?, TrustedStart::GENESIS)
    }

    /// The control stream's records, read only past the head it was last read to.
    fn control_rows(&self) -> Option<Vec<StoredEvent>> {
        let stream = self.0.control.as_ref()?;
        let mut tails = self.0.tails.try_borrow_mut().ok()?;
        let tail = tails.control.get_or_insert_with(|| Tail::new(Vec::new()));
        tail.follow(&self.0.sources.rows, stream, Vec::new, |rows, row| {
            rows.push(row.clone());
        })
        .then(|| tail.state.clone())
    }

    /// The trade date of the scheduler's latest tick, the date a deployment is read for.
    fn today(&self) -> Option<Date> {
        Some(self.now()?.date())
    }

    /// Every agent the control stream deploys on this account whose deployment reads now, on
    /// `date`, or for a port that names none on the date of the latest read (the latest tick's at
    /// the first). The deployments are read again when the control stream has grown, when `date`
    /// is another, or when the last read failed for any agent; each one that reads is kept as
    /// that agent's last. A research agent's deployment is its version read so, and the
    /// instrument its surviving thesis holds at the scheduler's latest tick, replayed at every ask
    /// so a thesis that ends ends the agent's openings at once (DEC-2819 item 2).
    fn deployments(&self, date: Option<Date>) -> BTreeMap<AgentId, Rc<Resolved>> {
        let (mut current, research) = self.read_control(date);
        if research.is_empty() {
            return current;
        }
        let Some(now) = self.now() else {
            return current;
        };
        for (agent, research) in research {
            if let Some(resolved) = self.openable(&agent, &research, now) {
                current.insert(agent, resolved);
            }
        }
        current
    }

    /// The pinned deployments and the research agents' versions that read now, read as
    /// [`Self::deployments`] says.
    fn read_control(
        &self,
        date: Option<Date>,
    ) -> (
        BTreeMap<AgentId, Rc<Resolved>>,
        BTreeMap<AgentId, Rc<Research>>,
    ) {
        let none = || (BTreeMap::new(), BTreeMap::new());
        let Some(control) = self.control_rows() else {
            return none();
        };
        let Ok(mut cache) = self.0.cache.try_borrow_mut() else {
            return none();
        };
        let same_control = cache.control == Some(control.len());
        let same_date = date.is_none_or(|day| cache.date == Some(day));
        if cache.read && same_control && same_date {
            return (cache.current.clone(), cache.research.clone());
        }
        let Some(day) = date.or(cache.date).or_else(|| self.today()) else {
            return none();
        };
        let agents = agents_of(&control, &self.0.scope, Environment::Paper)
            .map(|following| following.agents);
        let records: Vec<ControlRecord> =
            control.iter().filter_map(ControlRecord::of_row).collect();
        let every_row = records.len() == control.len();
        let mut current = BTreeMap::new();
        let mut research = BTreeMap::new();
        let mut read = every_row && agents.is_ok();
        if let Ok(agents) = &agents {
            cache.followed = agents.keys().cloned().collect();
        }
        for agent in agents.unwrap_or_default().into_keys() {
            match self.read_agent(&records, &agent, day) {
                Some(Read::Pinned(resolved)) => {
                    let resolved = Rc::new(*resolved);
                    cache.last.insert(agent.clone(), Rc::clone(&resolved));
                    if every_row {
                        current.insert(agent, resolved);
                    }
                }
                Some(Read::Research(read_now)) => {
                    let read_now = Rc::new(*read_now);
                    cache
                        .research_last
                        .insert(agent.clone(), Rc::clone(&read_now));
                    if every_row {
                        research.insert(agent, read_now);
                    }
                }
                None => read = false,
            }
        }
        cache.control = Some(control.len());
        cache.date = Some(day);
        cache.read = read;
        cache.current.clone_from(&current);
        cache.research.clone_from(&research);
        (current, research)
    }

    /// `agent`'s deployment from the control `records` on `day`: its one pinned instrument's, or,
    /// for a research agent, its confirmed version and the model that admits its instruments.
    fn read_agent(&self, records: &[ControlRecord], agent: &AgentId, day: Date) -> Option<Read> {
        let confirmed = self.confirm(records, agent, day)?;
        if confirmed.mandate().universe.pinned {
            return self
                .resolve_pinned(records, agent, day, &confirmed)
                .map(|resolved| Read::Pinned(Box::new(resolved)));
        }
        let model = confirmed
            .mandate()
            .behavior
            .signal_models
            .iter()
            .find(|model| model.admits_instruments)?;
        Some(Read::Research(Box::new(Research {
            model_id: model.id.as_str().to_owned(),
            model_version: model.version.clone(),
            content_hash: model.content_hash,
            confirmed,
            records: records.to_vec(),
            day,
        })))
    }

    /// `agent`'s confirmed version from the control `records` on `day`, as the paper shell reads
    /// it: one user and one approver (DEC-505 item 3), no live admission.
    fn confirm(
        &self,
        records: &[ControlRecord],
        agent: &AgentId,
        day: Date,
    ) -> Option<ConfirmedVersion> {
        let run = RunFacts {
            agent: mandate_spec::context::AgentId::new(&agent.0),
            validation_date: day,
            membership: Membership {
                workspace_users: 1,
                approver_users: 1,
            },
            live: LiveAdmission::refusing(),
        };
        confirmed_version(records, self.0.sources.artifacts.as_ref(), &run).ok()
    }

    /// The scheduler's latest tick, the instant a research agent's theses are replayed at: the
    /// clock stream folded only past the head it was last read to.
    fn now(&self) -> Option<UtcNanos> {
        let stream = self.0.clock.as_ref()?;
        let fresh = || Journaled::new("", stream.as_str(), "");
        let mut tails = self.0.tails.try_borrow_mut().ok()?;
        let tail = tails.clock.get_or_insert_with(|| Tail::new(fresh()));
        if !tail.follow(&self.0.sources.rows, stream, fresh, fold) {
            return None;
        }
        tail.state.now()
    }

    /// `agent`'s thesis records in stream order, each with its payload, read only past the head
    /// its stream was last read to; `None` when the stream cannot be read.
    fn theses(&self, agent: &AgentId) -> Option<Vec<Kept>> {
        let workspace = self.0.scope.workspace.0.as_str();
        let stream = StreamId::parse(&format!("agent:{workspace}:{}", agent.0))?;
        let mut tails = self.0.tails.try_borrow_mut().ok()?;
        let tail = tails
            .theses
            .entry(stream.as_str().to_owned())
            .or_insert_with(|| Tail::new(Vec::new()));
        tail.follow(&self.0.sources.rows, &stream, Vec::new, |kept, row| {
            if matches!(
                row.event_type.as_str(),
                "ThesisProposed" | "ThesisRevised" | "ThesisInvalidated"
            ) {
                let payload = parse(&row.body)
                    .ok()
                    .and_then(|body| body.get("payload").cloned());
                kept.push((row.event_type.clone(), payload));
            }
        })
        .then(|| tail.state.clone())
    }

    /// `agent`'s thesis records replayed at `at` ([`replay_theses`]); `None` when its stream or a
    /// record cannot be read.
    fn replayed(&self, agent: &AgentId, at: UtcNanos) -> Option<Replayed> {
        let kept = self.theses(agent)?;
        let records = kept.iter().map(|(event_type, payload)| {
            Ok(ThesisRecord {
                event_type: event_type.as_str(),
                payload: payload
                    .clone()
                    .ok_or(ReplayError::Unreadable { member: "payload" })?,
            })
        });
        replay_theses(records, at).ok()
    }

    /// The instrument `agent` may open in at `at`: the one its surviving thesis holds, resolved
    /// from the version that reads now. More than one surviving instrument, none, or an agent
    /// stream that cannot be read, answers none, so its openings are held.
    fn openable(&self, agent: &AgentId, research: &Research, at: UtcNanos) -> Option<Rc<Resolved>> {
        let replayed = self.replayed(agent, at)?;
        let [entry] = replayed.active.as_slice() else {
            return None;
        };
        let instrument = InstrumentId::new(entry.instrument.as_str()).ok()?;
        self.admitted(agent, research, &instrument, true)
    }

    /// Whether `agent` may open in `instrument` at the binding instant `at`: always for a pinned
    /// agent, whose deployment names it; for a research agent, only while the thesis holding it
    /// survives at `at`, so a thesis that ends between the scheduler's tick and the binding
    /// instant holds the opening.
    fn openable_at(&self, agent: &AgentId, instrument: &InstrumentId, at: UtcNanos) -> bool {
        let researching = match self.0.cache.try_borrow() {
            Ok(cache) => cache.research.contains_key(agent),
            Err(_) => return false,
        };
        if !researching {
            return true;
        }
        self.replayed(agent, at).is_some_and(|replayed| {
            matches!(replayed.active.as_slice(), [entry] if entry.instrument.as_str() == instrument.as_str())
        })
    }

    /// `instrument` as `agent`'s admission admitted it, resolved from `research`'s version: the
    /// resolution at that version's control length and trade date, read once and kept; with
    /// `fresh` false, else the last one that read, which an exit, a fill or a mark reads.
    fn admitted(
        &self,
        agent: &AgentId,
        research: &Research,
        instrument: &InstrumentId,
        fresh: bool,
    ) -> Option<Rc<Resolved>> {
        let key = (agent.clone(), instrument.clone());
        let at = (research.records.len(), research.day);
        let kept = self
            .0
            .cache
            .try_borrow()
            .ok()?
            .admitted
            .get(&key)
            .map(|(length, day, resolved)| ((*length, *day), Rc::clone(resolved)));
        if let Some((read_at, resolved)) = &kept
            && *read_at == at
        {
            return Some(Rc::clone(resolved));
        }
        let Some(resolved) = self.resolve_admitted(agent, research, instrument) else {
            return kept.filter(|_| !fresh).map(|(_, resolved)| resolved);
        };
        let resolved = Rc::new(resolved);
        self.0
            .cache
            .try_borrow_mut()
            .ok()?
            .admitted
            .insert(key, (at.0, at.1, Rc::clone(&resolved)));
        Some(resolved)
    }

    /// `instrument` resolved from `research`'s version, its own registered snapshot and the
    /// admitting model's registration (DEC-2819 item 3).
    fn resolve_admitted(
        &self,
        agent: &AgentId,
        research: &Research,
        instrument: &InstrumentId,
    ) -> Option<Resolved> {
        let admitted = Admitted {
            asset_id: instrument.as_str().to_owned(),
            model_id: research.model_id.clone(),
            model_version: research.model_version.clone(),
            content_hash: research.content_hash,
        };
        let store = self.0.sources.artifacts.as_ref();
        let configured =
            admitted_configuration(&research.records, store, &admitted, research.day).ok()?;
        let reviewed = Reviewed::from_admitted(&research.confirmed, &configured).ok()?;
        self.resolved(agent, &research.confirmed, &reviewed)
    }

    /// Every instrument a followed research agent's admission admitted, resolved for what an exit,
    /// a fill or a mark reads: from the version that reads now, else the last that read, and each
    /// one that cannot be resolved now as it last was.
    fn held(&self) -> Vec<Rc<Resolved>> {
        let researching: Vec<(AgentId, Rc<Research>)> = match self.0.cache.try_borrow() {
            Ok(cache) => cache
                .research_last
                .iter()
                .filter(|(agent, _)| cache.followed.contains(*agent))
                .map(|(agent, research)| (agent.clone(), Rc::clone(research)))
                .collect(),
            Err(_) => Vec::new(),
        };
        for (agent, research) in researching {
            for instrument in self.ever_admitted(&agent) {
                self.admitted(&agent, &research, &instrument, false);
            }
        }
        self.kept_admitted()
    }

    /// Every instrument a thesis record on `agent`'s stream admitted, for what an exit, a fill or a
    /// mark reads only: each `ThesisProposed` or `ThesisRevised` with `admitted` true and an
    /// instrument that reads, whatever any other record says. A record that cannot be read, which
    /// holds every opening, never takes from a held position the instrument an earlier or later
    /// record admitted, on a cold start too (`AGENTS.md` rule 13); naming one more instrument for
    /// exits adds no risk.
    fn ever_admitted(&self, agent: &AgentId) -> BTreeSet<InstrumentId> {
        let Some(kept) = self.theses(agent) else {
            return BTreeSet::new();
        };
        kept.iter()
            .filter(|(event_type, _)| {
                matches!(event_type.as_str(), "ThesisProposed" | "ThesisRevised")
            })
            .filter_map(|(_, payload)| {
                let payload = payload.as_ref()?;
                if payload.get("admitted") != Some(&Value::Bool(true)) {
                    return None;
                }
                InstrumentId::new(payload.get("instrument_id")?.as_str()?).ok()
            })
            .collect()
    }

    /// Every followed research agent's admitted instrument as it last resolved.
    fn kept_admitted(&self) -> Vec<Rc<Resolved>> {
        self.0
            .cache
            .try_borrow()
            .map(|cache| {
                cache
                    .admitted
                    .iter()
                    .filter(|((agent, _), _)| cache.followed.contains(agent))
                    .map(|(_, (_, _, resolved))| Rc::clone(resolved))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `agent`'s pinned deployment from the control `records` and the artifact store on `day`, of
    /// the version `confirmed` already read, as the paper shell reads it; `None` for a version
    /// that pins no one instrument and one model, so a research agent's version never reads as
    /// one instrument's here.
    fn resolve_pinned(
        &self,
        records: &[ControlRecord],
        agent: &AgentId,
        day: Date,
        confirmed: &ConfirmedVersion,
    ) -> Option<Resolved> {
        let store = self.0.sources.artifacts.as_ref();
        let mandate = confirmed.mandate();
        let ([instrument], [model]) = (
            mandate.universe.pinned_instruments.as_slice(),
            mandate.behavior.signal_models.as_slice(),
        ) else {
            return None;
        };
        let pinned = Pinned {
            asset_id: instrument.asset_id.as_str().to_owned(),
            symbol: instrument.symbol.clone(),
            model_id: model.id.as_str().to_owned(),
            model_version: model.version.clone(),
            content_hash: model.content_hash,
        };
        let configured = configuration(records, store, &pinned, day).ok()?;
        let reviewed = Reviewed::from_registered(confirmed, &configured).ok()?;
        self.resolved(agent, confirmed, &reviewed)
    }

    /// The ports' reading of `reviewed`, `agent`'s deployment of `confirmed`.
    fn resolved(
        &self,
        agent: &AgentId,
        confirmed: &ConfirmedVersion,
        reviewed: &Reviewed,
    ) -> Option<Resolved> {
        Some(Resolved {
            view: DeployedView::new(&agent.0, reviewed).ok()?,
            gate: DeployedGate::new(&agent.0, reviewed, self.0.sources.quotes.feed()).ok()?,
            version: reviewed.config_refs.mandate_version.clone()?,
            allocation: Usd::parse(reviewed.mandate.capital.allocation_usd.as_str()).ok()?,
            loss_carry: confirmed.connection_loss_carry(),
            symbol: reviewed.instrument.symbol.as_str().to_owned(),
            instrument: InstrumentId::new(reviewed.instrument.asset_id.as_str()).ok()?,
            asset_class: reviewed.instrument.asset_class,
            quote_max_age: reviewed.quote_max_age,
            confirmed: confirmed.clone(),
            pinned: confirmed.mandate().universe.pinned,
        })
    }

    /// `agent`'s deployment as it reads now, from every control row: what an opening reads.
    fn current(&self, agent: &AgentId) -> Option<Rc<Resolved>> {
        self.deployments(None).remove(agent)
    }

    /// `agent`'s deployment as it reads now, else its last that read: what an exit reads.
    fn lasting(&self, agent: &AgentId) -> Option<Rc<Resolved>> {
        self.current(agent)
            .or_else(|| self.last().remove(agent))
            .or_else(|| {
                let own = self.held();
                own.into_iter()
                    .find(|resolved| resolved.view.version(agent).is_some())
            })
    }

    /// Each followed agent's last deployment that read.
    fn last(&self) -> BTreeMap<AgentId, Rc<Resolved>> {
        self.0
            .cache
            .try_borrow()
            .map(|cache| {
                cache
                    .last
                    .iter()
                    .filter(|(agent, _)| cache.followed.contains(*agent))
                    .map(|(agent, resolved)| (agent.clone(), Rc::clone(resolved)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Every deployment an exit, a fill or a mark reads: each followed agent's current one, else
    /// its last, the current ones first, so a lookup by instrument prefers a deployment that reads
    /// now over one a stopped agent last read.
    fn lasting_all(&self) -> Vec<Rc<Resolved>> {
        let current = self.deployments(None);
        let last: Vec<Rc<Resolved>> = self
            .last()
            .into_iter()
            .filter(|(agent, _)| !current.contains_key(agent))
            .map(|(_, resolved)| resolved)
            .collect();
        current
            .into_values()
            .chain(last)
            .chain(self.held())
            .collect()
    }

    /// `answer` of the deployment whose instrument is `instrument`. Validation refuses a second
    /// agent's deployment on an instrument another agent's claims (mandate spec §6.4), so at most
    /// one followed deployment is on any instrument.
    fn of_instrument<T>(
        &self,
        instrument: &InstrumentId,
        answer: impl Fn(&DeployedView) -> Option<T>,
    ) -> Option<T> {
        self.lasting_all()
            .into_iter()
            .find(|resolved| &resolved.instrument == instrument)
            .and_then(|resolved| answer(&resolved.view))
    }

    /// The journal's fold for `agent` over the control, account and clock streams; `None` when a
    /// stream cannot be read.
    fn journaled(&self, agent: &AgentId) -> Option<Journaled> {
        let mut journaled = Journaled::new(
            self.0.account.as_ref()?.as_str(),
            self.0.clock.as_ref()?.as_str(),
            &agent.0,
        );
        for stream in [&self.0.control, &self.0.account, &self.0.clock] {
            for row in self.rows(stream.as_ref())? {
                fold(&mut journaled, &row);
            }
        }
        Some(journaled)
    }
}

/// Folds one stored row; a row whose body does not parse is folded with no payload, so one of a
/// type the fold reads holds every later opening.
fn fold(journaled: &mut Journaled, stored: &StoredEvent) {
    let payload = parse(&stored.body)
        .ok()
        .and_then(|body| body.get("payload").cloned())
        .unwrap_or(Value::Null);
    let row = Row {
        stream: &stored.stream_id,
        event_type: &stored.event_type,
        payload: &payload,
    };
    let recorded = UtcNanos::parse_rfc3339(&stored.recorded_at).ok();
    let _refused_rows_hold_every_later_opening = journaled.fold_recorded(row, recorded);
}

/// The agent's risk while nothing has traded on the account: its whole allocation is its equity,
/// its high-water mark, its day start and its capital base, its inherited loss is the connection's
/// loss carry its deployment was validated with (mandate spec §5.7, MI-14), nothing is latched or
/// restricted, and it has no exit, order, day trade or conduct counter yet. These are facts only
/// then (DEC-2380 item 2); once anything has traded the account risk view reads them, and until it
/// does the opening is held.
fn untraded(allocation: Usd, loss_carry: Usd) -> Option<AgentRisk> {
    Some(AgentRisk {
        risk: RiskSnapshot {
            agent_equity: allocation,
            high_water_mark: allocation,
            day_start_equity: allocation,
            capital_base: allocation,
            inherited_loss: loss_carry,
            latched: BTreeSet::new(),
            active_rungs: BTreeMap::new(),
            size_factor: Ratio::parse("1").ok()?,
            agent_mode: AgentMode::Normal,
        },
        conduct: ConductState::default(),
        instrument_restrictions: BTreeMap::new(),
        last_exit_fill_at: BTreeMap::new(),
        orders_today: 0,
        day_trades: DayTradeLedger::default(),
    })
}

/// The sane quote an observation carries, stamped at its risk-clock second; none for one that is
/// not sane or lacks a side.
fn sane(observation: &MarketObservation) -> Option<SaneQuote> {
    if !observation.sane {
        return None;
    }
    Some(SaneQuote {
        bid: observation.bid?,
        ask: observation.ask?,
        at: UtcNanos::from_parts(observation.observed_at.secs(), 0).ok()?,
    })
}

impl MandateView for Deployment {
    fn version(&self, agent: &AgentId) -> Option<MandateVersion> {
        self.lasting(agent)?.view.version(agent)
    }

    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<Fraction> {
        self.lasting(agent)?.view.crypto_stop_limit_offset(agent)
    }

    /// Whether `agent`'s deployment, as it reads now, covers `instrument`: an opening's question.
    fn covers(&self, agent: &AgentId, instrument: &InstrumentId) -> bool {
        self.current(agent)
            .is_some_and(|resolved| resolved.view.covers(agent, instrument))
    }
}

impl InstrumentSnapshot for Deployment {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        self.of_instrument(instrument, |view| view.asset_class(instrument))
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        self.of_instrument(instrument, |view| view.increment(instrument))
    }

    /// No reviewed instrument carries an exit tier yet ([`DeployedView`] answers none), so an
    /// exit with no limit takes the table's wider tier (DEC-2037 item 4).
    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
        None
    }

    fn fill_refs(&self, instrument: &InstrumentId) -> Option<FillConfigRefs> {
        self.of_instrument(instrument, |view| view.fill_refs(instrument))
    }

    /// The fill configuration every followed deployment of the account names alike, which an
    /// unattributed fill in an instrument no deployment holds is accounted under (DEC-2817). None
    /// when no deployment reads, or when two name different ones: the executor cannot say which the
    /// account's is, so it journals none, and the journal refuses the fill as before (DEC-2805).
    fn account_fill_refs(&self) -> Option<AccountFillRefs> {
        agreed(
            self.lasting_all()
                .into_iter()
                .map(|resolved| resolved.view.account_fill_refs()),
        )
    }
}

/// The one value every answer names alike: `None` when there is no answer, when any answer is
/// `None`, or when two differ (DEC-2817).
fn agreed<T: PartialEq>(mut answers: impl Iterator<Item = Option<T>>) -> Option<T> {
    let first = answers.next()??;
    answers
        .all(|answer| answer.as_ref() == Some(&first))
        .then_some(first)
}

impl Protections for Deployment {
    /// The protection the deployed agent's opening on its instrument is handed with. An opening
    /// the deployment does not cover, or one of an agent whose deployment does not read now, is
    /// handed none: the binding gate holds it all the same, and the process goes on with its exits.
    /// A covered opening whose protection cannot be read is [`Unplanned`] (TI-4).
    fn protection(
        &self,
        agent: &AgentId,
        instrument: &InstrumentId,
        entry: Price,
    ) -> Result<Option<ProtectionPrices>, Unplanned> {
        let Some(resolved) = self.current(agent) else {
            return Ok(None);
        };
        if !resolved.view.covers(agent, instrument) {
            return Ok(None);
        }
        resolved
            .view
            .protection(agent, instrument, entry)
            .map_err(|_| Unplanned)
    }
}

impl Marked for Deployment {
    /// Each deployment's instrument, as it reads now or as it last read, so a held position stays
    /// marked through an outage (DEC-2380 item 3).
    fn instruments(&self) -> BTreeSet<InstrumentId> {
        self.lasting_all()
            .into_iter()
            .map(|resolved| resolved.instrument.clone())
            .collect()
    }
}

impl QuoteFeed for Deployment {
    /// The broker's quote of a deployed instrument, asked by its symbol, since the executor names
    /// an instrument by its asset id and the broker quotes by symbol, and observed under its asset
    /// id again. An instrument no deployment names is asked as named.
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        let deployed = self
            .lasting_all()
            .into_iter()
            .find(|resolved| &resolved.instrument == instrument);
        let Some(resolved) = deployed else {
            return self.0.sources.quotes.observe(instrument, now);
        };
        let symbol = InstrumentId::new(&resolved.symbol).map_err(|_| "symbol_unreadable")?;
        let observed = self.0.sources.quotes.observe(&symbol, now)?;
        Ok(MarketObservation {
            instrument: instrument.clone(),
            ..observed
        })
    }

    fn feed(&self) -> &'static str {
        self.0.sources.quotes.feed()
    }
}

impl BindingGateSource for Deployment {
    /// The binding gate's input for the deployed agent's opening, read now. An order that reduces
    /// risk is answered `None` before anything is read, so the core keeps its own decision
    /// (`AGENTS.md` rule 13, DEC-2274 item 6). An opening is held, `None`, while its deployment,
    /// the journal's facts, the quote, the broker's facts or the bars are unread, while the
    /// version the journal has in force is not the one the deployment reads, or once anything has
    /// traded on the account: until then the account is flat with no order working and the
    /// agent's risk is its allocation, and after it the account risk view must read them.
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if !request.purpose.adds_risk() {
            return None;
        }
        let journaled = self.journaled(request.agent)?;
        let facts = journaled.facts(request.instrument.as_str(), FRESH).ok()?;
        let resolved = self
            .deployments(Some(facts.now.date()))
            .remove(request.agent)?;
        if facts.mandate_version != resolved.version
            || !resolved.view.covers(request.agent, request.instrument)
            || journaled.opening_hold(resolved.pinned).is_some()
            || !self.openable_at(request.agent, request.instrument, facts.now)
        {
            return None;
        }
        let agent = if journaled.untraded() {
            untraded(resolved.allocation, resolved.loss_carry)
        } else {
            self.traded(request.agent, &journaled, facts.now)
        };
        let sources = &self.0.sources;
        let quote = sane(&self.observe(request.instrument, facts.now).ok()?)?;
        let order = OrderFacts {
            quote,
            asset: sources.broker.asset(&resolved.symbol),
            rules: sources.broker.rules(),
            liquidity: sources.liquidity.liquidity(&resolved.symbol, facts.now),
            agent,
            halted: sources.broker.halted(&resolved.symbol),
            flat: true,
            journaled: facts,
        };
        resolved.gate.input(&order, request)
    }
}

impl Deployment {
    /// `agent`'s risk on a traded account that is flat again (DEC-2833 items 1 to 3), which
    /// `Journaled::opening_hold` has already found flat: the account risk book's snapshot, whose
    /// mode is the book's, and restrictions, with the counters `journaled` folds from the account stream at `now`. `None`
    /// while the counters do not read, or the book reads no risk for the agent, which holds the
    /// opening and never an exit. The day-trade ledger is empty: the gate answers only where day
    /// trades cannot bind (item 5).
    fn traded(&self, agent: &AgentId, journaled: &Journaled, now: UtcNanos) -> Option<AgentRisk> {
        let counted = journaled.counted(now)?;
        let GateRisk {
            risk,
            instrument_restrictions,
            ..
        } = self.risk(agent)?;
        Some(AgentRisk {
            risk,
            conduct: counted.conduct,
            instrument_restrictions,
            last_exit_fill_at: counted.last_exit_fill_at,
            orders_today: counted.orders_today,
            day_trades: DayTradeLedger::default(),
        })
    }

    /// `agent`'s risk as the process's risk book last read it: `None` while its fold is not open
    /// or its view is unreadable, which holds its openings and never its exits (DEC-2804 items 2
    /// and 7).
    pub fn risk(&self, agent: &AgentId) -> Option<GateRisk> {
        if self.0.risk_stale.get() {
            return None;
        }
        self.0.risk.try_borrow().ok()?.get(agent).cloned()
    }
}

impl RiskOpenings for Deployment {
    /// `agent`'s deployment as `control` reads it (the rows up to its first deployment), on
    /// `date`, its version validated on `equity` (DEC-2804 item 2).
    fn opening(
        &self,
        agent: &AgentId,
        control: &[StoredEvent],
        equity: Usd,
        date: Date,
    ) -> Option<RiskOpening> {
        let records: Vec<ControlRecord> =
            control.iter().filter_map(ControlRecord::of_row).collect();
        if records.len() != control.len() {
            return None;
        }
        let confirmed = self.confirm(&records, agent, date)?;
        let resolved = self.resolve_pinned(&records, agent, date, &confirmed)?;
        let input = resolved.confirmed.clone().with_equity(equity).ok()?;
        Some(RiskOpening {
            mandate: ValidatedMandate::new(input.mandate().clone(), input.context(), &[]).ok()?,
            instrument: resolved.instrument.as_str().to_owned(),
            asset_class: resolved.asset_class,
            inherited_loss_usd: resolved.loss_carry,
            mark_max_age_s: u32::try_from(resolved.quote_max_age.as_secs()).ok()?,
        })
    }

    /// Keeps `risks` for [`Self::risk`]. A hand-off that cannot be kept marks the map stale, and
    /// no agent's risk is answered until the next one is kept.
    fn read(&self, risks: BTreeMap<AgentId, GateRisk>) {
        match self.0.risk.try_borrow_mut() {
            Ok(mut held) => {
                *held = risks;
                self.0.risk_stale.set(false);
            }
            Err(_) => self.0.risk_stale.set(true),
        }
    }
}

/// A broker whose answers name each deployed instrument by its asset id wherever the broker named
/// it by its ticker, as the shell's paper run reads Alpaca's (DEC-938 item 1): the executor sends
/// the asset id, and Alpaca answers an order read back, a position and a `FILL` by the ticker, so
/// read as they come, the run's own fill would be another instrument's. A ticker no deployment
/// names stays as the broker spelled it ([DEC-2813](../../../docs/project/decisions/DEC-2813.md)).
pub struct Named<B> {
    broker: B,
    deployment: Option<Deployment>,
}

impl<B> Named<B> {
    /// `broker`, read through `deployment`'s tickers; through none when `None`.
    pub fn new(broker: B, deployment: Option<Deployment>) -> Self {
        Self { broker, deployment }
    }
}

impl<B: Broker> Broker for Named<B> {
    fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
        self.broker.profile()
    }

    fn call(&mut self, request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
        let answered = self.broker.call(request)?;
        Ok(match &self.deployment {
            Some(deployment) => deployment.symbols().outcome(answered),
            None => answered,
        })
    }

    fn declared(&self) -> Option<AccountDeclaration> {
        self.broker.declared()
    }

    fn has_later_checks(&self) -> bool {
        self.broker.has_later_checks()
    }

    fn later_check(
        &self,
        connection: ConnectionId,
        occasion: Occasion,
        account: AccountRead,
    ) -> Option<Result<CheckReport, &'static str>> {
        self.broker.later_check(connection, occasion, account)
    }
}

#[cfg(test)]
mod tests {
    use mandate_connections::checks::{AccountRead, CheckReport, Occasion};
    use mandate_connections::record::ConnectionId;
    use mandate_domain::{CapabilityProfile, ProfileError};
    use mandate_executor::{BrokerOutcome, BrokerRequest, ConnectorError};

    use super::{Named, agreed};
    use crate::Broker;

    /// A broker with later checks, each answering a refusal the test can recognise.
    struct Checking;

    impl Broker for Checking {
        fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
            mandate_alpaca::alpaca_profile()
        }

        fn call(&mut self, _request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
            Err(ConnectorError::NotSent { code: "fixture" })
        }

        fn has_later_checks(&self) -> bool {
            true
        }

        fn later_check(
            &self,
            _connection: ConnectionId,
            _occasion: Occasion,
            _account: AccountRead,
        ) -> Option<Result<CheckReport, &'static str>> {
            Some(Err("checked_by_the_inner_broker"))
        }
    }

    /// A broker that keeps every default.
    struct Plain;

    impl Broker for Plain {
        fn profile(&self) -> Result<CapabilityProfile, ProfileError> {
            mandate_alpaca::alpaca_profile()
        }

        fn call(&mut self, _request: &BrokerRequest) -> Result<BrokerOutcome, ConnectorError> {
            Err(ConnectorError::NotSent { code: "fixture" })
        }
    }

    /// DEC-2840: a broker whose later checks are not built runs none, and `Named` around it says
    /// so too, so the process never reads the account for one.
    #[test]
    fn a_broker_without_later_checks_runs_none_named_or_not() -> Result<(), String> {
        let connection = ConnectionId::new("conn-1").map_err(|e| format!("{e:?}"))?;
        assert!(!Plain.has_later_checks());
        assert_eq!(
            Plain.later_check(connection.clone(), Occasion::Daily, AccountRead::Unreadable),
            None
        );
        assert!(!Named::new(Plain, None).has_later_checks());
        Ok(())
    }

    /// DEC-2840: the binary wraps every broker in `Named`, so it passes the later checks through;
    /// otherwise the trait's default would answer none and the process would never run them.
    #[test]
    fn a_named_broker_runs_its_inner_broker_s_later_checks() -> Result<(), String> {
        let named = Named::new(Checking, None);
        assert!(named.has_later_checks());
        let connection = ConnectionId::new("conn-1").map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            named.later_check(connection, Occasion::Daily, AccountRead::Unreadable),
            Some(Err("checked_by_the_inner_broker"))
        );
        Ok(())
    }

    /// DEC-2817: the account's fill configuration is the one every deployment names alike; no
    /// deployment, one that names none, or two that differ name none.
    #[test]
    fn the_accounts_fill_configuration_is_the_one_every_deployment_names() {
        assert_eq!(agreed(std::iter::empty::<Option<u8>>()), None);
        assert_eq!(agreed([Some(1)].into_iter()), Some(1));
        assert_eq!(agreed([Some(1), Some(1), Some(1)].into_iter()), Some(1));
        assert_eq!(agreed([Some(1), Some(2)].into_iter()), None);
        assert_eq!(agreed([Some(1), Some(1), Some(2)].into_iter()), None);
        assert_eq!(agreed([None, Some(1)].into_iter()), None);
        assert_eq!(agreed([Some(1), None].into_iter()), None);
    }
}

/// DEC-2847: the hot ports read each stream only past the head they last read it to, and what they
/// fold so equals a fold of the whole stream at every point.
#[cfg(test)]
mod incremental {
    use std::cell::{Cell, RefCell};
    use std::collections::BTreeMap;
    use std::rc::Rc;

    use mandate_canon::{Digest, Value, parse};
    use mandate_executor::{AccountRef, AccountScope, AgentId, WorkspaceId};
    use mandate_journal::{StoredEvent, TrustedStart};
    use mandate_research::{ReplayError, ThesisRecord, replay_theses};
    use mandate_time::UtcNanos;
    use proptest::prelude::*;

    use super::{Deployment, Rows, Sources, Unread};
    use crate::quotes::NoQuotes;

    const CLOCK: &str = "clock:ws1";
    const AGENT: &str = "agent:ws1:demo-research";
    const T0: i64 = 1_793_106_000;

    /// The journal as the test appends it, and how many records every read has handed back.
    #[derive(Default)]
    struct Journal {
        streams: RefCell<BTreeMap<String, Vec<StoredEvent>>>,
        read: Cell<usize>,
        /// Refuse the next read past a held head, as a store does whose chain no longer follows it.
        refuse_next: Cell<bool>,
    }

    impl Journal {
        fn append(&self, stream: &str, event_type: &str, payload: &str) {
            let mut streams = self.streams.borrow_mut();
            let rows = streams.entry(stream.to_owned()).or_default();
            let seq = u64::try_from(rows.len()).unwrap() + 1;
            let prev_hash = rows.last().map_or(Digest::ZERO, |row| row.hash);
            let body = format!(r#"{{"event_type":"{event_type}","payload":{payload}}}"#);
            rows.push(StoredEvent {
                stream_id: stream.to_owned(),
                seq,
                event_id: format!("{stream}-{seq}"),
                event_type: event_type.to_owned(),
                schema_version: 1,
                environment: "paper".to_owned(),
                recorded_at: "2026-10-27T14:00:00.000000000Z".to_owned(),
                prev_hash,
                hash: Digest::of(format!("{stream}{seq}{body}").as_bytes()),
                body: body.into_bytes(),
            });
        }

        fn tick(&self, secs: i64) {
            let clock = UtcNanos::from_parts(T0 + secs, 0).unwrap();
            self.append(CLOCK, "ClockAdvanced", &format!(r#"{{"clock":"{clock}"}}"#));
        }

        fn rows(self: &Rc<Self>) -> Rows {
            let journal = Rc::clone(self);
            Box::new(move |stream, start: TrustedStart| {
                if start.from_seq > 1 && journal.refuse_next.replace(false) {
                    return None;
                }
                let streams = journal.streams.borrow();
                let since: Vec<StoredEvent> = streams
                    .get(stream.as_str())
                    .map(|rows| {
                        rows.iter()
                            .filter(|row| row.seq >= start.from_seq)
                            .cloned()
                            .collect()
                    })
                    .unwrap_or_default();
                if since
                    .first()
                    .is_some_and(|first| first.prev_hash != start.prev_hash)
                {
                    return None;
                }
                journal.read.set(journal.read.get() + since.len());
                Some(since)
            })
        }

        fn all(&self, stream: &str) -> Vec<StoredEvent> {
            self.streams
                .borrow()
                .get(stream)
                .cloned()
                .unwrap_or_default()
        }
    }

    fn deployment(journal: &Rc<Journal>) -> Deployment {
        Deployment::new(
            &AccountScope {
                account: AccountRef("01KDER4814371439000000000A".to_owned()),
                workspace: WorkspaceId("ws1".to_owned()),
            },
            Sources {
                rows: journal.rows(),
                artifacts: Box::new(BTreeMap::<Digest, Vec<u8>>::new()),
                quotes: Box::new(NoQuotes),
                liquidity: Box::new(Unread),
                broker: Box::new(Unread),
            },
        )
    }

    fn agent() -> AgentId {
        AgentId("demo-research".to_owned())
    }

    /// The oracle of the clock: the latest tick a `ClockAdvanced` names, read from the whole stream.
    fn latest_tick(journal: &Journal) -> Option<UtcNanos> {
        journal.all(CLOCK).iter().rev().find_map(|row| {
            let body = parse(&row.body).ok()?;
            UtcNanos::parse_rfc3339(body.get("payload")?.get("clock")?.as_str()?).ok()
        })
    }

    /// The oracle of the theses: every thesis record of the whole agent stream, in order.
    fn all_theses(journal: &Journal) -> Vec<(String, Option<Value>)> {
        journal
            .all(AGENT)
            .iter()
            .filter(|row| {
                matches!(
                    row.event_type.as_str(),
                    "ThesisProposed" | "ThesisRevised" | "ThesisInvalidated"
                )
            })
            .map(|row| {
                let payload = parse(&row.body)
                    .ok()
                    .and_then(|body| body.get("payload").cloned());
                (row.event_type.clone(), payload)
            })
            .collect()
    }

    fn thesis(n: u64) -> String {
        format!(
            r#"{{"thesis_id":"t{n}","lineage_id":"t{n}","revision":0,"instrument_id":"i{n}","admitted":true,"reason":null}}"#
        )
    }

    #[test]
    fn a_long_run_reads_each_record_once() {
        let journal = Rc::new(Journal::default());
        let deployment = deployment(&journal);
        let mut appended = 0;
        for secs in 0..10_000 {
            journal.tick(secs);
            appended += 1;
            if secs % 500 == 0 {
                journal.append(
                    AGENT,
                    "ThesisProposed",
                    &thesis(u64::try_from(secs).unwrap()),
                );
                appended += 1;
            }
            let before = journal.read.get();
            assert_eq!(
                deployment.now(),
                UtcNanos::from_parts(T0 + secs, 0).ok(),
                "at {secs}"
            );
            assert!(deployment.theses(&agent()).is_some());
            assert!(
                journal.read.get() - before <= 2,
                "a pass reads what was appended since, not the streams' length, at {secs}"
            );
        }
        assert_eq!(
            journal.read.get(),
            appended,
            "every record read exactly once"
        );
    }

    #[test]
    fn a_refused_read_past_the_head_is_read_again_from_the_first_record() {
        let journal = Rc::new(Journal::default());
        let deployment = deployment(&journal);
        journal.tick(0);
        journal.append(AGENT, "ThesisProposed", &thesis(1));
        assert_eq!(deployment.now(), latest_tick(&journal));
        assert_eq!(deployment.theses(&agent()), Some(all_theses(&journal)));
        journal.tick(1);
        journal.append(AGENT, "ThesisProposed", &thesis(2));
        journal.refuse_next.set(true);
        assert_eq!(
            deployment.now(),
            latest_tick(&journal),
            "refolded from the first record"
        );
        journal.refuse_next.set(true);
        assert_eq!(
            deployment.theses(&agent()),
            Some(all_theses(&journal)),
            "never folded twice"
        );
    }

    #[derive(Debug, Clone)]
    enum Step {
        Ticks(u8),
        Thesis,
        Invalidated,
        Unparsable,
        Other,
        Refuse,
        Read,
    }

    fn step() -> impl Strategy<Value = Step> {
        prop_oneof![
            (1u8..20).prop_map(Step::Ticks),
            Just(Step::Thesis),
            Just(Step::Invalidated),
            Just(Step::Unparsable),
            Just(Step::Other),
            Just(Step::Refuse),
            Just(Step::Read),
        ]
    }

    proptest! {
        /// At every read, the incremental clock and thesis records equal a fold of the whole
        /// streams, and the replay over them equals `replay_theses` over the whole stream, so the
        /// research agent's working universe is the one DEC-2819 replays.
        #[test]
        fn the_incremental_fold_equals_a_fold_of_the_whole_streams(steps in prop::collection::vec(step(), 1..120)) {
            let journal = Rc::new(Journal::default());
            let deployment = deployment(&journal);
            let mut secs = 0;
            let mut theses = 0;
            for step in steps {
                match step {
                    Step::Ticks(n) => for _ in 0..n {
                        journal.tick(secs);
                        secs += 1;
                    },
                    Step::Thesis => {
                        theses += 1;
                        journal.append(AGENT, "ThesisProposed", &thesis(theses));
                    }
                    Step::Invalidated => journal.append(AGENT, "ThesisInvalidated", &format!(r#"{{"thesis_id":"t{theses}"}}"#)),
                    Step::Unparsable => journal.append(AGENT, "ThesisRevised", "not json"),
                    Step::Other => journal.append(AGENT, "DecisionMade", r#"{"x":1}"#),
                    Step::Refuse => journal.refuse_next.set(true),
                    Step::Read => {
                        prop_assert_eq!(deployment.now(), latest_tick(&journal));
                        let kept = deployment.theses(&agent());
                        prop_assert_eq!(kept.clone(), Some(all_theses(&journal)));
                        let at = UtcNanos::from_parts(T0 + secs, 0).unwrap();
                        let whole = all_theses(&journal);
                        let expected = replay_theses(whole.iter().map(|(event_type, payload)| Ok(ThesisRecord {
                            event_type: event_type.as_str(),
                            payload: payload.clone().ok_or(ReplayError::Unreadable { member: "payload" })?,
                        })), at).ok();
                        prop_assert_eq!(deployment.replayed(&agent(), at), expected);
                    }
                }
            }
        }
    }
}
