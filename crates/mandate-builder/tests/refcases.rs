//! The `autonomy` (`MC-A`) and `builder` (`MC-B`) families of
//! [the mandate reference cases](../../../docs/specs/reference-cases/mandate.yaml), **loaded from
//! `fixtures/refcases/mandate.json`** rather than typed out here.
//!
//! Typing a case's figures into a test makes the test agree with whatever the author read, not with
//! the case; the founder owns the fixture, so the fixture is the input. Each case is one named test
//! reading **every** member of its `expect` block, and an `expect` member this harness has not been
//! taught fails the case loudly rather than going unchecked (DEC-85).
//!
//! **The gate's verdict is an input, not an output.** Every `B` case states a `gate_state` and
//! expects a `gate_dry_run` verdict, which is `mandate-risk`'s (DEC-130 items 2 and 15). This
//! harness therefore reads the case's own expected verdict and hands it to [`decide`] as the value
//! §6.2 step 2 says it is, then checks the `autonomy` block the case expects. When stream G's gate
//! lands, the harness-and-status PR composes propose → gate → `decide` and moves these cases in
//! `crates/mandate-refcases/status.toml`; nothing in that file changes here.
//!
//! **Four `B` cases are not here.** `MC-B17` and `MC-B30` to `MC-B32` assert the §5.5
//! `trim_to_target` risk exit and its four guards, which turn on ladder state the builder does not
//! hold; they are stream G's (DEC-130 item 3).
//!
//! Every case is pending until the implementation PR and fails on `BuilderError::Unimplemented`.

mod common;

use std::collections::BTreeSet;

use common::{asset, basis, dec, digest, fee, frac, mark, price, qty, rule_id, signed, unit, usd};
use mandate_builder::{
    AccountSnapshot, AccumulateGoal, Action, ActionContext, BuilderMandate, Direction, GateVerdict,
    GoalKind, Limits, Market, ModelOutput, Outcome, RiskContext, SignalModel, Sizes, Sizing,
    classify, decide, propose,
};
use mandate_domain::{AssetClass, AutonomyDecision, MarketSession, Purpose};
use mandate_num::{Usd, UsdExact};
use mandate_spec::DecGrammar;
use mandate_spec::condition::{Condition, ConditionField, ConditionValue, Operator};
use mandate_spec::document::{Approval, Autonomy, ModelId, OnTimeout, Rule, SizingMethod};
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json"))
        .unwrap_or_else(|e| panic!("the reference-case fixture parses: {e}"))
}

fn case(id: &str) -> Value {
    let f = fixture();
    f.get("cases")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the fixture lists cases"))
        .iter()
        .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("{id} is in the fixture"))
        .clone()
}

fn text_at(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("`{key}` is a string in {value}"))
        .to_owned()
}

/// The base document's block at `path`, with the case's own `replace` patches applied to it.
/// A patch outside `/autonomy/` is a fixture change this harness has not been taught, and it says
/// so rather than ignoring the patch.
fn patched(c: &Value, block: &str) -> Value {
    let f = fixture();
    let base = text_at(c, "base");
    let mut value = f
        .pointer(&format!("/bases/{base}/mandate/{block}"))
        .unwrap_or_else(|| panic!("base {base} has a {block} block"))
        .clone();
    for patch in c
        .get("patch")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let op = text_at(patch, "op");
        let path = text_at(patch, "path");
        assert_eq!(
            op, "replace",
            "only `replace` is interpreted; {path} uses {op}"
        );
        let prefix = format!("/{block}/");
        let Some(rest) = path.strip_prefix(&prefix) else {
            assert!(
                path.starts_with("/autonomy/") || path.starts_with(&format!("/{block}")),
                "this harness interprets /autonomy/... patches, not {path}"
            );
            continue;
        };
        let new = patch
            .get("value")
            .unwrap_or_else(|| panic!("{path} has a value"));
        let mut cursor = &mut value;
        let members: Vec<&str> = rest.split('/').collect();
        for member in &members[..members.len().saturating_sub(1)] {
            cursor = cursor
                .get_mut(*member)
                .unwrap_or_else(|| panic!("{path} names a member of the base"));
        }
        let last = members
            .last()
            .unwrap_or_else(|| panic!("{path} names a member"));
        cursor[*last] = new.clone();
    }
    value
}

fn condition_of(value: &Value) -> Condition {
    if let Some(children) = value.get("all").and_then(Value::as_array) {
        return Condition::All(children.iter().map(condition_of).collect());
    }
    if let Some(children) = value.get("any").and_then(Value::as_array) {
        return Condition::Any(children.iter().map(condition_of).collect());
    }
    if let Some(child) = value.get("not") {
        return Condition::Not(Box::new(condition_of(child)));
    }
    let field = field_of(&text_at(value, "field"));
    let op = operator_of(&text_at(value, "op"));
    let raw = value
        .get("value")
        .unwrap_or_else(|| panic!("a comparison has a value: {value}"));
    let condition_value = match raw {
        Value::Bool(flag) => ConditionValue::Bool(*flag),
        Value::String(member) => match field.kind() {
            mandate_spec::condition::FieldKind::Decimal => {
                ConditionValue::Decimal(dec(member, DecGrammar::Decimal))
            }
            _ => ConditionValue::Text(member.clone()),
        },
        Value::Array(members) => ConditionValue::List(
            members
                .iter()
                .map(|m| {
                    m.as_str()
                        .unwrap_or_else(|| panic!("a list member is text: {m}"))
                        .to_owned()
                })
                .collect(),
        ),
        other => panic!("a condition value is a string, a boolean, or a list, not {other}"),
    };
    Condition::Compare {
        field,
        op,
        value: condition_value,
    }
}

fn field_of(name: &str) -> ConditionField {
    for field in [
        ConditionField::Purpose,
        ConditionField::OrderUsd,
        ConditionField::CombinedScore,
        ConditionField::Instrument,
        ConditionField::AssetClass,
        ConditionField::Session,
        ConditionField::FirstTradeInInstrument,
        ConditionField::NewInstrument,
        ConditionField::ThesisConfidence,
        ConditionField::Drawdown,
        ConditionField::DailyPnlFraction,
        ConditionField::PositionUsdAfter,
        ConditionField::GrossUsdAfter,
        ConditionField::BoughtTodayUsd,
        ConditionField::PositionPnlFraction,
        ConditionField::UnusualInput,
    ] {
        if field.as_str() == name {
            return field;
        }
    }
    panic!("`{name}` is a §6.3 condition field")
}

fn operator_of(name: &str) -> Operator {
    for op in [
        Operator::Eq,
        Operator::Ne,
        Operator::Gt,
        Operator::Gte,
        Operator::Lt,
        Operator::Lte,
        Operator::In,
        Operator::NotIn,
    ] {
        if op.as_str() == name {
            return op;
        }
    }
    panic!("`{name}` is a §6.3 operator")
}

fn decision_of(name: &str) -> AutonomyDecision {
    match name {
        "auto" => AutonomyDecision::Auto,
        "ask" => AutonomyDecision::Ask,
        "deny" => AutonomyDecision::Deny,
        other => panic!("`{other}` is an autonomy decision"),
    }
}

fn autonomy_of(c: &Value) -> Autonomy {
    let block = patched(c, "autonomy");
    let rules = block
        .get("rules")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the autonomy block lists rules"))
        .iter()
        .map(|r| Rule {
            id: rule_id(&text_at(r, "id")),
            when: condition_of(
                r.get("when")
                    .unwrap_or_else(|| panic!("a rule has a `when`")),
            ),
            then: decision_of(&text_at(r, "then")),
        })
        .collect();
    let approval = block
        .get("approval")
        .unwrap_or_else(|| panic!("the autonomy block has an approval"));
    Autonomy {
        rules,
        default: decision_of(&text_at(&block, "default")),
        admission: decision_of(&text_at(&block, "admission")),
        approval: Approval {
            timeout_s: u32::try_from(
                approval
                    .get("timeout_s")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(|| panic!("`timeout_s` is an integer")),
            )
            .unwrap_or_else(|e| panic!("`timeout_s` fits: {e}")),
            on_timeout: {
                assert_eq!(
                    text_at(approval, "on_timeout"),
                    "skip",
                    "§6.4 allows one value"
                );
                OnTimeout::Skip
            },
            approvers: approval
                .get("approvers")
                .and_then(Value::as_array)
                .unwrap_or_else(|| panic!("`approvers` is a list"))
                .iter()
                .map(|a| {
                    common::approver(
                        a.as_str()
                            .unwrap_or_else(|| panic!("an approver is text: {a}")),
                    )
                })
                .collect(),
            two_approver_above_usd: approval
                .get("two_approver_above_usd")
                .and_then(Value::as_str)
                .map(|t| dec(t, DecGrammar::PositiveDecimal)),
        },
    }
}

/// The `action` block a `kind: autonomy` case states. `MC-A01` to `MC-A04` give a purpose and
/// nothing else, because a reducing purpose is decided before any field is read; the members they
/// omit are set to values no rule in any base matches, and the case asserts `builtin_risk_reducing`,
/// so a rule reaching them would change the answer and fail the case.
fn action_of(c: &Value) -> ActionContext {
    let a = c
        .get("action")
        .unwrap_or_else(|| panic!("an autonomy case states an action"));
    let purpose = match text_at(a, "purpose").as_str() {
        "open" => Purpose::Open,
        "increase" => Purpose::Increase,
        "discretionary_exit" => Purpose::DiscretionaryExit,
        "owner_exit" => Purpose::OwnerExit,
        "risk_exit" => Purpose::RiskExit,
        "protective" => Purpose::Protective,
        other => panic!("`{other}` is a §6.1 purpose"),
    };
    let decimal = |key: &str, fallback: &str| {
        a.get(key)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let flag = |key: &str| a.get(key).and_then(Value::as_bool).unwrap_or(false);
    let asset_class = match a
        .get("asset_class")
        .and_then(Value::as_str)
        .unwrap_or("crypto")
    {
        "crypto" => AssetClass::Crypto,
        "us_equity" => AssetClass::UsEquity,
        other => panic!("`{other}` is an asset class"),
    };
    let session = match a.get("session").and_then(Value::as_str).unwrap_or("crypto") {
        "pre_market" => MarketSession::PreMarket,
        "regular" => MarketSession::Regular,
        "after_hours" => MarketSession::AfterHours,
        "crypto" => MarketSession::Crypto,
        other => panic!("`{other}` is a §6.3 session"),
    };
    ActionContext {
        purpose,
        order_usd: usd(&decimal("order_usd", "0")),
        combined_score: unit(&decimal("combined_score", "0")),
        instrument: asset(
            a.get("instrument")
                .and_then(Value::as_str)
                .unwrap_or(common::BTC_INSTRUMENT),
        ),
        asset_class,
        session,
        first_trade_in_instrument: flag("first_trade_in_instrument"),
        new_instrument: flag("new_instrument"),
        thesis_confidence: unit(&decimal("thesis_confidence", "0")),
        drawdown: unit(&decimal("drawdown", "0")),
        daily_pnl_fraction: signed(&decimal("daily_pnl_fraction", "0")),
        position_usd_after: usd(&decimal("position_usd_after", "0")),
        gross_usd_after: usd(&decimal("gross_usd_after", "0")),
        bought_today_usd: usd(&decimal("bought_today_usd", "0")),
        position_pnl_fraction: signed(&decimal("position_pnl_fraction", "0")),
    }
}

/// One `kind: autonomy` case: classify the action against the patched policy and compare every
/// member of `expect`.
fn autonomy_case(id: &str) {
    let c = case(id);
    assert_eq!(text_at(&c, "kind"), "autonomy", "{id} is an autonomy case");
    let policy = autonomy_of(&c);
    let action = action_of(&c);
    let decided = classify(&policy, &action)
        .unwrap_or_else(|e| panic!("{id}: classify returns a decision, not {e}"));
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} states an expectation"));

    let named = match decided.decision {
        AutonomyDecision::Auto => "auto",
        AutonomyDecision::Ask => "ask",
        AutonomyDecision::Deny => "deny",
    };
    assert_eq!(named, text_at(expect, "decision"), "{id}: the decision");
    assert_eq!(
        decided.by.label(),
        text_at(expect, "by"),
        "{id}: what decided it"
    );
    let approvers = expect
        .get("approvers_required")
        .and_then(Value::as_u64)
        .map(|n| u8::try_from(n).unwrap_or_else(|e| panic!("{id}: an approver count fits: {e}")));
    assert_eq!(
        decided.approval.map(|a| a.approvers_required.get()),
        approvers,
        "{id}: the approvers required"
    );
    if let Some(on_timeout) = expect.get("on_timeout").and_then(Value::as_str) {
        assert_eq!(on_timeout, "skip", "{id}: §6.4 allows one value");
        assert_eq!(
            decided.approval.map(|a| a.on_timeout),
            Some(OnTimeout::Skip),
            "{id}: the ASK carries skip-on-timeout"
        );
    }
    let known: BTreeSet<&str> = ["decision", "by", "approvers_required", "on_timeout"]
        .into_iter()
        .collect();
    assert_unknown_members(id, expect, &known);
}

/// Every `expect` member this harness has not been taught fails the case, so a member added to the
/// fixture cannot go silently unchecked (DEC-85).
fn assert_unknown_members(id: &str, expect: &Value, known: &BTreeSet<&str>) {
    let object = expect
        .as_object()
        .unwrap_or_else(|| panic!("{id}: `expect` is an object"));
    let unknown: Vec<&String> = object
        .keys()
        .filter(|k| !known.contains(k.as_str()))
        .collect();
    assert!(
        unknown.is_empty(),
        "{id}: this harness does not read {unknown:?}"
    );
}

fn signal_models(c: &Value) -> Vec<SignalModel> {
    patched(c, "behavior")
        .get("signal_models")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("the behavior block lists signal models"))
        .iter()
        .map(|m| SignalModel {
            id: ModelId::parse(&text_at(m, "id")).unwrap_or_else(|e| panic!("a model id: {e}")),
            version: common::version(&text_at(m, "version")),
            content_hash: digest(&text_at(m, "content_hash")),
            weight: frac(&text_at(m, "weight")),
            max_output_age_s: u32::try_from(
                m.get("max_output_age_s")
                    .and_then(Value::as_u64)
                    .unwrap_or_else(|| panic!("`max_output_age_s` is an integer")),
            )
            .unwrap_or_else(|e| panic!("`max_output_age_s` fits: {e}")),
        })
        .collect()
}

fn builder_mandate(c: &Value) -> BuilderMandate {
    let behavior = patched(c, "behavior");
    let sizing = behavior
        .get("sizing")
        .unwrap_or_else(|| panic!("the behavior block has a sizing block"));
    assert_eq!(
        text_at(sizing, "method"),
        "conviction_linear",
        "v1 has one sizing method"
    );
    let risk = patched(c, "risk");
    let goal = patched(c, "goal");
    BuilderMandate {
        models: signal_models(c),
        sizing: Sizing {
            method: SizingMethod::ConvictionLinear,
            entry_threshold: frac(&text_at(sizing, "entry_threshold")),
            exit_threshold: frac(&text_at(sizing, "exit_threshold")),
            rebalance_band: frac(&text_at(sizing, "rebalance_band")),
        },
        limits: Limits {
            max_position_usd: usd(&text_at(&risk, "max_position_usd")),
            max_position_fraction: frac(&text_at(&risk, "max_position_fraction")),
            max_order_usd: usd(&text_at(&risk, "max_order_usd")),
            max_gross_exposure_usd: usd(&text_at(&risk, "max_gross_exposure_usd")),
        },
        goal: match text_at(&goal, "type").as_str() {
            "continuous" => GoalKind::Continuous,
            "profit_stop" => GoalKind::ProfitStop,
            "accumulate" => GoalKind::Accumulate(AccumulateGoal {
                instrument: asset(&text_at(&goal, "instrument")),
                target_qty: qty(&text_at(&goal, "target_qty")),
                max_avg_price: goal.get("max_avg_price").and_then(Value::as_str).map(price),
                max_spend_usd: usd(&text_at(&goal, "max_spend_usd")),
            }),
            other => panic!("`{other}` is a §3.1 goal type"),
        },
    }
}

/// The input block's `working_opening_orders`, summed as §8.3 step 3 sums them: the maximum cost of
/// the agent's working **opening** orders in this instrument.
fn working_cost(input: &Value) -> Usd {
    let mut total = Usd::ZERO;
    for order in input
        .get("working_opening_orders")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        total = total
            .checked_add(usd(&text_at(order, "max_cost")))
            .unwrap_or_else(|e| panic!("the working costs sum: {e}"));
    }
    total
}

/// A `builder` case's inputs. The members `ref.py` reads through `inp.get(key, default)` are listed
/// here with the same defaults, because in Rust every field is required (DEC-130 item 19) and the
/// fixture's omissions have to be resolved somewhere the reader can see them.
fn inputs(c: &Value) -> (AccountSnapshot, Market, RiskContext, Vec<ModelOutput>) {
    let input = c
        .get("input")
        .unwrap_or_else(|| panic!("a builder case states an input"));
    let quote = input
        .get("quote")
        .unwrap_or_else(|| panic!("a builder case quotes a market"));
    let position = qty(&text_at(input, "position_qty"));
    let bid = price(&text_at(quote, "bid"));
    let text_or = |key: &str, fallback: &str| {
        input
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let gross = match input.get("gross_usd").and_then(Value::as_str) {
        Some(stated) => usd(stated),
        None => position
            .notional(bid)
            .unwrap_or_else(|e| panic!("the position's value: {e}"))
            .checked_add(working_cost(input))
            .unwrap_or_else(|e| panic!("the gross default: {e}")),
    };
    let asset_class = match text_at(input, "asset_class").as_str() {
        "crypto" => AssetClass::Crypto,
        "us_equity" => AssetClass::UsEquity,
        other => panic!("`{other}` is an asset class"),
    };
    let session = match text_or("session", "regular").as_str() {
        "pre_market" => MarketSession::PreMarket,
        "regular" => MarketSession::Regular,
        "after_hours" => MarketSession::AfterHours,
        "crypto" => MarketSession::Crypto,
        other => panic!("`{other}` is a §6.3 session"),
    };
    let account = AccountSnapshot {
        agent_equity: usd(&text_at(input, "agent_equity")),
        position_qty: position,
        cost_basis: basis(&text_or("cost_basis_usd", "0")),
        risk_mark: mark(&text_at(quote, "bid")),
        gross_usd: gross,
        working_opening_cost: working_cost(input),
        goal_spent_usd: usd(&text_or("goal_spent_usd", "0")),
    };
    let market = Market {
        instrument: asset(&text_at(input, "instrument")),
        asset_class,
        session,
        in_close_window: input
            .get("in_close_window")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        bid,
        ask: price(&text_at(quote, "ask")),
        increment: qty(&text_at(input, "qty_increment")),
        min_order_usd: usd(&text_at(input, "min_order_usd")),
        fee_rate_cash: fee(&text_or("fee_rate_cash", "0")),
        fee_rate_asset: fee(&text_or("fee_rate_asset", "0")),
    };
    let risk = RiskContext {
        size_factor: frac(&text_or("size_factor", "1")),
        drawdown: unit(&text_or("drawdown", "0")),
        daily_pnl_fraction: signed(&text_or("daily_pnl_fraction", "0")),
        position_pnl_fraction: signed(&text_or("position_pnl_fraction", "0")),
        bought_today_usd: usd(&text_or("bought_today_usd", "0")),
        has_prior_fill: input
            .get("has_prior_fill")
            .and_then(Value::as_bool)
            .unwrap_or(!position.is_zero()),
        new_instrument: input
            .get("new_instrument")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        thesis_confidence: unit(&text_or("thesis_confidence", "0")),
    };
    let models = signal_models(c);
    let outputs = input
        .get("outputs")
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("a builder case lists outputs"))
        .iter()
        .map(|o| ModelOutput {
            model_id: ModelId::parse(&text_at(o, "model_id"))
                .unwrap_or_else(|e| panic!("a model id: {e}")),
            model_version: common::version(&text_at(o, "model_version")),
            content_hash: digest(&text_at(o, "content_hash")),
            instrument: asset(&text_at(o, "instrument_id")),
            as_of: common::at(&text_at(o, "as_of")),
            expires_at: common::at(&text_at(o, "expires_at")),
            direction: Direction::Long,
            conviction: common::conv(&text_at(o, "conviction")),
            confidence: unit(&text_at(o, "confidence")),
        })
        .collect();
    let _ = models;
    (account, market, risk, outputs)
}

/// One `kind: builder` case: propose, compare every reported figure, then hand the case's own
/// `gate_dry_run` verdict to `decide` and compare the `autonomy` block.
fn builder_case(id: &str) {
    let c = case(id);
    assert_eq!(text_at(&c, "kind"), "builder", "{id} is a builder case");
    let mandate = builder_mandate(&c);
    let (account, market, risk, outputs) = inputs(&c);
    let now = common::at(&text_at(
        c.get("input")
            .unwrap_or_else(|| panic!("{id} states an input")),
        "now",
    ));
    let proposal = propose(&mandate, &account, &market, &risk, &outputs, now)
        .unwrap_or_else(|e| panic!("{id}: propose returns a proposal, not {e}"));
    let expect = c
        .get("expect")
        .unwrap_or_else(|| panic!("{id} states an expectation"));

    assert_sizes(id, &proposal.sizes, expect);
    if let Some(used) = expect.get("outputs_used").and_then(Value::as_array) {
        let wanted: BTreeSet<String> = used
            .iter()
            .map(|m| {
                m.as_str()
                    .unwrap_or_else(|| panic!("{id}: a model id is text"))
                    .to_owned()
            })
            .collect();
        let got: BTreeSet<String> = proposal
            .combined
            .outputs_used
            .iter()
            .map(|m| m.as_str().to_owned())
            .collect();
        assert_eq!(got, wanted, "{id}: the models counted");
    }
    for (key, got) in [
        (
            "combined_conviction",
            proposal.combined.exit_conviction.to_string(),
        ),
        (
            "buy_conviction",
            proposal.combined.buy_conviction.to_string(),
        ),
        ("combined_score", proposal.combined.score.to_string()),
    ] {
        if let Some(wanted) = expect.get(key).and_then(Value::as_str) {
            assert_eq!(got, wanted, "{id}: {key}");
        }
    }
    assert_action(id, &proposal.action, expect);
    if let Some(clips) = expect.get("clipped_by").and_then(Value::as_array) {
        let wanted: BTreeSet<String> = clips
            .iter()
            .map(|m| {
                m.as_str()
                    .unwrap_or_else(|| panic!("{id}: a clip is text"))
                    .to_owned()
            })
            .collect();
        let got: BTreeSet<String> = proposal
            .clipped_by
            .iter()
            .map(|c| c.as_str().to_owned())
            .collect();
        assert_eq!(got, wanted, "{id}: the clips reported");
    }

    if let Some(dry_run) = expect.get("gate_dry_run") {
        let verdict = match text_at(dry_run, "verdict").as_str() {
            "allow" => GateVerdict::Allow,
            "deny" => GateVerdict::Deny,
            "defer" => GateVerdict::Defer,
            other => panic!("{id}: `{other}` is a §9.1 verdict"),
        };
        let outcome = decide(&autonomy_of(&c), &proposal, verdict)
            .unwrap_or_else(|e| panic!("{id}: decide returns an outcome, not {e}"));
        assert_outcome(id, &outcome, expect);
    }

    let known: BTreeSet<&str> = [
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
    ]
    .into_iter()
    .collect();
    assert_unknown_members(id, expect, &known);
}

fn assert_sizes(id: &str, sizes: &Sizes, expect: &Value) {
    assert_eq!(
        sizes.cap.to_string(),
        text_at(expect, "cap"),
        "{id}: the position cap"
    );
    assert_eq!(
        sizes.current_mv.to_string(),
        text_at(expect, "current_mv"),
        "{id}: MV at the risk mark"
    );
    assert_eq!(
        sizes.target_value.as_ref().map(UsdExact::to_string),
        expect
            .get("target_value")
            .and_then(Value::as_str)
            .map(str::to_owned),
        "{id}: the target value T"
    );
    assert_eq!(
        sizes.delta.as_ref().map(UsdExact::to_string),
        expect
            .get("delta")
            .and_then(Value::as_str)
            .map(str::to_owned),
        "{id}: Delta"
    );
}

fn assert_action(id: &str, action: &Action, expect: &Value) {
    match text_at(expect, "action").as_str() {
        "hold" => match action {
            Action::Hold { reason } => assert_eq!(
                reason.as_str(),
                text_at(expect, "reason"),
                "{id}: the hold reason"
            ),
            other => panic!("{id}: the builder holds, and proposed {other:?}"),
        },
        "sell" => match action {
            Action::Sell {
                purpose,
                qty: quantity,
                limit_price,
                order_usd,
                shape,
            } => {
                assert_eq!(
                    purpose_name(*purpose),
                    text_at(expect, "purpose"),
                    "{id}: the purpose label"
                );
                assert_eq!(
                    quantity.to_string(),
                    text_at(expect, "qty"),
                    "{id}: the quantity"
                );
                assert_eq!(
                    limit_price.to_string(),
                    text_at(expect, "limit_price"),
                    "{id}: the limit price"
                );
                assert_eq!(
                    order_usd.to_string(),
                    text_at(expect, "order_usd"),
                    "{id}: the order value"
                );
                assert_eq!(
                    shape.as_str(),
                    expect
                        .get("order_type")
                        .and_then(Value::as_str)
                        .unwrap_or("limit"),
                    "{id}: the order shape"
                );
            }
            other => panic!("{id}: the builder sells, and proposed {other:?}"),
        },
        "buy" => match action {
            Action::Buy {
                purpose,
                qty: quantity,
                limit_price,
                order_usd,
                ..
            } => {
                assert_eq!(
                    purpose_name(*purpose),
                    text_at(expect, "purpose"),
                    "{id}: the purpose label"
                );
                assert_eq!(
                    quantity.to_string(),
                    text_at(expect, "qty"),
                    "{id}: the quantity"
                );
                assert_eq!(
                    limit_price.to_string(),
                    text_at(expect, "limit_price"),
                    "{id}: the limit price"
                );
                assert_eq!(
                    order_usd.to_string(),
                    text_at(expect, "order_usd"),
                    "{id}: the order value"
                );
            }
            other => panic!("{id}: the builder buys, and proposed {other:?}"),
        },
        other => panic!("{id}: `{other}` is not an action this harness reads"),
    }
}

fn purpose_name(purpose: Purpose) -> &'static str {
    match purpose {
        Purpose::Open => "open",
        Purpose::Increase => "increase",
        Purpose::DiscretionaryExit => "discretionary_exit",
        Purpose::OwnerExit => "owner_exit",
        Purpose::RiskExit => "risk_exit",
        Purpose::Protective => "protective",
    }
}

/// The cases carry `skipped` and `deferred` in the `autonomy` field; in Rust they are outcomes and
/// not decisions, so the harness maps the two shapes (DEC-130 item 14).
fn assert_outcome(id: &str, outcome: &Outcome, expect: &Value) {
    let Some(autonomy) = expect.get("autonomy") else {
        return;
    };
    let decision = text_at(autonomy, "decision");
    let by = text_at(autonomy, "by");
    match (decision.as_str(), outcome) {
        ("skipped", Outcome::Skipped) | ("deferred", Outcome::Deferred) => {
            assert_eq!(
                by, "gate_dry_run",
                "{id}: a skip and a defer are the gate's"
            );
        }
        (_, Outcome::Classified(classified)) => {
            let named = match classified.decision {
                AutonomyDecision::Auto => "auto",
                AutonomyDecision::Ask => "ask",
                AutonomyDecision::Deny => "deny",
            };
            assert_eq!(named, decision, "{id}: the autonomy decision");
            assert_eq!(classified.by.label(), by, "{id}: what decided it");
            let approvers = autonomy
                .get("approvers_required")
                .and_then(Value::as_u64)
                .map(|n| u8::try_from(n).unwrap_or_else(|e| panic!("{id}: a count fits: {e}")));
            assert_eq!(
                classified.approval.map(|a| a.approvers_required.get()),
                approvers,
                "{id}: the approvers required"
            );
            if autonomy.get("on_timeout").is_some() {
                assert_eq!(
                    classified.approval.map(|a| a.on_timeout),
                    Some(OnTimeout::Skip),
                    "{id}: the ASK carries skip-on-timeout"
                );
            }
        }
        (wanted, got) => panic!("{id}: the case expects `{wanted}` and the crate returned {got:?}"),
    }
}

/// `MC-A01`: Discretionary exit is AUTO regardless of rules.
#[test]
fn mc_a01() {
    autonomy_case("MC-A01");
}

/// `MC-A02`: Protective order is AUTO.
#[test]
fn mc_a02() {
    autonomy_case("MC-A02");
}

/// `MC-A03`: Risk exit is AUTO.
#[test]
fn mc_a03() {
    autonomy_case("MC-A03");
}

/// `MC-A04`: Owner exit is AUTO.
#[test]
fn mc_a04() {
    autonomy_case("MC-A04");
}

/// `MC-A05`: Large open: ASK by large_orders.
#[test]
fn mc_a05() {
    autonomy_case("MC-A05");
}

/// `MC-A06`: Low score open: ASK.
#[test]
fn mc_a06() {
    autonomy_case("MC-A06");
}

/// `MC-A07`: Routine open: AUTO.
#[test]
fn mc_a07() {
    autonomy_case("MC-A07");
}

/// `MC-A08`: Order exactly at the threshold is not large.
#[test]
fn mc_a08() {
    autonomy_case("MC-A08");
}

/// `MC-A09`: No rule matches: default ask.
#[test]
fn mc_a09() {
    autonomy_case("MC-A09");
}

/// `MC-A10`: Two approvers above the threshold.
#[test]
fn mc_a10() {
    autonomy_case("MC-A10");
}

/// `MC-A11`: Bought-today rule catches order splitting.
#[test]
fn mc_a11() {
    autonomy_case("MC-A11");
}

/// `MC-A12`: The admission ceiling turns an auto rule into ask for a new instrument.
#[test]
fn mc_a12() {
    autonomy_case("MC-A12");
}

/// `MC-A13`: Admission auto confirmed by the owner is AUTO.
#[test]
fn mc_a13() {
    autonomy_case("MC-A13");
}

/// `MC-A14`: Admission deny overrides an auto rule.
#[test]
fn mc_a14() {
    autonomy_case("MC-A14");
}

/// `MC-A15`: The admission ceiling never loosens a deny rule.
#[test]
fn mc_a15() {
    autonomy_case("MC-A15");
}

/// `MC-A16`: A thesis-confidence rule asks below the owner threshold.
#[test]
fn mc_a16() {
    autonomy_case("MC-A16");
}

/// `MC-B01`: Two models, open, AUTO by the routine rule.
#[test]
fn mc_b01() {
    builder_case("MC-B01");
}

/// `MC-B02`: Same outputs with the 0.5 ladder factor.
#[test]
fn mc_b02() {
    builder_case("MC-B02");
}

/// `MC-B03`: Between thresholds: hold.
#[test]
fn mc_b03() {
    builder_case("MC-B03");
}

/// `MC-B04`: Below the exit threshold: discretionary exit, AUTO built-in.
#[test]
fn mc_b04() {
    builder_case("MC-B04");
}

/// `MC-B05`: Increase an existing position.
#[test]
fn mc_b05() {
    builder_case("MC-B05");
}

/// `MC-B06`: Missing model counts as fully bearish for buys: hold.
#[test]
fn mc_b06() {
    builder_case("MC-B06");
}

/// `MC-B07`: Missing model counts as zero for exits: still exits.
#[test]
fn mc_b07() {
    builder_case("MC-B07");
}

/// `MC-B08`: Low combined score: ASK.
#[test]
fn mc_b08() {
    builder_case("MC-B08");
}

/// `MC-B09`: Future as_of is ignored.
#[test]
fn mc_b09() {
    builder_case("MC-B09");
}

/// `MC-B10`: Output older than the model's max_output_age_s is ignored.
#[test]
fn mc_b10() {
    builder_case("MC-B10");
}

/// `MC-B11`: Duplicate outputs: the latest per model wins.
#[test]
fn mc_b11() {
    builder_case("MC-B11");
}

/// `MC-B12`: Wrong model version is ignored (counts as missing).
#[test]
fn mc_b12() {
    builder_case("MC-B12");
}

/// `MC-B13`: Score rounds to 12 places before the rule compares it.
#[test]
fn mc_b13() {
    builder_case("MC-B13");
}

/// `MC-B14`: Clipped to max_order_usd.
#[test]
fn mc_b14() {
    builder_case("MC-B14");
}

/// `MC-B15`: Working opening order counts toward the target.
#[test]
fn mc_b15() {
    builder_case("MC-B15");
}

/// `MC-B16`: Above target with positive conviction and limit_buys: hold.
#[test]
fn mc_b16() {
    builder_case("MC-B16");
}

/// `MC-B18`: Delta within the rebalance band: hold.
#[test]
fn mc_b18() {
    builder_case("MC-B18");
}

/// `MC-B19`: Value after limit clips below the band: hold.
#[test]
fn mc_b19() {
    builder_case("MC-B19");
}

/// `MC-B20`: No fresh outputs: hold.
#[test]
fn mc_b20() {
    builder_case("MC-B20");
}

/// `MC-B21`: Gate dry run denies: no ASK is sent.
#[test]
fn mc_b21() {
    builder_case("MC-B21");
}

/// `MC-B22`: Discretionary exit outside the regular session is deferred.
#[test]
fn mc_b22() {
    builder_case("MC-B22");
}

/// `MC-B23`: Discretionary exit in the close window goes out as a marketable limit.
#[test]
fn mc_b23() {
    builder_case("MC-B23");
}

/// `MC-B24`: Averaging-down rule denies an increase.
#[test]
fn mc_b24() {
    builder_case("MC-B24");
}

/// `MC-B25`: Re-entry after a round trip is not a first trade.
#[test]
fn mc_b25() {
    builder_case("MC-B25");
}

/// `MC-B26`: Accumulate clipped to the remaining target quantity.
#[test]
fn mc_b26() {
    builder_case("MC-B26");
}

/// `MC-B27`: Accumulate clipped by max_avg_price.
#[test]
fn mc_b27() {
    builder_case("MC-B27");
}

/// `MC-B28`: Accumulate with fees: quantity received and spend include fees.
#[test]
fn mc_b28() {
    builder_case("MC-B28");
}

/// `MC-B29`: Accumulate never sells on negative conviction.
#[test]
fn mc_b29() {
    builder_case("MC-B29");
}
