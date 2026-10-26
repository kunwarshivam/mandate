#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The fetchers on recorded responses: requests built from typed values, permitted before they
//! reach the transport (DP-9), paced, and capped (DEC-1130 items 3 and 10). No network.

mod common;

use mandate_sources::SourceError;
use mandate_sources::fetch::{Fetcher, RawResponse};
use mandate_sources::ingest::{IngestContext, PageToken, edgar_submissions};
use mandate_sources::record::Capture;

use common::*;

const NOW: &str = "2026-10-16T12:00:00Z";

fn fetcher(answers: Vec<Result<RawResponse, SourceError>>) -> Fetcher<Recorded> {
    Fetcher::new(allowlist(), Recorded::answering(answers))
}

#[test]
fn a_news_request_names_only_typed_values_and_carries_the_credential() {
    let page = fixture("recorded/alpaca-news-page.json");
    let mut f = fetcher(vec![Recorded::ok(&page), Recorded::ok(&page)]);
    let body = block_on(f.alpaca_news(
        &news_source(),
        &auth(),
        &[symbol("AAPL"), symbol("BRK.B")],
        at("2026-10-15T00:00:00Z"),
        None,
        at(NOW),
    ))
    .unwrap();
    assert_eq!(body, page);
    let token = PageToken::parse("MTcw/=").unwrap();
    block_on(f.alpaca_news(
        &news_source(),
        &auth(),
        &[symbol("MSFT")],
        at("2026-10-15T00:00:00.5Z"),
        Some(&token),
        at("2026-10-16T12:00:01Z"),
    ))
    .unwrap();
    assert_eq!(
        f.transport().sent.borrow().as_slice(),
        [
            ("https://data.alpaca.markets/v1beta1/news?symbols=AAPL,BRK.B&start=2026-10-15T00:00:00Z&limit=50&sort=desc".to_owned(), true),
            ("https://data.alpaca.markets/v1beta1/news?symbols=MSFT&start=2026-10-15T00:00:00.500000000Z&limit=50&sort=desc&page_token=MTcw/=".to_owned(), true),
        ]
    );
}

#[test]
fn edgar_requests_carry_no_credential_and_name_the_archive_path() {
    let index = fixture("recorded/edgar-submissions-0000320193.json");
    let mut f = fetcher(vec![Recorded::ok(&index), Recorded::ok(b"<html/>")]);
    let body = block_on(f.edgar_submissions(
        &edgar_source(),
        mandate_sources::symbols::Cik::parse("320193").unwrap(),
        at(NOW),
    ))
    .unwrap();
    let tables = Tables::new();
    let source = edgar_source();
    let ctx: IngestContext<'_> = tables.ctx(&source, at(NOW), Capture::Live);
    let parsed = edgar_submissions(&ctx, &body).unwrap();
    block_on(f.edgar_document(
        &edgar_source(),
        &parsed.entries[1],
        at("2026-10-16T12:00:00.125Z"),
    ))
    .unwrap();
    assert_eq!(
        f.transport().sent.borrow().as_slice(),
        [
            ("https://data.sec.gov/submissions/CIK0000320193.json".to_owned(), false),
            ("https://www.sec.gov/Archives/edgar/data/320193/000032019326000090/aapl-8k-20261001.htm".to_owned(), false),
        ]
    );
}

#[test]
fn an_unlisted_or_unconfirmed_or_mismatched_source_sends_nothing() {
    let mut f = fetcher(vec![Recorded::ok(b"{}")]);
    let cik = mandate_sources::symbols::Cik::parse("320193").unwrap();
    for id in ["evil.blog", "wire.unconfirmed"] {
        assert_eq!(
            block_on(f.edgar_submissions(&source(id), cik, at(NOW))),
            Err(SourceError::SourceNotAllowlisted)
        );
        assert_eq!(
            block_on(f.alpaca_news(
                &source(id),
                &auth(),
                &[symbol("AAPL")],
                at(NOW),
                None,
                at(NOW)
            )),
            Err(SourceError::SourceNotAllowlisted)
        );
    }
    assert_eq!(
        block_on(f.edgar_submissions(&news_source(), cik, at(NOW))),
        Err(SourceError::EndpointNotAllowlisted)
    );
    assert_eq!(
        block_on(f.alpaca_news(
            &edgar_source(),
            &auth(),
            &[symbol("AAPL")],
            at(NOW),
            None,
            at(NOW)
        )),
        Err(SourceError::EndpointNotAllowlisted)
    );
    assert!(f.transport().sent.borrow().is_empty());
}

#[test]
fn the_checked_in_allowlist_lets_no_news_request_out() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/sources/allowlist-v1.toml");
    let list =
        mandate_sources::allowlist::Allowlist::parse(&std::fs::read_to_string(path).unwrap())
            .unwrap();
    let mut f = Fetcher::new(list, Recorded::answering(vec![Recorded::ok(b"{}")]));
    assert_eq!(
        block_on(f.alpaca_news(
            &news_source(),
            &auth(),
            &[symbol("AAPL")],
            at(NOW),
            None,
            at(NOW)
        )),
        Err(SourceError::SourceNotAllowlisted)
    );
    assert!(f.transport().sent.borrow().is_empty());
}

/// DEC-2276: the version in use, 3, lets a news request out on the founder's approval, to the
/// news endpoint only, with its credential.
#[test]
fn the_checked_in_version_three_lets_a_news_request_out_to_its_endpoint() {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../config/sources/allowlist-v3.toml");
    let list =
        mandate_sources::allowlist::Allowlist::parse(&std::fs::read_to_string(path).unwrap())
            .unwrap();
    let mut f = Fetcher::new(list, Recorded::answering(vec![Recorded::ok(b"{}")]));
    block_on(f.alpaca_news(
        &news_source(),
        &auth(),
        &[symbol("AAPL")],
        at(NOW),
        None,
        at(NOW),
    ))
    .unwrap();
    let sent = f.transport().sent.borrow();
    assert_eq!(sent.len(), 1);
    assert!(
        sent[0]
            .0
            .starts_with("https://data.alpaca.markets/v1beta1/news?"),
        "{:?}",
        sent[0].0
    );
    assert!(sent[0].1, "the request carries the credential");
}

#[test]
fn a_request_inside_the_minimum_interval_is_refused_before_sending() {
    let mut f = fetcher(vec![
        Recorded::ok(b"{}"),
        Recorded::ok(b"{}"),
        Recorded::ok(b"{}"),
    ]);
    let cik = mandate_sources::symbols::Cik::parse("320193").unwrap();
    block_on(f.edgar_submissions(&edgar_source(), cik, at("2026-10-16T12:00:00Z"))).unwrap();
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at("2026-10-16T12:00:00.124999999Z"))),
        Err(SourceError::TooSoon)
    );
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at("2026-10-16T11:59:59Z"))),
        Err(SourceError::TooSoon)
    );
    block_on(f.edgar_submissions(&edgar_source(), cik, at("2026-10-16T12:00:00.125Z"))).unwrap();
    block_on(f.alpaca_news(
        &news_source(),
        &auth(),
        &[symbol("AAPL")],
        at(NOW),
        None,
        at("2026-10-16T12:00:00.125Z"),
    ))
    .unwrap();
    assert_eq!(f.transport().sent.borrow().len(), 3);
}

#[test]
fn a_failed_send_still_counts_against_the_interval() {
    let mut f = fetcher(vec![Err(SourceError::Transport), Recorded::ok(b"{}")]);
    let cik = mandate_sources::symbols::Cik::parse("320193").unwrap();
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at(NOW))),
        Err(SourceError::Transport)
    );
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at("2026-10-16T12:00:00.1Z"))),
        Err(SourceError::TooSoon)
    );
}

#[test]
fn a_non_200_status_and_an_oversized_body_are_refused() {
    let mut f = fetcher(vec![
        Ok(RawResponse {
            status: 429,
            body: b"slow down".to_vec(),
        }),
        Ok(RawResponse {
            status: 301,
            body: Vec::new(),
        }),
        Ok(RawResponse {
            status: 200,
            body: vec![b'x'; 8_388_609],
        }),
        Ok(RawResponse {
            status: 200,
            body: vec![b'x'; 8_388_608],
        }),
    ]);
    let cik = mandate_sources::symbols::Cik::parse("320193").unwrap();
    let times = [
        "2026-10-16T12:00:00Z",
        "2026-10-16T12:00:01Z",
        "2026-10-16T12:00:02Z",
        "2026-10-16T12:00:03Z",
    ];
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at(times[0]))),
        Err(SourceError::Status)
    );
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at(times[1]))),
        Err(SourceError::Status)
    );
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at(times[2]))),
        Err(SourceError::ResponseTooLarge)
    );
    assert_eq!(
        block_on(f.edgar_submissions(&edgar_source(), cik, at(times[3])))
            .unwrap()
            .len(),
        8_388_608
    );
}

#[test]
fn a_news_request_needs_one_to_fifty_tickers() {
    let mut f = fetcher(vec![]);
    let many: Vec<_> = (0..51).map(|i| symbol(&format!("A{i}"))).collect();
    assert_eq!(
        block_on(f.alpaca_news(&news_source(), &auth(), &[], at(NOW), None, at(NOW))),
        Err(SourceError::BadField)
    );
    assert_eq!(
        block_on(f.alpaca_news(&news_source(), &auth(), &many, at(NOW), None, at(NOW))),
        Err(SourceError::BadField)
    );
    assert!(f.transport().sent.borrow().is_empty());
    assert_eq!(format!("{:?}", auth()), "NewsAuth(redacted)");
}
