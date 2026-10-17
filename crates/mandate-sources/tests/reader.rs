#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The typed reader lane I2 consumes (agent harness spec §6.3, §10.2; data-plane spec §4.6; DP-1,
//! DP-9; DEC-996 items 5, 6 and 9).

mod common;

use std::collections::BTreeSet;

use mandate_research::AllowlistVersion;
use mandate_research::drift::{DriftState, InputClass};
use mandate_sources::SourceError;
use mandate_sources::allowlist::Allowlist;
use mandate_sources::ingest::{alpaca_news_page, edgar_filing, edgar_submissions};
use mandate_sources::reader::{
    FILING_EXCERPT_BYTES, FILINGS_LIMIT, LicensedText, NEWS_ITEM_BYTES, NEWS_LIMIT, ReadQuery,
    SourceReader, StoreReader, observation,
};
use mandate_sources::record::Capture;
use mandate_sources::store::ItemStore;
use mandate_time::UtcNanos;

use common::*;

fn news_item(id: u32, published: UtcNanos, headline: &str, summary: &str, ticker: &str) -> String {
    format!(
        r#"{{"news":[{{"id":{id},"headline":"{headline}","created_at":"{published}","updated_at":"{published}","summary":"{summary}","url":"https://www.example-wire.com/{id}","symbols":["{ticker}"]}}],"next_page_token":null}}"#
    )
}

fn hours(base: &str, h: i64) -> UtcNanos {
    let t = at(base);
    UtcNanos::from_parts(t.secs() + h * 3_600, 0).unwrap()
}

const CUT: &str = "2026-10-16T12:00:00Z";

struct World {
    tables: Tables,
    news: ItemStore,
    filings: ItemStore,
}

impl World {
    fn new() -> Self {
        Self {
            tables: Tables::new(),
            news: ItemStore::news(),
            filings: ItemStore::filings(),
        }
    }

    fn add_news(&mut self, source: &str, body: &str, received: UtcNanos) {
        let source = common::source(source);
        let page = alpaca_news_page(
            &self.tables.ctx(&source, received, Capture::Live),
            body.as_bytes(),
        )
        .unwrap();
        for draft in page.drafts {
            self.news.ingest(draft).unwrap();
        }
    }

    fn reader(&self, licensed: LicensedText) -> StoreReader<'_> {
        StoreReader {
            allowlist: &self.tables.allowlist,
            news: &self.news,
            filings: &self.filings,
            licensed_text: licensed,
        }
    }
}

fn query(instruments: &[&str]) -> ReadQuery {
    ReadQuery {
        as_of: at(CUT),
        allowlist_version: AllowlistVersion(7),
        instruments: instruments.iter().map(|i| asset(i)).collect(),
        window_s: None,
        limit: None,
    }
}

#[test]
fn news_reads_hold_the_harness_caps_newest_first() {
    let mut world = World::new();
    let base = at("2026-10-16T11:30:00Z");
    for i in 0..60 {
        let t = UtcNanos::from_parts(base.secs() - i * 1_800, 0).unwrap();
        world.add_news(
            "alpaca.news",
            &news_item(u32::try_from(i).unwrap(), t, "h", "s", "AAPL"),
            t,
        );
    }
    let batch = world
        .reader(LicensedText::Included)
        .news(&query(&[AAPL]))
        .unwrap();
    assert_eq!(batch.items.len(), usize::try_from(NEWS_LIMIT).unwrap());
    assert_eq!(batch.items[0].typed.vendor_id.as_str(), "0");
    assert_eq!(batch.items[49].typed.vendor_id.as_str(), "49");
    assert_eq!(batch.dropped, 10);
}

#[test]
fn the_window_is_48_hours_of_knowledge_time_and_a_query_only_narrows_it() {
    let mut world = World::new();
    for (id, h) in [(1, -1), (2, -47), (3, -48), (4, -49)] {
        let t = hours(CUT, h);
        world.add_news("alpaca.news", &news_item(id, t, "h", "s", "AAPL"), t);
    }
    let reader = world.reader(LicensedText::Included);
    let ids = |q: &ReadQuery| -> Vec<String> {
        reader
            .news(q)
            .unwrap()
            .items
            .iter()
            .map(|i| i.typed.vendor_id.as_str().to_owned())
            .collect()
    };
    assert_eq!(ids(&query(&[AAPL])), ["1", "2"]);
    let mut wider = query(&[AAPL]);
    wider.window_s = Some(1_000_000);
    wider.limit = Some(1_000);
    assert_eq!(ids(&wider), ["1", "2"]);
    let mut narrower = query(&[AAPL]);
    narrower.window_s = Some(7_200);
    assert_eq!(ids(&narrower), ["1"]);
    let mut one = query(&[AAPL]);
    one.limit = Some(1);
    let batch = reader.news(&one).unwrap();
    assert_eq!(batch.items.len(), 1);
    assert_eq!(batch.dropped, 1);
}

#[test]
fn nothing_after_the_cut_off_and_nothing_outside_the_universe_is_read() {
    let mut world = World::new();
    let before = hours(CUT, -2);
    let after = hours(CUT, 1);
    world.add_news(
        "alpaca.news",
        &news_item(1, before, "h", "s", "AAPL"),
        before,
    );
    world.add_news(
        "alpaca.news",
        &news_item(2, before, "h", "s", "MSFT"),
        after,
    );
    world.add_news(
        "alpaca.news",
        &news_item(3, before, "h", "s", "TSLA"),
        before,
    );
    let reader = world.reader(LicensedText::Included);
    let ids: Vec<String> = reader
        .news(&query(&[AAPL, MSFT]))
        .unwrap()
        .items
        .iter()
        .map(|i| i.typed.vendor_id.as_str().to_owned())
        .collect();
    assert_eq!(ids, ["1"]);
    assert!(reader.news(&query(&[])).unwrap().items.is_empty());
}

#[test]
fn a_query_under_another_allowlist_version_is_refused() {
    let world = World::new();
    let mut stale = query(&[AAPL]);
    stale.allowlist_version = AllowlistVersion(6);
    let reader = world.reader(LicensedText::Included);
    assert_eq!(
        reader.news(&stale),
        Err(SourceError::AllowlistVersionMismatch)
    );
    assert_eq!(
        reader.filings(&stale),
        Err(SourceError::AllowlistVersionMismatch)
    );
}

#[test]
fn a_source_revoked_in_the_version_in_effect_is_never_returned() {
    let mut world = World::new();
    let t = hours(CUT, -1);
    world.add_news("alpaca.news", &news_item(1, t, "h", "s", "AAPL"), t);
    let revoked = Allowlist::parse(
        &TEST_ALLOWLIST
            .replace("version = 7", "version = 8")
            .replace(
                "id = \"alpaca.news\"\nkind = \"news\"\nlicensed = true\ncounsel_confirmed = true",
                "id = \"alpaca.news\"\nkind = \"news\"\nlicensed = true\ncounsel_confirmed = false",
            ),
    )
    .unwrap();
    assert!(revoked.active(&news_source()).is_none());
    let reader = StoreReader {
        allowlist: &revoked,
        news: &world.news,
        filings: &world.filings,
        licensed_text: LicensedText::Included,
    };
    let mut q = query(&[AAPL]);
    q.allowlist_version = AllowlistVersion(8);
    assert!(reader.news(&q).unwrap().items.is_empty());
}

#[test]
fn licensed_text_is_withheld_unless_included_and_the_typed_fields_stay() {
    let mut world = World::new();
    let t = hours(CUT, -1);
    world.add_news(
        "alpaca.news",
        &news_item(1, t, "headline", "summary", "AAPL"),
        t,
    );
    let withheld = world
        .reader(LicensedText::Withheld)
        .news(&query(&[AAPL]))
        .unwrap();
    let included = world
        .reader(LicensedText::Included)
        .news(&query(&[AAPL]))
        .unwrap();
    let (w, i) = (&withheld.items[0], &included.items[0]);
    assert!(w.licensed && i.licensed);
    assert_eq!(w.typed, i.typed);
    assert_eq!((&w.headline, &w.summary, &w.link), (&None, &None, &None));
    assert_eq!(i.headline.as_ref().unwrap().as_untrusted_str(), "headline");
    assert_eq!(i.summary.as_ref().unwrap().as_untrusted_str(), "summary");
    assert_eq!(
        i.link.as_ref().unwrap().as_untrusted_str(),
        "https://www.example-wire.com/1"
    );
}

#[test]
fn a_news_item_s_text_is_at_most_2000_bytes_headline_first() {
    let mut world = World::new();
    let t = hours(CUT, -1);
    let headline = "H".repeat(900);
    let summary = "S".repeat(3_000);
    world.add_news(
        "alpaca.news",
        &news_item(1, t, &headline, &summary, "AAPL"),
        t,
    );
    let item = &world
        .reader(LicensedText::Included)
        .news(&query(&[AAPL]))
        .unwrap()
        .items[0];
    let headline_len = item.headline.as_ref().unwrap().len_bytes();
    let link_len = item.link.as_ref().unwrap().len_bytes();
    let summary_len = item.summary.as_ref().unwrap().len_bytes();
    assert_eq!(headline_len, 900);
    assert_eq!(link_len, "https://www.example-wire.com/1".len());
    assert_eq!(headline_len + link_len + summary_len, NEWS_ITEM_BYTES);
}

fn add_filings(world: &mut World, received: &str, capture: Capture, document: &[u8]) {
    let source = edgar_source();
    let body = fixture("recorded/edgar-submissions-0000320193.json");
    let ctx = world.tables.ctx(&source, at(received), capture);
    let index = edgar_submissions(&ctx, &body).unwrap();
    for entry in &index.entries {
        let own = [document, entry.accession.as_str().as_bytes()].concat();
        world
            .filings
            .ingest(edgar_filing(&ctx, entry, &own).unwrap())
            .unwrap();
    }
}

#[test]
fn filings_reads_hold_seven_days_and_the_excerpt_cap() {
    let mut world = World::new();
    add_filings(
        &mut world,
        "2026-10-15T00:00:00Z",
        Capture::Backfill,
        "x".repeat(7_000).as_bytes(),
    );
    let mut q = query(&[AAPL]);
    q.as_of = at("2026-10-15T00:00:00Z");
    let batch = world.reader(LicensedText::Withheld).filings(&q).unwrap();
    let forms: Vec<&str> = batch
        .items
        .iter()
        .map(|e| e.typed.filing.as_ref().unwrap().form.as_str())
        .collect();
    assert_eq!(forms, ["10-K/A"]);
    let excerpt = batch.items[0].excerpt.as_ref().unwrap();
    assert_eq!(excerpt.len_bytes(), FILING_EXCERPT_BYTES);
    assert_eq!(batch.dropped, 0);
    let mut wide = q.clone();
    wide.as_of = at("2026-10-02T00:00:00Z");
    let forms: Vec<String> = world
        .reader(LicensedText::Withheld)
        .filings(&wide)
        .unwrap()
        .items
        .iter()
        .map(|e| e.typed.filing.as_ref().unwrap().form.as_str().to_owned())
        .collect();
    assert_eq!(forms, ["8-K"]);
    assert_eq!(FILINGS_LIMIT, 20);
}

#[test]
fn a_news_read_never_returns_filings_and_a_filings_read_never_news() {
    let mut world = World::new();
    let t = hours(CUT, -1);
    world.add_news("alpaca.news", &news_item(1, t, "h", "s", "AAPL"), t);
    add_filings(&mut world, "2026-10-16T11:00:00Z", Capture::Live, b"doc");
    let reader = world.reader(LicensedText::Included);
    assert!(
        reader
            .news(&query(&[AAPL]))
            .unwrap()
            .items
            .iter()
            .all(|i| i.typed.filing.is_none())
    );
    let filings = reader.filings(&query(&[AAPL])).unwrap();
    assert_eq!(filings.items.len(), 3);
    assert!(filings.items.iter().all(|e| e.typed.filing.is_some()));
}

#[test]
fn each_item_s_drift_observation_is_its_typed_record_at_knowledge_time() {
    let mut world = World::new();
    let published = hours(CUT, -5);
    let received = hours(CUT, -3);
    world.add_news(
        "alpaca.news",
        &news_item(1, published, "h", "s", "AAPL"),
        received,
    );
    add_filings(&mut world, "2026-10-16T11:00:00Z", Capture::Live, b"doc");
    let reader = world.reader(LicensedText::Withheld);
    let news = &reader.news(&query(&[AAPL])).unwrap().items[0];
    let seen = observation(&news.typed);
    assert_eq!(seen.source, news_source());
    assert_eq!(seen.class, InputClass::News);
    assert_eq!(seen.observed_at, received);
    assert_eq!(seen.content, news.typed.content);
    assert_eq!(seen.length_bytes, news.typed.length_bytes);
    let filing = &reader.filings(&query(&[AAPL])).unwrap().items[0];
    let seen = observation(&filing.typed);
    assert_eq!(seen.class, InputClass::Filing);
    assert_eq!(seen.source, edgar_source());
    assert_eq!(seen.length_bytes, 23);
    assert_eq!(seen.observed_at, at("2026-10-16T11:00:00Z"));
    let mut drift = DriftState::new();
    drift.observe(&seen).unwrap();
    assert_eq!(
        drift.report().unwrap().unusual().unwrap(),
        BTreeSet::from([edgar_source()]),
        "one observation is no baseline yet, which E17-5 reads as unusual"
    );
    let sources: BTreeSet<_> = world.tables.allowlist.research_allowlist().sources;
    assert!(sources.contains(&seen.source));
}
