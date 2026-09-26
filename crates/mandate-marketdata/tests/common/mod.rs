//! Shared test support: recorded Alpaca responses and the datasets they belong to.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the helpers"
)]

use std::fs;
use std::path::{Path, PathBuf};

use mandate_marketdata::model::{AssetClass, DatasetId, Feed, Kind, Symbol, Timeframe};
use mandate_time::Date;

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alpaca")
}

/// A recorded scenario: the exact request of each page, in order, and its body.
pub struct Scenario {
    pub requests: Vec<String>,
    pub bodies: Vec<Vec<u8>>,
}

pub fn scenario(name: &str) -> Scenario {
    let dir = fixtures_dir().join(name);
    let requests: Vec<String> = fs::read_to_string(dir.join("requests.txt"))
        .unwrap_or_else(|e| panic!("{name}/requests.txt: {e}"))
        .lines()
        .map(str::to_owned)
        .collect();
    let bodies = (1..=requests.len())
        .map(|n| fs::read(dir.join(format!("page-{n}.json"))).unwrap())
        .collect();
    Scenario { requests, bodies }
}

pub fn day(s: &str) -> Date {
    Date::parse(s).unwrap()
}

pub fn dataset(asset_class: AssetClass, feed: Feed, kind: Kind, symbol: &str) -> DatasetId {
    DatasetId::new(asset_class, feed, kind, Symbol::parse(symbol).unwrap()).unwrap()
}

pub fn spy_sip_1hour() -> DatasetId {
    dataset(
        AssetClass::UsEquity,
        Feed::Sip,
        Kind::Bars("1Hour".parse::<Timeframe>().unwrap()),
        "SPY",
    )
}

pub fn shy_iex_trades() -> DatasetId {
    dataset(AssetClass::UsEquity, Feed::Iex, Kind::Trades, "SHY")
}

pub fn btc_1hour() -> DatasetId {
    dataset(
        AssetClass::Crypto,
        Feed::CryptoUs,
        Kind::Bars("1Hour".parse::<Timeframe>().unwrap()),
        "BTC/USD",
    )
}

pub fn btc_trades() -> DatasetId {
    dataset(AssetClass::Crypto, Feed::CryptoUs, Kind::Trades, "BTC/USD")
}
