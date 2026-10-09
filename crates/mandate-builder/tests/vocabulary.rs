//! The crate's **live** code: the stable reason codes (ES-09), the labels and spellings the journal
//! and the reference cases use, the model-version grammar, and the projection of an action onto the
//! §6.3 field names.
//!
//! These are live tests, not pending ones, and the crate needs them for two reasons.
//!
//! The first is DEC-128 item 22: this is the code every other E6-2 test needs in order to construct
//! or read a value at all, so a pending test that passed on a stub of it would pin nothing (DEC-110).
//!
//! The second is the mutation gate, and it is worth stating because it caught nobody's eye until this
//! PR ran it. DEC-137 makes the gate run on a crate that still has pending tests, exempting only
//! `Unimplemented` stub bodies — but `cargo mutants` tests each mutant with
//! `cargo nextest run --package=<the mutated crate>`, and a crate whose every test is `#[ignore]`d
//! answers that with "no tests to run" and a **non-zero exit**. cargo-mutants reads that as a test
//! failure, so it reported all 30 `mandate-builder` mutants as caught when nothing had run. One live
//! test is what makes the gate's verdict on this crate mean anything; these pin every live item, so
//! the verdict is also true.

use std::num::NonZeroU8;

use mandate_builder::{
    ActionContext, BuilderError, Clip, DecidedBy, HoldReason, ModelVersion, OrderShape, RequestedBy,
};
use mandate_domain::{AssetClass, AutonomyDecision, MarketSession, Purpose};
use mandate_num::{NumError, Ratio, Signed, Unit, Usd};
use mandate_spec::condition::{ConditionField, Facts};
use mandate_spec::document::RuleId;
use mandate_time::Date;

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("`{text}` is a USD amount: {e}"))
}

fn unit(text: &str) -> Unit {
    Unit::parse(text).unwrap_or_else(|e| panic!("`{text}` is a unit value: {e}"))
}

fn signed(text: &str) -> Signed {
    Signed::parse(text).unwrap_or_else(|e| panic!("`{text}` is a signed value: {e}"))
}

fn ratio(text: &str) -> Ratio {
    Ratio::parse(text).unwrap_or_else(|e| panic!("`{text}` is a ratio: {e}"))
}

/// ES-09: every refusal has a distinct, stable code, and no two share one.
#[test]
fn every_builder_error_has_its_own_stable_code() {
    let named: Vec<(BuilderError, &str)> = vec![
        (BuilderError::Unimplemented, "unimplemented"),
        (
            BuilderError::UnsupportedSizingMethod,
            "unsupported_sizing_method",
        ),
        (BuilderError::ConditionTooDeep, "condition_too_deep"),
        (
            BuilderError::ConditionTypeMismatch,
            "condition_type_mismatch",
        ),
        (BuilderError::ReservedField, "reserved_field"),
        (BuilderError::DuplicateRuleId, "duplicate_rule_id"),
        (BuilderError::NoSignalModels, "no_signal_models"),
        (BuilderError::WeightSumZero, "weight_sum_zero"),
        (BuilderError::CrossedQuote, "crossed_quote"),
        (
            BuilderError::AccumulateInstrumentMismatch,
            "accumulate_instrument_mismatch",
        ),
        (
            BuilderError::MalformedModelVersion,
            "malformed_model_version",
        ),
        (BuilderError::NothingProposed, "nothing_proposed"),
        (BuilderError::UntradableSession, "untradable_session"),
        (
            BuilderError::OutputInstrumentMismatch,
            "output_instrument_mismatch",
        ),
        (BuilderError::NoOpeningForm, "no_opening_form"),
    ];
    for (error, code) in &named {
        assert_eq!(error.code(), *code, "{error:?}");
        assert!(
            !error.to_string().is_empty(),
            "{error:?} has a message an operator can read"
        );
    }
    let mut codes: Vec<&str> = named.iter().map(|(_, code)| *code).collect();
    codes.sort_unstable();
    let count = codes.len();
    codes.dedup();
    assert_eq!(codes.len(), count, "no two refusals share a code");

    assert_eq!(
        BuilderError::from(NumError::TooPrecise).code(),
        NumError::TooPrecise.code(),
        "a wrapped numeric refusal keeps its own code, so `too_precise` reaches the caller"
    );
    assert_eq!(
        BuilderError::from(NumError::Overflow).code(),
        "overflow",
        "and so does `overflow`, which is how an input too wide refuses the order (DEC-130 item 8)"
    );
}

/// §6.2: `by` is written the way the reference cases and the journal write it.
#[test]
fn decided_by_is_labelled_the_way_the_reference_cases_write_it() {
    assert_eq!(
        DecidedBy::BuiltinRiskReducing.label(),
        "builtin_risk_reducing"
    );
    assert_eq!(DecidedBy::Default.label(), "default");
    assert_eq!(DecidedBy::AdmissionCeiling.label(), "admission_ceiling");
    assert_eq!(
        DecidedBy::ClientCeiling.label(),
        "client_ceiling",
        "§6.4's `decided_by` and DEC-252's `DecisionMade` spell the client ceiling this way"
    );
    assert_eq!(
        DecidedBy::ReviewCeiling.label(),
        "review_ceiling",
        "§6.4's `decided_by`, the journal spec's `DecisionMade`, and MC-D spell the review ceiling \
         this way"
    );
    let id = RuleId::parse("low_score").unwrap_or_else(|e| panic!("a rule id: {e}"));
    assert_eq!(
        DecidedBy::Rule(id.clone()).label(),
        "rule:low_score",
        "a rule's label is `rule:` and its id, as MC-A05 to MC-A16 write it"
    );
    assert_ne!(
        DecidedBy::Rule(id).label(),
        DecidedBy::Default.label(),
        "a rule and the default are never the same label"
    );
}

/// §8.3: every hold reason is spelled as the `B` family spells it, and no two alike.
#[test]
fn every_hold_reason_clip_and_shape_has_its_own_spelling() {
    let reasons: [(HoldReason, &str); 9] = [
        (HoldReason::NoFreshOutputs, "no_fresh_outputs"),
        (HoldReason::NoPosition, "no_position"),
        (
            HoldReason::DiscretionaryExitsDisabled,
            "discretionary_exits_disabled",
        ),
        (HoldReason::BetweenThresholds, "between_thresholds"),
        (HoldReason::AtOrAboveTarget, "at_or_above_target"),
        (HoldReason::WithinRebalanceBand, "within_rebalance_band"),
        (
            HoldReason::BelowBandAfterClipping,
            "below_band_after_clipping",
        ),
        (
            HoldReason::WouldExceedMaxAvgPrice,
            "would_exceed_max_avg_price",
        ),
        (
            HoldReason::BelowMinimumAfterClipping,
            "below_minimum_after_clipping",
        ),
    ];
    for (reason, spelling) in reasons {
        assert_eq!(reason.as_str(), spelling, "{reason:?}");
    }
    let mut spellings: Vec<&str> = reasons.iter().map(|(_, s)| *s).collect();
    spellings.sort_unstable();
    let count = spellings.len();
    spellings.dedup();
    assert_eq!(
        spellings.len(),
        count,
        "no two hold reasons share a spelling"
    );

    assert_eq!(Clip::Limits.as_str(), "limits");
    assert_eq!(Clip::Goal.as_str(), "goal");
    assert_ne!(Clip::Limits.as_str(), Clip::Goal.as_str());
    assert_eq!(OrderShape::Limit.as_str(), "limit");
    assert_eq!(OrderShape::MarketableLimit.as_str(), "marketable_limit");
    assert_ne!(
        OrderShape::Limit.as_str(),
        OrderShape::MarketableLimit.as_str()
    );
}

/// §8.1: a model version is the schema's `major.minor.patch` of at most six digits a part, and
/// nothing wider. Leading zeros are rejected rather than folded, so two spellings of one version can
/// never both pin a model.
#[test]
fn a_model_version_is_the_schemas_three_part_form() {
    for accepted in [
        "0.0.0",
        "1.0.0",
        "0.3.0",
        "999999.999999.999999",
        "10.20.30",
    ] {
        assert!(
            ModelVersion::parse(accepted).is_ok_and(|v| v.as_str() == accepted),
            "`{accepted}` is a model version"
        );
    }
    for refused in [
        "",
        "1",
        "1.0",
        "1.0.0.0",
        "01.0.0",
        "1.00.0",
        "1.0.0-rc1",
        "v1.0.0",
        "1.0.x",
        "1000000.0.0",
        "1..0",
        " 1.0.0",
    ] {
        assert_eq!(
            ModelVersion::parse(refused)
                .map(|v| v.as_str().to_owned())
                .map_err(|e| e.code()),
            Err("malformed_model_version"),
            "`{refused}` is not a model version"
        );
    }
}

fn an_action() -> ActionContext {
    ActionContext {
        purpose: Purpose::Increase,
        order_usd: usd("400"),
        combined_score: unit("0.74"),
        instrument: mandate_domain::AssetId::parse("7b4a1c2e-2222-4a2b-9c3d-000000000002")
            .unwrap_or_else(|e| panic!("an asset id: {e}")),
        asset_class: AssetClass::UsEquity,
        session: MarketSession::AfterHours,
        first_trade_in_instrument: true,
        new_instrument: false,
        thesis_confidence: unit("0.5"),
        drawdown: unit("0.02"),
        daily_pnl_fraction: signed("-0.01"),
        position_usd_after: usd("700"),
        gross_usd_after: usd("900"),
        bought_today_usd: usd("1100"),
        position_pnl_fraction: signed("0.05"),
        requested_by: RequestedBy::Agent,
        risk_day: Date::parse("2026-09-22").unwrap_or_else(|e| panic!("a date: {e}")),
    }
}

/// §6.3: every field a rule may read projects onto the value the action holds, and the fields it may
/// not read are absent rather than defaulted.
///
/// `Condition::matches` treats an absent fact as an **error**, not as a false, so a field wired to the
/// wrong value and a field wired to `None` are different failures and both matter.
#[test]
fn every_condition_field_projects_onto_the_value_the_action_holds() {
    let action = an_action();
    assert_eq!(action.enum_field(ConditionField::Purpose), Some("increase"));
    assert_eq!(
        action.enum_field(ConditionField::AssetClass),
        Some("us_equity")
    );
    assert_eq!(
        action.enum_field(ConditionField::Session),
        Some("after_hours"),
        "§6.3's four session names are `MarketSession`'s own spellings (DEC-128 item 23)"
    );
    assert_eq!(
        action.enum_field(ConditionField::Instrument),
        Some("7b4a1c2e-2222-4a2b-9c3d-000000000002")
    );
    for absent in [
        ConditionField::OrderUsd,
        ConditionField::NewInstrument,
        ConditionField::UnusualInput,
    ] {
        assert_eq!(action.enum_field(absent), None, "{absent:?} is not text");
    }

    for (field, expected) in [
        (ConditionField::OrderUsd, "400"),
        (ConditionField::CombinedScore, "0.74"),
        (ConditionField::ThesisConfidence, "0.5"),
        (ConditionField::Drawdown, "0.02"),
        (ConditionField::DailyPnlFraction, "-0.01"),
        (ConditionField::PositionUsdAfter, "700"),
        (ConditionField::GrossUsdAfter, "900"),
        (ConditionField::BoughtTodayUsd, "1100"),
        (ConditionField::PositionPnlFraction, "0.05"),
    ] {
        assert_eq!(
            action.decimal_field(field),
            Some(ratio(expected)),
            "{field:?} reads {expected}"
        );
    }
    for absent in [
        ConditionField::Purpose,
        ConditionField::FirstTradeInInstrument,
        ConditionField::UnusualInput,
    ] {
        assert_eq!(
            action.decimal_field(absent),
            None,
            "{absent:?} is not a decimal"
        );
    }

    assert_eq!(
        action.bool_field(ConditionField::FirstTradeInInstrument),
        Some(true)
    );
    assert_eq!(
        action.bool_field(ConditionField::NewInstrument),
        Some(false)
    );
    assert_eq!(
        action.bool_field(ConditionField::UnusualInput),
        None,
        "V-018 reserves it, so no fact is offered and `classify` refuses the rule first"
    );
    for absent in [ConditionField::Purpose, ConditionField::OrderUsd] {
        assert_eq!(action.bool_field(absent), None, "{absent:?} is not a flag");
    }

    let opened = ActionContext {
        purpose: Purpose::Open,
        ..an_action()
    };
    assert_eq!(opened.enum_field(ConditionField::Purpose), Some("open"));
    for (purpose, spelling) in [
        (Purpose::DiscretionaryExit, "discretionary_exit"),
        (Purpose::OwnerExit, "owner_exit"),
        (Purpose::RiskExit, "risk_exit"),
        (Purpose::Protective, "protective"),
    ] {
        let reducing = ActionContext {
            purpose,
            ..an_action()
        };
        assert_eq!(
            reducing.enum_field(ConditionField::Purpose),
            Some(spelling),
            "a rule never sees {purpose:?}, but the projection still names it rather than guessing"
        );
    }
    let crypto = ActionContext {
        asset_class: AssetClass::Crypto,
        session: MarketSession::Crypto,
        ..an_action()
    };
    assert_eq!(
        crypto.enum_field(ConditionField::AssetClass),
        Some("crypto")
    );
    assert_eq!(crypto.enum_field(ConditionField::Session), Some("crypto"));
}

/// §6.4: an approver count is a `NonZeroU8`, so "an ASK nobody has to answer" is unrepresentable.
#[test]
fn an_approval_always_needs_at_least_one_approver() {
    assert_eq!(NonZeroU8::MIN.get(), 1);
    assert_eq!(NonZeroU8::new(0), None, "zero approvers is not a count");
    assert!(
        AutonomyDecision::Auto < AutonomyDecision::Ask
            && AutonomyDecision::Ask < AutonomyDecision::Deny,
        "the strictness order the admission ceiling takes its maximum over (MI-17)"
    );
}
