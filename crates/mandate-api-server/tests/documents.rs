//! The mandate document a deployed version names (DEC-1020 items 1 and 2): read from the store by
//! its digest, checked, parsed as a mandate, and summarized; anything else is not reported.

use std::collections::BTreeMap;

use mandate_api_server::documents::envelope;
use mandate_canon::{Digest, Value, parse, to_canonical};

const DOCUMENT: &str = include_str!("fixtures/mandate.json");

fn canonical() -> Vec<u8> {
    to_canonical(&parse(DOCUMENT.as_bytes()).expect("JSON"))
}

fn version(bytes: &[u8]) -> String {
    format!("sha256:{}", Digest::of(bytes).to_hex())
}

fn store(bytes: &[u8]) -> BTreeMap<Digest, Vec<u8>> {
    BTreeMap::from([(Digest::of(bytes), bytes.to_vec())])
}

#[test]
fn the_envelope_is_the_documents_own_values() {
    let bytes = canonical();
    let found = envelope(&store(&bytes), &version(&bytes)).expect("found");
    assert_eq!(found.label, "tracer-aapl");
    assert_eq!(found.connection_id, "conn_alpaca_paper_01");
    let expected = parse(
        br#"{"allocation_usd":"1000","max_loss_from_allocation":"0.1","goal":"continuous","goal_end_date":null,"asset_classes":["us_equity"],"max_position_usd":"1000","max_order_usd":"300","max_orders_per_day":50,"max_daily_loss":"0.02","max_drawdown":"0.08"}"#,
    )
    .expect("JSON");
    assert_eq!(found.mandate, expected);
}

#[test]
fn a_goal_end_date_is_served_as_the_document_states_it() {
    let Value::Object(mut document) = parse(DOCUMENT.as_bytes()).expect("JSON") else {
        panic!("an object");
    };
    let goal = parse(br#"{"type":"continuous","end_date":"2026-12-31","on_complete":"release"}"#)
        .expect("JSON");
    document.insert(mandate_canon::Key::new("goal").expect("a key"), goal);
    let bytes = to_canonical(&Value::Object(document));
    let found = envelope(&store(&bytes), &version(&bytes)).expect("found");
    assert_eq!(
        found.mandate.get("goal_end_date"),
        Some(&Value::Str("2026-12-31".into()))
    );
}

#[test]
fn an_absent_corrupt_encrypted_or_unparsable_document_is_not_reported() {
    let bytes = canonical();
    let empty: BTreeMap<Digest, Vec<u8>> = BTreeMap::new();
    assert_eq!(envelope(&empty, &version(&bytes)), None, "absent");
    let tampered = BTreeMap::from([(Digest::of(&bytes), b"{}".to_vec())]);
    assert_eq!(
        envelope(&tampered, &version(&bytes)),
        None,
        "does not hash to its digest"
    );
    let ciphertext = b"\x00\x01opaque ciphertext".to_vec();
    assert_eq!(
        envelope(&store(&ciphertext), &version(&ciphertext)),
        None,
        "not a mandate (encrypted)"
    );
    let not_mandate = to_canonical(&parse(br#"{"name":"x"}"#).expect("JSON"));
    assert_eq!(envelope(&store(&not_mandate), &version(&not_mandate)), None);
    assert_eq!(
        envelope(&store(&bytes), "not-a-ref"),
        None,
        "a malformed version"
    );
}
