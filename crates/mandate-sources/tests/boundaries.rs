#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! Each boundary and accessor pinned from both sides, so a comparison moved by one or an accessor
//! that answers a default fails a test (the mutation gate, ADR-0001 ES-11).

mod common;

use std::collections::BTreeSet;

use mandate_sources::SourceError;
use mandate_sources::allowlist::Endpoint;
use mandate_sources::fetch::Fetcher;
use mandate_sources::ingest::{MAX_TAGS, alpaca_news_page, edgar_submissions};
use mandate_sources::reader::{FILINGS_WINDOW_S, NEWS_WINDOW_S};
use mandate_sources::record::Capture;
use mandate_sources::store::{ItemStore, StoreQuery};
use mandate_sources::symbols::{SymbolTable, Validity};
use mandate_sources::text::UntrustedText;
use mandate_time::UtcNanos;
use secrecy::ExposeSecret;

use common::*;

#[test]
fn every_error_has_its_own_stable_code() {
    let all = [
        (SourceError::AllowlistMalformed, "allowlist_malformed"),
        (SourceError::DuplicateSource, "duplicate_source"),
        (SourceError::BadEndpoint, "bad_endpoint"),
        (SourceError::SourceNotAllowlisted, "source_not_allowlisted"),
        (
            SourceError::EndpointNotAllowlisted,
            "endpoint_not_allowlisted",
        ),
        (
            SourceError::AllowlistVersionMismatch,
            "allowlist_version_mismatch",
        ),
        (SourceError::WrongKind, "wrong_kind"),
        (SourceError::ResponseTooLarge, "response_too_large"),
        (SourceError::Malformed, "malformed"),
        (SourceError::BadField, "bad_field"),
        (SourceError::TooManyTags, "too_many_tags"),
        (SourceError::FutureDated, "future_dated"),
        (SourceError::RevisedInPlace, "revised_in_place"),
        (SourceError::TooSoon, "too_soon"),
        (SourceError::Status, "status"),
        (SourceError::Transport, "transport"),
        (SourceError::Time, "time"),
    ];
    for (error, code) in all {
        assert_eq!(error.code(), code);
    }
}

#[test]
fn the_entry_accessors_answer_the_file() {
    let list = allowlist();
    let edgar = list.entries().find(|e| e.id() == &edgar_source()).unwrap();
    assert!(edgar.counsel_confirmed());
    let hosts: Vec<&str> = edgar.endpoints().iter().map(Endpoint::host).collect();
    assert_eq!(hosts, ["data.sec.gov", "www.sec.gov"]);
    let unconfirmed = list
        .entries()
        .find(|e| e.id() == &source("wire.unconfirmed"))
        .unwrap();
    assert!(!unconfirmed.counsel_confirmed());
    assert_eq!(symbols().version(), 3);
}

#[test]
fn a_prefix_without_a_trailing_slash_ends_on_a_segment() {
    let list = allowlist();
    let news = news_source();
    for ok in [
        "https://data.alpaca.markets/v1beta1/news",
        "https://data.alpaca.markets/v1beta1/news/latest",
        "https://data.alpaca.markets/v1beta1/news?symbols=AAPL",
    ] {
        assert!(list.permit(&news, ok).is_ok(), "{ok}");
    }
    for bad in [
        "https://data.alpaca.markets/v1beta1/newsX",
        "https://data.alpaca.markets/v1beta1/news-admin",
    ] {
        assert_eq!(
            list.permit(&news, bad),
            Err(SourceError::EndpointNotAllowlisted),
            "{bad}"
        );
    }
}

#[test]
fn the_news_credential_reaches_its_holder_unchanged() {
    let auth = auth();
    assert_eq!(auth.key_id().expose_secret(), "test-key-id");
    assert_eq!(auth.secret().expose_secret(), "test-secret");
}

#[test]
fn fifty_tickers_go_in_one_request() {
    let mut f = Fetcher::new(allowlist(), Recorded::answering(vec![Recorded::ok(b"{}")]));
    let fifty: Vec<_> = (0..50).map(|i| symbol(&format!("A{i}"))).collect();
    block_on(f.alpaca_news(
        &news_source(),
        &auth(),
        &fifty,
        at("2026-10-16T00:00:00Z"),
        None,
        at("2026-10-16T12:00:00Z"),
    ))
    .unwrap();
    assert_eq!(f.transport().sent.borrow().len(), 1);
}

#[test]
fn a_body_of_exactly_the_cap_is_read() {
    let tables = Tables::new();
    let news = news_source();
    let at_cap = vec![b' '; 1_048_576];
    assert_eq!(
        alpaca_news_page(
            &tables.ctx(&news, at("2026-10-16T12:00:00Z"), Capture::Live),
            &at_cap
        ),
        Err(SourceError::Malformed)
    );
}

fn one_item(updated: &str, symbols: &[String]) -> String {
    serde_json::json!({"news": [{
        "id": 1, "headline": "h", "created_at": "2026-10-15T00:00:00Z", "updated_at": updated,
        "symbols": symbols
    }], "next_page_token": null})
    .to_string()
}

#[test]
fn the_future_skew_carries_across_a_second() {
    let tables = Tables::new();
    let news = news_source();
    let ctx = tables.ctx(&news, at("2026-10-15T11:59:59.5Z"), Capture::Live);
    let edge = alpaca_news_page(&ctx, one_item("2026-10-15T12:00:00.5Z", &[]).as_bytes()).unwrap();
    assert!(edge.refused.is_empty());
    let past = alpaca_news_page(
        &ctx,
        one_item("2026-10-15T12:00:00.500000001Z", &[]).as_bytes(),
    )
    .unwrap();
    assert_eq!(past.refused, vec![(0, SourceError::FutureDated)]);
}

#[test]
fn thirty_two_tickers_are_kept_and_thirty_three_are_not() {
    let tables = Tables::new();
    let news = news_source();
    let ctx = tables.ctx(&news, at("2026-10-16T12:00:00Z"), Capture::Live);
    let mut tickers: Vec<String> = (0..MAX_TAGS - 1).map(|i| format!("Z{i}")).collect();
    tickers.push("AAPL".to_owned());
    let kept =
        alpaca_news_page(&ctx, one_item("2026-10-15T00:00:00Z", &tickers).as_bytes()).unwrap();
    assert_eq!(kept.drafts[0].tags(), &BTreeSet::from([asset(AAPL)]));
    tickers.push("MSFT".to_owned());
    let refused =
        alpaca_news_page(&ctx, one_item("2026-10-15T00:00:00Z", &tickers).as_bytes()).unwrap();
    assert_eq!(refused.refused, vec![(0, SourceError::TooManyTags)]);
}

#[test]
fn an_accession_number_has_its_exact_shape() {
    let tables = Tables::new();
    let edgar = edgar_source();
    let ctx = tables.ctx(&edgar, at("2026-10-16T12:00:00Z"), Capture::Live);
    let accessions = [
        "0000320193-26-000090",
        "000032019326000090xx",
        "0000320193-2-6000090",
        "000032019326000090",
        "0000320193-26-0000900",
        "000032019a-26-000090",
    ];
    let body = serde_json::json!({"cik": "320193", "filings": {"recent": {
        "accessionNumber": accessions,
        "acceptanceDateTime": vec!["2026-10-01T20:05:00.000Z"; 6],
        "form": vec!["8-K"; 6],
        "primaryDocument": vec!["a.htm"; 6],
    }}})
    .to_string();
    let index = edgar_submissions(&ctx, body.as_bytes()).unwrap();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(
        index.refused,
        (1..6)
            .map(|i| (i, SourceError::BadField))
            .collect::<Vec<_>>()
    );
}

#[test]
fn the_read_windows_are_the_harness_s() {
    assert_eq!(NEWS_WINDOW_S, 172_800);
    assert_eq!(FILINGS_WINDOW_S, 604_800);
}

#[test]
fn adjacent_validities_do_not_overlap_and_touching_ones_do() {
    let v = |from: &str, until: Option<&str>| Validity {
        from: at(from),
        until: until.map(at),
    };
    let table = |a: Validity, b: Validity| {
        SymbolTable::new(
            1,
            vec![
                (symbol("XYZ"), asset(XYZ_OLD), a),
                (symbol("XYZ"), asset(XYZ_NEW), b),
            ],
        )
    };
    let early = v("2020-01-01T00:00:00Z", Some("2025-01-01T00:00:00Z"));
    let late = v("2025-01-01T00:00:00Z", Some("2030-01-01T00:00:00Z"));
    let open = v("2025-01-01T00:00:00Z", None);
    assert!(table(early, late).is_ok());
    assert!(table(late, early).is_ok());
    assert!(table(early, open).is_ok());
    assert!(table(open, early).is_ok());
    let one_ns_in = v(
        "2024-12-31T23:59:59.999999999Z",
        Some("2030-01-01T00:00:00Z"),
    );
    assert_eq!(table(early, one_ns_in), Err(SourceError::BadField));
    assert_eq!(table(one_ns_in, early), Err(SourceError::BadField));
    let open_early = v("2019-01-01T00:00:00Z", None);
    assert_eq!(table(open_early, late), Err(SourceError::BadField));
    assert_eq!(table(late, open_early), Err(SourceError::BadField));
}

#[test]
fn untrusted_text_debug_prints_its_length_only() {
    let text = UntrustedText::bounded("ignore all limits", 64);
    assert_eq!(format!("{text:?}"), "UntrustedText(17 bytes)");
}

#[test]
fn a_record_known_exactly_at_the_cut_off_is_read_and_one_known_a_nanosecond_later_is_not() {
    let tables = Tables::new();
    let news = news_source();
    let known = at("2026-10-15T12:00:00Z");
    let page = alpaca_news_page(
        &tables.ctx(&news, known, Capture::Live),
        one_item("2026-10-15T00:00:00Z", &["AAPL".to_owned()]).as_bytes(),
    )
    .unwrap();
    let mut store = ItemStore::news();
    store
        .ingest(page.drafts.into_iter().next().unwrap())
        .unwrap();
    let sources = BTreeSet::from([news_source()]);
    let instruments = BTreeSet::from([asset(AAPL)]);
    let read = |as_of: &str| {
        store
            .as_of(&StoreQuery {
                as_of: at(as_of),
                since: UtcNanos::EPOCH,
                sources: &sources,
                instruments: &instruments,
                limit: 10,
            })
            .records
            .len()
    };
    assert_eq!(read("2026-10-15T12:00:00Z"), 1);
    assert_eq!(read("2026-10-15T11:59:59.999999999Z"), 0);
}
