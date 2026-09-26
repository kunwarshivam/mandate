//! Shared test support: recorded Alpaca responses, a scripted transport, a clock that moves only
//! when paused and records each pause, and scratch directories.

#![allow(
    dead_code,
    reason = "each test binary uses a different subset of the helpers"
)]

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_marketdata::client::{Pause, RateHeaders, Response, Transport, TransportError};
use mandate_marketdata::model::{AssetClass, DatasetId, Feed, Kind, Symbol, Timeframe};
use mandate_time::{Date, UtcNanos};

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
        rate: RateHeaders::default(),
        body: body.to_vec(),
    })
}

/// A success carrying `X-Ratelimit-Limit`, `-Remaining`, and `-Reset` (Unix seconds).
pub fn ok_rated(
    body: &[u8],
    limit: u32,
    remaining: u32,
    reset: i64,
) -> Result<Response, TransportError> {
    Ok(Response {
        status: 200,
        rate: rate(limit, remaining, reset),
        body: body.to_vec(),
    })
}

pub fn rate(limit: u32, remaining: u32, reset: i64) -> RateHeaders {
    RateHeaders {
        limit: Some(limit.to_string()),
        remaining: Some(remaining.to_string()),
        reset: Some(reset.to_string()),
    }
}

/// A corporate-actions page with no action, as Alpaca answers for a symbol without any.
pub const NO_ACTIONS: &[u8] = br#"{"corporate_actions":{},"next_page_token":null}"#;

pub fn status(code: u16) -> Result<Response, TransportError> {
    Ok(Response {
        status: code,
        rate: RateHeaders::default(),
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

/// 2026-09-26T00:00:00Z, a whole minute, where [`RecordingPause`] starts by default.
pub const START_SECS: i64 = 1_790_380_800;

/// A clock that stands still until paused: each pause is recorded and moves it forward by exactly
/// its duration, so no test waits.
#[derive(Clone)]
pub struct RecordingPause {
    start: UtcNanos,
    pauses: Arc<Mutex<Vec<Duration>>>,
}

impl Default for RecordingPause {
    fn default() -> Self {
        Self::starting_at(UtcNanos::from_parts(START_SECS, 0).unwrap())
    }
}

impl RecordingPause {
    pub fn starting_at(start: UtcNanos) -> Self {
        Self {
            start,
            pauses: Arc::default(),
        }
    }

    pub fn pauses(&self) -> Vec<Duration> {
        self.pauses.lock().unwrap().clone()
    }

    /// Time since the start, the sum of every pause.
    pub fn elapsed(&self) -> Duration {
        self.pauses.lock().unwrap().iter().sum()
    }
}

impl Pause for RecordingPause {
    fn now(&self) -> UtcNanos {
        let elapsed = self.elapsed();
        let nanos = u64::from(self.start.nanos()) + u64::from(elapsed.subsec_nanos());
        let secs = self.start.secs() + i64::try_from(elapsed.as_secs()).unwrap();
        UtcNanos::from_parts(
            secs + i64::try_from(nanos / 1_000_000_000).unwrap(),
            u32::try_from(nanos % 1_000_000_000).unwrap(),
        )
        .unwrap()
    }

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

/// A fresh directory under the system temp dir, removed when dropped.
pub struct Scratch(PathBuf);

static SCRATCH: AtomicUsize = AtomicUsize::new(0);

impl Scratch {
    pub fn new(label: &str) -> Self {
        let n = SCRATCH.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "mandate-marketdata-{label}-{}-{n}",
            std::process::id()
        ));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
