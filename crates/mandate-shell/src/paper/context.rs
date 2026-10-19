//! The two trusted contexts, assembled from the judged snapshot and nothing else: the executor's,
//! whose binding gate source answers only the reviewed agent on the reviewed instrument, and the
//! run's, whose builder and advisory gate contexts carry the same facts. The opening's
//! classification facts are the builder's [`buy_action`] and its protection is the executor's
//! [`equity_bracket_prices`], so no money or quantity is computed here (DEC-138 item 3).

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_alpaca::LatestQuote;
use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, Market as BuilderMarket,
    RiskContext as BuilderRiskContext, buy_action,
};
use mandate_domain::{AssetClass as DomainAssetClass, Environment, MarketSession};
use mandate_executor::{
    BindingGateConfigRefs, BindingGateInput, BindingGateRequest, BindingGateSource, FillConfigRefs,
    InstrumentSnapshot, MandateVersion, MandateView as ExecutorMandateView, equity_bracket_prices,
};
use mandate_num::{
    CostBasis, FeeRate, Fraction, MarkPrice, Price, Qty, ShareIncrement, Signed, SizeFraction,
    Unit, Usd,
};
use mandate_risk::SaneQuote;
use mandate_runtime::{AgentId, OrderExecution, ProtectionPrices, TimeInForce};
use mandate_spec::ValidationContext;
use mandate_spec::context::stop_limit_asset_classes;
use mandate_spec::document::{Protection, ProvenanceMap};
use mandate_time::{Date, UtcNanos};

use super::artifacts::Artifacts;
use super::facts::{BrokerFacts, PaperFacts};
use super::gate::{advisory_gate_context, gate_template, reservation};
use super::judge::{judge, judge_submission_time};
use super::{absent, usd};
use crate::adapters::{BuilderContext, DecisionContext, ExecutorContext, RunContext};
use crate::envelope::{IdSpace, Ids};
use crate::error::Cause;

/// The contexts the shipping binary must supply before it can read the broker or decide.
pub struct Contexts {
    pub executor: ExecutorContext,
    pub run: RunContext,
    /// The IEX quote the executor's binding gate judges, the preflight's until the run hands it
    /// a fresher read (DEC-943 item 2).
    pub quote: GateQuote,
}

/// The IEX quote the binding gate judges at each request, shared between the run that reads
/// quotes and the gate that judges them (DEC-943 item 2). It starts as the preflight's quote; an
/// asked opening's runner hands it the fresh quote each wake read, so a grant answered after
/// the preflight quote's bound is judged on a quote read at its own wake, never on the
/// preflight's. The gate's recheck is unchanged: the quote's age at the gate's own clock,
/// `iex_quote_max_age_s` (DEC-935 item 2), and a stamp after that clock refuses.
#[derive(Debug, Clone)]
pub struct GateQuote(Rc<Cell<SaneQuote>>);

impl GateQuote {
    pub(super) fn of(quote: &LatestQuote) -> Self {
        Self(Rc::new(Cell::new(sane(quote))))
    }

    /// Hands the binding gate `quote`, a fresh IEX read the run judged at its read: every
    /// binding request from now on judges it in place of the one held before.
    pub fn hand(&self, quote: &LatestQuote) {
        self.0.set(sane(quote));
    }

    fn current(&self) -> SaneQuote {
        self.0.get()
    }
}

/// The gate's view of an IEX quote: its bid, its ask, and its own stamp.
fn sane(quote: &LatestQuote) -> SaneQuote {
    SaneQuote {
        bid: quote.bid,
        ask: quote.ask,
        at: quote.at,
    }
}

/// The submission clock, injected so the binding gate rechecks freshness without reading a wall
/// clock in the library.
pub trait PaperClock {
    fn now(&self) -> Option<UtcNanos>;
}

struct FixedPaperClock(UtcNanos);

impl PaperClock for FixedPaperClock {
    fn now(&self) -> Option<UtcNanos> {
        Some(self.0)
    }
}

/// Judges `facts` at `now` and assembles the two trusted contexts from them. `connection` is the
/// environment of the connection the run chose (the live door's choice, `mandate-paper`'s
/// `chosen`), which the run context hands V-001 as the connection's environment, never a
/// constant: a mandate whose environment differs from it refuses at the cycle's validation
/// (DEC-505 item 3, DEC-965).
///
/// Refused, in order: any broker position or open order (DEC-470 item 1); an account that is not
/// `ACTIVE`, is blocked or suspended, or owes accrued fees; an asset record older than the quote
/// bound, read after `now`, or naming another asset id, class, or exchange; an ETP classification
/// dated after `now`; a quote for another instrument, off the IEX feed, stamped after `now`, older
/// than the bound, or crossed; a clock outside the New York regular session or inside its close
/// window; and, under a declared `intraday_margin` regime, a maintenance figure the executor refuses
/// or a reported maintenance deficit (DEC-840 item 4).
///
/// # Errors
/// [`Cause::Absent`] naming the first fact that does not hold.
pub fn load_contexts(
    artifacts: &Artifacts,
    facts: &PaperFacts,
    now: UtcNanos,
    agent: &AgentId,
    connection: Environment,
) -> Result<Contexts, Cause> {
    load_contexts_with_clock(
        artifacts,
        facts,
        now,
        agent,
        connection,
        Rc::new(FixedPaperClock(now)),
    )
}

/// [`load_contexts`] with a clock that the binding gate reads again for every order request.
///
/// # Errors
/// The same refusals as [`load_contexts`].
pub fn load_contexts_with_clock(
    artifacts: &Artifacts,
    facts: &PaperFacts,
    now: UtcNanos,
    agent: &AgentId,
    connection: Environment,
    clock: Rc<dyn PaperClock>,
) -> Result<Contexts, Cause> {
    let configuration = artifacts.production_configuration()?;
    let config = configuration.gate.clone();
    let today = judge(artifacts, &facts.broker, now, &config)?;
    let template = gate_template(artifacts, facts, now, config)?;
    let quote = GateQuote::of(&facts.broker.quote);
    let source = Rc::new(TrustedPaperContext {
        template: template.clone(),
        agent: agent.0.clone(),
        increment: artifacts.instrument.increment,
        fees: artifacts.fees.clone(),
        quote: quote.clone(),
        quote_max_age: artifacts.quote_max_age,
        clock,
        stop_limit_offset: stop_limit_offset(&artifacts.mandate.protection)?,
    });
    let executor = ExecutorContext::new(
        Rc::new(Ids {
            space: IdSpace::Account,
        }),
        source.clone(),
        source.clone(),
        source,
        configuration.executor,
        artifacts.fees.clone(),
    );
    Ok(Contexts {
        executor,
        run: run_context(artifacts, &facts.broker, today, template, connection)?,
        quote,
    })
}

/// The executor context of the own-fill reconciliation a run performs before its preflight is
/// judged (DEC-955 item 5): the reviewed agent's mandate version, the reviewed instrument's class,
/// increment and fill configuration, the registered fee configuration, and a binding gate that
/// answers no request, so nothing this context carries can ever let an order through.
///
/// # Errors
/// [`Cause::Absent`] when the registered configuration cannot be read.
pub fn reconcile_context(artifacts: &Artifacts, agent: &AgentId) -> Result<ExecutorContext, Cause> {
    let configuration = artifacts.production_configuration()?;
    let source = Rc::new(ReconcilingPaperContext {
        agent: agent.0.clone(),
        asset: artifacts.instrument.asset_id.as_str().to_owned(),
        increment: artifacts.instrument.increment,
        config_refs: artifacts.config_refs.clone(),
    });
    Ok(ExecutorContext::new(
        Rc::new(Ids {
            space: IdSpace::Account,
        }),
        source.clone(),
        source.clone(),
        source,
        configuration.executor,
        artifacts.fees.clone(),
    ))
}

/// [`reconcile_context`]'s view: the reviewed agent on the reviewed instrument, as
/// [`TrustedPaperContext`] answers them, with no market fact and a binding gate that answers no
/// request (DEC-955 item 5).
pub(super) struct ReconcilingPaperContext {
    pub(super) agent: String,
    pub(super) asset: String,
    pub(super) increment: ShareIncrement,
    pub(super) config_refs: BindingGateConfigRefs,
}

impl ReconcilingPaperContext {
    fn reviewed(&self, agent: &mandate_executor::AgentId, instrument: &InstrumentId) -> bool {
        agent.0 == self.agent && instrument.as_str() == self.asset
    }
}

impl ExecutorMandateView for ReconcilingPaperContext {
    fn version(&self, agent: &mandate_executor::AgentId) -> Option<MandateVersion> {
        (agent.0 == self.agent)
            .then(|| self.config_refs.mandate_version.clone())
            .flatten()
            .map(MandateVersion)
    }

    fn crypto_stop_limit_offset(&self, _agent: &mandate_executor::AgentId) -> Option<Fraction> {
        None
    }

    fn covers(&self, agent: &mandate_executor::AgentId, instrument: &InstrumentId) -> bool {
        self.reviewed(agent, instrument)
    }
}

impl InstrumentSnapshot for ReconcilingPaperContext {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        (instrument.as_str() == self.asset).then_some(AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        (instrument.as_str() == self.asset).then_some(self.increment)
    }

    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<mandate_executor::ExitTier> {
        None
    }

    /// The registered fill configuration, as [`TrustedPaperContext`] names it (DEC-938 item 2).
    fn fill_refs(&self, instrument: &InstrumentId) -> Option<FillConfigRefs> {
        (instrument.as_str() == self.asset)
            .then(|| fill_refs(&self.config_refs))
            .flatten()
    }
}

impl BindingGateSource for ReconcilingPaperContext {
    /// No request is answered, so the binding gate refuses every order this context could be
    /// asked about: the own-fill reconciliation never sends one (DEC-955 item 5).
    fn input(&self, _request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        None
    }
}

/// The fill records' configuration from the registered references: the fee configuration, the
/// trading calendar (which also settles the paper run, DEC-938 item 2) and the instrument
/// snapshot, or none when any is missing, which the journal refuses.
fn fill_refs(refs: &BindingGateConfigRefs) -> Option<FillConfigRefs> {
    let calendar = refs.trading_calendar.clone()?;
    Some(FillConfigRefs {
        fee_config: refs.fee_config.clone()?,
        settlement_calendar: calendar.clone(),
        trading_calendar: calendar,
        instrument_snapshot: refs.instrument_snapshot.clone()?,
    })
}

/// The mandate's protection for an entry at `entry`, as the executor derives it, or none when the
/// mandate disables protection. Enabled protection without a stop distance is refused rather than
/// sent unprotected.
pub(super) fn protection_prices(
    protection: &Protection,
    entry: Price,
) -> Result<Option<ProtectionPrices>, Cause> {
    if !protection.enabled {
        return Ok(None);
    }
    let stop_distance = protection
        .stop_distance
        .as_ref()
        .ok_or_else(|| absent("the mandate's stop distance"))?;
    let take_profit_distance = protection
        .take_profit_distance
        .as_ref()
        .map(|distance| Fraction::parse(distance.as_str()))
        .transpose()?;
    let bracket = equity_bracket_prices(
        entry,
        Fraction::parse(stop_distance.as_str())?,
        take_profit_distance,
    )?;
    Ok(Some(ProtectionPrices {
        stop: bracket.stop,
        take_profit: bracket.take_profit,
    }))
}

/// The classes a stop-limit protects that the run's own validation reads (V-008, W-002; DEC-539
/// item 2, DEC-1152 item 2). A paper connection is Alpaca's, the only paper broker the default
/// build compiles (LT-14), so it reads Alpaca's row of the table. A live connection reads none: the
/// profile absent counts every class, which fails closed and is Robinhood's row for the classes v1
/// trades.
pub(super) fn run_stop_limit_classes(
    connection: Environment,
) -> Option<BTreeSet<DomainAssetClass>> {
    match connection {
        Environment::Paper => stop_limit_asset_classes("alpaca"),
        Environment::Live => None,
    }
}

/// The mandate's stop-limit offset when its protection is on and sets one; an offset that does
/// not read as a fraction refuses the run rather than leaving a stop-limit without its limit.
pub(super) fn stop_limit_offset(protection: &Protection) -> Result<Option<Fraction>, Cause> {
    let Some(offset) = protection
        .crypto_stop_limit_offset
        .as_ref()
        .filter(|_| protection.enabled)
    else {
        return Ok(None);
    };
    Ok(Some(Fraction::parse(offset.as_str())?))
}

fn run_context(
    artifacts: &Artifacts,
    broker: &BrokerFacts,
    today: Date,
    template: BindingGateInput,
    connection: Environment,
) -> Result<RunContext, Cause> {
    let instrument = artifacts.instrument.asset_id.clone();
    let quote = &broker.quote;
    let allocation = usd(artifacts.mandate.capital.allocation_usd.as_str())?;
    let protection = protection_prices(&artifacts.mandate.protection, quote.ask)?;
    let account = BuilderAccountSnapshot {
        agent_equity: allocation,
        position_qty: Qty::ZERO,
        cost_basis: CostBasis::ZERO,
        risk_mark: MarkPrice::from(quote.bid),
        gross_usd: Usd::ZERO,
        working_opening_cost: Usd::ZERO,
        goal_spent_usd: Usd::ZERO,
    };
    let market = BuilderMarket {
        instrument,
        asset_class: DomainAssetClass::UsEquity,
        session: MarketSession::Regular,
        in_close_window: false,
        bid: quote.bid,
        ask: quote.ask,
        increment: artifacts.instrument.increment_qty,
        min_order_usd: usd("1")?,
        fee_rate_cash: FeeRate::parse("0")?,
        fee_rate_asset: FeeRate::parse("0")?,
    };
    let risk = BuilderRiskContext {
        size_factor: SizeFraction::ONE,
        drawdown: Unit::ZERO,
        daily_pnl_fraction: Signed::ZERO,
        position_pnl_fraction: Signed::ZERO,
        bought_today_usd: Usd::ZERO,
        has_prior_fill: false,
        new_instrument: false,
        thesis_confidence: Unit::ZERO,
        risk_day: today,
    };
    let action = buy_action(
        &account,
        &market,
        &risk,
        Unit::ONE,
        artifacts.instrument.increment_qty,
    )?;
    let builder = BuilderContext {
        account,
        market,
        risk,
        action,
        model_content_hashes: BTreeMap::from([(
            (artifacts.model_id.clone(), artifacts.model_version.clone()),
            artifacts.model_hash,
        )]),
        execution: OrderExecution {
            asset_class: DomainAssetClass::UsEquity,
            tif: TimeInForce::Day,
            protection_required: protection.is_some(),
            protection,
        },
    };
    Ok(RunContext {
        validation: ValidationContext {
            account_equity_usd: broker.account.equity,
            other_allocations_usd: Usd::ZERO,
            validation_date: today,
            registry: None,
            provenance: ProvenanceMap::default(),
            workspace_users: 1,
            approver_users: 1,
            independent_approval_required: false,
            disclosures_accepted: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            claimed_by_other_agents: BTreeSet::new(),
            connection_environment: Some(connection),
            connection_loss_carry_usd: Usd::ZERO,
            eligibility_failures: BTreeSet::new(),
            previous_version: None,
            current_mandate_version: None,
            stop_limit_asset_classes: run_stop_limit_classes(connection),
        },
        policies: Vec::new(),
        author: "founder".to_owned(),
        members: artifacts.members.clone(),
        restricted_instruments: BTreeSet::new(),
        decision: Some(DecisionContext {
            builder: Some(builder),
            gate: Some({
                let (risk, mode) = (template.risk.clone(), template.agent.mode);
                advisory_gate_context(template, risk, mode)
            }),
        }),
        governance: None,
        live_cli_confirm: None,
    })
}

/// The executor's view of the one reviewed agent on the one reviewed instrument, answering every
/// binding gate request with the snapshot's template and every other agent or instrument with
/// nothing.
pub(super) struct TrustedPaperContext {
    pub(super) template: BindingGateInput,
    pub(super) agent: String,
    pub(super) increment: ShareIncrement,
    pub(super) fees: mandate_accounting::Config,
    pub(super) quote: GateQuote,
    pub(super) quote_max_age: Duration,
    pub(super) clock: Rc<dyn PaperClock>,
    /// The mandate's `stop_limit_offset`, the stop-limit's limit below its stop that protection
    /// places on a profile with no OCO (§5.4, DEC-539, DEC-966 item 5); none when it sets none.
    pub(super) stop_limit_offset: Option<Fraction>,
}

impl TrustedPaperContext {
    fn reviewed(&self, agent: &mandate_executor::AgentId, instrument: &InstrumentId) -> bool {
        agent.0 == self.agent && instrument.as_str() == self.template.asset.as_str()
    }
}

impl ExecutorMandateView for TrustedPaperContext {
    fn version(&self, agent: &mandate_executor::AgentId) -> Option<MandateVersion> {
        (agent.0 == self.agent)
            .then(|| self.template.config_refs.mandate_version.clone())
            .flatten()
            .map(MandateVersion)
    }

    fn crypto_stop_limit_offset(&self, agent: &mandate_executor::AgentId) -> Option<Fraction> {
        self.stop_limit_offset.filter(|_| agent.0 == self.agent)
    }

    fn covers(&self, agent: &mandate_executor::AgentId, instrument: &InstrumentId) -> bool {
        self.reviewed(agent, instrument)
    }
}

impl InstrumentSnapshot for TrustedPaperContext {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        (instrument.as_str() == self.template.asset.as_str()
            && self.template.instrument.asset_class == mandate_risk::AssetClass::UsEquity)
            .then_some(AssetClass::UsEquity)
    }

    fn increment(&self, instrument: &InstrumentId) -> Option<ShareIncrement> {
        (instrument.as_str() == self.template.asset.as_str()).then_some(self.increment)
    }

    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<mandate_executor::ExitTier> {
        None
    }

    /// The reviewed instrument's fills are accounted under the registered fee configuration,
    /// trading calendar and instrument snapshot the binding gate names, and settle on that same
    /// trading calendar, which is the one `mandate-accounting` computes settlement on here
    /// (DEC-938 item 2); any other instrument's fill names none, so the journal refuses it.
    fn fill_refs(&self, instrument: &InstrumentId) -> Option<FillConfigRefs> {
        if instrument.as_str() != self.template.asset.as_str() {
            return None;
        }
        fill_refs(&self.template.config_refs)
    }
}

impl BindingGateSource for TrustedPaperContext {
    /// The template, with the quote the run last handed the gate ([`GateQuote`], DEC-943 item 2)
    /// and `mandate-accounting`'s fee reservation of the very order the executor asks about. An
    /// order it cannot price gets no input, so the gate refuses it.
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if !self.reviewed(request.agent, request.instrument) {
            return None;
        }
        let now = self.clock.now()?;
        let quote = self.quote.current();
        if let Err(error) =
            judge_submission_time(quote.at, self.quote_max_age, now, &self.template.config)
        {
            let _ = error;
            return None;
        }
        let mut market = self.template.market.clone();
        market.quote = Some(quote);
        match reservation(
            &self.fees,
            request.instrument,
            request.side,
            request.qty,
            request.limit,
            now,
        ) {
            Ok(fee_reservation) => Some(BindingGateInput {
                now,
                fee_reservation,
                market,
                ..self.template.clone()
            }),
            Err(_) => None,
        }
    }
}
