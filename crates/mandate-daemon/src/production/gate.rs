//! The deployment's gate dry run over the journaled fold (DEC-2387): `mandate-ports`'s advisory
//! pass over a context built from [`Facts`], with DEC-2378 item 11's four checks deferred to the
//! executor's binding gate. It can only narrow: every opening it allows is still judged by the
//! binding gate on the account stream (`AGENTS.md` rules 1 and 12), and while any check is
//! deferred it allows none.

use std::collections::{BTreeMap, BTreeSet};

use mandate_gate_context::{Freshness, Journaled};
use mandate_num::{Qty, Ratio, Usd};
use mandate_ports::{AdvisoryGateContext, AdvisoryOrderFacts, evaluate_deferring, gate_mandate};
use mandate_risk::{
    AccountSnapshot, AccountState, AccountType, Advisory, AgentId as GateAgentId, AgentMode,
    AgentSnapshot, AssetId, ClientOrderId, ConductState, DayTradeLedger, DayTradeRegime, Deferred,
    EtpClass, GateConfig, GatePass, InstrumentSnapshot, MarketSnapshot, Origin, ProposedKind,
    RiskSnapshot, SaneQuote, TimeInForce, ValidatedMandate, Verdict, WorkingUniverse,
};
use mandate_runtime::{DryRunVerdict, GateDryRun, Proposal};
use mandate_spec::document::Mandate;

use super::Facts;

/// The checks deferred to the binding gate because nothing journals their facts yet (DEC-2378
/// item 11): each turns on, under condition 2, only once its fact is folded.
pub const UNJOURNALED: [Deferred; 4] = [
    Deferred::Eligibility,
    Deferred::LiquidityFloor,
    Deferred::Participation,
    Deferred::DayTradeBudget,
];

const DEFERRED_TO_BINDING: &str = "deferred_to_binding";
const FACTS_HELD: &str = "facts_held";
const GATE_REFUSED: &str = "gate_refused";
const RULE_SET_UNAVAILABLE: &str = "rule_set_unavailable";
const WORKING_ORDERS_UNSEEN: &str = "working_orders_unseen";

/// The dry run for one deployment: the shared fold, the stored mandate, the registered rule set's
/// gate configuration (`None` when none could be read, DEC-2387 item 3) and how fresh a mark and a
/// snapshot must be.
pub struct Gate {
    facts: Facts,
    mandate: Mandate,
    gate_mandate: Option<ValidatedMandate>,
    config: Option<GateConfig>,
    fresh: Freshness,
}

impl Gate {
    pub fn new(
        facts: Facts,
        mandate: Mandate,
        config: Option<GateConfig>,
        fresh: Freshness,
    ) -> Self {
        let gate_mandate = gate_mandate(&mandate).ok();
        Self {
            facts,
            mandate,
            gate_mandate,
            config,
            fresh,
        }
    }

    /// The advisory pass of the opening `proposal` over the fold, with [`UNJOURNALED`] deferred,
    /// or the reason it could not be evaluated: `rule_set_unavailable`, `facts_held` for a fact
    /// the fold holds, `working_orders_unseen` while any order of the agent's works, whose cost
    /// the fold cannot see, and `gate_refused` when the gate refuses to evaluate it.
    ///
    /// # Errors
    /// The reason code the dry run journals.
    pub fn advisory(&self, proposal: &Proposal) -> Result<Advisory, &'static str> {
        let config = self.config.clone().ok_or(RULE_SET_UNAVAILABLE)?;
        let mandate = self.gate_mandate.clone().ok_or(GATE_REFUSED)?;
        let context = self
            .facts
            .read(|journaled| self.context(journaled, proposal, config, mandate))
            .ok_or(FACTS_HELD)??;
        evaluate_deferring(proposal, &context, &BTreeSet::from(UNJOURNALED))
            .map_err(|_| GATE_REFUSED)
    }

    fn context(
        &self,
        journaled: &Journaled,
        proposal: &Proposal,
        config: GateConfig,
        mandate: ValidatedMandate,
    ) -> Result<AdvisoryGateContext, &'static str> {
        let instrument = proposal.instrument.as_str();
        let facts = journaled
            .facts(instrument, self.fresh)
            .map_err(|_| FACTS_HELD)?;
        if !journaled.working().is_empty() {
            return Err(WORKING_ORDERS_UNSEEN);
        }
        let asset = AssetId::new(instrument).map_err(|_| GATE_REFUSED)?;
        let mut positions = BTreeMap::new();
        let mut market_values = BTreeMap::new();
        for (held, qty) in journaled.held() {
            let bid = journaled
                .facts(&held, self.fresh)
                .map_err(|_| FACTS_HELD)?
                .mark
                .bid;
            let id = AssetId::new(&held).map_err(|_| GATE_REFUSED)?;
            market_values.insert(id.clone(), qty.notional(bid).map_err(|_| GATE_REFUSED)?);
            positions.insert(id, qty);
        }
        let account = &facts.account;
        let blocked = account.status != "ACTIVE"
            || account.trading_blocked
            || account.account_blocked
            || account.trade_suspended_by_user;
        let long_market_value = account
            .equity
            .checked_sub(account.cash)
            .map_err(|_| GATE_REFUSED)?
            .max(Usd::ZERO);
        let allocation = self
            .mandate
            .capital
            .allocation_usd
            .to_usd()
            .map_err(|_| GATE_REFUSED)?;
        let gate_agent = GateAgentId(1);
        let whole_share = Qty::parse("1").map_err(|_| GATE_REFUSED)?;
        Ok(AdvisoryGateContext {
            now: facts.now,
            pass: GatePass::First,
            config,
            mandate,
            risk: RiskSnapshot {
                agent_equity: allocation,
                high_water_mark: allocation,
                day_start_equity: allocation,
                capital_base: allocation,
                inherited_loss: Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: Ratio::parse("1").map_err(|_| GATE_REFUSED)?,
                agent_mode: AgentMode::Normal,
            },
            account: AccountSnapshot {
                account_type: if account.multiplier == 1 {
                    AccountType::Cash
                } else {
                    AccountType::Margin
                },
                state: if blocked {
                    AccountState::Blocked
                } else {
                    AccountState::Active
                },
                crypto_active: account.crypto_status == "ACTIVE",
                regime: DayTradeRegime::LegacyPdt,
                equity: account.equity,
                prior_close_equity: Usd::ZERO,
                model_buying_power: account.cash.min(account.non_marginable_buying_power),
                broker_buying_power: account.buying_power,
                broker_non_marginable_buying_power: account.non_marginable_buying_power,
                positions: positions.clone(),
                market_values: BTreeMap::from([(asset.clone(), long_market_value)]),
                working_orders: BTreeMap::new(),
                unknown_orders: BTreeSet::new(),
                related_account_resting: BTreeMap::new(),
            },
            agent: AgentSnapshot {
                agent: gate_agent,
                mode: AgentMode::Normal,
                instrument_restrictions: BTreeMap::new(),
                positions,
                market_values,
                working_orders: BTreeSet::new(),
                instrument_groups: BTreeMap::new(),
                last_exit_fill_at: BTreeMap::new(),
                orders_today: 0,
                day_trades: DayTradeLedger::default(),
            },
            instrument: InstrumentSnapshot {
                instrument: asset.clone(),
                asset_class: proposal.asset_class,
                exchange: None,
                status_active: false,
                tradable: false,
                fractionable: false,
                ipo: true,
                ptp_no_exception: true,
                etp: EtpClass::Unclassified,
                etp_classified_at: None,
                quote_currency: None,
                prior_close: None,
                median_dollar_volume_20d: None,
                median_dollar_volume_30d: None,
                min_order_size: whole_share,
                qty_increment: whole_share,
                halted: false,
                status_feed_current: false,
            },
            market: MarketSnapshot {
                quote: Some(SaneQuote {
                    bid: facts.mark.bid,
                    ask: facts.mark.bid,
                    at: facts.mark.at,
                }),
                last_trade: None,
                trailing_5m_volume: None,
                adv_20d: None,
            },
            conduct: ConductState::default(),
            universe: WorkingUniverse::Known {
                instruments: self
                    .mandate
                    .universe
                    .pinned_instruments
                    .iter()
                    .filter_map(|pinned| AssetId::new(pinned.asset_id.as_str()).ok())
                    .collect(),
                pinned: true,
            },
            order: AdvisoryOrderFacts {
                kind: ProposedKind::Plain,
                tif: TimeInForce::Day,
                extended_hours: false,
                origin: Origin::OrderBuilder,
                owner_confirmed_bid: None,
                client_order_id: ClientOrderId(1),
                fee_reservation: Usd::ZERO,
            },
        })
    }
}

/// The dry run's answer to an advisory pass: an allow only when the gate allows and nothing was
/// deferred; any deferral denies `deferred_to_binding` until `DecisionMade` can name it (DEC-2387
/// item 6); a gate denial or hold carries the gate's reason code; anything else is denied with the
/// code the pass gave.
#[must_use]
pub fn verdict(advisory: Result<Advisory, &'static str>) -> DryRunVerdict {
    let reason_code = match advisory {
        Ok(advisory) => match (advisory.decision.verdict, advisory.decision.reason) {
            (Verdict::Allow, _) if advisory.deferred.is_empty() => return DryRunVerdict::Allow,
            (Verdict::Allow, _) => DEFERRED_TO_BINDING,
            (_, Some(reason)) => reason.as_str(),
            (_, None) => GATE_REFUSED,
        },
        Err(code) => code,
    };
    DryRunVerdict::Deny {
        reason_code: reason_code.to_owned(),
    }
}

impl GateDryRun for Gate {
    /// An opening or an increase is the advisory pass's [`verdict`]; any other purpose is the
    /// binding gate's alone, so the dry run never denies an exit (`AGENTS.md` rule 13).
    fn check(&self, proposal: &Proposal) -> DryRunVerdict {
        if !proposal.purpose.adds_risk() {
            return DryRunVerdict::Allow;
        }
        verdict(self.advisory(proposal))
    }
}
