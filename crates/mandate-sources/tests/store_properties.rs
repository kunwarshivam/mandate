#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::as_conversions,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! DP-1 and DP-2 over random streams of news items, versions, copies, late arrivals, and cut-offs,
//! each against an oracle that keeps its own ledger of what arrived when.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use mandate_sources::SourceError;
use mandate_sources::ingest::alpaca_news_page;
use mandate_sources::record::Capture;
use mandate_sources::store::{Ingested, ItemStore, StoreQuery};
use mandate_time::UtcNanos;
use proptest::prelude::*;

use common::*;

const BASE: i64 = 1_790_000_000;

#[derive(Debug, Clone)]
struct Arrival {
    vendor: u8,
    version_s: u16,
    body: u8,
    received_s: u16,
    capture_backfill: bool,
    ticker: u8,
}

fn arrival() -> impl Strategy<Value = Arrival> {
    (0u8..4, 0u16..400, 0u8..3, 0u16..4_000, any::<bool>(), 0u8..3).prop_map(
        |(vendor, version_s, body, received_s, capture_backfill, ticker)| Arrival {
            vendor,
            version_s,
            body,
            received_s,
            capture_backfill,
            ticker,
        },
    )
}

fn secs(offset: u16) -> UtcNanos {
    UtcNanos::from_parts(BASE + i64::from(offset), 0).unwrap()
}

const TICKERS: [&str; 3] = ["AAPL", "MSFT", "TSLA"];

fn page(a: &Arrival) -> String {
    let time = secs(a.version_s);
    format!(
        r#"{{"news":[{{"id":{},"headline":"story {} body {}","created_at":"{}","updated_at":"{}","summary":"","url":"","symbols":["{}"]}}],"next_page_token":null}}"#,
        a.vendor, a.vendor, a.body, time, time, TICKERS[usize::from(a.ticker)]
    )
}

/// What the oracle saw arrive: a record per accepted arrival, judged by its own rules.
#[derive(Debug, Clone)]
struct Seen {
    vendor: u8,
    version_s: u16,
    knowledge: UtcNanos,
    ticker: u8,
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn dp1_an_as_of_read_returns_exactly_what_was_knowable(
        arrivals in prop::collection::vec(arrival(), 1..40),
        cut_offs in prop::collection::vec(0u16..4_500, 1..6),
        window in 1u16..4_500,
        wanted in prop::collection::btree_set(0u8..3, 1..3),
    ) {
        let tables = Tables::new();
        let source = news_source();
        let mut store = ItemStore::news();
        let mut seen: Vec<Seen> = Vec::new();
        let mut contents: BTreeMap<(u8, u16, u8, u8), ()> = BTreeMap::new();
        let mut latest: BTreeMap<u8, (u16, (u8, u8))> = BTreeMap::new();
        for a in &arrivals {
            let received = secs(a.received_s);
            let capture = if a.capture_backfill { Capture::Backfill } else { Capture::Live };
            let parsed = alpaca_news_page(&tables.ctx(&source, received, capture), page(a).as_bytes()).unwrap();
            let future = a.version_s > a.received_s.saturating_add(1);
            if future {
                prop_assert_eq!(parsed.refused, vec![(0, SourceError::FutureDated)]);
                continue;
            }
            let draft = parsed.drafts.into_iter().next().unwrap();
            let outcome = store.ingest(draft);
            let key = (a.vendor, a.version_s, a.body, a.ticker);
            if contents.contains_key(&key) {
                prop_assert!(matches!(outcome, Ok(Ingested::Duplicate(_))));
                continue;
            }
            match latest.get(&a.vendor) {
                Some((version, _)) if *version >= a.version_s => {
                    prop_assert_eq!(outcome, Err(SourceError::RevisedInPlace));
                    continue;
                }
                _ => {}
            }
            prop_assert!(matches!(outcome, Ok(Ingested::Stored(_))));
            contents.insert(key, ());
            latest.insert(a.vendor, (a.version_s, (a.body, a.ticker)));
            let knowledge = if a.capture_backfill { secs(a.version_s) } else { received };
            seen.push(Seen { vendor: a.vendor, version_s: a.version_s, knowledge, ticker: a.ticker });
        }

        let sources = BTreeSet::from([news_source()]);
        let instruments: BTreeSet<_> = wanted.iter().map(|t| asset([AAPL, MSFT, TSLA][usize::from(*t)])).collect();
        for cut in cut_offs {
            let as_of = secs(cut);
            let since = UtcNanos::from_parts(BASE + i64::from(cut) - i64::from(window), 0).unwrap();
            let mut expected: BTreeMap<u8, &Seen> = BTreeMap::new();
            for s in seen.iter().filter(|s| s.knowledge <= as_of) {
                let newer = expected.get(&s.vendor).is_none_or(|e| e.version_s < s.version_s);
                if newer {
                    expected.insert(s.vendor, s);
                }
            }
            let mut expected: Vec<(u8, u16)> = expected
                .values()
                .filter(|s| s.knowledge > since && wanted.contains(&s.ticker))
                .map(|s| (s.vendor, s.version_s))
                .collect();
            expected.sort();
            let answer = store.as_of(&StoreQuery { as_of, since, sources: &sources, instruments: &instruments, limit: 1_000 });
            prop_assert_eq!(answer.dropped, 0);
            for pair in answer.records.windows(2) {
                prop_assert!(pair[0].typed().knowledge_time >= pair[1].typed().knowledge_time);
            }
            let mut got: Vec<(u8, u16)> = answer
                .records
                .iter()
                .map(|r| {
                    let t = r.typed();
                    (t.vendor_id.as_str().parse().unwrap(), (t.vendor_version.secs() - BASE) as u16)
                })
                .collect();
            got.sort();
            prop_assert_eq!(got, expected);
        }
    }

    #[test]
    fn dp2_a_stored_record_is_never_revised_or_removed(
        arrivals in prop::collection::vec(arrival(), 1..40),
    ) {
        let tables = Tables::new();
        let source = news_source();
        let mut store = ItemStore::news();
        for a in &arrivals {
            let before = store.records().to_vec();
            let received = secs(a.received_s);
            let capture = if a.capture_backfill { Capture::Backfill } else { Capture::Live };
            let parsed = alpaca_news_page(&tables.ctx(&source, received, capture), page(a).as_bytes()).unwrap();
            for draft in parsed.drafts {
                let _ = store.ingest(draft);
            }
            prop_assert!(store.records().len() >= before.len());
            prop_assert_eq!(&store.records()[..before.len()], before.as_slice());
        }
    }

    #[test]
    fn a_limit_keeps_the_newest_and_counts_the_rest(
        count in 1u16..30,
        limit in 0usize..35,
    ) {
        let tables = Tables::new();
        let source = news_source();
        let mut store = ItemStore::news();
        for i in 0..count {
            let body = format!(
                r#"{{"news":[{{"id":{i},"headline":"h{i}","created_at":"{t}","updated_at":"{t}","summary":"","url":"","symbols":["AAPL"]}}],"next_page_token":null}}"#,
                t = secs(i)
            );
            let parsed = alpaca_news_page(&tables.ctx(&source, secs(i), Capture::Live), body.as_bytes()).unwrap();
            store.ingest(parsed.drafts.into_iter().next().unwrap()).unwrap();
        }
        let sources = BTreeSet::from([news_source()]);
        let instruments = BTreeSet::from([asset(AAPL)]);
        let answer = store.as_of(&StoreQuery { as_of: secs(5_000), since: UtcNanos::EPOCH, sources: &sources, instruments: &instruments, limit });
        let kept = limit.min(usize::from(count));
        prop_assert_eq!(answer.records.len(), kept);
        prop_assert_eq!(answer.dropped, usize::from(count) - kept);
        let newest: Vec<String> = (0..count).rev().take(kept).map(|i| i.to_string()).collect();
        let got: Vec<String> = answer.records.iter().map(|r| r.typed().vendor_id.as_str().to_owned()).collect();
        prop_assert_eq!(got, newest);
    }
}

#[test]
fn a_source_outside_the_query_is_never_returned() {
    let tables = Tables::new();
    let source = news_source();
    let mut store = ItemStore::news();
    let body = fixture("recorded/alpaca-news-page.json");
    for draft in alpaca_news_page(&tables.ctx(&source, at("2026-10-15T17:00:00Z"), Capture::Live), &body).unwrap().drafts {
        store.ingest(draft).unwrap();
    }
    let other = BTreeSet::from([edgar_source()]);
    let instruments = BTreeSet::from([asset(AAPL), asset(MSFT)]);
    let answer = store.as_of(&StoreQuery {
        as_of: at("2026-10-16T00:00:00Z"),
        since: UtcNanos::EPOCH,
        sources: &other,
        instruments: &instruments,
        limit: 10,
    });
    assert!(answer.records.is_empty());
    assert_eq!(answer.dropped, 0);
}
