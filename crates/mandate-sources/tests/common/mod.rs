#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: each test binary uses a subset, and a broken fixture should stop the test"
)]
//! Shared fixtures: an allowlist with both sources confirmed, the tagging tables, a recording
//! transport, and a poll-once executor for the fetchers' futures.

use std::cell::RefCell;
use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

use mandate_domain::AssetId;
use mandate_research::SourceId;
use mandate_sources::SourceError;
use mandate_sources::allowlist::{Allowlist, PermittedUrl};
use mandate_sources::fetch::{NewsAuth, RawResponse, Transport};
use mandate_sources::ingest::IngestContext;
use mandate_sources::record::Capture;
use mandate_sources::symbols::{Cik, CikTable, Symbol, SymbolTable, Validity};
use mandate_time::UtcNanos;

pub const AAPL: &str = "11111111-1111-4111-8111-111111111111";
pub const MSFT: &str = "22222222-2222-4222-8222-222222222222";
pub const TSLA: &str = "33333333-3333-4333-8333-333333333333";
pub const XYZ_OLD: &str = "44444444-4444-4444-8444-444444444444";
pub const XYZ_NEW: &str = "55555555-5555-4555-8555-555555555555";
pub const GME: &str = "66666666-6666-4666-8666-666666666666";
pub const NVDA: &str = "77777777-7777-4777-8777-777777777777";

/// Both sources in effect, as a test allowlist; the checked-in file keeps Alpaca news unconfirmed.
pub const TEST_ALLOWLIST: &str = r#"
version = 7

[[source]]
id = "sec.edgar"
kind = "filings"
licensed = false
counsel_confirmed = true
min_interval_ms = 125
max_response_bytes = 8388608
endpoints = ["https://data.sec.gov/submissions/", "https://www.sec.gov/Archives/edgar/data/"]

[[source]]
id = "alpaca.news"
kind = "news"
licensed = true
counsel_confirmed = true
min_interval_ms = 300
max_response_bytes = 1048576
endpoints = ["https://data.alpaca.markets/v1beta1/news"]

[[source]]
id = "wire.unconfirmed"
kind = "news"
licensed = true
counsel_confirmed = false
min_interval_ms = 300
max_response_bytes = 1048576
endpoints = ["https://news.example.com/v1/"]
"#;

pub fn allowlist() -> Allowlist {
    Allowlist::parse(TEST_ALLOWLIST).unwrap()
}

pub fn source(id: &str) -> SourceId {
    SourceId::new(id).unwrap()
}

pub fn news_source() -> SourceId {
    source("alpaca.news")
}

pub fn edgar_source() -> SourceId {
    source("sec.edgar")
}

pub fn asset(id: &str) -> AssetId {
    AssetId::parse(id).unwrap()
}

pub fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

pub fn symbol(text: &str) -> Symbol {
    Symbol::parse(text).unwrap()
}

pub fn forever(from: &str) -> Validity {
    Validity {
        from: at(from),
        until: None,
    }
}

/// `XYZ` names one company until 2026-06-01 and another after, so a reused ticker is tested.
pub fn symbols() -> SymbolTable {
    SymbolTable::new(
        3,
        vec![
            (symbol("AAPL"), asset(AAPL), forever("2000-01-01T00:00:00Z")),
            (symbol("MSFT"), asset(MSFT), forever("2000-01-01T00:00:00Z")),
            (symbol("TSLA"), asset(TSLA), forever("2000-01-01T00:00:00Z")),
            (symbol("GME"), asset(GME), forever("2000-01-01T00:00:00Z")),
            (symbol("NVDA"), asset(NVDA), forever("2000-01-01T00:00:00Z")),
            (
                symbol("XYZ"),
                asset(XYZ_OLD),
                Validity {
                    from: at("2000-01-01T00:00:00Z"),
                    until: Some(at("2026-06-01T00:00:00Z")),
                },
            ),
            (symbol("XYZ"), asset(XYZ_NEW), forever("2026-06-01T00:00:00Z")),
        ],
    )
    .unwrap()
}

pub fn ciks() -> CikTable {
    CikTable::new(
        2,
        vec![(
            Cik::parse("320193").unwrap(),
            symbol("AAPL"),
            forever("2000-01-01T00:00:00Z"),
        )],
    )
    .unwrap()
}

pub struct Tables {
    pub allowlist: Allowlist,
    pub symbols: SymbolTable,
    pub ciks: CikTable,
}

impl Tables {
    pub fn new() -> Self {
        Self {
            allowlist: allowlist(),
            symbols: symbols(),
            ciks: ciks(),
        }
    }

    pub fn ctx<'a>(
        &'a self,
        source: &'a SourceId,
        received_at: UtcNanos,
        capture: Capture,
    ) -> IngestContext<'a> {
        IngestContext {
            allowlist: &self.allowlist,
            source,
            received_at,
            capture,
            symbols: &self.symbols,
            ciks: &self.ciks,
        }
    }
}

pub fn fixture(path: &str) -> Vec<u8> {
    let full = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(path);
    std::fs::read(&full).unwrap_or_else(|e| panic!("{}: {e}", full.display()))
}

/// Drives a future that never waits, as every fetch over [`Recorded`] is.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(output) => output,
        Poll::Pending => panic!("a recorded fetch never waits"),
    }
}

/// A transport that answers from a queue of recorded responses and records every URL it was
/// handed, with whether credentials came along.
#[derive(Default)]
pub struct Recorded {
    pub answers: RefCell<Vec<Result<RawResponse, SourceError>>>,
    pub sent: RefCell<Vec<(String, bool)>>,
}

impl Recorded {
    pub fn answering(answers: Vec<Result<RawResponse, SourceError>>) -> Self {
        Self {
            answers: RefCell::new(answers.into_iter().rev().collect()),
            sent: RefCell::new(Vec::new()),
        }
    }

    pub fn ok(body: &[u8]) -> Result<RawResponse, SourceError> {
        Ok(RawResponse {
            status: 200,
            body: body.to_vec(),
        })
    }
}

impl Transport for Recorded {
    async fn get(
        &self,
        url: &PermittedUrl,
        auth: Option<&NewsAuth>,
    ) -> Result<RawResponse, SourceError> {
        self.sent
            .borrow_mut()
            .push((url.as_str().to_owned(), auth.is_some()));
        self.answers
            .borrow_mut()
            .pop()
            .unwrap_or(Err(SourceError::Transport))
    }
}

pub fn auth() -> NewsAuth {
    NewsAuth::new(
        secrecy::SecretString::from("test-key-id".to_owned()),
        secrecy::SecretString::from("test-secret".to_owned()),
    )
}
