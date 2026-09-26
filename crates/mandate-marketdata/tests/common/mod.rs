//! Shared test support: recorded Alpaca responses, a scripted transport, and a pause that only
//! records.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the helpers"
)]

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_marketdata::client::{Pause, Response, Transport, TransportError};
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

pub fn ok(body: &[u8]) -> Result<Response, TransportError> {
    Ok(Response {
        status: 200,
        body: body.to_vec(),
    })
}

pub fn status(code: u16) -> Result<Response, TransportError> {
    Ok(Response {
        status: code,
        body: br#"{"message":"scripted"}"#.to_vec(),
    })
}

#[derive(Default)]
struct Script {
    replies: VecDeque<Result<Response, TransportError>>,
    requested: Vec<String>,
}

/// Answers each request with the next scripted reply and records what was asked.
#[derive(Clone, Default)]
pub struct FakeTransport {
    script: Arc<Mutex<Script>>,
}

impl FakeTransport {
    pub fn serving(replies: impl IntoIterator<Item = Result<Response, TransportError>>) -> Self {
        let fake = Self::default();
        fake.push(replies);
        fake
    }

    pub fn push(&self, replies: impl IntoIterator<Item = Result<Response, TransportError>>) {
        self.script.lock().unwrap().replies.extend(replies);
    }

    pub fn requested(&self) -> Vec<String> {
        self.script.lock().unwrap().requested.clone()
    }

    pub fn unused(&self) -> usize {
        self.script.lock().unwrap().replies.len()
    }
}

impl Transport for FakeTransport {
    async fn get(&self, path_and_query: &str) -> Result<Response, TransportError> {
        let mut script = self.script.lock().unwrap();
        script.requested.push(path_and_query.to_owned());
        script
            .replies
            .pop_front()
            .unwrap_or_else(|| panic!("unscripted request {path_and_query}"))
    }
}

/// Records each pause instead of waiting.
#[derive(Clone, Default)]
pub struct RecordingPause {
    pauses: Arc<Mutex<Vec<Duration>>>,
}

impl RecordingPause {
    pub fn pauses(&self) -> Vec<Duration> {
        self.pauses.lock().unwrap().clone()
    }
}

impl Pause for RecordingPause {
    async fn pause(&self, duration: Duration) {
        self.pauses.lock().unwrap().push(duration);
    }
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
