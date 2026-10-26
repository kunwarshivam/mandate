#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The reader's invariants under random fetches, repeats, vendor updates, and restarts with a host
//! clock that may step back (journal spec §9.37 rules 284 and 285, DEC-2280 items 3 and 5):
//!
//! - every item version served with a ticker is stored exactly once, whatever repeats it, and
//!   nothing else is stored;
//! - each record's `knowledge_at` is the latest of its first receipt, its publication, its
//!   update, and the record before it, so the stream's known times never go back.
//!
//! The oracle keeps its own ledger of what was served and when, from the generated pages alone.

mod common;

use std::collections::BTreeMap;

use mandate_canon::Value;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use proptest::test_runner::{Config, TestCaseError, TestRunner};

use common::*;

/// One served item: its vendor id, and whether it is the vendor's later version.
#[derive(Debug, Clone, Copy)]
struct Served {
    id: u64,
    updated: bool,
}

/// One step: the gap since the last, whether the reader restarts first, how far the restarted
/// host clock stands behind, and the page it is served.
#[derive(Debug, Clone)]
struct Step {
    gap_s: i64,
    restart: bool,
    behind_s: i64,
    items: Vec<Served>,
}

fn created(id: u64) -> UtcNanos {
    plus(t0(), -7_200 + i64::try_from(id).unwrap() * 60)
}

fn updated(served: Served) -> UtcNanos {
    plus(created(served.id), if served.updated { 30 } else { 0 })
}

fn steps() -> impl Strategy<Value = Vec<Step>> {
    let served = (1_u64..=6, any::<bool>()).prop_map(|(id, updated)| Served { id, updated });
    let step = (
        1_i64..=120,
        prop::bool::weighted(0.2),
        0_i64..=90,
        prop::collection::vec(served, 0..5),
    )
        .prop_map(|(gap_s, restart, behind_s, items)| Step {
            gap_s,
            restart,
            behind_s,
            items,
        });
    prop::collection::vec(step, 1..12)
}

fn item_id(served: Served) -> String {
    format!("{}:{}", served.id, updated(served))
}

/// One generated sequence of steps, checked against the ledger.
fn holds(steps: &[Step]) -> Result<(), TestCaseError> {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let transport = Recorded::default();
    let mut now = t0();
    let mut reader = started(&journal, &shelf, &transport, now);
    let mut first_receipt: BTreeMap<String, UtcNanos> = BTreeMap::new();
    let mut times: BTreeMap<String, (UtcNanos, UtcNanos)> = BTreeMap::new();
    for step in steps {
        now = plus(now, step.gap_s);
        if step.restart {
            now = plus(now, -step.behind_s);
            reader = started(&journal, &shelf, &transport, now);
        }
        let page_items: Vec<String> = step
            .items
            .iter()
            .map(|s| {
                item(
                    s.id,
                    &created(s.id).to_string(),
                    &updated(*s).to_string(),
                    &["SPY"],
                )
            })
            .collect();
        transport.answer(page(&page_items, None));
        block_on(reader.step(now));
        for served in &step.items {
            first_receipt.entry(item_id(*served)).or_insert(now);
            times.insert(item_id(*served), (created(served.id), updated(*served)));
        }
    }
    let rows: Vec<_> = journal
        .news()
        .into_iter()
        .filter(|row| row.event_type == "NewsItemStored")
        .collect();
    let stored: Vec<String> = rows
        .iter()
        .map(|row| member(row, "item_id").as_str().unwrap().to_owned())
        .collect();
    let mut sorted = stored.clone();
    sorted.sort();
    sorted.dedup();
    prop_assert_eq!(
        sorted.len(),
        stored.len(),
        "no version stored twice: {:?}",
        stored
    );
    let served: Vec<String> = first_receipt.keys().cloned().collect();
    prop_assert_eq!(
        sorted,
        served,
        "every version served is stored, and nothing else"
    );
    let mut last: Option<UtcNanos> = None;
    for (row, id) in rows.iter().zip(&stored) {
        let (published, revised) = times[id];
        let expected = [
            Some(first_receipt[id]),
            Some(published),
            Some(revised),
            last,
        ]
        .into_iter()
        .flatten()
        .max()
        .unwrap();
        let Value::Str(known) = member(row, "knowledge_at") else {
            panic!("knowledge_at is a timestamp")
        };
        prop_assert_eq!(UtcNanos::parse(&known).unwrap(), expected, "{}", id);
        last = Some(expected);
    }

    Ok(())
}

#[test]
fn each_version_is_stored_once_and_known_times_never_go_back() {
    let mut runner = TestRunner::new(Config {
        cases: 96,
        failure_persistence: None,
        ..Config::default()
    });
    if let Err(failure) = runner.run(&steps(), |steps| holds(&steps)) {
        panic!("{failure}");
    }
}
