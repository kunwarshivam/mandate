//! Production projections from a validated mandate into the two views the decision path reads.

use std::collections::{BTreeMap, BTreeSet};
use std::ptr;

use mandate_builder::{BuilderMandate, GoalKind, autonomy_policy};
use mandate_domain::Environment;
use mandate_num::{Price, Qty, SizeFraction, Usd};
use mandate_spec::document::{Goal, ProvenanceMap};
use mandate_spec::{Mandate, ValidatedMandate, ValidationContext};
use mandate_time::Date;
use proptest::prelude::*;
use serde_json::Value;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../../../fixtures/refcases/mandate.json"))
        .unwrap_or_else(|e| panic!("the mandate reference fixture parses: {e}"))
}

fn mandate_value(base: &str) -> Value {
    fixture()
        .pointer(&format!("/bases/{base}/mandate"))
        .unwrap_or_else(|| panic!("the fixture has the `{base}` base"))
        .clone()
}

fn validated_value(value: &Value) -> ValidatedMandate {
    let encoded =
        serde_json::to_vec(value).unwrap_or_else(|e| panic!("the fixture serializes: {e}"));
    let canonical = mandate_canon::parse(&encoded)
        .unwrap_or_else(|e| panic!("the fixture is canonical JSON: {e}"));
    let mandate =
        Mandate::parse(&canonical).unwrap_or_else(|e| panic!("the fixture is a mandate: {e}"));
    let context = ValidationContext {
        account_equity_usd: Usd::parse("100000")
            .unwrap_or_else(|e| panic!("the account equity parses: {e}")),
        other_allocations_usd: Usd::ZERO,
        validation_date: Date::parse("2026-09-20")
            .unwrap_or_else(|e| panic!("the validation date parses: {e}")),
        registry: None,
        provenance: ProvenanceMap::default(),
        workspace_users: 2,
        approver_users: 2,
        independent_approval_required: false,
        disclosures_accepted: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        connection_environment: Some(Environment::Paper),
        connection_loss_carry_usd: Usd::ZERO,
        eligibility_failures: BTreeSet::new(),
        previous_version: None,
        current_mandate_version: None,
    };
    ValidatedMandate::new(mandate, &context, &[])
        .unwrap_or_else(|e| panic!("the fixture mandate validates: {e}"))
}

fn validated(base: &str) -> ValidatedMandate {
    validated_value(&mandate_value(base))
}

fn assert_projection(base: &str) {
    let validated = validated(base);
    let document = validated.mandate();
    let projected = BuilderMandate::try_from(&validated)
        .unwrap_or_else(|e| panic!("the validated mandate projects: {e}"));

    assert_eq!(
        projected.models.len(),
        document.behavior.signal_models.len()
    );
    for (actual, expected) in projected
        .models
        .iter()
        .zip(&document.behavior.signal_models)
    {
        assert_eq!(actual.id, expected.id);
        assert_eq!(actual.version.as_str(), expected.version);
        assert_eq!(actual.content_hash, expected.content_hash);
        assert_eq!(
            actual.weight,
            SizeFraction::parse(expected.weight.as_str())
                .unwrap_or_else(|e| panic!("the model weight fits: {e}"))
        );
        assert_eq!(actual.max_output_age_s, expected.max_output_age_s);
    }

    let sizing = &document.behavior.sizing;
    assert_eq!(projected.sizing.method, sizing.method);
    assert_eq!(
        projected.sizing.entry_threshold,
        SizeFraction::parse(sizing.entry_threshold.as_str())
            .unwrap_or_else(|e| panic!("the entry threshold fits: {e}"))
    );
    assert_eq!(
        projected.sizing.exit_threshold,
        SizeFraction::parse(sizing.exit_threshold.as_str())
            .unwrap_or_else(|e| panic!("the exit threshold fits: {e}"))
    );
    assert_eq!(
        projected.sizing.rebalance_band,
        SizeFraction::parse(sizing.rebalance_band.as_str())
            .unwrap_or_else(|e| panic!("the rebalance band fits: {e}"))
    );

    let risk = &document.risk;
    assert_eq!(
        projected.limits.max_position_usd,
        Usd::parse(risk.max_position_usd.as_str())
            .unwrap_or_else(|e| panic!("the position limit fits: {e}"))
    );
    assert_eq!(
        projected.limits.max_position_fraction,
        SizeFraction::parse(risk.max_position_fraction.as_str())
            .unwrap_or_else(|e| panic!("the position fraction fits: {e}"))
    );
    assert_eq!(
        projected.limits.max_order_usd,
        Usd::parse(risk.max_order_usd.as_str())
            .unwrap_or_else(|e| panic!("the order limit fits: {e}"))
    );
    assert_eq!(
        projected.limits.max_gross_exposure_usd,
        Usd::parse(risk.max_gross_exposure_usd.as_str())
            .unwrap_or_else(|e| panic!("the gross limit fits: {e}"))
    );

    match (&projected.goal, &document.goal) {
        (GoalKind::Continuous, Goal::Continuous { .. })
        | (GoalKind::ProfitStop, Goal::ProfitStop { .. }) => {}
        (
            GoalKind::Accumulate(actual),
            Goal::Accumulate {
                instrument,
                target_qty,
                max_avg_price,
                max_spend_usd,
                ..
            },
        ) => {
            assert_eq!(&actual.instrument, instrument);
            assert_eq!(
                actual.target_qty,
                Qty::parse(target_qty.as_str())
                    .unwrap_or_else(|e| panic!("the target quantity fits: {e}"))
            );
            assert_eq!(
                actual.max_avg_price,
                max_avg_price.as_ref().map(|price| {
                    Price::parse(price.as_str())
                        .unwrap_or_else(|e| panic!("the average price fits: {e}"))
                })
            );
            assert_eq!(
                actual.max_spend_usd,
                Usd::parse(max_spend_usd.as_str())
                    .unwrap_or_else(|e| panic!("the spend limit fits: {e}"))
            );
        }
        (actual, expected) => {
            panic!("goal projection mismatch: projected {actual:?} from {expected:?}")
        }
    }

    assert!(ptr::eq(autonomy_policy(&validated), &document.autonomy));
}

#[test]
fn every_builder_and_autonomy_field_projects_from_the_validated_document() {
    for base in ["two_stock_swing", "btc_accumulator", "research_equity"] {
        assert_projection(base);
    }
}

proptest! {
    #[test]
    fn a_schema_valid_fraction_too_precise_for_sizing_fails_closed(
        final_digit in 1_u8..=9
    ) {
        let mut value = mandate_value("two_stock_swing");
        value["behavior"]["sizing"]["entry_threshold"] =
            Value::String(format!("0.123456789012{final_digit}"));
        let validated = validated_value(&value);

        let error = BuilderMandate::try_from(&validated)
            .expect_err("a 13-place size fraction must be refused");
        prop_assert_eq!(error.code(), "too_precise");
    }
}
