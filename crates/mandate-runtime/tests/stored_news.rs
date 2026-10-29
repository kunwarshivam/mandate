#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! A news item the news reader stored, offered as an event trigger (journal spec §9.37, DEC-2288):
//! the offer is built from the `NewsItemStored` row and the record its `item_ref` names, and takes
//! `item_ref` as its `data_ref` as it is, never rebuilding the item. A record that does not read is
//! `news_record_unreadable`; one that is not the record the row names is `news_record_mismatch`.
//!
//! The rows and records are `fixtures/news/`, as the news reader stores them (its own test pins
//! them), so this crate reads the reader's bytes without depending on its crate. Each expected
//! value is typed here from the fixture's text, not computed by the runtime.

use std::collections::BTreeSet;

use mandate_canon::{Digest, Key, Value, parse, to_canonical};
use mandate_research_run::{ItemKind, Symbol};
use mandate_runtime::{RetrievedObservation, RiskClock, StoredRefusal};
use mandate_time::UtcNanos;

const LICENSED_ROW: &[u8] = include_bytes!("../../../fixtures/news/licensed.row.json");
const LICENSED_RECORD: &[u8] = include_bytes!("../../../fixtures/news/licensed.record.json");
const UNLICENSED_ROW: &[u8] = include_bytes!("../../../fixtures/news/unlicensed.row.json");
const UNLICENSED_RECORD: &[u8] = include_bytes!("../../../fixtures/news/unlicensed.record.json");

/// The fixture's item: its `item_id`, `knowledge_at`, and the content hash both records carry.
const ITEM_ID: &str = "41000003:2026-10-26T13:05:00.000000000Z";
const KNOWN: &str = "2026-10-26T15:00:00Z";
const CONTENT: &str = "66a1d129949747a7c8d58a470d6c8faf0228725449fda820e3502cc5735d4cff";
const LICENSED_REF: &str =
    "sha256:bbf39bbd6017fb65b0b82df4d4c25ec382d7072bcc2ea2b6f30ed7eb7792f90b";

fn key(name: &str) -> Key {
    Key::new(name).unwrap()
}

/// `bytes` parsed, with `edit` applied to the object at `path` (empty for the top level).
fn edited(
    bytes: &[u8],
    path: &[&str],
    edit: impl FnOnce(&mut std::collections::BTreeMap<Key, Value>),
) -> Vec<u8> {
    let mut value = parse(bytes).unwrap();
    let mut at = &mut value;
    for name in path {
        let Value::Object(members) = at else {
            panic!("{name}: an object")
        };
        at = members.get_mut(&key(name)).unwrap();
    }
    let Value::Object(members) = at else {
        panic!("an object")
    };
    edit(members);
    to_canonical(&value)
}

/// `record` with member `name` set to `value`, and `row` re-pointed at the edited record, so the
/// digest holds and only the member differs.
fn record_with(row: &[u8], record: &[u8], name: &str, value: Value) -> (Vec<u8>, Vec<u8>) {
    let record = edited(record, &[], |members| {
        members.insert(key(name), value);
    });
    let row = repointed(row, &record);
    (row, record)
}

/// `row` with its `item_ref` naming `record`'s bytes.
fn repointed(row: &[u8], record: &[u8]) -> Vec<u8> {
    let reference = format!("sha256:{}", Digest::of(record).to_hex());
    edited(row, &["payload"], |payload| {
        payload.insert(key("item_ref"), Value::Str(reference));
    })
}

fn text(value: &str) -> Value {
    Value::Str(value.to_owned())
}

#[test]
fn a_stored_item_s_offer_names_its_item_ref_as_it_is_and_its_facts_from_the_record() {
    for (row, record, licensed) in [
        (LICENSED_ROW, LICENSED_RECORD, true),
        (UNLICENSED_ROW, UNLICENSED_RECORD, false),
    ] {
        let offer = RetrievedObservation::stored(row, record)
            .unwrap_or_else(|refused| panic!("licensed {licensed}: {refused:?}"));
        let item_ref = parse(row)
            .unwrap()
            .get("payload")
            .and_then(|payload| payload.get("item_ref"))
            .and_then(Value::as_str)
            .unwrap()
            .to_owned();
        assert_eq!(
            format!("sha256:{}", offer.observation.data_ref.to_hex()),
            item_ref,
            "licensed {licensed}: data_ref is item_ref"
        );
        if licensed {
            assert_eq!(item_ref, LICENSED_REF);
        }
        assert_eq!(offer.observation.source, "alpaca.news");
        assert_eq!(
            offer
                .observation
                .instrument_id
                .as_ref()
                .map(|id| id.as_str()),
            Some("SPY"),
            "an item about one ticker names it, as a captured one does"
        );
        let known = UtcNanos::parse_rfc3339(KNOWN).unwrap();
        assert_eq!(offer.observation.as_of, RiskClock::from_secs(known.secs()));
        let facts = &offer.item;
        assert_eq!(facts.item_id.as_str(), ITEM_ID);
        assert_eq!(facts.source.as_str(), "alpaca.news");
        assert_eq!(facts.kind, ItemKind::News);
        assert_eq!(facts.symbols, BTreeSet::from([Symbol::new("SPY").unwrap()]));
        assert_eq!(facts.instrument, None);
        assert_eq!(facts.knowledge_at, known);
        assert_eq!(facts.content.map(|c| c.to_hex()), Some(CONTENT.to_owned()));
        assert_eq!(facts.length_bytes, 34, "the record's length, text or none");
    }
}

/// DEC-2288 item 1: the offer carries no text, licensed or not, and a licensed record that holds
/// any is not the record the reader stores.
#[test]
fn no_offer_carries_text_and_a_licensed_record_holding_text_is_a_mismatch() {
    for (row, record) in [
        (LICENSED_ROW, LICENSED_RECORD),
        (UNLICENSED_ROW, UNLICENSED_RECORD),
    ] {
        let offer = format!("{:?}", RetrievedObservation::stored(row, record).unwrap());
        for held in ["Headline", "Summary", "news.example"] {
            assert!(!offer.contains(held), "{held} in {offer}");
        }
    }
    for member in ["headline", "summary", "link"] {
        let (row, record) = record_with(LICENSED_ROW, LICENSED_RECORD, member, text("leaked"));
        assert_eq!(
            RetrievedObservation::stored(&row, &record),
            Err(StoredRefusal::Mismatch),
            "{member}"
        );
    }
}

#[test]
fn a_record_that_is_not_the_one_its_row_names_is_a_mismatch() {
    assert_eq!(
        RetrievedObservation::stored(LICENSED_ROW, UNLICENSED_RECORD),
        Err(StoredRefusal::Mismatch),
        "bytes that do not hash to item_ref"
    );
    let cases = [
        ("item_id", text("41000003:2026-10-26T13:06:00.000000000Z")),
        ("source", text("sec.edgar")),
        ("symbols", Value::Array(vec![text("QQQ")])),
        ("symbols", Value::Array(vec![text("QQQ"), text("SPY")])),
        ("knowledge_at", text("2026-10-26T15:00:01.000000000Z")),
        ("published_at", text("2026-10-26T13:00:01.000000000Z")),
        ("licensed", Value::Bool(false)),
        ("kind", text("filing")),
    ];
    for (name, value) in cases {
        let (row, record) = record_with(LICENSED_ROW, LICENSED_RECORD, name, value.clone());
        assert_eq!(
            RetrievedObservation::stored(&row, &record),
            Err(StoredRefusal::Mismatch),
            "{name} {value:?}"
        );
    }
}

#[test]
fn a_row_or_record_that_does_not_read_is_unreadable() {
    let not_json = b"{not canonical".to_vec();
    assert_eq!(
        RetrievedObservation::stored(&not_json, LICENSED_RECORD),
        Err(StoredRefusal::Unreadable),
        "a row that does not parse"
    );
    assert_eq!(
        RetrievedObservation::stored(&repointed(LICENSED_ROW, &not_json), &not_json),
        Err(StoredRefusal::Unreadable),
        "a record that does not parse, though its bytes hash to item_ref"
    );
    let other_type = edited(LICENSED_ROW, &[], |members| {
        members.insert(key("event_type"), text("StreamOpened"));
    });
    assert_eq!(
        RetrievedObservation::stored(&other_type, LICENSED_RECORD),
        Err(StoredRefusal::Unreadable),
        "a row of another type"
    );
    for (name, value) in [
        ("item_ref", None),
        ("item_ref", Some(text("sha256:nothex"))),
        ("item_id", None),
        ("source", None),
        ("symbols", Some(text("SPY"))),
        ("knowledge_at", Some(text("yesterday"))),
        ("published_at", None),
        ("licensed", Some(text("true"))),
    ] {
        let row = edited(LICENSED_ROW, &["payload"], |payload| match value.clone() {
            Some(value) => {
                payload.insert(key(name), value);
            }
            None => {
                payload.remove(&key(name));
            }
        });
        assert_eq!(
            RetrievedObservation::stored(&row, LICENSED_RECORD),
            Err(StoredRefusal::Unreadable),
            "the row's {name} {value:?}"
        );
    }
    for (name, value) in [
        ("content_hash", None),
        ("content_hash", Some(text("sha256:nothex"))),
        ("length_bytes", Some(text("34"))),
        ("length_bytes", None),
        ("item_id", None),
        ("symbols", Some(Value::Array(vec![Value::Bool(true)]))),
    ] {
        let record = edited(LICENSED_RECORD, &[], |members| match value.clone() {
            Some(value) => {
                members.insert(key(name), value);
            }
            None => {
                members.remove(&key(name));
            }
        });
        assert_eq!(
            RetrievedObservation::stored(&repointed(LICENSED_ROW, &record), &record),
            Err(StoredRefusal::Unreadable),
            "the record's {name} {value:?}"
        );
    }
}

#[test]
fn the_refusals_have_stable_codes() {
    assert_eq!(StoredRefusal::Unreadable.code(), "news_record_unreadable");
    assert_eq!(StoredRefusal::Mismatch.code(), "news_record_mismatch");
}

/// An item about several tickers names no single instrument, as a captured one does: the core
/// journals the ticker the matched thesis holds.
#[test]
fn an_item_about_several_tickers_names_no_instrument() {
    let two = Value::Array(vec![text("QQQ"), text("SPY")]);
    let record = edited(LICENSED_RECORD, &[], |members| {
        members.insert(key("symbols"), two.clone());
    });
    let row = edited(&repointed(LICENSED_ROW, &record), &["payload"], |payload| {
        payload.insert(key("symbols"), two.clone());
    });
    let offer = RetrievedObservation::stored(&row, &record).unwrap();
    assert_eq!(offer.observation.instrument_id, None);
    assert_eq!(
        offer.item.symbols,
        BTreeSet::from([Symbol::new("QQQ").unwrap(), Symbol::new("SPY").unwrap()])
    );
}
