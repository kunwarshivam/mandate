//! The deployment's gate dry run over the journaled fold (DEC-2388): `mandate-ports`'s advisory
//! pass over a context built from [`Facts`], with DEC-2378 item 11's four checks and DEC-2388
//! item 8's unseen parts deferred to the executor's binding gate. It can only narrow: every
//! opening it allows is still judged by the binding gate on the account stream (`AGENTS.md`
//! rules 1 and 12), and an allow with any check deferred names each, so the runtime asks the owner
//! and never reports one passed (DEC-2388 item 13).

use std::collections::{BTreeMap, BTreeSet};

use mandate_accounting::AssetClass;
use mandate_bars::LiquidityFacts;
use mandate_gate_context::{Freshness, Journaled};
use mandate_num::{Qty, Ratio, Usd};
use mandate_ports::{AdvisoryGateContext, AdvisoryOrderFacts, evaluate_deferring, gate_mandate};
use mandate_risk::{
    AccountSnapshot, AccountState, AccountType, Advisory, AgentId as GateAgentId, AgentMode,
    AgentSnapshot, AssetId, ClientOrderId, ConductState, DayTradeLedger, DayTradeRegime, Deferred,
    EtpClass, GateConfig, GatePass, InstrumentSnapshot, MarketSnapshot, Origin, ProposedKind,
    Purpose as GatePurpose, RiskSnapshot, SaneQuote, TimeInForce, ValidatedMandate, Verdict,
    WorkingUniverse,
};
use mandate_runtime::{
    Deferral, DryRunVerdict, GateDryRun, LiquidityRead, Proposal, Purpose as RuntimePurpose,
};
use mandate_spec::document::Mandate;
use mandate_time::UtcNanos;

use super::Facts;
use crate::bars::{BarsReader, liquidity_deferred};

/// The checks deferred to the binding gate because nothing journals their facts yet (DEC-2378
/// item 11): each turns on, under condition 2, only once its fact is folded.
pub const UNJOURNALED: [Deferred; 4] = [
    Deferred::Eligibility,
    Deferred::LiquidityFloor,
    Deferred::Participation,
    Deferred::DayTradeBudget,
];

/// The parts of the gate's checks whose facts the fold does not carry (DEC-2388 items 7 and 8):
/// evaluated on what the context holds, so a refusal still refuses, and named, so none is ever
/// reported passed. Each leaves this set only once its fact is folded from the journal.
pub const UNSEEN: [Deferred; 10] = [
    Deferred::AgentMode,
    Deferred::AgentEquity,
    Deferred::ReentryCooldown,
    Deferred::Halt,
    Deferred::OtherOrders,
    Deferred::QuoteAsk,
    Deferred::ConductHistory,
    Deferred::RelatedAccounts,
    Deferred::FeeReservation,
    Deferred::AccountExposure,
];

const FACTS_HELD: &str = "facts_held";
const GATE_REFUSED: &str = "gate_refused";
const RULE_SET_UNAVAILABLE: &str = "rule_set_unavailable";
const WORKING_ORDERS_UNSEEN: &str = "working_orders_unseen";

/// The dry run for one deployment: the shared fold, the deployed mandate `version` and its stored
/// document, the registered rule set's gate configuration (`None` when none could be read,
/// DEC-2388 item 3) and how fresh a mark and a snapshot must be.
pub struct Gate {
    facts: Facts,
    liquidity: Box<dyn Liquidity>,
    version: String,
    mandate: Mandate,
    gate_mandate: Option<ValidatedMandate>,
    config: Option<GateConfig>,
    fresh: Freshness,
}

impl Gate {
    pub fn new(
        facts: Facts,
        (version, mandate): (&str, Mandate),
        config: Option<GateConfig>,
        fresh: Freshness,
    ) -> Self {
        let gate_mandate = gate_mandate(&mandate).ok();
        Self {
            facts,
            liquidity: Box::new(BarsReader::default()),
            version: version.to_owned(),
            mandate,
            gate_mandate,
            config,
            fresh,
        }
    }

    /// This gate reading the liquidity floor's and participation's inputs through `liquidity`
    /// (DEC-2274 item 3). Without it there are no figures, and both checks stay deferred.
    #[must_use]
    pub fn reading(mut self, liquidity: Box<dyn Liquidity>) -> Self {
        self.liquidity = liquidity;
        self
    }

    /// The advisory pass of the opening `proposal` over the fold, with [`UNJOURNALED`] and
    /// [`UNSEEN`] deferred, or the reason it could not be evaluated: `rule_set_unavailable`,
    /// `facts_held` for a fact the fold holds, `working_orders_unseen` while any order of the
    /// agent's works, whose cost the fold cannot see, and `gate_refused` when the gate refuses to
    /// evaluate it or types it as anything but an opening or an increase, so a proposal labelled as
    /// adding risk is never judged as an exit, which defers nothing (DEC-2388 item 6). The mandate
    /// version in force in the fold must be the one this gate was built on: after an applied change
    /// the opening is `facts_held` until the process restarts on the new version.
    ///
    /// # Errors
    /// The reason code the dry run journals.
    pub fn advisory(&self, proposal: &Proposal) -> Result<Advisory, &'static str> {
        self.read(proposal).map(|(advisory, _)| advisory)
    }

    /// The [`advisory`](Self::advisory) pass of `proposal` and the liquidity figures it was
    /// evaluated on, with the fold's risk clock they were read at; `None` when none were read.
    ///
    /// # Errors
    /// The reason code the dry run journals, as [`advisory`](Self::advisory) gives it.
    #[allow(
        clippy::disallowed_methods,
        reason = "the cell's runtime's advisory dry run, the one caller that may defer checks (DEC-2378 item 11)"
    )]
    pub fn read(
        &self,
        proposal: &Proposal,
    ) -> Result<(Advisory, Option<LiquidityRead>), &'static str> {
        let config = self.config.clone().ok_or(RULE_SET_UNAVAILABLE)?;
        let mandate = self.gate_mandate.clone().ok_or(GATE_REFUSED)?;
        let (context, figures) = self
            .facts
            .read(|journaled| self.context(journaled, proposal, config, mandate))
            .ok_or(FACTS_HELD)??;
        let deferred: BTreeSet<Deferred> = UNJOURNALED
            .into_iter()
            .filter(|deferred| {
                !matches!(deferred, Deferred::LiquidityFloor | Deferred::Participation)
            })
            .chain(liquidity_deferred(figures.as_ref()))
            .chain(UNSEEN)
            .collect();
        let read_at = context.now;
        let advisory =
            evaluate_deferring(proposal, &context, &deferred).map_err(|_| GATE_REFUSED)?;
        let read = figures.map(|facts| LiquidityRead {
            prior_close: facts.prior_close,
            median_dollar_volume_20d: facts.median_dollar_volume_20d,
            adv_20d: facts.adv_20d,
            trailing_5m_volume: facts.trailing_5m_volume,
            read_at,
        });
        match advisory.decision.purpose {
            GatePurpose::Open | GatePurpose::Increase => Ok((advisory, read)),
            _ => Err(GATE_REFUSED),
        }
    }

    fn context(
        &self,
        journaled: &Journaled,
        proposal: &Proposal,
        config: GateConfig,
        mandate: ValidatedMandate,
    ) -> Result<(AdvisoryGateContext, Option<LiquidityFacts>), &'static str> {
        let instrument = proposal.instrument.as_str();
        let facts = journaled
            .facts(instrument, self.fresh)
            .map_err(|_| FACTS_HELD)?;
        if facts.mandate_version != self.version {
            return Err(FACTS_HELD);
        }
        let figures = self.figures(proposal, facts.now);
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
        let context = AdvisoryGateContext {
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
                prior_close: figures.as_ref().map(|f| f.prior_close),
                median_dollar_volume_20d: figures.as_ref().map(|f| f.median_dollar_volume_20d),
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
                trailing_5m_volume: figures.as_ref().map(|f| f.trailing_5m_volume),
                adv_20d: figures.as_ref().map(|f| f.adv_20d),
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
        };
        Ok((context, figures))
    }

    /// The liquidity figures of `proposal`'s instrument at `now`, the fold's risk clock, read only
    /// for a proposal labelled an opening or an increase in a pinned US equity, by its symbol: the
    /// stored bars are US equities' alone. An exit never reads them (`AGENTS.md` rule 13,
    /// DEC-2274 item 3).
    fn figures(&self, proposal: &Proposal, now: UtcNanos) -> Option<LiquidityFacts> {
        if proposal.asset_class != AssetClass::UsEquity {
            return None;
        }
        let purpose = match proposal.purpose {
            RuntimePurpose::Open => GatePurpose::Open,
            RuntimePurpose::Increase => GatePurpose::Increase,
            _ => return None,
        };
        let pinned = self
            .mandate
            .universe
            .pinned_instruments
            .iter()
            .find(|pinned| pinned.asset_id.as_str() == proposal.instrument.as_str())?;
        self.liquidity.opening(purpose, &pinned.symbol, now)
    }
}

/// Where an opening's dry run reads the liquidity floor's and participation's inputs: the stored
/// bars through [`BarsReader`] in production (DEC-2274 item 3).
pub trait Liquidity {
    /// The figures for an opening or an increase in `symbol` at `now`, or `None` when there are
    /// none, which keeps both checks deferred.
    fn opening(&self, purpose: GatePurpose, symbol: &str, now: UtcNanos) -> Option<LiquidityFacts>;
}

impl Liquidity for BarsReader {
    fn opening(&self, purpose: GatePurpose, symbol: &str, now: UtcNanos) -> Option<LiquidityFacts> {
        BarsReader::opening(self, purpose, symbol, now)
    }
}

/// The dry run's answer to an advisory pass (DEC-2388 item 13):
/// - **An allow with nothing deferred** is an allow.
/// - **An allow with any check deferred** names each as a [`Deferral`], with the figures the
///   liquidity floor and participation were evaluated on. Should either still be deferred, no
///   figures are given, so both are named, never reported evaluated. The runtime then asks the
///   owner, and the binding gate evaluates every deferred check.
/// - **A gate denial or hold** carries the gate's reason code, with the figures when they were
///   read.
/// - **Anything else** is denied with the code the pass gave.
#[must_use]
pub fn verdict(read: Result<(Advisory, Option<LiquidityRead>), &'static str>) -> DryRunVerdict {
    let (advisory, liquidity) = match read {
        Ok(read) => read,
        Err(code) => {
            return DryRunVerdict::Deny {
                reason_code: code.to_owned(),
            };
        }
    };
    match (advisory.decision.verdict, advisory.decision.reason) {
        (Verdict::Allow, _) => {
            let liquidity = liquidity.filter(|_| {
                !advisory.deferred.contains(&Deferred::LiquidityFloor)
                    && !advisory.deferred.contains(&Deferred::Participation)
            });
            let deferred: BTreeSet<Deferral> = advisory
                .deferred
                .iter()
                .filter_map(|deferred| deferral(*deferred))
                .collect();
            if deferred.is_empty() && liquidity.is_some() {
                DryRunVerdict::Allow
            } else {
                DryRunVerdict::AllowDeferring {
                    deferred,
                    liquidity,
                }
            }
        }
        (_, reason) => {
            let reason_code = reason
                .map_or(GATE_REFUSED, |reason| reason.as_str())
                .to_owned();
            match liquidity {
                Some(liquidity) => DryRunVerdict::DenyOnFigures {
                    reason_code,
                    liquidity,
                },
                None => DryRunVerdict::Deny { reason_code },
            }
        }
    }
}

/// The runtime's name for a deferred part, or `None` for the liquidity floor and participation,
/// which the runtime names itself exactly when no figures were evaluated (journal spec rule 270).
#[must_use]
pub fn deferral(deferred: Deferred) -> Option<Deferral> {
    match deferred {
        Deferred::LiquidityFloor | Deferred::Participation => None,
        Deferred::Eligibility => Some(Deferral::Eligibility),
        Deferred::DayTradeBudget => Some(Deferral::DayTradeBudget),
        Deferred::AgentMode => Some(Deferral::AgentMode),
        Deferred::AgentEquity => Some(Deferral::AgentEquity),
        Deferred::ReentryCooldown => Some(Deferral::ReentryCooldown),
        Deferred::Halt => Some(Deferral::Halt),
        Deferred::OtherOrders => Some(Deferral::OtherOrders),
        Deferred::QuoteAsk => Some(Deferral::QuoteAsk),
        Deferred::ConductHistory => Some(Deferral::ConductHistory),
        Deferred::RelatedAccounts => Some(Deferral::RelatedAccounts),
        Deferred::FeeReservation => Some(Deferral::FeeReservation),
        Deferred::AccountExposure => Some(Deferral::AccountExposure),
    }
}

impl GateDryRun for Gate {
    /// An opening or an increase is the advisory pass's [`verdict`]; any other purpose is the
    /// binding gate's alone, so the dry run never denies an exit (`AGENTS.md` rule 13).
    fn check(&self, proposal: &Proposal) -> DryRunVerdict {
        if !proposal.purpose.adds_risk() {
            return DryRunVerdict::Allow;
        }
        verdict(self.read(proposal))
    }
}
