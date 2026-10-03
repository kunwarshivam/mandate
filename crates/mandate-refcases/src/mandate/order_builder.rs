//! Family B of the `mandate` suite: the thirty-five `builder` cases (spec §6.2, §8.1 to §8.3), run
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
//! A `trim_to_target` base's trim inputs make one scene for both of its gate calls, the trim and
//! the builder's dry run: the instrument's minimum is the case's `min_order_size`, which such a
//! case must state, as `ref.py` requires (DEC-399 item 8), the confirmed `scale_sizes` rung has
//! been active for `scale_active_s`, and the goal is Holding when `holding` says so.
//!
//! **What the harness fills.** The fixture header's four defaults (`session` regular,
//! `in_close_window` false, fee rates 0, `has_prior_fill` = position above zero) and `ref.py`'s
//! (`size_factor` 1, and 0 for the position's P&L fraction, the cost basis and the goal spend). The
//! drawdown, the day's P&L fraction and the day's buys are 0 and not accepted: no base has an
//! autonomy rule that reads them. Every base pins its universe, so no order can be an admission:
//! `new_instrument` is false and `thesis_confidence` 0 (§6.3, "0 when no thesis applies"), and a
//! base that does not pin is refused rather than guessed about. The gate is handed no fee
//! reservation, so a cash fee rate above zero is refused, and so is an equity asset fee rate.
//!
//! **Every key is read (DEC-85).** The case, its `input`, `quote`, `gate_state`, every output and
//! working order, `expect`, `gate_dry_run` and `autonomy` are each swept against the members read
//! here. An expectation the builder has an answer for must be stated: a computed target or Delta,
//! a clip, an order field. One it has no answer for must be absent: `reason` on an order, an order
//! field on a hold, `clipped_by` on a sell. `clipped_by` absent on a hold means no clip, and
//! `order_type` absent on a sell means a plain limit.
//!
//! **A `trim_to_target` base asks the risk engine first** (§6.2 step 1): a trim
//! `mandate_risk::trim_proposals` proposes is the case's action, and with none the case's
//! `trim_withheld` must name the guards that hold before the builder proposes (DEC-400). A crypto
//! pair's quote currency is read from the pinned instrument's `symbol` (`BTC/USD`), and a symbol
//! that does not name USD as its quote is never read as USD (DEC-254 items 1 and 7, DEC-285).

use std::collections::{BTreeMap, BTreeSet};

use mandate_builder::{
    AccountSnapshot, AccumulateGoal, Action, ActionContext, BuilderError, BuilderMandate, Clip,
    Direction, GateVerdict, GoalKind, HoldReason, Limits, Market, ModelOutput, ModelVersion,
    OrderShape, Outcome, Proposal, RequestedBy, RiskContext, SignalModel, Sizing, classify, decide,
    propose,
};
use mandate_domain::{AssetClass, AssetId, MarketSession, Purpose};
use mandate_num::{
    Conviction, CostBasis, FeeRate, Fraction, MarkPrice, NumError, Price, Qty, Ratio, Rounding,
    Signed, SizeFraction, Unit, Usd, UsdExact,
};
use mandate_risk as gate;
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};
use mandate_spec::document::{self as spec_doc, Goal, LadderAction, Mandate, ModelId};
use mandate_time::{UtcNanos, new_york_date_and_hour};

use super::autonomy;
use super::{at_of, digest, instant, not_implemented, num, unknown_members, validated_mandate};
use crate::{Json, at, ensure, expect_eq, list_at, str_at, u64_at};

const CASE_KEYS: &[&str] = &["id", "kind", "title", "base", "input", "expect"];
/// The members `ref.py`'s builder reads, and nothing else, less three it reads only as autonomy
/// facts (`drawdown`, `daily_pnl_fraction` and `bought_today_usd`): no family-B base has a rule on
/// any of them, so no case could show one read, and each is refused until a case states it
/// (DEC-250 item 16).
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
    "position_pnl_fraction",
    "scale_active_s",
    "holding",
    "min_order_size",
    "open_sell_qty",
];
/// §5.5's trim inputs, which only a `trim_to_target` mandate reads.
const TRIM_INPUT_KEYS: [&str; 4] = [
    "scale_active_s",
    "holding",
    "min_order_size",
    "open_sell_qty",
];
const TRIM_EXPECT_KEYS: [&str; 2] = ["origin", "trim_withheld"];
/// Every member a trim's expectation may state (MC-B17).
const TRIM_KEYS: [&str; 10] = [
    "cap",
    "current_mv",
    "action",
    "purpose",
    "origin",
    "reason",
    "qty",
    "limit_price",
    "order_usd",
    "autonomy",
];
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
        return trim_first(fixture, document, &stated, &config, expect);
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

/// §6.2 step 1: under `trim_to_target` the risk engine proposes first (DEC-250 item 1). A trim
/// `mandate_risk::trim_proposals` answers is the case's action, compared with every member the case
/// states; with none, the case's `trim_withheld` must name exactly the §5.5 guards that hold, and
/// the builder then proposes as on any other mandate (DEC-400).
fn trim_first(
    fixture: &Json,
    document: &Mandate,
    stated: &Inputs,
    config: &gate::GateConfig,
    expect: &Json,
) -> Result<(), String> {
    let trim = stated
        .trim
        .as_ref()
        .ok_or("a `trim_to_target` base read no trim inputs")?;
    let scene = Scene::read(fixture, document, stated, config)?;
    let instruments = BTreeMap::from([(
        scene.instrument.instrument.clone(),
        scene.instrument.clone(),
    )]);
    let trims = gate::trim_proposals(
        stated.now,
        config,
        &scene.mandate,
        &scene.risk,
        &scene.agent,
        &scene.account,
        &instruments,
    )
    .map_err(|e| gate_error("trim_proposals", &e))?;
    let clock = gate::session_at(stated.now, config, stated.gate_class())
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
    expect_eq("cap", proposal.sizes.cap, exact(expect, "cap")?)?;
    expect_eq(
        "current_mv",
        proposal.sizes.current_mv,
        exact(expect, "current_mv")?,
    )?;
    let guards = trim_guards(document, stated, &clock, expect, trim)?;
    let unguarded = guards.as_ref().is_some_and(Vec::is_empty);
    let judged = match (trims.as_slice(), unguarded) {
        ([trim], true) => compare_trim(document, stated, &clock, &proposal, trim, expect),
        ([], false) => {
            let stated_guards = match expect.get("trim_withheld") {
                None => None,
                Some(_) => Some(
                    list_at(expect, "trim_withheld")?
                        .iter()
                        .map(|g| g.as_str().ok_or("`trim_withheld` lists a non-string"))
                        .collect::<Result<Vec<_>, _>>()?,
                ),
            };
            expect_eq("trim_withheld", stated_guards, guards)?;
            ensure(expect.get("origin").is_none(), || {
                "`origin`: the builder's proposal states none".to_owned()
            })?;
            judge(fixture, document, stated, config, &proposal, expect)
        }
        (trims, _) => Err(format!(
            "`trim_proposals` answered {trims:?} where §5.5's guards give {guards:?}"
        )),
    };
    stated.agrees_with_clock(&clock, judged)
}

/// `reference/mandate/ref.py`'s guards, in its order, on the case's own figures, with
/// `below_minimum_order` exempting a trim of the whole position as `ref.py` does (DEC-423):
/// `None` when no trim is due (no factor below one, or an excess under the band), else every guard
/// that withholds it, empty when none does. Read from the case, not from `trim_proposals`, which names no guard,
/// so the gate's answer is judged against them rather than explained by them (DEC-400 item 2).
fn trim_guards(
    document: &Mandate,
    stated: &Inputs,
    clock: &gate::SessionAt,
    expect: &Json,
    trim: &TrimInputs,
) -> Result<Option<Vec<&'static str>>, String> {
    let factor = UsdExact::of_ratio(stated.gate_size_factor);
    let one = num(UsdExact::parse("1"), "one")?;
    let cap = exact(expect, "cap")?;
    let current_mv = exact(expect, "current_mv")?;
    let band = num(
        cap.checked_mul(UsdExact::of(num(
            Usd::parse(document.behavior.sizing.rebalance_band.as_str()),
            "rebalance_band",
        )?)),
        "the band",
    )?;
    let excess = num(
        cap.checked_mul(factor)
            .and_then(|target| current_mv.checked_sub(target)),
        "the excess",
    )?;
    if !num(factor.is_below(one), "size_factor")? || num(excess.is_below(band), "excess")? {
        return Ok(None);
    }
    let whole_excess = num(
        excess.ceiled_quotient(UsdExact::of_price(stated.bid), stated.increment),
        "the trim's quantity",
    )?;
    let on_sale = trim.open_sell_qty;
    let sell = less_or_zero(whole_excess, on_sale)?.min(less_or_zero(stated.position, on_sale)?);
    if sell.is_zero() {
        return Ok(None);
    }
    let mut guards = Vec::new();
    if trim.active_s < u64::from(document.risk.breach_confirm_s) {
        guards.push("rung_not_confirmed");
    }
    if trim.holding {
        guards.push("holding");
    }
    if stated.asset_class == AssetClass::UsEquity && clock.session != gate::Session::Regular {
        guards.push("regular_session_only");
    }
    let closes_the_position = sell == stated.position;
    if sell < trim.min_order_size && !closes_the_position {
        guards.push("below_minimum_order");
    }
    Ok(Some(guards))
}

/// `from` less `taken`, or none when `taken` covers it: what resting sells leave of the excess and
/// of the position (DEC-399 item 7).
fn less_or_zero(from: Qty, taken: Qty) -> Result<Qty, String> {
    match from.checked_sub(taken) {
        Err(NumError::Negative) => Ok(Qty::ZERO),
        left => num(left, "a quantity less the resting sells"),
    }
}

/// MC-B17's members for a trim, its cap and market value already compared: the risk exit itself,
/// a sell at the bid by the risk engine, AUTO as every risk-reducing purpose is (§6.2 step 3). The
/// trim reaches no dry run and no builder figure, so the case states none (DEC-400 item 3). The
/// `limit_price` the case states is checked against the case's own bid: `TrimProposal` carries no
/// price, so it pins the case file, not the gate. §6.2 step 3 reads only the purpose; the other
/// facts are the case's, and a trim sells a held position, so it is never a first trade, which is
/// also what `!has_prior_fill` gives on every trim case.
fn compare_trim(
    document: &Mandate,
    stated: &Inputs,
    clock: &gate::SessionAt,
    proposal: &Proposal,
    trim: &gate::TrimProposal,
    expect: &Json,
) -> Result<(), String> {
    unknown_members(expect, &TRIM_KEYS)
        .map_err(|unknown| format!("a trim states none of: {unknown}"))?;
    expect_eq(
        "instrument",
        trim.instrument.clone(),
        stated.instrument_id()?,
    )?;
    expect_eq("action", "sell", str_at(expect, "action")?)?;
    expect_eq("origin", "risk_engine", str_at(expect, "origin")?)?;
    expect_eq("reason", "trim_to_target", str_at(expect, "reason")?)?;
    expect_eq(
        "the purpose the gate assigns",
        trim.purpose,
        gate::Purpose::RiskExit,
    )?;
    let order_usd = num(trim.qty.notional(stated.bid), "order_usd")?;
    order_fields(expect, (Purpose::RiskExit, trim.qty, stated.bid, order_usd))?;
    let classified = classify(
        &document.autonomy,
        &ActionContext {
            purpose: Purpose::RiskExit,
            order_usd,
            combined_score: proposal.combined.score,
            instrument: stated.instrument.clone(),
            asset_class: stated.asset_class,
            session: market_session(clock.session),
            first_trade_in_instrument: false,
            new_instrument: stated.risk.new_instrument,
            thesis_confidence: stated.risk.thesis_confidence,
            drawdown: stated.risk.drawdown,
            daily_pnl_fraction: stated.risk.daily_pnl_fraction,
            position_usd_after: num(
                proposal
                    .sizes
                    .current_mv
                    .checked_sub(UsdExact::of(order_usd))
                    .and_then(|after| after.round(12, Rounding::HalfEven)),
                "the position after the trim",
            )?,
            gross_usd_after: stated.account.gross_usd,
            bought_today_usd: stated.risk.bought_today_usd,
            position_pnl_fraction: stated.risk.position_pnl_fraction,
            requested_by: RequestedBy::Agent,
            risk_day: stated.risk.risk_day,
        },
    )
    .map_err(|e| builder_error("classify", &e))?;
    autonomy::compare(at_of(expect, "autonomy")?, &classified)
        .map_err(|e| format!("`autonomy`: {e}"))
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
    let scene = Scene::read(fixture, document, stated, config)?;
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
    /// The fee reservation is zero because [`Inputs::read`] refuses a cash fee rate above zero.
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
    /// The pinned instrument's quote currency, from its `symbol` (DEC-285).
    quote_currency: Option<gate::QuoteCurrency>,
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
    /// §5.5's trim inputs, read on a `trim_to_target` base only. Both gate calls on such a base,
    /// the trim and the builder's dry run, see them through one [`Scene`] (#498 review, m3).
    trim: Option<TrimInputs>,
}

/// What a `trim_to_target` case states for §5.5's guards: how long the rung has been active, whether
/// the goal is Holding, the instrument's minimum order size, which `ref.py` requires, and the
/// quantity of the agent's own non-protective sells already resting in the instrument, which the
/// trim is sized after (DEC-399 item 7). Until the reference PR states `open_sell_qty` on every trim
/// case, an unstated one is none resting.
struct TrimInputs {
    active_s: u64,
    holding: bool,
    min_order_size: Qty,
    open_sell_qty: Qty,
}

impl TrimInputs {
    fn read(input: &Json) -> Result<Self, String> {
        let active_s = match input.get("scale_active_s") {
            None => 0,
            Some(value) => value
                .as_u64()
                .ok_or("`scale_active_s` is not a whole number of seconds")?,
        };
        Ok(Self {
            active_s,
            holding: flag_or(input, "holding", false)?,
            min_order_size: num(
                Qty::parse(str_at(input, "min_order_size")?),
                "min_order_size",
            )?,
            open_sell_qty: match input.get("open_sell_qty") {
                None => Qty::ZERO,
                Some(_) => num(Qty::parse(str_at(input, "open_sell_qty")?), "open_sell_qty")?,
            },
        })
    }
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
        ensure(fee_rate_cash == no_fee, || {
            "`fee_rate_cash` above zero: the gate is handed no fee reservation, which holds only \
             for a fee paid in the asset (trading-domain §7.2), so a cash fee would reach the \
             buying-power check unreserved (§9.5, DEC-250 item 17)"
                .to_owned()
        })?;
        ensure(
            asset_class == AssetClass::Crypto || fee_rate_asset == no_fee,
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
            quote_currency: crate::quote_currency_of(&pinned.symbol),
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
                drawdown: Unit::ZERO,
                daily_pnl_fraction: Signed::ZERO,
                position_pnl_fraction: num(
                    Signed::parse(text_or(input, "position_pnl_fraction", "0")?),
                    "position_pnl_fraction",
                )?,
                bought_today_usd: Usd::ZERO,
                has_prior_fill: flag_or(input, "has_prior_fill", !position.is_zero())?,
                new_instrument: false,
                thesis_confidence: Unit::ZERO,
                risk_day: mandate_spec::risk::risk_day(now)
                    .map_err(|e| format!("`now` has no risk day: {}", e.code()))?
                    .day,
            },
            gate_size_factor: num(Ratio::parse(size_factor), "size_factor")?,
            outputs,
            state,
            stated_session: MarketSession::parse_condition_form(text_or(
                input, "session", "regular",
            )?)
            .map_err(|e| format!("`session`: {}", e.code()))?,
            stated_close_window: flag_or(input, "in_close_window", false)?,
            trim: match document.risk.scale_action {
                spec_doc::ScaleAction::TrimToTarget => Some(TrimInputs::read(input)?),
                spec_doc::ScaleAction::LimitBuys => None,
            },
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
        let listed = state
            .get("working_universe")
            .and_then(Json::as_array)
            .ok_or("`gate_state.working_universe` is not a list")?;
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
        .enumerate()
        .map(|(index, order)| {
            unknown_members(order, WORKING_ORDER_KEYS)
                .map_err(|unknown| format!("`{what}` order members not interpreted: {unknown}"))?;
            let member = |key: &str| format!("{what}[{index}].{key}");
            let text = |key: &str| {
                order
                    .get(key)
                    .and_then(Json::as_str)
                    .ok_or_else(|| format!("`{}` is not a string", member(key)))
            };
            Ok((
                gate_asset(&asset(text("instrument")?, &member("instrument"))?)?,
                num(Usd::parse(text("max_cost")?), &member("max_cost"))?,
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
        if let Some(trim) = stated.trim.as_ref().filter(|t| !t.open_sell_qty.is_zero()) {
            working_orders.insert(
                gate::ClientOrderId(next_id),
                gate::WorkingOrder {
                    agent: THE_AGENT,
                    instrument: instrument.clone(),
                    side: gate::Side::Sell,
                    max_cost: Usd::ZERO,
                    open_qty: trim.open_sell_qty,
                    protective: false,
                    opening: false,
                    submitted_on: today,
                },
            );
            next_id = next_id.saturating_add(1);
        }
        let agent_equity = stated.account.agent_equity;
        let mut instrument_snapshot = listing(stated, instrument, one)?;
        let mut active_rungs = BTreeMap::new();
        let mut goal_state = GoalState::Running;
        if let Some(trim) = &stated.trim {
            instrument_snapshot.min_order_size = trim.min_order_size;
            if let Some(rung) = scaling_rung(document, stated.gate_size_factor)? {
                active_rungs.insert(rung, trim.active_s);
            }
            if trim.holding {
                goal_state = GoalState::Holding;
            }
        }
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
                active_rungs,
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
            instrument: instrument_snapshot,
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
        quote_currency: if equity {
            Some(gate::QuoteCurrency::Usd)
        } else {
            stated.quote_currency
        },
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
    use std::collections::BTreeSet;
    use std::path::Path;
    use std::sync::Arc;

    use serde_json::json;

    use crate::{Json, mandate, read_fixture};

    const PLANTED: &str = "zz_planted";

    /// The cases that pass against the merged `mandate-builder` and `mandate-risk`: all thirty-five
    /// since the trim arm compares MC-B17 and MC-B30 to MC-B32 (DEC-400), MC-B33 and MC-B34 landed
    /// in #504 (DEC-399 item 8), and MC-B35, the whole-position trim below the minimum, in #530
    /// (DEC-423).
    const PASSING: [&str; 35] = [
        "MC-B01", "MC-B02", "MC-B03", "MC-B04", "MC-B05", "MC-B06", "MC-B07", "MC-B08", "MC-B09",
        "MC-B10", "MC-B11", "MC-B12", "MC-B13", "MC-B14", "MC-B15", "MC-B16", "MC-B17", "MC-B18",
        "MC-B19", "MC-B20", "MC-B21", "MC-B22", "MC-B23", "MC-B24", "MC-B25", "MC-B26", "MC-B27",
        "MC-B28", "MC-B29", "MC-B30", "MC-B31", "MC-B32", "MC-B33", "MC-B34", "MC-B35",
    ];

    /// MC-B36 and MC-B37, a trim sized after a resting sell (DEC-399 item 7), which the reference
    /// PR after this harness adds, with `open_sell_qty` stated on every trim case. Until it lands
    /// the fixture holds them or not; if present they must pass, and the counts below are stated
    /// for both fixtures. A cleanup drops the fixture without them once they land.
    const AWAITED: [&str; 2] = ["MC-B36", "MC-B37"];
    const DOCTORINGS_WITH: usize = 1092;
    /// Four objects in each of 37 cases, 69 outputs, and two working orders.
    const PLANTS_WITH: usize = 219;
    const REFUSED_WITH: usize = 1485;

    /// Whether the fixture already holds the [`AWAITED`] cases.
    fn awaited(fixture: &Json) -> Result<bool, String> {
        let present = crate::list_at(fixture, "cases")?
            .iter()
            .filter(|c| c["id"].as_str().is_some_and(|id| AWAITED.contains(&id)))
            .count();
        crate::ensure(present == 0 || present == AWAITED.len(), || {
            format!(
                "the fixture holds {present} of the {} awaited cases, not none or all",
                AWAITED.len()
            )
        })?;
        Ok(present == AWAITED.len())
    }

    /// `without` on today's fixture, `with` once the awaited cases land.
    fn counted(fixture: &Json, without: usize, with: usize) -> Result<usize, String> {
        Ok(if awaited(fixture)? { with } else { without })
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
        let expected = counted(
            fixture,
            PASSING.len(),
            PASSING.len().saturating_add(AWAITED.len()),
        )?;
        crate::ensure(cases.len() == expected, || {
            format!("family B is {expected} cases, found {}", cases.len())
        })?;
        Ok(cases)
    }

    fn listed(id: &str) -> bool {
        PASSING.contains(&id) || AWAITED.contains(&id)
    }

    fn passing(fixture: &Json) -> Result<Vec<Json>, String> {
        Ok(family_b(fixture)?
            .into_iter()
            .filter(|c| c["id"].as_str().is_some_and(listed))
            .collect())
    }

    /// The gate and the case's own guards must agree.
    ///
    /// - A trim's minimum is the instrument's minimum order size, not the dollar minimum (§5.5,
    ///   DEC-399 item 8): MC-B17 under a 1000-dollar minimum still trims its 3 shares, and at a
    ///   `min_order_size` of 3 it still does, while at 4 the trim is withheld and the case fails
    ///   naming `below_minimum_order`. On a 3-share grid MC-B17 still trims (#498 review, m1). A
    ///   trim case that states no `min_order_size` is refused naming it, never given a default.
    /// - A trim of the whole position is a full close, which the minimum exempts (DEC-423):
    ///   MC-B17 reshaped as 1 share at a 999 bid trims that share at a 2-share minimum. Under the
    ///   guard without the exemption, the gate's trim and the case's guards disagree, and the case
    ///   fails naming both.
    /// - MC-B30 confirmed for 60 seconds is due and unguarded, so the gate's trim is the case's
    ///   action, and the builder's figures the case states are refused.
    /// - Each guard is judged alone, since MC-B31 states two and either would mask the other:
    ///   MC-B30 confirmed and Holding withholds `holding` only, MC-B31 not Holding withholds
    ///   `regular_session_only` only, and MC-B30 confirmed at a minimum of 4 shares withholds
    ///   `below_minimum_order` only; each passes, and a gate that trims through the guard fails
    ///   naming both answers (#478 review, M1).
    #[test]
    fn a_trim_the_gate_and_the_guards_disagree_on_fails_naming_both() -> Result<(), String> {
        let fixture = fixture()?;
        let dollars = doctored(&fixture, "MC-B17", "/input/min_order_usd", |v| {
            *v = json!("1000");
        })?;
        run(dollars, "MC-B17").map_err(|e| format!("MC-B17 under a 1000-dollar minimum: {e}"))?;
        let at_the_trim = doctored(&fixture, "MC-B17", "", |case| {
            put(case, "input", "min_order_size", json!("3"));
        })?;
        run(at_the_trim, "MC-B17")
            .map_err(|e| format!("MC-B17 at a minimum of its own 3 shares: {e}"))?;
        let a_share_above = doctored(&fixture, "MC-B17", "", |case| {
            put(case, "input", "min_order_size", json!("4"));
        })?;
        fails_naming(
            run(a_share_above, "MC-B17"),
            "below_minimum_order",
            "MC-B17 at a minimum a share above its trim",
        )?;
        let on_the_grid = doctored(&fixture, "MC-B17", "/input/qty_increment", |v| {
            *v = json!("3");
        })?;
        run(on_the_grid, "MC-B17").map_err(|e| format!("MC-B17 on a 3-share grid: {e}"))?;
        let unstated = doctored(&fixture, "MC-B17", "/input", remove("min_order_size"))?;
        fails_naming(
            run(unstated, "MC-B17"),
            "min_order_size",
            "MC-B17 with no minimum order size",
        )?;
        let confirmed = doctored(&fixture, "MC-B30", "/input/scale_active_s", |v| {
            *v = json!(60);
        })?;
        fails_naming(
            run(confirmed, "MC-B30"),
            "a trim states none of: buy_conviction",
            "MC-B30 confirmed at 60 s",
        )?;
        let holding_alone = doctored(&fixture, "MC-B30", "", |case| {
            put(case, "input", "scale_active_s", json!(60));
            put(case, "input", "holding", json!(true));
            put(case, "expect", "trim_withheld", json!(["holding"]));
        })?;
        run(holding_alone, "MC-B30").map_err(|e| format!("MC-B30 confirmed and Holding: {e}"))?;
        let session_alone = doctored(&fixture, "MC-B31", "", |case| {
            put(case, "input", "holding", json!(false));
            put(
                case,
                "expect",
                "trim_withheld",
                json!(["regular_session_only"]),
            );
        })?;
        run(session_alone, "MC-B31").map_err(|e| format!("MC-B31 not Holding: {e}"))?;
        let minimum_alone = doctored(&fixture, "MC-B30", "", |case| {
            put(case, "input", "scale_active_s", json!(60));
            put(case, "input", "min_order_size", json!("4"));
            put(
                case,
                "expect",
                "trim_withheld",
                json!(["below_minimum_order"]),
            );
        })?;
        run(minimum_alone, "MC-B30").map_err(|e| format!("MC-B30 below the minimum size: {e}"))?;
        let whole_position = doctored(&fixture, "MC-B17", "", |case| {
            put(case, "input", "position_qty", json!("1"));
            put(case, "input", "quote", json!({"bid": "999", "ask": "1000"}));
            put(case, "input", "min_order_size", json!("2"));
            put(case, "expect", "qty", json!("1"));
            put(case, "expect", "limit_price", json!("999"));
            put(case, "expect", "order_usd", json!("999"));
        })?;
        run(whole_position, "MC-B17")
            .map_err(|e| format!("MC-B17 as 1 share below a 2-share minimum: {e}"))
    }

    /// A trim is sized after the agent's own resting sells (DEC-399 item 7), in the gate's scene and
    /// in the case's guards alike. MC-B17's excess is 3 shares of its 10:
    /// - with 2 shares resting, the trim is the 1-share remainder, and a case that states the
    ///   whole excess's 3 fails naming `qty`;
    /// - MC-B30 confirmed, with 2 resting and a 3-share minimum, withholds that remainder as
    ///   `below_minimum_order`, which the whole excess would meet, so a reading that ignored the
    ///   resting sell fails naming both answers;
    /// - with 3 or more resting, no trim is due at all, and the case states no `trim_withheld`.
    #[test]
    fn a_trim_is_sized_after_the_agent_s_resting_sells() -> Result<(), String> {
        let fixture = fixture()?;
        let remainder = doctored(&fixture, "MC-B17", "", |case| {
            put(case, "input", "open_sell_qty", json!("2"));
            put(case, "expect", "qty", json!("1"));
            put(case, "expect", "order_usd", json!("99.9"));
        })?;
        run(remainder, "MC-B17").map_err(|e| format!("MC-B17 beside 2 resting shares: {e}"))?;
        let whole_excess = doctored(&fixture, "MC-B17", "", |case| {
            put(case, "input", "open_sell_qty", json!("2"));
        })?;
        fails_naming(
            run(whole_excess, "MC-B17"),
            "qty",
            "MC-B17 beside 2 resting shares, stating the whole excess",
        )?;
        let below = doctored(&fixture, "MC-B30", "", |case| {
            put(case, "input", "scale_active_s", json!(120));
            put(case, "input", "open_sell_qty", json!("2"));
            put(case, "input", "min_order_size", json!("3"));
            put(
                case,
                "expect",
                "trim_withheld",
                json!(["below_minimum_order"]),
            );
        })?;
        run(below, "MC-B30").map_err(|e| {
            format!("MC-B30 confirmed, a 1-share remainder at a 3-share minimum: {e}")
        })?;
        for resting in ["3", "10"] {
            let covered = doctored(&fixture, "MC-B30", "", |case| {
                put(case, "input", "scale_active_s", json!(120));
                put(case, "input", "open_sell_qty", json!(resting));
                case.get_mut("expect")
                    .and_then(Json::as_object_mut)
                    .map(|members| members.remove("trim_withheld"));
            })?;
            run(covered, "MC-B30")
                .map_err(|e| format!("MC-B30 confirmed with {resting} shares resting: {e}"))?;
        }
        Ok(())
    }

    /// Both gate calls on a `trim_to_target` base see one scene (#498 review, m3). MC-B01, a
    /// 7-share opening buy, moved onto the trim base, where no trim is due at a factor of one, so
    /// its order reaches the builder's dry run:
    /// - at a `min_order_size` of 7 it passes as stated;
    /// - at 8 the dry run reads that minimum too, so the gate meets trading spec §5.3 rule 2 for the
    ///   opening, where a dry run on `qty_increment` would allow it. Until DEC-129 item 27 gives
    ///   rule 2 its reason code the gate refuses to decide, naming the rule; after it, the case
    ///   fails at `gate_dry_run`. Either names the minimum's rule or the dry run.
    ///
    /// The scene's Holding goal and active rung reach the dry run too, but only `trim_proposals`
    /// reads either, so no dry run can show them.
    #[test]
    fn both_gate_calls_on_a_trim_base_see_one_scene() -> Result<(), String> {
        let fixture = fixture()?;
        let on_the_trim_base = |minimum: &str| {
            doctored(&fixture, "MC-B01", "", |case| {
                if let Some(members) = case.as_object_mut() {
                    members.insert("base".to_owned(), json!("two_stock_swing_trim"));
                }
                put(case, "input", "min_order_size", json!(minimum));
            })
        };
        run(on_the_trim_base("7")?, "MC-B01")
            .map_err(|e| format!("MC-B01 on the trim base at a 7-share minimum: {e}"))?;
        match run(on_the_trim_base("8")?, "MC-B01") {
            Ok(()) => Err("MC-B01 on the trim base at an 8-share minimum: the case still passed")?,
            Err(e) if e.contains("gate_dry_run") || e.contains("§5.3 rule 2") => {}
            Err(e) => Err(format!(
                "MC-B01 on the trim base at an 8-share minimum: the failure names neither \
                 `gate_dry_run` nor §5.3 rule 2: {e}"
            ))?,
        }
        Ok(())
    }

    /// Sets `key` in the case's `object` member, adding it if the case does not state it.
    fn put(case: &mut Json, object: &str, key: &str, value: Json) {
        case.get_mut(object)
            .and_then(Json::as_object_mut)
            .map(|members| members.insert(key.to_owned(), value));
    }

    /// Whether the case's base trims (`scale_action: trim_to_target`), where the trim guards are
    /// inputs the arm reads rather than members it refuses.
    fn trims(fixture: &Json, case: &Json) -> Result<bool, String> {
        let base = crate::str_at(case, "base")?;
        Ok(fixture
            .pointer(&format!("/bases/{base}/mandate/risk/scale_action"))
            .is_some_and(|action| action == "trim_to_target"))
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
    /// A trim's reason, whose sibling is a hold reason, and the two proposers §6.2 step 1 orders.
    const TRIM_REASON: &str = "trim_to_target";
    const ORIGINS: [&str; 2] = ["risk_engine", "builder"];
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
            ("/expect", "reason") if value == TRIM_REASON => HOLD_REASONS.first().map(|r| json!(r)),
            ("/expect", "reason") => next_in(&HOLD_REASONS, value),
            ("/expect", "origin") => next_in(&ORIGINS, value),
            ("/expect", "trim_withheld") => {
                let stated: Vec<&str> = value.as_array()?.iter().filter_map(Json::as_str).collect();
                let other = [vec!["rung_not_confirmed"], vec!["holding"]]
                    .into_iter()
                    .find(|guards| *guards != stated)?;
                Some(json!(other))
            }
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

    /// The enum-valued members that need no sibling: `on_timeout` has one variant, `skip`, and
    /// `action` decides which other members a case may state, so a sibling fails at one of those
    /// before its own comparison.
    const NO_SIBLING: [&str; 2] = ["on_timeout", "action"];

    /// A snake-case word, the spelling of every closed vocabulary a family-B case states.
    fn is_word(value: &Json) -> bool {
        value.as_str().is_some_and(|text| {
            !text.is_empty() && text.chars().all(|c| c.is_ascii_lowercase() || c == '_')
        })
    }

    /// A stated value from a closed vocabulary: a word, the null a gate reason may be, or a list of
    /// words. Decimals, model ids (`llm.news_research`) and `rule:` deciders are none of these.
    fn is_enum_value(value: &Json) -> bool {
        match value {
            Json::Null => true,
            Json::Array(items) => !items.is_empty() && items.iter().all(is_word),
            _ => is_word(value),
        }
    }

    /// The expectation objects of `case`, each with its pointer.
    fn expectation_objects(case: &Json) -> Result<Vec<(String, Json)>, String> {
        let expect = crate::at(case, "expect")?;
        let mut objects = vec![("/expect".to_owned(), expect.clone())];
        for nested in ["gate_dry_run", "autonomy"] {
            if let Some(value) = expect.get(nested) {
                objects.push((format!("/expect/{nested}"), value.clone()));
            }
        }
        Ok(objects)
    }

    /// Every `(pointer, member)` some family-B case states with an enum value, read from the
    /// fixture rather than listed, so an empty `clipped_by` counts because another case lists a
    /// clip, and a case that gains an enum-valued expectation is swept without an edit here.
    fn enum_valued_members(fixture: &Json) -> Result<BTreeSet<(String, String)>, String> {
        let mut found = BTreeSet::new();
        for case in family_b(fixture)? {
            for (at, object) in expectation_objects(&case)? {
                for (member, value) in members(&object)? {
                    if is_enum_value(&value) {
                        found.insert((at.clone(), member));
                    }
                }
            }
        }
        Ok(found)
    }

    /// Every enum-valued expectation of case `id` in `fixture`, swapped for its sibling, fails the
    /// case at that member's comparison, whose message starts `<member>: expected`; and each one
    /// but [`NO_SIBLING`]'s has a [`sibling`] arm, so a vocabulary whose arm answers `None` fails
    /// here rather than going unswept.
    fn every_sibling_fails_its_comparison(fixture: &Json, id: &str) -> Result<(), String> {
        let enum_valued = enum_valued_members(fixture)?;
        let case = family_b(fixture)?
            .into_iter()
            .find(|c| c["id"] == id)
            .ok_or_else(|| format!("no case {id}"))?;
        for (at, object) in expectation_objects(&case)? {
            for (member, value) in members(&object)? {
                let Some(other) = sibling(&at, &member, &value) else {
                    crate::ensure(
                        !enum_valued.contains(&(at.clone(), member.clone()))
                            || NO_SIBLING.contains(&member.as_str()),
                        || format!("{id}: {at}/{member} is enum-valued and has no sibling arm"),
                    )?;
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
            }
        }
        Ok(())
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
    fn every_builder_case_passes() -> Result<(), String> {
        let fixture = fixture()?;
        let mut seen = 0_usize;
        for case in family_b(&fixture)? {
            let id = id(&case)?;
            crate::ensure(listed(&id), || format!("{id} is not listed"))?;
            run(fixture.clone(), &id).map_err(|e| format!("{id}: {e}"))?;
            seen = seen.saturating_add(1);
        }
        crate::expect_eq(
            "cases listed",
            counted(
                &fixture,
                PASSING.len(),
                PASSING.len().saturating_add(AWAITED.len()),
            )?,
            seen,
        )
    }

    /// MC-B26's pinned pair is read from its base's `symbol` (DEC-285): `BTC/USD` passes, and a
    /// pair quoted in a stablecoin, or a symbol that names no quote, fails at check 2 with
    /// `crypto_pair_not_usd`.
    #[test]
    fn a_pinned_pair_s_quote_currency_is_its_symbol_s() -> Result<(), String> {
        let fixture = fixture()?;
        let with_symbol = |symbol: &str| -> Result<Json, String> {
            let mut copy = fixture.clone();
            let pinned = copy
                .pointer_mut("/bases/btc_accumulator/mandate/universe/pinned_instruments/0")
                .and_then(Json::as_object_mut)
                .ok_or("btc_accumulator pins one instrument")?;
            pinned.insert("symbol".to_owned(), json!(symbol));
            Ok(copy)
        };
        run(with_symbol("BTC/USD")?, "MC-B26")?;
        for symbol in ["BTC/USDT", "BTCUSD", "BTC/usd"] {
            fails_naming(
                run(with_symbol(symbol)?, "MC-B26"),
                "Some(CryptoPairNotUsd)",
                &format!("MC-B26 pinned as {symbol}"),
            )?;
        }
        Ok(())
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
        for case in passing(&fixture)? {
            let id = id(&case)?;
            every_sibling_fails_its_comparison(&fixture, &id)?;
            for (at, object) in expectation_objects(&case)? {
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
        crate::expect_eq(
            "doctorings, counted from the fixture",
            doctorings,
            counted(&fixture, 1046, DOCTORINGS_WITH)?,
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
        crate::expect_eq("holds", holds, counted(&fixture, 14, 15)?)
    }

    /// Every member of a case is read: a plant at the top level, in `input`, its `quote`, its
    /// `gate_state`, each output and each working order fails naming the plant, and so does a
    /// trim guard on a mandate that does not trim, and an autonomy fact no base's rule reads, even
    /// stated as its default.
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
            for (guard, value) in [
                ("scale_active_s", json!(0)),
                ("holding", json!(false)),
                ("min_order_size", json!("1")),
                ("open_sell_qty", json!("0")),
            ] {
                if trims(&fixture, &case)? {
                    break;
                }
                let stated = doctored(&fixture, &id, "/input", insert(guard, value))?;
                fails_naming(run(stated, &id), guard, &format!("{id}: {guard} stated"))?;
            }
            for fact in ["drawdown", "daily_pnl_fraction", "bought_today_usd"] {
                let stated = doctored(&fixture, &id, "/input", insert(fact, json!("0")))?;
                fails_naming(run(stated, &id), fact, &format!("{id}: {fact} stated"))?;
            }
        }
        crate::expect_eq(
            "plants: four objects per case, every output and two working orders",
            plants,
            counted(&fixture, 35 * 4 + 65 + 2, PLANTS_WITH)?,
        )
    }

    /// Every input the harness reads is parsed, never defaulted: each one replaced by an unreadable
    /// word, and each one replaced by a value of the wrong type, fails the case naming it. That is
    /// the input's own scalars, the quote's bid and ask, the gate state's `agent_equity` and
    /// `orders_today`, its `positions_mv` and `last_exit_fill_at` maps whole and each value, its
    /// `working_universe` whole and each entry, each working order's `instrument` and `max_cost`,
    /// in the gate state and in a restated `working_opening_orders`, and each output's members. An
    /// entry planted in either map with an unreadable key or value fails naming the map, since no
    /// family-B case states a `last_exit_fill_at` entry for the sweep to replace.
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
            for map in ["positions_mv", "last_exit_fill_at"] {
                let named = format!("gate_state.{map}");
                scalars.push((format!("/input/gate_state/{map}"), named.clone()));
                for (key, _) in members(crate::at(input, &named)?)? {
                    scalars.push((format!("/input/gate_state/{map}/{key}"), named.clone()));
                }
            }
            let universe = "gate_state.working_universe";
            scalars.push((
                "/input/gate_state/working_universe".to_owned(),
                universe.to_owned(),
            ));
            for (index, _) in crate::list_at(input, universe)?.iter().enumerate() {
                scalars.push((
                    format!("/input/gate_state/working_universe/{index}"),
                    universe.to_owned(),
                ));
            }
            for (pointer, what) in [
                (
                    "/input/gate_state/working_opening_orders",
                    "gate_state.working_opening_orders",
                ),
                ("/input/working_opening_orders", "working_opening_orders"),
            ] {
                let listed = case.pointer(pointer).and_then(Json::as_array);
                for (index, _) in listed.into_iter().flatten().enumerate() {
                    for key in ["instrument", "max_cost"] {
                        scalars.push((
                            format!("{pointer}/{index}/{key}"),
                            format!("`{what}[{index}].{key}`"),
                        ));
                    }
                }
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
                    *v = if v.is_string() || v.is_array() {
                        json!(7)
                    } else {
                        json!([])
                    };
                })?;
                fails_naming(
                    run(mistyped, &id),
                    &named,
                    &format!("{id}: {pointer} mistyped"),
                )?;
                refused = refused.saturating_add(1);
            }
            let instrument = crate::str_at(input, "instrument")?;
            for (map, readable) in [
                ("positions_mv", json!("1")),
                ("last_exit_fill_at", json!("2026-09-01T00:00:00.000000000Z")),
            ] {
                let pointer = format!("/input/gate_state/{map}");
                for (key, value) in [
                    (PLANTED, readable),
                    (instrument, json!(PLANTED)),
                    (instrument, json!(7)),
                ] {
                    let planted = doctored(&fixture, &id, &pointer, insert(key, value.clone()))?;
                    fails_naming(
                        run(planted, &id),
                        &format!("gate_state.{map}"),
                        &format!("{id}: {pointer} given {key}: {value}"),
                    )?;
                    refused = refused.saturating_add(1);
                }
            }
        }
        crate::expect_eq(
            "inputs refused, counted from the fixture",
            refused,
            counted(&fixture, 1390, REFUSED_WITH)?,
        )
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
                Box::new(insert("fee_rate_asset", json!("0.001"))),
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

    /// The gate is handed no fee reservation, so a cash fee rate above zero is refused, for a crypto
    /// case as for an equity one; a stated zero, the header's default, is read and passes, and an
    /// unreadable one fails naming it.
    #[test]
    fn a_cash_fee_rate_above_zero_is_refused() -> Result<(), String> {
        let fixture = fixture()?;
        for id in ["MC-B01", "MC-B29"] {
            let charged = doctored(
                &fixture,
                id,
                "/input",
                insert("fee_rate_cash", json!("0.001")),
            )?;
            fails_naming(
                run(charged, id),
                "`fee_rate_cash` above zero",
                &format!("{id}: a cash fee rate"),
            )?;
            let unreadable = doctored(
                &fixture,
                id,
                "/input",
                insert("fee_rate_cash", json!(PLANTED)),
            )?;
            fails_naming(
                run(unreadable, id),
                "fee_rate_cash",
                &format!("{id}: an unreadable cash fee rate"),
            )?;
            let free = doctored(&fixture, id, "/input", insert("fee_rate_cash", json!("0")))?;
            run(free, id).map_err(|e| format!("{id} with a stated zero cash fee rate: {e}"))?;
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

    /// MC-B22, after hours, and MC-B23, in the close window, pass as the fixture states them, and
    /// each enum-valued expectation swapped for a sibling fails at its comparison. MC-B23 is the
    /// only case that states a sell's `order_type` and MC-B22 the only deferred one, so these two
    /// comparisons are proved here by name as well as by [`PASSING`]'s sweep. A label that
    /// contradicts `now` failing the case is proved by
    /// `the_builder_and_the_gate_see_one_scene`'s MC-B01, given `after_hours` and a close window
    /// at a regular-session instant (DEC-250 item 3).
    #[test]
    fn the_two_session_cases_pass_as_stated_and_compare_every_sibling() -> Result<(), String> {
        let fixture = fixture()?;
        for id in ["MC-B22", "MC-B23"] {
            run(fixture.clone(), id).map_err(|e| format!("{id}: {e}"))?;
            every_sibling_fails_its_comparison(&fixture, id)?;
        }
        Ok(())
    }
}
