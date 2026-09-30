//! Family B of the `mandate` suite: the thirty-two `builder` cases (spec §6.2, §8.1 to §8.3), run
//! through `mandate_builder::propose`, `mandate_risk::evaluate` as §6.2 step 2's dry run, and
//! `mandate_builder::decide` (E6-2, DEC-130, DEC-250).
//!
//! **The dry run is the gate's, never the case's.** A case's `gate_dry_run` is what the real gate
//! must answer about the order the builder proposed, and `decide` is handed that answer. Nothing a
//! case expects is an input to anything under test, so a builder that proposes the wrong order
//! meets the gate's verdict on the wrong order, not the verdict the fixture wrote down. A hold
//! reaches neither: the case may state no dry run or autonomy for one, and `decide` must refuse it
//! with `nothing_proposed`.
//!
//! **One scene for the builder and the gate** (DEC-250 items 3 to 5). The session and the close
//! window come from `mandate_risk::session_at` at the case's `now`, for both, because a caller never
//! labels the session (DEC-129 item 9); the case's `session` and `in_close_window` are claims about
//! that clock, compared once everything else has been. The agent's exposure is one set of figures:
//! `gate_state.agent_equity` is the case's `agent_equity`, the instrument's `positions_mv` entry is
//! `position_qty × bid` (the risk mark, and absent when flat), the builder's working cost is the
//! gate state's working opening orders in the instrument, and its gross exposure is the gate
//! state's positions and working orders. A case may restate `gross_usd` or `working_opening_orders`
//! only as those same figures.
//!
//! **The gate's other inputs** are DEC-178's "every other check passes" environment, as for family
//! G: a healthy risk state at the case's equity, a margin account at the fixture's
//! `account_equity_usd`, and a liquid, tradable, unhalted listing of the pinned instrument, quoted at
//! the case's own bid and ask at `now`, whose minimum order size is the case's `qty_increment`.
//!
//! **What the harness fills.** The fixture header's four defaults (`session` regular,
//! `in_close_window` false, fee rates 0, `has_prior_fill` = position above zero) and `ref.py`'s
//! (`size_factor` 1, and 0 for the drawdown, both P&L fractions, the day's buys, the cost basis and
//! the goal spend). Every base pins its universe, so no order can be an admission: `new_instrument`
//! is false and `thesis_confidence` 0 (§6.3, "0 when no thesis applies"), and a base that does not
//! pin is refused rather than guessed about.
//!
//! **Every key is read (DEC-85).** The case, its `input`, `quote`, `gate_state`, every output and
//! working order, `expect`, `gate_dry_run` and `autonomy` are each swept against the members read
//! here. An expectation the builder has an answer for must be stated: a computed target or Delta,
//! a clip, an order field. One it has no answer for must be absent: `reason` on an order, an order
//! field on a hold, `clipped_by` on a sell. `clipped_by` absent on a hold means no clip, and
//! `order_type` absent on a sell means a plain limit.
//!
//! **Cases that cannot pass yet fail at their owner.** A `trim_to_target` base asks the risk engine
//! first (§6.2 step 1), and `mandate_risk::trim_proposals` is E6-4's stub; a crypto opening reaches
//! `mandate_risk::evaluate`, whose check 2 is owed to E6-10 (DEC-129 item 29).

use std::collections::{BTreeMap, BTreeSet};

use mandate_builder::{
    AccountSnapshot, AccumulateGoal, Action, BuilderError, BuilderMandate, Clip, Direction,
    GateVerdict, GoalKind, HoldReason, Limits, Market, ModelOutput, ModelVersion, OrderShape,
    Outcome, Proposal, RiskContext, SignalModel, Sizing, decide, propose,
};
use mandate_domain::{AssetClass, AssetId, MarketSession, Purpose};
use mandate_num::{
    Conviction, CostBasis, FeeRate, Fraction, MarkPrice, Price, Qty, Ratio, Signed, SizeFraction,
    Unit, Usd, UsdExact,
};
use mandate_risk as gate;
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};
use mandate_spec::document::{self as spec_doc, Goal, LadderAction, Mandate, ModelId};
use mandate_time::{UtcNanos, new_york_date_and_hour};

use super::autonomy;
use super::{at_of, digest, instant, not_implemented, num, unknown_members, validated_mandate};
use crate::{Json, at, ensure, expect_eq, list_at, str_at, u64_at};

const CASE_KEYS: &[&str] = &["id", "kind", "title", "base", "input", "expect"];
/// The members `ref.py`'s builder reads, and nothing else.
const INPUT_KEYS: &[&str] = &[
    "now",
    "instrument",
    "asset_class",
    "agent_equity",
    "position_qty",
    "quote",
    "qty_increment",
    "min_order_usd",
    "gate_state",
    "outputs",
    "size_factor",
    "working_opening_orders",
    "gross_usd",
    "session",
    "in_close_window",
    "cost_basis_usd",
    "goal_spent_usd",
    "fee_rate_cash",
    "fee_rate_asset",
    "has_prior_fill",
    "drawdown",
    "daily_pnl_fraction",
    "bought_today_usd",
    "position_pnl_fraction",
    "scale_active_s",
    "holding",
];
/// §5.5's trim guards, which only a `trim_to_target` mandate reads.
const TRIM_INPUT_KEYS: [&str; 2] = ["scale_active_s", "holding"];
const TRIM_EXPECT_KEYS: [&str; 2] = ["origin", "trim_withheld"];
const QUOTE_KEYS: &[&str] = &["bid", "ask"];
const GATE_STATE_KEYS: &[&str] = &[
    "agent_equity",
    "positions_mv",
    "working_opening_orders",
    "orders_today",
    "last_exit_fill_at",
    "working_universe",
];
const WORKING_ORDER_KEYS: &[&str] = &["instrument", "max_cost"];
const OUTPUT_KEYS: &[&str] = &[
    "model_id",
    "model_version",
    "content_hash",
    "instrument_id",
    "as_of",
    "expires_at",
    "conviction",
    "confidence",
];
/// The members a `builder` case's `expect` states, which the suite's `EXPECT_KEYS` sweeps.
pub(super) const EXPECT_KEYS: &[&str] = &[
    "cap",
    "current_mv",
    "outputs_used",
    "combined_conviction",
    "buy_conviction",
    "combined_score",
    "target_value",
    "delta",
    "action",
    "reason",
    "purpose",
    "qty",
    "limit_price",
    "order_usd",
    "order_type",
    "clipped_by",
    "gate_dry_run",
    "autonomy",
    "origin",
    "trim_withheld",
];
const DRY_RUN_KEYS: &[&str] = &["verdict", "reason"];
/// The members only an order states.
const ORDER_KEYS: [&str; 5] = ["purpose", "qty", "limit_price", "order_usd", "order_type"];
/// The one agent every `builder` case is about.
const THE_AGENT: gate::AgentId = gate::AgentId(1);

/// `kind: builder` — propose, ask the gate about the proposal, decide, and compare every member.
pub(super) fn builder_case(fixture: &Json, case: &Json) -> Result<(), String> {
    unknown_members(case, CASE_KEYS)
        .map_err(|unknown| format!("case keys not interpreted: {unknown}"))?;
    let validated = validated_mandate(fixture, case)?;
    let document = validated.mandate();
    let input = at_of(case, "input")?;
    unknown_members(input, INPUT_KEYS)
        .map_err(|unknown| format!("`input` members not interpreted: {unknown}"))?;
    let expect = at_of(case, "expect")?;
    unknown_members(expect, EXPECT_KEYS)
        .map_err(|unknown| format!("expectations not interpreted: {unknown}"))?;
    let stated = Inputs::read(document, input)?;
    let config = test_default_gate_config()?;
    if document.risk.scale_action == spec_doc::ScaleAction::TrimToTarget {
        return trim_first(fixture, document, &stated, &config, input);
    }
    for key in TRIM_INPUT_KEYS {
        ensure(input.get(key).is_none(), || {
            format!("`{key}`: only a `trim_to_target` mandate reads it (§5.5)")
        })?;
    }
    for key in TRIM_EXPECT_KEYS {
        ensure(expect.get(key).is_none(), || {
            format!("`{key}`: only a `trim_to_target` mandate's risk engine reports it (§5.5)")
        })?;
    }
    let clock = gate::session_at(stated.now, &config, stated.gate_class())
        .map_err(|e| gate_error("session_at", &e))?;
    let proposal = propose(
        &builder_mandate(document)?,
        &stated.account,
        &stated.market(&clock),
        &stated.risk,
        &stated.outputs,
        stated.now,
    )
    .map_err(|e| builder_error("propose", &e))?;
    let judged = judge(fixture, document, &stated, &config, &proposal, expect);
    stated.agrees_with_clock(&clock, judged)
}

/// §6.2 step 1: under `trim_to_target` the risk engine proposes first, and a case cannot pass until
/// its trim, its `origin` and its `trim_withheld` guards are compared, which E6-4's
/// `trim_proposals` must answer before this arm can.
fn trim_first(
    fixture: &Json,
    document: &Mandate,
    stated: &Inputs,
    config: &gate::GateConfig,
    input: &Json,
) -> Result<(), String> {
    let active_s = match input.get("scale_active_s") {
        None => 0,
        Some(value) => value
            .as_u64()
            .ok_or("`scale_active_s` is not a whole number of seconds")?,
    };
    let goal_state = if flag_or(input, "holding", false)? {
        GoalState::Holding
    } else {
        GoalState::Running
    };
    let mut scene = Scene::read(fixture, document, stated, config, goal_state)?;
    if let Some(rung) = scaling_rung(document, stated.gate_size_factor)? {
        scene.risk.active_rungs.insert(rung, active_s);
    }
    let instruments = BTreeMap::from([(
        scene.instrument.instrument.clone(),
        scene.instrument.clone(),
    )]);
    match gate::trim_proposals(
        stated.now,
        config,
        &scene.mandate,
        &scene.risk,
        &scene.agent,
        &instruments,
    ) {
        Err(e) => Err(gate_error("trim_proposals", &e)),
        Ok(trims) => Err(format!(
            "`mandate_risk::trim_proposals` answered {trims:?}, and this arm does not yet compare a \
             `trim_to_target` case's trim, `origin` or `trim_withheld` (§5.5), so the case cannot \
             pass until it does"
        )),
    }
}

/// The `scale_sizes` rung whose factor is the case's size factor, which is the rung a trim case
/// says has been active for `scale_active_s`; none at a factor of 1.
fn scaling_rung(document: &Mandate, size_factor: Ratio) -> Result<Option<u8>, String> {
    if size_factor == num(Ratio::parse("1"), "size_factor")? {
        return Ok(None);
    }
    for (index, rung) in document.risk.drawdown_ladder.iter().enumerate() {
        let Some(factor) = &rung.factor else {
            continue;
        };
        if rung.action == LadderAction::ScaleSizes
            && num(Ratio::parse(factor.as_str()), "drawdown_ladder.factor")? == size_factor
        {
            return u8::try_from(index)
                .map(Some)
                .map_err(|_| "the ladder has too many rungs".to_owned());
        }
    }
    Err("`size_factor`: no one `scale_sizes` rung of the base has this factor".to_owned())
}

/// Everything after the proposal: its figures, then the dry run and the outcome for an order.
fn judge(
    fixture: &Json,
    document: &Mandate,
    stated: &Inputs,
    config: &gate::GateConfig,
    proposal: &Proposal,
    expect: &Json,
) -> Result<(), String> {
    compare_proposal(expect, proposal)?;
    let Some(order) = Order::of(&proposal.action) else {
        for key in ["gate_dry_run", "autonomy"] {
            ensure(expect.get(key).is_none(), || {
                format!(
                    "`{key}`: the case states one, but a hold reaches neither the gate nor approval"
                )
            })?;
        }
        return match decide(&document.autonomy, proposal, GateVerdict::Allow) {
            Err(e) if e.code() == "nothing_proposed" => Ok(()),
            other => Err(format!(
                "`mandate_builder::decide` must refuse a hold with `nothing_proposed`, and returned \
                 {other:?}"
            )),
        };
    };
    let scene = Scene::read(fixture, document, stated, config, GoalState::Running)?;
    let proposed = order.proposed(stated.instrument_id()?, scene.next_order_id);
    let decision = gate::evaluate(&scene.input(stated.now, &proposed))
        .map_err(|e| gate_error("evaluate", &e))?;
    compare_dry_run(at_of(expect, "gate_dry_run")?, &decision, &order)
        .map_err(|e| format!("`gate_dry_run`: {e}"))?;
    let verdict = match decision.verdict {
        gate::Verdict::Allow => GateVerdict::Allow,
        gate::Verdict::Deny => GateVerdict::Deny,
        gate::Verdict::Defer => GateVerdict::Defer,
        gate::Verdict::Hold => {
            return Err(
                "the gate holds the order, and §6.2 step 2 gives `decide` no hold to take".into(),
            );
        }
    };
    let outcome =
        decide(&document.autonomy, proposal, verdict).map_err(|e| builder_error("decide", &e))?;
    compare_outcome(at_of(expect, "autonomy")?, &outcome).map_err(|e| format!("`autonomy`: {e}"))
}

/// The three combined figures, the models counted, the four sizes, the clips, and the action.
fn compare_proposal(expect: &Json, proposal: &Proposal) -> Result<(), String> {
    let sizes = &proposal.sizes;
    expect_eq("cap", sizes.cap, exact(expect, "cap")?)?;
    expect_eq("current_mv", sizes.current_mv, exact(expect, "current_mv")?)?;
    for (key, got) in [("target_value", sizes.target_value), ("delta", sizes.delta)] {
        let wanted = match expect.get(key) {
            None => None,
            Some(_) => Some(exact(expect, key)?),
        };
        expect_eq(key, got, wanted)?;
    }
    let used = list_at(expect, "outputs_used")?
        .iter()
        .map(|m| {
            let text = m.as_str().ok_or("`outputs_used` lists a non-string")?;
            ModelId::parse(text).map_err(|e| format!("`outputs_used`: `{text}`: {}", e.code()))
        })
        .collect::<Result<BTreeSet<_>, String>>()?;
    expect_eq("outputs_used", proposal.combined.outputs_used.clone(), used)?;
    let conviction = |key: &str| num(Conviction::parse(str_at(expect, key)?), key);
    expect_eq(
        "combined_conviction",
        proposal.combined.exit_conviction,
        conviction("combined_conviction")?,
    )?;
    expect_eq(
        "buy_conviction",
        proposal.combined.buy_conviction,
        conviction("buy_conviction")?,
    )?;
    expect_eq(
        "combined_score",
        proposal.combined.score,
        num(
            Unit::parse(str_at(expect, "combined_score")?),
            "combined_score",
        )?,
    )?;
    let clips = match expect.get("clipped_by") {
        None => BTreeSet::new(),
        Some(_) => list_at(expect, "clipped_by")?
            .iter()
            .map(|c| {
                c.as_str()
                    .ok_or_else(|| "`clipped_by` lists a non-string".to_owned())
                    .and_then(clip_named)
            })
            .collect::<Result<BTreeSet<_>, String>>()?,
    };
    expect_eq("clipped_by", proposal.clipped_by.clone(), clips)?;
    let wanted = str_at(expect, "action")?;
    match (wanted, &proposal.action) {
        ("hold", Action::Hold { reason }) => {
            absent(expect, &ORDER_KEYS, "a hold")?;
            expect_eq(
                "reason",
                *reason,
                hold_reason_named(str_at(expect, "reason")?)?,
            )
        }
        (
            "buy",
            Action::Buy {
                purpose,
                qty,
                limit_price,
                order_usd,
                ..
            },
        ) => {
            absent(expect, &["reason", "order_type"], "a buy")?;
            ensure(expect.get("clipped_by").is_some(), || {
                "`clipped_by`: a buy states the clips it met, none included".to_owned()
            })?;
            order_fields(expect, (*purpose, *qty, *limit_price, *order_usd))
        }
        (
            "sell",
            Action::Sell {
                purpose,
                qty,
                limit_price,
                order_usd,
                shape,
            },
        ) => {
            absent(expect, &["reason", "clipped_by"], "a sell")?;
            order_fields(expect, (*purpose, *qty, *limit_price, *order_usd))?;
            let wanted = match expect.get("order_type") {
                None => OrderShape::Limit,
                Some(_) => shape_named(str_at(expect, "order_type")?)?,
            };
            expect_eq("order_type", *shape, wanted)
        }
        (wanted, got) => Err(format!(
            "`action`: the case expects `{wanted}`, and the builder proposed {got:?}"
        )),
    }
}

fn order_fields(
    expect: &Json,
    (purpose, qty, limit_price, order_usd): (Purpose, Qty, Price, Usd),
) -> Result<(), String> {
    expect_eq(
        "purpose",
        purpose,
        autonomy::purpose(str_at(expect, "purpose")?)?,
    )?;
    expect_eq("qty", qty, num(Qty::parse(str_at(expect, "qty")?), "qty")?)?;
    expect_eq(
        "limit_price",
        limit_price,
        num(Price::parse(str_at(expect, "limit_price")?), "limit_price")?,
    )?;
    expect_eq(
        "order_usd",
        order_usd,
        num(Usd::parse(str_at(expect, "order_usd")?), "order_usd")?,
    )
}

fn absent(expect: &Json, keys: &[&str], what: &str) -> Result<(), String> {
    keys.iter().try_for_each(|key| {
        ensure(expect.get(*key).is_none(), || {
            format!("`{key}`: the case states one, and {what} has none")
        })
    })
}

fn exact(expect: &Json, key: &str) -> Result<UsdExact, String> {
    num(UsdExact::parse(str_at(expect, key)?), key)
}

/// The verdict, the reason, the purpose the gate assigns, and how an allowed order must be sent: an
/// opening as proposed, and an exit paced, if at all, to the quantity and price the case states and
/// marketable exactly when its `order_type` is.
fn compare_dry_run(stated: &Json, decision: &gate::Decision, order: &Order) -> Result<(), String> {
    unknown_members(stated, DRY_RUN_KEYS)
        .map_err(|unknown| format!("members not interpreted: {unknown}"))?;
    let verdict = verdict_named(str_at(stated, "verdict")?)?;
    let reason = match at(stated, "reason")? {
        Json::Null => None,
        Json::String(text) => Some(reason_named(text)?),
        _ => return Err("`reason` is neither a reason code nor null".to_owned()),
    };
    expect_eq("verdict", decision.verdict, verdict)
        .map_err(|e| format!("{e}, the gate's reason {:?}", decision.reason))?;
    expect_eq("reason", decision.reason, reason)?;
    expect_eq(
        "the purpose the gate assigns",
        decision.purpose,
        gate_purpose(order.purpose),
    )?;
    match (decision.verdict, order.side, &decision.pacing) {
        (gate::Verdict::Allow, gate::Side::Sell, pacing) => {
            expect_eq(
                "order_type, as the gate paces the exit",
                pacing.as_ref().is_some_and(|p| p.marketable_limit_required),
                order.marketable,
            )?;
            match pacing {
                Some(p) => {
                    expect_eq("the paced quantity", p.qty, order.qty)?;
                    expect_eq("the paced limit price", p.limit_price, order.limit_price)
                }
                None => Ok(()),
            }
        }
        (_, _, None) => Ok(()),
        (_, _, Some(p)) => Err(format!(
            "the gate paced an order it did not allow as an exit: {p:?}"
        )),
    }
}

/// A skip and a defer are the dry run's, labelled `gate_dry_run` (DEC-130 item 14); a classified
/// outcome is compared as family A compares one.
fn compare_outcome(stated: &Json, outcome: &Outcome) -> Result<(), String> {
    let gate_outcome = match outcome {
        Outcome::Skipped => "skipped",
        Outcome::Deferred => "deferred",
        Outcome::Classified(classified) => return autonomy::compare(stated, classified),
    };
    unknown_members(stated, &["decision", "by"])
        .map_err(|unknown| format!("expectations not interpreted: {unknown}"))?;
    expect_eq("decision", gate_outcome, str_at(stated, "decision")?)?;
    expect_eq("by", "gate_dry_run", str_at(stated, "by")?)
}

/// The order a buy or a sell proposes, as the gate is asked about it.
struct Order {
    side: gate::Side,
    purpose: Purpose,
    qty: Qty,
    limit_price: Price,
    marketable: bool,
}

impl Order {
    fn of(action: &Action) -> Option<Self> {
        match action {
            Action::Hold { .. } => None,
            Action::Buy {
                purpose,
                qty,
                limit_price,
                ..
            } => Some(Self {
                side: gate::Side::Buy,
                purpose: *purpose,
                qty: *qty,
                limit_price: *limit_price,
                marketable: false,
            }),
            Action::Sell {
                purpose,
                qty,
                limit_price,
                shape,
                ..
            } => Some(Self {
                side: gate::Side::Sell,
                purpose: *purpose,
                qty: *qty,
                limit_price: *limit_price,
                marketable: *shape == OrderShape::MarketableLimit,
            }),
        }
    }

    /// The builder is the origin of every order it proposes; the gate assigns the purpose from that,
    /// the side and the position (§6.1, DEC-129 item 19). A marketable exit goes as a plain limit and
    /// the gate's pacing says it must be marketable, since the proposal has no marketable kind.
    fn proposed(&self, instrument: gate::AssetId, id: gate::ClientOrderId) -> gate::ProposedOrder {
        gate::ProposedOrder {
            instrument,
            side: self.side,
            qty: self.qty,
            limit_price: self.limit_price,
            kind: gate::ProposedKind::Plain,
            tif: gate::TimeInForce::Day,
            extended_hours: false,
            origin: gate::Origin::OrderBuilder,
            owner_confirmed_bid: None,
            client_order_id: id,
            fee_reservation: Usd::ZERO,
        }
    }
}

/// A case's `input`, read whole, with the scene's cross-checks applied.
struct Inputs {
    now: UtcNanos,
    instrument: AssetId,
    asset_class: AssetClass,
    position: Qty,
    bid: Price,
    ask: Price,
    increment: Qty,
    min_order_usd: Usd,
    fee_rate_cash: FeeRate,
    fee_rate_asset: FeeRate,
    account: AccountSnapshot,
    risk: RiskContext,
    gate_size_factor: Ratio,
    outputs: Vec<ModelOutput>,
    state: GateState,
    stated_session: MarketSession,
    stated_close_window: bool,
}

impl Inputs {
    fn read(document: &Mandate, input: &Json) -> Result<Self, String> {
        let now = instant(at(input, "now")?, "now")?;
        let instrument = asset(str_at(input, "instrument")?, "instrument")?;
        let asset_class = AssetClass::parse(str_at(input, "asset_class")?)
            .map_err(|e| format!("`asset_class`: {}", e.code()))?;
        let pinned = document
            .universe
            .pinned_instruments
            .iter()
            .find(|entry| entry.asset_id == instrument)
            .ok_or("`instrument` is not a pinned instrument of the base")?;
        expect_eq(
            "asset_class, beside the pinned instrument's",
            asset_class,
            pinned.asset_class,
        )?;
        ensure(document.universe.pinned, || {
            "the base does not pin its universe, so the case would have to say whether the order is \
             an admission (`new_instrument`) and on what thesis (`thesis_confidence`)"
                .to_owned()
        })?;
        let usd = |key: &str| num(Usd::parse(str_at(input, key)?), key);
        let agent_equity = usd("agent_equity")?;
        let position = num(Qty::parse(str_at(input, "position_qty")?), "position_qty")?;
        let quote = at_of(input, "quote")?;
        unknown_members(quote, QUOTE_KEYS)
            .map_err(|unknown| format!("`quote` members not interpreted: {unknown}"))?;
        let bid_text = str_at(quote, "bid")?;
        let bid = num(Price::parse(bid_text), "quote.bid")?;
        let ask = num(Price::parse(str_at(quote, "ask")?), "quote.ask")?;
        let state = GateState::read(at_of(input, "gate_state")?)?;

        expect_eq(
            "gate_state.agent_equity, beside `agent_equity`",
            state.agent_equity,
            agent_equity,
        )?;
        let held = gate_asset(&instrument)?;
        match state.positions_mv.get(&held) {
            Some(value) => expect_eq(
                "gate_state.positions_mv, for the instrument at position_qty × bid",
                *value,
                num(position.notional(bid), "position_qty × bid")?,
            )?,
            None => ensure(position.is_zero(), || {
                "`gate_state.positions_mv` has no entry for the instrument the agent holds"
                    .to_owned()
            })?,
        }
        let in_instrument: Vec<(gate::AssetId, Usd)> = state
            .working
            .iter()
            .filter(|(order, _)| *order == held)
            .cloned()
            .collect();
        if let Some(listed) = input.get("working_opening_orders") {
            expect_eq(
                "working_opening_orders, beside the gate state's in the instrument",
                working_orders(listed, "working_opening_orders")?,
                in_instrument.clone(),
            )?;
        }
        let working_cost = sum(in_instrument.iter().map(|(_, cost)| *cost))?;
        let gross = sum(state
            .positions_mv
            .values()
            .copied()
            .chain(state.working.iter().map(|(_, cost)| *cost)))?;
        if input.get("gross_usd").is_some() {
            expect_eq(
                "gross_usd, beside the gate state's positions and working orders",
                usd("gross_usd")?,
                gross,
            )?;
        }

        let fee = |key: &str| num(FeeRate::parse(text_or(input, key, "0")?), key);
        let fee_rate_cash = fee("fee_rate_cash")?;
        let fee_rate_asset = fee("fee_rate_asset")?;
        let no_fee = num(FeeRate::parse("0"), "a zero fee rate")?;
        ensure(
            asset_class == AssetClass::Crypto
                || (fee_rate_cash == no_fee && fee_rate_asset == no_fee),
            || {
                "an equity fee rate: the fixture states no fee schedule to reserve an equity \
                 buy's fees from (§9.5)"
                    .to_owned()
            },
        )?;
        let size_factor = text_or(input, "size_factor", "1")?;
        let outputs = list_at(input, "outputs")?
            .iter()
            .enumerate()
            .map(|(index, output)| model_output(index, output))
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Self {
            now,
            instrument,
            asset_class,
            position,
            bid,
            ask,
            increment: num(Qty::parse(str_at(input, "qty_increment")?), "qty_increment")?,
            min_order_usd: usd("min_order_usd")?,
            fee_rate_cash,
            fee_rate_asset,
            account: AccountSnapshot {
                agent_equity,
                position_qty: position,
                cost_basis: num(
                    CostBasis::parse(text_or(input, "cost_basis_usd", "0")?),
                    "cost_basis_usd",
                )?,
                risk_mark: num(MarkPrice::parse(bid_text), "quote.bid")?,
                gross_usd: gross,
                working_opening_cost: working_cost,
                goal_spent_usd: num(
                    Usd::parse(text_or(input, "goal_spent_usd", "0")?),
                    "goal_spent_usd",
                )?,
            },
            risk: RiskContext {
                size_factor: num(SizeFraction::parse(size_factor), "size_factor")?,
                drawdown: num(Unit::parse(text_or(input, "drawdown", "0")?), "drawdown")?,
                daily_pnl_fraction: num(
                    Signed::parse(text_or(input, "daily_pnl_fraction", "0")?),
                    "daily_pnl_fraction",
                )?,
                position_pnl_fraction: num(
                    Signed::parse(text_or(input, "position_pnl_fraction", "0")?),
                    "position_pnl_fraction",
                )?,
                bought_today_usd: num(
                    Usd::parse(text_or(input, "bought_today_usd", "0")?),
                    "bought_today_usd",
                )?,
                has_prior_fill: flag_or(input, "has_prior_fill", !position.is_zero())?,
                new_instrument: false,
                thesis_confidence: Unit::ZERO,
            },
            gate_size_factor: num(Ratio::parse(size_factor), "size_factor")?,
            outputs,
            state,
            stated_session: MarketSession::parse_condition_form(text_or(
                input, "session", "regular",
            )?)
            .map_err(|e| format!("`session`: {}", e.code()))?,
            stated_close_window: flag_or(input, "in_close_window", false)?,
        })
    }

    fn gate_class(&self) -> gate::AssetClass {
        match self.asset_class {
            AssetClass::UsEquity => gate::AssetClass::UsEquity,
            AssetClass::Crypto => gate::AssetClass::Crypto,
        }
    }

    fn instrument_id(&self) -> Result<gate::AssetId, String> {
        gate_asset(&self.instrument)
    }

    /// The market the builder reads, its session and close window from the gate's own clock.
    fn market(&self, clock: &gate::SessionAt) -> Market {
        Market {
            instrument: self.instrument.clone(),
            asset_class: self.asset_class,
            session: market_session(clock.session),
            in_close_window: clock.close_window,
            bid: self.bid,
            ask: self.ask,
            increment: self.increment,
            min_order_usd: self.min_order_usd,
            fee_rate_cash: self.fee_rate_cash,
            fee_rate_asset: self.fee_rate_asset,
        }
    }

    /// The case's `session` and `in_close_window` against the calendar at `now`, compared last, so a
    /// case whose labels contradict its clock fails at the first figure they change and says why.
    fn agrees_with_clock(
        &self,
        clock: &gate::SessionAt,
        judged: Result<(), String>,
    ) -> Result<(), String> {
        let derived = market_session(clock.session);
        let mut conflicts = Vec::new();
        if self.stated_session != derived {
            conflicts.push(format!(
                "`session`: the case states `{}`, and the calendar says `{}` at `now` (DEC-129 item 9)",
                self.stated_session.as_str(),
                derived.as_str()
            ));
        }
        if self.stated_close_window != clock.close_window {
            conflicts.push(format!(
                "`in_close_window`: the case states {}, and the calendar says {} at `now`",
                self.stated_close_window, clock.close_window
            ));
        }
        match (judged, conflicts.is_empty()) {
            (Ok(()), true) => Ok(()),
            (Ok(()), false) => Err(conflicts.join("; ")),
            (Err(e), true) => Err(e),
            (Err(e), false) => Err(format!("{e}; and {}", conflicts.join("; "))),
        }
    }
}

/// The dry run's view of the account, as the case states it.
struct GateState {
    agent_equity: Usd,
    positions_mv: BTreeMap<gate::AssetId, Usd>,
    working: Vec<(gate::AssetId, Usd)>,
    orders_today: u32,
    last_exit_fill_at: BTreeMap<gate::AssetId, UtcNanos>,
    working_universe: BTreeSet<gate::AssetId>,
}

impl GateState {
    fn read(state: &Json) -> Result<Self, String> {
        unknown_members(state, GATE_STATE_KEYS)
            .map_err(|unknown| format!("`gate_state` members not interpreted: {unknown}"))?;
        let mut positions_mv = BTreeMap::new();
        for (held, value) in object_at(state, "positions_mv")? {
            let value = value
                .as_str()
                .ok_or("`gate_state.positions_mv` holds a non-string")?;
            positions_mv.insert(
                gate_asset(&asset(held, "gate_state.positions_mv")?)?,
                num(Usd::parse(value), "gate_state.positions_mv")?,
            );
        }
        let mut last_exit_fill_at = BTreeMap::new();
        for (exited, when) in object_at(state, "last_exit_fill_at")? {
            last_exit_fill_at.insert(
                gate_asset(&asset(exited, "gate_state.last_exit_fill_at")?)?,
                instant(when, "gate_state.last_exit_fill_at")?,
            );
        }
        let listed = list_at(state, "working_universe")?;
        let working_universe = listed
            .iter()
            .map(|i| {
                let text = i
                    .as_str()
                    .ok_or("`gate_state.working_universe` lists a non-string")?;
                gate_asset(&asset(text, "gate_state.working_universe")?)
            })
            .collect::<Result<BTreeSet<_>, String>>()?;
        ensure(working_universe.len() == listed.len(), || {
            "`gate_state.working_universe` names an instrument twice".to_owned()
        })?;
        Ok(Self {
            agent_equity: num(
                Usd::parse(str_at(state, "agent_equity")?),
                "gate_state.agent_equity",
            )?,
            positions_mv,
            working: working_orders(
                at(state, "working_opening_orders")?,
                "gate_state.working_opening_orders",
            )?,
            orders_today: u32::try_from(u64_at(state, "orders_today")?)
                .map_err(|_| "`gate_state.orders_today` does not fit a u32".to_owned())?,
            last_exit_fill_at,
            working_universe,
        })
    }
}

fn working_orders(listed: &Json, what: &str) -> Result<Vec<(gate::AssetId, Usd)>, String> {
    listed
        .as_array()
        .ok_or_else(|| format!("`{what}` is not a list"))?
        .iter()
        .map(|order| {
            unknown_members(order, WORKING_ORDER_KEYS)
                .map_err(|unknown| format!("`{what}` order members not interpreted: {unknown}"))?;
            Ok((
                gate_asset(&asset(str_at(order, "instrument")?, what)?)?,
                num(Usd::parse(str_at(order, "max_cost")?), what)?,
            ))
        })
        .collect()
}

/// One §8.2 output. `direction` is not stated because v1 has one (DEC-32).
fn model_output(index: usize, output: &Json) -> Result<ModelOutput, String> {
    let what = |key: &str| format!("outputs[{index}].{key}");
    unknown_members(output, OUTPUT_KEYS)
        .map_err(|unknown| format!("`outputs[{index}]` members not interpreted: {unknown}"))?;
    let text = |key: &str| str_at(output, key).map_err(|e| format!("`{}`: {e}", what(key)));
    Ok(ModelOutput {
        model_id: ModelId::parse(text("model_id")?)
            .map_err(|e| format!("`{}`: {}", what("model_id"), e.code()))?,
        model_version: ModelVersion::parse(text("model_version")?)
            .map_err(|e| format!("`{}`: {}", what("model_version"), e.code()))?,
        content_hash: digest(text("content_hash")?)
            .map_err(|e| format!("`{}`: {e}", what("content_hash")))?,
        instrument: asset(text("instrument_id")?, &what("instrument_id"))?,
        as_of: instant(at(output, "as_of")?, &what("as_of"))?,
        expires_at: instant(at(output, "expires_at")?, &what("expires_at"))?,
        direction: Direction::Long,
        conviction: num(Conviction::parse(text("conviction")?), &what("conviction"))?,
        confidence: num(Unit::parse(text("confidence")?), &what("confidence"))?,
    })
}

/// The validated document's §8 fields as `mandate-builder`'s view, which `mandate-spec` does not
/// supply yet (DEC-130 item 5); a value too wide for its type is refused, never approximated.
fn builder_mandate(document: &Mandate) -> Result<BuilderMandate, String> {
    let fraction = |value: &mandate_spec::SchemaDec, what: &str| {
        num(SizeFraction::parse(value.as_str()), what)
    };
    let usd = |value: &mandate_spec::SchemaDec, what: &str| num(Usd::parse(value.as_str()), what);
    let behavior = &document.behavior;
    let models = behavior
        .signal_models
        .iter()
        .map(|m| {
            Ok(SignalModel {
                id: m.id.clone(),
                version: ModelVersion::parse(&m.version)
                    .map_err(|e| format!("signal model `{}`: {}", m.id.as_str(), e.code()))?,
                content_hash: m.content_hash,
                weight: fraction(&m.weight, "signal_models.weight")?,
                max_output_age_s: m.max_output_age_s,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    let sizing = &behavior.sizing;
    let risk = &document.risk;
    Ok(BuilderMandate {
        models,
        sizing: Sizing {
            method: sizing.method,
            entry_threshold: fraction(&sizing.entry_threshold, "entry_threshold")?,
            exit_threshold: fraction(&sizing.exit_threshold, "exit_threshold")?,
            rebalance_band: fraction(&sizing.rebalance_band, "rebalance_band")?,
        },
        limits: Limits {
            max_position_usd: usd(&risk.max_position_usd, "max_position_usd")?,
            max_position_fraction: fraction(&risk.max_position_fraction, "max_position_fraction")?,
            max_order_usd: usd(&risk.max_order_usd, "max_order_usd")?,
            max_gross_exposure_usd: usd(&risk.max_gross_exposure_usd, "max_gross_exposure_usd")?,
        },
        goal: match &document.goal {
            Goal::Continuous { .. } => GoalKind::Continuous,
            Goal::ProfitStop { .. } => GoalKind::ProfitStop,
            Goal::Accumulate {
                instrument,
                target_qty,
                max_avg_price,
                max_spend_usd,
                ..
            } => GoalKind::Accumulate(AccumulateGoal {
                instrument: instrument.clone(),
                target_qty: num(Qty::parse(target_qty.as_str()), "target_qty")?,
                max_avg_price: max_avg_price
                    .as_ref()
                    .map(|p| num(Price::parse(p.as_str()), "max_avg_price"))
                    .transpose()?,
                max_spend_usd: usd(max_spend_usd, "max_spend_usd")?,
            }),
        },
    })
}

/// Everything one dry run reads but the proposal, owned, so [`Scene::input`] can lend it out.
struct Scene {
    mandate: gate::ValidatedMandate,
    config: gate::GateConfig,
    risk: gate::RiskSnapshot,
    account: gate::AccountSnapshot,
    agent: gate::AgentSnapshot,
    instrument: gate::InstrumentSnapshot,
    market: gate::MarketSnapshot,
    conduct: gate::ConductState,
    universe: gate::WorkingUniverse,
    next_order_id: gate::ClientOrderId,
}

impl Scene {
    fn read(
        fixture: &Json,
        document: &Mandate,
        stated: &Inputs,
        config: &gate::GateConfig,
        goal_state: GoalState,
    ) -> Result<Self, String> {
        let account_equity = num(
            Usd::parse(str_at(
                at_of(fixture, "validation_context_defaults")?,
                "account_equity_usd",
            )?),
            "account_equity_usd",
        )?;
        let instrument = stated.instrument_id()?;
        let one = num(Qty::parse("1"), "one share")?;
        let state = &stated.state;
        let positions: BTreeMap<gate::AssetId, Qty> = state
            .positions_mv
            .keys()
            .map(|held| {
                let qty = if *held == instrument {
                    stated.position
                } else {
                    one
                };
                (held.clone(), qty)
            })
            .collect();
        let (today, _) = new_york_date_and_hour(stated.now).map_err(|e| format!("`now`: {e}"))?;
        let mut working_orders = BTreeMap::new();
        let mut next_id: u64 = 1;
        for (order, max_cost) in &state.working {
            working_orders.insert(
                gate::ClientOrderId(next_id),
                gate::WorkingOrder {
                    agent: THE_AGENT,
                    instrument: order.clone(),
                    side: gate::Side::Buy,
                    max_cost: *max_cost,
                    open_qty: one,
                    protective: false,
                    opening: true,
                    submitted_on: today,
                },
            );
            next_id = next_id.saturating_add(1);
        }
        let agent_equity = stated.account.agent_equity;
        Ok(Self {
            mandate: gate_mandate(document, goal_state)?,
            config: config.clone(),
            risk: gate::RiskSnapshot {
                agent_equity,
                high_water_mark: agent_equity,
                day_start_equity: agent_equity,
                capital_base: agent_equity,
                inherited_loss: Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: stated.gate_size_factor,
                agent_mode: gate::AgentMode::Normal,
            },
            account: gate::AccountSnapshot {
                account_type: gate::AccountType::Margin,
                state: gate::AccountState::Active,
                crypto_active: true,
                regime: gate::DayTradeRegime::IntradayMargin {
                    maintenance_excess: account_equity,
                },
                equity: account_equity,
                prior_close_equity: account_equity,
                model_buying_power: account_equity,
                broker_buying_power: account_equity,
                broker_non_marginable_buying_power: account_equity,
                positions: positions.clone(),
                market_values: state.positions_mv.clone(),
                working_orders: working_orders.clone(),
                unknown_orders: BTreeSet::new(),
                related_account_resting: BTreeMap::new(),
            },
            agent: gate::AgentSnapshot {
                agent: THE_AGENT,
                mode: gate::AgentMode::Normal,
                instrument_restrictions: BTreeMap::new(),
                positions,
                market_values: state.positions_mv.clone(),
                working_orders: working_orders.keys().copied().collect(),
                instrument_groups: BTreeMap::new(),
                last_exit_fill_at: state.last_exit_fill_at.clone(),
                orders_today: state.orders_today,
                day_trades: gate::DayTradeLedger::default(),
            },
            instrument: listing(stated, instrument, one)?,
            market: gate::MarketSnapshot {
                quote: Some(gate::SaneQuote {
                    bid: stated.bid,
                    ask: stated.ask,
                    at: stated.now,
                }),
                last_trade: None,
                trailing_5m_volume: Some(num(Qty::parse("1000000"), "trailing_5m_volume")?),
                adv_20d: Some(num(Qty::parse("10000000"), "adv_20d")?),
            },
            conduct: gate::ConductState::default(),
            universe: gate::WorkingUniverse::Known {
                instruments: state.working_universe.clone(),
                pinned: document.universe.pinned,
            },
            next_order_id: gate::ClientOrderId(next_id),
        })
    }

    fn input<'a>(
        &'a self,
        now: UtcNanos,
        proposed: &'a gate::ProposedOrder,
    ) -> gate::GateInput<'a> {
        gate::GateInput {
            now,
            pass: gate::GatePass::First,
            config: &self.config,
            mandate: &self.mandate,
            risk: &self.risk,
            account: &self.account,
            agent: &self.agent,
            instrument: &self.instrument,
            market: &self.market,
            conduct: &self.conduct,
            universe: &self.universe,
            proposed,
        }
    }
}

/// The pinned instrument as the gate's eligibility floor, sessions and collar see it: tradable,
/// unhalted and liquid, last closed at the case's bid, fractional exactly when its increment is
/// below one unit, and never refusing a quantity on the increment the case sizes to.
fn listing(
    stated: &Inputs,
    instrument: gate::AssetId,
    one: Qty,
) -> Result<gate::InstrumentSnapshot, String> {
    let equity = stated.asset_class == AssetClass::UsEquity;
    let liquid = num(Usd::parse("90000000"), "the median dollar volume")?;
    Ok(gate::InstrumentSnapshot {
        instrument,
        asset_class: stated.gate_class(),
        exchange: equity.then_some(gate::Exchange::Nasdaq),
        status_active: true,
        tradable: true,
        fractionable: stated.increment < one,
        ipo: false,
        ptp_no_exception: false,
        etp: gate::EtpClass::Plain,
        etp_classified_at: Some(stated.now),
        quote_currency: equity.then_some(gate::QuoteCurrency::Usd),
        prior_close: Some(stated.bid),
        median_dollar_volume_20d: Some(liquid),
        median_dollar_volume_30d: Some(liquid),
        min_order_size: stated.increment,
        halted: false,
        status_feed_current: true,
    })
}

/// `configs.test_default` of the trading-domain reference cases, with the five settings it does not
/// carry (the crypto liquidity floor, the minimum resting time, the two participation caps, and the
/// ETP classification age) at the values `mandate-risk`'s own `test_default_config` uses.
fn test_default_gate_config() -> Result<gate::GateConfig, String> {
    let usd = |text: &str| num(Usd::parse(text), "the gate configuration");
    let fraction = |text: &str| num(Fraction::parse(text), "the gate configuration");
    Ok(gate::GateConfig {
        price_floor: usd("5")?,
        liquidity_floor_usd: usd("1000000")?,
        crypto_liquidity_floor_usd: usd("1000000")?,
        collar_liquid_threshold_usd: usd("50000000")?,
        collar_liquid_x: fraction("0.01")?,
        collar_other_x: fraction("0.02")?,
        collar_crypto_x: fraction("0.02")?,
        collar_passive_band: fraction("0.2")?,
        opposite_fill_interval_s: 60,
        min_resting_time_s: 2,
        order_to_fill_max: 10,
        order_to_fill_min_orders: 20,
        order_size_participation: fraction("0.05")?,
        daily_participation: fraction("0.05")?,
        close_window_minutes: 10,
        legacy_pdt_equity_threshold: usd("25000")?,
        etp_classification_max_age_s: 604_800,
    })
}

/// The validated document's risk block, sizing band and universe flags as `mandate-risk`'s
/// stand-in `ValidatedMandate`. A rung's `scale_action` is the block's, which the schema states once
/// for the whole ladder.
fn gate_mandate(
    document: &Mandate,
    goal_state: GoalState,
) -> Result<gate::ValidatedMandate, String> {
    let r = &document.risk;
    let usd = |value: &mandate_spec::SchemaDec, what: &str| num(Usd::parse(value.as_str()), what);
    let fraction =
        |value: &mandate_spec::SchemaDec, what: &str| num(Fraction::parse(value.as_str()), what);
    let scale_action = match r.scale_action {
        spec_doc::ScaleAction::LimitBuys => ScaleAction::LimitBuys,
        spec_doc::ScaleAction::TrimToTarget => ScaleAction::TrimToTarget,
    };
    let drawdown_ladder = r
        .drawdown_ladder
        .iter()
        .enumerate()
        .map(|(index, rung)| {
            Ok(Rung {
                index: u8::try_from(index).map_err(|_| "the ladder has too many rungs")?,
                at: fraction(&rung.at, "drawdown_ladder.at")?,
                action: match rung.action {
                    LadderAction::ScaleSizes => RungAction::ScaleSizes,
                    LadderAction::ExitsOnly => RungAction::ExitsOnly,
                    LadderAction::FlattenAndPause => RungAction::FlattenAndPause,
                },
                factor: rung
                    .factor
                    .as_ref()
                    .map(|f| fraction(f, "drawdown_ladder.factor"))
                    .transpose()?,
                scale_action: Some(scale_action),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok(gate::ValidatedMandate::from_validated_parts(
        RiskLimits {
            max_position_usd: usd(&r.max_position_usd, "max_position_usd")?,
            max_position_fraction: fraction(&r.max_position_fraction, "max_position_fraction")?,
            max_order_usd: usd(&r.max_order_usd, "max_order_usd")?,
            max_gross_exposure_usd: usd(&r.max_gross_exposure_usd, "max_gross_exposure_usd")?,
            max_orders_per_day: r.max_orders_per_day,
            reentry_cooldown_s: r.reentry_cooldown_s,
            rebalance_band: fraction(&document.behavior.sizing.rebalance_band, "rebalance_band")?,
            breach_confirm_s: r.breach_confirm_s,
            drawdown_ladder,
        },
        goal_state,
        document.universe.leveraged_etps_enabled,
        document.universe.leveraged_etp_disclosure_version.is_some(),
    ))
}

fn market_session(session: gate::Session) -> MarketSession {
    match session {
        gate::Session::Overnight => MarketSession::Overnight,
        gate::Session::PreMarket => MarketSession::PreMarket,
        gate::Session::Regular => MarketSession::Regular,
        gate::Session::AfterHours => MarketSession::AfterHours,
        gate::Session::Continuous => MarketSession::Crypto,
    }
}

fn gate_purpose(purpose: Purpose) -> gate::Purpose {
    match purpose {
        Purpose::Open => gate::Purpose::Open,
        Purpose::Increase => gate::Purpose::Increase,
        Purpose::DiscretionaryExit => gate::Purpose::DiscretionaryExit,
        Purpose::OwnerExit => gate::Purpose::OwnerExit,
        Purpose::RiskExit => gate::Purpose::RiskExit,
        Purpose::Protective => gate::Purpose::Protective,
    }
}

/// §8.3's hold reasons, spelt as the fixture writes them rather than taken from the crate, so the
/// fixture's word is checked against the crate's and not with it.
fn hold_reason_named(text: &str) -> Result<HoldReason, String> {
    match text {
        "no_fresh_outputs" => Ok(HoldReason::NoFreshOutputs),
        "no_position" => Ok(HoldReason::NoPosition),
        "discretionary_exits_disabled" => Ok(HoldReason::DiscretionaryExitsDisabled),
        "between_thresholds" => Ok(HoldReason::BetweenThresholds),
        "at_or_above_target" => Ok(HoldReason::AtOrAboveTarget),
        "within_rebalance_band" => Ok(HoldReason::WithinRebalanceBand),
        "below_band_after_clipping" => Ok(HoldReason::BelowBandAfterClipping),
        "would_exceed_max_avg_price" => Ok(HoldReason::WouldExceedMaxAvgPrice),
        "below_minimum_after_clipping" => Ok(HoldReason::BelowMinimumAfterClipping),
        other => Err(format!("`reason`: `{other}` is not a §8.3 hold reason")),
    }
}

fn clip_named(text: &str) -> Result<Clip, String> {
    match text {
        "limits" => Ok(Clip::Limits),
        "goal" => Ok(Clip::Goal),
        other => Err(format!("`clipped_by`: `{other}` is not a §8.3 clip")),
    }
}

fn shape_named(text: &str) -> Result<OrderShape, String> {
    match text {
        "limit" => Ok(OrderShape::Limit),
        "marketable_limit" => Ok(OrderShape::MarketableLimit),
        other => Err(format!("`order_type`: `{other}` is not an order shape")),
    }
}

fn verdict_named(text: &str) -> Result<gate::Verdict, String> {
    match text {
        "allow" => Ok(gate::Verdict::Allow),
        "deny" => Ok(gate::Verdict::Deny),
        "defer" => Ok(gate::Verdict::Defer),
        "hold" => Ok(gate::Verdict::Hold),
        other => Err(format!("`verdict`: `{other}` is not a verdict")),
    }
}

/// A reason code by its registered spelling, searched over every variant so a correct gate can only
/// fail here on a code the crate cannot emit.
fn reason_named(text: &str) -> Result<gate::ReasonCode, String> {
    gate::ReasonCode::ALL
        .iter()
        .copied()
        .find(|code| code.as_str() == text)
        .ok_or_else(|| format!("`reason`: `{text}` is not a registered reason code"))
}

fn asset(text: &str, what: &str) -> Result<AssetId, String> {
    AssetId::parse(text).map_err(|e| format!("`{what}`: `{text}`: {}", e.code()))
}

fn gate_asset(id: &AssetId) -> Result<gate::AssetId, String> {
    gate::AssetId::new(id.as_str()).map_err(|e| format!("`{id}`: {e}"))
}

fn sum(values: impl Iterator<Item = Usd>) -> Result<Usd, String> {
    values.into_iter().try_fold(Usd::ZERO, |total, value| {
        num(total.checked_add(value), "a sum of dollar figures")
    })
}

/// A member the fixture header or `ref.py` gives a default, as text; a stated non-string fails.
fn text_or<'a>(value: &'a Json, key: &str, default: &'a str) -> Result<&'a str, String> {
    match value.get(key) {
        None => Ok(default),
        Some(stated) => stated
            .as_str()
            .ok_or_else(|| format!("`{key}` is not a string")),
    }
}

fn flag_or(value: &Json, key: &str, default: bool) -> Result<bool, String> {
    match value.get(key) {
        None => Ok(default),
        Some(stated) => stated
            .as_bool()
            .ok_or_else(|| format!("`{key}` is not a boolean")),
    }
}

fn object_at<'a>(value: &'a Json, key: &str) -> Result<&'a serde_json::Map<String, Json>, String> {
    at(value, key)?
        .as_object()
        .ok_or_else(|| format!("`gate_state.{key}` is not an object"))
}

/// A `mandate-builder` refusal, with `unimplemented` turned into the message DEC-77 requires.
fn builder_error(what: &str, error: &BuilderError) -> String {
    if error.code() == "unimplemented" {
        not_implemented(&format!("`mandate_builder::{what}`"))
    } else {
        format!("`mandate_builder::{what}`: {error} ({})", error.code())
    }
}

/// A `mandate-risk` refusal to decide, which is never a pass: an owed check names its story.
fn gate_error(what: &str, e: &gate::GateError) -> String {
    format!("`mandate_risk::{what}`: {e} ({})", e.code())
}

/// The harness's own oracle: a family-B case passes only because every member was read and
/// compared, and a case that cannot pass fails at its owner. Each test doctors the real fixture and
/// runs the case through the suite, so the suite's own sweeps are part of what is tested.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use serde_json::json;

    use super::{gate, gate_error, instant, market_session, test_default_gate_config};
    use crate::{Json, mandate, read_fixture};

    const PLANTED: &str = "zz_planted";

    /// The cases that pass against the merged `mandate-builder` and `mandate-risk`.
    const PASSING: [&str; 23] = [
        "MC-B01", "MC-B02", "MC-B03", "MC-B04", "MC-B05", "MC-B06", "MC-B07", "MC-B08", "MC-B09",
        "MC-B10", "MC-B11", "MC-B12", "MC-B13", "MC-B14", "MC-B15", "MC-B16", "MC-B18", "MC-B19",
        "MC-B20", "MC-B21", "MC-B24", "MC-B25", "MC-B29",
    ];

    /// The cases that cannot pass yet, each with what its failure must say.
    const OWED: [(&str, &[&str]); 7] = [
        ("MC-B17", &["mandate_risk::trim_proposals", "pending E6-4"]),
        ("MC-B30", &["mandate_risk::trim_proposals", "pending E6-4"]),
        ("MC-B31", &["mandate_risk::trim_proposals", "pending E6-4"]),
        ("MC-B32", &["mandate_risk::trim_proposals", "pending E6-4"]),
        ("MC-B26", &["mandate_risk::evaluate", "pending E6-10"]),
        ("MC-B27", &["mandate_risk::evaluate", "pending E6-10"]),
        ("MC-B28", &["mandate_risk::evaluate", "pending E6-10"]),
    ];

    /// The cases whose clock DEC-250 item 12 moves onto their labels, judged by the fixture's own
    /// consistency: each passes when its `session` and `in_close_window` agree with the calendar at
    /// its `now`, and otherwise fails naming the conflict as its needles say. MC-B31 carries the
    /// same `after_hours` label but is not here: a `trim_to_target` case stops at
    /// `trim_proposals` before its labels are compared, so it is owed to E6-4 either way.
    const CLOCKED: [(&str, &[&str]); 2] = [
        (
            "MC-B22",
            &[
                "`session`",
                "states `after_hours`",
                "the calendar says `regular`",
            ],
        ),
        (
            "MC-B23",
            &[
                "`in_close_window`",
                "states true",
                "the calendar says false",
            ],
        ),
    ];

    /// Whether case `id`'s `session` and `in_close_window`, defaulting to `regular` and false as
    /// the case-file header says, agree with `mandate_risk::session_at` at its `now` under the gate
    /// configuration the harness runs. The labels are compared here rather than by the harness's
    /// own comparison, so a harness that stops comparing them cannot also change what is expected.
    fn labels_agree_with_calendar(fixture: &Json, id: &str) -> Result<bool, String> {
        let case = family_b(fixture)?
            .into_iter()
            .find(|c| c["id"] == id)
            .ok_or_else(|| format!("no case {id}"))?;
        let input = crate::at(&case, "input")?;
        let class = match crate::str_at(input, "asset_class")? {
            "us_equity" => gate::AssetClass::UsEquity,
            "crypto" => gate::AssetClass::Crypto,
            other => return Err(format!("{id}: asset class `{other}`")),
        };
        let now = instant(crate::at(input, "now")?, "now")?;
        let clock = gate::session_at(now, &test_default_gate_config()?, class)
            .map_err(|e| gate_error("session_at", &e))?;
        let session = input
            .get("session")
            .map_or(Some("regular"), Json::as_str)
            .ok_or_else(|| format!("{id}: `session` is not text"))?;
        let close_window = input
            .get("in_close_window")
            .map_or(Some(false), Json::as_bool)
            .ok_or_else(|| format!("{id}: `in_close_window` is not a flag"))?;
        Ok(session == market_session(clock.session).as_str() && close_window == clock.close_window)
    }

    fn fixture() -> Result<Json, String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
        read_fixture(&dir, "mandate.json").map(Arc::unwrap_or_clone)
    }

    fn family_b(fixture: &Json) -> Result<Vec<Json>, String> {
        let cases: Vec<Json> = crate::list_at(fixture, "cases")?
            .iter()
            .filter(|c| c["kind"] == "builder")
            .cloned()
            .collect();
        crate::ensure(cases.len() == 32, || {
            format!("family B is thirty-two cases, found {}", cases.len())
        })?;
        Ok(cases)
    }

    fn passing(fixture: &Json) -> Result<Vec<Json>, String> {
        Ok(family_b(fixture)?
            .into_iter()
            .filter(|c| c["id"].as_str().is_some_and(|id| PASSING.contains(&id)))
            .collect())
    }

    fn id(case: &Json) -> Result<String, String> {
        crate::str_at(case, "id").map(str::to_owned)
    }

    fn run(fixture: Json, id: &str) -> Result<(), String> {
        let wanted = format!("mandate::{id}");
        let case = mandate::cases(&Arc::new(fixture))
            .into_iter()
            .find(|c| c.id == wanted)
            .ok_or_else(|| format!("no case {wanted}"))?;
        (case.run)()
    }

    /// The fixture with one case's member at `pointer` rewritten by `doctor`.
    fn doctored(
        fixture: &Json,
        id: &str,
        pointer: &str,
        doctor: impl FnOnce(&mut Json),
    ) -> Result<Json, String> {
        let mut copy = fixture.clone();
        let case = copy
            .get_mut("cases")
            .and_then(Json::as_array_mut)
            .and_then(|cases| cases.iter_mut().find(|c| c["id"] == id))
            .ok_or_else(|| format!("no case {id}"))?;
        doctor(
            case.pointer_mut(pointer)
                .ok_or_else(|| format!("{id} has no {pointer}"))?,
        );
        Ok(copy)
    }

    fn insert(key: &str, value: Json) -> impl FnOnce(&mut Json) {
        let key = key.to_owned();
        move |object| {
            object.as_object_mut().map(|m| m.insert(key, value));
        }
    }

    fn remove(key: &str) -> impl FnOnce(&mut Json) {
        let key = key.to_owned();
        move |object| {
            object.as_object_mut().map(|m| m.remove(&key));
        }
    }

    /// One doctoring among several of different kinds.
    type Doctor = Box<dyn FnOnce(&mut Json)>;

    fn set(value: &mut Json, pointer: &str, new: Json) {
        if let Some(slot) = value.pointer_mut(pointer) {
            *slot = new;
        }
    }

    /// The doctored case must fail, and its failure must name `named`.
    fn fails_naming(result: Result<(), String>, named: &str, what: &str) -> Result<(), String> {
        match result {
            Ok(()) => Err(format!("{what}: the case still passed")),
            Err(e) if e.contains(named) => Ok(()),
            Err(e) => Err(format!("{what}: the failure does not name `{named}`: {e}")),
        }
    }

    /// A value of the same JSON type that no family-B expectation holds: a decimal with one more
    /// digit, so the comparison and not the parse refuses it, a word with a letter more, a flag
    /// flipped, a count plus one, and a list without its last member.
    fn changed(value: &Json) -> Json {
        match value {
            Json::Number(n) => json!(n.as_u64().unwrap_or_default().saturating_add(1)),
            Json::String(s)
                if !s.is_empty()
                    && s.chars()
                        .all(|c| c.is_ascii_digit() || c == '.' || c == '-') =>
            {
                Json::String(format!("{s}1"))
            }
            Json::String(s) => Json::String(format!("{s}x")),
            Json::Bool(b) => Json::Bool(!b),
            Json::Array(items) if !items.is_empty() => Json::Array(
                items
                    .split_last()
                    .map(|(_, rest)| rest.to_vec())
                    .unwrap_or_default(),
            ),
            _ => Json::String(PLANTED.to_owned()),
        }
    }

    /// §8.3's hold reasons, §6.1's purposes, the order shapes, the verdicts, and the decisions a
    /// case can state, spelt here so a sibling is chosen without the harness's parsers.
    const HOLD_REASONS: [&str; 9] = [
        "no_fresh_outputs",
        "no_position",
        "discretionary_exits_disabled",
        "between_thresholds",
        "at_or_above_target",
        "within_rebalance_band",
        "below_band_after_clipping",
        "would_exceed_max_avg_price",
        "below_minimum_after_clipping",
    ];
    const PURPOSES: [&str; 6] = [
        "open",
        "increase",
        "discretionary_exit",
        "owner_exit",
        "risk_exit",
        "protective",
    ];
    const ORDER_TYPES: [&str; 2] = ["limit", "marketable_limit"];
    const VERDICTS: [&str; 4] = ["allow", "deny", "defer", "hold"];
    const DECISIONS: [&str; 5] = ["auto", "ask", "deny", "skipped", "deferred"];
    /// Two registered reason codes, so a stated code has a sibling that is also one.
    const GATE_REASONS: [&str; 2] = [
        "max_orders_per_day",
        "discretionary_exit_regular_session_only",
    ];

    /// The member after `value` in a closed vocabulary, wrapping round.
    fn next_in(vocabulary: &[&str], value: &Json) -> Option<Json> {
        let position = vocabulary.iter().position(|v| value == *v)?;
        let next = vocabulary
            .get(position.saturating_add(1))
            .or(vocabulary.first())?;
        Some(json!(next))
    }

    /// A different *valid* value of the same closed vocabulary as the enum-valued member `member`
    /// of the object at `at`, or `None` for a member that has no such vocabulary. [`changed`]
    /// makes a word no parser accepts, so an enum-valued member whose comparison were deleted
    /// would still fail, at its parse; a sibling parses, so only the comparison can refuse it.
    /// `on_timeout` has one variant, `skip`, and so no sibling.
    fn sibling(at: &str, member: &str, value: &Json) -> Option<Json> {
        match (at, member) {
            ("/expect", "reason") => next_in(&HOLD_REASONS, value),
            ("/expect", "purpose") => next_in(&PURPOSES, value),
            ("/expect", "order_type") => next_in(&ORDER_TYPES, value),
            ("/expect", "clipped_by") => {
                let stated: Vec<&str> = value.as_array()?.iter().filter_map(Json::as_str).collect();
                let other = [vec!["limits"], vec!["goal"]]
                    .into_iter()
                    .find(|clips| *clips != stated)?;
                Some(json!(other))
            }
            ("/expect/gate_dry_run", "verdict") => next_in(&VERDICTS, value),
            ("/expect/gate_dry_run", "reason") => match value {
                Json::Null => GATE_REASONS.first().map(|code| json!(code)),
                _ => next_in(&GATE_REASONS, value),
            },
            ("/expect/autonomy", "decision") => next_in(&DECISIONS, value),
            ("/expect/autonomy", "by") => Some(if value == "default" {
                json!("builtin_risk_reducing")
            } else {
                json!("default")
            }),
            _ => None,
        }
    }

    /// Every enum-valued expectation of case `id` in `fixture`, swapped for its sibling, fails the
    /// case at that member's comparison, whose message starts `<member>: expected`. Returns how
    /// many were swapped.
    fn every_sibling_fails_its_comparison(fixture: &Json, id: &str) -> Result<usize, String> {
        let case = family_b(fixture)?
            .into_iter()
            .find(|c| c["id"] == id)
            .ok_or_else(|| format!("no case {id}"))?;
        let expect = crate::at(&case, "expect")?;
        let mut objects = vec![("/expect".to_owned(), expect.clone())];
        for nested in ["gate_dry_run", "autonomy"] {
            if let Some(value) = expect.get(nested) {
                objects.push((format!("/expect/{nested}"), value.clone()));
            }
        }
        let mut swapped = 0_usize;
        for (at, object) in objects {
            for (member, value) in members(&object)? {
                let Some(other) = sibling(&at, &member, &value) else {
                    continue;
                };
                crate::ensure(other != value, || {
                    format!("{id}: {at}/{member}'s sibling is its own value")
                })?;
                let edited = doctored(fixture, id, &format!("{at}/{member}"), |v| *v = other)?;
                fails_naming(
                    run(edited, id),
                    &format!("{member}: expected"),
                    &format!("{id}: {at}/{member} swapped for a sibling variant"),
                )?;
                swapped = swapped.saturating_add(1);
            }
        }
        Ok(swapped)
    }

    fn members(value: &Json) -> Result<Vec<(String, Json)>, String> {
        Ok(value
            .as_object()
            .ok_or("an object")?
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect())
    }

    #[test]
    fn every_builder_case_passes_or_fails_at_its_owner() -> Result<(), String> {
        let fixture = fixture()?;
        let mut seen = 0_usize;
        for case in family_b(&fixture)? {
            let id = id(&case)?;
            let result = run(fixture.clone(), &id);
            if PASSING.contains(&id.as_str()) {
                result.map_err(|e| format!("{id}: {e}"))?;
            } else if let Some((_, needles)) = CLOCKED.iter().find(|(clocked, _)| *clocked == id) {
                if labels_agree_with_calendar(&fixture, &id)? {
                    result.map_err(|e| format!("{id}, its labels agreeing with its clock: {e}"))?;
                } else {
                    for needle in *needles {
                        fails_naming(
                            result.clone(),
                            needle,
                            &format!("{id}, its labels contradicting its clock"),
                        )?;
                    }
                }
            } else {
                let (_, needles) = OWED
                    .iter()
                    .find(|(owed, _)| *owed == id)
                    .ok_or_else(|| format!("{id} is neither passing, clocked nor owed"))?;
                for needle in *needles {
                    fails_naming(result.clone(), needle, &id)?;
                }
            }
            seen = seen.saturating_add(1);
        }
        crate::expect_eq(
            "cases listed",
            PASSING.len() + CLOCKED.len() + OWED.len(),
            seen,
        )
    }

    /// Every expected member, and every member of a dry run and an outcome, is compared and none is
    /// optional: each one edited, and each one dropped, fails the case naming it, a member planted
    /// beside them fails naming the plant, and each enum-valued one swapped for a sibling variant
    /// fails at its own comparison, so the gate's verdict and reason, a hold's reason, an order's
    /// purpose and its clips are each compared rather than only parsed.
    #[test]
    fn every_expected_member_is_compared_and_required() -> Result<(), String> {
        let fixture = fixture()?;
        let mut doctorings = 0_usize;
        let mut siblings = 0_usize;
        for case in passing(&fixture)? {
            let id = id(&case)?;
            siblings = siblings.saturating_add(every_sibling_fails_its_comparison(&fixture, &id)?);
            let expect = crate::at(&case, "expect")?;
            let mut objects = vec![("/expect".to_owned(), expect.clone())];
            for nested in ["gate_dry_run", "autonomy"] {
                if let Some(value) = expect.get(nested) {
                    objects.push((format!("/expect/{nested}"), value.clone()));
                }
            }
            for (at, object) in objects {
                for (member, value) in members(&object)? {
                    let edited = doctored(&fixture, &id, &format!("{at}/{member}"), |v| {
                        *v = changed(&value);
                    })?;
                    fails_naming(
                        run(edited, &id),
                        &member,
                        &format!("{id}: {at}/{member} edited"),
                    )?;
                    let dropped = doctored(&fixture, &id, &at, remove(&member))?;
                    fails_naming(
                        run(dropped, &id),
                        &member,
                        &format!("{id}: {at}/{member} dropped"),
                    )?;
                    doctorings = doctorings.saturating_add(2);
                }
                let planted = doctored(&fixture, &id, &at, insert(PLANTED, Json::Null))?;
                fails_naming(
                    run(planted, &id),
                    PLANTED,
                    &format!("{id}: a plant in {at}"),
                )?;
            }
        }
        crate::expect_eq("doctorings, counted from the fixture", doctorings, 698)?;
        crate::expect_eq(
            "siblings: ten hold reasons, one hold's and eleven buys' clips, and each of thirteen \
             orders' purpose, verdict, reason, decision and decider",
            siblings,
            10 + 1 + 11 + 13 * 5,
        )
    }

    /// A hold reaches neither the gate nor approval, so a hold that states either fails naming it,
    /// and so does a hold that states an order field, and a buy or a sell a member it has none of.
    #[test]
    fn an_expectation_the_action_cannot_have_is_refused() -> Result<(), String> {
        let fixture = fixture()?;
        let mut holds = 0_usize;
        for case in passing(&fixture)? {
            let id = id(&case)?;
            let action = crate::str_at(&case, "expect.action")?;
            let refused: &[(&str, Json)] = match action {
                "hold" => &[
                    ("gate_dry_run", json!({"verdict": "allow", "reason": null})),
                    ("autonomy", json!({"decision": "auto", "by": "default"})),
                    ("purpose", json!("open")),
                    ("qty", json!("1")),
                    ("limit_price", json!("1")),
                    ("order_usd", json!("1")),
                    ("order_type", json!("limit")),
                ],
                "buy" => &[
                    ("reason", json!("no_position")),
                    ("order_type", json!("limit")),
                ],
                _ => &[("reason", json!("no_position")), ("clipped_by", json!([]))],
            };
            if action == "hold" {
                holds = holds.saturating_add(1);
            }
            for (member, value) in refused {
                if case.pointer(&format!("/expect/{member}")).is_some() {
                    continue;
                }
                let stated = doctored(&fixture, &id, "/expect", insert(member, value.clone()))?;
                fails_naming(run(stated, &id), member, &format!("{id}: {member} stated"))?;
            }
            for (member, value) in [("origin", json!("builder")), ("trim_withheld", json!([]))] {
                let stated = doctored(&fixture, &id, "/expect", insert(member, value))?;
                fails_naming(run(stated, &id), member, &format!("{id}: {member} stated"))?;
            }
        }
        crate::expect_eq("holds", holds, 10)
    }

    /// Every member of a case is read: a plant at the top level, in `input`, its `quote`, its
    /// `gate_state`, each output and each working order fails naming the plant, and so does a
    /// trim guard on a mandate that does not trim.
    #[test]
    fn every_input_member_is_read_and_no_other_is_accepted() -> Result<(), String> {
        let fixture = fixture()?;
        let mut plants = 0_usize;
        for case in passing(&fixture)? {
            let id = id(&case)?;
            let input = crate::at(&case, "input")?;
            let mut at = vec![
                String::new(),
                "/input".to_owned(),
                "/input/quote".to_owned(),
                "/input/gate_state".to_owned(),
            ];
            for (index, _) in crate::list_at(input, "outputs")?.iter().enumerate() {
                at.push(format!("/input/outputs/{index}"));
            }
            for (index, _) in crate::list_at(input, "gate_state.working_opening_orders")?
                .iter()
                .enumerate()
            {
                at.push(format!("/input/gate_state/working_opening_orders/{index}"));
            }
            if let Some(listed) = input.get("working_opening_orders").and_then(Json::as_array) {
                for (index, _) in listed.iter().enumerate() {
                    at.push(format!("/input/working_opening_orders/{index}"));
                }
            }
            for pointer in at {
                let planted = doctored(&fixture, &id, &pointer, insert(PLANTED, json!("1")))?;
                fails_naming(
                    run(planted, &id),
                    PLANTED,
                    &format!("{id}: a plant at {pointer}"),
                )?;
                plants = plants.saturating_add(1);
            }
            for (guard, value) in [("scale_active_s", json!(0)), ("holding", json!(false))] {
                let stated = doctored(&fixture, &id, "/input", insert(guard, value))?;
                fails_naming(run(stated, &id), guard, &format!("{id}: {guard} stated"))?;
            }
        }
        crate::expect_eq(
            "plants: four objects per case, forty-four outputs and two working orders",
            plants,
            23 * 4 + 44 + 2,
        )
    }

    /// Every scalar the harness reads is parsed, never defaulted: each one replaced by a value of
    /// the wrong type fails the case naming it.
    #[test]
    fn an_unreadable_input_is_refused_naming_it() -> Result<(), String> {
        let fixture = fixture()?;
        let mut refused = 0_usize;
        for case in passing(&fixture)? {
            let id = id(&case)?;
            let input = crate::at(&case, "input")?;
            let mut scalars: Vec<(String, String)> = members(input)?
                .into_iter()
                .filter(|(_, v)| !v.is_object() && !v.is_array())
                .map(|(k, _)| (format!("/input/{k}"), k))
                .collect();
            for key in ["bid", "ask"] {
                scalars.push((format!("/input/quote/{key}"), key.to_owned()));
            }
            for key in ["agent_equity", "orders_today"] {
                scalars.push((format!("/input/gate_state/{key}"), key.to_owned()));
            }
            for (index, output) in crate::list_at(input, "outputs")?.iter().enumerate() {
                for (key, _) in members(output)? {
                    scalars.push((
                        format!("/input/outputs/{index}/{key}"),
                        format!("outputs[{index}].{key}"),
                    ));
                }
            }
            for (pointer, named) in scalars {
                let unreadable = doctored(&fixture, &id, &pointer, |v| *v = json!(PLANTED))?;
                fails_naming(run(unreadable, &id), &named, &format!("{id}: {pointer}"))?;
                let mistyped = doctored(&fixture, &id, &pointer, |v| {
                    *v = if v.is_string() { json!(7) } else { json!([]) };
                })?;
                fails_naming(
                    run(mistyped, &id),
                    &named,
                    &format!("{id}: {pointer} mistyped"),
                )?;
                refused = refused.saturating_add(1);
            }
        }
        crate::expect_eq("scalars refused, counted from the fixture", refused, 611)
    }

    /// The builder and the gate see one account: the gate state's equity, the instrument's market
    /// value, the working orders and the gross exposure must be the case's own figures, and the
    /// session labels must be the calendar's at `now`.
    #[test]
    fn the_builder_and_the_gate_see_one_scene() -> Result<(), String> {
        let fixture = fixture()?;
        let xyz = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
        let checks: Vec<(&str, &str, Doctor, &str)> = vec![
            (
                "MC-B01",
                "/input/gate_state",
                Box::new(insert("agent_equity", json!("9999"))),
                "gate_state.agent_equity",
            ),
            (
                "MC-B04",
                "/input/gate_state/positions_mv",
                Box::new(remove(xyz)),
                "gate_state.positions_mv",
            ),
            (
                "MC-B04",
                "/input/gate_state/positions_mv",
                Box::new(insert(xyz, json!("499.6"))),
                "gate_state.positions_mv",
            ),
            (
                "MC-B01",
                "/input/gate_state/positions_mv",
                Box::new(insert(xyz, json!("1"))),
                "gate_state.positions_mv",
            ),
            (
                "MC-B19",
                "/input",
                Box::new(insert("gross_usd", json!("1999"))),
                "gross_usd",
            ),
            (
                "MC-B15",
                "/input",
                Box::new(insert("working_opening_orders", json!([]))),
                "working_opening_orders",
            ),
            (
                "MC-B01",
                "/input",
                Box::new(insert("session", json!("after_hours"))),
                "`session`",
            ),
            (
                "MC-B01",
                "/input",
                Box::new(insert("in_close_window", json!(true))),
                "`in_close_window`",
            ),
            (
                "MC-B01",
                "/input",
                Box::new(insert("asset_class", json!("crypto"))),
                "asset_class",
            ),
            (
                "MC-B01",
                "/input",
                Box::new(insert("fee_rate_cash", json!("0.001"))),
                "equity fee rate",
            ),
            (
                "MC-B01",
                "/input",
                Box::new(insert(
                    "instrument",
                    json!("7b4a1c2e-9999-4a2b-9c3d-000000000009"),
                )),
                "not a pinned instrument",
            ),
            (
                "MC-B29",
                "/input",
                Box::new(insert("asset_class", json!("us_equity"))),
                "asset_class",
            ),
        ];
        for (id, pointer, doctor, named) in checks {
            let edited = doctored(&fixture, id, pointer, doctor)?;
            fails_naming(
                run(edited, id),
                named,
                &format!("{id}: {pointer} for {named}"),
            )?;
        }
        Ok(())
    }

    /// The defaults that decide a case are the case's to state: without its size factor, its P&L
    /// fraction, or its working order, the case fails. `gross_usd` and the input's
    /// `working_opening_orders` only restate the gate state, so dropping them changes nothing.
    #[test]
    fn a_stated_input_that_decides_the_case_cannot_be_dropped() -> Result<(), String> {
        let fixture = fixture()?;
        let no_working_order = |input: &mut Json| {
            set(input, "/gate_state/working_opening_orders", json!([]));
            input
                .as_object_mut()
                .map(|m| m.remove("working_opening_orders"));
        };
        let checks: Vec<(&str, &str, Doctor, &str)> = vec![
            (
                "MC-B02",
                "size_factor",
                Box::new(remove("size_factor")),
                "target_value",
            ),
            (
                "MC-B16",
                "size_factor",
                Box::new(remove("size_factor")),
                "target_value",
            ),
            (
                "MC-B24",
                "position_pnl_fraction",
                Box::new(remove("position_pnl_fraction")),
                "`autonomy`: decision",
            ),
            (
                "MC-B15",
                "its working order",
                Box::new(no_working_order),
                "delta",
            ),
        ];
        for (id, what, doctor, named) in checks {
            let dropped = doctored(&fixture, id, "/input", doctor)?;
            fails_naming(run(dropped, id), named, &format!("{id}: {what} dropped"))?;
        }
        for (id, member) in [
            ("MC-B19", "gross_usd"),
            ("MC-B15", "working_opening_orders"),
        ] {
            let dropped = doctored(&fixture, id, "/input", remove(member))?;
            run(dropped, id).map_err(|e| format!("{id} without its restated {member}: {e}"))?;
        }
        Ok(())
    }

    /// MC-B22 and MC-B23 fail only because their labels contradict their clock: moved to an
    /// instant where the calendar agrees with the label, each passes as stated, and each
    /// enum-valued expectation swapped for a sibling fails at its comparison. MC-B23 is the only
    /// case that states a sell's `order_type` and MC-B22 the only deferred one, so this is where
    /// those two comparisons are proved.
    #[test]
    fn the_two_session_cases_pass_once_now_agrees_with_their_labels() -> Result<(), String> {
        let fixture = fixture()?;
        let mut siblings = 0_usize;
        for (id, now, as_of, expires_at) in [
            (
                "MC-B22",
                "2026-09-22T21:00:00.000000000Z",
                "2026-09-22T20:59:00.000000000Z",
                "2026-09-22T22:00:00.000000000Z",
            ),
            (
                "MC-B23",
                "2026-09-22T19:55:00.000000000Z",
                "2026-09-22T19:54:00.000000000Z",
                "2026-09-22T21:00:00.000000000Z",
            ),
        ] {
            let moved = doctored(&fixture, id, "/input", |input| {
                set(input, "/now", json!(now));
                let outputs = input
                    .pointer_mut("/outputs")
                    .and_then(Json::as_array_mut)
                    .into_iter()
                    .flatten();
                for output in outputs {
                    set(output, "/as_of", json!(as_of));
                    set(output, "/expires_at", json!(expires_at));
                }
            })?;
            run(moved.clone(), id).map_err(|e| format!("{id} at {now}: {e}"))?;
            siblings = siblings.saturating_add(every_sibling_fails_its_comparison(&moved, id)?);
        }
        crate::expect_eq(
            "siblings: each case's purpose, verdict, reason, decision and decider, and MC-B23's \
             order type",
            siblings,
            11,
        )
    }
}
