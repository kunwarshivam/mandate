//! The capability profile (E7-23 B1, DEC-531, DEC-630): its refusals, its canonical object, and
//! a hash that follows the profile's content and nothing else.

use std::collections::BTreeSet;

use mandate_domain::{
    AssetClass, CapabilityProfile, Cell, Idempotency, MarketSession, OrderType, ProfileError,
    ProtectionForm, QuantityForm, Retry, Row, TimeInForce,
};

fn cell(
    order_type: OrderType,
    quantity_form: QuantityForm,
    tifs: &[TimeInForce],
    protection: &[ProtectionForm],
) -> Cell {
    Cell {
        order_type,
        quantity_form,
        times_in_force: tifs.iter().copied().collect(),
        protection_forms: protection.iter().copied().collect(),
    }
}

/// Two rows, given out of canonical order, and a broker whose retries are of unknown effect.
fn rows() -> Vec<Row> {
    vec![
        Row {
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            cells: vec![
                cell(
                    OrderType::Market,
                    QuantityForm::Notional,
                    &[TimeInForce::Day],
                    &[],
                ),
                cell(
                    OrderType::Limit,
                    QuantityForm::Whole,
                    &[TimeInForce::Gtc, TimeInForce::Day],
                    &[ProtectionForm::Oco, ProtectionForm::Bracket],
                ),
            ],
        },
        Row {
            asset_class: AssetClass::Crypto,
            session: MarketSession::Crypto,
            cells: vec![cell(
                OrderType::StopLimit,
                QuantityForm::Fractional,
                &[TimeInForce::Ioc, TimeInForce::Gtc],
                &[ProtectionForm::StopLimit],
            )],
        },
    ]
}

const IDEMPOTENCY: Idempotency = Idempotency {
    client_order_id: true,
    retry: Retry::Unknown,
    query_by_client_order_id: false,
};

#[test]
#[ignore = "pending E7-23"]
fn each_defect_is_refused_with_its_own_reason() {
    let only = |cells: Vec<Cell>| {
        vec![Row {
            asset_class: AssetClass::UsEquity,
            session: MarketSession::Regular,
            cells,
        }]
    };
    let whole = || {
        cell(
            OrderType::Limit,
            QuantityForm::Whole,
            &[TimeInForce::Day],
            &[],
        )
    };
    let refuse = |version, rows, idempotency, reason| {
        assert_eq!(
            CapabilityProfile::new(version, rows, idempotency),
            Err(reason)
        );
    };
    refuse(0, rows(), IDEMPOTENCY, ProfileError::VersionZero);
    refuse(1, vec![], IDEMPOTENCY, ProfileError::NoRows);
    refuse(
        1,
        [only(vec![whole()]), only(vec![whole()])].concat(),
        IDEMPOTENCY,
        ProfileError::DuplicateRow,
    );
    refuse(1, only(vec![]), IDEMPOTENCY, ProfileError::EmptyRow);
    refuse(
        1,
        only(vec![whole(), whole()]),
        IDEMPOTENCY,
        ProfileError::DuplicateCell,
    );
    let no_tif = cell(OrderType::Limit, QuantityForm::Whole, &[], &[]);
    refuse(
        1,
        only(vec![no_tif]),
        IDEMPOTENCY,
        ProfileError::NoTimeInForce,
    );
    let without = Idempotency {
        client_order_id: false,
        retry: Retry::NotIdempotent,
        query_by_client_order_id: false,
    };
    for claim in [
        Idempotency {
            retry: Retry::Idempotent,
            ..without
        },
        Idempotency {
            retry: Retry::Unknown,
            ..without
        },
        Idempotency {
            query_by_client_order_id: true,
            ..without
        },
    ] {
        refuse(
            1,
            only(vec![whole()]),
            claim,
            ProfileError::ClaimWithoutClientId,
        );
    }
    assert!(CapabilityProfile::new(1, only(vec![whole()]), without).is_ok());
}

#[test]
fn every_profile_error_has_a_distinct_stable_code() {
    let errors = [
        ProfileError::Unimplemented,
        ProfileError::VersionZero,
        ProfileError::NoRows,
        ProfileError::DuplicateRow,
        ProfileError::EmptyRow,
        ProfileError::DuplicateCell,
        ProfileError::NoTimeInForce,
        ProfileError::ClaimWithoutClientId,
        ProfileError::NonCanonical,
    ];
    let codes: BTreeSet<&str> = errors.iter().map(|e| e.code()).collect();
    assert_eq!(codes.len(), errors.len(), "ES-09: one code per variant");
    assert!(codes.iter().all(|c| !c.is_empty() && c.is_ascii()));
}
