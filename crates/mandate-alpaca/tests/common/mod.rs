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
use mandate_alpaca::error::TransportError;
use mandate_alpaca::http::{HttpRequest, Method, Response, TradingTransport};
use mandate_time::UtcNanos;

pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/alpaca-trading")
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
    let dir = fixtures_dir().join(name);
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
    let mut names: Vec<String> = fs::read_dir(fixtures_dir())
        .unwrap_or_else(|e| panic!("{}: {e}", fixtures_dir().display()))
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

/// A clock that starts at a fixed instant and moves only when paused, so a retry test is
/// deterministic and instant.
#[derive(Clone, Default)]
pub struct FakeClock {
    elapsed: Arc<Mutex<Duration>>,
}

impl FakeClock {
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
