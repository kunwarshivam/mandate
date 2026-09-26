//! Smoke test against the real market-data host with the paper credentials. CI never calls Alpaca
//! (ADR-0001 ES-19): it runs only when `MANDATE_LIVE_ALPACA_DATA=1`.

mod common;

use common::{btc_1hour, day};
use mandate_marketdata::client::{Client, TokioPause};
use mandate_marketdata::http::{AlpacaDataHttp, Credentials};

const OPT_IN: &str = "MANDATE_LIVE_ALPACA_DATA";

#[tokio::test(flavor = "current_thread")]
async fn paper_data_smoke() {
    if std::env::var(OPT_IN).as_deref() != Ok("1") {
        eprintln!("skipped: set {OPT_IN}=1 to call the Alpaca market-data host");
        return;
    }
    let http = AlpacaDataHttp::new(Credentials::from_env().unwrap()).unwrap();
    let client = Client::new(http, TokioPause);
    let records = client
        .fetch_day(&btc_1hour(), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(records.len(), 24);
}
