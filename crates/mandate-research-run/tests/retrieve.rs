//! Spec §6.3's reads at a cut-off: caps, the oldest-first drop, knowledge-time filters (HI-13,
//! DP-1), the allowlist, licensed text (HI-23), scope, and the capture record (HI-2).

#![allow(
    clippy::field_reassign_with_default,
    reason = "each case starts from the empty fixture and states only the reads it is about"
)]

mod common;

use common::{
    AAPL_ID, BLOG, CUT_OFF, DAY, EDGAR, Fixture, MSFT_ID, NEWSWIRE, bars, filing, instrument, news,
    plan, scope, symbol,
};
use mandate_canon::{Digest, parse, to_canonical};
use mandate_research_run::{DropReason, ItemKind, RetrievedItem, retrieve};

fn kept(items: &[RetrievedItem], kind: ItemKind) -> Vec<String> {
    items
        .iter()
        .filter(|item| item.kind() == kind)
        .map(|item| {
            item.item_id()
                .unwrap_or_else(|e| panic!("every kept item has an id: {e}"))
                .as_str()
                .to_owned()
        })
        .collect()
}

fn dropped(retrieval: &mandate_research_run::Retrieval, reason: DropReason) -> Vec<String> {
    retrieval
        .drops
        .iter()
        .filter(|drop| drop.reason == reason)
        .map(|drop| drop.item.as_str().to_owned())
        .collect()
}

#[test]
fn bars_are_capped_per_instrument_and_the_oldest_go_first() {
    let mut fixture = Fixture::default();
    fixture.bars = bars("AAPL", 25);
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    let kept_bars: Vec<i64> = retrieval
        .items
        .iter()
        .filter_map(|item| match item {
            RetrievedItem::Bar(bar) => Some(bar.knowledge_at.secs()),
            _ => None,
        })
        .collect();
    assert_eq!(kept_bars.len(), 20, "20 bars per instrument (§6.3)");
    let oldest_kept = CUT_OFF - 20 * DAY;
    assert!(
        kept_bars.iter().all(|secs| *secs >= oldest_kept),
        "the five oldest bars are the ones dropped"
    );
    assert_eq!(dropped(&retrieval, DropReason::OverCap).len(), 5);
}

#[test]
fn a_record_known_after_the_cut_off_is_dropped_whatever_its_published_time() {
    let mut fixture = Fixture::default();
    let mut leaked = news("n-late", NEWSWIRE, "AAPL", CUT_OFF + 1);
    leaked.published_at = common::at(CUT_OFF - 3_600);
    fixture.news = vec![leaked, news("n-ok", NEWSWIRE, "AAPL", CUT_OFF)];
    let mut late_bar = common::bar("AAPL", CUT_OFF + 60, "101");
    late_bar.knowledge_at = common::at(CUT_OFF + 60);
    fixture.bars = vec![late_bar];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::News), vec!["n-ok"]);
    assert!(kept(&retrieval.items, ItemKind::Bar).is_empty());
    let late = dropped(&retrieval, DropReason::AfterCutOff);
    assert!(
        late.contains(&"n-late".to_owned()),
        "HI-13: knowledge time decides, not published time"
    );
    assert_eq!(late.len(), 2, "the late bar is dropped too");
}

#[test]
fn news_outside_its_48_hour_window_is_dropped() {
    let mut fixture = Fixture::default();
    fixture.news = vec![
        news("n-edge", NEWSWIRE, "AAPL", CUT_OFF - 2 * DAY),
        news("n-old", NEWSWIRE, "AAPL", CUT_OFF - 2 * DAY - 1),
    ];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::News), vec!["n-edge"]);
    assert_eq!(dropped(&retrieval, DropReason::BeforeWindow), vec!["n-old"]);
}

#[test]
fn filings_have_a_seven_day_window() {
    let mut fixture = Fixture::default();
    fixture.filings = vec![
        filing("f-edge", "AAPL", CUT_OFF - 7 * DAY),
        filing("f-old", "AAPL", CUT_OFF - 7 * DAY - 1),
    ];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::Filing), vec!["f-edge"]);
    assert_eq!(dropped(&retrieval, DropReason::BeforeWindow), vec!["f-old"]);
}

#[test]
fn a_source_off_the_allowlist_is_dropped_whatever_the_reader_returned() {
    let mut fixture = Fixture::default();
    fixture.news = vec![
        news("n-blog", BLOG, "AAPL", CUT_OFF - 60),
        news("n-wire", NEWSWIRE, "AAPL", CUT_OFF - 60),
    ];
    let mut planted = filing("f-planted", "AAPL", CUT_OFF - 60);
    planted.source = common::source(BLOG);
    fixture.filings = vec![planted];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::News), vec!["n-wire"]);
    assert!(kept(&retrieval.items, ItemKind::Filing).is_empty());
    assert_eq!(
        dropped(&retrieval, DropReason::NotAllowlisted),
        vec!["f-planted", "n-blog"]
    );
}

#[test]
fn licensed_text_is_never_kept_until_the_prompt_record_can_redact_it() {
    let mut fixture = Fixture::default();
    let mut licensed = news("n-licensed", NEWSWIRE, "AAPL", CUT_OFF - 60);
    licensed.licensed = true;
    fixture.news = vec![licensed];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert!(kept(&retrieval.items, ItemKind::News).is_empty());
    assert_eq!(
        dropped(&retrieval, DropReason::Licensed),
        vec!["n-licensed"]
    );
}

#[test]
fn news_is_capped_at_50_items_oldest_first() {
    let mut fixture = Fixture::default();
    fixture.news = (0..60)
        .map(|n| news(&format!("n-{n:02}"), NEWSWIRE, "AAPL", CUT_OFF - 3_600 + n))
        .collect();
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    let kept_news = kept(&retrieval.items, ItemKind::News);
    assert_eq!(kept_news.len(), 50);
    assert!(!kept_news.contains(&"n-09".to_owned()));
    assert!(kept_news.contains(&"n-10".to_owned()));
    let over: Vec<String> = (0..10).map(|n| format!("n-{n:02}")).collect();
    assert_eq!(dropped(&retrieval, DropReason::OverCap), over);
}

#[test]
fn news_text_is_cut_to_2000_bytes_on_a_character_boundary_and_the_cut_recorded() {
    let mut fixture = Fixture::default();
    let mut long = news("n-long", NEWSWIRE, "AAPL", CUT_OFF - 60);
    long.headline = "H".repeat(100);
    long.summary = "é".repeat(1_500);
    fixture.news = vec![long];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    let Some(RetrievedItem::News(kept_item)) = retrieval
        .items
        .iter()
        .find(|item| item.kind() == ItemKind::News)
    else {
        panic!("the long item is kept, trimmed");
    };
    let bytes = kept_item.headline.len() + kept_item.summary.len();
    assert!(bytes <= 2_000, "{bytes} bytes kept");
    assert!(bytes >= 1_999, "the cut keeps as much as fits: {bytes}");
    assert_eq!(dropped(&retrieval, DropReason::Trimmed), vec!["n-long"]);
}

#[test]
fn news_about_no_instrument_in_scope_is_dropped() {
    let mut fixture = Fixture::default();
    fixture.news = vec![news("n-tsla", NEWSWIRE, "TSLA", CUT_OFF - 60)];
    fixture.filings = vec![filing("f-tsla", "TSLA", CUT_OFF - 60)];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert!(retrieval.items.is_empty());
    assert_eq!(
        dropped(&retrieval, DropReason::OutsideScope),
        vec!["f-tsla", "n-tsla"]
    );
}

#[test]
fn filings_cover_held_instruments_outside_the_data_universe() {
    let mut fixture = Fixture::default();
    fixture.filings = vec![filing("f-held", "TSLA", CUT_OFF - 60)];
    let mut held_scope = scope();
    held_scope.held.insert(symbol("TSLA"));
    let retrieval =
        retrieve(&plan(), &held_scope, fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::Filing), vec!["f-held"]);
}

#[test]
fn a_duplicate_item_id_is_kept_once() {
    let mut fixture = Fixture::default();
    fixture.news = vec![
        news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60),
        news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 30),
    ];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::News), vec!["n-1"]);
    assert_eq!(dropped(&retrieval, DropReason::Duplicate), vec!["n-1"]);
}

#[test]
fn a_failed_read_is_an_empty_read() {
    let mut fixture = Fixture::default();
    fixture.news = vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)];
    fixture.failing = true;
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert!(retrieval.items.is_empty());
    assert!(fixture.calls.get() > 0, "the readers were asked");
}

#[test]
fn text_queries_take_their_scope_from_the_harness() {
    let fixture = Fixture::default();
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(retrieval.cut_off, common::at(CUT_OFF));
    let queries = fixture.queries.borrow();
    assert_eq!(queries.len(), 2, "one news query and one filings query");
    for query in queries.iter() {
        assert_eq!(query.workspace, "ws1");
        assert_eq!(query.cut_off, common::at(CUT_OFF));
        assert_eq!(query.allowlist_version, 3);
        assert!(query.symbols.contains(&symbol("AAPL")));
    }
    assert_eq!(queries[0].window_start, common::at(CUT_OFF - 2 * DAY));
    assert_eq!(queries[0].max_items, 50);
    assert_eq!(queries[1].window_start, common::at(CUT_OFF - 7 * DAY));
    assert_eq!(queries[1].max_items, 20);
}

#[test]
fn items_come_out_in_one_fixed_order_and_the_same_reads_give_the_same_retrieval() {
    let mut fixture = Fixture::default();
    fixture.instruments = vec![instrument("MSFT", MSFT_ID), instrument("AAPL", AAPL_ID)];
    fixture.bars = [bars("MSFT", 2), bars("AAPL", 2)].concat();
    fixture.news = vec![
        news("n-b", NEWSWIRE, "MSFT", CUT_OFF - 60),
        news("n-a", NEWSWIRE, "AAPL", CUT_OFF - 120),
    ];
    fixture.filings = vec![filing("f-1", "AAPL", CUT_OFF - 60)];
    let first = retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    let second = retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(first, second);
    let kinds: Vec<ItemKind> = first.items.iter().map(RetrievedItem::kind).collect();
    assert_eq!(
        kinds,
        vec![
            ItemKind::Instrument,
            ItemKind::Instrument,
            ItemKind::Bar,
            ItemKind::Bar,
            ItemKind::Bar,
            ItemKind::Bar,
            ItemKind::News,
            ItemKind::News,
            ItemKind::Filing,
        ]
    );
    assert_eq!(
        first.items[0].symbol().map(|s| s.as_str()),
        Some("AAPL"),
        "instruments by symbol"
    );
}

#[test]
fn an_instrument_outside_the_scope_is_not_kept() {
    let mut fixture = Fixture::default();
    fixture.instruments = vec![instrument("AAPL", AAPL_ID), instrument("TSLA", MSFT_ID)];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::Instrument).len(), 1);
}

#[test]
fn every_record_is_canonical_json_and_its_digest_is_the_data_ref() {
    let mut fixture = Fixture::default();
    fixture.instruments = vec![instrument("AAPL", AAPL_ID)];
    fixture.bars = bars("AAPL", 1);
    fixture.news = vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)];
    fixture.filings = vec![filing("f-1", "AAPL", CUT_OFF - 60)];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(retrieval.items.len(), 4);
    let mut ids = std::collections::BTreeSet::new();
    for item in &retrieval.items {
        let record = item.record().unwrap_or_else(|e| panic!("{e}"));
        let value = parse(&record).unwrap_or_else(|e| panic!("a record is strict JSON: {e:?}"));
        assert_eq!(
            to_canonical(&value),
            record,
            "the record is already canonical"
        );
        assert_eq!(
            item.data_ref().unwrap_or_else(|e| panic!("{e}")),
            Digest::of(&record)
        );
        assert_eq!(
            value.get("kind").and_then(|v| v.as_str()),
            Some(item.kind().as_str())
        );
        assert!(ids.insert(item.item_id().unwrap_or_else(|e| panic!("{e}"))));
    }
}

#[test]
fn a_news_record_carries_the_drift_facts_and_its_text() {
    let mut fixture = Fixture::default();
    fixture.news = vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    let record = retrieval.items[0]
        .record()
        .unwrap_or_else(|e| panic!("{e}"));
    let value = parse(&record).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(value.get("source").and_then(|v| v.as_str()), Some(NEWSWIRE));
    assert_eq!(
        value.get("content_hash").and_then(|v| v.as_str()),
        Some(format!("sha256:{}", Digest::of(b"n-1")).as_str())
    );
    assert!(value.get("knowledge_at").is_some());
    assert!(value.get("length_bytes").and_then(|v| v.as_int()).is_some());
    assert_eq!(
        value.get("headline").and_then(|v| v.as_str()),
        Some("Headline n-1")
    );
}

#[test]
fn platform_items_have_derived_ids_and_platform_sources() {
    let mut fixture = Fixture::default();
    fixture.instruments = vec![instrument("AAPL", AAPL_ID)];
    fixture.bars = vec![common::bar("AAPL", CUT_OFF - DAY, "100")];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    let ids: Vec<(String, String)> = retrieval
        .items
        .iter()
        .map(|item| {
            (
                item.item_id()
                    .unwrap_or_else(|e| panic!("{e}"))
                    .as_str()
                    .to_owned(),
                item.source()
                    .unwrap_or_else(|e| panic!("{e}"))
                    .as_str()
                    .to_owned(),
            )
        })
        .collect();
    assert_eq!(
        ids,
        vec![
            (
                format!("instrument:{AAPL_ID}"),
                "platform.instruments".to_owned()
            ),
            (
                format!("bar:AAPL:{}", CUT_OFF - DAY),
                "platform.bars".to_owned()
            ),
        ]
    );
    let _ = EDGAR;
}

#[test]
fn a_link_and_a_form_are_cut_to_their_caps() {
    let mut fixture = Fixture::default();
    let mut long = news("n-link", NEWSWIRE, "AAPL", CUT_OFF - 60);
    long.link = "l".repeat(600);
    fixture.news = vec![long];
    let mut filed = filing("f-form", "AAPL", CUT_OFF - 60);
    filed.form = "F".repeat(40);
    fixture.filings = vec![filed];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    for item in &retrieval.items {
        match item {
            RetrievedItem::News(kept) => assert_eq!(kept.link.len(), 512),
            RetrievedItem::Filing(kept) => assert_eq!(kept.form.len(), 32),
            _ => {}
        }
    }
    assert_eq!(
        dropped(&retrieval, DropReason::Trimmed),
        vec!["f-form", "n-link"]
    );
}

#[test]
fn a_reader_id_in_the_platform_s_namespace_is_dropped() {
    let mut fixture = Fixture::default();
    fixture.news = vec![news("bar:AAPL:1", NEWSWIRE, "AAPL", CUT_OFF - 60)];
    fixture.filings = vec![filing("instrument:x", "AAPL", CUT_OFF - 60)];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert!(retrieval.items.is_empty());
    assert_eq!(
        dropped(&retrieval, DropReason::ReservedId),
        vec!["bar:AAPL:1", "instrument:x"]
    );
}

#[test]
fn two_bars_in_one_second_are_one_bar() {
    let mut fixture = Fixture::default();
    let first = common::bar("AAPL", CUT_OFF - DAY, "100");
    let mut second = first.clone();
    second.knowledge_at =
        mandate_time::UtcNanos::from_parts(CUT_OFF - DAY, 5).unwrap_or_else(|e| panic!("{e}"));
    fixture.bars = vec![first, second];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::Bar).len(), 1);
    assert_eq!(dropped(&retrieval, DropReason::Duplicate).len(), 1);
}

#[test]
fn a_bar_known_at_the_cut_off_is_kept() {
    let mut fixture = Fixture::default();
    fixture.bars = vec![common::bar("AAPL", CUT_OFF, "100")];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(kept(&retrieval.items, ItemKind::Bar).len(), 1);
}

#[test]
fn text_at_its_cap_is_kept_whole_and_one_byte_more_is_cut() {
    let mut fixture = Fixture::default();
    let mut at_cap = news("n-cap", NEWSWIRE, "AAPL", CUT_OFF - 60);
    at_cap.headline = "h".repeat(1_000);
    at_cap.summary = "s".repeat(1_000);
    at_cap.link = "l".repeat(512);
    let mut over = news("n-over", NEWSWIRE, "AAPL", CUT_OFF - 50);
    over.headline = "h".repeat(1_001);
    over.summary = "s".repeat(1_000);
    fixture.news = vec![at_cap, over];
    let mut excerpt_cap = filing("f-cap", "AAPL", CUT_OFF - 60);
    excerpt_cap.excerpt = "e".repeat(4_000);
    excerpt_cap.form = "F".repeat(32);
    let mut excerpt_over = filing("f-over", "AAPL", CUT_OFF - 50);
    excerpt_over.excerpt = "e".repeat(4_001);
    fixture.filings = vec![excerpt_cap, excerpt_over];
    let retrieval =
        retrieve(&plan(), &scope(), fixture.readers()).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(
        dropped(&retrieval, DropReason::Trimmed),
        vec!["f-over", "n-over"]
    );
    for item in &retrieval.items {
        match item {
            RetrievedItem::Filing(kept) => assert_eq!(kept.excerpt.len(), 4_000),
            RetrievedItem::News(kept) => {
                assert_eq!(kept.headline.len() + kept.summary.len(), 2_000);
            }
            _ => {}
        }
    }
}

#[test]
fn an_item_names_its_one_instrument_and_its_text_length() {
    let mut one = news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60);
    one.headline = "abc".to_owned();
    one.summary = "de".to_owned();
    let mut two = one.clone();
    two.symbols.insert(symbol("MSFT"));
    let one = RetrievedItem::News(one);
    assert_eq!(one.symbol().map(|s| s.as_str()), Some("AAPL"));
    assert_eq!(one.length_bytes(), 5);
    assert_eq!(
        RetrievedItem::News(two).symbol(),
        None,
        "a multi-instrument item names none"
    );
    let mut excerpt = filing("f-1", "AAPL", CUT_OFF - 60);
    excerpt.excerpt = "1234567".to_owned();
    assert_eq!(RetrievedItem::Filing(excerpt).length_bytes(), 7);
    assert_eq!(
        RetrievedItem::Bar(common::bar("AAPL", CUT_OFF, "1")).length_bytes(),
        0
    );
}
