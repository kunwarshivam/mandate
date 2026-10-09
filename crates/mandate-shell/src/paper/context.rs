//! The two trusted contexts, assembled from the judged snapshot and nothing else: the executor's,
//! whose binding gate source answers only the reviewed agent on the reviewed instrument, and the
//! run's, whose builder and advisory gate contexts carry the same facts. The opening's
//! classification facts are the builder's [`buy_action`] and its protection is the executor's
//! [`equity_bracket_prices`], so no money or quantity is computed here (DEC-138 item 3).

use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, Market as BuilderMarket,
    RiskContext as BuilderRiskContext, buy_action,
};
use mandate_domain::{AssetClass as DomainAssetClass, MarketSession};
use mandate_executor::{
    BindingGateInput, BindingGateRequest, BindingGateSource, InstrumentSnapshot, MandateVersion,
    MandateView as ExecutorMandateView, equity_bracket_prices,
};
use mandate_num::{
    CostBasis, FeeRate, Fraction, MarkPrice, Price, Qty, ShareIncrement, Signed, SizeFraction,
    Unit, Usd,
};
use mandate_runtime::{AgentId, OrderExecution, ProtectionPrices, TimeInForce};
use mandate_spec::ValidationContext;
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

/// Judges `facts` at `now` and assembles the two trusted contexts from them.
///
/// Refused, in order: any broker position or open order (DEC-470 item 1); an account that is not
/// `ACTIVE`, is blocked or suspended, or owes accrued fees; an asset record older than the quote
/// bound, read after `now`, or naming another asset id, class, or exchange; an ETP classification
/// dated after `now`; a quote for another instrument, off the IEX feed, stamped after `now`, older
/// than the bound, or crossed; and a clock outside the New York regular session or inside its close
/// window.
///
/// # Errors
/// [`Cause::Absent`] naming the first fact that does not hold.
pub fn load_contexts(
    artifacts: &Artifacts,
    facts: &PaperFacts,
    now: UtcNanos,
    agent: &AgentId,
) -> Result<Contexts, Cause> {
    load_contexts_with_clock(artifacts, facts, now, agent, Rc::new(FixedPaperClock(now)))
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
    clock: Rc<dyn PaperClock>,
) -> Result<Contexts, Cause> {
    let configuration = artifacts.production_configuration()?;
    let config = configuration.gate.clone();
    let today = judge(artifacts, &facts.broker, now, &config)?;
    let template = gate_template(artifacts, facts, now, config)?;
    let source = Rc::new(TrustedPaperContext {
        template: template.clone(),
        agent: agent.0.clone(),
        increment: artifacts.instrument.increment,
        fees: artifacts.fees.clone(),
        quote_at: facts.broker.quote.at,
        quote_max_age: artifacts.quote_max_age,
        clock,
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
        run: run_context(artifacts, &facts.broker, today, template)?,
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

fn run_context(
    artifacts: &Artifacts,
    broker: &BrokerFacts,
    today: Date,
    template: BindingGateInput,
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
            connection_environment: Some(mandate_domain::Environment::Paper),
            connection_loss_carry_usd: Usd::ZERO,
            eligibility_failures: BTreeSet::new(),
            previous_version: None,
            current_mandate_version: None,
        },
        policies: Vec::new(),
        author: "founder".to_owned(),
        restricted_instruments: BTreeSet::new(),
        decision: Some(DecisionContext {
            builder: Some(builder),
            gate: Some(advisory_gate_context(template)),
        }),
        governance: None,
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
    pub(super) quote_at: UtcNanos,
    pub(super) quote_max_age: Duration,
    pub(super) clock: Rc<dyn PaperClock>,
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

    fn crypto_stop_limit_offset(&self, _agent: &mandate_executor::AgentId) -> Option<Fraction> {
        None
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
}

impl BindingGateSource for TrustedPaperContext {
    /// The template, with `mandate-accounting`'s fee reservation of the very order the executor
    /// asks about. An order it cannot price gets no input, so the gate refuses it.
    fn input(&self, request: &BindingGateRequest<'_>) -> Option<BindingGateInput> {
        if !self.reviewed(request.agent, request.instrument) {
            return None;
        }
        let now = self.clock.now()?;
        if let Err(error) = judge_submission_time(
            self.quote_at,
            self.quote_max_age,
            now,
            &self.template.config,
        ) {
            let _ = error;
            return None;
        }
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
                ..self.template.clone()
            }),
            Err(_) => None,
        }
    }
}
