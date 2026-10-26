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
//! Every port reads the store when it is asked, as the process reads its streams each pass, so
//! nothing is ever answered from rows the store no longer shows. A deployment the run cannot read
//! answers nothing: no instrument is covered, no protection is handed, nothing is marked, and
//! every opening is held (`AGENTS.md` rule 3). An exit is never answered and reads nothing
//! here, so nothing it might wait on can hold it (rule 13).

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::rc::Rc;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::{Value, parse};
use mandate_executor::{
    AccountScope, AgentId, BindingGateInput, BindingGateRequest, BindingGateSource, ExitTier,
    FillConfigRefs, InstrumentSnapshot, MandateVersion, MandateView, MarketObservation,
    ProtectionPrices,
};
use mandate_gate_context::{Freshness, Journaled, Row};
use mandate_gate_inputs::control::{
    ControlRecord, LiveAdmission, Pinned, RunFacts, configuration, confirmed_version,
};
use mandate_gate_inputs::{DeployedView, Reviewed};
use mandate_journal::{ArtifactSource, Environment, StoredEvent, StreamId};
use mandate_num::{Fraction, Price, Ratio, ShareIncrement, Usd};
use mandate_risk::{AgentMode, ConductState, DayTradeLedger, RiskSnapshot, SaneQuote};
use mandate_spec::context::Membership;
use mandate_time::{Date, UtcNanos};

use crate::follow::agents_of;
use crate::gate::{AccountRules, AgentRisk, AssetRecord, DeployedGate, Liquidity, OrderFacts};
use crate::marks::Marked;
use crate::quotes::QuoteFeed;
use crate::{Protections, Unplanned, Wiring};

/// How fresh, in risk-clock seconds, the journal's facts an opening reads must be: a mark within a
/// minute, as the executor marks each deployed instrument at every tick, and the account's
/// snapshot within an hour, as the runtime's dry run reads them (DEC-2378 item 4).
pub const FRESH: Freshness = Freshness {
    mark_max_age_s: 60,
    snapshot_max_age_s: 3600,
};

/// How a port reads one stream: the store's records from its first, `None` when the store cannot
/// answer.
pub type Rows = Box<dyn Fn(&StreamId) -> Option<Vec<StoredEvent>>>;

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

/// One followed agent's deployment, read.
struct Resolved {
    view: DeployedView,
    gate: DeployedGate,
    allocation: Usd,
    symbol: String,
    instrument: InstrumentId,
}

/// The deployments read at one control-stream length and trade date, so a port asked again within
/// a pass reads no artifact twice; a longer control stream or a new date reads them again.
#[derive(Default)]
struct Cache {
    control: usize,
    date: Option<Date>,
    resolved: BTreeMap<AgentId, Option<Rc<Resolved>>>,
}

struct Inner {
    scope: AccountScope,
    control: Option<StreamId>,
    account: Option<StreamId>,
    clock: Option<StreamId>,
    sources: Sources,
    cache: RefCell<Cache>,
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
        }))
    }

    /// `wiring` with every production port read from the deployment over `rows`, `artifacts`,
    /// `liquidity` and `broker`; its quote feed is the deployment's too, so the binding gate reads
    /// the quote the process hands the core.
    pub fn wired(
        wiring: Wiring,
        scope: &AccountScope,
        rows: Rows,
        artifacts: Box<dyn ArtifactSource>,
        (liquidity, broker): (Box<dyn LiquidityRead>, Box<dyn BrokerFacts>),
    ) -> Wiring {
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
        Wiring {
            mandates: Box::new(deployment.clone()),
            instruments: Box::new(deployment.clone()),
            gate: Box::new(deployment.clone()),
            protections: Box::new(deployment.clone()),
            quotes: Box::new(deployment.clone()),
            marked: Box::new(deployment),
            ..wiring
        }
    }

    fn rows(&self, stream: Option<&StreamId>) -> Option<Vec<StoredEvent>> {
        (self.0.sources.rows)(stream?)
    }

    /// The trade date of the scheduler's latest tick, the date a deployment is read for.
    fn today(&self) -> Option<Date> {
        let mut journaled = Journaled::new("", self.0.clock.as_ref()?.as_str(), "");
        for row in self.rows(self.0.clock.as_ref())? {
            fold(&mut journaled, &row);
        }
        Some(journaled.now()?.date())
    }

    /// Every agent the control stream deploys on this account, each with its deployment read on
    /// `date`, or on the latest tick's date when none is given; an agent whose deployment does not
    /// read is listed with none.
    fn deployments(&self, date: Option<Date>) -> BTreeMap<AgentId, Option<Rc<Resolved>>> {
        let Some(control) = self.rows(self.0.control.as_ref()) else {
            return BTreeMap::new();
        };
        let Ok(mut cache) = self.0.cache.try_borrow_mut() else {
            return BTreeMap::new();
        };
        let current = cache.control == control.len() && date.is_none_or(|d| cache.date == Some(d));
        if current && cache.date.is_some() {
            return cache.resolved.clone();
        }
        let day = date.or_else(|| self.today());
        let Some(day) = day else {
            return BTreeMap::new();
        };
        let agents = agents_of(&control, &self.0.scope, Environment::Paper)
            .map(|following| following.agents)
            .unwrap_or_default();
        let records: Option<Vec<ControlRecord>> =
            control.iter().map(ControlRecord::of_row).collect();
        let resolved = agents
            .into_keys()
            .map(|agent| {
                let read = records
                    .as_deref()
                    .and_then(|records| self.resolve(records, &agent, day))
                    .map(Rc::new);
                (agent, read)
            })
            .collect();
        *cache = Cache {
            control: control.len(),
            date: Some(day),
            resolved,
        };
        cache.resolved.clone()
    }

    /// `agent`'s deployment from the control `records` and the artifact store on `day`, as the
    /// paper shell reads it: one user and one approver (DEC-505 item 3), no live admission.
    fn resolve(&self, records: &[ControlRecord], agent: &AgentId, day: Date) -> Option<Resolved> {
        let store = self.0.sources.artifacts.as_ref();
        let run = RunFacts {
            agent: mandate_spec::context::AgentId::new(&agent.0),
            validation_date: day,
            membership: Membership {
                workspace_users: 1,
                approver_users: 1,
            },
            live: LiveAdmission::refusing(),
        };
        let confirmed = confirmed_version(records, store, &run).ok()?;
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
        let reviewed = Reviewed::from_registered(&confirmed, &configured).ok()?;
        Some(Resolved {
            view: DeployedView::new(&agent.0, &reviewed).ok()?,
            gate: DeployedGate::new(&agent.0, &reviewed, self.0.sources.quotes.feed()).ok()?,
            allocation: Usd::parse(reviewed.mandate.capital.allocation_usd.as_str()).ok()?,
            symbol: reviewed.instrument.symbol.as_str().to_owned(),
            instrument: InstrumentId::new(reviewed.instrument.asset_id.as_str()).ok()?,
        })
    }

    fn deployed(&self, agent: &AgentId) -> Option<Rc<Resolved>> {
        self.deployments(None).remove(agent).flatten()
    }

    /// The first read deployment whose instrument is `instrument`.
    fn of_instrument(&self, instrument: &InstrumentId) -> Option<Rc<Resolved>> {
        self.deployments(None)
            .into_values()
            .flatten()
            .find(|resolved| &resolved.instrument == instrument)
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
    let _refused_rows_hold_every_later_opening = journaled.fold(row);
}

/// The agent's risk while nothing has traded on the account: its whole allocation is its equity,
/// its high-water mark, its day start and its capital base, nothing is latched or restricted, and
/// it has no exit, order, day trade or conduct counter yet. These are facts only then (DEC-2380
/// item 2); once anything has traded the account risk view reads them, and until it does the
/// opening is held.
fn untraded(allocation: Usd) -> Option<AgentRisk> {
    Some(AgentRisk {
        risk: RiskSnapshot {
            agent_equity: allocation,
            high_water_mark: allocation,
            day_start_equity: allocation,
            capital_base: allocation,
            inherited_loss: Usd::ZERO,
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
        self.deployed(agent)?.view.version(agent)
    }

    fn crypto_stop_limit_offset(&self, agent: &AgentId) -> Option<Fraction> {
        self.deployed(agent)?.view.crypto_stop_limit_offset(agent)
    }

    fn covers(&self, agent: &AgentId, instrument: &InstrumentId) -> bool {
        self.deployed(agent)
            .is_some_and(|resolved| resolved.view.covers(agent, instrument))
    }
}

impl InstrumentSnapshot for Deployment {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        self.of_instrument(instrument)?.view.asset_class(instrument)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        self.of_instrument(instrument)?.view.increment(instrument)
    }

    fn exit_tier(&self, instrument: &InstrumentId) -> Option<ExitTier> {
        self.of_instrument(instrument)?.view.exit_tier(instrument)
    }

    fn fill_refs(&self, instrument: &InstrumentId) -> Option<FillConfigRefs> {
        self.of_instrument(instrument)?.view.fill_refs(instrument)
    }
}

impl Protections for Deployment {
    /// The protection the deployed agent's opening on its instrument is handed with. An opening
    /// the deployment does not cover, or one of an agent whose deployment does not read, is
    /// handed none: the binding gate holds it all the same, and the process goes on with its exits.
    /// A covered opening whose protection cannot be read is [`Unplanned`] (TI-4).
    fn protection(
        &self,
        agent: &AgentId,
        instrument: &InstrumentId,
        entry: Price,
    ) -> Result<Option<ProtectionPrices>, Unplanned> {
        let Some(resolved) = self.deployed(agent) else {
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
    /// Each read deployment's instrument (DEC-2380 item 3).
    fn instruments(&self) -> BTreeSet<InstrumentId> {
        self.deployments(None)
            .into_values()
            .flatten()
            .map(|resolved| resolved.instrument.clone())
            .collect()
    }
}

impl QuoteFeed for Deployment {
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        self.0.sources.quotes.observe(instrument, now)
    }

    fn feed(&self) -> &'static str {
        self.0.sources.quotes.feed()
    }
}

impl BindingGateSource for Deployment {
    /// The binding gate's input for the deployed agent's opening, read now. An order that reduces
    /// risk is answered `None` before anything is read, so the core keeps its own decision
    /// (`AGENTS.md` rule 13, DEC-2274 item 6). An opening is held, `None`, while its deployment,
    /// the journal's facts, the quote, the broker's facts or the bars are unread, or once the
    /// account has traded and the agent's risk is not read.
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if !request.purpose.adds_risk() {
            return None;
        }
        let journaled = self.journaled(request.agent)?;
        let facts = journaled.facts(request.instrument.as_str(), FRESH).ok()?;
        let resolved = self
            .deployments(Some(facts.now.date()))
            .remove(request.agent)
            .flatten()?;
        if !resolved.view.covers(request.agent, request.instrument) || !journaled.untraded() {
            return None;
        }
        let sources = &self.0.sources;
        let quote = sane(&sources.quotes.observe(request.instrument, facts.now).ok()?)?;
        let order = OrderFacts {
            quote,
            asset: sources.broker.asset(&resolved.symbol),
            rules: sources.broker.rules(),
            liquidity: sources.liquidity.liquidity(&resolved.symbol, facts.now),
            agent: untraded(resolved.allocation),
            halted: sources.broker.halted(&resolved.symbol),
            flat: journaled.working().is_empty(),
            journaled: facts,
        };
        resolved.gate.input(&order, request)
    }
}
