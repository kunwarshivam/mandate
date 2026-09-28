//! Shared test support: the recorded scenarios, a scripted transport that asserts what was sent,
//! and a clock that moves only when paused.
//!
//! No test touches a network (ADR-0001 ES-19). A scenario is a directory under
//! `tests/fixtures/alpaca-trading/<name>/` holding `requests.txt` (one
//! `<METHOD> <path and query>[ <canonical body>]` per request, in order), `statuses.txt` (each
//! response's HTTP status, in the same order), and `response-<n>.json` (each body, byte for
//! byte).

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_alpaca::client::Pause;
use mandate_alpaca::data::{DataTransport, QuoteRequest};
use mandate_alpaca::error::TransportError;
use mandate_alpaca::http::{HttpRequest, Method, Response, TradingTransport};
use mandate_time::UtcNanos;

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alpaca-trading")
}

/// The latest-quote scenarios of E7-8, recorded against the data host's shape.
pub fn data_fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alpaca-data")
}

/// One recorded exchange: exactly what was sent and exactly what came back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Exchange {
    pub method: Method,
    pub path_and_query: String,
    pub body: Option<String>,
    pub status: u16,
    pub response: Vec<u8>,
}

/// A recorded scenario, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scenario {
    pub name: String,
    pub exchanges: Vec<Exchange>,
}

fn method_of(word: &str) -> Method {
    match word {
        "GET" => Method::Get,
        "POST" => Method::Post,
        "DELETE" => Method::Delete,
        other => panic!("{other} is not a method this crate uses"),
    }
}

/// Loads one scenario. A malformed fixture is a panic here rather than a silent skip: the
/// fixtures are the tests PR's contract.
pub fn scenario(name: &str) -> Scenario {
    scenario_in(&fixtures_dir(), name)
}

/// Loads one latest-quote scenario.
pub fn data_scenario(name: &str) -> Scenario {
    scenario_in(&data_fixtures_dir(), name)
}

fn scenario_in(root: &Path, name: &str) -> Scenario {
    let dir = root.join(name);
    let requests = fs::read_to_string(dir.join("requests.txt"))
        .unwrap_or_else(|e| panic!("{name}/requests.txt: {e}"));
    let statuses = fs::read_to_string(dir.join("statuses.txt"))
        .unwrap_or_else(|e| panic!("{name}/statuses.txt: {e}"));
    let lines: Vec<&str> = requests.lines().collect();
    let codes: Vec<&str> = statuses.lines().collect();
    assert_eq!(
        lines.len(),
        codes.len(),
        "{name}: {} requests and {} statuses",
        lines.len(),
        codes.len()
    );
    let exchanges = lines
        .iter()
        .zip(codes.iter())
        .enumerate()
        .map(|(index, (line, code))| {
            let mut parts = line.splitn(3, ' ');
            let method = method_of(parts.next().unwrap_or_else(|| panic!("{name}: empty line")));
            let path_and_query = parts
                .next()
                .unwrap_or_else(|| panic!("{name}: no path on line {index}"))
                .to_owned();
            let body = parts.next().map(str::to_owned);
            let n = index.saturating_add(1);
            let response = fs::read(dir.join(format!("response-{n}.json")))
                .unwrap_or_else(|e| panic!("{name}/response-{n}.json: {e}"));
            Exchange {
                method,
                path_and_query,
                body,
                status: code
                    .parse()
                    .unwrap_or_else(|e| panic!("{name}: status `{code}`: {e}")),
                response,
            }
        })
        .collect();
    Scenario {
        name: name.to_owned(),
        exchanges,
    }
}

/// Every scenario directory, sorted, so a scan cannot miss one that was added later.
pub fn every_scenario() -> Vec<String> {
    scenarios_in(&fixtures_dir())
}

/// Every latest-quote scenario directory, sorted.
pub fn every_data_scenario() -> Vec<String> {
    scenarios_in(&data_fixtures_dir())
}

fn scenarios_in(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(root)
        .unwrap_or_else(|e| panic!("{}: {e}", root.display()))
        .filter_map(|entry| {
            let path = entry.ok()?.path();
            path.is_dir()
                .then(|| path.file_name()?.to_str().map(str::to_owned))
                .flatten()
        })
        .collect();
    names.sort();
    names
}

/// Answers each request with the next scripted reply and records what was asked, so a test can
/// assert on the **method and the body** as well as the path.
#[derive(Clone, Default)]
pub struct FakeTransport {
    script: Arc<Mutex<Script>>,
}

#[derive(Default)]
struct Script {
    replies: VecDeque<Result<Response, TransportError>>,
    sent: Vec<HttpRequest>,
}

impl FakeTransport {
    pub fn serving(replies: impl IntoIterator<Item = Result<Response, TransportError>>) -> Self {
        let fake = Self::default();
        fake.push(replies);
        fake
    }

    /// A transport that replays one recorded scenario, in order.
    pub fn replaying(scenario: &Scenario) -> Self {
        Self::serving(scenario.exchanges.iter().map(|exchange| {
            Ok(Response {
                status: exchange.status,
                body: exchange.response.clone(),
            })
        }))
    }

    pub fn push(&self, replies: impl IntoIterator<Item = Result<Response, TransportError>>) {
        if let Ok(mut script) = self.script.lock() {
            script.replies.extend(replies);
        }
    }

    /// Everything the client sent, in order.
    pub fn sent(&self) -> Vec<HttpRequest> {
        self.script
            .lock()
            .map(|script| script.sent.clone())
            .unwrap_or_default()
    }
}

impl TradingTransport for FakeTransport {
    async fn send(&self, request: &HttpRequest) -> Result<Response, TransportError> {
        let mut script = match self.script.lock() {
            Ok(script) => script,
            Err(poisoned) => poisoned.into_inner(),
        };
        script.sent.push(request.clone());
        script
            .replies
            .pop_front()
            .unwrap_or(Err(TransportError::Timeout))
    }
}

/// Answers each latest-quote read with the next scripted reply and records what was asked, the
/// data host's counterpart of [`FakeTransport`].
#[derive(Clone, Default)]
pub struct FakeDataTransport {
    script: Arc<Mutex<DataScript>>,
}

#[derive(Default)]
struct DataScript {
    replies: VecDeque<Result<Response, TransportError>>,
    sent: Vec<QuoteRequest>,
}

impl FakeDataTransport {
    pub fn serving(replies: impl IntoIterator<Item = Result<Response, TransportError>>) -> Self {
        let fake = Self::default();
        if let Ok(mut script) = fake.script.lock() {
            script.replies.extend(replies);
        }
        fake
    }

    /// A transport that replays one latest-quote scenario, in order.
    pub fn replaying(scenario: &Scenario) -> Self {
        Self::serving(scenario.exchanges.iter().map(|exchange| {
            Ok(Response {
                status: exchange.status,
                body: exchange.response.clone(),
            })
        }))
    }

    /// Everything the client sent, in order.
    pub fn sent(&self) -> Vec<QuoteRequest> {
        self.script
            .lock()
            .map(|script| script.sent.clone())
            .unwrap_or_default()
    }
}

impl DataTransport for FakeDataTransport {
    async fn send(&self, request: &QuoteRequest) -> Result<Response, TransportError> {
        let mut script = match self.script.lock() {
            Ok(script) => script,
            Err(poisoned) => poisoned.into_inner(),
        };
        script.sent.push(request.clone());
        script
            .replies
            .pop_front()
            .unwrap_or(Err(TransportError::Timeout))
    }
}

/// A clock that starts at a fixed instant and moves only when paused, so a retry test is
/// deterministic and instant.
#[derive(Clone, Default)]
pub struct FakeClock {
    elapsed: Arc<Mutex<Duration>>,
}

impl FakeClock {
    /// A clock reading `at`, so a read of a dated answer can be judged against a known now.
    pub fn at(at: UtcNanos) -> Self {
        let since_epoch = Duration::new(u64::try_from(at.secs()).unwrap_or(0), at.nanos());
        Self {
            elapsed: Arc::new(Mutex::new(since_epoch)),
        }
    }

    pub fn elapsed(&self) -> Duration {
        self.elapsed.lock().map(|at| *at).unwrap_or_default()
    }
}

impl Pause for FakeClock {
    fn now(&self) -> UtcNanos {
        let secs = i64::try_from(self.elapsed().as_secs()).unwrap_or(i64::MAX);
        UtcNanos::from_parts(secs, self.elapsed().subsec_nanos()).unwrap_or(UtcNanos::EPOCH)
    }

    async fn pause(&self, duration: Duration) {
        if let Ok(mut at) = self.elapsed.lock() {
            *at = at.saturating_add(duration);
        }
    }
}

/// The answer of a client call, with the stubs' own refusal turned into the failure it is.
///
/// A pending test must fail on the crate's `Unimplemented` and on nothing else, so a case that
/// expects an error first passes the result through here: an `Unimplemented` panics with the
/// crate's own message rather than satisfying an `expect_err`.
pub fn answered<T: core::fmt::Debug>(
    result: Result<T, mandate_alpaca::ClientError>,
) -> Result<T, mandate_alpaca::ClientError> {
    if let Err(error @ mandate_alpaca::ClientError::Unimplemented { .. }) = &result {
        panic!("{error}");
    }
    result
}

/// A recorded response body, by scenario and zero-based exchange.
pub fn body(name: &str, index: usize) -> Vec<u8> {
    scenario(name)
        .exchanges
        .get(index)
        .map(|exchange| exchange.response.clone())
        .unwrap_or_else(|| panic!("{name} has no exchange {index}"))
}

/// One scripted reply with a recorded body.
pub fn reply(status: u16, name: &str, index: usize) -> Result<Response, TransportError> {
    Ok(Response {
        status,
        body: body(name, index),
    })
}

/// One scripted reply with an inline body.
pub fn inline(status: u16, text: &str) -> Result<Response, TransportError> {
    Ok(Response {
        status,
        body: text.as_bytes().to_vec(),
    })
}

/// Rewrites every decimal string in a JSON value into canonical text (no trailing fractional
/// zeros), so a recorded `"1.00"` and a `mandate-num` `"1"` compare equal while every other byte
/// of the body still has to match.
pub fn canonical_decimals(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::String(text) => {
            let decimal = !text.is_empty()
                && text.bytes().filter(|b| *b == b'.').count() <= 1
                && text
                    .strip_prefix('-')
                    .unwrap_or(text)
                    .bytes()
                    .all(|b| b.is_ascii_digit() || b == b'.');
            if decimal && text.contains('.') {
                let trimmed = text.trim_end_matches('0').trim_end_matches('.');
                serde_json::Value::String(trimmed.to_owned())
            } else {
                value.clone()
            }
        }
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.iter()
                .map(|(key, inner)| (key.clone(), canonical_decimals(inner)))
                .collect(),
        ),
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.iter().map(canonical_decimals).collect())
        }
        other => other.clone(),
    }
}

/// A key-ID-shaped sentinel the redaction tests check never reaches output. It is not a
/// credential: it is allowlisted by id in `.gitleaks.toml` for this file alone.
pub const SENTINEL_KEY_ID: &str = "PKSENTINELTRADING000";
/// A secret-shaped sentinel, the same way.
pub const SENTINEL_SECRET: &str = "sentinelTradingSecretValue0123456789abcdefgh";

/// Whether a word has the shape of an Alpaca key ID, which is what the fixture scan looks for.
pub fn looks_like_key_id(word: &str) -> bool {
    word.len() == 20
        && (word.starts_with("PK") || word.starts_with("AK"))
        && word
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
}
