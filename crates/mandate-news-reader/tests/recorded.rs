#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The recorded news transport and the fixture key (E2-10c, DEC-2085): what the cell's news reader
//! runs on. The recording answers each request with the recorded items about the tickers it asks
//! for, as one page, whatever its start, and reaches no host; the fixture key is a paper key the
//! reader holds as it would the vault's. Over the real in-memory journal, every recorded item about
//! a polled ticker is journaled once, however many rounds ask again, and no other.

mod common;

use std::collections::BTreeSet;

use mandate_canon::{Value, parse};
use mandate_news_reader::NewsReader;
use mandate_news_reader::recorded::{RecordedNews, recorded_key};
use mandate_research::SourceId;
use mandate_sources::SourceError;
use mandate_sources::allowlist::Allowlist;
use mandate_sources::fetch::{Fetcher, Transport};
use mandate_sources::symbols::Symbol;
use secrecy::ExposeSecret;

use common::*;

/// The cell's recording, as its image carries it.
const CELL_RECORDING: &[u8] = include_bytes!("../../../deploy/cell/news-reader/recorded-news.json");

/// The ids of a page's items, in order.
fn ids_of(body: &[u8]) -> Vec<u64> {
    let page = parse(body).unwrap();
    let Some(Value::Array(items)) = page.get("news").cloned() else {
        panic!("a page holds its items under news")
    };
    assert_eq!(page.get("next_page_token"), Some(&Value::Null), "one page");
    items
        .iter()
        .map(|item| match item.get("id") {
            Some(Value::Int(id)) => id.get(),
            other => panic!("an item id: {other:?}"),
        })
        .collect()
}

/// One request for `tickers` through the allowlisted fetcher, as the reader asks.
fn asked_for(recording: &RecordedNews, tickers: &[&str]) -> Result<Vec<u8>, SourceError> {
    let mut fetcher = Fetcher::new(Allowlist::parse(ALLOWLIST).unwrap(), recording.clone());
    let source = SourceId::new("alpaca.news").unwrap();
    let symbols: Vec<Symbol> = tickers.iter().map(|t| Symbol::parse(t).unwrap()).collect();
    block_on(fetcher.alpaca_news(
        &source,
        &recorded_key().unwrap(),
        &symbols,
        t0(),
        None,
        t0(),
    ))
}

#[test]
fn a_request_is_answered_with_the_recorded_items_about_its_tickers_as_one_page() {
    let recording = RecordedNews::parse(CELL_RECORDING).unwrap();
    assert_eq!(
        ids_of(&asked_for(&recording, &["SPY"]).unwrap()),
        [9001, 9002]
    );
    assert_eq!(
        ids_of(&asked_for(&recording, &["QQQ"]).unwrap()),
        [9002, 9003]
    );
    assert_eq!(
        ids_of(&asked_for(&recording, &["QQQ", "SPY"]).unwrap()),
        [9001, 9002, 9003]
    );
    assert_eq!(
        ids_of(&asked_for(&recording, &["AAPL"]).unwrap()),
        Vec::<u64>::new()
    );
}

#[test]
fn the_recording_answers_only_a_request_that_carries_the_key() {
    let recording = RecordedNews::parse(CELL_RECORDING).unwrap();
    let url = Allowlist::parse(ALLOWLIST)
        .unwrap()
        .permit(
            &SourceId::new("alpaca.news").unwrap(),
            "https://data.alpaca.markets/v1beta1/news?symbols=SPY&start=2026-10-24T15:00:00Z&limit=50&sort=desc",
        )
        .unwrap();
    assert_eq!(
        block_on(recording.get(&url, None)).err(),
        Some(SourceError::Status),
        "no key, no answer, as the vendor refuses it"
    );
    assert!(block_on(recording.get(&url, Some(&recorded_key().unwrap()))).is_ok());
}

#[test]
fn a_recording_that_is_not_a_page_of_items_with_tickers_is_refused() {
    for bad in [
        &b"not json"[..],
        br#"{"items": []}"#,
        br#"{"news": {}}"#,
        br#"{"news": [{"id": 1}]}"#,
        br#"{"news": [{"id": 1, "symbols": "SPY"}]}"#,
        br#"{"news": [{"id": 1, "symbols": [7]}]}"#,
    ] {
        assert_eq!(
            RecordedNews::parse(bad).err(),
            Some("recorded_news_malformed"),
            "{}",
            String::from_utf8_lossy(bad)
        );
    }
    assert!(RecordedNews::parse(br#"{"news": []}"#).is_ok());
}

#[test]
fn the_fixture_key_is_a_paper_key() {
    let key = recorded_key().unwrap();
    assert!(key.key_id().expose_secret().starts_with("PK"));
}

/// Over the real in-memory journal, the reader following an agent with a thesis on SPY stores
/// each recorded SPY item once, however many rounds the recording answers again, and never the
/// item about QQQ alone, which it does not poll.
#[test]
fn every_recorded_item_about_a_polled_ticker_is_journaled_once_and_no_other() {
    let journal = Journal::default();
    let shelf = Shelf::default();
    spy(&journal, &shelf);
    let recording = RecordedNews::parse(CELL_RECORDING).unwrap();
    let mut reader = NewsReader::start(
        journal.clone(),
        shelf.clone(),
        recording,
        &config(),
        recorded_key(),
        t0(),
    );
    assert_eq!(reader.not_ready(), None);
    for second in 0..600 {
        block_on(reader.step(plus(t0(), second)));
    }
    let stored: Vec<String> = stored(&journal)
        .iter()
        .map(|row| match member(row, "item_id") {
            Value::Str(id) => id,
            other => panic!("an item id: {other:?}"),
        })
        .collect();
    let vendor_ids: BTreeSet<&str> = stored
        .iter()
        .map(|id| id.split(':').next().unwrap())
        .collect();
    assert_eq!(vendor_ids, BTreeSet::from(["9001", "9002"]));
    assert_eq!(stored.len(), 2, "each once: {stored:?}");
}
