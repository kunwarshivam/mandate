//! The `gate` and `agent_flatten` families of
//! [the mandate reference cases](../../../docs/specs/reference-cases/mandate.yaml), **loaded from
//! `fixtures/refcases/mandate.json`** rather than typed out here.
//!
//! Typing a case's figures into a test makes the test agree with whatever the author read, not with
//! the case; the founder owns the fixture, so the fixture is the input. Each case becomes one named
//! test (`MC-G01`), its base document is patched by the case's own RFC 6902 `replace` operations,
//! and its `expect` block is compared whole.
//!
//! When stream F's `mandate` suite lands in `mandate-refcases`, these move there as a `kind: gate`
//! and `kind: agent_flatten` interpretation and this file goes; it lives here meanwhile because
//! that suite does not exist yet and creating it would collide with F's tests PR (the brief's
//! Dependencies).
//!
//! **The purpose is not injected.** DEC-129 item 19 says the `kind: gate` harness treats the case's
//! `purpose` as already assigned. Rather than adding an override the gate would have to trust — the
//! one thing §9.1 forbids — the harness picks an `Origin` that yields that purpose and gives the
//! agent a position at least the size of a sell, which is what a case stating an exit implies.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{Scenario, asset, at, fraction, price, qty, usd};
use mandate_risk::spec_types::{GoalState, RiskLimits, Rung, RungAction, ScaleAction};
use mandate_risk::{
    AgentId, AgentMode, AgentPosition, AssetClass, AssetId, Check, CheckOutcome, ClientOrderId,
    FlattenInitiator, FlattenInput, FlattenPricing, Origin, Purpose, ReasonCode, Session, Side,
    ValidatedMandate, Verdict, WorkingUniverse, agent_flatten, evaluate,
};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json"))
        .unwrap_or_else(|e| panic!("the reference-case fixture parses: {e}"))
}

fn case(id: &str) -> Value {
    let f = fixture();
    let cases = f
        .get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the fixture lists cases"));
    cases
        .iter()
        .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("{id} is in the fixture"))
        .clone()
}

fn s(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("{key} is a string in {v}"))
        .to_owned()
}

/// The case's base document with its own `replace` patches applied. Only `/risk/...` paths appear
/// in the G family; anything else is a fixture change this harness has not been taught, and it
/// fails loudly rather than ignoring the patch (DEC-85).
fn patched_risk(c: &Value) -> Value {
    let f = fixture();
    let base = s(c, "base");
    let mut risk = f
        .pointer(&format!("/bases/{base}/mandate/risk"))
        .unwrap_or_else(|| panic!("base {base} has a risk block"))
        .clone();
    for p in c
        .get("patch")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let op = s(p, "op");
        let path = s(p, "path");
        assert_eq!(
            op, "replace",
            "only `replace` is interpreted; {path} uses {op}"
        );
        let field = path
            .strip_prefix("/risk/")
            .unwrap_or_else(|| panic!("only /risk/... patches are interpreted, not {path}"));
        let value = p
            .get("value")
            .unwrap_or_else(|| panic!("{path} has a value"));
        risk[field] = value.clone();
    }
    risk
}

/// The named base's own `sizing.rebalance_band`, not a constant typed here: the value belongs to
/// the fixture, and a hardcoded copy silently keeps its old reading when the founder changes the
/// base. The base name is the case's own `base` field, threaded in rather than looked for inside
/// the risk block, which carries only the fields the fixture puts there.
fn sizing_rebalance_band(base: &str) -> mandate_num::Fraction {
    let f = fixture();
    let from_base = f
        .pointer(&format!(
            "/bases/{base}/mandate/behavior/sizing/rebalance_band"
        ))
        .and_then(Value::as_str);
    fraction(from_base.unwrap_or_else(|| {
        panic!(
            "base {base} states behavior.sizing.rebalance_band; this harness does not invent one"
        )
    }))
}

fn limits_from(base: &str, risk: &Value) -> RiskLimits {
    let ladder = risk
        .get("drawdown_ladder")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the risk block has a ladder"))
        .iter()
        .enumerate()
        .map(|(i, r)| Rung {
            index: u8::try_from(i).unwrap_or(u8::MAX),
            at: fraction(&s(r, "at")),
            action: match s(r, "action").as_str() {
                "scale_sizes" => RungAction::ScaleSizes,
                "exits_only" => RungAction::ExitsOnly,
                "flatten_and_pause" => RungAction::FlattenAndPause,
                other => panic!("unknown rung action {other}"),
            },
            factor: r.get("factor").and_then(Value::as_str).map(fraction),
            scale_action: match risk.get("scale_action").and_then(Value::as_str) {
                Some("limit_buys") => Some(ScaleAction::LimitBuys),
                Some("trim_to_target") => Some(ScaleAction::TrimToTarget),
                _ => None,
            },
        })
        .collect();
    RiskLimits {
        max_position_usd: usd(&s(risk, "max_position_usd")),
        max_position_fraction: fraction(&s(risk, "max_position_fraction")),
        max_order_usd: usd(&s(risk, "max_order_usd")),
        max_gross_exposure_usd: usd(&s(risk, "max_gross_exposure_usd")),
        max_orders_per_day: u32::try_from(
            risk.get("max_orders_per_day")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| panic!("max_orders_per_day is a number")),
        )
        .unwrap_or(u32::MAX),
        reentry_cooldown_s: u32::try_from(
            risk.get("reentry_cooldown_s")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| panic!("reentry_cooldown_s is a number")),
        )
        .unwrap_or(u32::MAX),
        rebalance_band: sizing_rebalance_band(base),
        breach_confirm_s: u32::try_from(
            risk.get("breach_confirm_s")
                .and_then(Value::as_u64)
                .unwrap_or_else(|| panic!("breach_confirm_s is a number")),
        )
        .unwrap_or(u32::MAX),
        drawdown_ladder: ladder,
    }
}

fn origin_for(purpose: &str) -> Origin {
    match purpose {
        "open" | "increase" => Origin::OrderBuilder,
        "discretionary_exit" => Origin::OrderBuilder,
        "risk_exit" => Origin::RiskEngine,
        "owner_exit" => Origin::OwnerClose,
        "protective" => Origin::ProtectiveLeg,
        other => panic!("unknown purpose {other}"),
    }
}

fn expected_purpose(purpose: &str) -> Purpose {
    match purpose {
        "open" => Purpose::Open,
        "increase" => Purpose::Increase,
        "discretionary_exit" => Purpose::DiscretionaryExit,
        "risk_exit" => Purpose::RiskExit,
        "owner_exit" => Purpose::OwnerExit,
        "protective" => Purpose::Protective,
        other => panic!("unknown purpose {other}"),
    }
}

/// Everything the harness reads out of a `kind: gate` case, built without calling the gate.
struct GateCase {
    scenario: Scenario,
    case: Value,
    purpose_text: String,
}

/// Reads one `kind: gate` case into a [`Scenario`], touching nothing in the crate but its types.
///
/// This is split from [`run_gate`] because a pending test that panics in its own setup fails
/// exactly as a pending test must, so `cargo xtask ci pending` cannot tell a wired-up test from a
/// broken harness. [`every_gate_case_builds_its_scenario_without_the_gate`] runs this half for
/// every case and is not pending, so a harness that stops reading the fixture fails loudly today
/// rather than passing as evidence.
fn gate_scenario(id: &str) -> GateCase {
    let c = case(id);
    let risk_block = patched_risk(&c);
    let state = c.get("state").unwrap_or_else(|| panic!("{id} has a state"));
    let prop = c
        .get("proposed")
        .unwrap_or_else(|| panic!("{id} has a proposal"));
    c.get("expect")
        .unwrap_or_else(|| panic!("{id} has an expectation"));

    let purpose_text = s(prop, "purpose");
    let selling = matches!(
        purpose_text.as_str(),
        "discretionary_exit" | "risk_exit" | "owner_exit"
    );
    let instrument = asset(&s(prop, "instrument"));
    let order_qty = qty(&s(prop, "qty"));

    let mut sc = Scenario::allowing();
    sc.mandate = ValidatedMandate::from_validated_parts(
        limits_from(&s(&c, "base"), &risk_block),
        GoalState::Running,
        false,
        false,
    );
    sc.now = at(&s(state, "now"));
    sc.risk = common::healthy_risk(&s(state, "agent_equity"));
    sc.account.equity = usd(&s(state, "agent_equity"));
    sc.instrument = common::equity_instrument(instrument.as_str());

    sc.agent.market_values.clear();
    sc.agent.positions.clear();
    for (k, v) in state
        .get("positions_mv")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let held = asset(k);
        sc.agent.market_values.insert(
            held.clone(),
            usd(v
                .as_str()
                .unwrap_or_else(|| panic!("a market value is text"))),
        );
        sc.agent.positions.insert(held, qty("1"));
    }
    if selling {
        sc.agent.positions.insert(instrument.clone(), order_qty);
    }

    sc.account.working_orders.clear();
    for (i, w) in state
        .get("working_opening_orders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let id_n = u64::try_from(i).unwrap_or(0);
        let mut o = common::open_order(AgentId(1), &s(w, "instrument"), &s(w, "max_cost"));
        o.agent = AgentId(1);
        sc.account.working_orders.insert(ClientOrderId(id_n), o);
        sc.agent.working_orders.insert(ClientOrderId(id_n));
    }

    sc.agent.orders_today = u32::try_from(
        state
            .get("orders_today")
            .and_then(Value::as_u64)
            .unwrap_or(0),
    )
    .unwrap_or(0);

    sc.agent.last_exit_fill_at.clear();
    for (k, v) in state
        .get("last_exit_fill_at")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        sc.agent.last_exit_fill_at.insert(
            asset(k),
            at(v.as_str().unwrap_or_else(|| panic!("an instant is text"))),
        );
    }

    sc.agent.instrument_groups.clear();
    let mut group_ids: BTreeMap<String, u64> = BTreeMap::new();
    for (k, v) in state
        .get("instrument_groups")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let name = v.as_str().unwrap_or("").to_owned();
        let next = u64::try_from(group_ids.len()).unwrap_or(0);
        let gid = *group_ids.entry(name).or_insert(next);
        sc.agent
            .instrument_groups
            .insert(asset(k), mandate_risk::GroupId(gid));
    }

    let universe: BTreeSet<AssetId> = state
        .get("working_universe")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("{id} states a working universe"))
        .iter()
        .map(|v| {
            asset(
                v.as_str()
                    .unwrap_or_else(|| panic!("an instrument id is text")),
            )
        })
        .collect();
    sc.universe = WorkingUniverse::Known {
        instruments: universe,
        pinned: true,
    };

    sc.proposed = common::proposal(
        instrument.as_str(),
        if selling { Side::Sell } else { Side::Buy },
        &s(prop, "qty"),
        &s(prop, "limit_price"),
        origin_for(&purpose_text),
    );

    GateCase {
        scenario: sc,
        case: c,
        purpose_text,
    }
}

/// The `kind: gate` cases whose `expect` the full gate cannot give, each with the one check outside
/// `ref.py`'s `gate` subset that decides it and the `computed` keys it stops the gate before
/// reaching (DEC-150 item 1, amending DEC-129 item 19; the MC-G02 finding on claim #123).
///
/// DEC-129 item 19 says a `kind: gate` case pins `ref.py`'s `gate`: the working universe,
/// concentration, order size, the cooldown, orders per day and gross exposure. This harness drives
/// the full `evaluate` instead, which is stricter than a case whenever a check outside that subset
/// fails. `MC-G02` proposes an increase in instrument 2 beside a working opening order in
/// instrument 2, so trading-domain §5.3 rule 6 (at most one working non-protective order per
/// instrument per account, check 4) denies it `working_order_limit` before checks 6 and 7 compute
/// its gross exposure. For a listed case the harness asserts that checks 1 and 2 pass, that the
/// named check is the only failure and decides the verdict, that the `computed` figures the gate
/// reached match the case, and that the listed keys were never reached. What the entry gives up is
/// `MC-G02`'s own `gross` and `gross_limit`. `MC-G04` and `MC-G06` pin gross exposure at check 7
/// only on a denial, and `MC-G13`, the other allow case that states `gross`, stays pending on
/// E6-8's stub, so until E6-8 lands no live reference case pins `gross` on an allow path (DEC-150
/// item 1; `gate.rs`'s in-module tests hold it meanwhile). A listed
/// case whose full verdict already matches its `expect` fails, so no entry outlives its reason, and
/// every case is named one by one: a rule such as "skip the verdict when check 2 passes" would also
/// exempt `MC-G04`, `MC-G06` and `MC-G07`, whose own denials sit at checks 6 and 7.
const FULL_GATE_ONLY: [(&str, Check, ReasonCode, &[&str]); 1] = [(
    "MC-G02",
    Check::OrderConstraints,
    ReasonCode::WorkingOrderLimit,
    &["gross", "gross_limit"],
)];

/// Drives one `kind: gate` case end to end and compares its whole `expect` block.
fn run_gate(id: &str) {
    let GateCase {
        scenario: sc,
        case: c,
        purpose_text,
    } = gate_scenario(id);
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} has an expectation"));

    let d = evaluate(&sc.input()).unwrap_or_else(|e| panic!("{id}: the gate decides, not {e}"));

    let want_verdict = match s(expect, "verdict").as_str() {
        "allow" => Verdict::Allow,
        "deny" => Verdict::Deny,
        "defer" => Verdict::Defer,
        other => panic!("unknown verdict {other}"),
    };
    let want_reason = expect
        .get("reason")
        .and_then(Value::as_str)
        .map(|r| reason_code(r, id));
    if let Some(&(_, check, code, unreached)) = FULL_GATE_ONLY.iter().find(|entry| entry.0 == id) {
        assert_ne!(
            (d.verdict, d.reason),
            (want_verdict, want_reason),
            "{id}: the full gate now gives the case's own verdict, so its FULL_GATE_ONLY entry has \
             expired; delete it"
        );
        assert_eq!(
            (d.verdict, d.reason, d.purpose),
            (Verdict::Deny, Some(code), expected_purpose(&purpose_text)),
            "{id}: only the check FULL_GATE_ONLY names may decide it"
        );
        assert_eq!(
            d.checks.get(..2),
            Some(
                &[
                    CheckOutcome::Passed(Check::AccountAndMode),
                    CheckOutcome::Passed(Check::UniverseAndLimits),
                ][..]
            ),
            "{id}: checks 1 and 2, where the case's own limits sit, pass"
        );
        let failed: Vec<&CheckOutcome> = d
            .checks
            .iter()
            .filter(|c| matches!(c, CheckOutcome::Failed(..)))
            .collect();
        assert_eq!(
            failed,
            [&CheckOutcome::Failed(check, code)],
            "{id}: the named check is the only failure"
        );
        compare_computed(id, expect, &d, unreached);
        return;
    }
    assert_eq!(
        (d.verdict, d.reason),
        (want_verdict, want_reason),
        "{id}: {}",
        c.get("title").and_then(Value::as_str).unwrap_or("")
    );
    assert_eq!(
        d.purpose,
        expected_purpose(&purpose_text),
        "{id}: the gate assigns the purpose the case states"
    );

    compare_computed(id, expect, &d, &[]);
}

/// The `computed` block, which 13 of the 16 G cases carry and which is the case's own arithmetic.
///
/// Comparing the verdict alone would let a gate reach the right answer from the wrong figures — the
/// exact failure mode `MC-G05` exists to catch, where the cap binds at 1425 rather than 1500. Every
/// key the case states is compared; a key the harness cannot yet produce fails as "not interpreted
/// until <story>" rather than being skipped (DEC-85).
///
/// A key in `unreached` is one the deciding check stops the gate before computing
/// ([`FULL_GATE_ONLY`]): it is asserted absent rather than compared, so a gate that computed it
/// anyway, or computed it wrongly, is still caught.
fn compare_computed(id: &str, expect: &Value, d: &mandate_risk::Decision, unreached: &[&str]) {
    let Some(computed) = expect.get("computed").and_then(Value::as_object) else {
        return;
    };
    for (key, want) in computed {
        if unreached.contains(&key.as_str()) {
            assert_eq!(
                d.computed.get(key.as_str()),
                None,
                "{id}: computed.{key} is after the deciding check, so the gate never reached it"
            );
            continue;
        }
        let want_text = match want {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.to_string(),
            other => panic!("{id}: computed.{key} is text or a number, not {other}"),
        };
        let got = d.computed.get(key.as_str()).unwrap_or_else(|| {
            panic!(
                "{id}: the case states computed.{key} = {want_text}, which this gate does not \
                 report; a figure the case pins is not optional"
            )
        });
        assert_eq!(got, want_text, "{id}: computed.{key}");
    }
}

/// Drives one `kind: agent_flatten` case and compares its whole plan.
fn run_flatten(id: &str) {
    let c = case(id);
    let input = c
        .get("input")
        .unwrap_or_else(|| panic!("{id} has an input"));
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} has an expectation"));
    let agent_name = s(input, "agent");

    let mut agents: BTreeMap<String, u64> = BTreeMap::new();
    let mut agent_id = |n: &str| {
        let next = u64::try_from(agents.len()).unwrap_or(0);
        AgentId(*agents.entry(n.to_owned()).or_insert(next))
    };
    let me = agent_id(&agent_name);

    let mut orders = BTreeMap::new();
    let mut order_names: Vec<String> = Vec::new();
    for (i, o) in input
        .get("open_orders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .enumerate()
    {
        let name = s(o, "client_order_id");
        order_names.push(name.clone());
        let mut w = common::open_order(agent_id(&s(o, "agent")), &s(o, "instrument"), "0");
        w.agent = agent_id(&s(o, "agent"));
        orders.insert(ClientOrderId(u64::try_from(i).unwrap_or(0)), w);
    }

    let positions: Vec<AgentPosition> = input
        .get("agent_positions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| AgentPosition {
            agent: agent_id(&s(p, "agent")),
            instrument: asset(&s(p, "instrument")),
            asset_class: match s(p, "asset_class").as_str() {
                "us_equity" => AssetClass::UsEquity,
                "crypto" => AssetClass::Crypto,
                other => panic!("unknown asset class {other}"),
            },
            qty: qty(&s(p, "qty")),
        })
        .collect();

    let broker: BTreeMap<AssetId, mandate_num::Qty> = input
        .get("broker_positions")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|p| (asset(&s(p, "instrument")), qty(&s(p, "qty"))))
        .collect();

    let owner_confirmed = input
        .get("owner_confirmed_bid")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let plan = agent_flatten(&FlattenInput {
        agent: me,
        open_orders: &orders,
        agent_positions: &positions,
        broker_positions: &broker,
        session: match s(input, "session").as_str() {
            "regular" => Session::Regular,
            "after_hours" => Session::AfterHours,
            "pre_market" => Session::PreMarket,
            "overnight" => Session::Overnight,
            other => panic!("unknown session {other}"),
        },
        initiator: match s(input, "initiator").as_str() {
            "owner" => FlattenInitiator::Owner,
            _ => FlattenInitiator::RiskLimit,
        },
        owner_confirmed_bid: if owner_confirmed {
            Some(price(&s(input, "confirmed_bid")))
        } else {
            None
        },
        max_exit_offset: input
            .get("max_exit_offset")
            .and_then(Value::as_str)
            .map_or_else(|| fraction("0.03"), fraction),
        owner_floor_price: input
            .get("owner_floor_price")
            .and_then(Value::as_str)
            .map(price),
    })
    .unwrap_or_else(|e| panic!("{id}: the flatten plans, not {e}"));

    assert_eq!(
        plan.mode_applied_first,
        match s(expect, "mode_applied_first").as_str() {
            "paused" => AgentMode::Paused,
            "stopped" => AgentMode::Stopped,
            other => panic!("unknown mode {other}"),
        },
        "{id}: the final mode is applied first"
    );
    assert_eq!(
        plan.purpose,
        expected_purpose(&s(expect, "purpose")),
        "{id}: the flatten's purpose"
    );
    assert!(
        !plan.cancel_all_endpoint && !plan.close_position_endpoint,
        "{id}: an agent-scoped flatten never uses the account-wide endpoints"
    );

    let want_cancels: Vec<ClientOrderId> =
        expect
            .get("cancel_client_order_ids")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|v| {
                let name = v.as_str().unwrap_or("");
                ClientOrderId(
                    u64::try_from(order_names.iter().position(|n| n == name).unwrap_or_else(
                        || panic!("{id} cancels an order it did not open: {name}"),
                    ))
                    .unwrap_or(0),
                )
            })
            .collect();
    assert_eq!(
        plan.cancel_client_order_ids, want_cancels,
        "{id}: exactly this agent's orders, sorted"
    );

    let want_sells = expect
        .get("sells")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let want_deferred = expect
        .get("deferred_sells")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    assert_eq!(
        (plan.sells.len(), plan.deferred_sells.len()),
        (want_sells, want_deferred),
        "{id}: the sells and the deferred sells"
    );

    for (got, want) in plan.deferred_sells.iter().zip(
        expect
            .get("deferred_sells")
            .and_then(Value::as_array)
            .into_iter()
            .flatten(),
    ) {
        assert_eq!(
            (got.instrument.as_str(), got.qty),
            (s(want, "instrument").as_str(), qty(&s(want, "qty"))),
            "{id}: a deferred sell is the agent's sub-ledger quantity"
        );
        assert_eq!(
            s(want, "until"),
            "regular_session_open",
            "{id}: the only deferral this harness interprets is to the regular-session open; \
             anything else is not interpreted until the story that adds it"
        );
    }

    for (got, want) in plan.sells.iter().zip(
        expect
            .get("sells")
            .and_then(Value::as_array)
            .into_iter()
            .flatten(),
    ) {
        assert_eq!(
            (got.instrument.as_str(), got.qty),
            (s(want, "instrument").as_str(), qty(&s(want, "qty"))),
            "{id}: a sell is the agent's sub-ledger quantity"
        );
        assert_eq!(
            got.pricing,
            match s(want, "pricing").as_str() {
                "market_or_ladder" => FlattenPricing::MarketOrLadder,
                "exit_price_ladder" => FlattenPricing::ExitPriceLadder,
                other => panic!("unknown pricing {other}"),
            },
            "{id}: a flatten prices every sell by the session alone"
        );
        assert_eq!(
            got.floor_price,
            want.get("floor_price").and_then(Value::as_str).map(price),
            "{id}: the owner's floor price"
        );
        if let Some(remainder) = want.get("remainder").and_then(Value::as_str) {
            assert_eq!(
                remainder, "rests_at_floor_then_waits_for_open",
                "{id}: the only remainder this harness interprets"
            );
            assert!(
                got.rests_at_floor_then_waits_for_open,
                "{id}: the case states the remainder rests at the floor and waits for the open"
            );
        } else {
            assert!(
                !got.rests_at_floor_then_waits_for_open,
                "{id}: no remainder is stated, so none is planned"
            );
        }
    }

    for (instrument, broker_qty) in &broker {
        let sold: Option<mandate_num::Qty> = plan
            .sells
            .iter()
            .find(|sell| &sell.instrument == instrument)
            .map(|sell| sell.qty);
        if let Some(sold) = sold {
            assert!(
                sold <= *broker_qty,
                "{id}: the plan sells the agent's sub-ledger, never the broker's {broker_qty}"
            );
        }
    }
}

/// Resolves a case's reason text through [`ReasonCode`]'s own registered spellings.
///
/// An enumerated subset here is a trap: a case naming a code the list forgot panics as
/// "unregistered" on a **correct** gate, which is how `MC-G07`'s `max_orders_per_day` would have
/// failed. Searching every variant means the only way to fail is a code the crate genuinely cannot
/// emit.
fn reason_code(text: &str, case_id: &str) -> ReasonCode {
    ReasonCode::ALL
        .iter()
        .find(|c| c.as_str() == text)
        .copied()
        .unwrap_or_else(|| panic!("{case_id} names {text}, which no ReasonCode variant spells"))
}

/// `MC-G01`: Per-instrument cap exceeded (current + working + proposed).
///
/// This case and the nineteen below it are plain functions rather than a macro expansion, because
/// `cargo xtask ci pending` scans the source for `fn <name>()` beside an `#[ignore = "pending …"]`
/// literal: a macro-generated test is invisible to it, so review round 5 found these twenty
/// ungated, and one of them passing on the stubs would not have failed CI. Written out, every one
/// is gated like the rest; stream J did the same in #141. The scan's blind spot itself is in the
/// tracker's known issues, for a separate xtask change.
#[test]
fn mc_g01() {
    run_gate("MC-G01");
}

/// `MC-G02`: Per-instrument cap met exactly.
#[test]
fn mc_g02() {
    run_gate("MC-G02");
}

/// `MC-G03`: Order size above max_order_usd.
#[test]
fn mc_g03() {
    run_gate("MC-G03");
}

/// `MC-G04`: Agent gross exposure exceeded (working order in another instrument counts).
#[test]
fn mc_g04() {
    run_gate("MC-G04");
}

/// `MC-G05`: Fraction cap binds when equity falls.
#[test]
fn mc_g05() {
    run_gate("MC-G05");
}

/// `MC-G06`: Equity below max_gross_exposure_usd caps gross exposure.
#[test]
fn mc_g06() {
    run_gate("MC-G06");
}

/// `MC-G07`: Orders per day reached.
#[test]
fn mc_g07() {
    run_gate("MC-G07");
}

/// `MC-G08`: Exit is allowed when orders per day is at the limit.
#[test]
fn mc_g08() {
    run_gate("MC-G08");
}

/// `MC-G09`: Risk exit larger than max_order_usd is allowed.
#[test]
fn mc_g09() {
    run_gate("MC-G09");
}

/// `MC-G10`: Owner exit is allowed by every mandate limit.
#[test]
fn mc_g10() {
    run_gate("MC-G10");
}

/// `MC-G11`: Re-entry cooldown after an exit fill.
#[test]
fn mc_g11() {
    run_gate("MC-G11");
}

/// `MC-G12`: Re-entry cooldown applies across the instrument group.
#[test]
fn mc_g12() {
    run_gate("MC-G12");
}

/// `MC-G13`: Re-entry allowed once the cooldown has passed.
#[test]
fn mc_g13() {
    run_gate("MC-G13");
}

/// `MC-G14`: Opening an instrument outside the working universe.
#[test]
fn mc_g14() {
    run_gate("MC-G14");
}

/// `MC-G15`: Exiting an instrument outside the working universe is allowed.
#[test]
fn mc_g15() {
    run_gate("MC-G15");
}

/// `MC-G16`: An empty working universe denies every opening.
#[test]
fn mc_g16() {
    run_gate("MC-G16");
}

/// `MC-F01`: Automated flatten on a shared account touches only that agent.
#[test]
fn mc_f01() {
    run_flatten("MC-F01");
}

/// `MC-F02`: Automated flatten after hours: equity sells wait; crypto sells go now.
#[test]
fn mc_f02() {
    run_flatten("MC-F02");
}

/// `MC-F03`: Owner kill switch after hours with the bid confirmed: sells now, never below the floor price.
#[test]
fn mc_f03() {
    run_flatten("MC-F03");
}

/// `MC-F04`: Owner kill switch after hours without confirmation: equity sells wait.
#[test]
fn mc_f04() {
    run_flatten("MC-F04");
}

/// The harness passes a case's `purpose` through rather than re-deriving it (DEC-129 item 19).
///
/// `MC-G15` is the case that makes the difference: a `discretionary_exit` of 1 in an instrument the
/// state gives no `positions_mv` entry. A harness that re-derived the purpose from side and
/// position would call it a zero crossing and deny; the case expects an allow.
#[test]
fn a_gate_case_purpose_is_passed_through() {
    run_gate("MC-G15");
}

/// Every gate case's scenario is built from the fixture, without calling the gate. Not pending.
///
/// This exists because review round 4 found the harness panicking in its own setup — it looked for
/// the base's `rebalance_band` under a key nothing set, and at the wrong path — so all sixteen
/// `MC-G` tests plus `a_gate_case_purpose_is_passed_through` failed before reaching `evaluate`.
/// They would have failed on a correct gate too, and `cargo xtask ci pending` cannot tell a
/// pending test that stops at the stub from one that stops at a broken harness: both just fail.
/// Running the reading half here, not pending, makes that failure loud today.
#[test]
fn every_gate_case_builds_its_scenario_without_the_gate() {
    for n in 1..=16 {
        let id = format!("MC-G{n:02}");
        let built = gate_scenario(&id);
        assert_eq!(
            s(&built.case, "id"),
            id,
            "the scenario is built from the case it names"
        );
        expected_purpose(&built.purpose_text);
        assert!(
            built.scenario.mandate.risk().rebalance_band > mandate_num::Fraction::ZERO,
            "{id}: the rebalance band comes from the base, not from a default"
        );
    }
}

/// The fixture really does carry the twenty cases this file drives, so a renamed or removed case
/// fails here rather than silently reducing the suite. Not pending: it reads the fixture only.
#[test]
fn the_fixture_carries_every_case_this_suite_drives() {
    let f = fixture();
    let ids: BTreeSet<String> = f
        .get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the fixture lists cases"))
        .iter()
        .filter(|c| {
            matches!(
                c.get("kind").and_then(Value::as_str),
                Some("gate" | "agent_flatten")
            )
        })
        .filter_map(|c| c.get("id").and_then(Value::as_str).map(str::to_owned))
        .collect();
    let driven: BTreeSet<String> = (1..=16)
        .map(|n| format!("MC-G{n:02}"))
        .chain((1..=4).map(|n| format!("MC-F{n:02}")))
        .collect();
    assert_eq!(
        ids, driven,
        "every gate and agent_flatten case in the fixture has a test here, and no test names a \
         case the fixture does not carry"
    );
}
