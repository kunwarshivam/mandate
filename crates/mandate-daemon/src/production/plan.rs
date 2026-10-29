//! The deployment's order plan over the journaled fold (DEC-2378 item 10): `mandate-ports`'s
//! sizing and classification, with every fact from [`Facts`]. It proposes an opening only, and
//! only where the agent is flat with nothing working, so it never sizes on a cost basis or a P&L
//! the daemon cannot see.

use std::collections::BTreeMap;

use mandate_builder::{
    AccountSnapshot as BuilderAccountSnapshot, Market as BuilderMarket, RequestedBy,
    RiskContext as BuilderRiskContext, buy_action,
};
use mandate_canon::{Digest, Value};
use mandate_domain::{AssetClass, AssetId, AutonomyDecision, Environment, MarketSession};
use mandate_gate_context::Freshness;
use mandate_num::{CostBasis, FeeRate, Fraction, MarkPrice, Qty, Signed, SizeFraction, Unit, Usd};
use mandate_ports::BuilderContext;
use mandate_runtime::{
    Autonomy, Classified, MandateView, ModelDirection, OrderExecution, OrderPlan, OwnerRequest,
    Proposal, ProtectionPrices, SignalInputs, TimeInForce,
};
use mandate_spec::ValidationContext;
use mandate_spec::condition::{Condition, ConditionField};
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

    /// The validated mandate and the builder's facts for an opening in `instrument`, or `None` when
    /// any is missing: the fold holds the opening, the agent holds the instrument or has an
    /// order working in it, the session admits no opening, or the mandate does not validate.
    fn context(&self, opened: Opened<'_>) -> Option<(ValidatedMandate, BuilderContext)> {
        self.context_with(opened, Holding::Flat)
    }

    /// [`Plan::context`] for `holding`: an owner's request (DEC-2858 item 10) is clipped against
    /// the position the fold says the agent holds, marked at the sane bid, where an opening
    /// requires none; neither is sized while an order of the agent's works in the instrument.
    fn context_with(
        &self,
        opened: Opened<'_>,
        holding: Holding,
    ) -> Option<(ValidatedMandate, BuilderContext)> {
        let instrument = opened.asset_id.as_str();
        let (facts, held, working, models) = self.facts.read(|journaled| {
            (
                journaled.facts(instrument, self.fresh),
                journaled.held().get(instrument).copied(),
                journaled
                    .working()
                    .values()
                    .any(|named| named == instrument),
                journaled.models(),
            )
        })?;
        let admitted = !working && (holding == Holding::Held || held.is_none());
        let facts = facts.ok().filter(|_| admitted)?;
        let session = opening_session(opened.asset_class, facts.now)?;
        let validation = validation_context(facts.account.equity, facts.now)?;
        let validated = ValidatedMandate::new(self.mandate.clone(), &validation, &[]).ok()?;
        let held = held.unwrap_or(Qty::ZERO);
        let builder = builder_context(&self.mandate, opened, &facts, session, &models, held)?;
        Some((validated, builder))
    }
}

impl OrderPlan for Plan {
    /// The thesis exit of an instrument the agent holds first (mandate spec §8.3 step 2, MI-34,
    /// DEC-2391), and only then an opening: an exit never waits behind a buy.
    fn plan(&self, _view: &MandateView, inputs: &SignalInputs) -> Option<Proposal> {
        self.thesis_exit(inputs).or_else(|| self.opening(inputs))
    }

    /// Mandate spec §6 over the proposal's own figures. An exit is built-in AUTO, never asked or
    /// denied (§6.1, `AGENTS.md` rules 2 and 13). A proposal the plan could not have made,
    /// or one whose facts have gone, is denied, which never adds risk (`AGENTS.md` rule 3). The
    /// plan cannot see every fact an autonomy rule may read (drawdown, daily P&L, what the agent
    /// bought today, gross exposure), so it never answers `auto` for an opening: §6.2 step 5c with
    /// `auto_allowed` false makes an `auto` opening `ask` (DEC-2378 item 10). A null score is an
    /// owner's or a client's request (journal spec rule 319, DEC-2858): it is classified unscored,
    /// so a rule reading the score denies it, and as the owner's, the client ceiling being the
    /// runtime's to apply after.
    fn classify(&self, view: &MandateView, proposal: &Proposal) -> Classified {
        if !proposal.purpose.adds_risk() {
            return Classified {
                autonomy: Autonomy::Auto,
                decided_by: Some(BUILTIN_RISK_REDUCING.to_owned()),
            };
        }
        self.classify_opening(view, proposal)
    }
    /// An owner's or a client's request (DEC-2858) for the deployment's own pinned asset, clipped
    /// by the order builder over the same journaled facts an opening is sized on, so only while the
    /// agent is flat with nothing working and the session admits an opening. `None` for any other
    /// instrument, an unpinned mandate's (whose one instrument is its thesis's), or a missing fact
    /// (`AGENTS.md` rule 3).
    fn requested(&self, _view: &MandateView, request: &OwnerRequest) -> Option<Proposal> {
        let pinned = self
            .mandate
            .universe
            .pinned_instruments
            .iter()
            .find(|pinned| pinned.asset_id.as_str() == request.instrument.as_str())?;
        let (validated, builder) = self.context_with(Opened::pinned(pinned), Holding::Held)?;
        mandate_ports::requested(&validated, &builder, request)
            .ok()
            .flatten()
    }
}

/// What a context admits of the agent's position in the instrument: an opening only while the agent
/// is flat, so it never sizes on a cost basis or a P&L the daemon cannot see; an owner's request on
/// top of what it holds, which is clipped, never sized.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Holding {
    Flat,
    Held,
}

/// The instrument an opening is sized and classified for: a pin, or an unpinned mandate's
/// working instrument with the class its thesis states.
#[derive(Clone, Copy)]
struct Opened<'a> {
    asset_id: &'a AssetId,
    asset_class: AssetClass,
}

impl<'a> Opened<'a> {
    fn pinned(pinned: &'a InstrumentRef) -> Self {
        Self {
            asset_id: &pinned.asset_id,
            asset_class: pinned.asset_class,
        }
    }
}

/// Whether any model's output in `inputs` is a `Long`: an unpinned mandate's opening is sized only
/// from one (DEC-2827 keeps a `Long` only on the instrument the executor would open in).
fn any_long(inputs: &SignalInputs) -> bool {
    inputs.outputs.values().any(|outputs| {
        outputs
            .values()
            .any(|output| output.direction == ModelDirection::Long)
    })
}

impl Plan {
    /// An unpinned mandate's opening, on its working universe (DEC-2841): the one instrument the
    /// agent stream's single surviving thesis holds, replayed at the fold's clock as the executor
    /// and the models replay it (DEC-2819, DEC-2827), with the class the runtime's fold of its
    /// thesis gives (`inputs.thesis_classes`), within the envelope's asset classes, and sized from
    /// journaled facts as a pinned opening is (DEC-2378 item 10). `None` when any of it is
    /// missing, two theses survive, or a record does not replay (`AGENTS.md` rule 3).
    fn working_opening(&self, inputs: &SignalInputs) -> Option<Proposal> {
        let working = self.facts.working_instrument()?;
        let instrument = mandate_accounting::InstrumentId::new(&working).ok()?;
        let asset_class = *inputs.thesis_classes.get(&instrument)?;
        if !self.mandate.universe.asset_classes.contains(&asset_class) {
            return None;
        }
        let asset_id = AssetId::parse(&working).ok()?;
        let opened = Opened {
            asset_id: &asset_id,
            asset_class,
        };
        let (validated, builder) = self.context(opened)?;
        mandate_ports::size(&validated, &builder, inputs)
            .ok()
            .flatten()
            .filter(|proposal| proposal.purpose.adds_risk())
    }

    /// An unpinned mandate's opening classified as a pinned one's is (DEC-2841), only while its
    /// instrument is still the working one and its class is the one its admitted thesis states and
    /// the envelope allows; any other proposal is denied (`AGENTS.md` rule 3).
    fn classify_working(&self, _view: &MandateView, proposal: &Proposal) -> Classified {
        let denied = Classified {
            autonomy: Autonomy::Deny,
            decided_by: None,
        };
        let instrument = proposal.instrument.as_str();
        let working = self.facts.working_instrument();
        let class = self.facts.thesis_class(instrument);
        if working.as_deref() != Some(instrument)
            || class != Some(proposal.asset_class)
            || !self
                .mandate
                .universe
                .asset_classes
                .contains(&proposal.asset_class)
        {
            return denied;
        }
        let Ok(asset_id) = AssetId::parse(instrument) else {
            return denied;
        };
        let opened = Opened {
            asset_id: &asset_id,
            asset_class: proposal.asset_class,
        };
        self.classified(opened, proposal).unwrap_or(denied)
    }
}

/// The `decided_by` label of an exit's built-in AUTO (journal spec §9.1, mandate spec §6.2 step 3).
const BUILTIN_RISK_REDUCING: &str = "builtin_risk_reducing";

impl Plan {
    /// §8.3 step 2's first branch over the fold: the first instrument, in id order, whose thesis
    /// ended and which the agent's fills still hold, sold whole less the runtime's working exits at
    /// its last sane bid, by `mandate_ports::thesis_exit`, with its asset class from its thesis
    /// record. Nothing an opening needs is read: no validation against the account's equity, no
    /// protective stop, no model registry, no snapshot's age, so a drawdown below the allocation
    /// never holds the exit (`AGENTS.md` rule 13). Only a mark fresh within [`Freshness`]'s mark age
    /// is needed, because the exit is priced at the bid and the gate's collar reads it; without one
    /// the exit is held as the binding gate holds one (`exit_unpriced`, trading spec §5.3 rule 4)
    /// and offered again at the next evaluation, while a risk exit, an owner exit, or a kill switch
    /// sells without it. A pinned universe has no theses (V-037). One exit is proposed per
    /// evaluation; the next ended instrument is offered at the next.
    fn thesis_exit(&self, inputs: &SignalInputs) -> Option<Proposal> {
        let unaged = Freshness {
            mark_max_age_s: self.fresh.mark_max_age_s,
            snapshot_max_age_s: u32::MAX,
        };
        inputs.ended_theses.iter().find_map(|instrument| {
            let class = *inputs.thesis_classes.get(instrument)?;
            let (held, facts) = self.facts.read(|journaled| {
                (
                    journaled.held().get(instrument.as_str()).copied(),
                    journaled.facts(instrument.as_str(), unaged),
                )
            })?;
            let (held, bid) = (held?, facts.ok()?.mark.bid);
            let account = BuilderAccountSnapshot {
                agent_equity: Usd::ZERO,
                position_qty: held,
                cost_basis: CostBasis::ZERO,
                risk_mark: MarkPrice::from(bid),
                gross_usd: Usd::ZERO,
                working_opening_cost: Usd::ZERO,
                goal_spent_usd: Usd::ZERO,
            };
            let market = BuilderMarket {
                instrument: AssetId::parse(instrument.as_str()).ok()?,
                asset_class: class,
                session: exit_session(class),
                in_close_window: false,
                bid,
                ask: bid,
                increment: Qty::parse("1").ok()?,
                min_order_usd: Usd::ZERO,
                fee_rate_cash: FeeRate::parse("0").ok()?,
                fee_rate_asset: FeeRate::parse("0").ok()?,
            };
            let execution = OrderExecution {
                asset_class: class,
                tif: match class {
                    AssetClass::UsEquity => TimeInForce::Day,
                    AssetClass::Crypto => TimeInForce::Gtc,
                },
                protection_required: false,
                protection: None,
            };
            mandate_ports::thesis_exit(
                self.mandate.universe.pinned,
                &account,
                &market,
                execution,
                inputs,
            )
            .ok()
            .flatten()
        })
    }

    fn opening(&self, inputs: &SignalInputs) -> Option<Proposal> {
        if !self.mandate.universe.pinned && any_long(inputs) {
            return self.working_opening(inputs);
        }
        self.mandate
            .universe
            .pinned_instruments
            .iter()
            .find_map(|pinned| {
                let (validated, builder) = self.context(Opened::pinned(pinned))?;
                mandate_ports::size(&validated, &builder, inputs)
                    .ok()
                    .flatten()
            })
            .filter(|proposal| proposal.purpose.adds_risk())
    }

    fn classify_opening(&self, view: &MandateView, proposal: &Proposal) -> Classified {
        if !self.mandate.universe.pinned {
            return self.classify_working(view, proposal);
        }
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
        self.classified(Opened::pinned(pinned), proposal)
            .unwrap_or(denied)
    }
}

impl Plan {
    fn classified(&self, opened: Opened<'_>, proposal: &Proposal) -> Option<Classified> {
        let score = match &proposal.combined_score {
            Value::Null => None,
            Value::Str(score) => Some(Unit::parse(score).ok()?),
            _ => return None,
        };
        let holding = match score {
            Some(_) => Holding::Flat,
            None => Holding::Held,
        };
        let (validated, builder) = self.context_with(opened, holding)?;
        let mut action = buy_action(
            &builder.account,
            &builder.market,
            &builder.risk,
            score.unwrap_or(Unit::ZERO),
            proposal.qty,
        )
        .ok()?;
        if score.is_none() {
            action.combined_score = None;
            action.requested_by = RequestedBy::Owner;
        }
        let classified = mandate_ports::classify(&validated, &action, proposal, false).ok()?;
        if proposal.purpose.adds_risk() && denies_on_an_unseen_fact(&self.mandate) {
            return Some(Classified {
                autonomy: Autonomy::Deny,
                decided_by: Some(mandate_ports::POLICY_OVERLAY.to_owned()),
            });
        }
        Some(mandate_ports::auto_not_allowed(classified, proposal))
    }
}

/// The rule fields (mandate spec §6.3) the plan's builder facts do not carry from the fold: it
/// sees no earlier fill, drawdown, daily P&L, gross exposure, or the day's buys, and no thesis or
/// research admission, and the input-drift field is reserved. On these a rule reads zero or
/// false, whatever the account did.
const UNSEEN: [ConditionField; 8] = [
    ConditionField::FirstTradeInInstrument,
    ConditionField::NewInstrument,
    ConditionField::ThesisConfidence,
    ConditionField::Drawdown,
    ConditionField::DailyPnlFraction,
    ConditionField::GrossUsdAfter,
    ConditionField::BoughtTodayUsd,
    ConditionField::UnusualInput,
];

/// Whether a `deny` rule of `mandate` reads a field the plan cannot see, anywhere in its
/// condition. Such a rule cannot be judged on the plan's facts, and the owner chose to refuse
/// what it names, so every opening and increase is denied rather than asked: an approver must
/// never be offered what a `deny` rule would refuse (`AGENTS.md` rules 1 and 3).
fn denies_on_an_unseen_fact(mandate: &Mandate) -> bool {
    mandate.autonomy.rules.iter().any(|rule| {
        rule.then == AutonomyDecision::Deny
            && rule.when.comparisons().iter().any(|(comparison, _)| {
                matches!(comparison, Condition::Compare { field, .. } if UNSEEN.contains(field))
            })
    })
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

/// The session an exit in `asset_class` is labelled with: crypto's, or the regular session for
/// an equity. It only labels the builder's market: whether an equity exit waits for the regular
/// session is the gate's `defer` and the executor's, never the plan's (`AGENTS.md` rule 13), and
/// how it is priced, marketable in the close window included, is the executor's (trading spec
/// §5.6), since the runtime's proposal carries no order shape.
fn exit_session(asset_class: AssetClass) -> MarketSession {
    match asset_class {
        AssetClass::Crypto => MarketSession::Crypto,
        AssetClass::UsEquity => MarketSession::Regular,
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

/// The builder's facts for a first opening, flat and with nothing working (DEC-2378 item 10), or
/// for an owner's request on top of the `held` position, which is its gross exposure at the bid:
/// the agent's equity is its allocation, the risk mark is the sane bid, and the ask is that same
/// bid, so a buy never pays above it. A whole share is the increment, which never buys a
/// fraction the instrument may not admit.
fn builder_context(
    mandate: &Mandate,
    opened: Opened<'_>,
    facts: &mandate_gate_context::Facts,
    session: MarketSession,
    models: &BTreeMap<(String, String), String>,
    held: Qty,
) -> Option<BuilderContext> {
    let allocation = mandate.capital.allocation_usd.to_usd().ok()?;
    let bid = facts.mark.bid;
    let increment = Qty::parse("1").ok()?;
    let account = BuilderAccountSnapshot {
        agent_equity: allocation,
        position_qty: held,
        cost_basis: CostBasis::ZERO,
        risk_mark: MarkPrice::from(bid),
        gross_usd: held.notional(bid).ok()?,
        working_opening_cost: Usd::ZERO,
        goal_spent_usd: Usd::ZERO,
    };
    let market = BuilderMarket {
        instrument: opened.asset_id.clone(),
        asset_class: opened.asset_class,
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
            asset_class: opened.asset_class,
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
