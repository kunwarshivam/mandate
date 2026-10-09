//! Alpaca's capability profile (E7-23 B1, B2a): trading spec §5.2's US equities rows and crypto's
//! one resting stop-limit as data (DEC-531 items 5 and 6, DEC-630, DEC-838), handed to the executor
//! through `BrokerConnector::profile` without a request to the broker.

mod common;

use common::{FakeClock, FakeTransport};
use mandate_alpaca::{RetryPolicy, TradingClient, alpaca_profile};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::{
    AssetClass, MarketSession, OrderType, ProtectionForm, QuantityForm, TimeInForce,
};
use mandate_executor::{
    BrokerConnector, OcoLegs, OrderType as ExecutorOrderType, ProtectionPrices,
    TimeInForce as ExecutorTif, protective_shape,
};
use mandate_num::{Fraction, Price, Qty};
use mandate_time::UtcNanos;

/// The profile written by hand from §5.2 and Alpaca's order documentation, as DEC-630 item 1
/// reads them. Whole shares take `day` or `gtc`. A cell lists the forms its order may be sent as:
/// a market or limit order may be a bracket's entry, a limit order an OCO's parent, and a stop or
/// stop-limit order neither, since the documentation names no such entry and the tighter set is
/// taken (DEC-176). No equity order is the resting stop-limit, which DEC-36 gives to crypto.
/// Fractional and notional orders take `day` only and no OCO or bracket. Alpaca refuses a second
/// order with a client order id it holds (the recorded `submit_duplicate_client_order_id`) and
/// answers `/v2/orders:by_client_order_id`. Crypto takes simple orders only, and its one resting
/// stop-limit goes as `gtc` (§5.2), in whole and fractional quantities alike (DEC-838 item 3).
const ALPACA: &str = concat!(
    r#"{"idempotency":{"client_order_id":true,"query_by_client_order_id":true,"retry":"idempotent"},"#,
    r#""kind":"broker_profile","profile_version":1,"rows":[{"asset_class":"crypto","cells":["#,
    r#"{"order_type":"stop_limit","protection_forms":["stop_limit"],"quantity_form":"fractional","#,
    r#""times_in_force":["gtc"]},"#,
    r#"{"order_type":"stop_limit","protection_forms":["stop_limit"],"quantity_form":"whole","#,
    r#""times_in_force":["gtc"]}"#,
    r#"],"session":"crypto"},{"asset_class":"us_equity","cells":["#,
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
#[ignore = "pending E7-23"]
fn alpaca_declares_trading_spec_5_2_and_dec_630s_object() {
    let profile = alpaca_profile().unwrap();
    assert_eq!(
        String::from_utf8(to_canonical(profile.canonical())).unwrap(),
        ALPACA
    );
    assert_eq!(profile.content_hash(), Digest::of(ALPACA.as_bytes()));
}

#[test]
#[ignore = "pending E7-23"]
fn a_fractional_or_notional_order_is_day_only_and_never_protective() {
    let profile = alpaca_profile().unwrap();
    let rows = profile
        .canonical()
        .get("rows")
        .and_then(Value::as_array)
        .unwrap();
    assert_eq!(
        rows.len(),
        2,
        "DEC-531 item 6: US equities in the regular session, and crypto (B2a)"
    );
    let equities = rows
        .iter()
        .find(|row| row.get("asset_class").and_then(Value::as_str) == Some("us_equity"))
        .unwrap();
    let cells = equities.get("cells").and_then(Value::as_array).unwrap();
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
#[ignore = "pending E7-23"]
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
    for quantity_form in [QuantityForm::Whole, QuantityForm::Fractional] {
        let crypto = declared
            .cell(
                AssetClass::Crypto,
                MarketSession::Crypto,
                OrderType::StopLimit,
                quantity_form,
            )
            .map(|cell| (cell.times_in_force.clone(), cell.protection_forms.clone()));
        assert_eq!(
            crypto,
            Ok((
                [TimeInForce::Gtc].into(),
                [ProtectionForm::StopLimit].into()
            )),
            "DEC-36, DEC-838 item 3: crypto's one resting stop-limit, whole or fractional"
        );
    }
}

/// LT-14 through the executor's own reading (DEC-838): Alpaca's profile protects an equity with
/// a take-profit by a GTC OCO, and crypto, whole or fractional, by one GTC stop-limit at
/// stop x (1 - offset), whatever the clock's session.
#[test]
#[ignore = "pending E7-23"]
fn alpacas_profile_protects_an_equity_by_oco_and_crypto_by_one_stop_limit() {
    let profile = alpaca_profile().unwrap();
    let price = |text| Price::parse(text).unwrap();
    let prices = ProtectionPrices {
        stop: price("140"),
        take_profit: Some(price("170")),
    };
    let offset = Some(Fraction::parse("0.01").unwrap());
    let shape = |class, quantity| {
        let quantity = Qty::parse(quantity).unwrap();
        protective_shape(
            &profile,
            class,
            MarketSession::PreMarket,
            quantity,
            prices,
            offset,
        )
        .unwrap()
        .unwrap()
    };
    let equity = shape(AssetClass::UsEquity, "10");
    assert_eq!(
        (equity.form, equity.order_type, equity.tif),
        (
            ProtectionForm::Oco,
            ExecutorOrderType::Limit,
            ExecutorTif::Gtc
        )
    );
    let legs = OcoLegs {
        take_profit: price("170"),
        stop: price("140"),
        qty: Qty::parse("10").unwrap(),
    };
    assert_eq!(equity.oco, Some(legs));
    for quantity in ["2", "0.5"] {
        let crypto = shape(AssetClass::Crypto, quantity);
        assert_eq!(
            (crypto.form, crypto.order_type, crypto.tif, crypto.oco),
            (
                ProtectionForm::StopLimit,
                ExecutorOrderType::StopLimit,
                ExecutorTif::Gtc,
                None
            ),
            "{quantity}"
        );
        assert_eq!(
            (crypto.stop_price, crypto.limit_price),
            (Some(price("140")), Some(price("138.6"))),
            "{quantity}"
        );
    }
}
