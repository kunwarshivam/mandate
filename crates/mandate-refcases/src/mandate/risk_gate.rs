//! Families G and F of the `mandate` suite: the `gate` cases (mandate spec §5.3 through trading-domain
//! spec §9.1) and the `agent_flatten` cases (the agent-scoped kill switch, trading-domain spec §5.5),
//! run through `mandate-risk`'s `evaluate` and `agent_flatten` (DEC-178).
//!
//! **Every key is read (DEC-85).** A `gate` case's `state`, each working opening order, `proposed`,
//! and `expect.computed`, and an `agent_flatten` case's `input`, each open order, position, broker
//! position, sell, and deferred sell, are swept against the members this module reads; a member it
//! does not know fails the case naming it. `expect` itself is swept by the suite's `EXPECT_KEYS`.
//!
//! **What the harness fills for a `gate` case, and why.** The case's patched base is parsed and
//! validated by `mandate-spec` against `validation_context_defaults`, and its risk block, sizing band,
//! and universe flags are projected onto `mandate-risk`'s own `ValidatedMandate`, which is still that
//! crate's stand-in type (`spec_types`, DEC-128 item 21). The header's "every other check passes" is
//! an environment in which they do: a healthy risk state at the case's `agent_equity`, a margin
//! account at the fixture's `account_equity_usd` that holds what the agent holds, a liquid,
//! tradable, unhalted NASDAQ equity quoting at the proposal's own limit price at `now`, and the
//! organisation settings of the trading-domain `test_default` configuration. The purpose is never
//! injected: the harness picks the `Origin` the case's purpose implies and gives the agent a
//! position at least the size of a sell (DEC-129 item 19), and the gate's own assignment must equal
//! the case's. The full gate is driven, not `ref.py`'s mandate-limit subset (DEC-150 item 1), with
//! [`FULL_GATE_ONLY`] naming the one case whose own state trips a check outside that subset.

use std::collections::{BTreeMap, BTreeSet};

use mandate_num::{Fraction, Price, Qty, Ratio, Usd};
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};
use mandate_risk::{
    AccountSnapshot, AccountState, AccountType, AgentId, AgentMode, AgentPosition, AgentSnapshot,
    AssetClass, AssetId, Check, CheckOutcome, ClientOrderId, ConductState, DayTradeLedger,
    DayTradeRegime, Decision, DeferredSell, EtpClass, Exchange, FlattenInitiator, FlattenInput,
    FlattenPricing, FlattenSell, GateConfig, GateError, GateInput, GatePass, GroupId,
    InstrumentSnapshot, MarketSnapshot, Origin, ProposedKind, ProposedOrder, Purpose, ReasonCode,
    RiskSnapshot, SaneQuote, Session, Side, TimeInForce, ValidatedMandate, Verdict, WorkingOrder,
    WorkingUniverse, agent_flatten, evaluate,
};
use mandate_spec::document::{self as spec_doc, LadderAction, Mandate};
use mandate_time::{UtcNanos, new_york_date_and_hour};

use super::{at_of, instant, num, unknown_members, validated_mandate};
use crate::{Json, at, ensure, expect_eq, list_at, str_at, u64_at};

/// The members a `gate` case's `expect` states, which `EXPECT_KEYS` sweeps.
pub(super) const GATE_EXPECT_KEYS: &[&str] = &["verdict", "reason", "computed"];
/// The members an `agent_flatten` case's `expect` states, which `EXPECT_KEYS` sweeps.
pub(super) const FLATTEN_EXPECT_KEYS: &[&str] = &[
    "mode_applied_first",
    "purpose",
    "cancel_client_order_ids",
    "cancel_all_endpoint",
    "close_position_endpoint",
    "sells",
    "deferred_sells",
];

const STATE_KEYS: &[&str] = &[
    "now",
    "agent_equity",
    "positions_mv",
    "working_opening_orders",
    "orders_today",
    "last_exit_fill_at",
    "working_universe",
    "instrument_groups",
];
const WORKING_ORDER_KEYS: &[&str] = &["instrument", "max_cost"];
const PROPOSED_KEYS: &[&str] = &["instrument", "purpose", "qty", "limit_price"];
/// Every figure `mandate_risk::Computed` can report, by the name the cases use.
const COMPUTED_KEYS: &[&str] = &[
    "instrument_total",
    "cap",
    "order_usd",
    "gross",
    "gross_limit",
    "orders_today",
    "last_exit_fill_at",
    "instrument",
];

const INPUT_KEYS: &[&str] = &[
    "agent",
    "open_orders",
    "agent_positions",
    "broker_positions",
    "session",
    "initiator",
    "owner_confirmed_bid",
    "confirmed_bid",
    "max_exit_offset",
];
const OPEN_ORDER_KEYS: &[&str] = &["client_order_id", "agent", "instrument"];
const POSITION_KEYS: &[&str] = &["agent", "instrument", "asset_class", "qty"];
const BROKER_POSITION_KEYS: &[&str] = &["instrument", "qty"];
const SELL_KEYS: &[&str] = &["instrument", "qty", "pricing", "floor_price", "remainder"];
const DEFERRED_KEYS: &[&str] = &["instrument", "qty", "until"];

/// A `gate` case the full gate rightly decides differently from `ref.py`'s mandate-limit subset,
/// named case by case, never by rule (DEC-150 item 1, the ruling on #217).
struct FullGateOnly {
    case: &'static str,
    /// The verdict and reason the case states, pinned so an edit to the case cannot pass unseen.
    stated: (Verdict, Option<ReasonCode>),
    check: Check,
    reason: ReasonCode,
    /// The `computed` figures the deciding check stops the gate before reaching, with the values the
    /// case states for them: pinned to the case's text, and asserted absent from the gate's report.
    unreached: &'static [(&'static str, &'static str)],
}

/// `MC-G02` proposes an increase in instrument 2 beside a working opening order in instrument 2, so
/// trading-domain §5.3 rule 6 (one working non-protective order per instrument per account, check 4)
/// denies it `working_order_limit` before check 7 computes its gross exposure. For a listed case the
/// arm requires the case to state what the entry pins, checks 1 and 2 to pass, the named check to be
/// the only failure and to decide the verdict, the reached figures to match, and the unreached ones
/// to be absent; and a listed case whose full verdict already matches its `expect` fails, so the
/// entry expires with its reason.
const FULL_GATE_ONLY: &[FullGateOnly] = &[FullGateOnly {
    case: "MC-G02",
    stated: (Verdict::Allow, None),
    check: Check::OrderConstraints,
    reason: ReasonCode::WorkingOrderLimit,
    unreached: &[("gross", "1500"), ("gross_limit", "2000")],
}];

/// `kind: gate` — one proposed order through `mandate_risk::evaluate`: the verdict, the reason, the
/// purpose the gate assigns, the one failing check, and every `computed` figure the case states.
pub(super) fn gate_case(fixture: &Json, case: &Json) -> Result<(), String> {
    let id = str_at(case, "id")?;
    let validated = validated_mandate(fixture, case)?;
    let state = at_of(case, "state")?;
    unknown_members(state, STATE_KEYS)
        .map_err(|unknown| format!("`state` members not interpreted: {unknown}"))?;
    let proposed = at_of(case, "proposed")?;
    unknown_members(proposed, PROPOSED_KEYS)
        .map_err(|unknown| format!("`proposed` members not interpreted: {unknown}"))?;
    let expect = at_of(case, "expect")?;
    let want_verdict = verdict_named(str_at(expect, "verdict")?)?;
    let want_reason = match at(expect, "reason")? {
        Json::Null => None,
        Json::String(text) => Some(reason_named(text)?),
        _ => return Err("`reason` is neither a reason code nor null".to_owned()),
    };
    let computed = match expect.get("computed") {
        Some(stated) => {
            unknown_members(stated, COMPUTED_KEYS)
                .map_err(|unknown| format!("`computed` members not interpreted: {unknown}"))?;
            Some(stated)
        }
        None => None,
    };
    let purpose = purpose_named(str_at(proposed, "purpose")?)?;
    let account_equity = num(
        Usd::parse(str_at(
            at_of(fixture, "validation_context_defaults")?,
            "account_equity_usd",
        )?),
        "account_equity_usd",
    )?;
    let scene = Scene::read(
        validated.mandate(),
        state,
        proposed,
        purpose,
        account_equity,
    )?;
    let decision = evaluate(&scene.input()).map_err(|e| gate_error("evaluate", &e))?;

    expect_eq("purpose", decision.purpose, purpose)?;
    if let Some(listed) = FULL_GATE_ONLY.iter().find(|entry| entry.case == id) {
        return full_gate_only(listed, &decision, (want_verdict, want_reason), computed);
    }
    expect_eq("verdict", decision.verdict, want_verdict)?;
    expect_eq("reason", decision.reason, want_reason)?;
    let failed = failed_checks(&decision);
    match want_reason {
        Some(reason) => ensure(
            failed.len() == 1 && failed.iter().all(|(_, code)| *code == reason),
            || format!("the one failing check must be `{reason}`, got {failed:?}"),
        )?,
        None => expect_eq(
            "an allowed order's checks",
            decision.checks.as_slice(),
            EVERY_CHECK_PASSED.as_slice(),
        )?,
    }
    match computed {
        Some(stated) => compare_computed(stated, &decision, &[]),
        None => Ok(()),
    }
}

/// Trading-domain §9.1's eight checks in its order, each passed: what an allowed US-equity order
/// must report, so an allow cannot come from a check the gate skipped or listed as not reached.
const EVERY_CHECK_PASSED: [CheckOutcome; 8] = [
    CheckOutcome::Passed(Check::AccountAndMode),
    CheckOutcome::Passed(Check::UniverseAndLimits),
    CheckOutcome::Passed(Check::SessionAndHalt),
    CheckOutcome::Passed(Check::OrderConstraints),
    CheckOutcome::Passed(Check::MarkAndCollar),
    CheckOutcome::Passed(Check::ConductControls),
    CheckOutcome::Passed(Check::BuyingPowerAndExposure),
    CheckOutcome::Passed(Check::DayTradeBudget),
];

/// The [`FULL_GATE_ONLY`] assertions for one listed case.
fn full_gate_only(
    listed: &FullGateOnly,
    decision: &Decision,
    wanted: (Verdict, Option<ReasonCode>),
    computed: Option<&Json>,
) -> Result<(), String> {
    ensure((decision.verdict, decision.reason) != wanted, || {
        format!(
            "{}: the full gate now gives the case's own verdict, so its FULL_GATE_ONLY entry has \
             expired; delete it",
            listed.case
        )
    })?;
    let pinned = |what: &str| {
        format!(
            "{what}: the case no longer states what its FULL_GATE_ONLY entry pins; re-review the entry"
        )
    };
    ensure(wanted.0 == listed.stated.0, || pinned("verdict"))?;
    ensure(wanted.1 == listed.stated.1, || pinned("reason"))?;
    expect_eq(
        "the full gate's verdict and reason",
        (decision.verdict, decision.reason),
        (Verdict::Deny, Some(listed.reason)),
    )?;
    expect_eq(
        "checks 1 and 2, where the case's own limits sit",
        decision.checks.get(..2),
        Some(
            &[
                CheckOutcome::Passed(Check::AccountAndMode),
                CheckOutcome::Passed(Check::UniverseAndLimits),
            ][..],
        ),
    )?;
    expect_eq(
        "the failing checks",
        failed_checks(decision),
        vec![(listed.check, listed.reason)],
    )?;
    let stated = computed.ok_or_else(|| pinned("computed"))?;
    for (key, _) in listed.unreached {
        ensure(stated.get(*key).is_some(), || {
            pinned(&format!("computed.{key}"))
        })?;
    }
    compare_computed(stated, decision, listed.unreached)
}

fn failed_checks(decision: &Decision) -> Vec<(Check, ReasonCode)> {
    decision
        .checks
        .iter()
        .filter_map(|outcome| match outcome {
            CheckOutcome::Failed(check, reason) => Some((*check, *reason)),
            CheckOutcome::Passed(_) | CheckOutcome::NotReached(_) => None,
        })
        .collect()
}

/// Every figure the case states, compared by value with the one the gate reports; a figure the gate
/// did not report fails, and one in `unreached` must state its pinned value and be absent from the
/// report rather than compared with it.
fn compare_computed(
    stated: &Json,
    decision: &Decision,
    unreached: &[(&str, &str)],
) -> Result<(), String> {
    let members = stated
        .as_object()
        .ok_or("`computed` is not an object")?
        .iter();
    let c = &decision.computed;
    for (key, want) in members {
        let what = format!("computed.{key}");
        if let Some((_, pinned)) = unreached.iter().find(|(name, _)| name == key) {
            expect_eq(
                &format!("{what}: as its FULL_GATE_ONLY entry pins it"),
                text(want, &what)?.as_str(),
                *pinned,
            )?;
            ensure(c.get(key).is_none(), || {
                format!("{what}: after the deciding check, so the gate must not have reached it")
            })?;
            continue;
        }
        let missing = || format!("{what}: the case states it, and the gate did not report it");
        match key.as_str() {
            "instrument_total" | "cap" | "order_usd" | "gross" | "gross_limit" => {
                let got = match key.as_str() {
                    "instrument_total" => c.instrument_total,
                    "cap" => c.cap,
                    "order_usd" => c.order_usd,
                    "gross" => c.gross,
                    _ => c.gross_limit,
                };
                let wanted = num(Usd::parse(&text(want, &what)?), &what)?;
                expect_eq(&what, got.ok_or_else(missing)?, wanted)?;
            }
            "orders_today" => {
                let wanted = want
                    .as_u64()
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or_else(|| format!("{what} is not a count"))?;
                expect_eq(&what, c.orders_today.ok_or_else(missing)?, wanted)?;
            }
            "last_exit_fill_at" => {
                let wanted = instant(want, &what)?;
                expect_eq(&what, c.last_exit_fill_at.ok_or_else(missing)?, wanted)?;
            }
            "instrument" => {
                let wanted = asset(&text(want, &what)?)?;
                expect_eq(&what, c.instrument.clone().ok_or_else(missing)?, wanted)?;
            }
            other => return Err(format!("`computed.{other}` is not interpreted")),
        }
    }
    Ok(())
}

/// Everything one `gate` decision reads, owned, so [`Scene::input`] can lend it out.
struct Scene {
    now: UtcNanos,
    config: GateConfig,
    mandate: ValidatedMandate,
    risk: RiskSnapshot,
    account: AccountSnapshot,
    agent: AgentSnapshot,
    instrument: InstrumentSnapshot,
    market: MarketSnapshot,
    conduct: ConductState,
    universe: WorkingUniverse,
    proposed: ProposedOrder,
}

/// The one agent every `gate` case is about.
const THE_AGENT: AgentId = AgentId(1);

impl Scene {
    fn read(
        document: &Mandate,
        state: &Json,
        proposed: &Json,
        purpose: Purpose,
        account_equity: Usd,
    ) -> Result<Self, String> {
        let now = instant(at(state, "now")?, "state.now")?;
        let agent_equity = num(
            Usd::parse(str_at(state, "agent_equity")?),
            "state.agent_equity",
        )?;
        let instrument = asset(str_at(proposed, "instrument")?)?;
        let qty = num(Qty::parse(str_at(proposed, "qty")?), "proposed.qty")?;
        let limit_price = num(
            Price::parse(str_at(proposed, "limit_price")?),
            "proposed.limit_price",
        )?;
        let (origin, side) = origin_for(purpose);
        let one = one_share()?;

        let mut market_values = BTreeMap::new();
        let mut positions = BTreeMap::new();
        for (held, value) in object_at(state, "positions_mv")? {
            let held = asset(held)?;
            let value = num(Usd::parse(&text(value, "positions_mv")?), "positions_mv")?;
            market_values.insert(held.clone(), value);
            positions.insert(held, one);
        }
        if side == Side::Sell {
            positions.insert(instrument.clone(), qty);
        }

        let (today, _) = new_york_date_and_hour(now).map_err(|e| format!("`state.now`: {e}"))?;
        let mut working_orders = BTreeMap::new();
        let mut next_id: u64 = 1;
        for order in list_at(state, "working_opening_orders")? {
            unknown_members(order, WORKING_ORDER_KEYS).map_err(|unknown| {
                format!("working opening order members not interpreted: {unknown}")
            })?;
            working_orders.insert(
                ClientOrderId(next_id),
                WorkingOrder {
                    agent: THE_AGENT,
                    instrument: asset(str_at(order, "instrument")?)?,
                    side: Side::Buy,
                    max_cost: num(Usd::parse(str_at(order, "max_cost")?), "max_cost")?,
                    open_qty: one,
                    protective: false,
                    opening: true,
                    submitted_on: today,
                },
            );
            next_id = next_id.saturating_add(1);
        }

        let mut last_exit_fill_at = BTreeMap::new();
        for (exited, when) in object_at(state, "last_exit_fill_at")? {
            last_exit_fill_at.insert(asset(exited)?, instant(when, "last_exit_fill_at")?);
        }
        let mut instrument_groups = BTreeMap::new();
        if let Some(groups) = state.get("instrument_groups") {
            let groups = groups
                .as_object()
                .ok_or("`state.instrument_groups` is not an object")?;
            let names: BTreeSet<String> = groups
                .values()
                .map(|g| text(g, "instrument_groups"))
                .collect::<Result<_, _>>()?;
            for (member, group) in groups {
                let name = text(group, "instrument_groups")?;
                let rank = names.iter().position(|n| *n == name).ok_or_else(|| {
                    format!("`state.instrument_groups`: the group `{name}` has no rank")
                })?;
                let rank = u64::try_from(rank).map_err(|e| e.to_string())?;
                instrument_groups.insert(asset(member)?, GroupId(rank));
            }
        }

        let listed = list_at(state, "working_universe")?;
        let instruments = listed
            .iter()
            .map(|i| asset(&text(i, "working_universe")?))
            .collect::<Result<BTreeSet<_>, String>>()?;
        ensure(instruments.len() == listed.len(), || {
            "`state.working_universe` names an instrument twice".to_owned()
        })?;

        let orders_today = u32::try_from(u64_at(state, "orders_today")?)
            .map_err(|_| "`state.orders_today` does not fit a u32".to_owned())?;

        Ok(Self {
            now,
            config: test_default_gate_config()?,
            mandate: gate_mandate(document)?,
            risk: RiskSnapshot {
                agent_equity,
                high_water_mark: agent_equity,
                day_start_equity: agent_equity,
                capital_base: agent_equity,
                inherited_loss: Usd::ZERO,
                latched: BTreeSet::new(),
                active_rungs: BTreeMap::new(),
                size_factor: num(Ratio::parse("1"), "size_factor")?,
                agent_mode: AgentMode::Normal,
            },
            account: AccountSnapshot {
                account_type: AccountType::Margin,
                state: AccountState::Active,
                crypto_active: true,
                regime: DayTradeRegime::IntradayMargin {
                    maintenance_excess: account_equity,
                },
                equity: account_equity,
                prior_close_equity: account_equity,
                model_buying_power: account_equity,
                broker_buying_power: account_equity,
                broker_non_marginable_buying_power: account_equity,
                positions: positions.clone(),
                market_values: market_values.clone(),
                working_orders: working_orders.clone(),
                unknown_orders: BTreeSet::new(),
                related_account_resting: BTreeMap::new(),
            },
            agent: AgentSnapshot {
                agent: THE_AGENT,
                mode: AgentMode::Normal,
                instrument_restrictions: BTreeMap::new(),
                positions,
                market_values,
                working_orders: working_orders.keys().copied().collect(),
                instrument_groups,
                last_exit_fill_at,
                orders_today,
                day_trades: DayTradeLedger::default(),
            },
            instrument: listed_equity(document, &instrument, limit_price, now)?,
            market: MarketSnapshot {
                quote: Some(SaneQuote {
                    bid: limit_price,
                    ask: limit_price,
                    at: now,
                }),
                last_trade: Some((limit_price, now)),
                trailing_5m_volume: Some(num(Qty::parse("1000000"), "trailing_5m_volume")?),
                adv_20d: Some(num(Qty::parse("10000000"), "adv_20d")?),
            },
            conduct: ConductState::default(),
            universe: WorkingUniverse::Known {
                instruments,
                pinned: document.universe.pinned,
            },
            proposed: ProposedOrder {
                instrument,
                side,
                qty,
                limit_price,
                kind: ProposedKind::Plain,
                tif: TimeInForce::Day,
                extended_hours: false,
                origin,
                owner_confirmed_bid: None,
                client_order_id: ClientOrderId(next_id),
                fee_reservation: Usd::ZERO,
            },
        })
    }

    fn input(&self) -> GateInput<'_> {
        GateInput {
            now: self.now,
            pass: GatePass::First,
            config: &self.config,
            mandate: &self.mandate,
            risk: &self.risk,
            account: &self.account,
            agent: &self.agent,
            instrument: &self.instrument,
            market: &self.market,
            conduct: &self.conduct,
            universe: &self.universe,
            proposed: &self.proposed,
        }
    }
}

/// The proposed instrument as the gate's eligibility floor, sessions, and collar see it: the pinned
/// entry's asset class, and otherwise a liquid, tradable, unhalted NASDAQ listing that last closed and
/// now quotes at the proposal's limit price, so every check outside the mandate limits passes.
///
/// A US equity only: the gate still owes check 2's "USD pairs only" for crypto (E6-10), and no
/// `gate` case proposes one, so a crypto proposal fails here rather than at an unrelated stub.
fn listed_equity(
    document: &Mandate,
    instrument: &AssetId,
    price: Price,
    now: UtcNanos,
) -> Result<InstrumentSnapshot, String> {
    let pinned = document
        .universe
        .pinned_instruments
        .iter()
        .find(|entry| entry.asset_id.as_str() == instrument.as_str())
        .ok_or_else(|| {
            format!(
                "`{}` is not a pinned instrument of the base, so the harness has no asset class \
                 for it",
                instrument.as_str()
            )
        })?;
    ensure(
        pinned.asset_class == mandate_domain::AssetClass::UsEquity,
        || "the `gate` arm builds a US-equity instrument only".to_owned(),
    )?;
    Ok(InstrumentSnapshot {
        instrument: instrument.clone(),
        asset_class: AssetClass::UsEquity,
        exchange: Some(Exchange::Nasdaq),
        status_active: true,
        tradable: true,
        fractionable: false,
        ipo: false,
        ptp_no_exception: false,
        etp: EtpClass::Plain,
        etp_classified_at: Some(now),
        prior_close: Some(price),
        median_dollar_volume_20d: Some(num(Usd::parse("90000000"), "median_dollar_volume_20d")?),
        median_dollar_volume_30d: None,
        min_order_size: one_share()?,
        halted: false,
        status_feed_current: true,
    })
}

/// `configs.test_default` of the trading-domain reference cases, with the five settings it does not
/// carry (the crypto liquidity floor, the minimum resting time, the two participation caps, and the
/// ETP classification age) at the values `mandate-risk`'s own `test_default_config` uses.
fn test_default_gate_config() -> Result<GateConfig, String> {
    let usd = |text: &str| num(Usd::parse(text), "the gate configuration");
    let fraction = |text: &str| num(Fraction::parse(text), "the gate configuration");
    Ok(GateConfig {
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

/// The validated document's risk block, sizing band, and universe flags as `mandate-risk`'s
/// stand-in `ValidatedMandate`. Every value comes from the document; a rung's `scale_action` is the
/// block's, which the schema states once for the whole ladder.
fn gate_mandate(document: &Mandate) -> Result<ValidatedMandate, String> {
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
    Ok(ValidatedMandate::from_validated_parts(
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
        GoalState::Running,
        document.universe.leveraged_etps_enabled,
        document.universe.leveraged_etp_disclosure_version.is_some(),
    ))
}

/// The `Origin` and side that make the gate assign `purpose` (DEC-129 item 19): the order builder
/// buys to open or increase and sells a discretionary exit; the risk engine sells a risk exit; the
/// owner closes an owner exit; a protective leg is protective.
fn origin_for(purpose: Purpose) -> (Origin, Side) {
    match purpose {
        Purpose::Open | Purpose::Increase => (Origin::OrderBuilder, Side::Buy),
        Purpose::DiscretionaryExit => (Origin::OrderBuilder, Side::Sell),
        Purpose::RiskExit => (Origin::RiskEngine, Side::Sell),
        Purpose::OwnerExit => (Origin::OwnerClose, Side::Sell),
        Purpose::Protective => (Origin::ProtectiveLeg, Side::Sell),
    }
}

fn purpose_named(text: &str) -> Result<Purpose, String> {
    match text {
        "open" => Ok(Purpose::Open),
        "increase" => Ok(Purpose::Increase),
        "discretionary_exit" => Ok(Purpose::DiscretionaryExit),
        "risk_exit" => Ok(Purpose::RiskExit),
        "owner_exit" => Ok(Purpose::OwnerExit),
        "protective" => Ok(Purpose::Protective),
        other => Err(format!("`{other}` is not a purpose")),
    }
}

fn verdict_named(text: &str) -> Result<Verdict, String> {
    match text {
        "allow" => Ok(Verdict::Allow),
        "deny" => Ok(Verdict::Deny),
        "defer" => Ok(Verdict::Defer),
        "hold" => Ok(Verdict::Hold),
        other => Err(format!("`{other}` is not a verdict")),
    }
}

/// A reason code by its registered spelling, searched over every variant so a correct gate can only
/// fail here on a code the crate cannot emit.
fn reason_named(text: &str) -> Result<ReasonCode, String> {
    ReasonCode::ALL
        .iter()
        .copied()
        .find(|code| code.as_str() == text)
        .ok_or_else(|| format!("`{text}` is not a registered reason code"))
}

/// `kind: agent_flatten` — `mandate_risk::agent_flatten`'s plan compared whole: the mode applied
/// first, the purpose, this agent's cancels in order, both account-wide endpoints, every sell with its
/// pricing, floor, and remainder, and every deferred sell.
///
/// Client order ids and agents are names in the fixture and integers in the crate: each order's id is
/// the rank of its name among the case's names, so the plan's ascending ids are the names in text
/// order, and each agent is numbered by first appearance. `max_exit_offset` is read when stated and is
/// otherwise zero, which no plan reads: the floor exists only for an owner's confirmed bid, and a case
/// that confirms one without an offset fails.
pub(super) fn agent_flatten_case(case: &Json) -> Result<(), String> {
    let input = at_of(case, "input")?;
    unknown_members(input, INPUT_KEYS)
        .map_err(|unknown| format!("`input` members not interpreted: {unknown}"))?;
    let expect = at_of(case, "expect")?;

    let mut agents: BTreeMap<String, AgentId> = BTreeMap::new();
    let mut agent_id = |name: &str| {
        let next = u64::try_from(agents.len()).unwrap_or(u64::MAX);
        *agents.entry(name.to_owned()).or_insert(AgentId(next))
    };
    let me = agent_id(str_at(input, "agent")?);

    let listed_orders = list_at(input, "open_orders")?;
    let names: BTreeSet<String> = listed_orders
        .iter()
        .map(|o| str_at(o, "client_order_id").map(str::to_owned))
        .collect::<Result<_, _>>()?;
    ensure(names.len() == listed_orders.len(), || {
        "two open orders share a client order id".to_owned()
    })?;
    let order_id = |name: &str| -> Result<ClientOrderId, String> {
        let rank = names
            .iter()
            .position(|n| n == name)
            .ok_or_else(|| format!("`{name}` is not an open order of this case"))?;
        Ok(ClientOrderId(
            u64::try_from(rank).map_err(|e| e.to_string())?,
        ))
    };
    let one = one_share()?;
    let mut open_orders = BTreeMap::new();
    for order in listed_orders {
        unknown_members(order, OPEN_ORDER_KEYS)
            .map_err(|unknown| format!("open order members not interpreted: {unknown}"))?;
        open_orders.insert(
            order_id(str_at(order, "client_order_id")?)?,
            WorkingOrder {
                agent: agent_id(str_at(order, "agent")?),
                instrument: asset(str_at(order, "instrument")?)?,
                side: Side::Buy,
                max_cost: Usd::ZERO,
                open_qty: one,
                protective: false,
                opening: true,
                submitted_on: mandate_time::Date::parse("2026-01-01")
                    .map_err(|e| format!("a placeholder date: {e}"))?,
            },
        );
    }

    let agent_positions = list_at(input, "agent_positions")?
        .iter()
        .map(|p| {
            unknown_members(p, POSITION_KEYS)
                .map_err(|unknown| format!("agent position members not interpreted: {unknown}"))?;
            Ok(AgentPosition {
                agent: agent_id(str_at(p, "agent")?),
                instrument: asset(str_at(p, "instrument")?)?,
                asset_class: asset_class_named(str_at(p, "asset_class")?)?,
                qty: num(Qty::parse(str_at(p, "qty")?), "agent_positions.qty")?,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    let mut broker_positions = BTreeMap::new();
    for p in list_at(input, "broker_positions")? {
        unknown_members(p, BROKER_POSITION_KEYS)
            .map_err(|unknown| format!("broker position members not interpreted: {unknown}"))?;
        broker_positions.insert(
            asset(str_at(p, "instrument")?)?,
            num(Qty::parse(str_at(p, "qty")?), "broker_positions.qty")?,
        );
    }

    let confirmed = match input.get("owner_confirmed_bid") {
        None => false,
        Some(flag) => flag
            .as_bool()
            .ok_or("`owner_confirmed_bid` is not a boolean")?,
    };
    let owner_confirmed_bid = match (confirmed, input.get("confirmed_bid")) {
        (true, Some(_)) => Some(num(
            Price::parse(str_at(input, "confirmed_bid")?),
            "confirmed_bid",
        )?),
        (true, None) => return Err("the owner confirmed a bid the case does not state".to_owned()),
        (false, Some(_)) => {
            return Err("the case states a `confirmed_bid` the owner did not confirm".to_owned());
        }
        (false, None) => None,
    };
    let max_exit_offset = match (input.get("max_exit_offset"), owner_confirmed_bid) {
        (Some(_), _) => num(
            Fraction::parse(str_at(input, "max_exit_offset")?),
            "max_exit_offset",
        )?,
        (None, Some(_)) => {
            return Err(
                "a confirmed bid needs the `max_exit_offset` its floor is priced at".into(),
            );
        }
        (None, None) => Fraction::ZERO,
    };

    let plan = agent_flatten(&FlattenInput {
        agent: me,
        open_orders: &open_orders,
        agent_positions: &agent_positions,
        broker_positions: &broker_positions,
        session: session_named(str_at(input, "session")?)?,
        initiator: match str_at(input, "initiator")? {
            "risk_limit" => FlattenInitiator::RiskLimit,
            "owner" => FlattenInitiator::Owner,
            other => return Err(format!("`{other}` is not a flatten initiator")),
        },
        owner_confirmed_bid,
        max_exit_offset,
        owner_floor_price: None,
    })
    .map_err(|e| gate_error("agent_flatten", &e))?;

    expect_eq(
        "mode_applied_first",
        plan.mode_applied_first,
        mode_named(str_at(expect, "mode_applied_first")?)?,
    )?;
    expect_eq(
        "purpose",
        plan.purpose,
        purpose_named(str_at(expect, "purpose")?)?,
    )?;
    let cancels = list_at(expect, "cancel_client_order_ids")?
        .iter()
        .map(|name| order_id(&text(name, "cancel_client_order_ids")?))
        .collect::<Result<Vec<_>, String>>()?;
    expect_eq(
        "cancel_client_order_ids",
        plan.cancel_client_order_ids,
        cancels,
    )?;
    for (key, got) in [
        ("cancel_all_endpoint", plan.cancel_all_endpoint),
        ("close_position_endpoint", plan.close_position_endpoint),
    ] {
        let wanted = at(expect, key)?
            .as_bool()
            .ok_or_else(|| format!("`{key}` is not a boolean"))?;
        expect_eq(key, got, wanted)?;
    }
    let sells = list_at(expect, "sells")?
        .iter()
        .map(flatten_sell)
        .collect::<Result<Vec<_>, String>>()?;
    compare_sells(&plan.sells, &sells)?;
    let deferred = list_at(expect, "deferred_sells")?
        .iter()
        .enumerate()
        .map(|(index, sell)| deferred_sell(index, sell))
        .collect::<Result<Vec<_>, String>>()?;
    compare_deferred_sells(&plan.deferred_sells, &deferred)
}

/// The plan's sells against the case's, in order and member by member, so a failure names the one
/// member that differs (`sells[1].pricing`). Each sell is taken apart whole, so a member the crate
/// adds to `FlattenSell` does not compile here until it is compared.
fn compare_sells(got: &[FlattenSell], wanted: &[FlattenSell]) -> Result<(), String> {
    expect_eq("sells", got.len(), wanted.len())?;
    for (index, (sell, want)) in got.iter().zip(wanted).enumerate() {
        let FlattenSell {
            instrument,
            qty,
            pricing,
            floor_price,
            rests_at_floor_then_waits_for_open,
        } = sell;
        let member = |name: &str| format!("sells[{index}].{name}");
        expect_eq(&member("instrument"), instrument, &want.instrument)?;
        expect_eq(&member("qty"), qty, &want.qty)?;
        expect_eq(&member("pricing"), pricing, &want.pricing)?;
        expect_eq(&member("floor_price"), floor_price, &want.floor_price)?;
        expect_eq(
            &member("remainder"),
            rests_at_floor_then_waits_for_open,
            &want.rests_at_floor_then_waits_for_open,
        )?;
    }
    Ok(())
}

/// The plan's deferred sells against the case's, in order and member by member, as
/// [`compare_sells`] does.
fn compare_deferred_sells(got: &[DeferredSell], wanted: &[DeferredSell]) -> Result<(), String> {
    expect_eq("deferred_sells", got.len(), wanted.len())?;
    for (index, (sell, want)) in got.iter().zip(wanted).enumerate() {
        let DeferredSell { instrument, qty } = sell;
        let member = |name: &str| format!("deferred_sells[{index}].{name}");
        expect_eq(&member("instrument"), instrument, &want.instrument)?;
        expect_eq(&member("qty"), qty, &want.qty)?;
    }
    Ok(())
}

/// One expected sell. An absent `floor_price` is no floor and an absent `remainder` is none, so a plan
/// that adds either where the case states neither fails.
fn flatten_sell(sell: &Json) -> Result<FlattenSell, String> {
    unknown_members(sell, SELL_KEYS)
        .map_err(|unknown| format!("sell members not interpreted: {unknown}"))?;
    Ok(FlattenSell {
        instrument: asset(str_at(sell, "instrument")?)?,
        qty: num(Qty::parse(str_at(sell, "qty")?), "sells.qty")?,
        pricing: match str_at(sell, "pricing")? {
            "market_or_ladder" => FlattenPricing::MarketOrLadder,
            "exit_price_ladder" => FlattenPricing::ExitPriceLadder,
            other => return Err(format!("`{other}` is not a flatten pricing")),
        },
        floor_price: match sell.get("floor_price") {
            None => None,
            Some(_) => Some(num(
                Price::parse(str_at(sell, "floor_price")?),
                "sells.floor_price",
            )?),
        },
        rests_at_floor_then_waits_for_open: match sell.get("remainder") {
            None => false,
            Some(_) => match str_at(sell, "remainder")? {
                "rests_at_floor_then_waits_for_open" => true,
                other => return Err(format!("`{other}` is not a remainder the plan can hold")),
            },
        },
    })
}

/// One expected deferred sell. `until` must be the regular-session open, the only deferral a
/// `DeferredSell` means, so a case naming another fails rather than being read as that one.
fn deferred_sell(index: usize, sell: &Json) -> Result<DeferredSell, String> {
    unknown_members(sell, DEFERRED_KEYS)
        .map_err(|unknown| format!("deferred sell members not interpreted: {unknown}"))?;
    expect_eq(
        &format!("deferred_sells[{index}].until"),
        str_at(sell, "until")?,
        "regular_session_open",
    )?;
    Ok(DeferredSell {
        instrument: asset(str_at(sell, "instrument")?)?,
        qty: num(Qty::parse(str_at(sell, "qty")?), "deferred_sells.qty")?,
    })
}

fn session_named(text: &str) -> Result<Session, String> {
    match text {
        "overnight" => Ok(Session::Overnight),
        "pre_market" => Ok(Session::PreMarket),
        "regular" => Ok(Session::Regular),
        "after_hours" => Ok(Session::AfterHours),
        "continuous" => Ok(Session::Continuous),
        other => Err(format!("`{other}` is not a session")),
    }
}

fn mode_named(text: &str) -> Result<AgentMode, String> {
    match text {
        "normal" => Ok(AgentMode::Normal),
        "exits_only" => Ok(AgentMode::ExitsOnly),
        "paused" => Ok(AgentMode::Paused),
        "stopped" => Ok(AgentMode::Stopped),
        other => Err(format!("`{other}` is not an agent mode")),
    }
}

fn asset_class_named(text: &str) -> Result<AssetClass, String> {
    match text {
        "us_equity" => Ok(AssetClass::UsEquity),
        "crypto" => Ok(AssetClass::Crypto),
        other => Err(format!("`{other}` is not an asset class")),
    }
}

/// The one share each harness-built position, working order, and lot size holds where the case
/// states only a dollar figure.
fn one_share() -> Result<Qty, String> {
    num(Qty::parse("1"), "one share")
}

fn asset(text: &str) -> Result<AssetId, String> {
    AssetId::new(text).map_err(|e| format!("`{text}`: {e}"))
}

fn text(value: &Json, what: &str) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("`{what}` holds something that is not a string"))
}

fn object_at<'a>(value: &'a Json, key: &str) -> Result<&'a serde_json::Map<String, Json>, String> {
    at(value, key)?
        .as_object()
        .ok_or_else(|| format!("`{key}` is not an object"))
}

/// A `mandate-risk` refusal to decide, which is never a pass: an owed check names its story.
fn gate_error(what: &str, e: &GateError) -> String {
    format!("`mandate_risk::{what}`: {e} ({})", e.code())
}

/// The hand-copied gate configuration against the fixture it copies, so a regenerated fixture cannot
/// leave it stale.
#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use mandate_canon::DecStr;
    use mandate_num::{Fraction, Usd};
    use mandate_risk::GateConfig;

    use super::super::num;
    use crate::{Json, at, expect_eq, read_fixture, str_at, u64_at};

    /// The members of `configs.test_default.gate` that `GateConfig` does not carry, because another
    /// crate reads them: the data profile, the exit ladder and its step, the stop-limit offset, the
    /// stop watchdog, the protective-replace buffer, the bracket timeout, the unprotected-exposure
    /// bound, and the 403 threshold.
    const NOT_GATE_CONFIG: [&str; 8] = [
        "data_profile",
        "unexplained_403_threshold",
        "crypto_stop_limit_offset",
        "stop_watchdog_s",
        "protective_replace_buffer_trading_days",
        "bracket_partial_fill_timeout_s",
        "max_unprotected_s",
        "exit_ladder",
    ];
    /// The members `GateConfig` reads from the fixture, with the nested ones swept one level down.
    const READ: [&str; 7] = [
        "price_floor",
        "liquidity_floor_usd",
        "collar",
        "opposite_fill_interval_s",
        "order_to_fill",
        "close_window_minutes",
        "legacy_pdt_equity_threshold",
    ];
    const COLLAR: [&str; 5] = [
        "liquid_threshold_usd",
        "liquid_x",
        "other_x",
        "crypto_x",
        "passive_band",
    ];
    const ORDER_TO_FILL: [&str; 2] = ["max", "min_orders"];

    fn gate_fixture() -> Result<Json, String> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/refcases");
        let fixture = read_fixture(&dir, "trading-domain.json").map(Arc::unwrap_or_clone)?;
        at(&fixture, "configs.test_default.gate").cloned()
    }

    fn members_are(object: &Json, what: &str, known: &[&str]) -> Result<(), String> {
        let mut got: Vec<&str> = object
            .as_object()
            .ok_or_else(|| format!("`{what}` is not an object"))?
            .keys()
            .map(String::as_str)
            .collect();
        got.sort_unstable();
        let mut wanted = known.to_vec();
        wanted.sort_unstable();
        expect_eq(&format!("the members of `{what}`"), got, wanted)
    }

    /// Every `GateConfig` field is compared: the twelve the fixture states by value with the
    /// fixture's, and the five it does not state with `mandate-risk`'s `test_default_config` values.
    /// `GateConfig` is taken apart whole, so a field the crate adds does not compile here until it is
    /// compared, and the fixture's members must be exactly the ones read and the ones named in
    /// [`NOT_GATE_CONFIG`], so a member the fixture grows, one of the five included, fails here until
    /// it is placed.
    #[test]
    fn the_gate_configuration_is_the_fixtures_test_default() -> Result<(), String> {
        let gate = gate_fixture()?;
        let top: Vec<&str> = READ.iter().chain(NOT_GATE_CONFIG.iter()).copied().collect();
        members_are(&gate, "configs.test_default.gate", &top)?;
        members_are(at(&gate, "collar")?, "collar", &COLLAR)?;
        members_are(at(&gate, "order_to_fill")?, "order_to_fill", &ORDER_TO_FILL)?;

        let decimal =
            |path: &str| DecStr::parse(str_at(&gate, path)?).map_err(|e| format!("`{path}`: {e}"));
        let usd = |path: &str| num(Usd::parse(decimal(path)?.as_str()), path);
        let fraction = |path: &str| num(Fraction::parse(decimal(path)?.as_str()), path);
        let count = |path: &str| {
            u32::try_from(u64_at(&gate, path)?).map_err(|_| format!("`{path}` does not fit a u32"))
        };
        let GateConfig {
            price_floor,
            liquidity_floor_usd,
            crypto_liquidity_floor_usd,
            collar_liquid_threshold_usd,
            collar_liquid_x,
            collar_other_x,
            collar_crypto_x,
            collar_passive_band,
            opposite_fill_interval_s,
            min_resting_time_s,
            order_to_fill_max,
            order_to_fill_min_orders,
            order_size_participation,
            daily_participation,
            close_window_minutes,
            legacy_pdt_equity_threshold,
            etp_classification_max_age_s,
        } = super::test_default_gate_config()?;

        expect_eq("price_floor", price_floor, usd("price_floor")?)?;
        expect_eq(
            "liquidity_floor_usd",
            liquidity_floor_usd,
            usd("liquidity_floor_usd")?,
        )?;
        expect_eq(
            "collar.liquid_threshold_usd",
            collar_liquid_threshold_usd,
            usd("collar.liquid_threshold_usd")?,
        )?;
        expect_eq(
            "collar.liquid_x",
            collar_liquid_x,
            fraction("collar.liquid_x")?,
        )?;
        expect_eq(
            "collar.other_x",
            collar_other_x,
            fraction("collar.other_x")?,
        )?;
        expect_eq(
            "collar.crypto_x",
            collar_crypto_x,
            fraction("collar.crypto_x")?,
        )?;
        expect_eq(
            "collar.passive_band",
            collar_passive_band,
            fraction("collar.passive_band")?,
        )?;
        expect_eq(
            "opposite_fill_interval_s",
            opposite_fill_interval_s,
            count("opposite_fill_interval_s")?,
        )?;
        expect_eq(
            "order_to_fill.max",
            order_to_fill_max,
            decimal("order_to_fill.max")?
                .as_str()
                .parse::<u32>()
                .map_err(|e| format!("`order_to_fill.max`: {e}"))?,
        )?;
        expect_eq(
            "order_to_fill.min_orders",
            order_to_fill_min_orders,
            count("order_to_fill.min_orders")?,
        )?;
        expect_eq(
            "close_window_minutes",
            close_window_minutes,
            count("close_window_minutes")?,
        )?;
        expect_eq(
            "legacy_pdt_equity_threshold",
            legacy_pdt_equity_threshold,
            usd("legacy_pdt_equity_threshold")?,
        )?;

        let own = |text: &str, what: &str| num(Usd::parse(text), what);
        let own_fraction = |text: &str, what: &str| num(Fraction::parse(text), what);
        expect_eq(
            "crypto_liquidity_floor_usd",
            crypto_liquidity_floor_usd,
            own("1000000", "crypto_liquidity_floor_usd")?,
        )?;
        expect_eq("min_resting_time_s", min_resting_time_s, 2)?;
        expect_eq(
            "order_size_participation",
            order_size_participation,
            own_fraction("0.05", "order_size_participation")?,
        )?;
        expect_eq(
            "daily_participation",
            daily_participation,
            own_fraction("0.05", "daily_participation")?,
        )?;
        expect_eq(
            "etp_classification_max_age_s",
            etp_classification_max_age_s,
            604_800,
        )
    }
}
