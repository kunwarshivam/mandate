//! Smoke test against the real market-data host with the paper credentials. CI never calls Alpaca
//! (ADR-0001 ES-19): the test is built only with the `live-alpaca` feature, so a run without it
//! lists no live test rather than a skipped one that passes, and with it the credentials are
//! required: `cargo nextest run -p mandate-marketdata --features live-alpaca --test live`.

mod common;

use common::{btc_1hour, day};
use mandate_marketdata::client::{Client, TokioPause};
use mandate_marketdata::http::{AlpacaDataHttp, Credentials, KEY_ID_VAR, SECRET_VAR};

#[tokio::test(flavor = "current_thread")]
async fn paper_data_smoke() {
    let credentials = Credentials::from_env().unwrap_or_else(|e| {
        panic!("the live test needs {KEY_ID_VAR} and {SECRET_VAR} in the environment: {e}")
    });
    let http = AlpacaDataHttp::new(credentials).unwrap();
    let client = Client::new(http, TokioPause);
    let records = client
        .fetch_day(&btc_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(records.len(), 24);
}
