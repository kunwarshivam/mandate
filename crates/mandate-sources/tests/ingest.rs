#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! Ingestion and the stores on recorded fixtures (data-plane spec §4.2 to §4.5, §5.2; DP-1, DP-2;
//! DEC-1130 items 4, 5 and 7).

mod common;

use std::collections::BTreeSet;

use mandate_canon::Digest;
use mandate_research::{AllowlistVersion, ContentHash};
use mandate_sources::SourceError;
use mandate_sources::allowlist::SourceKind;
use mandate_sources::ingest::{
    DocumentName, PageToken, STORED_EXCERPT_BYTES, alpaca_news_page, edgar_filing,
    edgar_submissions,
};
use mandate_sources::record::{Capture, FormType, VendorId};
use mandate_sources::store::{Ingested, ItemStore, StoreQuery};
use mandate_sources::symbols::{Cik, Symbol, SymbolTable, Validity};

use common::*;

const RECEIVED: &str = "2026-10-15T17:00:00Z";

fn news_store(tables: &Tables, file: &str, received: &str, capture: Capture) -> ItemStore {
    let mut store = ItemStore::news();
    ingest_news(&mut store, tables, file, received, capture);
    store
}

fn ingest_news(
    store: &mut ItemStore,
    tables: &Tables,
    file: &str,
    received: &str,
    capture: Capture,
) -> Vec<Ingested> {
    let body = fixture(file);
    let digest = store.put_raw(&body);
    assert_eq!(store.raw(&digest), Some(body.as_slice()));
    let source = news_source();
    let page = alpaca_news_page(&tables.ctx(&source, at(received), capture), &body).unwrap();
    page.drafts
        .into_iter()
        .map(|d| store.ingest(d).unwrap())
        .collect()
}

#[test]
fn a_news_page_becomes_records_from_structured_fields() {
    let tables = Tables::new();
    let source = news_source();
    let body = fixture("recorded/alpaca-news-page.json");
    let page = alpaca_news_page(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    assert!(page.refused.is_empty());
    assert_eq!(
        page.next_page_token,
        Some(PageToken::parse("MTcwMDAwMDAwMDAwMDAwMDAwMHw0MTAwMDAwMQ==").unwrap())
    );
    let ids: Vec<&str> = page.drafts.iter().map(|d| d.vendor_id().as_str()).collect();
    assert_eq!(ids, ["41000003", "41000002", "41000001"]);

    let mut store = ItemStore::news();
    for draft in page.drafts {
        store.ingest(draft).unwrap();
    }
    let second = store.records()[1].typed();
    assert_eq!(
        second.item_id.as_str(),
        "alpaca.news/41000002@2026-10-15T13:05:00.500000000Z"
    );
    assert_eq!(second.source, source);
    assert_eq!(second.allowlist_version, AllowlistVersion(7));
    assert_eq!(second.kind, SourceKind::News);
    assert_eq!(second.published_at, at("2026-10-15T13:00:00Z"));
    assert_eq!(second.vendor_version, at("2026-10-15T13:05:00.5Z"));
    assert_eq!(second.received_at, at(RECEIVED));
    assert_eq!(second.knowledge_time, at(RECEIVED));
    assert_eq!(second.tags, BTreeSet::from([asset(AAPL), asset(MSFT)]));
    assert_eq!(second.supersedes, None);
    assert_eq!(second.filing, None);
    let raw_item = br#"{"id":41000002,"headline":"Apple and Microsoft extend a licensing agreement","author":"Staff","created_at":"2026-10-15T13:00:00Z","updated_at":"2026-10-15T13:05:00.5Z","summary":"Terms were not disclosed.","content":"","url":"https://www.example-wire.com/news/41000002","symbols":["AAPL","MSFT"],"source":"benzinga","images":[]}"#;
    assert_eq!(second.content, ContentHash::new(Digest::of(raw_item)));
    assert_eq!(second.length_bytes, 328);
    assert_eq!(u64::try_from(raw_item.len()).unwrap(), second.length_bytes);
    let text = store.records()[1].text();
    assert_eq!(
        text.headline.as_ref().unwrap().as_untrusted_str(),
        "Apple and Microsoft extend a licensing agreement"
    );
    assert_eq!(
        text.summary.as_ref().unwrap().as_untrusted_str(),
        "Terms were not disclosed."
    );
    assert_eq!(
        text.link.as_ref().unwrap().as_untrusted_str(),
        "https://www.example-wire.com/news/41000002"
    );
    assert_eq!(text.excerpt, None);
}

/// DEC-2280 item 2: a parsed news item keeps the vendor's tickers as the vendor listed them, a
/// ticker the tagging table does not know included, while its tags still resolve only the known
/// ones; a filing's are its CIK table's.
#[test]
fn a_parsed_item_keeps_the_vendor_s_tickers_as_parsed() {
    let tables = Tables::new();
    let source = news_source();
    let body = fixture("recorded/alpaca-news-page.json");
    let page = alpaca_news_page(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    let tickers: Vec<Vec<&str>> = page
        .drafts
        .iter()
        .map(|d| d.tickers().iter().map(Symbol::as_str).collect())
        .collect();
    assert_eq!(
        tickers,
        [vec!["MSFT"], vec!["AAPL", "MSFT"], vec!["AAPL", "ZZZZ"]]
    );
    let last = &page.drafts[2];
    assert_eq!(last.tags(), &BTreeSet::from([asset(AAPL)]));
    assert_eq!(last.source(), &source);
    assert_eq!(last.allowlist_version(), AllowlistVersion(7));
    assert_eq!(last.published_at(), at("2026-10-15T12:00:00Z"));
    assert_eq!(last.vendor_version(), at("2026-10-15T12:00:00Z"));
    assert_eq!(last.received_at(), at(RECEIVED));
    let raw_item = br#"{"id":41000001,"headline":"Apple schedules its quarterly results call","author":"Staff","created_at":"2026-10-15T12:00:00Z","updated_at":"2026-10-15T12:00:00Z","summary":"The call is set for after the close.","content":"","url":"https://www.example-wire.com/news/41000001","symbols":["AAPL","ZZZZ"],"source":"benzinga","images":[]}"#;
    assert_eq!(last.content(), &ContentHash::new(Digest::of(raw_item)));
    assert_eq!(
        last.text().headline.as_ref().unwrap().as_untrusted_str(),
        "Apple schedules its quarterly results call"
    );

    let unsorted = br#"{"news":[{"id":7,"created_at":"2026-10-15T12:00:00Z","updated_at":"2026-10-15T12:00:00Z","symbols":["MSFT","AAPL","MSFT"]},{"id":8,"created_at":"2026-10-15T12:00:00Z","updated_at":"2026-10-15T12:00:00Z","symbols":[]}],"next_page_token":null}"#;
    let page =
        alpaca_news_page(&tables.ctx(&source, at(RECEIVED), Capture::Live), unsorted).unwrap();
    let tickers: Vec<Vec<&str>> = page
        .drafts
        .iter()
        .map(|d| d.tickers().iter().map(Symbol::as_str).collect())
        .collect();
    assert_eq!(
        tickers,
        [vec!["MSFT", "AAPL", "MSFT"], vec![]],
        "as parsed: the vendor's order, a repeat, and none at all are kept as they came"
    );
}

#[test]
fn an_unknown_ticker_is_dropped_and_tags_resolve_as_of_knowledge_time() {
    let tables = Tables::new();
    let store = news_store(
        &tables,
        "recorded/alpaca-news-page.json",
        RECEIVED,
        Capture::Live,
    );
    assert_eq!(
        store.records()[2].typed().tags,
        BTreeSet::from([asset(AAPL)])
    );

    let body = br#"{"news":[{"id":1,"headline":"h","created_at":"2026-05-01T00:00:00Z","updated_at":"2026-05-01T00:00:00Z","summary":"","url":"","symbols":["XYZ"]}],"next_page_token":null}"#;
    let source = news_source();
    let before = alpaca_news_page(
        &tables.ctx(&source, at("2026-05-01T00:00:01Z"), Capture::Live),
        body,
    )
    .unwrap();
    assert_eq!(before.drafts[0].tags(), &BTreeSet::from([asset(XYZ_OLD)]));
    let after = alpaca_news_page(
        &tables.ctx(&source, at("2026-07-01T00:00:00Z"), Capture::Live),
        body,
    )
    .unwrap();
    assert_eq!(after.drafts[0].tags(), &BTreeSet::from([asset(XYZ_NEW)]));
    let backfilled = alpaca_news_page(
        &tables.ctx(&source, at("2026-07-01T00:00:00Z"), Capture::Backfill),
        body,
    )
    .unwrap();
    assert_eq!(
        backfilled.drafts[0].tags(),
        &BTreeSet::from([asset(XYZ_OLD)])
    );
}

#[test]
fn knowledge_time_is_receipt_for_live_and_the_vendor_update_for_backfill() {
    let tables = Tables::new();
    let live = news_store(
        &tables,
        "recorded/alpaca-news-page.json",
        RECEIVED,
        Capture::Live,
    );
    let backfill = news_store(
        &tables,
        "recorded/alpaca-news-page.json",
        RECEIVED,
        Capture::Backfill,
    );
    for record in live.records() {
        assert_eq!(record.typed().knowledge_time, at(RECEIVED));
    }
    let times: Vec<_> = backfill
        .records()
        .iter()
        .map(|r| r.typed().knowledge_time)
        .collect();
    assert_eq!(
        times,
        [
            at("2026-10-15T14:30:00Z"),
            at("2026-10-15T13:05:00.5Z"),
            at("2026-10-15T12:00:00Z")
        ]
    );
}

#[test]
fn a_source_time_after_receipt_beyond_the_skew_is_refused() {
    let tables = Tables::new();
    let source = news_source();
    let body = fixture("recorded/alpaca-news-page.json");
    let just_inside = alpaca_news_page(
        &tables.ctx(&source, at("2026-10-15T14:29:59Z"), Capture::Live),
        &body,
    )
    .unwrap();
    assert!(just_inside.refused.is_empty());
    let outside = alpaca_news_page(
        &tables.ctx(
            &source,
            at("2026-10-15T14:29:58.999999999Z"),
            Capture::Backfill,
        ),
        &body,
    )
    .unwrap();
    assert_eq!(outside.refused, vec![(0, SourceError::FutureDated)]);
    assert_eq!(outside.drafts.len(), 2);
}

#[test]
fn an_exact_copy_is_stored_once_and_a_vendor_update_is_a_linked_version() {
    let tables = Tables::new();
    let mut store = ItemStore::news();
    let first = ingest_news(
        &mut store,
        &tables,
        "recorded/alpaca-news-page.json",
        RECEIVED,
        Capture::Live,
    );
    let again = ingest_news(
        &mut store,
        &tables,
        "recorded/alpaca-news-page.json",
        "2026-10-15T17:05:00Z",
        Capture::Live,
    );
    assert!(first.iter().all(|i| matches!(i, Ingested::Stored(_))));
    assert!(again.iter().all(|i| matches!(i, Ingested::Duplicate(_))));
    assert_eq!(store.records().len(), 3);

    let update = ingest_news(
        &mut store,
        &tables,
        "recorded/alpaca-news-update.json",
        "2026-10-15T16:00:02Z",
        Capture::Live,
    );
    assert_eq!(store.records().len(), 4);
    let original = store.records()[2].typed().clone();
    let newer = store.records()[3].typed();
    assert_eq!(update, vec![Ingested::Stored(newer.item_id.clone())]);
    assert_eq!(newer.supersedes, Some(original.item_id.clone()));
    assert_eq!(newer.group, original.group);
    assert_eq!(newer.vendor_id, original.vendor_id);
    assert_eq!(newer.tags, BTreeSet::from([asset(AAPL)]));
}

#[test]
fn different_content_under_a_stored_vendor_version_is_refused() {
    let tables = Tables::new();
    let source = news_source();
    let mut store = news_store(
        &tables,
        "recorded/alpaca-news-page.json",
        RECEIVED,
        Capture::Live,
    );
    let before = store.records().to_vec();
    for updated_at in ["2026-10-15T12:00:00Z", "2026-10-15T11:00:00Z"] {
        let body = format!(
            r#"{{"news":[{{"id":41000001,"headline":"rewritten","created_at":"2026-10-15T12:00:00Z","updated_at":"{updated_at}","summary":"","url":"","symbols":["AAPL"]}}],"next_page_token":null}}"#
        );
        let page = alpaca_news_page(
            &tables.ctx(&source, at(RECEIVED), Capture::Live),
            body.as_bytes(),
        )
        .unwrap();
        let draft = page.drafts.into_iter().next().unwrap();
        assert_eq!(store.ingest(draft), Err(SourceError::RevisedInPlace));
    }
    assert_eq!(store.records(), before.as_slice());
}

#[test]
fn syndicated_copies_share_a_group_and_other_stories_do_not() {
    let tables = Tables::new();
    let store = news_store(
        &tables,
        "recorded/alpaca-news-syndicated.json",
        "2026-10-16T10:00:00Z",
        Capture::Live,
    );
    let groups: Vec<_> = store.records().iter().map(|r| r.typed().group).collect();
    assert_eq!(groups[0], groups[1]);
    assert_ne!(groups[0], groups[2]);
}

#[test]
fn a_store_refuses_a_draft_of_the_other_kind() {
    let tables = Tables::new();
    let source = news_source();
    let body = fixture("recorded/alpaca-news-page.json");
    let page = alpaca_news_page(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    let mut filings = ItemStore::filings();
    assert_eq!(
        filings.ingest(page.drafts[0].clone()),
        Err(SourceError::WrongKind)
    );
    assert!(filings.records().is_empty());
}

#[test]
fn a_news_parser_refuses_a_filing_source_an_unlisted_source_and_an_oversized_body() {
    let tables = Tables::new();
    let body = fixture("recorded/alpaca-news-page.json");
    let edgar = edgar_source();
    assert_eq!(
        alpaca_news_page(&tables.ctx(&edgar, at(RECEIVED), Capture::Live), &body),
        Err(SourceError::WrongKind)
    );
    for id in ["evil.blog", "wire.unconfirmed"] {
        let unlisted = source(id);
        assert_eq!(
            alpaca_news_page(&tables.ctx(&unlisted, at(RECEIVED), Capture::Live), &body),
            Err(SourceError::SourceNotAllowlisted)
        );
    }
    let news = news_source();
    let huge = vec![b' '; 1_048_577];
    assert_eq!(
        alpaca_news_page(&tables.ctx(&news, at(RECEIVED), Capture::Live), &huge),
        Err(SourceError::ResponseTooLarge)
    );
    for malformed in [&b"not json"[..], br#"{"items":[]}"#, br#"{"news":{}}"#] {
        assert_eq!(
            alpaca_news_page(&tables.ctx(&news, at(RECEIVED), Capture::Live), malformed),
            Err(SourceError::Malformed)
        );
    }
}

#[test]
fn the_edgar_index_reads_rows_and_never_the_filer_s_own_tickers() {
    let tables = Tables::new();
    let source = edgar_source();
    let body = fixture("recorded/edgar-submissions-0000320193.json");
    let index =
        edgar_submissions(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    assert!(index.refused.is_empty());
    assert_eq!(index.entries.len(), 3);
    let eight_k = &index.entries[1];
    assert_eq!(eight_k.cik, Cik::parse("0000320193").unwrap());
    assert_eq!(
        eight_k.accession,
        VendorId::parse("0000320193-26-000090").unwrap()
    );
    assert_eq!(eight_k.form, FormType::parse("8-K").unwrap());
    assert_eq!(eight_k.accepted_at, at("2026-10-01T20:05:00Z"));
    assert_eq!(
        eight_k.primary_document,
        DocumentName::parse("aapl-8k-20261001.htm").unwrap()
    );
    assert_eq!(index.entries[0].form.as_str(), "10-K/A");

    let document = fixture("recorded/edgar-doc-8k.htm");
    let draft = edgar_filing(
        &tables.ctx(&source, at(RECEIVED), Capture::Backfill),
        eight_k,
        &document,
    )
    .unwrap();
    assert_eq!(draft.tags(), &BTreeSet::from([asset(AAPL)]));
    assert_eq!(draft.knowledge_time(), at("2026-10-01T20:05:00Z"));
    let mut store = ItemStore::filings();
    store.ingest(draft).unwrap();
    let typed = store.records()[0].typed();
    assert_eq!(typed.kind, SourceKind::Filings);
    assert_eq!(typed.content, ContentHash::new(Digest::of(&document)));
    assert_eq!(typed.length_bytes, u64::try_from(document.len()).unwrap());
    assert_eq!(typed.published_at, at("2026-10-01T20:05:00Z"));
    let filing = typed.filing.as_ref().unwrap();
    assert_eq!(filing.form.as_str(), "8-K");
    assert_eq!(filing.accession.as_str(), "0000320193-26-000090");
    let excerpt = store.records()[0].text().excerpt.as_ref().unwrap();
    assert_eq!(excerpt.as_untrusted_str().as_bytes(), document.as_slice());
    assert_eq!(store.records()[0].text().headline, None);
}

#[test]
fn an_amended_filing_is_a_new_record_and_the_original_stays_readable() {
    let tables = Tables::new();
    let source = edgar_source();
    let body = fixture("recorded/edgar-submissions-0000320193.json");
    let index =
        edgar_submissions(&tables.ctx(&source, at(RECEIVED), Capture::Backfill), &body).unwrap();
    let mut store = ItemStore::filings();
    for entry in index.entries.iter().rev() {
        let doc = format!("document {}", entry.accession.as_str());
        let draft = edgar_filing(
            &tables.ctx(&source, at(RECEIVED), Capture::Backfill),
            entry,
            doc.as_bytes(),
        )
        .unwrap();
        assert_eq!(
            draft.tickers(),
            [Symbol::parse("AAPL").unwrap()],
            "a filing's tickers are its CIK table's"
        );
        store.ingest(draft).unwrap();
    }
    assert_eq!(store.records().len(), 3);
    let sources = BTreeSet::from([edgar_source()]);
    let instruments = BTreeSet::from([asset(AAPL)]);
    let read = |as_of: &str| -> Vec<String> {
        store
            .as_of(&StoreQuery {
                as_of: at(as_of),
                since: at("2000-01-01T00:00:00Z"),
                sources: &sources,
                instruments: &instruments,
                limit: 10,
            })
            .records
            .iter()
            .map(|r| r.typed().filing.as_ref().unwrap().form.as_str().to_owned())
            .collect()
    };
    assert_eq!(read("2026-10-01T00:00:00Z"), ["10-K"]);
    assert_eq!(read("2026-10-15T00:00:00Z"), ["10-K/A", "8-K", "10-K"]);
    assert_eq!(read("2026-09-20T11:59:59.999999999Z"), Vec::<String>::new());
}

#[test]
fn a_long_document_is_cut_to_the_stored_excerpt() {
    let tables = Tables::new();
    let source = edgar_source();
    let body = fixture("recorded/edgar-submissions-0000320193.json");
    let index =
        edgar_submissions(&tables.ctx(&source, at(RECEIVED), Capture::Live), &body).unwrap();
    let document = "é".repeat(STORED_EXCERPT_BYTES);
    let draft = edgar_filing(
        &tables.ctx(&source, at(RECEIVED), Capture::Live),
        &index.entries[0],
        document.as_bytes(),
    )
    .unwrap();
    let mut store = ItemStore::filings();
    store.ingest(draft).unwrap();
    let excerpt = store.records()[0].text().excerpt.as_ref().unwrap();
    assert_eq!(excerpt.len_bytes(), STORED_EXCERPT_BYTES);
    assert_eq!(
        excerpt.as_untrusted_str(),
        "é".repeat(STORED_EXCERPT_BYTES / 2)
    );
    assert_eq!(
        store.records()[0].typed().length_bytes,
        u64::try_from(document.len()).unwrap()
    );
}

#[test]
fn identifiers_parse_only_in_their_allowed_forms() {
    assert_eq!(Cik::parse("0000320193").unwrap().padded(), "0000320193");
    assert_eq!(Cik::parse("320193").unwrap().get(), 320_193);
    for bad in ["", "12345678901", "32019a", "-1", " 1"] {
        assert_eq!(Cik::parse(bad), Err(SourceError::BadField), "{bad:?}");
    }
    for good in ["AAPL", "BRK.B", "BTC/USD", "RDS-A", "A", "ABCDEFGHIJKL"] {
        assert_eq!(Symbol::parse(good).unwrap().as_str(), good);
    }
    for bad in ["", "aapl", "AAPL ", "ABCDEFGHIJKLM", "A\"B", "A,B"] {
        assert_eq!(Symbol::parse(bad), Err(SourceError::BadField), "{bad:?}");
    }
    for good in ["41000001", "0000320193-26-000090", "a_b.c-d"] {
        assert_eq!(VendorId::parse(good).unwrap().as_str(), good);
    }
    for bad in ["", "a b", "a/b", "a;b", &"x".repeat(65)] {
        assert_eq!(VendorId::parse(bad), Err(SourceError::BadField), "{bad:?}");
    }
    for good in ["10-K", "10-K/A", "SC 13G", "8-K"] {
        assert_eq!(FormType::parse(good).unwrap().as_str(), good);
    }
    for bad in ["", "8-k", "8-K;", "ABCDEFGHIJKLMNOPQ"] {
        assert_eq!(FormType::parse(bad), Err(SourceError::BadField), "{bad:?}");
    }
    for good in ["aapl-8k.htm", "d123_x.txt"] {
        assert_eq!(DocumentName::parse(good).unwrap().as_str(), good);
    }
    for bad in ["", "..", "a/b.htm", "../x", "a..b", "a b"] {
        assert_eq!(
            DocumentName::parse(bad),
            Err(SourceError::BadField),
            "{bad:?}"
        );
    }
    for good in ["abc", "MTcw/=_-"] {
        assert_eq!(PageToken::parse(good).unwrap().as_str(), good);
    }
    for bad in ["", "a&b", "a b", "a+b", "x\"&symbols=GME", &"x".repeat(257)] {
        assert_eq!(PageToken::parse(bad), Err(SourceError::BadField), "{bad:?}");
    }
}

#[test]
fn tagging_tables_refuse_overlaps_and_empty_validities() {
    let row = |from: &str, until: Option<&str>| Validity {
        from: at(from),
        until: until.map(at),
    };
    assert_eq!(
        SymbolTable::new(
            1,
            vec![
                (
                    symbol("XYZ"),
                    asset(XYZ_OLD),
                    row("2020-01-01T00:00:00Z", None)
                ),
                (
                    symbol("XYZ"),
                    asset(XYZ_NEW),
                    row("2025-01-01T00:00:00Z", None)
                ),
            ]
        ),
        Err(SourceError::BadField)
    );
    assert_eq!(
        SymbolTable::new(
            1,
            vec![(
                symbol("XYZ"),
                asset(XYZ_OLD),
                row("2020-01-01T00:00:00Z", Some("2020-01-01T00:00:00Z"))
            )]
        ),
        Err(SourceError::BadField)
    );
    let table = SymbolTable::new(
        1,
        vec![
            (
                symbol("XYZ"),
                asset(XYZ_OLD),
                row("2020-01-01T00:00:00Z", Some("2025-01-01T00:00:00Z")),
            ),
            (
                symbol("XYZ"),
                asset(XYZ_NEW),
                row("2025-01-01T00:00:00Z", None),
            ),
        ],
    )
    .unwrap();
    assert_eq!(table.version(), 1);
    assert_eq!(
        table.resolve(&symbol("XYZ"), at("2019-12-31T23:59:59.999999999Z")),
        None
    );
    assert_eq!(
        table.resolve(&symbol("XYZ"), at("2020-01-01T00:00:00Z")),
        Some(&asset(XYZ_OLD))
    );
    assert_eq!(
        table.resolve(&symbol("XYZ"), at("2024-12-31T23:59:59.999999999Z")),
        Some(&asset(XYZ_OLD))
    );
    assert_eq!(
        table.resolve(&symbol("XYZ"), at("2025-01-01T00:00:00Z")),
        Some(&asset(XYZ_NEW))
    );
    assert_eq!(
        table.resolve(&symbol("ABC"), at("2025-01-01T00:00:00Z")),
        None
    );
    let ciks = ciks();
    assert_eq!(ciks.version(), 2);
    assert_eq!(
        ciks.symbols(Cik::parse("320193").unwrap(), at(RECEIVED)),
        vec![&symbol("AAPL")]
    );
    assert!(
        ciks.symbols(Cik::parse("1318605").unwrap(), at(RECEIVED))
            .is_empty()
    );
    assert!(
        ciks.symbols(Cik::parse("320193").unwrap(), at("1999-01-01T00:00:00Z"))
            .is_empty()
    );
}
