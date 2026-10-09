//! C1 tests part 1 (E7-6; DEC-860 items 1, 3, 5): the profile against trading spec §5.2, the
//! `ref_id` of an idempotency key, and connections spec §6.2's ten states. Oracles: the profile by
//! hand, `sha2` and a Python `hashlib` vector, and the spec's state table typed here.

use mandate_canon::{Digest, parse, to_canonical};
use mandate_domain::{AssetClass, MarketSession, ProtectionForm};
use mandate_executor::{
    BrokerConnector, ClientOrderId, EventId, IntentId, OrderType, ProtectionPrices, TimeInForce,
    protective_shape,
};
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Fraction, Price, Qty};
use mandate_robinhood::{
    RobinhoodConnector, RobinhoodError, Tools, executor_status, ref_id, robinhood_profile,
};
use proptest::test_runner::{Config, TestRunner};
use serde_json::Value;
use sha2::{Digest as _, Sha256};

/// §5.2's Robinhood rows, typed by hand.
const ROBINHOOD: &str = concat!(
    r#"{"idempotency":{"client_order_id":true,"query_by_client_order_id":false,"retry":"unknown"},"#,
    r#""kind":"broker_profile","profile_version":1,"rows":[{"asset_class":"us_equity","cells":["#,
    r#"{"order_type":"limit","protection_forms":[],"quantity_form":"whole","times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"market","protection_forms":[],"quantity_form":"fractional","times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"market","protection_forms":[],"quantity_form":"notional","times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"market","protection_forms":[],"quantity_form":"whole","times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"stop","protection_forms":[],"quantity_form":"whole","times_in_force":["day","gtc"]},"#,
    r#"{"order_type":"stop_limit","protection_forms":["stop_limit"],"quantity_form":"whole","#,
    r#""times_in_force":["day","gtc"]}"#,
    r#"],"session":"regular"}]}"#,
);

/// A seam that never answers, so a profile cannot come from the broker.
struct Silent;

impl Tools for Silent {
    async fn call_tool(
        &self,
        _: CallClass,
        _: &'static str,
        _: &Value,
    ) -> Result<String, McpError> {
        Err(McpError::Network)
    }
}

fn key(intent: &str) -> ClientOrderId {
    ClientOrderId::for_intent(&IntentId(EventId(intent.to_owned()))).unwrap()
}

/// RFC 9562 §5.8 over SHA-256, written out here: the first 16 bytes, version 8, variant `10`.
fn uuid_v8(text: &str) -> String {
    let mut bytes = Sha256::digest(text.as_bytes())[..16].to_vec();
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    let cut = |from: usize, to: usize| &hex[from..to];
    let groups = [cut(0, 8), cut(8, 12), cut(12, 16), cut(16, 20), cut(20, 32)];
    groups.join("-")
}

#[test]
fn robinhoods_profile_is_trading_spec_5_2s_table() {
    let profile = robinhood_profile().unwrap();
    let expected = to_canonical(&parse(ROBINHOOD.as_bytes()).unwrap());
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    assert_eq!(
        text(to_canonical(profile.canonical())),
        text(expected.clone())
    );
    assert_eq!(profile.content_hash(), Digest::of(&expected));
    assert!(
        !profile.idempotency().retry.may_resend_blindly(),
        "DEC-529 item 4: never re-sent"
    );
    let connector = RobinhoodConnector::new(Silent, "5QR00001".to_owned());
    assert_eq!(BrokerConnector::profile(&connector).unwrap(), profile);
}

/// DEC-529 item 7: one `gtc` stop-limit at stop × (1 − offset), never an OCO.
#[test]
fn robinhoods_profile_protects_by_one_gtc_stop_limit() {
    let profile = robinhood_profile().unwrap();
    let price = |text| Price::parse(text).unwrap();
    let prices = ProtectionPrices {
        stop: price("80"),
        take_profit: Some(price("95")),
    };
    let shape = |offset: Option<&str>| {
        let offset = offset.map(|text| Fraction::parse(text).unwrap());
        let (equity, regular) = (AssetClass::UsEquity, MarketSession::Regular);
        let qty = Qty::parse("1").unwrap();
        protective_shape(&profile, equity, regular, qty, prices, offset).unwrap()
    };
    let stop = shape(Some("0.01")).unwrap();
    assert_eq!(
        (stop.form, stop.order_type, stop.tif, stop.oco),
        (
            ProtectionForm::StopLimit,
            OrderType::StopLimit,
            TimeInForce::Gtc,
            None
        )
    );
    assert_eq!(
        (stop.stop_price, stop.limit_price),
        (Some(price("80")), Some(price("79.2")))
    );
    assert_eq!(shape(None), None, "no OCO to fall back to");
}

#[test]
fn the_ref_id_is_the_version_8_uuid_of_the_keys_sha_256() {
    let entry = key("01JENTRY");
    assert_eq!(
        ref_id(&entry),
        Ok("f7c66ef4-4c93-85d1-afb6-a421f9b4b661".to_owned())
    );
    let protection = ClientOrderId::for_protection(&entry, &EventId("01JPROT".to_owned())).unwrap();
    assert_eq!(
        ref_id(&protection),
        Ok("2aa60f50-8acd-8fbf-bc03-ebd7ff23b2a2".to_owned())
    );
    let mut runner = TestRunner::new(Config::with_cases(256));
    let verdict = runner.run(&("[0-9A-Z]{26}", "[0-9A-Z]{26}"), |(one, other)| {
        let (one, other) = (key(&one), key(&other));
        let derived = ref_id(&one).unwrap();
        proptest::prop_assert_eq!(&derived, &uuid_v8(one.as_str()));
        proptest::prop_assert_eq!(
            ref_id(&one).unwrap(),
            derived.clone(),
            "a restart derives it again"
        );
        proptest::prop_assert_eq!(one == other, ref_id(&other).unwrap() == derived);
        Ok(())
    });
    verdict.unwrap();
}

#[test]
fn each_contract_state_reads_as_the_connections_spec_says() {
    let table = "new accepted, queued accepted, confirmed accepted, unconfirmed accepted, \
        partially_filled partially_filled, filled filled, cancelled canceled, rejected rejected, \
        failed rejected, voided canceled";
    let rows: Vec<(&str, &str)> = table
        .split(", ")
        .filter_map(|r| r.split_once(' '))
        .collect();
    assert_eq!(rows.len(), 10, "the contract's ten states");
    for (state, status) in rows {
        assert_eq!(executor_status(state), Ok(status), "{state}");
    }
    for other in [
        "in_doubt", "", "Filled", "canceled", "accepted", "expired", " filled",
    ] {
        assert_eq!(
            executor_status(other),
            Err(RobinhoodError::UnknownState),
            "{other:?}"
        );
    }
}

/// Live, so the mutation gate can judge the crate (#175): each error's text is stable, and the
/// connector hands over the crate's own profile, whatever it is.
#[test]
fn the_errors_read_stably_and_the_connector_hands_over_the_profile() {
    let unimplemented = RobinhoodError::Unimplemented { story: "E7-6" };
    assert_eq!(
        unimplemented.to_string(),
        "E7-6 has not been implemented yet"
    );
    let unknown = RobinhoodError::UnknownState.to_string();
    assert_eq!(unknown, "the order state is none of the contract's ten");
    let connector = RobinhoodConnector::new(Silent, "5QR00001".to_owned());
    assert_eq!(BrokerConnector::profile(&connector), robinhood_profile());
}
