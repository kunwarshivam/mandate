//! Alpaca's capability profile (E7-23 B1): trading spec §5.2's US equities rows as data
//! (DEC-531 items 5 and 6, DEC-630), handed to the executor through
//! `BrokerConnector::profile` without a request to the broker.

mod common;

use common::{FakeClock, FakeTransport};
use mandate_alpaca::{RetryPolicy, TradingClient, alpaca_profile};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::{
    AssetClass, MarketSession, OrderType, ProfileError, ProtectionForm, QuantityForm,
};
use mandate_executor::BrokerConnector;
use mandate_time::UtcNanos;

/// The profile written by hand from §5.2 and Alpaca's order documentation, as DEC-630 item 1
/// reads them. Whole shares take `day` or `gtc`. A cell lists the forms its order may be sent as:
/// a market or limit order may be a bracket's entry, a limit order an OCO's parent, and a stop or
/// stop-limit order neither, since the documentation names no such entry and the tighter set is
/// taken (DEC-176). No equity order is the resting stop-limit, which DEC-36 gives to crypto.
/// Fractional and notional orders take `day` only and no OCO or bracket. Alpaca refuses a second
/// order with a client order id it holds (the recorded `submit_duplicate_client_order_id`) and
/// answers `/v2/orders:by_client_order_id`.
const ALPACA: &str = concat!(
    r#"{"idempotency":{"client_order_id":true,"query_by_client_order_id":true,"retry":"idempotent"},"#,
    r#""kind":"broker_profile","profile_version":1,"rows":[{"asset_class":"us_equity","cells":["#,
    r#"{"order_type":"limit","protection_forms":[],"quantity_form":"fractional","times_in_force":["day"]},"#,
    r#"{"order_type":"limit","protection_forms":[],"quantity_form":"notional","times_in_force":["day"]},"#,
    r#"{"order_type":"limit","protection_forms":["bracket","oco"],"quantity_form":"whole","#,
    r#""times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"market","protection_forms":[],"quantity_form":"fractional","times_in_force":["day"]},"#,
    r#"{"order_type":"market","protection_forms":[],"quantity_form":"notional","times_in_force":["day"]},"#,
    r#"{"order_type":"market","protection_forms":["bracket"],"quantity_form":"whole","#,
    r#""times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"stop","protection_forms":[],"quantity_form":"fractional","times_in_force":["day"]},"#,
    r#"{"order_type":"stop","protection_forms":[],"quantity_form":"notional","times_in_force":["day"]},"#,
    r#"{"order_type":"stop","protection_forms":[],"quantity_form":"whole","#,
    r#""times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"stop_limit","protection_forms":[],"quantity_form":"fractional","times_in_force":["day"]},"#,
    r#"{"order_type":"stop_limit","protection_forms":[],"quantity_form":"notional","times_in_force":["day"]},"#,
    r#"{"order_type":"stop_limit","protection_forms":[],"quantity_form":"whole","#,
    r#""times_in_force":["day","gtc"]}"#,
    r#"],"session":"regular"}]}"#,
);

fn texts(cell: &Value, key: &str) -> Vec<String> {
    let members = cell.get(key).and_then(Value::as_array).unwrap_or_default();
    members
        .iter()
        .filter_map(Value::as_str)
        .map(str::to_owned)
        .collect()
}

#[test]
fn alpaca_declares_trading_spec_5_2_and_dec_630s_object() {
    let profile = alpaca_profile().unwrap();
    assert_eq!(
        String::from_utf8(to_canonical(profile.canonical())).unwrap(),
        ALPACA
    );
    assert_eq!(profile.content_hash(), Digest::of(ALPACA.as_bytes()));
}

#[test]
fn a_fractional_or_notional_order_is_day_only_and_never_protective() {
    let profile = alpaca_profile().unwrap();
    let rows = profile
        .canonical()
        .get("rows")
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(
        rows.len(),
        1,
        "DEC-531 item 6: US equities in the regular session only"
    );
    let cells = rows[0].get("cells").and_then(Value::as_array).unwrap();
    assert_eq!(
        cells.len(),
        12,
        "four order types, three quantity forms each"
    );
    for cell in cells {
        let form = cell.get("quantity_form").and_then(Value::as_str).unwrap();
        let (tifs, protection) = (
            texts(cell, "times_in_force"),
            texts(cell, "protection_forms"),
        );
        if form == "whole" {
            assert_eq!(tifs, ["day", "gtc"], "{cell:?}");
            assert!(
                !protection.contains(&"stop_limit".to_owned()),
                "DEC-36: the resting stop-limit is crypto's: {cell:?}"
            );
        } else {
            assert_eq!(
                tifs,
                ["day"],
                "§5.2: fractional and notional are day only: {cell:?}"
            );
            assert!(
                protection.is_empty(),
                "§5.2: not allowed in OCO or bracket: {cell:?}"
            );
        }
    }
}

#[test]
fn the_connector_hands_the_executor_its_profile_without_calling_the_broker() {
    let transport = FakeTransport::default();
    let client = TradingClient::new(
        transport.clone(),
        FakeClock::at(UtcNanos::EPOCH),
        RetryPolicy::default(),
    );
    let declared = BrokerConnector::profile(&client).unwrap();
    assert_eq!(declared, alpaca_profile().unwrap());
    assert_eq!(declared.content_hash(), Digest::of(ALPACA.as_bytes()));
    assert!(
        transport.sent().is_empty(),
        "a profile is declared, never asked of the broker"
    );
    assert!(declared.idempotency().retry.may_resend_blindly());
    let protection = |order_type| {
        declared
            .cell(
                AssetClass::UsEquity,
                MarketSession::Regular,
                order_type,
                QuantityForm::Whole,
            )
            .map(|cell| cell.protection_forms.clone())
    };
    assert_eq!(
        protection(OrderType::Market),
        Ok([ProtectionForm::Bracket].into())
    );
    assert_eq!(
        protection(OrderType::Limit),
        Ok([ProtectionForm::Bracket, ProtectionForm::Oco].into()),
        "an OCO's parent is always a limit order"
    );
    assert_eq!(protection(OrderType::Stop), Ok([].into()));
    assert_eq!(protection(OrderType::StopLimit), Ok([].into()));
    assert_eq!(
        declared.cell(
            AssetClass::Crypto,
            MarketSession::Crypto,
            OrderType::StopLimit,
            QuantityForm::Fractional,
        ),
        Err(ProfileError::NotOffered),
        "DEC-531 item 6: crypto's row comes with B2a"
    );
}
