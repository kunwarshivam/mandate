//! Hand-calculated cases for E6-2: every figure recomputed from [mandate spec §6] and [§8.3] against
//! the two base mandates, and every "never" or "always" clause of those sections given a case that
//! would break if the rule were read the other way.
//!
//! The `MC-A` and `MC-B` reference cases named in each doc comment are the committed contract; the
//! numbers here are recomputed from the spec, not copied from the crate.
//!
//! [mandate spec §6]: ../../../docs/specs/mandate.md#6-autonomy-dec-42-dec-48-dec-58
//! [§8.3]: ../../../docs/specs/mandate.md#83-order-builder-conviction_linear-dec-47-dec-60

mod common;

use common::*;
use mandate_builder::{
    Action, AutonomyPolicy, BuilderError, Clip, Condition, DecidedBy, Decision, Field, GateVerdict,
    GoalKind, HoldReason, ModelId, ModelVersion, Op, OrderShape, Outcome, Proposal, Purpose,
    Session, Value, classify, combine, decide, propose,
};
use mandate_num::{NumError, SizeFraction, Unit, Usd};

#[track_caller]
fn buy_of(proposal: &Proposal) -> (Purpose, String, String, String) {
    match &proposal.action {
        Action::Buy {
            purpose,
            qty,
            limit_price,
            order_usd,
            ..
        } => (
            *purpose,
            qty.to_string(),
            limit_price.to_string(),
            order_usd.to_string(),
        ),
        other => panic!("expected a buy, got {other:?}"),
    }
}

#[track_caller]
fn sell_of(proposal: &Proposal) -> (Purpose, String, String, String, OrderShape) {
    match &proposal.action {
        Action::Sell {
            purpose,
            qty,
            limit_price,
            order_usd,
            shape,
        } => (
            *purpose,
            qty.to_string(),
            limit_price.to_string(),
            order_usd.to_string(),
            *shape,
        ),
        other => panic!("expected a sell, got {other:?}"),
    }
}

#[track_caller]
fn hold_of(proposal: &Proposal) -> HoldReason {
    match proposal.action {
        Action::Hold { reason } => reason,
        ref other => panic!("expected a hold, got {other:?}"),
    }
}

#[track_caller]
fn action_of(proposal: &Proposal) -> &mandate_builder::ActionContext {
    match &proposal.action {
        Action::Buy { action, .. } => action,
        other => panic!("expected a buy, got {other:?}"),
    }
}

/// Spec §6.2 step 3: every purpose other than `open` and `increase` is AUTO by a built-in rule,
/// whatever the owner's rules say, because reducing risk never needs approval (MI-1, DEC-05).
/// `MC-A01` to `MC-A04`.
#[test]
#[ignore = "pending E6-2"]
fn every_reducing_purpose_is_auto_by_the_builtin() {
    let deny_everything = policy_with(
        vec![rule(
            "deny_all",
            compare(
                Field::Purpose,
                Op::In,
                Value::Set(
                    ["increase", "open"]
                        .iter()
                        .map(|s| (*s).to_owned())
                        .collect(),
                ),
            ),
            Decision::Deny,
        )],
        Decision::Deny,
        Decision::Deny,
        None,
    );
    for purpose in [
        Purpose::DiscretionaryExit,
        Purpose::Protective,
        Purpose::RiskExit,
        Purpose::OwnerExit,
    ] {
        let action = mandate_builder::ActionContext {
            purpose,
            ..routine_open()
        };
        let result = classify(&deny_everything, &action).unwrap();
        assert_eq!(result.decision, Decision::Auto, "{purpose:?}");
        assert_eq!(result.by, DecidedBy::BuiltinRiskReducing, "{purpose:?}");
        assert!(result.approval.is_none(), "{purpose:?}");
    }
}

/// Spec §6.2 step 4: the **first** matching rule decides. A later rule that would also match is not
/// read, which is what makes rule order part of the mandate.
#[test]
#[ignore = "pending E6-2"]
fn the_first_matching_rule_decides_and_a_later_one_is_not_read() {
    let policy = policy_with(
        vec![
            rule(
                "first",
                compare(Field::OrderUsd, Op::Gt, Value::Money(usd("100"))),
                Decision::Ask,
            ),
            rule(
                "second",
                compare(Field::OrderUsd, Op::Gt, Value::Money(usd("200"))),
                Decision::Deny,
            ),
        ],
        Decision::Auto,
        Decision::Auto,
        None,
    );
    let decided = classify(&policy, &routine_open()).unwrap();
    assert_eq!(decided.decision, Decision::Ask);
    assert_eq!(
        decided.by,
        DecidedBy::Rule(mandate_builder::RuleId::new("first"))
    );
}

/// Spec §6.2 step 4: with no rule matching, `autonomy.default` decides. `MC-A09`.
#[test]
#[ignore = "pending E6-2"]
fn no_rule_matches_so_the_default_decides() {
    let policy = policy_with(Vec::new(), Decision::Ask, Decision::Ask, None);
    let decided = classify(&policy, &routine_open()).unwrap();
    assert_eq!(decided.decision, Decision::Ask);
    assert_eq!(decided.by, DecidedBy::Default);
    let approval = decided.approval.unwrap();
    assert_eq!(approval.approvers_required.get(), 1);
    assert_eq!(approval.on_timeout, mandate_builder::OnTimeout::Skip);
}

/// Spec §6.2 step 5: the admission ceiling turns an `auto` rule into `ask` for the first order in a
/// newly admitted instrument, because the platform default for `autonomy.admission` is `ask` and an
/// admission is never automatic unless the owner entered `auto` for it. `MC-A12`.
#[test]
#[ignore = "pending E6-2"]
fn the_admission_ceiling_turns_auto_into_ask_for_a_new_instrument() {
    let action = mandate_builder::ActionContext {
        new_instrument: true,
        thesis_confidence: unit("0.9"),
        ..routine_open()
    };
    let decided = classify(&base_policy(), &action).unwrap();
    assert_eq!(decided.decision, Decision::Ask);
    assert_eq!(decided.by, DecidedBy::AdmissionCeiling);
    assert_eq!(decided.approval.unwrap().approvers_required.get(), 1);
}

/// Spec §6.2 step 5: the ceiling only tightens, so a `deny` rule still denies under
/// `admission: auto`, and the decision is reported as the rule's, not the ceiling's (DEC-05).
/// `MC-A15`.
#[test]
#[ignore = "pending E6-2"]
fn the_admission_ceiling_never_loosens_a_deny_rule() {
    let mut rules = vec![rule(
        "no_new",
        compare(Field::NewInstrument, Op::Eq, Value::Flag(true)),
        Decision::Deny,
    )];
    rules.extend(base_rules());
    let policy = policy_with(rules, Decision::Ask, Decision::Auto, None);
    let action = mandate_builder::ActionContext {
        new_instrument: true,
        thesis_confidence: unit("0.9"),
        ..routine_open()
    };
    let decided = classify(&policy, &action).unwrap();
    assert_eq!(decided.decision, Decision::Deny);
    assert_eq!(
        decided.by,
        DecidedBy::Rule(mandate_builder::RuleId::new("no_new"))
    );
    assert!(decided.approval.is_none());
}

/// Spec §6.2 step 5: an owner who entered `auto` for admissions gets the rule's decision, and `by`
/// names the rule rather than the ceiling, because the ceiling changed nothing (DEC-130 item 13).
/// `MC-A13`.
#[test]
#[ignore = "pending E6-2"]
fn an_admission_ceiling_that_changes_nothing_reports_the_rule() {
    let policy = policy_with(base_rules(), Decision::Ask, Decision::Auto, None);
    let action = mandate_builder::ActionContext {
        new_instrument: true,
        thesis_confidence: unit("0.9"),
        ..routine_open()
    };
    let decided = classify(&policy, &action).unwrap();
    assert_eq!(decided.decision, Decision::Auto);
    assert_eq!(
        decided.by,
        DecidedBy::Rule(mandate_builder::RuleId::new("routine"))
    );
}

/// Spec §6.2 step 5: the ceiling applies only when `new_instrument` is true, so an order in an
/// instrument the agent already holds is untouched by `admission: deny`.
#[test]
#[ignore = "pending E6-2"]
fn an_admission_ceiling_does_not_touch_an_order_in_a_held_instrument() {
    let policy = policy_with(base_rules(), Decision::Ask, Decision::Deny, None);
    let decided = classify(&policy, &routine_open()).unwrap();
    assert_eq!(decided.decision, Decision::Auto);
    assert_eq!(
        decided.by,
        DecidedBy::Rule(mandate_builder::RuleId::new("routine"))
    );
}

/// Spec §6.2 step 5: `admission: deny` overrides an `auto` rule, and the ceiling is what decided.
/// `MC-A14`.
#[test]
#[ignore = "pending E6-2"]
fn an_admission_deny_overrides_an_auto_rule() {
    let policy = policy_with(base_rules(), Decision::Ask, Decision::Deny, None);
    let action = mandate_builder::ActionContext {
        new_instrument: true,
        thesis_confidence: unit("0.9"),
        ..routine_open()
    };
    let decided = classify(&policy, &action).unwrap();
    assert_eq!(decided.decision, Decision::Deny);
    assert_eq!(decided.by, DecidedBy::AdmissionCeiling);
    assert!(decided.approval.is_none());
}

/// Spec §6.4: an ASK above `two_approver_above_usd` needs two distinct approvers, and the comparison
/// is **strict**, so an order exactly at the threshold needs one. `MC-A10`.
#[test]
#[ignore = "pending E6-2"]
fn two_approvers_above_the_threshold_and_one_at_it() {
    let policy = policy_with(base_rules(), Decision::Ask, Decision::Ask, Some(usd("500")));
    let above = mandate_builder::ActionContext {
        order_usd: usd("600"),
        combined_score: unit("0.6"),
        ..routine_open()
    };
    let at = mandate_builder::ActionContext {
        order_usd: usd("500"),
        combined_score: unit("0.6"),
        ..routine_open()
    };
    let decided = classify(&policy, &above).unwrap();
    assert_eq!(
        decided.by,
        DecidedBy::Rule(mandate_builder::RuleId::new("low_score"))
    );
    assert_eq!(decided.approval.unwrap().approvers_required.get(), 2);
    assert_eq!(
        classify(&policy, &at)
            .unwrap()
            .approval
            .unwrap()
            .approvers_required
            .get(),
        1
    );
}

/// Spec §6.4: `on_timeout` is always `skip`, so an unanswered ASK adds no risk (DEC-06). An AUTO or
/// a DENY carries no approval at all.
#[test]
#[ignore = "pending E6-2"]
fn an_ask_always_carries_skip_on_timeout() {
    let asked = mandate_builder::ActionContext {
        combined_score: unit("0.6"),
        ..routine_open()
    };
    let decided = classify(&base_policy(), &asked).unwrap();
    assert_eq!(decided.decision, Decision::Ask);
    assert_eq!(
        decided.approval.unwrap().on_timeout,
        mandate_builder::OnTimeout::Skip
    );
    assert!(
        classify(&base_policy(), &routine_open())
            .unwrap()
            .approval
            .is_none()
    );
}

/// Spec §6.2 step 4 with the base rules: a large order asks by `large_orders`, a thin score asks by
/// `low_score`, a routine order is AUTO, and an order **exactly** at the 900 USD threshold is not
/// large, because the rule compares with `gt`. `MC-A05` to `MC-A08`.
#[test]
#[ignore = "pending E6-2"]
fn the_base_rules_classify_the_four_reference_actions() {
    let large = mandate_builder::ActionContext {
        order_usd: usd("950"),
        combined_score: unit("0.9"),
        ..routine_open()
    };
    let thin = mandate_builder::ActionContext {
        combined_score: unit("0.6"),
        ..routine_open()
    };
    let at_threshold = mandate_builder::ActionContext {
        order_usd: usd("900"),
        ..routine_open()
    };
    let cases: [(&mandate_builder::ActionContext, Decision, &str); 4] = [
        (&large, Decision::Ask, "large_orders"),
        (&thin, Decision::Ask, "low_score"),
        (&routine_open(), Decision::Auto, "routine"),
        (&at_threshold, Decision::Auto, "routine"),
    ];
    for (action, decision, by) in cases {
        let decided = classify(&base_policy(), action).unwrap();
        assert_eq!(decided.decision, decision, "{by}");
        assert_eq!(
            decided.by,
            DecidedBy::Rule(mandate_builder::RuleId::new(by))
        );
    }
}

/// Spec §6.3: `all`, `any`, and `not` evaluate as written, to V-017's four levels.
#[test]
#[ignore = "pending E6-2"]
fn nested_conditions_evaluate_as_written() {
    let four_deep = Condition::All(vec![Condition::Any(vec![Condition::Not(Box::new(
        compare(Field::OrderUsd, Op::Gt, Value::Money(usd("1000"))),
    ))])]);
    let policy = policy_with(
        vec![rule("nested", four_deep, Decision::Deny)],
        Decision::Auto,
        Decision::Auto,
        None,
    );
    assert_eq!(
        classify(&policy, &routine_open()).unwrap().decision,
        Decision::Deny,
        "300 is not above 1000, so the innermost comparison is false and the `not` matches"
    );
    let big = mandate_builder::ActionContext {
        order_usd: usd("1200"),
        ..routine_open()
    };
    assert_eq!(classify(&policy, &big).unwrap().decision, Decision::Auto);
}

/// Spec §6.3: decimal fields compare numerically, never as text, so `9` is below `10` and `0.650`
/// equals `0.65`.
#[test]
#[ignore = "pending E6-2"]
fn a_decimal_condition_compares_numerically_not_lexically() {
    let policy = policy_with(
        vec![rule(
            "above_ten",
            compare(Field::OrderUsd, Op::Gt, Value::Money(usd("10"))),
            Decision::Deny,
        )],
        Decision::Auto,
        Decision::Auto,
        None,
    );
    let nine = mandate_builder::ActionContext {
        order_usd: usd("9"),
        ..routine_open()
    };
    assert_eq!(classify(&policy, &nine).unwrap().decision, Decision::Auto);
    let eleven = mandate_builder::ActionContext {
        order_usd: usd("11"),
        ..routine_open()
    };
    assert_eq!(classify(&policy, &eleven).unwrap().decision, Decision::Deny);
}

/// Spec §6.3: the exposure fields are the order's **after** values, so a rule on `bought_today_usd`
/// catches what order splitting would otherwise evade. `MC-A11`.
#[test]
#[ignore = "pending E6-2"]
fn bought_today_catches_order_splitting() {
    let mut rules = vec![rule(
        "daily_buys",
        compare(Field::BoughtTodayUsd, Op::Gt, Value::Money(usd("2000"))),
        Decision::Ask,
    )];
    rules.extend(base_rules());
    let policy = policy_with(rules, Decision::Ask, Decision::Ask, None);
    let split = mandate_builder::ActionContext {
        order_usd: usd("500"),
        combined_score: unit("0.9"),
        bought_today_usd: usd("2300"),
        ..routine_open()
    };
    let decided = classify(&policy, &split).unwrap();
    assert_eq!(decided.decision, Decision::Ask);
    assert_eq!(
        decided.by,
        DecidedBy::Rule(mandate_builder::RuleId::new("daily_buys"))
    );
}

/// Spec §6.3: a thesis-confidence rule lets an owner who allowed `auto` for admissions still ask on
/// a thin thesis. `MC-A16`.
#[test]
#[ignore = "pending E6-2"]
fn a_thesis_confidence_rule_asks_below_the_owner_threshold() {
    let mut rules = vec![rule(
        "thin_thesis",
        compare(Field::ThesisConfidence, Op::Lt, Value::Unit(unit("0.6"))),
        Decision::Ask,
    )];
    rules.extend(base_rules());
    let policy = policy_with(rules, Decision::Ask, Decision::Auto, None);
    let thin = mandate_builder::ActionContext {
        new_instrument: true,
        thesis_confidence: unit("0.5"),
        ..routine_open()
    };
    let decided = classify(&policy, &thin).unwrap();
    assert_eq!(decided.decision, Decision::Ask);
    assert_eq!(
        decided.by,
        DecidedBy::Rule(mandate_builder::RuleId::new("thin_thesis"))
    );
}

/// V-023: a condition whose value does not match its field's type is refused when the policy is
/// built, so no evaluation can meet it (DEC-130 item 12). An enum field takes its listed values
/// alone: `purpose` never takes `discretionary_exit`, because rules see only `open` and `increase`.
#[test]
fn a_condition_whose_value_does_not_match_its_field_type_is_refused() {
    let cases = [
        (
            "an ordering operator on an enum",
            Field::Session,
            Op::Gt,
            Value::Text("regular".to_owned()),
        ),
        (
            "a set on a decimal",
            Field::OrderUsd,
            Op::In,
            Value::Set(["1".to_owned()].into_iter().collect()),
        ),
        (
            "a flag compared with an ordering operator",
            Field::NewInstrument,
            Op::Gt,
            Value::Flag(true),
        ),
        (
            "a purpose rules never see",
            Field::Purpose,
            Op::Eq,
            Value::Text("discretionary_exit".to_owned()),
        ),
        (
            "a unit-typed field given money",
            Field::CombinedScore,
            Op::Lt,
            Value::Money(usd("1")),
        ),
        (
            "an empty set",
            Field::Instrument,
            Op::In,
            Value::Set(Default::default()),
        ),
    ];
    for (why, field, op, value) in cases {
        let built = AutonomyPolicy::new(
            vec![rule(
                "bad",
                compare(field, op, value.clone()),
                Decision::Ask,
            )],
            Decision::Ask,
            Decision::Ask,
            None,
        );
        assert_eq!(
            built.err().map(|e| e.code()),
            Some("condition_type_mismatch"),
            "{why}: {field:?} {op:?} {value:?}"
        );
    }
}

/// V-018: `unusual_input` is reserved until the input-drift detector ships, so a rule naming it is
/// refused and the field is never reachable.
#[test]
fn a_rule_using_unusual_input_is_refused() {
    let built = AutonomyPolicy::new(
        vec![rule(
            "drift",
            compare(Field::UnusualInput, Op::Eq, Value::Flag(true)),
            Decision::Ask,
        )],
        Decision::Ask,
        Decision::Ask,
        None,
    );
    assert_eq!(built.err().map(|e| e.code()), Some("reserved_field"));
}

/// V-017: conditions nest at most four levels, and the fourth is allowed.
#[test]
fn a_condition_deeper_than_four_levels_is_refused_and_four_is_allowed() {
    let leaf = compare(Field::OrderUsd, Op::Gt, Value::Money(usd("1")));
    let four = Condition::All(vec![Condition::Any(vec![Condition::Not(Box::new(
        leaf.clone(),
    ))])]);
    let five = Condition::All(vec![Condition::Any(vec![Condition::Not(Box::new(
        Condition::All(vec![leaf]),
    ))])]);
    assert!(
        AutonomyPolicy::new(
            vec![rule("four", four, Decision::Ask)],
            Decision::Ask,
            Decision::Ask,
            None
        )
        .is_ok()
    );
    assert_eq!(
        AutonomyPolicy::new(
            vec![rule("five", five, Decision::Ask)],
            Decision::Ask,
            Decision::Ask,
            None
        )
        .err()
        .map(|e| e.code()),
        Some("condition_too_deep")
    );
}

/// Spec §3: rule ids are unique, so a repeated id cannot make the first match ambiguous.
#[test]
fn two_rules_may_not_share_an_id() {
    let leaf = compare(Field::OrderUsd, Op::Gt, Value::Money(usd("1")));
    let built = AutonomyPolicy::new(
        vec![
            rule("same", leaf.clone(), Decision::Ask),
            rule("same", leaf, Decision::Deny),
        ],
        Decision::Ask,
        Decision::Ask,
        None,
    );
    assert_eq!(built.err().map(|e| e.code()), Some("duplicate_rule_id"));
}

/// DEC-130 item 8: a mandate fraction the schema accepts with more than 12 places, and a model output
/// beyond 18, are `too_precise`, so the builder refuses rather than approximating a size. Refusing
/// adds no risk; an approximated size is an order nobody specified.
#[test]
fn a_weight_beyond_twelve_places_is_refused() {
    assert_eq!(
        SizeFraction::parse("0.0000000000001").err(),
        Some(NumError::TooPrecise)
    );
    assert!(SizeFraction::parse("0.000000000001").is_ok());
    assert_eq!(SizeFraction::parse("1.5").err(), Some(NumError::AboveOne));
    assert_eq!(SizeFraction::parse("-0.5").err(), Some(NumError::Negative));
}

/// The same bound one scale up for a model's own numbers, which are not envelope fields: `MC-B13`
/// carries a 13-place confidence, so 12 would reject a committed case.
#[test]
fn a_confidence_beyond_eighteen_places_is_refused() {
    assert!(Unit::parse("0.6499999999999").is_ok());
    assert!(Unit::parse("0.000000000000000001").is_ok());
    assert_eq!(
        Unit::parse("0.0000000000000000001").err(),
        Some(NumError::TooPrecise)
    );
    assert_eq!(
        Unit::parse("1.000000000000000001").err(),
        Some(NumError::AboveOne)
    );
}

/// Spec §8.2: fresh means `as_of ≤ now < expires_at`. An output whose `as_of` is in the future is not
/// fresh, so a model cannot reach forward past its own data cut-off. `MC-B09`.
#[test]
#[ignore = "pending E6-2"]
fn a_future_as_of_is_not_fresh() {
    let future = output_at(
        &momentum_model(),
        XYZ,
        "0.8",
        "0.9",
        "2026-09-22T14:01:00.000000000Z",
        HOUR_AHEAD,
    );
    let combined = combine(&[momentum_model()], &[future], time(NOW)).unwrap();
    assert!(combined.outputs_used.is_empty());
}

/// Spec §8.2: `now − as_of ≤ max_output_age_s`, inclusive at the bound. Momentum allows 900 seconds,
/// so an output 900 seconds old counts and one 901 seconds old does not. `MC-B10`.
#[test]
#[ignore = "pending E6-2"]
fn an_output_at_exactly_max_output_age_is_fresh_and_one_second_later_is_not() {
    let at_bound = output_at(
        &momentum_model(),
        XYZ,
        "0.8",
        "0.9",
        "2026-09-22T13:45:00.000000000Z",
        HOUR_AHEAD,
    );
    let past_bound = output_at(
        &momentum_model(),
        XYZ,
        "0.8",
        "0.9",
        "2026-09-22T13:44:59.000000000Z",
        HOUR_AHEAD,
    );
    let fresh = combine(&[momentum_model()], &[at_bound], time(NOW)).unwrap();
    assert_eq!(fresh.outputs_used.len(), 1);
    let stale = combine(&[momentum_model()], &[past_bound], time(NOW)).unwrap();
    assert!(stale.outputs_used.is_empty());
}

/// Spec §8.2: `as_of ≤ now` is **inclusive**, so an output whose data cut-off is exactly now is
/// fresh. The planted-bug pass found this gap: an implementation reading the bound as strict passed
/// every other freshness case.
#[test]
#[ignore = "pending E6-2"]
fn an_output_whose_as_of_is_exactly_now_is_fresh() {
    let at_now = output_at(&momentum_model(), XYZ, "0.8", "0.9", NOW, HOUR_AHEAD);
    let combined = combine(&[momentum_model()], &[at_now], time(NOW)).unwrap();
    assert_eq!(
        combined.outputs_used.len(),
        1,
        "as_of ≤ now is inclusive, so an output cut off exactly now counts"
    );
}

/// Spec §8.2: `now < expires_at` is strict, so an output expiring exactly now has expired.
#[test]
#[ignore = "pending E6-2"]
fn an_output_expiring_exactly_now_is_not_fresh() {
    let expiring = output_at(&momentum_model(), XYZ, "0.8", "0.9", MINUTE_AGO, NOW);
    let combined = combine(&[momentum_model()], &[expiring], time(NOW)).unwrap();
    assert!(combined.outputs_used.is_empty());
}

/// Spec §8.1 and §8.2: the pinned triple is compared whole. A version or a content hash that is not
/// the pinned one is ignored and counts as **missing**, so a substituted model lowers a buy
/// conviction and never raises one (DEC-67). `MC-B12`.
#[test]
#[ignore = "pending E6-2"]
fn a_wrong_model_version_counts_as_missing() {
    let mut wrong_version = fresh_output(&momentum_model(), XYZ, "1", "1");
    wrong_version.model_version = ModelVersion::new("1.0.1");
    let mut wrong_hash = fresh_output(&momentum_model(), XYZ, "1", "1");
    wrong_hash.content_hash = mandate_builder::ContentHash::new(HASH_NEWS);
    for output in [wrong_version, wrong_hash] {
        let combined = combine(&[momentum_model()], &[output], time(NOW)).unwrap();
        assert!(combined.outputs_used.is_empty());
        assert_eq!(combined.exit_conviction.to_string(), "0");
        assert_eq!(
            combined.buy_conviction.to_string(),
            "-1",
            "the one configured model is missing, so the buy conviction is fully bearish"
        );
    }
}

/// Spec §8.1: an output from a model the mandate does not configure is ignored, whatever it says.
#[test]
#[ignore = "pending E6-2"]
fn an_unpinned_model_id_is_ignored() {
    let mut stranger = fresh_output(&momentum_model(), XYZ, "1", "1");
    stranger.model_id = ModelId::new("quant.stranger");
    let combined = combine(&[momentum_model()], &[stranger], time(NOW)).unwrap();
    assert!(combined.outputs_used.is_empty());
}

/// Spec §8.2: only the latest fresh output per model counts, by `as_of`. `MC-B11`.
#[test]
#[ignore = "pending E6-2"]
fn duplicate_outputs_take_the_latest_as_of() {
    let older = output_at(
        &momentum_model(),
        XYZ,
        "-1",
        "1",
        "2026-09-22T13:50:00.000000000Z",
        HOUR_AHEAD,
    );
    let newer = output_at(&momentum_model(), XYZ, "1", "1", MINUTE_AGO, HOUR_AHEAD);
    let combined = combine(&[momentum_model()], &[older, newer], time(NOW)).unwrap();
    assert_eq!(combined.exit_conviction.to_string(), "1");
}

/// Spec §8.2: ties in `as_of` break by journal order, which the caller supplies as the order of
/// `outputs`. The later position wins, so replaying the same stream cannot choose differently
/// (DEC-130 item 10).
#[test]
#[ignore = "pending E6-2"]
fn two_outputs_with_one_as_of_take_the_later_journal_position() {
    let first = fresh_output(&momentum_model(), XYZ, "-1", "1");
    let second = fresh_output(&momentum_model(), XYZ, "1", "1");
    let combined = combine(&[momentum_model()], &[first, second], time(NOW)).unwrap();
    assert_eq!(combined.exit_conviction.to_string(), "1");
}

/// Spec §8.3 step 1: a missing model counts as **zero** in the exit conviction, so an outage never
/// forces a sell. Momentum alone at −0.8 and 0.9 gives −0.432, still past the 0.3 exit threshold.
/// `MC-B07`.
#[test]
#[ignore = "pending E6-2"]
fn a_missing_model_counts_as_zero_for_the_exit_conviction() {
    let outputs = vec![fresh_output(&momentum_model(), XYZ, "-0.8", "0.9")];
    let combined = combine(&swing_mandate().models, &outputs, time(NOW)).unwrap();
    assert_eq!(combined.exit_conviction.to_string(), "-0.432");
    assert_eq!(combined.buy_conviction.to_string(), "-0.832");
    assert_eq!(combined.score.to_string(), "0.54");
}

/// Spec §8.3 step 1: a missing model counts as **fully bearish** in the buy conviction, so an outage
/// never enlarges a buy (MI-10). Momentum alone at 0.8 and 0.9 gives 0.432 for an exit but
/// 0.432 − 0.4 = 0.032 for a buy, below the 0.3 entry threshold. `MC-B06`.
#[test]
#[ignore = "pending E6-2"]
fn a_missing_model_counts_as_fully_bearish_for_buys() {
    let outputs = vec![fresh_output(&momentum_model(), XYZ, "0.8", "0.9")];
    let combined = combine(&swing_mandate().models, &outputs, time(NOW)).unwrap();
    assert_eq!(combined.exit_conviction.to_string(), "0.432");
    assert_eq!(combined.buy_conviction.to_string(), "0.032");
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::BetweenThresholds);
}

/// Spec §8.3 step 1: W is the sum over **all** configured models, not the fresh ones. With one of
/// two weights fresh, dividing by the fresh weight alone would read 0.8 where the spec reads 0.48.
#[test]
#[ignore = "pending E6-2"]
fn the_denominator_counts_configured_models_not_fresh_ones() {
    let outputs = vec![fresh_output(&momentum_model(), XYZ, "0.8", "1")];
    let combined = combine(&swing_mandate().models, &outputs, time(NOW)).unwrap();
    assert_eq!(combined.exit_conviction.to_string(), "0.48");
    assert_eq!(combined.score.to_string(), "0.6");
}

/// Spec §8.3 step 1 and §6.3: all three figures round to 12 places **before** a rule compares them.
/// At 13 places the score is 0.64999999999996, below `low_score`'s 0.65, and the order would ASK; at
/// 12 it is 0.65 and the order is AUTO. `MC-B13`.
#[test]
#[ignore = "pending E6-2"]
fn the_score_rounds_to_twelve_places_before_the_rule_compares_it() {
    let outputs = vec![
        fresh_output(&momentum_model(), XYZ, "1", "0.65"),
        fresh_output(&news_model(), XYZ, "1", "0.6499999999999"),
    ];
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.combined.score.to_string(), "0.65");
    assert_eq!(proposal.combined.buy_conviction.to_string(), "0.65");
    let (purpose, qty, limit, order) = buy_of(&proposal);
    assert_eq!(
        (purpose, qty.as_str(), limit.as_str(), order.as_str()),
        (Purpose::Open, "9", "100", "900")
    );
    assert!(proposal.clipped_by.is_empty());
    let outcome = decide(&base_policy(), &proposal, GateVerdict::Allow).unwrap();
    match outcome {
        Outcome::Classified(autonomy) => {
            assert_eq!(autonomy.decision, Decision::Auto);
            assert_eq!(
                autonomy.by,
                DecidedBy::Rule(mandate_builder::RuleId::new("routine"))
            );
        }
        other => panic!("expected a classification, got {other:?}"),
    }
}

/// Spec §8.3 step 1: with no fresh output the builder holds, and still reports all three figures, so
/// an outage is visible as an outage rather than as a blank (DEC-130 item 9). `MC-B20`.
#[test]
#[ignore = "pending E6-2"]
fn no_fresh_output_holds_and_reports_zero_minus_one_and_zero() {
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &[],
        time(NOW),
    )
    .unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::NoFreshOutputs);
    assert!(proposal.combined.outputs_used.is_empty());
    assert_eq!(proposal.combined.exit_conviction.to_string(), "0");
    assert_eq!(proposal.combined.buy_conviction.to_string(), "-1");
    assert_eq!(proposal.combined.score.to_string(), "0");
    assert_eq!(proposal.sizes.cap.to_string(), "1500");
    assert_eq!(proposal.sizes.current_mv.to_string(), "0");
    assert!(proposal.sizes.target_value.is_none());
}

/// A swing account holding `shares` at the reference cases' 99.9 mark.
fn held(shares: &str, cost: &str) -> mandate_builder::AccountSnapshot {
    let position = qty(shares);
    let market_value = position.notional(price("99.9")).unwrap();
    mandate_builder::AccountSnapshot {
        position_qty: position,
        cost_basis: basis(cost),
        gross_usd: market_value,
        ..flat_account()
    }
}

/// Spec §8.3 step 2: an exit fires at `c ≤ −exit_threshold` and sells the **whole** position at the
/// bid, minus nothing, because there are no working exits in this input. `MC-B04`.
#[test]
#[ignore = "pending E6-2"]
fn an_exit_at_the_threshold_sells_the_whole_position() {
    let outputs = vec![
        fresh_output(&momentum_model(), XYZ, "-0.8", "0.9"),
        fresh_output(&news_model(), XYZ, "-0.8", "0.5"),
    ];
    let proposal = propose(
        &swing_mandate(),
        &held("7", "700"),
        &equity_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.combined.exit_conviction.to_string(), "-0.592");
    let (purpose, shares, limit, order, shape) = sell_of(&proposal);
    assert_eq!(purpose, Purpose::DiscretionaryExit);
    assert_eq!(
        (shares.as_str(), limit.as_str(), order.as_str()),
        ("7", "99.9", "699.3")
    );
    assert_eq!(shape, PLAIN);
}

/// Spec §3.1: an `accumulate` goal disables discretionary exits, so a conviction past the exit
/// threshold holds rather than selling. `MC-B29`.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_never_sells_on_negative_conviction() {
    let outputs = vec![fresh_output(&mean_reversion_model(), BTC, "-0.81", "0.9")];
    let account = mandate_builder::AccountSnapshot {
        position_qty: qty("0.05"),
        cost_basis: basis("2749.5"),
        risk_mark: mark("54990"),
        gross_usd: usd("2749.5"),
        ..flat_account()
    };
    let proposal = propose(
        &btc_mandate(),
        &account,
        &crypto_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::DiscretionaryExitsDisabled);
}

/// Spec §8.3 step 2: a flat agent past the exit threshold holds `no_position`, because there is
/// nothing to sell and a sell above the position would cross zero (DEC-32).
#[test]
#[ignore = "pending E6-2"]
fn a_flat_position_below_the_exit_threshold_holds() {
    let outputs = vec![
        fresh_output(&momentum_model(), XYZ, "-0.8", "0.9"),
        fresh_output(&news_model(), XYZ, "-0.8", "0.5"),
    ];
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::NoPosition);
}

/// DEC-130 item 21: in step 2 the `accumulate` check comes **before** the flat-position check, so a
/// flat `accumulate` agent reads `discretionary_exits_disabled`. Reporting `no_position` instead
/// would read as though a position would otherwise have been sold.
#[test]
#[ignore = "pending E6-2"]
fn a_flat_accumulate_agent_below_the_exit_threshold_holds_exits_disabled() {
    let outputs = vec![fresh_output(&mean_reversion_model(), BTC, "-0.81", "0.9")];
    let account = mandate_builder::AccountSnapshot {
        risk_mark: mark("55000"),
        ..flat_account()
    };
    let proposal = propose(
        &btc_mandate(),
        &account,
        &crypto_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::DiscretionaryExitsDisabled);
}

/// Spec §8.3 step 2: between the thresholds nothing is proposed — no new buy and no sell. `MC-B03`.
#[test]
#[ignore = "pending E6-2"]
fn between_the_thresholds_nothing_is_proposed() {
    let outputs = vec![
        fresh_output(&momentum_model(), XYZ, "0.3", "0.9"),
        fresh_output(&news_model(), XYZ, "0.2", "0.5"),
    ];
    let proposal = propose(
        &swing_mandate(),
        &held("7", "700"),
        &equity_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.combined.exit_conviction.to_string(), "0.202");
    assert_eq!(hold_of(&proposal), HoldReason::BetweenThresholds);
}

/// Spec §8.3 step 2: `cap = min(max_position_usd, max_position_fraction × E)`. The swing base caps at
/// 1500 USD while 20% of a 10000 USD equity is 2000, so the dollar limit binds; at a 5000 USD equity
/// the fraction binds at 1000.
#[test]
#[ignore = "pending E6-2"]
fn the_cap_is_the_lesser_of_the_dollar_and_fraction_limits() {
    let outputs = swing_outputs();
    let dollar_bound = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(dollar_bound.sizes.cap.to_string(), "1500");
    let smaller = mandate_builder::AccountSnapshot {
        agent_equity: usd("5000"),
        ..flat_account()
    };
    let fraction_bound = propose(
        &swing_mandate(),
        &smaller,
        &equity_market(),
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(fraction_bound.sizes.cap.to_string(), "1000");
}

/// Spec §5.5 and §8.3 step 2: an active drawdown rung's size factor scales the target, so the same
/// outputs buy half as much. `MC-B02`.
#[test]
#[ignore = "pending E6-2"]
fn the_ladder_size_factor_scales_the_target() {
    let scaled = mandate_builder::RiskContext {
        size_factor: fraction("0.5"),
        ..quiet_risk()
    };
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &scaled,
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.sizes.target_value.unwrap().to_string(), "354");
    let (_, shares, _, order) = buy_of(&proposal);
    assert_eq!((shares.as_str(), order.as_str()), ("3", "300"));
}

/// Spec §8.3 step 1 to 3 end to end: the reference case's two models, cap, target, delta, clip, and
/// classification. `MC-B01`.
#[test]
#[ignore = "pending E6-2"]
fn two_models_open_auto_by_the_routine_rule() {
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.combined.exit_conviction.to_string(), "0.472");
    assert_eq!(proposal.combined.buy_conviction.to_string(), "0.472");
    assert_eq!(proposal.combined.score.to_string(), "0.74");
    assert_eq!(proposal.sizes.cap.to_string(), "1500");
    assert_eq!(proposal.sizes.current_mv.to_string(), "0");
    assert_eq!(proposal.sizes.target_value.unwrap().to_string(), "708");
    assert_eq!(proposal.sizes.delta.unwrap().to_string(), "708");
    let (purpose, shares, limit, order) = buy_of(&proposal);
    assert_eq!(
        (purpose, shares.as_str(), limit.as_str(), order.as_str()),
        (Purpose::Open, "7", "100", "700")
    );
    assert!(proposal.clipped_by.is_empty());
    assert_eq!(
        proposal
            .combined
            .outputs_used
            .iter()
            .map(ModelId::as_str)
            .collect::<Vec<_>>(),
        vec!["llm.news_research", "quant.momentum"]
    );
}

/// Spec §8.3 step 3: a working opening order counts toward the target, so a 400 USD order already
/// working leaves 308 USD of the 708 USD target and the buy is 300 USD, not 700. `MC-B15`.
#[test]
#[ignore = "pending E6-2"]
fn a_working_opening_order_counts_toward_the_target() {
    let working = mandate_builder::AccountSnapshot {
        working_opening_cost: usd("400"),
        gross_usd: usd("400"),
        ..flat_account()
    };
    let proposal = propose(
        &swing_mandate(),
        &working,
        &equity_market(),
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.sizes.delta.unwrap().to_string(), "308");
    let (_, shares, _, order) = buy_of(&proposal);
    assert_eq!((shares.as_str(), order.as_str()), ("3", "300"));
}

/// Spec §8.3 step 3: a position above its target holds. There are no signal trims in v1, so a
/// positive conviction never sells — positions shrink by exits and by `trim_to_target`. `MC-B16`.
#[test]
#[ignore = "pending E6-2"]
fn at_or_above_target_holds_and_never_sells() {
    let proposal = propose(
        &swing_mandate(),
        &held("15", "1500"),
        &equity_market(),
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.sizes.current_mv.to_string(), "1498.5");
    assert_eq!(proposal.sizes.delta.unwrap().to_string(), "-790.5");
    assert_eq!(hold_of(&proposal), HoldReason::AtOrAboveTarget);
}

/// Spec §8.3 step 3: a delta inside `rebalance_band × cap` holds, so the agent does not churn. Seven
/// shares leave 8.7 USD of the target, below the 75 USD band. `MC-B18`.
#[test]
#[ignore = "pending E6-2"]
fn a_delta_inside_the_band_holds() {
    let proposal = propose(
        &swing_mandate(),
        &held("7", "700"),
        &equity_market(),
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.sizes.current_mv.to_string(), "699.3");
    assert_eq!(proposal.sizes.delta.unwrap().to_string(), "8.7");
    assert_eq!(hold_of(&proposal), HoldReason::WithinRebalanceBand);
}

/// Spec §8.3 step 3: the buy value is the least of the delta, `max_order_usd`, the cap headroom, and
/// the gross headroom. Each bound that **can** bind binds here, and `clipped_by` records `limits`
/// exactly when one of the last three cut the delta down. `MC-B14`, `MC-B19`.
#[test]
#[ignore = "pending E6-2"]
fn each_bound_that_can_bind_binds_in_turn() {
    let full = vec![
        fresh_output(&momentum_model(), XYZ, "1", "1"),
        fresh_output(&news_model(), XYZ, "1", "1"),
    ];
    let by_delta = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert!(
        by_delta.clipped_by.is_empty(),
        "the delta binds at 708 of a 1500 cap, so no limit clipped it"
    );
    assert_eq!(buy_of(&by_delta).3, "700");

    let by_order = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &full,
        time(NOW),
    )
    .unwrap();
    assert_eq!(by_order.sizes.target_value.unwrap().to_string(), "1500");
    assert_eq!(
        buy_of(&by_order).3,
        "1000",
        "a full-conviction target of 1500 is cut to max_order_usd"
    );
    assert_eq!(
        by_order.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits]
    );

    let crowded = mandate_builder::AccountSnapshot {
        gross_usd: usd("1500"),
        ..flat_account()
    };
    let by_gross = propose(
        &swing_mandate(),
        &crowded,
        &equity_market(),
        &quiet_risk(),
        &full,
        time(NOW),
    )
    .unwrap();
    assert_eq!(
        buy_of(&by_gross).3,
        "500",
        "1500 USD of other positions leaves 500 of the 2000 gross limit"
    );
    assert_eq!(
        by_gross.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits]
    );
}

/// The cap headroom of spec §8.3 step 3 is never the bound that binds: the target is
/// `buy_conviction × cap × size factor` with both factors at most one, so `T ≤ cap` and therefore
/// `delta = T − MV − working ≤ cap − MV − working` always, with equality only at full conviction and
/// no active rung. The bound is belt and braces, not a clip, and a proposal cut to exactly the cap
/// headroom is therefore cut by the delta and reports no clip at all.
#[test]
#[ignore = "pending E6-2"]
fn the_cap_headroom_is_never_the_binding_bound() {
    let mut mandate = swing_mandate();
    mandate.limits.max_order_usd = usd("1500");
    mandate.limits.max_gross_exposure_usd = usd("10000");
    let full = vec![
        fresh_output(&momentum_model(), XYZ, "1", "1"),
        fresh_output(&news_model(), XYZ, "1", "1"),
    ];
    let holding = mandate_builder::AccountSnapshot {
        position_qty: qty("5"),
        cost_basis: basis("500"),
        gross_usd: usd("499.5"),
        ..flat_account()
    };
    let proposal = propose(
        &mandate,
        &holding,
        &equity_market(),
        &quiet_risk(),
        &full,
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.sizes.cap.to_string(), "1500");
    assert_eq!(proposal.sizes.current_mv.to_string(), "499.5");
    assert_eq!(proposal.sizes.delta.unwrap().to_string(), "1000.5");
    assert!(
        proposal.clipped_by.is_empty(),
        "the delta and the cap headroom are both 1000.5, so the delta bound it and nothing clipped"
    );
    assert_eq!(buy_of(&proposal).3, "1000");
}

/// Spec §8.3 step 3: a value that falls below the band **after** clipping holds, so clipping never
/// leaves a tiny top-up. A 1950 USD gross leaves 50 USD of the 2000 limit, which buys no whole share.
/// `MC-B19`.
#[test]
#[ignore = "pending E6-2"]
fn a_value_below_the_band_after_clipping_holds() {
    let crowded = mandate_builder::AccountSnapshot {
        gross_usd: usd("1950"),
        ..flat_account()
    };
    let full = vec![
        fresh_output(&momentum_model(), XYZ, "1", "1"),
        fresh_output(&news_model(), XYZ, "1", "1"),
    ];
    let proposal = propose(
        &swing_mandate(),
        &crowded,
        &equity_market(),
        &quiet_risk(),
        &full,
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.sizes.delta.unwrap().to_string(), "1500");
    assert_eq!(hold_of(&proposal), HoldReason::BelowBandAfterClipping);
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits]
    );
}

/// Spec §8.3 step 5: a value above the band but below the minimum order holds.
#[test]
#[ignore = "pending E6-2"]
fn a_value_below_the_minimum_order_holds() {
    let market = mandate_builder::Market {
        min_order_usd: usd("2000"),
        ..equity_market()
    };
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &market,
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::BelowMinimumAfterClipping);
}

/// DEC-130 item 21: the step-5 guard is `n ≤ 0` **or** below the minimum order, not the minimum
/// alone. With a zero band and a zero minimum, a fully clipped budget truncates to no shares at all,
/// and only the quantity guard stands between that and an order for nothing.
#[test]
#[ignore = "pending E6-2"]
fn a_zero_quantity_never_becomes_a_buy_at_a_zero_minimum_and_zero_band() {
    let mut mandate = swing_mandate();
    mandate.sizing.rebalance_band = SizeFraction::ZERO;
    let market = mandate_builder::Market {
        min_order_usd: Usd::ZERO,
        ..equity_market()
    };
    let crowded = mandate_builder::AccountSnapshot {
        gross_usd: usd("1990"),
        ..flat_account()
    };
    let full = vec![
        fresh_output(&momentum_model(), XYZ, "1", "1"),
        fresh_output(&news_model(), XYZ, "1", "1"),
    ];
    let proposal = propose(&mandate, &crowded, &market, &quiet_risk(), &full, time(NOW)).unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::BelowMinimumAfterClipping);
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits]
    );
}

/// Spec §6.1: a buy with no position is `open` and a buy with one is `increase`. The gate assigns the
/// purpose from side and position; the builder's label agrees with it. `MC-B05`.
#[test]
#[ignore = "pending E6-2"]
fn a_buy_with_no_position_is_open_and_with_one_is_increase() {
    let full = vec![
        fresh_output(&momentum_model(), XYZ, "1", "1"),
        fresh_output(&news_model(), XYZ, "1", "1"),
    ];
    let opened = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &full,
        time(NOW),
    )
    .unwrap();
    assert_eq!(buy_of(&opened).0, Purpose::Open);
    let increased = propose(
        &swing_mandate(),
        &held("7", "700"),
        &equity_market(),
        &quiet_risk(),
        &full,
        time(NOW),
    )
    .unwrap();
    let (purpose, shares, _, order) = buy_of(&increased);
    assert_eq!(
        (purpose, shares.as_str(), order.as_str()),
        (Purpose::Increase, "8", "800")
    );
}

/// DEC-130 item 19 and `MC-B25`: `first_trade_in_instrument` comes from `has_prior_fill`, never from
/// the position, because a re-entry after a round trip is flat and is **not** a first trade.
#[test]
#[ignore = "pending E6-2"]
fn an_absent_prior_fill_flag_is_not_inferred_from_the_position() {
    let returning = mandate_builder::RiskContext {
        has_prior_fill: true,
        ..quiet_risk()
    };
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &returning,
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert!(!action_of(&proposal).first_trade_in_instrument);
    let first = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    assert!(action_of(&first).first_trade_in_instrument);
}

/// Spec §6.3: the exposure fields on the proposed action are this order's **after** values, computed
/// from the same numbers that sized it.
#[test]
#[ignore = "pending E6-2"]
fn the_action_context_carries_this_orders_after_values() {
    let working = mandate_builder::AccountSnapshot {
        working_opening_cost: usd("400"),
        gross_usd: usd("400"),
        ..flat_account()
    };
    let risk = mandate_builder::RiskContext {
        bought_today_usd: usd("1200"),
        ..quiet_risk()
    };
    let proposal = propose(
        &swing_mandate(),
        &working,
        &equity_market(),
        &risk,
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    let action = action_of(&proposal);
    assert_eq!(action.order_usd.to_string(), "300");
    assert_eq!(action.position_usd_after.to_string(), "700");
    assert_eq!(action.gross_usd_after.to_string(), "700");
    assert_eq!(action.bought_today_usd.to_string(), "1500");
    assert_eq!(action.combined_score.to_string(), "0.74");
}

/// A `btc_accumulator` account holding `shares` with `cost` of basis, marked at 55000.
fn accumulating(shares: &str, cost: &str) -> mandate_builder::AccountSnapshot {
    mandate_builder::AccountSnapshot {
        position_qty: qty(shares),
        cost_basis: basis(cost),
        risk_mark: mark("55000"),
        gross_usd: qty(shares).notional(price("55000")).unwrap(),
        ..flat_account()
    }
}

fn full_conviction_btc() -> Vec<mandate_builder::ModelOutput> {
    vec![fresh_output(&mean_reversion_model(), BTC, "1", "1")]
}

/// Spec §8.3 step 4: `n ≤ (target_qty − position) ÷ β`, truncated to the increment. With 0.145 of a
/// 0.15 target held, the remaining 0.005 binds below the 0.018148820 the limits allowed. `MC-B26`.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_clipped_to_the_remaining_target_quantity() {
    let proposal = propose(
        &btc_mandate(),
        &accumulating("0.145", "7975"),
        &crypto_market(),
        &quiet_risk(),
        &full_conviction_btc(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(proposal.sizes.cap.to_string(), "10000");
    assert_eq!(proposal.sizes.current_mv.to_string(), "7975");
    assert_eq!(proposal.sizes.delta.unwrap().to_string(), "2025");
    let (purpose, shares, limit, order) = buy_of(&proposal);
    assert_eq!(
        (purpose, shares.as_str(), limit.as_str(), order.as_str()),
        (Purpose::Increase, "0.005", "55100", "275.5")
    );
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits, Clip::Goal]
    );
}

/// Spec §8.3 step 4: `n ≤ (max_spend_usd − goal spend) ÷ a`. Goal spend counts buy fills including
/// fees and sales never reduce it (§3.1), so 8900 already spent leaves 100 USD of the 9000 cap.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_clipped_by_max_spend() {
    let account = mandate_builder::AccountSnapshot {
        goal_spent_usd: usd("8900"),
        ..accumulating("0.1", "5500")
    };
    let proposal = propose(
        &btc_mandate(),
        &account,
        &crypto_market(),
        &quiet_risk(),
        &full_conviction_btc(),
        time(NOW),
    )
    .unwrap();
    let (_, shares, _, order) = buy_of(&proposal);
    assert_eq!(
        (shares.as_str(), order.as_str()),
        ("0.001814882", "99.9999982")
    );
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits, Clip::Goal]
    );
}

/// Spec §8.3 step 4: with `a − max_avg_price × β > 0` the average-price clip binds at
/// `(max_avg_price × position − cost basis) ÷ (a − max_avg_price × β)`. A 55000 USD ceiling one dollar
/// above the average already paid leaves room for 0.01, and the projected average lands **exactly** on
/// the ceiling, which the strict comparison of the guard allows. `MC-B27`.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_clipped_by_max_avg_price() {
    let mut mandate = btc_mandate();
    mandate.goal = GoalKind::Accumulate(mandate_builder::AccumulateGoal {
        instrument: instrument(BTC),
        target_qty: qty("0.15"),
        max_avg_price: Some(price("55000")),
        max_spend_usd: usd("9000"),
    });
    let proposal = propose(
        &mandate,
        &accumulating("0.1", "5499"),
        &crypto_market(),
        &quiet_risk(),
        &full_conviction_btc(),
        time(NOW),
    )
    .unwrap();
    let (_, shares, _, order) = buy_of(&proposal);
    assert_eq!((shares.as_str(), order.as_str()), ("0.01", "551"));
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits, Clip::Goal]
    );
}

/// Spec §8.3 step 4: after the clips, a projected average **above** `max_avg_price` holds. It is
/// reachable when `a − max_avg_price × β ≤ 0` leaves the clip off and the average already paid is
/// above the ceiling, which is the one path the three clips do not cover.
#[test]
#[ignore = "pending E6-2"]
fn a_projected_average_above_max_avg_price_holds() {
    let proposal = propose(
        &btc_mandate(),
        &accumulating("0.1", "5900"),
        &crypto_market(),
        &quiet_risk(),
        &full_conviction_btc(),
        time(NOW),
    )
    .unwrap();
    assert_eq!(hold_of(&proposal), HoldReason::WouldExceedMaxAvgPrice);
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits]
    );
}

/// Spec §8.3 step 4: fees are inside the clips. The cash rate raises the per-unit cost `a` and the
/// asset rate lowers the quantity received `β`, so the remaining-quantity clip buys 0.005010020 to
/// receive the 0.005 the goal still wants. `MC-B28`.
#[test]
#[ignore = "pending E6-2"]
fn accumulate_with_fees_counts_the_spend_and_the_quantity_received() {
    let market = mandate_builder::Market {
        fee_rate_cash: rate("0.001"),
        fee_rate_asset: rate("0.002"),
        ..crypto_market()
    };
    let proposal = propose(
        &btc_mandate(),
        &accumulating("0.145", "7975"),
        &market,
        &quiet_risk(),
        &full_conviction_btc(),
        time(NOW),
    )
    .unwrap();
    let (_, shares, _, order) = buy_of(&proposal);
    assert_eq!(
        (shares.as_str(), order.as_str()),
        ("0.00501002", "276.052102")
    );
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits, Clip::Goal]
    );
}

/// Spec §8.3 step 4: when `a − max_avg_price × β` is not positive the average-price clip is left off
/// rather than divided by, because a ceiling above the price it would pay cannot be reached by buying.
/// With a small position and plenty of room, no goal bound binds at all.
#[test]
#[ignore = "pending E6-2"]
fn a_max_avg_price_denominator_that_is_not_positive_leaves_the_clip_off() {
    let proposal = propose(
        &btc_mandate(),
        &accumulating("0.01", "550"),
        &crypto_market(),
        &quiet_risk(),
        &full_conviction_btc(),
        time(NOW),
    )
    .unwrap();
    let (_, shares, _, order) = buy_of(&proposal);
    assert_eq!(
        (shares.as_str(), order.as_str()),
        ("0.01814882", "999.999982")
    );
    assert_eq!(
        proposal.clipped_by.iter().copied().collect::<Vec<_>>(),
        vec![Clip::Limits]
    );
}

/// Spec §6.2 step 2: a `deny` dry run skips the action and **no approval is ever requested**, so a
/// proposal the gate would refuse never reaches an approver (DEC-05). `MC-B21`.
#[test]
#[ignore = "pending E6-2"]
fn a_gate_deny_skips_and_asks_nobody() {
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap();
    let asking = policy_with(base_rules(), Decision::Ask, Decision::Ask, None);
    assert_eq!(
        decide(&asking, &proposal, GateVerdict::Deny).unwrap(),
        Outcome::Skipped
    );
    assert!(
        matches!(
            decide(&asking, &proposal, GateVerdict::Allow).unwrap(),
            Outcome::Classified(_)
        ),
        "the same proposal is classified when the gate allows, so the skip was the verdict's doing"
    );
}

/// Spec §6.2 step 2 and trading spec §9.6: a `defer` leaves nothing stored and never becomes a deny.
/// The builder does not derive it — [`decide`] sees no session — so the verdict is the gate's alone
/// (DEC-130 item 15). `MC-B22`.
#[test]
#[ignore = "pending E6-2"]
fn a_defer_verdict_stores_nothing_and_never_becomes_a_deny() {
    let outputs = vec![
        fresh_output(&momentum_model(), XYZ, "-0.8", "0.9"),
        fresh_output(&news_model(), XYZ, "-0.8", "0.5"),
    ];
    let after_hours = mandate_builder::Market {
        session: Session::AfterHours,
        ..equity_market()
    };
    let proposal = propose(
        &swing_mandate(),
        &held("7", "700"),
        &after_hours,
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(
        sell_of(&proposal).0,
        Purpose::DiscretionaryExit,
        "the exit is still proposed: a deferral is not a refusal to propose"
    );
    assert_eq!(
        decide(&base_policy(), &proposal, GateVerdict::Defer).unwrap(),
        Outcome::Deferred
    );
}

/// Trading spec §9.6 and DEC-70: an equity discretionary exit inside the close window goes out as a
/// **marketable** limit order. This is the builder's, not the gate's: it changes the order rather than
/// the verdict. `MC-B23`.
#[test]
#[ignore = "pending E6-2"]
fn a_discretionary_exit_in_the_close_window_is_a_marketable_limit() {
    let outputs = vec![
        fresh_output(&momentum_model(), XYZ, "-0.8", "0.9"),
        fresh_output(&news_model(), XYZ, "-0.8", "0.5"),
    ];
    let closing = mandate_builder::Market {
        in_close_window: true,
        ..equity_market()
    };
    let proposal = propose(
        &swing_mandate(),
        &held("7", "700"),
        &closing,
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    assert_eq!(sell_of(&proposal).4, OrderShape::MarketableLimit);
}

/// Crypto has no regular session and no close window, so a crypto exit is a plain limit whatever the
/// clock says (DEC-130 item 15).
#[test]
#[ignore = "pending E6-2"]
fn a_crypto_discretionary_exit_is_a_plain_limit_in_any_session() {
    let mut mandate = btc_mandate();
    mandate.goal = GoalKind::Continuous;
    let outputs = vec![fresh_output(&mean_reversion_model(), BTC, "-0.8", "0.9")];
    let closing = mandate_builder::Market {
        in_close_window: true,
        ..crypto_market()
    };
    let proposal = propose(
        &mandate,
        &accumulating("0.05", "2750"),
        &closing,
        &quiet_risk(),
        &outputs,
        time(NOW),
    )
    .unwrap();
    let (purpose, shares, limit, _, shape) = sell_of(&proposal);
    assert_eq!(
        (purpose, shares.as_str(), limit.as_str()),
        (Purpose::DiscretionaryExit, "0.05", "55000")
    );
    assert_eq!(shape, PLAIN);
}

/// Spec §6.3: a rule on `position_pnl_fraction` denies an increase into a losing position, which is
/// how an owner forbids averaging down. `MC-B24`.
#[test]
#[ignore = "pending E6-2"]
fn an_averaging_down_rule_denies_an_increase() {
    let mut rules = vec![rule(
        "no_averaging_down",
        compare(
            Field::PositionPnlFraction,
            Op::Lt,
            Value::Signed(mandate_num::Signed::ZERO),
        ),
        Decision::Deny,
    )];
    rules.extend(base_rules());
    let policy = policy_with(rules, Decision::Ask, Decision::Ask, None);
    let losing = mandate_builder::RiskContext {
        position_pnl_fraction: mandate_num::Signed::parse("-0.05").unwrap(),
        ..quiet_risk()
    };
    let full = vec![
        fresh_output(&momentum_model(), XYZ, "1", "1"),
        fresh_output(&news_model(), XYZ, "1", "1"),
    ];
    let proposal = propose(
        &swing_mandate(),
        &held("7", "750"),
        &equity_market(),
        &losing,
        &full,
        time(NOW),
    )
    .unwrap();
    assert_eq!(buy_of(&proposal).0, Purpose::Increase);
    match decide(&policy, &proposal, GateVerdict::Allow).unwrap() {
        Outcome::Classified(autonomy) => {
            assert_eq!(autonomy.decision, Decision::Deny);
            assert_eq!(
                autonomy.by,
                DecidedBy::Rule(mandate_builder::RuleId::new("no_averaging_down"))
            );
            assert!(autonomy.approval.is_none());
        }
        other => panic!("expected a classification, got {other:?}"),
    }
}

/// A hold is [`Outcome::NotProposed`] whatever the verdict says, because there is no order to gate.
#[test]
#[ignore = "pending E6-2"]
fn a_hold_has_no_outcome_to_classify() {
    let proposal = propose(
        &swing_mandate(),
        &flat_account(),
        &equity_market(),
        &quiet_risk(),
        &[],
        time(NOW),
    )
    .unwrap();
    for verdict in [GateVerdict::Allow, GateVerdict::Deny, GateVerdict::Defer] {
        assert_eq!(
            decide(&base_policy(), &proposal, verdict).unwrap(),
            Outcome::NotProposed
        );
    }
}

/// DEC-130 item 17: sizing off a crossed quote would price an order against a market that does not
/// exist, so the quote is refused. `reference/mandate/ref.py` does not raise this and no committed
/// case reaches it; it is the one refusal in `BuilderError` the reference implementation would not
/// produce.
#[test]
#[ignore = "pending E6-2"]
fn a_crossed_quote_is_refused_rather_than_sized() {
    let crossed = mandate_builder::Market {
        bid: price("101"),
        ask: price("100"),
        ..equity_market()
    };
    let err = propose(
        &swing_mandate(),
        &flat_account(),
        &crossed,
        &quiet_risk(),
        &swing_outputs(),
        time(NOW),
    )
    .unwrap_err();
    assert_eq!(err.code(), "crossed_quote");
}

/// Spec §8.1: a mandate with no signal model has no conviction to combine, and an agent whose weights
/// sum to zero has no denominator, so both are errors rather than a zero that reads as bearish.
#[test]
#[ignore = "pending E6-2"]
fn a_mandate_without_models_or_weight_is_refused() {
    assert_eq!(
        combine(&[], &[], time(NOW)).unwrap_err().code(),
        "no_signal_models"
    );
    let mut weightless = momentum_model();
    weightless.weight = SizeFraction::ZERO;
    assert_eq!(
        combine(&[weightless], &[], time(NOW)).unwrap_err().code(),
        "weight_sum_zero"
    );
}

/// Every error the crate can return has a stable code (ES-09), and `unimplemented` is the stubs' own,
/// which the implementation PR removes.
#[test]
fn error_codes_are_stable() {
    let codes = [
        (BuilderError::Unimplemented, "unimplemented"),
        (BuilderError::ConditionTooDeep, "condition_too_deep"),
        (
            BuilderError::ConditionTypeMismatch("purpose"),
            "condition_type_mismatch",
        ),
        (BuilderError::ReservedField, "reserved_field"),
        (
            BuilderError::DuplicateRuleId("same".to_owned()),
            "duplicate_rule_id",
        ),
        (BuilderError::NoSignalModels, "no_signal_models"),
        (BuilderError::WeightSumZero, "weight_sum_zero"),
        (BuilderError::CrossedQuote, "crossed_quote"),
        (
            BuilderError::AccumulateInstrumentMismatch,
            "accumulate_instrument_mismatch",
        ),
        (BuilderError::Num(NumError::TooPrecise), "too_precise"),
    ];
    for (error, code) in codes {
        assert_eq!(error.code(), code, "{error:?}");
    }
}
