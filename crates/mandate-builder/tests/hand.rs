//! Every figure of [mandate spec §6](../../../docs/specs/mandate.md#6-autonomy-dec-42-dec-48-dec-58)
//! and [§8.3](../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60)
//! recomputed by hand from the rule, against the `two_stock_swing` and `btc_accumulator` bases the
//! reference cases use.
//!
//! These are the boundaries and the orderings the `A` and `B` families state once each and that a
//! generated case would reach only by accident: the two freshness bounds and their exact instants,
//! the tie-break among duplicate outputs, the order the four clips bind in, the two orderings
//! DEC-130 item 21 fixes, and the refusals of item 8.
//!
//! **Every `propose` test asserts the three combined figures and the reported sizes as well as the
//! action.** A hold is the plausible do-nothing answer for §8.3, so a test that asserted only "it
//! held" would pass on an implementation that holds for every input; asserting c, b, s, cap and MV
//! beside the reason is what makes the test fail on one (DEC-110). Likewise every `classify` test
//! asserts `by` and not only the decision, and each one that expects AUTO carries a contrasting
//! action under the same rules that must not be AUTO.
//!
//! All of them are pending until the implementation PR: the crate is stubs, so each fails on
//! `BuilderError::Unimplemented` (DEC-77, DEC-83, DEC-110).

mod common;

use common::{
    BTC_INSTRUMENT, NOW, OTHER_INSTRUMENT, SWING_INSTRUMENT, account, asset, base_policy,
    base_rules, basis, btc_accumulator, compare, crypto_market, decimal, digest, fee, flag,
    flat_account, frac, ids, mark, mean_reversion, model, momentum, news, output, policy, price,
    qty, quiet_risk, rule_id, signed, swing_market, text, timed_output, two_stock_swing, unit, usd,
    version,
};
use mandate_builder::{
    AccountSnapshot, AccumulateGoal, Action, ActionContext, BuilderMandate, Classification, Clip,
    Combined, DecidedBy, GateVerdict, GoalKind, HoldReason, Limits, Market, ModelOutput,
    OrderShape, Outcome, Proposal, RiskContext, SignalModel, Sizes, Sizing, classify, combine,
    decide, propose,
};
use mandate_domain::{AssetClass, AutonomyDecision, MarketSession, Purpose};
use mandate_num::{Signed, SizeFraction, Unit, UsdExact};
use mandate_spec::condition::{Condition, ConditionField, ConditionValue, Operator};
use mandate_spec::document::{Autonomy, OnTimeout, Rule};
use mandate_spec::{DecGrammar, SchemaDec};

fn proposed(
    mandate: &BuilderMandate,
    account: &AccountSnapshot,
    market: &Market,
    risk: &RiskContext,
    outputs: &[ModelOutput],
) -> Proposal {
    propose(mandate, account, market, risk, outputs, common::at(NOW))
        .unwrap_or_else(|e| panic!("propose returns a proposal, not {e}"))
}

fn classified(policy: &Autonomy, action: &ActionContext) -> Classification {
    classify(policy, action).unwrap_or_else(|e| panic!("classify returns a decision, not {e}"))
}

fn assert_combined(combined: &Combined, used: &[&str], exit: &str, buy: &str, score: &str) {
    assert_eq!(combined.outputs_used, ids(used), "the models counted");
    assert_eq!(
        combined.exit_conviction.to_string(),
        exit,
        "the exit conviction c"
    );
    assert_eq!(
        combined.buy_conviction.to_string(),
        buy,
        "the buy conviction b"
    );
    assert_eq!(combined.score.to_string(), score, "the combined score s");
}

fn assert_sizes(
    sizes: &Sizes,
    cap: &str,
    current_mv: &str,
    target: Option<&str>,
    delta: Option<&str>,
) {
    assert_eq!(sizes.cap.to_string(), cap, "the position cap");
    assert_eq!(
        sizes.current_mv.to_string(),
        current_mv,
        "MV at the risk mark"
    );
    assert_eq!(
        sizes.target_value.as_ref().map(UsdExact::to_string),
        target.map(str::to_owned),
        "the target value T"
    );
    assert_eq!(
        sizes.delta.as_ref().map(UsdExact::to_string),
        delta.map(str::to_owned),
        "Delta"
    );
}

fn hold_reason(action: &Action) -> HoldReason {
    match action {
        Action::Hold { reason } => *reason,
        other => panic!("the builder holds, and instead proposed {other:?}"),
    }
}

/// An `open` action at the stated order value and combined score, with every other fact at the
/// value `MC-A05` to `MC-A11` state: no admission, no thesis, nothing bought today.
fn opening(order_usd: &str, combined_score: &str) -> ActionContext {
    ActionContext {
        purpose: Purpose::Open,
        order_usd: usd(order_usd),
        combined_score: unit(combined_score),
        instrument: asset(BTC_INSTRUMENT),
        asset_class: AssetClass::Crypto,
        session: MarketSession::Crypto,
        first_trade_in_instrument: false,
        new_instrument: false,
        thesis_confidence: Unit::ZERO,
        drawdown: Unit::ZERO,
        daily_pnl_fraction: Signed::ZERO,
        position_usd_after: usd("0"),
        gross_usd_after: usd("0"),
        bought_today_usd: usd("0"),
        position_pnl_fraction: Signed::ZERO,
    }
}

fn reducing(purpose: Purpose) -> ActionContext {
    ActionContext {
        purpose,
        ..opening("300", "0.8")
    }
}

/// §6.2 step 3, MI-1, DEC-05: the four purposes other than `open` and `increase` are AUTO by the
/// built-in, whatever the rules say. The anchor is the same rule set denying an `open`, so the test
/// cannot pass on an implementation that returns AUTO for everything.
#[test]
#[ignore = "pending E6-2"]
fn every_reducing_purpose_is_auto_by_the_builtin() {
    let deny_everything = policy(
        vec![Rule {
            id: rule_id("deny_all"),
            when: compare(ConditionField::OrderUsd, Operator::Gte, decimal("0")),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Deny,
        AutonomyDecision::Deny,
        None,
    );
    for purpose in [
        Purpose::DiscretionaryExit,
        Purpose::OwnerExit,
        Purpose::RiskExit,
        Purpose::Protective,
    ] {
        let decided = classified(&deny_everything, &reducing(purpose));
        assert_eq!(
            decided.decision,
            AutonomyDecision::Auto,
            "{purpose:?} is AUTO"
        );
        assert_eq!(decided.by, DecidedBy::BuiltinRiskReducing, "{purpose:?}");
        assert_eq!(decided.approval, None, "{purpose:?} asks nobody");
    }
    let opening_action = classified(&deny_everything, &opening("300", "0.8"));
    assert_eq!(
        opening_action.decision,
        AutonomyDecision::Deny,
        "the same rules deny an open, so the four AUTOs above are the built-in and not a default"
    );
}

/// §6.1: the builder labels a buy `open` with no position and `increase` with one, and the label
/// follows the position rather than the order.
#[test]
#[ignore = "pending E6-2"]
fn a_buy_with_no_position_is_open_and_with_one_is_increase() {
    let mandate = two_stock_swing();
    let outputs = [
        output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9"),
        output(&news(), SWING_INSTRUMENT, "0.2", "0.5"),
    ];
    let flat = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &flat.combined,
        &["llm.news_research", "quant.momentum"],
        "0.472",
        "0.472",
        "0.74",
    );
    assert_sizes(&flat.sizes, "1500", "0", Some("708"), Some("708"));
    match &flat.action {
        Action::Buy {
            purpose,
            qty: q,
            order_usd,
            ..
        } => {
            assert_eq!(*purpose, Purpose::Open, "no position, so the label is open");
            assert_eq!(*q, qty("7"));
            assert_eq!(*order_usd, usd("700"));
        }
        other => panic!("a buy of 7 at 100, not {other:?}"),
    }
    let held = proposed(
        &mandate,
        &account("3", "99.9"),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&held.sizes, "1500", "299.7", Some("708"), Some("408.3"));
    match &held.action {
        Action::Buy {
            purpose,
            qty: q,
            order_usd,
            ..
        } => {
            assert_eq!(
                *purpose,
                Purpose::Increase,
                "a position, so the label is increase"
            );
            assert_eq!(*q, qty("4"));
            assert_eq!(*order_usd, usd("400"));
        }
        other => panic!("a buy of 4 at 100, not {other:?}"),
    }
}

/// §6.2 step 4: the **first** matching rule decides. `low_score` and `routine` both match an
/// opening action scored 0.6; the answer is `low_score`'s ASK, and reversing the two rules gives
/// `routine`'s AUTO, so the test pins the order and not one rule's outcome.
#[test]
#[ignore = "pending E6-2"]
fn the_first_matching_rule_decides_and_a_later_one_is_not_read() {
    let action = opening("300", "0.6");
    let forwards = classified(&base_policy(), &action);
    assert_eq!(forwards.decision, AutonomyDecision::Ask);
    assert_eq!(forwards.by, DecidedBy::Rule(rule_id("low_score")));

    let mut reversed = base_rules();
    reversed.reverse();
    let backwards = classified(
        &policy(reversed, AutonomyDecision::Ask, AutonomyDecision::Ask, None),
        &action,
    );
    assert_eq!(
        backwards.decision,
        AutonomyDecision::Auto,
        "`routine` is first now, so it decides and `low_score` is never read"
    );
    assert_eq!(backwards.by, DecidedBy::Rule(rule_id("routine")));
}

/// §6.2 step 4: with no rule matching, `autonomy.default` decides and `by` says so (MC-A09). The
/// anchor is the same action under the base rules, which does match one.
#[test]
#[ignore = "pending E6-2"]
fn no_rule_matches_so_the_default_decides() {
    let empty = policy(
        Vec::new(),
        AutonomyDecision::Ask,
        AutonomyDecision::Ask,
        None,
    );
    let decided = classified(&empty, &opening("100", "0.9"));
    assert_eq!(decided.decision, AutonomyDecision::Ask);
    assert_eq!(decided.by, DecidedBy::Default);

    let denying_default = policy(
        Vec::new(),
        AutonomyDecision::Deny,
        AutonomyDecision::Ask,
        None,
    );
    assert_eq!(
        classified(&denying_default, &opening("100", "0.9")).decision,
        AutonomyDecision::Deny,
        "the default is read, not assumed to be ask"
    );
    assert_eq!(
        classified(&base_policy(), &opening("100", "0.9")).by,
        DecidedBy::Rule(rule_id("routine")),
        "a matching rule is still preferred to the default"
    );
}

/// §6.2 step 5, MC-A12: for a new instrument the decision becomes the stricter of the rule's and
/// `autonomy.admission`, and `by` names the ceiling **only** when the ceiling changed the answer —
/// MC-A13's `auto` admission leaves `rule:routine` in place.
#[test]
#[ignore = "pending E6-2"]
fn the_admission_ceiling_turns_auto_into_ask_for_a_new_instrument() {
    let admitted = ActionContext {
        new_instrument: true,
        thesis_confidence: unit("0.9"),
        ..opening("300", "0.8")
    };
    let asked = classified(&base_policy(), &admitted);
    assert_eq!(asked.decision, AutonomyDecision::Ask);
    assert_eq!(asked.by, DecidedBy::AdmissionCeiling);
    assert_eq!(
        asked.approval.map(|a| a.approvers_required.get()),
        Some(1),
        "an ASK the ceiling raised still carries its approval"
    );

    let owner_allowed = policy(
        base_rules(),
        AutonomyDecision::Ask,
        AutonomyDecision::Auto,
        None,
    );
    let automatic = classified(&owner_allowed, &admitted);
    assert_eq!(automatic.decision, AutonomyDecision::Auto);
    assert_eq!(
        automatic.by,
        DecidedBy::Rule(rule_id("routine")),
        "the ceiling did not change the decision, so `by` stays the rule's (MC-A13)"
    );
}

/// §6.2 step 5, MC-A14 and MC-A15: the ceiling only tightens. An `auto` admission leaves a `deny`
/// rule denying, and a `deny` admission overrides an `auto` rule.
#[test]
#[ignore = "pending E6-2"]
fn the_admission_ceiling_never_loosens_a_deny_rule() {
    let admitted = ActionContext {
        new_instrument: true,
        thesis_confidence: unit("0.9"),
        ..opening("300", "0.8")
    };
    let mut rules = vec![Rule {
        id: rule_id("no_new"),
        when: compare(ConditionField::NewInstrument, Operator::Eq, flag(true)),
        then: AutonomyDecision::Deny,
    }];
    rules.extend(base_rules());
    let loose_admission = policy(rules, AutonomyDecision::Ask, AutonomyDecision::Auto, None);
    let denied = classified(&loose_admission, &admitted);
    assert_eq!(denied.decision, AutonomyDecision::Deny, "MC-A15");
    assert_eq!(denied.by, DecidedBy::Rule(rule_id("no_new")));
    assert_eq!(
        denied.approval, None,
        "a DENY is never overridden, so nobody is asked"
    );

    let strict_admission = policy(
        base_rules(),
        AutonomyDecision::Ask,
        AutonomyDecision::Deny,
        None,
    );
    let ceiling_denied = classified(&strict_admission, &admitted);
    assert_eq!(ceiling_denied.decision, AutonomyDecision::Deny, "MC-A14");
    assert_eq!(ceiling_denied.by, DecidedBy::AdmissionCeiling);
}

/// §6.2 step 5: the ceiling applies only when `new_instrument` is true. The same `deny` admission
/// leaves an order in a held instrument exactly where the rules put it.
#[test]
#[ignore = "pending E6-2"]
fn an_admission_ceiling_does_not_touch_an_order_in_a_held_instrument() {
    let strict_admission = policy(
        base_rules(),
        AutonomyDecision::Ask,
        AutonomyDecision::Deny,
        None,
    );
    let held = classified(&strict_admission, &opening("300", "0.8"));
    assert_eq!(held.decision, AutonomyDecision::Auto);
    assert_eq!(held.by, DecidedBy::Rule(rule_id("routine")));

    let admitted = ActionContext {
        new_instrument: true,
        ..opening("300", "0.8")
    };
    assert_eq!(
        classified(&strict_admission, &admitted).decision,
        AutonomyDecision::Deny,
        "the one flag is the difference, so the ceiling is read and not ignored"
    );
}

/// §6.4, MC-A10: an ASK above `two_approver_above_usd` needs two approvers, and the comparison is
/// strict, so an order exactly at the threshold needs one.
#[test]
#[ignore = "pending E6-2"]
fn two_approvers_above_the_threshold_and_one_at_it() {
    let with_threshold = policy(
        base_rules(),
        AutonomyDecision::Ask,
        AutonomyDecision::Ask,
        Some("500"),
    );
    let above = classified(&with_threshold, &opening("600", "0.6"));
    assert_eq!(above.decision, AutonomyDecision::Ask);
    assert_eq!(above.approval.map(|a| a.approvers_required.get()), Some(2));

    let at_threshold = classified(&with_threshold, &opening("500", "0.6"));
    assert_eq!(
        at_threshold.approval.map(|a| a.approvers_required.get()),
        Some(1),
        "the comparison is `>`, so 500 is not above 500"
    );

    let unset = classified(&base_policy(), &opening("600", "0.6"));
    assert_eq!(
        unset.approval.map(|a| a.approvers_required.get()),
        Some(1),
        "`null` means one approver at any size"
    );
}

/// §6.4, DEC-06: `on_timeout` is always `skip`, so an unanswered ASK adds no risk. An AUTO and a
/// DENY carry no approval at all.
#[test]
#[ignore = "pending E6-2"]
fn an_ask_always_carries_skip_on_timeout() {
    let asked = classified(&base_policy(), &opening("950", "0.9"));
    assert_eq!(asked.decision, AutonomyDecision::Ask);
    assert_eq!(asked.by, DecidedBy::Rule(rule_id("large_orders")));
    let approval = asked
        .approval
        .unwrap_or_else(|| panic!("an ASK carries an approval"));
    assert_eq!(approval.on_timeout, OnTimeout::Skip);

    assert_eq!(
        classified(&base_policy(), &opening("300", "0.8")).approval,
        None,
        "an AUTO asks nobody"
    );
    let denying = policy(
        vec![Rule {
            id: rule_id("never"),
            when: compare(ConditionField::OrderUsd, Operator::Gte, decimal("0")),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Ask,
        AutonomyDecision::Ask,
        None,
    );
    assert_eq!(
        classified(&denying, &opening("300", "0.8")).approval,
        None,
        "a DENY asks nobody"
    );
}

/// §6.3, V-017: `all`, `any`, and `not` evaluate as written and nest to four levels. The two halves
/// differ in one leaf, so the test pins the tree's meaning rather than one answer.
#[test]
#[ignore = "pending E6-2"]
fn nested_conditions_evaluate_as_written() {
    let four_levels = |threshold: &str| {
        Condition::All(vec![Condition::Any(vec![Condition::Not(Box::new(
            compare(ConditionField::OrderUsd, Operator::Lt, decimal(threshold)),
        ))])])
    };
    let matching = policy(
        vec![Rule {
            id: rule_id("nested"),
            when: four_levels("100"),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classified(&matching, &opening("300", "0.8")).by,
        DecidedBy::Rule(rule_id("nested")),
        "`not (300 < 100)` is true, so the rule matches through three combinators"
    );
    let not_matching = policy(
        vec![Rule {
            id: rule_id("nested"),
            when: four_levels("900"),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classified(&not_matching, &opening("300", "0.8")).by,
        DecidedBy::Default,
        "`not (300 < 900)` is false, so the same tree does not match"
    );
    let five_levels = policy(
        vec![Rule {
            id: rule_id("too_deep"),
            when: Condition::All(vec![four_levels("100")]),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classify(&five_levels, &opening("300", "0.8")).map_err(|e| e.code()),
        Err("condition_too_deep"),
        "V-017 allows four levels, and the order path refuses a fifth rather than evaluating it"
    );
}

/// §6.3: decimal fields compare numerically, never as text. The trap that survives the schema is
/// the digit count: `10` is above `9` and above `9.5`, while as text it sorts below both.
///
/// The other half of the trap — `0.650` against `0.65` — is unrepresentable, because the schema's
/// decimal grammars are canonical and refuse a trailing fractional zero. That is the trust ladder's
/// first rung doing the work, and the first assertion records it so the next reader does not go
/// looking for the test that is missing.
#[test]
#[ignore = "pending E6-2"]
fn a_decimal_condition_compares_numerically_not_lexically() {
    assert!(
        SchemaDec::parse("0.650", DecGrammar::Decimal).is_err(),
        "a trailing fractional zero is not in the grammar, so `0.650` never reaches a comparison"
    );

    let over_nine = policy(
        vec![Rule {
            id: rule_id("over_nine"),
            when: compare(ConditionField::OrderUsd, Operator::Gt, decimal("9")),
            then: AutonomyDecision::Ask,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classified(&over_nine, &opening("10", "0.8")).by,
        DecidedBy::Rule(rule_id("over_nine")),
        "10 is above 9; as text `10` sorts below `9`"
    );
    assert_eq!(
        classified(&over_nine, &opening("9", "0.8")).by,
        DecidedBy::Default,
        "9 is not above 9"
    );

    let over_nine_and_a_half = policy(
        vec![Rule {
            id: rule_id("over_nine"),
            when: compare(ConditionField::OrderUsd, Operator::Lt, decimal("9.5")),
            then: AutonomyDecision::Ask,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classified(&over_nine_and_a_half, &opening("10", "0.8")).by,
        DecidedBy::Default,
        "10 is not below 9.5; as text `10` sorts below `9.5`"
    );
}

/// §6.3, V-023: a value that is not of its field's type is refused rather than compared. The anchor
/// is the same field with a value of the right type, which classifies.
#[test]
#[ignore = "pending E6-2"]
fn a_condition_whose_value_does_not_match_its_field_type_is_refused() {
    let listed_decimal = policy(
        vec![Rule {
            id: rule_id("mistyped"),
            when: Condition::Compare {
                field: ConditionField::OrderUsd,
                op: Operator::In,
                value: ConditionValue::List(vec!["300".to_owned()]),
            },
            then: AutonomyDecision::Ask,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classify(&listed_decimal, &opening("300", "0.8")).map_err(|e| e.code()),
        Err("condition_type_mismatch"),
        "`in` does not apply to a decimal field"
    );
    let ordered_enum = policy(
        vec![Rule {
            id: rule_id("mistyped"),
            when: compare(ConditionField::Purpose, Operator::Gt, text("open")),
            then: AutonomyDecision::Ask,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classify(&ordered_enum, &opening("300", "0.8")).map_err(|e| e.code()),
        Err("condition_type_mismatch"),
        "an enum field has no ordering"
    );
    let well_typed = policy(
        vec![Rule {
            id: rule_id("typed"),
            when: compare(ConditionField::Purpose, Operator::Eq, text("open")),
            then: AutonomyDecision::Ask,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classified(&well_typed, &opening("300", "0.8")).by,
        DecidedBy::Rule(rule_id("typed")),
        "the same field with a value of its own type is evaluated"
    );
}

/// §6.3, V-018: `unusual_input` is reserved until the input-drift detector ships, so a rule naming
/// it is refused and the field is never reached.
#[test]
#[ignore = "pending E6-2"]
fn a_rule_using_unusual_input_is_refused() {
    let reserved = policy(
        vec![Rule {
            id: rule_id("drift"),
            when: compare(ConditionField::UnusualInput, Operator::Eq, flag(true)),
            then: AutonomyDecision::Ask,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classify(&reserved, &opening("300", "0.8")).map_err(|e| e.code()),
        Err("reserved_field")
    );
    let buried = policy(
        vec![Rule {
            id: rule_id("drift"),
            when: Condition::All(vec![
                compare(ConditionField::OrderUsd, Operator::Gt, decimal("0")),
                compare(ConditionField::UnusualInput, Operator::Ne, flag(false)),
            ]),
            then: AutonomyDecision::Ask,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classify(&buried, &opening("300", "0.8")).map_err(|e| e.code()),
        Err("reserved_field"),
        "the whole tree is checked, not the outermost comparison"
    );
}

/// §6.3, MC-A11: `bought_today_usd` is the risk day's opening and increasing value **including this
/// order**, which is what bounds order splitting. Two orders of 500 under a 2000 bound ask once the
/// day's total passes it, and not before.
#[test]
#[ignore = "pending E6-2"]
fn bought_today_catches_order_splitting() {
    let mut rules = vec![Rule {
        id: rule_id("daily_buys"),
        when: compare(
            ConditionField::BoughtTodayUsd,
            Operator::Gt,
            decimal("2000"),
        ),
        then: AutonomyDecision::Ask,
    }];
    rules.extend(base_rules());
    let bounded = policy(rules, AutonomyDecision::Ask, AutonomyDecision::Ask, None);

    let over = ActionContext {
        bought_today_usd: usd("2300"),
        ..opening("500", "0.9")
    };
    assert_eq!(
        classified(&bounded, &over).by,
        DecidedBy::Rule(rule_id("daily_buys"))
    );
    let under = ActionContext {
        bought_today_usd: usd("2000"),
        ..opening("500", "0.9")
    };
    assert_eq!(
        classified(&bounded, &under).by,
        DecidedBy::Rule(rule_id("routine")),
        "at the bound and not above it, the day's rule does not match"
    );
}

/// §6.3, DEC-85, DEC-130 item 19, MC-B25: `first_trade_in_instrument` comes from the caller's
/// `has_prior_fill` and is never inferred from the position, because a re-entry after a round trip
/// is a flat agent that has traded here before.
#[test]
#[ignore = "pending E6-2"]
fn an_absent_prior_fill_flag_is_not_inferred_from_the_position() {
    let mandate = two_stock_swing();
    let outputs = [
        output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9"),
        output(&news(), SWING_INSTRUMENT, "0.2", "0.5"),
    ];
    let re_entry = RiskContext {
        has_prior_fill: true,
        ..quiet_risk()
    };
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &re_entry,
        &outputs,
    );
    assert_sizes(&proposal.sizes, "1500", "0", Some("708"), Some("708"));
    match &proposal.action {
        Action::Buy { action, .. } => assert!(
            !action.first_trade_in_instrument,
            "flat but with a prior fill, so this is not a first trade"
        ),
        other => panic!("a buy, not {other:?}"),
    }
    let first = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    match &first.action {
        Action::Buy { action, .. } => assert!(
            action.first_trade_in_instrument,
            "the same flat position with no prior fill is a first trade"
        ),
        other => panic!("a buy, not {other:?}"),
    }
}

/// §8.2: fresh is `as_of ≤ now`, so an output cut after `now` is ignored (MC-B09) and one cut
/// exactly at `now` is used. The anchor is the second half: the later output is bearish enough to
/// change the action, so an implementation that ignored both would fail it.
#[test]
#[ignore = "pending E6-2"]
fn a_future_as_of_is_not_fresh() {
    let mandate = two_stock_swing();
    let base = [
        output(&momentum(), SWING_INSTRUMENT, "0.9", "0.9"),
        output(&news(), SWING_INSTRUMENT, "0.9", "0.9"),
    ];
    let one_nanosecond_late = timed_output(
        &news(),
        SWING_INSTRUMENT,
        "-1",
        "1",
        "2026-09-22T14:00:00.000000001Z",
        "2026-09-22T15:00:00.000000000Z",
    );
    let mut ignored = base.to_vec();
    ignored.push(one_nanosecond_late);
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &ignored,
    );
    assert_combined(
        &proposal.combined,
        &["llm.news_research", "quant.momentum"],
        "0.81",
        "0.81",
        "0.9",
    );
    assert_sizes(&proposal.sizes, "1500", "0", Some("1215"), Some("1215"));
    match &proposal.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("10"));
            assert_eq!(*order_usd, usd("1000"));
        }
        other => panic!("a buy clipped to max_order_usd, not {other:?}"),
    }
    assert_eq!(proposal.clipped_by, [Clip::Limits].into_iter().collect());

    let mut at_now = base.to_vec();
    at_now.push(timed_output(
        &news(),
        SWING_INSTRUMENT,
        "-1",
        "1",
        NOW,
        "2026-09-22T15:00:00.000000000Z",
    ));
    let bearish = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &at_now,
    );
    assert_combined(
        &bearish.combined,
        &["llm.news_research", "quant.momentum"],
        "0.086",
        "0.086",
        "0.94",
    );
    assert_eq!(
        hold_reason(&bearish.action),
        HoldReason::BetweenThresholds,
        "an output cut exactly at `now` is fresh, and this one is the latest"
    );
}

/// §8.2: `now − as_of ≤ max_output_age_s`, inclusive. `quant.momentum` is fresh for 900 seconds, so
/// an output cut exactly 900 seconds ago counts and one cut 901 seconds ago does not (MC-B10).
#[test]
#[ignore = "pending E6-2"]
fn an_output_at_exactly_max_output_age_is_fresh_and_one_second_later_is_not() {
    let mandate = two_stock_swing();
    let fresh_news = output(&news(), SWING_INSTRUMENT, "0.9", "0.9");
    let expiry = "2026-09-22T15:00:00.000000000Z";

    let at_the_bound = [
        timed_output(
            &momentum(),
            SWING_INSTRUMENT,
            "0.9",
            "0.9",
            "2026-09-22T13:45:00.000000000Z",
            expiry,
        ),
        fresh_news.clone(),
    ];
    let inside = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &at_the_bound,
    );
    assert_combined(
        &inside.combined,
        &["llm.news_research", "quant.momentum"],
        "0.81",
        "0.81",
        "0.9",
    );
    assert_sizes(&inside.sizes, "1500", "0", Some("1215"), Some("1215"));
    match &inside.action {
        Action::Buy { qty: q, .. } => assert_eq!(*q, qty("10")),
        other => panic!("a buy of 10, not {other:?}"),
    }

    let one_second_older = [
        timed_output(
            &momentum(),
            SWING_INSTRUMENT,
            "0.9",
            "0.9",
            "2026-09-22T13:44:59.000000000Z",
            expiry,
        ),
        fresh_news,
    ];
    let outside = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &one_second_older,
    );
    assert_combined(
        &outside.combined,
        &["llm.news_research"],
        "0.324",
        "-0.276",
        "0.36",
    );
    assert_sizes(&outside.sizes, "1500", "0", None, None);
    assert_eq!(hold_reason(&outside.action), HoldReason::BetweenThresholds);
}

/// §8.2: fresh is `now < expires_at`, so an output expiring exactly now is **not** fresh (MC-B06),
/// and one expiring a nanosecond later is.
#[test]
#[ignore = "pending E6-2"]
fn an_output_expiring_exactly_now_is_not_fresh() {
    let mandate = two_stock_swing();
    let momentum_output = output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9");
    let cut = "2026-09-22T13:59:00.000000000Z";

    let expiring_now = [
        momentum_output.clone(),
        timed_output(&news(), SWING_INSTRUMENT, "0.2", "0.5", cut, NOW),
    ];
    let expired = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &expiring_now,
    );
    assert_combined(
        &expired.combined,
        &["quant.momentum"],
        "0.432",
        "0.032",
        "0.54",
    );
    assert_sizes(&expired.sizes, "1500", "0", None, None);
    assert_eq!(hold_reason(&expired.action), HoldReason::BetweenThresholds);

    let expiring_next_nanosecond = [
        momentum_output,
        timed_output(
            &news(),
            SWING_INSTRUMENT,
            "0.2",
            "0.5",
            cut,
            "2026-09-22T14:00:00.000000001Z",
        ),
    ];
    let live = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &expiring_next_nanosecond,
    );
    assert_combined(
        &live.combined,
        &["llm.news_research", "quant.momentum"],
        "0.472",
        "0.472",
        "0.74",
    );
    match &live.action {
        Action::Buy { qty: q, .. } => assert_eq!(*q, qty("7")),
        other => panic!("a buy of 7, not {other:?}"),
    }
}

/// §8.1, DEC-67, MC-B12: an output whose version is not the pinned one is ignored and counts as
/// missing, which lowers the buy conviction rather than leaving it out of the average.
#[test]
#[ignore = "pending E6-2"]
fn a_wrong_model_version_counts_as_missing() {
    let mandate = two_stock_swing();
    let stale_version = ModelOutput {
        model_version: version("0.2.0"),
        ..output(&news(), SWING_INSTRUMENT, "0.9", "0.9")
    };
    let outputs = [
        output(&momentum(), SWING_INSTRUMENT, "0.9", "0.9"),
        stale_version,
    ];
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &proposal.combined,
        &["quant.momentum"],
        "0.486",
        "0.086",
        "0.54",
    );
    assert_sizes(&proposal.sizes, "1500", "0", None, None);
    assert_eq!(hold_reason(&proposal.action), HoldReason::BetweenThresholds);

    let pinned = [
        output(&momentum(), SWING_INSTRUMENT, "0.9", "0.9"),
        output(&news(), SWING_INSTRUMENT, "0.9", "0.9"),
    ];
    let accepted = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &pinned,
    );
    assert_combined(
        &accepted.combined,
        &["llm.news_research", "quant.momentum"],
        "0.81",
        "0.81",
        "0.9",
    );
}

/// §8.1, DEC-67, DEC-130 item 11: the pinned triple is compared **whole**. An output from a model
/// the mandate does not configure is ignored, and so is one whose id and version match but whose
/// content hash does not.
#[test]
#[ignore = "pending E6-2"]
fn an_unpinned_model_id_is_ignored() {
    let mandate = two_stock_swing();
    let unconfigured = ModelOutput {
        model_id: model("quant.mean_reversion"),
        model_version: version("1.0.0"),
        content_hash: digest(common::MEAN_REVERSION_HASH),
        ..output(&momentum(), SWING_INSTRUMENT, "-1", "1")
    };
    let wrong_hash = ModelOutput {
        content_hash: digest(common::MEAN_REVERSION_HASH),
        ..output(&news(), SWING_INSTRUMENT, "0.9", "0.9")
    };
    let outputs = [
        output(&momentum(), SWING_INSTRUMENT, "0.9", "0.9"),
        unconfigured,
        wrong_hash,
    ];
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &proposal.combined,
        &["quant.momentum"],
        "0.486",
        "0.086",
        "0.54",
    );
    assert_eq!(
        hold_reason(&proposal.action),
        HoldReason::BetweenThresholds,
        "neither the unconfigured model nor the mismatched hash entered the average"
    );

    let hash_corrected = [
        output(&momentum(), SWING_INSTRUMENT, "0.9", "0.9"),
        output(&news(), SWING_INSTRUMENT, "0.9", "0.9"),
    ];
    let accepted = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &hash_corrected,
    );
    assert_combined(
        &accepted.combined,
        &["llm.news_research", "quant.momentum"],
        "0.81",
        "0.81",
        "0.9",
    );
}

/// §8.2, MC-B11: only the latest fresh output per model counts, by `as_of`. The two halves swap the
/// two convictions, so the test pins which output was taken rather than one arithmetic result.
#[test]
#[ignore = "pending E6-2"]
fn duplicate_outputs_take_the_latest_as_of() {
    let mandate = two_stock_swing();
    let expiry = "2026-09-22T15:00:00.000000000Z";
    let newer_is_bullish = [
        timed_output(
            &momentum(),
            SWING_INSTRUMENT,
            "0.8",
            "0.9",
            "2026-09-22T13:59:00.000000000Z",
            expiry,
        ),
        output(&news(), SWING_INSTRUMENT, "0.2", "0.5"),
        timed_output(
            &momentum(),
            SWING_INSTRUMENT,
            "0.1",
            "0.9",
            "2026-09-22T13:58:00.000000000Z",
            expiry,
        ),
    ];
    let bullish = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &newer_is_bullish,
    );
    assert_combined(
        &bullish.combined,
        &["llm.news_research", "quant.momentum"],
        "0.472",
        "0.472",
        "0.74",
    );
    match &bullish.action {
        Action::Buy { qty: q, .. } => assert_eq!(*q, qty("7")),
        other => panic!("a buy of 7, not {other:?}"),
    }

    let newer_is_flat = [
        timed_output(
            &momentum(),
            SWING_INSTRUMENT,
            "0.1",
            "0.9",
            "2026-09-22T13:59:00.000000000Z",
            expiry,
        ),
        output(&news(), SWING_INSTRUMENT, "0.2", "0.5"),
        timed_output(
            &momentum(),
            SWING_INSTRUMENT,
            "0.8",
            "0.9",
            "2026-09-22T13:58:00.000000000Z",
            expiry,
        ),
    ];
    let flat = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &newer_is_flat,
    );
    assert_combined(
        &flat.combined,
        &["llm.news_research", "quant.momentum"],
        "0.094",
        "0.094",
        "0.74",
    );
    assert_eq!(hold_reason(&flat.action), HoldReason::BetweenThresholds);
}

/// §8.2, DEC-130 item 10: ties in `as_of` break by journal order, which is the index in the slice
/// the caller supplies. The later position wins.
#[test]
#[ignore = "pending E6-2"]
fn two_outputs_with_one_as_of_take_the_later_journal_position() {
    let mandate = two_stock_swing();
    let quiet = output(&momentum(), SWING_INSTRUMENT, "0.1", "0.9");
    let loud = output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9");
    let news_output = output(&news(), SWING_INSTRUMENT, "0.2", "0.5");

    let loud_last = [quiet.clone(), news_output.clone(), loud.clone()];
    let taken_loud = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &loud_last,
    );
    assert_combined(
        &taken_loud.combined,
        &["llm.news_research", "quant.momentum"],
        "0.472",
        "0.472",
        "0.74",
    );
    match &taken_loud.action {
        Action::Buy { qty: q, .. } => assert_eq!(*q, qty("7")),
        other => panic!("a buy of 7, not {other:?}"),
    }

    let quiet_last = [loud, news_output, quiet];
    let taken_quiet = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &quiet_last,
    );
    assert_combined(
        &taken_quiet.combined,
        &["llm.news_research", "quant.momentum"],
        "0.094",
        "0.094",
        "0.74",
    );
    assert_eq!(
        hold_reason(&taken_quiet.action),
        HoldReason::BetweenThresholds
    );
}

/// §8.3 step 1, MC-B07: a model without a fresh output counts as **0** for the exit conviction, so
/// an outage never forces a sell. The anchor is the buy conviction in the same call, which is
/// −0.886: if the exit used it the position would sell on a different number.
#[test]
#[ignore = "pending E6-2"]
fn a_missing_model_counts_as_zero_for_the_exit_conviction() {
    let mandate = two_stock_swing();
    let outputs = [output(&momentum(), SWING_INSTRUMENT, "-0.9", "0.9")];
    let proposal = proposed(
        &mandate,
        &account("5", "99.9"),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &proposal.combined,
        &["quant.momentum"],
        "-0.486",
        "-0.886",
        "0.54",
    );
    assert_sizes(&proposal.sizes, "1500", "499.5", None, None);
    match &proposal.action {
        Action::Sell {
            purpose,
            qty: q,
            limit_price,
            order_usd,
            shape,
        } => {
            assert_eq!(*purpose, Purpose::DiscretionaryExit);
            assert_eq!(*q, qty("5"), "the whole position");
            assert_eq!(*limit_price, price("99.9"), "at the bid");
            assert_eq!(*order_usd, usd("499.5"));
            assert_eq!(*shape, OrderShape::Limit);
        }
        other => panic!("a discretionary exit of 5 at 99.9, not {other:?}"),
    }
}

/// §8.3 step 1, MI-10, MC-B06: a model without a fresh output counts as **fully bearish** for the
/// buy conviction, so an outage never enlarges a buy. c is 0.432 and b is 0.032 in the same call.
#[test]
#[ignore = "pending E6-2"]
fn a_missing_model_counts_as_fully_bearish_for_buys() {
    let mandate = two_stock_swing();
    let outputs = [output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9")];
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &proposal.combined,
        &["quant.momentum"],
        "0.432",
        "0.032",
        "0.54",
    );
    assert_sizes(&proposal.sizes, "1500", "0", None, None);
    assert_eq!(
        hold_reason(&proposal.action),
        HoldReason::BetweenThresholds,
        "c is above the entry threshold and b is not; the buy reads b"
    );

    let both = [
        output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9"),
        output(&news(), SWING_INSTRUMENT, "0.2", "0.5"),
    ];
    let complete = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &both,
    );
    assert_combined(
        &complete.combined,
        &["llm.news_research", "quant.momentum"],
        "0.472",
        "0.472",
        "0.74",
    );
    match &complete.action {
        Action::Buy { qty: q, .. } => {
            assert_eq!(*q, qty("7"), "the same outputs plus the news model buy")
        }
        other => panic!("a buy of 7, not {other:?}"),
    }
}

/// §8.3 step 1: W is the sum over **all** configured models. With only the 0.4-weight model fresh,
/// dividing by the fresh weight would give c = 0.81 and s = 0.9; dividing by W = 1 gives 0.324 and
/// 0.36, and the agent holds instead of buying.
#[test]
#[ignore = "pending E6-2"]
fn the_denominator_counts_configured_models_not_fresh_ones() {
    let mandate = two_stock_swing();
    let outputs = [output(&news(), SWING_INSTRUMENT, "0.9", "0.9")];
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &proposal.combined,
        &["llm.news_research"],
        "0.324",
        "-0.276",
        "0.36",
    );
    assert_sizes(&proposal.sizes, "1500", "0", None, None);
    assert_eq!(hold_reason(&proposal.action), HoldReason::BetweenThresholds);
}

/// §8.3 step 1, MC-B13: the three figures round to 12 places **before** a rule compares them. At 13
/// places the score is 0.649999999999 96 and `low_score` would ASK; at 12 it is exactly 0.65 and
/// the order is AUTO by `routine`.
#[test]
#[ignore = "pending E6-2"]
fn the_score_rounds_to_twelve_places_before_the_rule_compares_it() {
    let mandate = two_stock_swing();
    let outputs = [
        output(&momentum(), SWING_INSTRUMENT, "1", "0.65"),
        output(&news(), SWING_INSTRUMENT, "1", "0.6499999999999"),
    ];
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &proposal.combined,
        &["llm.news_research", "quant.momentum"],
        "0.65",
        "0.65",
        "0.65",
    );
    assert_sizes(&proposal.sizes, "1500", "0", Some("975"), Some("975"));
    let action = match &proposal.action {
        Action::Buy {
            qty: q,
            order_usd,
            action,
            ..
        } => {
            assert_eq!(*q, qty("9"));
            assert_eq!(*order_usd, usd("900"));
            action.clone()
        }
        other => panic!("a buy of 9 at 100, not {other:?}"),
    };
    assert_eq!(action.combined_score, unit("0.65"));
    let decided = classified(&base_policy(), &action);
    assert_eq!(decided.decision, AutonomyDecision::Auto);
    assert_eq!(decided.by, DecidedBy::Rule(rule_id("routine")));

    let unrounded = ActionContext {
        combined_score: unit("0.649999999999"),
        ..action
    };
    assert_eq!(
        classified(&base_policy(), &unrounded).by,
        DecidedBy::Rule(rule_id("low_score")),
        "one place lower and the same rules ASK, which is what the rounding decides"
    );
}

/// §8.3 step 1, MC-B20, DEC-130 item 9: no fresh output holds, and still reports c = 0, b = −1 and
/// s = 0, so an outage is visible as an outage rather than as a blank.
#[test]
#[ignore = "pending E6-2"]
fn no_fresh_output_holds_and_reports_zero_minus_one_and_zero() {
    let proposal = proposed(
        &two_stock_swing(),
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &[],
    );
    assert_combined(&proposal.combined, &[], "0", "-1", "0");
    assert_sizes(&proposal.sizes, "1500", "0", None, None);
    assert_eq!(hold_reason(&proposal.action), HoldReason::NoFreshOutputs);
    assert!(
        proposal.clipped_by.is_empty(),
        "nothing was sized, so nothing was clipped"
    );
}

fn swing_outputs(momentum_conviction: &str, news_conviction: &str) -> Vec<ModelOutput> {
    vec![
        output(&momentum(), SWING_INSTRUMENT, momentum_conviction, "0.9"),
        output(&news(), SWING_INSTRUMENT, news_conviction, "0.5"),
    ]
}

/// A both-models-agree pair at confidence 1, so the combined figures are the conviction itself.
fn unanimous(conviction: &str) -> Vec<ModelOutput> {
    vec![
        output(&momentum(), SWING_INSTRUMENT, conviction, "1"),
        output(&news(), SWING_INSTRUMENT, conviction, "1"),
    ]
}

/// §8.3 step 2: the exit fires at c ≤ −`exit_threshold` — inclusive — and sells the **whole**
/// position at the bid. One place inside the threshold nothing is proposed, which is the anchor.
#[test]
#[ignore = "pending E6-2"]
fn an_exit_at_the_threshold_sells_the_whole_position() {
    let mandate = two_stock_swing();
    let at_threshold = proposed(
        &mandate,
        &account("5", "99.9"),
        &swing_market(),
        &quiet_risk(),
        &unanimous("-0.3"),
    );
    assert_combined(
        &at_threshold.combined,
        &["llm.news_research", "quant.momentum"],
        "-0.3",
        "-0.3",
        "1",
    );
    assert_sizes(&at_threshold.sizes, "1500", "499.5", None, None);
    match &at_threshold.action {
        Action::Sell {
            purpose,
            qty: q,
            limit_price,
            order_usd,
            shape,
        } => {
            assert_eq!(*purpose, Purpose::DiscretionaryExit);
            assert_eq!(*q, qty("5"));
            assert_eq!(*limit_price, price("99.9"));
            assert_eq!(*order_usd, usd("499.5"));
            assert_eq!(*shape, OrderShape::Limit);
        }
        other => panic!("a discretionary exit of the whole position, not {other:?}"),
    }

    let inside = proposed(
        &mandate,
        &account("5", "99.9"),
        &swing_market(),
        &quiet_risk(),
        &unanimous("-0.299999999999"),
    );
    assert_combined(
        &inside.combined,
        &["llm.news_research", "quant.momentum"],
        "-0.299999999999",
        "-0.299999999999",
        "1",
    );
    assert_eq!(
        hold_reason(&inside.action),
        HoldReason::BetweenThresholds,
        "one twelfth-place step inside the threshold and nothing is proposed"
    );
}

/// §8.3 step 2: with no position there is nothing to exit, and the reason says so rather than
/// reporting a sell of zero. The anchor is the same conviction with a position, which sells.
#[test]
#[ignore = "pending E6-2"]
fn a_flat_position_below_the_exit_threshold_holds() {
    let mandate = two_stock_swing();
    let flat = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &unanimous("-0.5"),
    );
    assert_combined(
        &flat.combined,
        &["llm.news_research", "quant.momentum"],
        "-0.5",
        "-0.5",
        "1",
    );
    assert_sizes(&flat.sizes, "1500", "0", None, None);
    assert_eq!(hold_reason(&flat.action), HoldReason::NoPosition);

    let held = proposed(
        &mandate,
        &account("5", "99.9"),
        &swing_market(),
        &quiet_risk(),
        &unanimous("-0.5"),
    );
    match &held.action {
        Action::Sell { qty: q, .. } => assert_eq!(*q, qty("5")),
        other => panic!("a sell of 5, not {other:?}"),
    }
}

/// §8.3 step 2 and §3.1, DEC-130 item 21: the `accumulate` check comes **before** the flat-position
/// check, so a flat accumulating agent holds `discretionary_exits_disabled` and never `no_position`.
/// The same inputs under a `profit_stop` goal give `no_position`, which is what makes the ordering
/// the thing this test pins.
#[test]
#[ignore = "pending E6-2"]
fn a_flat_accumulate_agent_below_the_exit_threshold_holds_exits_disabled() {
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "-0.9", "0.9")];
    let market = crypto_market("54990", "55000", "0.0001");
    let accumulating = proposed(
        &btc_accumulator(),
        &account("0", "54990"),
        &market,
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &accumulating.combined,
        &["quant.mean_reversion"],
        "-0.81",
        "-0.81",
        "0.9",
    );
    assert_sizes(&accumulating.sizes, "10000", "0", None, None);
    assert_eq!(
        hold_reason(&accumulating.action),
        HoldReason::DiscretionaryExitsDisabled
    );

    let continuous = BuilderMandate {
        goal: GoalKind::Continuous,
        ..btc_accumulator()
    };
    let flat = proposed(
        &continuous,
        &account("0", "54990"),
        &market,
        &quiet_risk(),
        &outputs,
    );
    assert_eq!(
        hold_reason(&flat.action),
        HoldReason::NoPosition,
        "the goal is what disabled the exit, not the empty position"
    );
}

/// §3.1, MC-B29: an `accumulate` agent never sells on a negative conviction, whatever its position.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_never_sells_on_negative_conviction() {
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "-0.9", "0.9")];
    let market = crypto_market("54990", "55000", "0.0001");
    let held = account("0.05", "54990");
    let accumulating = proposed(&btc_accumulator(), &held, &market, &quiet_risk(), &outputs);
    assert_combined(
        &accumulating.combined,
        &["quant.mean_reversion"],
        "-0.81",
        "-0.81",
        "0.9",
    );
    assert_sizes(&accumulating.sizes, "10000", "2749.5", None, None);
    assert_eq!(
        hold_reason(&accumulating.action),
        HoldReason::DiscretionaryExitsDisabled
    );

    let continuous = BuilderMandate {
        goal: GoalKind::Continuous,
        ..btc_accumulator()
    };
    let selling = proposed(&continuous, &held, &market, &quiet_risk(), &outputs);
    match &selling.action {
        Action::Sell {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.05"),
                "the same position sells under any other goal"
            );
            assert_eq!(*order_usd, usd("2749.5"));
        }
        other => panic!("a discretionary exit of 0.05, not {other:?}"),
    }
}

/// §8.3 step 2, MC-B03: between the thresholds nothing is proposed — no buy and no sell.
#[test]
#[ignore = "pending E6-2"]
fn between_the_thresholds_nothing_is_proposed() {
    let mandate = two_stock_swing();
    let held = account("5", "99.9");
    let quiet = proposed(
        &mandate,
        &held,
        &swing_market(),
        &quiet_risk(),
        &swing_outputs("0.3", "0.2"),
    );
    assert_combined(
        &quiet.combined,
        &["llm.news_research", "quant.momentum"],
        "0.202",
        "0.202",
        "0.74",
    );
    assert_sizes(&quiet.sizes, "1500", "499.5", None, None);
    assert_eq!(hold_reason(&quiet.action), HoldReason::BetweenThresholds);

    let bullish = proposed(
        &mandate,
        &held,
        &swing_market(),
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    assert!(
        matches!(bullish.action, Action::Buy { .. }),
        "0.472 is above the entry threshold, so the band is a band and not a floor"
    );
    let bearish = proposed(
        &mandate,
        &held,
        &swing_market(),
        &quiet_risk(),
        &swing_outputs("-0.8", "-0.2"),
    );
    assert!(
        matches!(bearish.action, Action::Sell { .. }),
        "−0.472 is below the exit threshold, so the band is bounded on both sides"
    );
}

/// §8.3 step 2: cap = min(`max_position_usd`, `max_position_fraction` × E), so which of the two
/// binds depends on the equity. At 10000 the dollar limit binds at 1500; at 5000 the fraction binds
/// at 1000, and the order is smaller for it.
#[test]
#[ignore = "pending E6-2"]
fn the_cap_is_the_lesser_of_the_dollar_and_fraction_limits() {
    let mandate = two_stock_swing();
    let outputs = swing_outputs("0.8", "0.2");
    let rich = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&rich.sizes, "1500", "0", Some("708"), Some("708"));
    match &rich.action {
        Action::Buy { qty: q, .. } => assert_eq!(*q, qty("7")),
        other => panic!("a buy of 7, not {other:?}"),
    }

    let smaller = AccountSnapshot {
        agent_equity: usd("5000"),
        ..flat_account()
    };
    let poor = proposed(&mandate, &smaller, &swing_market(), &quiet_risk(), &outputs);
    assert_sizes(&poor.sizes, "1000", "0", Some("472"), Some("472"));
    match &poor.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("4"));
            assert_eq!(*order_usd, usd("400"));
        }
        other => panic!("a buy of 4, not {other:?}"),
    }
    assert!(
        poor.clipped_by.is_empty(),
        "the cap shaped the target, it did not clip the order"
    );
}

/// §8.3 step 2, MC-B02: T = b × cap × size factor, so an active `scale_sizes` rung shrinks the
/// target and the order with it.
#[test]
#[ignore = "pending E6-2"]
fn the_ladder_size_factor_scales_the_target() {
    let mandate = two_stock_swing();
    let outputs = swing_outputs("0.8", "0.2");
    let scaled = RiskContext {
        size_factor: frac("0.5"),
        ..quiet_risk()
    };
    let halved = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &scaled,
        &outputs,
    );
    assert_sizes(&halved.sizes, "1500", "0", Some("354"), Some("354"));
    match &halved.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("3"));
            assert_eq!(*order_usd, usd("300"));
        }
        other => panic!("a buy of 3, not {other:?}"),
    }

    let full = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&full.sizes, "1500", "0", Some("708"), Some("708"));
    match &full.action {
        Action::Buy { qty: q, .. } => assert_eq!(*q, qty("7"), "factor 1 leaves the target alone"),
        other => panic!("a buy of 7, not {other:?}"),
    }
}

/// §8.3 step 3, MC-B15: Delta subtracts the max cost of the working **opening** orders, so an order
/// already resting counts toward the target and the agent does not buy the same exposure twice.
#[test]
#[ignore = "pending E6-2"]
fn a_working_opening_order_counts_toward_the_target() {
    let mandate = two_stock_swing();
    let outputs = swing_outputs("0.8", "0.2");
    let with_working = AccountSnapshot {
        gross_usd: usd("700"),
        working_opening_cost: usd("700"),
        ..flat_account()
    };
    let held_back = proposed(
        &mandate,
        &with_working,
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&held_back.sizes, "1500", "0", Some("708"), Some("8"));
    assert_eq!(
        hold_reason(&held_back.action),
        HoldReason::WithinRebalanceBand
    );

    let without = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&without.sizes, "1500", "0", Some("708"), Some("708"));
    match &without.action {
        Action::Buy { qty: q, .. } => {
            assert_eq!(*q, qty("7"), "the same target with no working order buys")
        }
        other => panic!("a buy of 7, not {other:?}"),
    }
}

/// §8.3 step 3, MC-B16: Delta ≤ 0 holds, and a positive conviction never produces a sell. There are
/// no signal trims in v1; positions shrink by exits and by `trim_to_target`, which is the risk
/// engine's (DEC-130 item 3).
#[test]
#[ignore = "pending E6-2"]
fn at_or_above_target_holds_and_never_sells() {
    let scaled = RiskContext {
        size_factor: frac("0.5"),
        ..quiet_risk()
    };
    let proposal = proposed(
        &two_stock_swing(),
        &account("10", "99.9"),
        &swing_market(),
        &scaled,
        &swing_outputs("0.8", "0.2"),
    );
    assert_combined(
        &proposal.combined,
        &["llm.news_research", "quant.momentum"],
        "0.472",
        "0.472",
        "0.74",
    );
    assert_sizes(&proposal.sizes, "1500", "999", Some("354"), Some("-645"));
    assert_eq!(hold_reason(&proposal.action), HoldReason::AtOrAboveTarget);
    assert!(
        !matches!(proposal.action, Action::Sell { .. }),
        "a position above its target is not trimmed by the builder"
    );
}

/// §8.3 step 3, MC-B18: Delta below `rebalance_band` × cap holds, and MV is taken at the **risk
/// mark**, not at the ask — 7 × 99.9 = 699.3, which is what makes Delta 8.7 rather than 8.
#[test]
#[ignore = "pending E6-2"]
fn a_delta_inside_the_band_holds() {
    let mandate = two_stock_swing();
    let outputs = swing_outputs("0.8", "0.2");
    let inside = proposed(
        &mandate,
        &account("7", "99.9"),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&inside.sizes, "1500", "699.3", Some("708"), Some("8.7"));
    assert_eq!(hold_reason(&inside.action), HoldReason::WithinRebalanceBand);

    let one_share_lighter = AccountSnapshot {
        gross_usd: usd("599.4"),
        ..account("6", "99.9")
    };
    let outside = proposed(
        &mandate,
        &one_share_lighter,
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&outside.sizes, "1500", "599.4", Some("708"), Some("108.6"));
    match &outside.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("1"),
                "108.6 is outside the 75 band, so the top-up is proposed"
            );
            assert_eq!(*order_usd, usd("100"));
        }
        other => panic!("a buy of 1, not {other:?}"),
    }
}

/// §8.3 step 3: the buy value is the least of Delta, `max_order_usd`, the cap headroom, and the
/// gross headroom, at the ask, truncated to the increment.
///
/// Three of the four can be strictly the smallest and each is exercised here. The **cap headroom
/// cannot**: T = b × cap × factor is at most cap, so cap − MV − working is never below
/// T − MV − working, and the two are equal exactly when b and the size factor are both one. The
/// last block asserts that equality rather than pretending to a fourth binding case, and the gross
/// headroom block also pins `min(max_gross_exposure_usd, E)`, which the equity reaches first.
#[test]
#[ignore = "pending E6-2"]
fn each_of_the_four_clips_binds_in_turn() {
    let mandate = two_stock_swing();
    let delta_binds = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    assert_sizes(&delta_binds.sizes, "1500", "0", Some("708"), Some("708"));
    match &delta_binds.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("7"), "Delta is the smallest of the four");
            assert_eq!(*order_usd, usd("700"));
        }
        other => panic!("a buy of 7, not {other:?}"),
    }
    assert!(delta_binds.clipped_by.is_empty(), "nothing clipped Delta");

    let order_binds = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &unanimous("1"),
    );
    assert_sizes(&order_binds.sizes, "1500", "0", Some("1500"), Some("1500"));
    match &order_binds.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("10"), "max_order_usd cuts 1500 to 1000");
            assert_eq!(*order_usd, usd("1000"));
        }
        other => panic!("a buy of 10, not {other:?}"),
    }
    assert_eq!(order_binds.clipped_by, [Clip::Limits].into_iter().collect());

    let crowded = AccountSnapshot {
        gross_usd: usd("1500"),
        ..flat_account()
    };
    let gross_binds = proposed(
        &mandate,
        &crowded,
        &swing_market(),
        &quiet_risk(),
        &unanimous("1"),
    );
    match &gross_binds.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("5"), "2000 of gross exposure less 1500 leaves 500");
            assert_eq!(*order_usd, usd("500"));
        }
        other => panic!("a buy of 5, not {other:?}"),
    }
    assert_eq!(gross_binds.clipped_by, [Clip::Limits].into_iter().collect());

    let thin_equity = AccountSnapshot {
        agent_equity: usd("1800"),
        gross_usd: usd("1700"),
        ..flat_account()
    };
    let equity_binds = proposed(
        &mandate,
        &thin_equity,
        &swing_market(),
        &quiet_risk(),
        &unanimous("1"),
    );
    assert_sizes(&equity_binds.sizes, "360", "0", Some("360"), Some("360"));
    match &equity_binds.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("1"),
                "the gross bound is min(2000, E = 1800) less 1700, so 100 and not 300"
            );
            assert_eq!(*order_usd, usd("100"));
        }
        other => panic!("a buy of 1, not {other:?}"),
    }

    let roomy = BuilderMandate {
        limits: Limits {
            max_order_usd: usd("2000"),
            max_gross_exposure_usd: usd("10000"),
            ..mandate.limits.clone()
        },
        ..mandate.clone()
    };
    let held = AccountSnapshot {
        gross_usd: usd("499.5"),
        ..account("5", "99.9")
    };
    let cap_headroom = proposed(
        &roomy,
        &held,
        &swing_market(),
        &quiet_risk(),
        &unanimous("1"),
    );
    assert_sizes(
        &cap_headroom.sizes,
        "1500",
        "499.5",
        Some("1500"),
        Some("1000.5"),
    );
    match &cap_headroom.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("10"),
                "at b = 1 and factor 1 the cap headroom equals Delta, so neither clips the other"
            );
            assert_eq!(*order_usd, usd("1000"));
        }
        other => panic!("a buy of 10, not {other:?}"),
    }
    assert!(
        cap_headroom.clipped_by.is_empty(),
        "the headroom did not cut Delta, so nothing is reported as clipped"
    );
}

/// §8.3 step 3, MC-B19: the band is compared against the value **after** the clips, not against
/// Delta alone, so a top-up the limits cut to nothing holds instead of going out tiny.
#[test]
#[ignore = "pending E6-2"]
fn a_value_below_the_band_after_clipping_holds() {
    let mandate = two_stock_swing();
    let nearly_full = AccountSnapshot {
        gross_usd: usd("1950"),
        ..flat_account()
    };
    let held = proposed(
        &mandate,
        &nearly_full,
        &swing_market(),
        &quiet_risk(),
        &unanimous("1"),
    );
    assert_combined(
        &held.combined,
        &["llm.news_research", "quant.momentum"],
        "1",
        "1",
        "1",
    );
    assert_sizes(&held.sizes, "1500", "0", Some("1500"), Some("1500"));
    assert_eq!(
        hold_reason(&held.action),
        HoldReason::BelowBandAfterClipping,
        "Delta is 1500, far outside the 75 band; the 50 of gross headroom is what falls inside it"
    );
    assert_eq!(held.clipped_by, [Clip::Limits].into_iter().collect());

    let roomier = AccountSnapshot {
        gross_usd: usd("1800"),
        ..flat_account()
    };
    let bought = proposed(
        &mandate,
        &roomier,
        &swing_market(),
        &quiet_risk(),
        &unanimous("1"),
    );
    match &bought.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("2"),
                "200 of headroom is above the band, so it goes out"
            );
            assert_eq!(*order_usd, usd("200"));
        }
        other => panic!("a buy of 2, not {other:?}"),
    }
}

/// §8.3 step 5: a value below the minimum order holds, even where the rebalance band admits it.
#[test]
#[ignore = "pending E6-2"]
fn a_value_below_the_minimum_order_holds() {
    let no_band = BuilderMandate {
        sizing: Sizing {
            rebalance_band: frac("0"),
            ..two_stock_swing().sizing
        },
        limits: Limits {
            max_order_usd: usd("150"),
            ..two_stock_swing().limits
        },
        ..two_stock_swing()
    };
    let high_minimum = Market {
        min_order_usd: usd("500"),
        ..swing_market()
    };
    let held = proposed(
        &no_band,
        &flat_account(),
        &high_minimum,
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    assert_sizes(&held.sizes, "1500", "0", Some("708"), Some("708"));
    assert_eq!(
        hold_reason(&held.action),
        HoldReason::BelowMinimumAfterClipping,
        "the band admits 100, and the minimum order does not"
    );
    assert_eq!(held.clipped_by, [Clip::Limits].into_iter().collect());

    let low_minimum = Market {
        min_order_usd: usd("50"),
        ..swing_market()
    };
    let bought = proposed(
        &no_band,
        &flat_account(),
        &low_minimum,
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    match &bought.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("1"));
            assert_eq!(*order_usd, usd("100"));
        }
        other => panic!("a buy of 1, not {other:?}"),
    }
}

/// §8.3 step 5, DEC-130 item 21: the guard is `n ≤ 0` **or** below the minimum order, not the
/// minimum alone, so a fully clipped budget can never become an order for nothing — not even where
/// `min_order_usd` and `rebalance_band` are both zero, which is the one place the minimum stops
/// guarding.
#[test]
#[ignore = "pending E6-2"]
fn a_zero_quantity_never_becomes_a_buy_at_a_zero_minimum_and_zero_band() {
    let base = two_stock_swing();
    let unguarded = BuilderMandate {
        sizing: Sizing {
            rebalance_band: frac("0"),
            ..base.sizing.clone()
        },
        limits: Limits {
            max_order_usd: usd("50"),
            ..base.limits.clone()
        },
        ..base.clone()
    };
    let free_market = Market {
        min_order_usd: usd("0"),
        ..swing_market()
    };
    let held = proposed(
        &unguarded,
        &flat_account(),
        &free_market,
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    assert_sizes(&held.sizes, "1500", "0", Some("708"), Some("708"));
    assert_eq!(
        hold_reason(&held.action),
        HoldReason::BelowMinimumAfterClipping
    );
    assert_eq!(held.clipped_by, [Clip::Limits].into_iter().collect());

    let one_share = BuilderMandate {
        limits: Limits {
            max_order_usd: usd("100"),
            ..base.limits
        },
        ..unguarded
    };
    let bought = proposed(
        &one_share,
        &flat_account(),
        &free_market,
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    match &bought.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("1"),
                "a budget of one share at a zero minimum is still an order"
            );
            assert_eq!(*order_usd, usd("100"));
        }
        other => panic!("a buy of 1, not {other:?}"),
    }
}

/// A `btc_accumulator` account: the position at the mark, the cost basis and goal spend stated, and
/// gross exposure equal to the position's value.
fn btc_account(
    position: &str,
    risk_mark: &str,
    cost_basis: &str,
    goal_spent: &str,
    gross: &str,
) -> AccountSnapshot {
    AccountSnapshot {
        agent_equity: usd("10000"),
        position_qty: qty(position),
        cost_basis: basis(cost_basis),
        risk_mark: mark(risk_mark),
        gross_usd: usd(gross),
        working_opening_cost: usd("0"),
        goal_spent_usd: usd(goal_spent),
    }
}

fn accumulating(
    target_qty: &str,
    max_avg_price: Option<&str>,
    max_spend_usd: &str,
) -> BuilderMandate {
    BuilderMandate {
        goal: GoalKind::Accumulate(AccumulateGoal {
            instrument: asset(BTC_INSTRUMENT),
            target_qty: qty(target_qty),
            max_avg_price: max_avg_price.map(price),
            max_spend_usd: usd(max_spend_usd),
        }),
        ..btc_accumulator()
    }
}

/// §8.3 step 4, MC-B26: `n ≤ (target_qty − position) ÷ β` truncated to the increment. At a target of
/// 0.15 with 0.14 held the remaining quantity is what binds; at 0.2 it is not, and the spend clip
/// leaves a much larger order.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_clipped_to_the_remaining_target_quantity() {
    let market = crypto_market("54990", "55000", "0.0001");
    let account = btc_account("0.14", "54990", "7700", "7700", "7698.6");
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "1", "1")];

    let clipped = proposed(
        &btc_accumulator(),
        &account,
        &market,
        &quiet_risk(),
        &outputs,
    );
    assert_combined(&clipped.combined, &["quant.mean_reversion"], "1", "1", "1");
    assert_sizes(
        &clipped.sizes,
        "10000",
        "7698.6",
        Some("10000"),
        Some("2301.4"),
    );
    match &clipped.action {
        Action::Buy {
            purpose,
            qty: q,
            limit_price,
            order_usd,
            ..
        } => {
            assert_eq!(*purpose, Purpose::Increase);
            assert_eq!(*q, qty("0.01"), "0.15 less the 0.14 held");
            assert_eq!(*limit_price, price("55000"));
            assert_eq!(*order_usd, usd("550"));
        }
        other => panic!("a buy of 0.01 at 55000, not {other:?}"),
    }
    assert_eq!(
        clipped.clipped_by,
        [Clip::Limits, Clip::Goal].into_iter().collect()
    );

    let further = accumulating("0.2", Some("58000"), "9000");
    let unclipped = proposed(&further, &account, &market, &quiet_risk(), &outputs);
    match &unclipped.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.0181"),
                "the goal no longer binds, so the limits' budget stands"
            );
            assert_eq!(*order_usd, usd("995.5"));
        }
        other => panic!("a buy of 0.0181, not {other:?}"),
    }
    assert_eq!(unclipped.clipped_by, [Clip::Limits].into_iter().collect());
}

/// §8.3 step 4: `n ≤ (max_spend_usd − goal spend) ÷ a`, truncated to the increment. The goal here
/// carries no `max_avg_price`, so the spend is the only bound that can bind.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_clipped_by_max_spend() {
    let market = crypto_market("54990", "55000", "0.0001");
    let mandate = accumulating("0.15", None, "9000");
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "1", "1")];

    let nearly_spent = btc_account("0.14", "54990", "7700", "8900", "7698.6");
    let clipped = proposed(&mandate, &nearly_spent, &market, &quiet_risk(), &outputs);
    assert_sizes(
        &clipped.sizes,
        "10000",
        "7698.6",
        Some("10000"),
        Some("2301.4"),
    );
    match &clipped.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("0.0018"), "100 of spend left at 55000 a unit");
            assert_eq!(*order_usd, usd("99"));
        }
        other => panic!("a buy of 0.0018, not {other:?}"),
    }
    assert_eq!(
        clipped.clipped_by,
        [Clip::Limits, Clip::Goal].into_iter().collect()
    );

    let unspent = btc_account("0.14", "54990", "7700", "0", "7698.6");
    let by_quantity = proposed(&mandate, &unspent, &market, &quiet_risk(), &outputs);
    match &by_quantity.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.01"),
                "with the spend unbinding, the remaining quantity binds"
            );
            assert_eq!(*order_usd, usd("550"));
        }
        other => panic!("a buy of 0.01, not {other:?}"),
    }
}

/// §8.3 step 4, MC-B27: `n ≤ (max_avg_price × position − cost basis) ÷ (a − max_avg_price × β)`.
/// Dropping `max_avg_price` from the same goal leaves an order three times the size.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_clipped_by_max_avg_price() {
    let market = crypto_market("59990", "60000", "0.0001");
    let account = btc_account("0.05", "59990", "2890", "2890", "2999.5");
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "0.9", "0.9")];

    let clipped = proposed(
        &btc_accumulator(),
        &account,
        &market,
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &clipped.combined,
        &["quant.mean_reversion"],
        "0.81",
        "0.81",
        "0.9",
    );
    assert_sizes(
        &clipped.sizes,
        "10000",
        "2999.5",
        Some("8100"),
        Some("5100.5"),
    );
    match &clipped.action {
        Action::Buy {
            qty: q,
            limit_price,
            order_usd,
            ..
        } => {
            assert_eq!(
                *q,
                qty("0.005"),
                "10 of headroom under the average over a 2000 denominator"
            );
            assert_eq!(*limit_price, price("60000"));
            assert_eq!(*order_usd, usd("300"));
        }
        other => panic!("a buy of 0.005 at 60000, not {other:?}"),
    }
    assert_eq!(
        clipped.clipped_by,
        [Clip::Limits, Clip::Goal].into_iter().collect()
    );

    let unbounded_average = accumulating("0.15", None, "9000");
    let unclipped = proposed(
        &unbounded_average,
        &account,
        &market,
        &quiet_risk(),
        &outputs,
    );
    match &unclipped.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.0166"),
                "without the average bound the limits' budget stands"
            );
            assert_eq!(*order_usd, usd("996"));
        }
        other => panic!("a buy of 0.0166, not {other:?}"),
    }
    assert_eq!(unclipped.clipped_by, [Clip::Limits].into_iter().collect());
}

/// §8.3 step 4: the `max_avg_price` clip applies only when `a − max_avg_price × β > 0`. At an ask of
/// 60000 against a 58000 average the denominator is 2000 and the clip binds at 0.005; at 55000 it is
/// −3000, the clip is left off, and the same goal admits an order ten times the size — because
/// buying below the average can only lower it.
#[test]
#[ignore = "pending E6-2"]
fn a_max_avg_price_denominator_that_is_not_positive_leaves_the_clip_off() {
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "1", "1")];

    let dear = proposed(
        &btc_accumulator(),
        &btc_account("0.05", "59990", "2890", "2890", "2999.5"),
        &crypto_market("59990", "60000", "0.0001"),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(
        &dear.sizes,
        "10000",
        "2999.5",
        Some("10000"),
        Some("7000.5"),
    );
    match &dear.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.005"),
                "60000 − 58000 is positive, so the clip binds"
            );
            assert_eq!(*order_usd, usd("300"));
        }
        other => panic!("a buy of 0.005, not {other:?}"),
    }
    assert_eq!(
        dear.clipped_by,
        [Clip::Limits, Clip::Goal].into_iter().collect()
    );

    let cheap = proposed(
        &btc_accumulator(),
        &btc_account("0.05", "54990", "2890", "2890", "2749.5"),
        &crypto_market("54990", "55000", "0.0001"),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(
        &cheap.sizes,
        "10000",
        "2749.5",
        Some("10000"),
        Some("7250.5"),
    );
    match &cheap.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.0181"),
                "55000 − 58000 is negative, so the clip is left off"
            );
            assert_eq!(*order_usd, usd("995.5"));
        }
        other => panic!("a buy of 0.0181, not {other:?}"),
    }
    assert_eq!(cheap.clipped_by, [Clip::Limits].into_iter().collect());
}

/// §8.3 step 4: the projected average is checked **after** the clips, so it still catches a buy the
/// clip did not cut — here the `max_avg_price` clip is off because the denominator is not positive,
/// and the position's existing average is already above the bound.
#[test]
#[ignore = "pending E6-2"]
fn a_projected_average_above_max_avg_price_holds() {
    let market = crypto_market("54990", "55000", "0.0001");
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "1", "1")];

    let dear_basis = btc_account("0.14", "54990", "8300", "8300", "7698.6");
    let held = proposed(
        &btc_accumulator(),
        &dear_basis,
        &market,
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(
        &held.sizes,
        "10000",
        "7698.6",
        Some("10000"),
        Some("2301.4"),
    );
    assert_eq!(
        hold_reason(&held.action),
        HoldReason::WouldExceedMaxAvgPrice,
        "8850 of basis over 0.15 is above 58000, and no clip cut the order to fix it"
    );
    assert_eq!(
        held.clipped_by,
        [Clip::Limits, Clip::Goal].into_iter().collect()
    );

    let cheap_basis = btc_account("0.14", "54990", "7700", "7700", "7698.6");
    let bought = proposed(
        &btc_accumulator(),
        &cheap_basis,
        &market,
        &quiet_risk(),
        &outputs,
    );
    match &bought.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.01"),
                "8250 over 0.15 is under the bound, so the same order goes out"
            );
            assert_eq!(*order_usd, usd("550"));
        }
        other => panic!("a buy of 0.01, not {other:?}"),
    }
}

/// §8.3 step 4, MC-B28: the fees are inside the clips. The asset fee makes the quantity received per
/// unit β = 1 − 0.0025, so reaching 0.15 takes 0.010025 rather than 0.01; the cash fee makes the
/// per-unit cost a = ask × 1.1, so a spend bound buys fewer units than the ask alone would say.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_with_fees_counts_the_spend_and_the_quantity_received() {
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "1", "1")];
    let account = btc_account("0.14", "54990", "7700", "7700", "7698.6");
    let fine_market = Market {
        increment: qty("0.000001"),
        ..crypto_market("54990", "55000", "0.0001")
    };

    let asset_fee = Market {
        fee_rate_asset: fee("0.0025"),
        ..fine_market.clone()
    };
    let with_asset_fee = proposed(
        &btc_accumulator(),
        &account,
        &asset_fee,
        &quiet_risk(),
        &outputs,
    );
    match &with_asset_fee.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.010025"),
                "0.01 ÷ 0.9975 truncated to the increment"
            );
            assert_eq!(*order_usd, usd("551.375"));
        }
        other => panic!("a buy of 0.010025, not {other:?}"),
    }
    assert_eq!(
        with_asset_fee.clipped_by,
        [Clip::Limits, Clip::Goal].into_iter().collect()
    );

    let free = proposed(
        &btc_accumulator(),
        &account,
        &fine_market,
        &quiet_risk(),
        &outputs,
    );
    match &free.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.01"),
                "without the fee the same goal needs 0.01 exactly"
            );
            assert_eq!(*order_usd, usd("550"));
        }
        other => panic!("a buy of 0.01, not {other:?}"),
    }

    let further = accumulating("0.2", Some("58000"), "9000");
    let part_spent = btc_account("0.14", "54990", "7700", "8000", "7698.6");
    let cash_fee = Market {
        fee_rate_cash: fee("0.1"),
        ..fine_market.clone()
    };
    let with_cash_fee = proposed(&further, &part_spent, &cash_fee, &quiet_risk(), &outputs);
    match &with_cash_fee.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.016528"),
                "1000 of spend left at 60500 a unit, not at 55000"
            );
            assert_eq!(*order_usd, usd("909.04"));
        }
        other => panic!("a buy of 0.016528, not {other:?}"),
    }
    assert_eq!(
        with_cash_fee.clipped_by,
        [Clip::Limits, Clip::Goal].into_iter().collect()
    );

    let no_cash_fee = proposed(&further, &part_spent, &fine_market, &quiet_risk(), &outputs);
    match &no_cash_fee.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(
                *q,
                qty("0.018181"),
                "without the cash fee the spend no longer binds"
            );
            assert_eq!(*order_usd, usd("999.955"));
        }
        other => panic!("a buy of 0.018181, not {other:?}"),
    }
    assert_eq!(no_cash_fee.clipped_by, [Clip::Limits].into_iter().collect());
}

/// A proposal carrying a buy of the stated value and score, with every other reported figure fixed.
/// Built by hand so the `decide` tests do not depend on `propose`: §6.2 step 2 takes the verdict as
/// a value, and these tests are about what it does with it.
fn buy_proposal(order_usd: &str, combined_score: &str) -> Proposal {
    Proposal {
        action: Action::Buy {
            purpose: Purpose::Open,
            qty: qty("8"),
            limit_price: price("100"),
            order_usd: usd(order_usd),
            action: ActionContext {
                order_usd: usd(order_usd),
                combined_score: unit(combined_score),
                instrument: asset(SWING_INSTRUMENT),
                asset_class: AssetClass::UsEquity,
                session: MarketSession::Regular,
                first_trade_in_instrument: true,
                position_usd_after: usd(order_usd),
                gross_usd_after: usd(order_usd),
                bought_today_usd: usd(order_usd),
                ..opening(order_usd, combined_score)
            },
        },
        combined: Combined {
            outputs_used: ids(&["llm.news_research", "quant.momentum"]),
            exit_conviction: conviction_of("0.54"),
            buy_conviction: conviction_of("0.54"),
            score: unit(combined_score),
        },
        sizes: Sizes {
            cap: UsdExact::parse("1500").unwrap_or_else(|e| panic!("1500 is exact: {e}")),
            current_mv: UsdExact::zero(),
            target_value: Some(
                UsdExact::parse("810").unwrap_or_else(|e| panic!("810 is exact: {e}")),
            ),
            delta: Some(UsdExact::parse("810").unwrap_or_else(|e| panic!("810 is exact: {e}"))),
        },
        clipped_by: Default::default(),
    }
}

fn conviction_of(text: &str) -> mandate_num::Conviction {
    mandate_num::Conviction::parse(text).unwrap_or_else(|e| panic!("`{text}` is a conviction: {e}"))
}

/// §6.2 step 2, MC-B21, DEC-05: a `deny` dry run skips the action, and **no approval is requested**
/// for an order the gate would deny. The same proposal under `allow` reaches the rules and ASKs, so
/// the test pins the skip rather than a proposal nobody would have asked about.
#[test]
#[ignore = "pending E6-2"]
fn a_gate_deny_skips_and_asks_nobody() {
    let proposal = buy_proposal("800", "0.6");
    let policy = base_policy();
    let denied = decide(&policy, &proposal, GateVerdict::Deny)
        .unwrap_or_else(|e| panic!("decide returns an outcome, not {e}"));
    assert_eq!(denied, Outcome::Skipped);

    let allowed = decide(&policy, &proposal, GateVerdict::Allow)
        .unwrap_or_else(|e| panic!("decide returns an outcome, not {e}"));
    match allowed {
        Outcome::Classified(decision) => {
            assert_eq!(decision.decision, AutonomyDecision::Ask);
            assert_eq!(decision.by, DecidedBy::Rule(rule_id("low_score")));
            assert_eq!(
                decision.approval.map(|a| a.approvers_required.get()),
                Some(1),
                "this is the approval the deny above must not have requested"
            );
        }
        other => panic!("an allowed proposal is classified, not {other:?}"),
    }
}

/// §6.2 step 2, MC-B22, DEC-48: a `defer` verdict stores nothing and never becomes a deny. The three
/// verdicts give three different outcomes, which is what stops one reading as another.
#[test]
#[ignore = "pending E6-2"]
fn a_defer_verdict_stores_nothing_and_never_becomes_a_deny() {
    let proposal = buy_proposal("800", "0.6");
    let policy = base_policy();
    let deferred = decide(&policy, &proposal, GateVerdict::Defer)
        .unwrap_or_else(|e| panic!("decide returns an outcome, not {e}"));
    assert_eq!(deferred, Outcome::Deferred);
    assert_ne!(deferred, Outcome::Skipped, "a defer is not a skip");

    assert_eq!(
        decide(&policy, &proposal, GateVerdict::Deny)
            .unwrap_or_else(|e| panic!("decide returns an outcome, not {e}")),
        Outcome::Skipped
    );
    assert!(
        matches!(
            decide(&policy, &proposal, GateVerdict::Allow),
            Ok(Outcome::Classified(_))
        ),
        "and an allow is neither"
    );
}

/// §9.6, DEC-70, MC-B23: an equity discretionary exit inside the close window is proposed as a
/// marketable limit order. Outside the window the same exit is a plain limit.
#[test]
#[ignore = "pending E6-2"]
fn a_discretionary_exit_in_the_close_window_is_a_marketable_limit() {
    let mandate = two_stock_swing();
    let held = account("5", "99.9");
    let outputs = swing_outputs("-0.8", "-0.2");
    let closing = Market {
        in_close_window: true,
        ..swing_market()
    };
    let marketable = proposed(&mandate, &held, &closing, &quiet_risk(), &outputs);
    assert_combined(
        &marketable.combined,
        &["llm.news_research", "quant.momentum"],
        "-0.472",
        "-0.472",
        "0.74",
    );
    assert_sizes(&marketable.sizes, "1500", "499.5", None, None);
    match &marketable.action {
        Action::Sell {
            shape,
            qty: q,
            limit_price,
            order_usd,
            ..
        } => {
            assert_eq!(*shape, OrderShape::MarketableLimit);
            assert_eq!(*q, qty("5"));
            assert_eq!(
                *limit_price,
                price("99.9"),
                "the price is still the bid, not a collar"
            );
            assert_eq!(*order_usd, usd("499.5"));
        }
        other => panic!("a marketable-limit exit of 5, not {other:?}"),
    }

    let open_session = proposed(&mandate, &held, &swing_market(), &quiet_risk(), &outputs);
    match &open_session.action {
        Action::Sell { shape, .. } => assert_eq!(
            *shape,
            OrderShape::Limit,
            "outside the window the same exit rests as a plain limit"
        ),
        other => panic!("a limit exit, not {other:?}"),
    }
}

/// §9.6, DEC-130 item 15: crypto has no regular session and no close window, so a crypto
/// discretionary exit is a plain limit even with `in_close_window` set, and in any session.
#[test]
#[ignore = "pending E6-2"]
fn a_crypto_discretionary_exit_is_a_plain_limit_in_any_session() {
    let continuous = BuilderMandate {
        goal: GoalKind::Continuous,
        ..btc_accumulator()
    };
    let held = account("0.05", "54990");
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "-0.9", "0.9")];
    let closing = Market {
        in_close_window: true,
        ..crypto_market("54990", "55000", "0.0001")
    };
    let proposal = proposed(&continuous, &held, &closing, &quiet_risk(), &outputs);
    assert_combined(
        &proposal.combined,
        &["quant.mean_reversion"],
        "-0.81",
        "-0.81",
        "0.9",
    );
    assert_sizes(&proposal.sizes, "10000", "2749.5", None, None);
    match &proposal.action {
        Action::Sell {
            shape,
            qty: q,
            limit_price,
            order_usd,
            ..
        } => {
            assert_eq!(*shape, OrderShape::Limit);
            assert_eq!(*q, qty("0.05"));
            assert_eq!(*limit_price, price("54990"));
            assert_eq!(*order_usd, usd("2749.5"));
        }
        other => panic!("a plain-limit crypto exit, not {other:?}"),
    }

    let equity_closing = Market {
        in_close_window: true,
        ..swing_market()
    };
    let equity = proposed(
        &two_stock_swing(),
        &account("5", "99.9"),
        &equity_closing,
        &quiet_risk(),
        &swing_outputs("-0.8", "-0.2"),
    );
    match &equity.action {
        Action::Sell { shape, .. } => assert_eq!(
            *shape,
            OrderShape::MarketableLimit,
            "the same flag on an equity does change the shape, so the crypto answer is the rule and not the default"
        ),
        other => panic!("a marketable-limit equity exit, not {other:?}"),
    }
}

/// DEC-130 item 8: a weight beyond 12 fractional places is `too_precise` at the type boundary, so
/// the mandate cannot be built and nothing is proposed. Exactly 12 places is inside the budget, and
/// the second half sizes an order off two such weights.
#[test]
#[ignore = "pending E6-2"]
fn a_weight_beyond_twelve_places_is_refused() {
    assert_eq!(
        SizeFraction::parse("0.0000000000001").map_err(|e| e.code()),
        Err("too_precise"),
        "13 fractional places is one more than the sizing chain can carry exactly"
    );
    assert!(
        SizeFraction::parse("0.000000000001").is_ok(),
        "12 places is the bound, and it is inclusive"
    );

    let fine_weights = BuilderMandate {
        models: vec![
            SignalModel {
                weight: frac("0.999999999999"),
                ..news()
            },
            SignalModel {
                weight: frac("0.000000000001"),
                ..momentum()
            },
        ],
        ..two_stock_swing()
    };
    let outputs = [output(&news(), SWING_INSTRUMENT, "1", "1")];
    let proposal = proposed(
        &fine_weights,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(
        &proposal.combined,
        &["llm.news_research"],
        "0.999999999999",
        "0.999999999998",
        "0.999999999999",
    );
    assert_sizes(
        &proposal.sizes,
        "1500",
        "0",
        Some("1499.999999997"),
        Some("1499.999999997"),
    );
    match &proposal.action {
        Action::Buy {
            qty: q, order_usd, ..
        } => {
            assert_eq!(*q, qty("10"));
            assert_eq!(*order_usd, usd("1000"));
        }
        other => panic!("a buy of 10 clipped to max_order_usd, not {other:?}"),
    }
}

/// DEC-130 item 8: a confidence or conviction beyond 18 fractional places is `too_precise`. At
/// exactly 18 the combine step still rounds once, to 12 places, which is what turns
/// 0.5999999999999999994 into 0.6 and 0.1999999999999999994 into 0.2.
#[test]
#[ignore = "pending E6-2"]
fn a_confidence_beyond_eighteen_places_is_refused() {
    assert_eq!(
        Unit::parse("0.0000000000000000001").map_err(|e| e.code()),
        Err("too_precise"),
        "19 places is beyond a confidence"
    );
    assert_eq!(
        mandate_num::Conviction::parse("-0.0000000000000000001").map_err(|e| e.code()),
        Err("too_precise"),
        "and beyond a conviction"
    );
    assert!(
        Unit::parse("0.999999999999999999").is_ok(),
        "18 places is the bound"
    );

    let outputs = [output(
        &momentum(),
        SWING_INSTRUMENT,
        "1",
        "0.999999999999999999",
    )];
    let proposal = proposed(
        &two_stock_swing(),
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_combined(&proposal.combined, &["quant.momentum"], "0.6", "0.2", "0.6");
    assert_sizes(&proposal.sizes, "1500", "0", None, None);
    assert_eq!(
        hold_reason(&proposal.action),
        HoldReason::BetweenThresholds,
        "b rounds to 0.2, which is below the 0.3 entry threshold"
    );
}

/// DEC-130 item 17: a crossed quote refuses the proposal rather than sizing an order against a
/// market that does not exist. A bid equal to the ask is not crossed and proposes as usual.
#[test]
#[ignore = "pending E6-2"]
fn a_crossed_quote_refuses_the_proposal() {
    let crossed = Market {
        bid: price("100.01"),
        ask: price("100"),
        ..swing_market()
    };
    assert_eq!(
        propose(
            &two_stock_swing(),
            &flat_account(),
            &crossed,
            &quiet_risk(),
            &swing_outputs("0.8", "0.2"),
            common::at(NOW),
        )
        .map_err(|e| e.code()),
        Err("crossed_quote")
    );

    let locked = Market {
        bid: price("100"),
        ask: price("100"),
        ..swing_market()
    };
    let proposal = proposed(
        &two_stock_swing(),
        &AccountSnapshot {
            risk_mark: mark("100"),
            ..flat_account()
        },
        &locked,
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    assert_sizes(&proposal.sizes, "1500", "0", Some("708"), Some("708"));
    match &proposal.action {
        Action::Buy { qty: q, .. } => {
            assert_eq!(*q, qty("7"), "a locked market is not a crossed one")
        }
        other => panic!("a buy of 7, not {other:?}"),
    }
}

/// §6.3: the three exposure fields a rule reads are the order's **after** values, and the thesis and
/// risk facts come from the [`RiskContext`] the caller supplied, not from anywhere else.
///
/// Pinned by hand as well as by `properties::position_and_gross_after_include_this_order`, because
/// the independent review of this PR found that four bugs here — any of the three after-values
/// leaving out this order or the working cost, and `new_instrument` or `thesis_confidence` not
/// copied — survived all 125 tests while the generator reached no buy at all.
#[test]
#[ignore = "pending E6-2"]
fn the_after_values_a_rule_reads_include_this_order() {
    let mandate = two_stock_swing();
    let account = AccountSnapshot {
        gross_usd: usd("400"),
        working_opening_cost: usd("100"),
        ..account("3", "99.9")
    };
    let risk = RiskContext {
        new_instrument: true,
        thesis_confidence: unit("0.77"),
        drawdown: unit("0.02"),
        daily_pnl_fraction: signed("-0.01"),
        position_pnl_fraction: signed("0.05"),
        bought_today_usd: usd("250"),
        has_prior_fill: true,
        ..quiet_risk()
    };
    let proposal = proposed(
        &mandate,
        &account,
        &swing_market(),
        &risk,
        &swing_outputs("0.8", "0.2"),
    );
    assert_sizes(&proposal.sizes, "1500", "299.7", Some("708"), Some("308.3"));
    let Action::Buy {
        qty: quantity,
        order_usd,
        action,
        ..
    } = &proposal.action
    else {
        panic!("a buy, not {:?}", proposal.action);
    };
    assert_eq!(
        *quantity,
        qty("3"),
        "308.3 of Delta buys three shares at 100"
    );
    assert_eq!(*order_usd, usd("300"));
    assert_eq!(
        action.order_usd,
        usd("300"),
        "the rule reads this order's value"
    );
    assert_eq!(
        action.position_usd_after,
        usd("699.7"),
        "299.7 of position at the mark, plus 100 of working cost, plus this 300"
    );
    assert_eq!(
        action.gross_usd_after,
        usd("700"),
        "400 of gross plus this 300"
    );
    assert_eq!(
        action.bought_today_usd,
        usd("550"),
        "250 bought today plus this 300"
    );
    assert_eq!(action.combined_score, unit("0.74"));
    assert!(
        action.new_instrument,
        "the flag is the caller's, not derived"
    );
    assert_eq!(action.thesis_confidence, unit("0.77"));
    assert_eq!(action.drawdown, unit("0.02"));
    assert_eq!(action.daily_pnl_fraction, signed("-0.01"));
    assert_eq!(action.position_pnl_fraction, signed("0.05"));
    assert!(!action.first_trade_in_instrument, "a prior fill was stated");
    assert_eq!(action.asset_class, AssetClass::UsEquity);
    assert_eq!(action.session, MarketSession::Regular);
    assert_eq!(action.instrument, asset(SWING_INSTRUMENT));
    assert_eq!(action.purpose, Purpose::Increase);
}

/// §6.2 step 3 and `AGENTS.md` rule 13: the built-in AUTO is reached **before** the order path
/// re-checks the rules, so a malformed rule set is never a reason a risk-reducing action is refused.
///
/// Validating first passes every other test in this suite, because every refusal test uses `Open`
/// and every reducing-purpose test uses a well-typed policy — the gap the independent review found.
#[test]
#[ignore = "pending E6-2"]
fn a_malformed_rule_set_never_blocks_a_reducing_purpose() {
    let reserved = policy(
        vec![Rule {
            id: rule_id("drift"),
            when: compare(ConditionField::UnusualInput, Operator::Eq, flag(true)),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Deny,
        AutonomyDecision::Deny,
        None,
    );
    let too_deep = policy(
        vec![Rule {
            id: rule_id("nested"),
            when: Condition::All(vec![Condition::All(vec![Condition::Any(vec![
                Condition::Not(Box::new(compare(
                    ConditionField::OrderUsd,
                    Operator::Gte,
                    decimal("0"),
                ))),
            ])])]),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Deny,
        AutonomyDecision::Deny,
        None,
    );
    let mistyped = policy(
        vec![Rule {
            id: rule_id("mistyped"),
            when: compare(ConditionField::Purpose, Operator::Gt, text("open")),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Deny,
        AutonomyDecision::Deny,
        None,
    );
    for (name, broken) in [
        ("a reserved field", &reserved),
        ("a fifth level of nesting", &too_deep),
        ("an ill-typed comparison", &mistyped),
    ] {
        for purpose in [
            Purpose::DiscretionaryExit,
            Purpose::OwnerExit,
            Purpose::RiskExit,
            Purpose::Protective,
        ] {
            let decided = classified(broken, &reducing(purpose));
            assert_eq!(
                decided.decision,
                AutonomyDecision::Auto,
                "{purpose:?} under {name}"
            );
            assert_eq!(
                decided.by,
                DecidedBy::BuiltinRiskReducing,
                "{purpose:?} under {name}"
            );
        }
        assert!(
            classify(broken, &opening("300", "0.8")).is_err(),
            "the same policy does refuse an opening action, so {name} is reached on that path"
        );
    }
}

/// §8.2, §8.3: only **this instrument's** outputs count. An output naming another is refused rather
/// than counted, because counting it would let one instrument's conviction open a position in
/// another, and ignoring it silently would be indistinguishable from a model that did not answer
/// (DEC-85).
#[test]
#[ignore = "pending E6-2"]
fn an_output_for_another_instrument_is_refused() {
    let mandate = two_stock_swing();
    let strayed = [
        output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9"),
        output(&news(), OTHER_INSTRUMENT, "0.2", "0.5"),
    ];
    assert_eq!(
        propose(
            &mandate,
            &flat_account(),
            &swing_market(),
            &quiet_risk(),
            &strayed,
            common::at(NOW),
        )
        .map_err(|e| e.code()),
        Err("output_instrument_mismatch")
    );
    assert_eq!(
        combine(
            &mandate.models,
            &strayed,
            &asset(SWING_INSTRUMENT),
            common::at(NOW),
        )
        .map_err(|e| e.code()),
        Err("output_instrument_mismatch"),
        "the rule has one home, in `combine`"
    );

    let matched = [
        output(&momentum(), SWING_INSTRUMENT, "0.8", "0.9"),
        output(&news(), SWING_INSTRUMENT, "0.2", "0.5"),
    ];
    let proposal = proposed(
        &mandate,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &matched,
    );
    assert_combined(
        &proposal.combined,
        &["llm.news_research", "quant.momentum"],
        "0.472",
        "0.472",
        "0.74",
    );
    match &proposal.action {
        Action::Buy { qty: quantity, .. } => assert_eq!(*quantity, qty("7")),
        other => panic!("a buy of 7, not {other:?}"),
    }
}

/// §8.3 step 4: the projected-average guard decides on its own, with **no** goal clip binding. The
/// `max_avg_price` clip is off because `a − max_avg × β` is not positive, and the existing average is
/// already above the bound, so no clip could have cut the order to fix it.
#[test]
#[ignore = "pending E6-2"]
fn a_projected_average_guard_fires_with_no_goal_clip() {
    let roomy_goal = accumulating("0.5", Some("58000"), "50000");
    let market = crypto_market("54990", "55000", "0.0001");
    let outputs = [output(&mean_reversion(), BTC_INSTRUMENT, "1", "1")];

    let dear = btc_account("0.14", "54990", "8300", "8300", "7698.6");
    let held = proposed(&roomy_goal, &dear, &market, &quiet_risk(), &outputs);
    assert_sizes(
        &held.sizes,
        "10000",
        "7698.6",
        Some("10000"),
        Some("2301.4"),
    );
    assert_eq!(
        hold_reason(&held.action),
        HoldReason::WouldExceedMaxAvgPrice,
        "9295.5 of basis over 0.1581 is above 58000"
    );
    assert_eq!(
        held.clipped_by,
        [Clip::Limits].into_iter().collect(),
        "no goal bound bound: the guard alone held the order"
    );

    let cheap = btc_account("0.14", "54990", "7700", "8300", "7698.6");
    let bought = proposed(&roomy_goal, &cheap, &market, &quiet_risk(), &outputs);
    match &bought.action {
        Action::Buy {
            qty: quantity,
            order_usd,
            ..
        } => {
            assert_eq!(
                *quantity,
                qty("0.0181"),
                "the same order, under a cheaper basis"
            );
            assert_eq!(*order_usd, usd("995.5"));
        }
        other => panic!("a buy of 0.0181, not {other:?}"),
    }
    assert_eq!(bought.clipped_by, [Clip::Limits].into_iter().collect());
}

/// §8.3 steps 2 and 3, the three inclusive-or-exclusive boundaries the exit threshold's test already
/// pins for its own side: b ≥ `entry_threshold` buys, Delta ≤ 0 holds, and Delta < the band holds
/// while Delta **equal** to the band goes on to be clipped.
#[test]
#[ignore = "pending E6-2"]
fn the_entry_threshold_is_inclusive_and_the_band_and_target_are_exclusive() {
    let outputs = swing_outputs("0.8", "0.2");
    let at_entry = BuilderMandate {
        sizing: Sizing {
            entry_threshold: frac("0.472"),
            ..two_stock_swing().sizing
        },
        ..two_stock_swing()
    };
    let bought = proposed(
        &at_entry,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    match &bought.action {
        Action::Buy { qty: quantity, .. } => {
            assert_eq!(*quantity, qty("7"), "b equal to the entry threshold buys")
        }
        other => panic!("a buy of 7, not {other:?}"),
    }
    let above_entry = BuilderMandate {
        sizing: Sizing {
            entry_threshold: frac("0.472000000001"),
            ..two_stock_swing().sizing
        },
        ..two_stock_swing()
    };
    let held = proposed(
        &above_entry,
        &flat_account(),
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_eq!(
        hold_reason(&held.action),
        HoldReason::BetweenThresholds,
        "one twelfth-place step above b and nothing is proposed"
    );

    let at_target = AccountSnapshot {
        gross_usd: usd("708"),
        working_opening_cost: usd("708"),
        ..flat_account()
    };
    let exactly_met = proposed(
        &two_stock_swing(),
        &at_target,
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&exactly_met.sizes, "1500", "0", Some("708"), Some("0"));
    assert_eq!(
        hold_reason(&exactly_met.action),
        HoldReason::AtOrAboveTarget,
        "a Delta of exactly zero is not positive"
    );

    let at_band = AccountSnapshot {
        gross_usd: usd("633"),
        working_opening_cost: usd("633"),
        ..flat_account()
    };
    let band_reached = proposed(
        &two_stock_swing(),
        &at_band,
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&band_reached.sizes, "1500", "0", Some("708"), Some("75"));
    assert_eq!(
        hold_reason(&band_reached.action),
        HoldReason::BelowBandAfterClipping,
        "a Delta equal to the band is not inside it, so it is clipped and then falls below it"
    );

    let inside_band = AccountSnapshot {
        gross_usd: usd("634"),
        working_opening_cost: usd("634"),
        ..flat_account()
    };
    let band_held = proposed(
        &two_stock_swing(),
        &inside_band,
        &swing_market(),
        &quiet_risk(),
        &outputs,
    );
    assert_sizes(&band_held.sizes, "1500", "0", Some("708"), Some("74"));
    assert_eq!(
        hold_reason(&band_held.action),
        HoldReason::WithinRebalanceBand,
        "one dollar less and the comparison is the band's own"
    );
}

/// §8.3 step 5: the minimum-order comparison is strict, so a value **equal** to `min_order_usd` is
/// proposed and one dollar more of minimum holds it.
#[test]
#[ignore = "pending E6-2"]
fn an_order_value_exactly_at_the_minimum_is_proposed() {
    let single_share = BuilderMandate {
        sizing: Sizing {
            rebalance_band: frac("0"),
            ..two_stock_swing().sizing
        },
        limits: Limits {
            max_order_usd: usd("100"),
            ..two_stock_swing().limits
        },
        ..two_stock_swing()
    };
    let at_minimum = Market {
        min_order_usd: usd("100"),
        ..swing_market()
    };
    let bought = proposed(
        &single_share,
        &flat_account(),
        &at_minimum,
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    match &bought.action {
        Action::Buy {
            qty: quantity,
            order_usd,
            ..
        } => {
            assert_eq!(*quantity, qty("1"));
            assert_eq!(*order_usd, usd("100"), "100 is not below a minimum of 100");
        }
        other => panic!("a buy of 1, not {other:?}"),
    }

    let above_minimum = Market {
        min_order_usd: usd("101"),
        ..swing_market()
    };
    let held = proposed(
        &single_share,
        &flat_account(),
        &above_minimum,
        &quiet_risk(),
        &swing_outputs("0.8", "0.2"),
    );
    assert_eq!(
        hold_reason(&held.action),
        HoldReason::BelowMinimumAfterClipping
    );
}

/// DEC-30 and `AGENTS.md` rule 13: the overnight session refuses an **opening** order, because §6.3's
/// `session` field has no name for it, and leaves an exit alone, because an exit is paced and never
/// denied and reads no session. A hold is a hold in any session.
#[test]
#[ignore = "pending E6-2"]
fn an_overnight_market_refuses_a_buy_and_still_proposes_an_exit() {
    let overnight = Market {
        session: MarketSession::Overnight,
        ..swing_market()
    };
    assert_eq!(
        propose(
            &two_stock_swing(),
            &flat_account(),
            &overnight,
            &quiet_risk(),
            &swing_outputs("0.8", "0.2"),
            common::at(NOW),
        )
        .map_err(|e| e.code()),
        Err("untradable_session"),
        "no opening order may trade overnight"
    );

    let exit = proposed(
        &two_stock_swing(),
        &account("5", "99.9"),
        &overnight,
        &quiet_risk(),
        &swing_outputs("-0.8", "-0.2"),
    );
    match &exit.action {
        Action::Sell {
            qty: quantity,
            order_usd,
            shape,
            ..
        } => {
            assert_eq!(*quantity, qty("5"), "an exit is never denied by a session");
            assert_eq!(*order_usd, usd("499.5"));
            assert_eq!(
                *shape,
                OrderShape::Limit,
                "the close window is an equity-session rule"
            );
        }
        other => panic!("a discretionary exit of 5, not {other:?}"),
    }

    let quiet = proposed(
        &two_stock_swing(),
        &flat_account(),
        &overnight,
        &quiet_risk(),
        &swing_outputs("0.3", "0.2"),
    );
    assert_eq!(
        hold_reason(&quiet.action),
        HoldReason::BetweenThresholds,
        "a hold proposes nothing, so there is nothing for the session to refuse"
    );
}

/// The refusals the crate declares and the order path reaches: two rules with one id, no signal
/// model, weights that sum to zero, an `accumulate` goal for another instrument, `decide` on a
/// holding proposal, and a unit-bounded condition value outside the unit interval.
#[test]
#[ignore = "pending E6-2"]
fn every_declared_refusal_is_reachable() {
    let duplicate = policy(
        vec![
            Rule {
                id: rule_id("same"),
                when: compare(ConditionField::OrderUsd, Operator::Gt, decimal("100")),
                then: AutonomyDecision::Ask,
            },
            Rule {
                id: rule_id("same"),
                when: compare(ConditionField::OrderUsd, Operator::Gt, decimal("200")),
                then: AutonomyDecision::Deny,
            },
        ],
        AutonomyDecision::Ask,
        AutonomyDecision::Ask,
        None,
    );
    assert_eq!(
        classify(&duplicate, &opening("300", "0.8")).map_err(|e| e.code()),
        Err("duplicate_rule_id"),
        "the first-match walk would report a `by` that names either"
    );

    let modelless = BuilderMandate {
        models: Vec::new(),
        ..two_stock_swing()
    };
    assert_eq!(
        propose(
            &modelless,
            &flat_account(),
            &swing_market(),
            &quiet_risk(),
            &[],
            common::at(NOW),
        )
        .map_err(|e| e.code()),
        Err("no_signal_models"),
        "§8.3's W has no denominator without a model"
    );

    let weightless = BuilderMandate {
        models: vec![SignalModel {
            weight: SizeFraction::ZERO,
            ..momentum()
        }],
        ..two_stock_swing()
    };
    assert_eq!(
        propose(
            &weightless,
            &flat_account(),
            &swing_market(),
            &quiet_risk(),
            &[output(&momentum(), SWING_INSTRUMENT, "1", "1")],
            common::at(NOW),
        )
        .map_err(|e| e.code()),
        Err("weight_sum_zero")
    );

    let other_instrument = accumulating("0.15", Some("58000"), "9000");
    let mismatched = BuilderMandate {
        goal: GoalKind::Accumulate(AccumulateGoal {
            instrument: asset(OTHER_INSTRUMENT),
            target_qty: qty("0.15"),
            max_avg_price: Some(price("58000")),
            max_spend_usd: usd("9000"),
        }),
        ..other_instrument
    };
    assert_eq!(
        propose(
            &mismatched,
            &btc_account("0.05", "54990", "0", "0", "2749.5"),
            &crypto_market("54990", "55000", "0.0001"),
            &quiet_risk(),
            &[output(&mean_reversion(), BTC_INSTRUMENT, "1", "1")],
            common::at(NOW),
        )
        .map_err(|e| e.code()),
        Err("accumulate_instrument_mismatch"),
        "V-003 pins the universe to the goal's one instrument"
    );

    let holding = Proposal {
        action: Action::Hold {
            reason: HoldReason::BetweenThresholds,
        },
        ..buy_proposal("800", "0.6")
    };
    for verdict in [GateVerdict::Allow, GateVerdict::Deny, GateVerdict::Defer] {
        assert_eq!(
            decide(&base_policy(), &holding, verdict).map_err(|e| e.code()),
            Err("nothing_proposed"),
            "a hold and a denied order are two different things on the journal"
        );
    }

    let above_one = policy(
        vec![Rule {
            id: rule_id("impossible"),
            when: compare(ConditionField::CombinedScore, Operator::Gt, decimal("2")),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classify(&above_one, &opening("300", "0.8")).map_err(|e| e.code()),
        Err("condition_type_mismatch"),
        "V-023 bounds a `combined_score` value to the closed unit interval"
    );
    let at_one = policy(
        vec![Rule {
            id: rule_id("possible"),
            when: compare(ConditionField::CombinedScore, Operator::Lte, decimal("1")),
            then: AutonomyDecision::Deny,
        }],
        AutonomyDecision::Auto,
        AutonomyDecision::Auto,
        None,
    );
    assert_eq!(
        classified(&at_one, &opening("300", "0.8")).by,
        DecidedBy::Rule(rule_id("possible")),
        "one is inside the interval, so the bound is closed"
    );
}
