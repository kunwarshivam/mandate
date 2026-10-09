//! Alpaca's capability profile (E7-23 B1, B2a): trading spec §5.2's US equities rows and crypto's
//! one resting stop-limit as data (DEC-531 items 5 and 6, DEC-630, DEC-838), handed to the executor
//! through `BrokerConnector::profile` without a request to the broker.

mod common;

use common::{FakeClock, FakeTransport};
use mandate_alpaca::{RetryPolicy, TradingClient, alpaca_profile};
use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_domain::{
    AssetClass, MarketSession, OrderType, ProtectionForm, QuantityForm, TimeInForce,
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

/// Crypto's row (E7-23 B2a): simple orders only, and its one resting stop-limit goes as `gtc`
/// (§5.2), in whole and fractional quantities alike (DEC-838 item 3).
const CRYPTO_ROW: &str = concat!(
    r#"{"asset_class":"crypto","cells":["#,
    r#"{"order_type":"stop_limit","protection_forms":["stop_limit"],"quantity_form":"fractional","#,
    r#""times_in_force":["gtc"]},"#,
    r#"{"order_type":"stop_limit","protection_forms":["stop_limit"],"quantity_form":"whole","#,
    r#""times_in_force":["gtc"]}"#,
    r#"],"session":"crypto"}"#,
);

/// The row of `asset_class` in a canonical profile object, found by name.
fn row(profile: &Value, asset_class: &str) -> Option<Value> {
    let rows = profile.get("rows").and_then(Value::as_array)?;
    let named = |row: &&Value| row.get("asset_class").and_then(Value::as_str) == Some(asset_class);
    rows.iter().find(named).cloned()
}

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
    let declared = profile.canonical();
    let expected = parse(ALPACA.as_bytes()).unwrap();
    for member in ["idempotency", "kind", "profile_version"] {
        assert_eq!(declared.get(member), expected.get(member), "{member}");
    }
    assert_eq!(row(declared, "us_equity"), row(&expected, "us_equity"));
    assert!(row(declared, "us_equity").is_some());
}

#[test]
fn a_fractional_or_notional_order_is_day_only_and_never_protective() {
    let profile = alpaca_profile().unwrap();
    let equities = row(profile.canonical(), "us_equity").unwrap();
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
fn the_connector_hands_the_executor_its_profile_without_calling_the_broker() {
    let transport = FakeTransport::default();
    let client = TradingClient::new(
        transport.clone(),
        FakeClock::at(UtcNanos::EPOCH),
        RetryPolicy::default(),
    );
    let declared = BrokerConnector::profile(&client).unwrap();
    assert_eq!(declared, alpaca_profile().unwrap());
    assert_eq!(
        declared.content_hash(),
        Digest::of(&to_canonical(declared.canonical()))
    );
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
}

/// E7-23 B2a (DEC-838 items 3 and 5): Alpaca's profile is the equities row and crypto's, exactly,
/// and the connector hands over crypto's one resting stop-limit, whole and fractional.
#[test]
#[ignore = "pending E7-23"]
fn alpacas_profile_declares_cryptos_one_resting_stop_limit() {
    let profile = alpaca_profile().unwrap();
    let Ok(Value::Object(mut expected)) = parse(ALPACA.as_bytes()) else {
        panic!("ALPACA is a canonical object");
    };
    let equities = row(&Value::Object(expected.clone()), "us_equity").unwrap();
    let rows = vec![parse(CRYPTO_ROW.as_bytes()).unwrap(), equities];
    expected.insert(Key::new("rows").unwrap(), Value::Array(rows));
    let expected = to_canonical(&Value::Object(expected));
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    assert_eq!(
        text(to_canonical(profile.canonical())),
        text(expected.clone()),
        "two rows, crypto's first"
    );
    assert_eq!(profile.content_hash(), Digest::of(&expected));
    let client = TradingClient::new(
        FakeTransport::default(),
        FakeClock::at(UtcNanos::EPOCH),
        RetryPolicy::default(),
    );
    let declared = BrokerConnector::profile(&client).unwrap();
    for quantity_form in [QuantityForm::Whole, QuantityForm::Fractional] {
        let crypto = declared
            .cell(
                AssetClass::Crypto,
                MarketSession::Crypto,
                OrderType::StopLimit,
                quantity_form,
            )
            .map(|cell| (cell.times_in_force.clone(), cell.protection_forms.clone()));
        let stop_limit = (
            [TimeInForce::Gtc].into(),
            [ProtectionForm::StopLimit].into(),
        );
        assert_eq!(crypto, Ok(stop_limit), "DEC-36, DEC-838 item 3");
    }
}
