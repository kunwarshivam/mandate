#![allow(
    clippy::unwrap_used,
    clippy::panic,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! A pass's event offers (DEC-2288 item 2): the recorded feed's captured items and the news
//! stream's stored ones merged, each item (its `data_ref`) once, in arrival order, whichever order
//! either source lists it in. The oracle keys every offer by `data_ref` in a map and sorts the
//! survivors itself.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::Digest;
use mandate_daemon::merge_offers;
use mandate_research_run::{ItemId, NewsItem, RetrievedItem, SourceId, Symbol};
use mandate_runtime::{RetrievedObservation, RiskClock};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const STORED_ROW: &[u8] = include_bytes!("../../../fixtures/news/licensed.row.json");
const STORED_RECORD: &[u8] = include_bytes!("../../../fixtures/news/licensed.record.json");

/// A captured feed item `n`, known at `known` seconds.
fn captured(n: u8, known: i64) -> RetrievedObservation {
    let at = UtcNanos::from_parts(known, 0).unwrap();
    let item = RetrievedItem::News(NewsItem {
        item_id: ItemId::new(&format!("nw-{n}")).unwrap(),
        source: SourceId::new("src.newswire").unwrap(),
        symbols: BTreeSet::from([Symbol::new("AAPL").unwrap()]),
        knowledge_at: at,
        published_at: at,
        headline: format!("Headline {n}"),
        summary: String::new(),
        link: String::new(),
        content_hash: Digest::of(&[n]),
        licensed: false,
    });
    RetrievedObservation::capture(&item, RiskClock::from_secs(known))
        .unwrap()
        .0
}

fn oracle(
    feed: &[RetrievedObservation],
    stored: &[RetrievedObservation],
) -> Vec<RetrievedObservation> {
    let mut by_ref = BTreeMap::new();
    for offer in feed.iter().chain(stored) {
        by_ref
            .entry(offer.observation.data_ref)
            .or_insert_with(|| offer.clone());
    }
    let mut offers: Vec<RetrievedObservation> = by_ref.into_values().collect();
    offers.sort_by(|a, b| {
        (
            a.item.knowledge_at,
            a.item.item_id.as_str(),
            a.observation.data_ref,
        )
            .cmp(&(
                b.item.knowledge_at,
                b.item.item_id.as_str(),
                b.observation.data_ref,
            ))
    });
    offers
}

/// An item both sources hold, listed in the opposite order by each, is offered once, in its place.
#[test]
fn an_item_in_both_the_feed_and_the_stream_is_offered_once_in_arrival_order() {
    let (early, late) = (captured(1, 100), captured(2, 300));
    let stored = RetrievedObservation::stored(STORED_ROW, STORED_RECORD).unwrap();
    let stored_at = stored.item.knowledge_at.secs();
    let middle = captured(3, stored_at);
    let expected = vec![early.clone(), late.clone(), middle.clone(), stored.clone()];
    assert!(
        expected
            .windows(2)
            .all(|pair| pair[0].item.knowledge_at <= pair[1].item.knowledge_at)
    );
    for (feed, from_stream) in [
        (
            vec![early.clone(), late.clone(), middle.clone()],
            vec![stored.clone(), late.clone(), early.clone()],
        ),
        (
            vec![middle.clone(), late.clone(), early.clone()],
            vec![early.clone(), late.clone(), stored.clone()],
        ),
    ] {
        assert_eq!(merge_offers(feed, from_stream), expected);
    }
    assert_eq!(merge_offers(Vec::new(), Vec::new()), Vec::new());
}

/// The oracle itself fails on a merge that keeps duplicates or keeps one source's order.
#[test]
fn the_oracle_catches_a_merge_that_keeps_duplicates_or_order() {
    let (a, b) = (captured(1, 200), captured(2, 100));
    let feed = vec![a.clone()];
    let stored = vec![a.clone(), b.clone()];
    let chained: Vec<RetrievedObservation> = feed.iter().chain(&stored).cloned().collect();
    assert_ne!(oracle(&feed, &stored), chained);
    assert_eq!(oracle(&feed, &stored), vec![b, a]);
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Any two lists of offers, repeats and all, merge to the oracle's.
    #[test]
    fn any_two_lists_merge_to_each_item_once_in_arrival_order(
        feed in prop::collection::vec((0u8..6, 0i64..4), 0..8),
        stored in prop::collection::vec((0u8..6, 0i64..4), 0..8),
    ) {
        let offer = |(n, slot): (u8, i64)| captured(n, 1_000 + 60 * slot + i64::from(n % 2));
        let feed: Vec<RetrievedObservation> = feed.into_iter().map(offer).collect();
        let stored: Vec<RetrievedObservation> = stored.into_iter().map(offer).collect();
        prop_assert_eq!(merge_offers(feed.clone(), stored.clone()), oracle(&feed, &stored));
    }
}
