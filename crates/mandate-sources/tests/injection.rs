#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The prompt-injection corpus (DP-8, E17-7, E2-10): text that instructs a model, spoofs
//! structured fields, or smuggles markup never changes a typed field, never adds a tag, never
//! leaves its caps, and never becomes a URL a fetcher sends. A structured field that carries a
//! payload refuses its item.

mod common;

use std::collections::BTreeSet;

use mandate_research::AllowlistVersion;
use mandate_sources::SourceError;
use mandate_sources::ingest::{
    STORED_HEADLINE_BYTES, STORED_LINK_BYTES, STORED_SUMMARY_BYTES, alpaca_news_page, edgar_filing,
    edgar_submissions,
};
use mandate_sources::reader::{
    LicensedText, NEWS_ITEM_BYTES, ReadQuery, SourceReader, StoreReader,
};
use mandate_sources::record::{Capture, TypedFields};
use mandate_sources::store::ItemStore;
use proptest::prelude::*;
use serde_json::Value;

use common::*;

const RECEIVED: &str = "2026-10-16T12:00:00Z";

/// The typed fields a decision can read, without the digest, the length, and the story group,
/// which are functions of the bytes by design (§4.4, DEC-266): equal across an item and its
/// benign twin exactly when no text reached a typed field.
fn decision_fields(t: &TypedFields) -> impl PartialEq + core::fmt::Debug {
    (
        t.item_id.clone(),
        t.source.clone(),
        t.allowlist_version,
        t.kind,
        t.vendor_id.clone(),
        t.vendor_version,
        t.published_at,
        t.received_at,
        t.knowledge_time,
        t.tags.clone(),
        t.supersedes.clone(),
        t.filing.clone(),
    )
}

/// The page with every free-text member of every item replaced by plain prose.
fn benign_twin(body: &[u8]) -> Vec<u8> {
    let mut page: Value = serde_json::from_slice(body).unwrap();
    for item in page["news"].as_array_mut().unwrap() {
        for key in ["headline", "summary", "content", "url", "author"] {
            item[key] = Value::String("An ordinary sentence.".to_owned());
        }
    }
    serde_json::to_vec(&page).unwrap()
}

#[test]
fn injected_news_text_changes_no_typed_field() {
    let tables = Tables::new();
    let source = news_source();
    let ctx = tables.ctx(&source, at(RECEIVED), Capture::Live);
    let body = fixture("injection/alpaca-news-injection.json");
    let injected = alpaca_news_page(&ctx, &body).unwrap();
    let twin = alpaca_news_page(&ctx, &benign_twin(&body)).unwrap();
    assert!(injected.refused.is_empty(), "{:?}", injected.refused);
    assert_eq!(injected.drafts.len(), 5);
    let mut a = ItemStore::news();
    let mut b = ItemStore::news();
    for (x, y) in injected.drafts.into_iter().zip(twin.drafts) {
        a.ingest(x).unwrap();
        b.ingest(y).unwrap();
    }
    for (x, y) in a.records().iter().zip(b.records()) {
        assert_eq!(decision_fields(x.typed()), decision_fields(y.typed()));
        assert_eq!(x.typed().source, news_source(), "the vendor's own `source` member is ignored");
        assert_eq!(x.typed().allowlist_version, AllowlistVersion(7));
        assert_eq!(x.typed().knowledge_time, at(RECEIVED));
    }
    let tags: Vec<BTreeSet<_>> = a.records().iter().map(|r| r.typed().tags.clone()).collect();
    assert_eq!(
        tags,
        [
            BTreeSet::from([asset(AAPL)]),
            BTreeSet::from([asset(MSFT)]),
            BTreeSet::from([asset(AAPL)]),
            BTreeSet::from([asset(MSFT)]),
            BTreeSet::from([asset(TSLA)]),
        ],
        "tags come from `symbols` alone: no GME, NVDA, or TSLA named in text"
    );
    assert_eq!(injected.next_page_token, None, "a continuation token carrying a payload is dropped");
}

#[test]
fn injected_news_text_stays_inside_its_caps() {
    let tables = Tables::new();
    let source = news_source();
    let body = fixture("injection/alpaca-news-injection.json");
    let page = alpaca_news_page(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    let mut store = ItemStore::news();
    for d in page.drafts {
        store.ingest(d).unwrap();
    }
    for r in store.records() {
        let t = r.text();
        assert!(t.headline.as_ref().is_none_or(|x| x.len_bytes() <= STORED_HEADLINE_BYTES));
        assert!(t.summary.as_ref().is_none_or(|x| x.len_bytes() <= STORED_SUMMARY_BYTES));
        assert!(t.link.as_ref().is_none_or(|x| x.len_bytes() <= STORED_LINK_BYTES));
        assert_eq!(t.excerpt, None);
    }
    let reader = StoreReader {
        allowlist: &tables.allowlist,
        news: &store,
        filings: &ItemStore::filings(),
        licensed_text: LicensedText::Included,
    };
    let batch = reader
        .news(&ReadQuery {
            as_of: at("2026-10-16T13:00:00Z"),
            allowlist_version: AllowlistVersion(7),
            instruments: [AAPL, MSFT, TSLA, GME, NVDA].iter().map(|i| asset(i)).collect(),
            window_s: None,
            limit: None,
        })
        .unwrap();
    assert_eq!(batch.items.len(), 5);
    for item in &batch.items {
        let bytes = [&item.headline, &item.summary, &item.link]
            .iter()
            .map(|x| x.as_ref().map_or(0, |t| t.len_bytes()))
            .sum::<usize>();
        assert!(bytes <= NEWS_ITEM_BYTES);
        assert!(item.typed.tags.iter().all(|t| *t != asset(GME) && *t != asset(NVDA)));
    }
}

#[test]
fn a_structured_field_carrying_a_payload_refuses_its_item() {
    let tables = Tables::new();
    let source = news_source();
    let body = fixture("injection/alpaca-news-refused.json");
    let page = alpaca_news_page(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    assert!(page.drafts.is_empty());
    assert_eq!(
        page.refused,
        vec![
            (0, SourceError::BadField),
            (1, SourceError::TooManyTags),
            (2, SourceError::FutureDated),
            (3, SourceError::Malformed),
            (4, SourceError::Malformed),
            (5, SourceError::Malformed),
        ]
    );
}

#[test]
fn an_injected_filing_index_and_document_change_no_typed_field() {
    let tables = Tables::new();
    let source = edgar_source();
    let ctx = tables.ctx(&source, at(RECEIVED), Capture::Live);
    let index = edgar_submissions(&ctx, &fixture("injection/edgar-submissions-injection.json")).unwrap();
    assert_eq!(index.entries.len(), 1);
    assert_eq!(
        index.refused,
        vec![(1, SourceError::BadField), (2, SourceError::BadField), (3, SourceError::BadField)]
    );
    let entry = &index.entries[0];
    let injected = edgar_filing(&ctx, entry, &fixture("injection/edgar-doc-injection.htm")).unwrap();
    let plain = edgar_filing(&ctx, entry, b"<html><body>Item 8.01. Nothing to report.</body></html>").unwrap();
    let mut a = ItemStore::filings();
    let mut b = ItemStore::filings();
    a.ingest(injected).unwrap();
    b.ingest(plain).unwrap();
    assert_eq!(decision_fields(a.records()[0].typed()), decision_fields(b.records()[0].typed()));
    assert_eq!(a.records()[0].typed().tags, BTreeSet::from([asset(AAPL)]));
}

#[test]
fn no_fetch_takes_a_url_from_an_item() {
    let tables = Tables::new();
    let source = news_source();
    let body = fixture("injection/alpaca-news-injection.json");
    let page = alpaca_news_page(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    let mut store = ItemStore::news();
    for d in page.drafts {
        store.ingest(d).unwrap();
    }
    for r in store.records() {
        if let Some(link) = &r.text().link {
            assert!(
                tables.allowlist.permit(&news_source(), link.as_untrusted_str()).is_err(),
                "an item's link is never a permitted URL"
            );
        }
    }
}

proptest! {
    #[test]
    fn any_text_leaves_every_typed_field_unchanged(
        headline in any::<String>(),
        summary in any::<String>(),
        url in any::<String>(),
        content in any::<String>(),
    ) {
        let tables = Tables::new();
        let source = news_source();
        let ctx = tables.ctx(&source, at(RECEIVED), Capture::Live);
        let item = |h: &str, s: &str, u: &str, c: &str| {
            serde_json::json!({"news": [{
                "id": 7, "headline": h, "summary": s, "url": u, "content": c,
                "created_at": "2026-10-16T11:00:00Z", "updated_at": "2026-10-16T11:00:00Z",
                "symbols": ["AAPL", "MSFT"], "source": "benzinga"
            }], "next_page_token": null})
            .to_string()
        };
        let injected = alpaca_news_page(&ctx, item(&headline, &summary, &url, &content).as_bytes()).unwrap();
        let plain = alpaca_news_page(&ctx, item("h", "s", "u", "c").as_bytes()).unwrap();
        let mut a = ItemStore::news();
        let mut b = ItemStore::news();
        a.ingest(injected.drafts.into_iter().next().unwrap()).unwrap();
        b.ingest(plain.drafts.into_iter().next().unwrap()).unwrap();
        prop_assert_eq!(decision_fields(a.records()[0].typed()), decision_fields(b.records()[0].typed()));
        let text = a.records()[0].text();
        prop_assert!(text.headline.as_ref().is_none_or(|x| x.len_bytes() <= STORED_HEADLINE_BYTES));
        prop_assert!(text.summary.as_ref().is_none_or(|x| x.len_bytes() <= STORED_SUMMARY_BYTES));
        prop_assert!(text.link.as_ref().is_none_or(|x| x.len_bytes() <= STORED_LINK_BYTES));
    }
}
