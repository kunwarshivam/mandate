//! The deployment's order plan over the journaled fold (DEC-2378 item 10): `mandate-ports`'s
//! sizing and classification, with every fact from [`Facts`]. It proposes an opening only, and
//! only where the agent is flat with nothing working, so it never sizes on a cost basis or a P&L
//! the daemon cannot see.

use std::collections::BTreeMap;

use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, Market as BuilderMarket,
    RiskContext as BuilderRiskContext, buy_action,
};
use mandate_canon::{Digest, Value};
use mandate_domain::{AssetClass, Environment, MarketSession};
use mandate_gate_context::Freshness;
use mandate_num::{CostBasis, FeeRate, Fraction, MarkPrice, Qty, Signed, SizeFraction, Unit, Usd};
use mandate_ports::BuilderContext;
use mandate_runtime::{
    Autonomy, Classified, MandateView, OrderExecution, OrderPlan, Proposal, ProtectionPrices,
    SignalInputs, TimeInForce,
};
use mandate_spec::ValidationContext;
use mandate_spec::document::{InstrumentRef, Mandate, ProvenanceMap};
use mandate_spec::{ValidatedMandate, context::stop_limit_asset_classes, risk::risk_day};
use mandate_time::{ExchangeCalendar, Session, UtcNanos};

use super::Facts;

/// Trading-domain spec §9's close window: no opening in the regular session's last 10 minutes.
const CLOSE_WINDOW_S: i64 = 600;

/// The plan for one deployment: its stored mandate, the shared fold, and how fresh a mark and a
/// snapshot must be for an opening.
pub struct Plan {
    facts: Facts,
    mandate: Mandate,
    fresh: Freshness,
}

impl Plan {
    pub fn new(facts: Facts, mandate: Mandate, fresh: Freshness) -> Self {
        Self {
            facts,
            mandate,
            fresh,
        }
    }

    /// The validated mandate and the builder's facts for an opening in `pinned`, or `None` when
    /// any is missing: the fold holds the opening, the agent holds the instrument or has an
    /// order working in it, the session admits no opening, or the mandate does not validate.
    fn context(&self, pinned: &InstrumentRef) -> Option<(ValidatedMandate, BuilderContext)> {
        let instrument = pinned.asset_id.as_str();
        let (facts, flat, models) = self.facts.read(|journaled| {
            let flat = !journaled.held().contains_key(instrument)
                && !journaled
                    .working()
                    .values()
                    .any(|named| named == instrument);
            (
                journaled.facts(instrument, self.fresh),
                flat,
                journaled.models(),
            )
        })?;
        let facts = facts.ok().filter(|_| flat)?;
        let session = opening_session(pinned.asset_class, facts.now)?;
        let validation = validation_context(facts.account.equity, facts.now)?;
        let validated = ValidatedMandate::new(self.mandate.clone(), &validation, &[]).ok()?;
        let builder = builder_context(&self.mandate, pinned, &facts, session, &models)?;
        Some((validated, builder))
    }
}

impl OrderPlan for Plan {
    fn plan(&self, _view: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        self.mandate
            .universe
            .pinned_instruments
            .iter()
            .find_map(|pinned| {
                let (validated, builder) = self.context(pinned)?;
                mandate_ports::size(&validated, &builder, inputs)
                    .ok()
                    .flatten()
            })
            .filter(|proposal| proposal.purpose.adds_risk())
    }

    /// Mandate spec §6 over the proposal's own figures. A proposal the plan could not have made,
    /// or one whose facts have gone, is denied, which never adds risk (`AGENTS.md` rule 3).
    fn classify(&self, _view: &MandateView, proposal: &Proposal) -> Classified {
        let denied = Classified {
            autonomy: Autonomy::Deny,
            decided_by: None,
        };
        let Some(pinned) = self
            .mandate
            .universe
            .pinned_instruments
            .iter()
            .find(|pinned| pinned.asset_id.as_str() == proposal.instrument.as_str())
        else {
            return denied;
        };
        self.classified(pinned, proposal).unwrap_or(denied)
    }
}

impl Plan {
    fn classified(&self, pinned: &InstrumentRef, proposal: &Proposal) -> Option<Classified> {
        let (validated, builder) = self.context(pinned)?;
        let Value::Str(score) = &proposal.combined_score else {
            return None;
        };
        let score = Unit::parse(score).ok()?;
        let action = buy_action(
            &builder.account,
            &builder.market,
            &builder.risk,
            score,
            proposal.qty,
        )
        .ok()?;
        mandate_ports::classify(&validated, &action, proposal, false).ok()
    }
}

/// The session an opening in `asset_class` may be proposed in at `now`: an equity's regular
/// session outside its close window, and crypto at any time. Any other answer, or a date the
/// committed calendar does not cover, proposes nothing.
fn opening_session(asset_class: AssetClass, now: UtcNanos) -> Option<MarketSession> {
    match asset_class {
        AssetClass::Crypto => Some(MarketSession::Crypto),
        AssetClass::UsEquity => {
            let calendar = ExchangeCalendar::us_equities().ok()?;
            let (today, _) = mandate_time::new_york_date_and_hour(now).ok()?;
            let span = calendar
                .sessions(today)
                .ok()?
                .into_iter()
                .find(|span| span.contains(now))?;
            let left = span.end().secs().saturating_sub(now.secs());
            (span.session() == Session::Regular && left > CLOSE_WINDOW_S)
                .then_some(MarketSession::Regular)
        }
    }
}

/// The validation context of a paper deployment at `now`, with the account's journaled equity,
/// as the shell's paper run builds it.
fn validation_context(account_equity: Usd, now: UtcNanos) -> Option<ValidationContext> {
    Some(ValidationContext {
        account_equity_usd: account_equity,
        other_allocations_usd: Usd::ZERO,
        validation_date: risk_day(now).ok()?.day,
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 1,
        approver_users: 1,
        independent_approval_required: false,
        disclosures_accepted: Default::default(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: Default::default(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: Default::default(),
        previous_version: None,
        current_mandate_version: None,
        stop_limit_asset_classes: stop_limit_asset_classes("alpaca"),
    })
}

/// The builder's facts for a first opening, flat and with nothing working (DEC-2378 item 10):
/// the agent's equity is its allocation, the risk mark is the sane bid, and the ask is that same
/// bid, so a buy never pays above it. A whole share is the increment, which never buys a
/// fraction the instrument may not admit.
fn builder_context(
    mandate: &Mandate,
    pinned: &InstrumentRef,
    facts: &mandate_gate_context::Facts,
    session: MarketSession,
    models: &BTreeMap<(String, String), String>,
) -> Option<BuilderContext> {
    let allocation = mandate.capital.allocation_usd.to_usd().ok()?;
    let bid = facts.mark.bid;
    let increment = Qty::parse("1").ok()?;
    let account = BuilderAccountSnapshot {
        agent_equity: allocation,
        position_qty: Qty::ZERO,
        cost_basis: CostBasis::ZERO,
        risk_mark: MarkPrice::from(bid),
        gross_usd: Usd::ZERO,
        working_opening_cost: Usd::ZERO,
        goal_spent_usd: Usd::ZERO,
    };
    let market = BuilderMarket {
        instrument: pinned.asset_id.clone(),
        asset_class: pinned.asset_class,
        session,
        in_close_window: false,
        bid,
        ask: bid,
        increment,
        min_order_usd: Usd::parse("1").ok()?,
        fee_rate_cash: FeeRate::parse("0").ok()?,
        fee_rate_asset: FeeRate::parse("0").ok()?,
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
        risk_day: risk_day(facts.now).ok()?.day,
    };
    let action = buy_action(&account, &market, &risk, Unit::ONE, increment).ok()?;
    let protection = protection(mandate, bid)?;
    let model_content_hashes = models
        .iter()
        .map(|(model, hash)| {
            let hex = hash.strip_prefix("sha256:")?;
            Some((model.clone(), Digest::from_hex(hex)?))
        })
        .collect::<Option<BTreeMap<_, _>>>()?;
    Some(BuilderContext {
        account,
        market,
        risk,
        action,
        model_content_hashes,
        execution: OrderExecution {
            asset_class: pinned.asset_class,
            tif: TimeInForce::Day,
            protection_required: protection.is_some(),
            protection,
        },
    })
}

/// The mandate's protection for an entry at `entry`, the executor's bracket (`Price::
/// equity_bracket`), or `Some(None)` when the mandate disables it. Enabled protection without a
/// stop distance proposes nothing rather than an unprotected opening.
fn protection(mandate: &Mandate, entry: mandate_num::Price) -> Option<Option<ProtectionPrices>> {
    let protection = &mandate.protection;
    if !protection.enabled {
        return Some(None);
    }
    let stop_distance = Fraction::parse(protection.stop_distance.as_ref()?.as_str()).ok()?;
    let take_profit_distance = match &protection.take_profit_distance {
        Some(distance) => Some(Fraction::parse(distance.as_str()).ok()?),
        None => None,
    };
    let (stop, take_profit) = entry
        .equity_bracket(stop_distance, take_profit_distance)
        .ok()?;
    Some(Some(ProtectionPrices { stop, take_profit }))
}
