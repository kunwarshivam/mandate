#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The stored news items the agent runtime reads (DEC-2288): `fixtures/news/` holds a licensed and
//! an unlicensed item as this reader stores them, each its `NewsItemStored` row's body, which names
//! the item's `item_ref`, and the typed record's bytes that `item_ref` names. The runtime's tests
//! read those files and never this crate, so the two crates share bytes and no layering edge, and
//! this test fails the moment the reader's writer stores anything else.

mod common;

use mandate_canon::parse;
use mandate_journal::{ArtifactRef, StoredEvent, get_artifact};
use mandate_process::State;
use mandate_sources::allowlist::Allowlist;

use common::*;

const LICENSED_ROW: &[u8] = include_bytes!("../../../fixtures/news/licensed.row.json");
const LICENSED_RECORD: &[u8] = include_bytes!("../../../fixtures/news/licensed.record.json");
const UNLICENSED_ROW: &[u8] = include_bytes!("../../../fixtures/news/unlicensed.row.json");
const UNLICENSED_RECORD: &[u8] = include_bytes!("../../../fixtures/news/unlicensed.record.json");

/// The one item the reader stores in each case: SPY's, from the followed agent's thesis.
fn the_item() -> String {
    item(
        41000003,
        "2026-10-26T13:00:00Z",
        "2026-10-26T13:05:00Z",
        &["SPY"],
    )
}

/// The row and the record the reader stores for [`the_item`], under an allowlist whose news source
/// is licensed or not.
fn stored_as(licensed: bool) -> (StoredEvent, Vec<u8>) {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let mut config = config();
    if !licensed {
        let unlicensed = ALLOWLIST.replacen("licensed = true", "licensed = false", 1);
        config.allowlist = Allowlist::parse(&unlicensed).unwrap();
    }
    let transport = Recorded::answering(&[page(&[the_item()], None)]);
    let mut reader = started_with(&journal, &shelf, &transport, &config, t0());
    assert_eq!(reader.lifecycle().state(), State::Ready);
    block_on(reader.step(t0()));
    let rows = stored(&journal);
    assert_eq!(rows.len(), 1, "one item stored");
    let row = rows[0].clone();
    let reference = ArtifactRef::parse(member(&row, "item_ref").as_str().unwrap()).unwrap();
    let record = get_artifact(&*shelf.objects.borrow(), &reference).unwrap();
    (row, record)
}

#[test]
fn the_reader_stores_exactly_the_fixture_s_rows_and_records() {
    for (licensed, row_file, record_file) in [
        (true, LICENSED_ROW, LICENSED_RECORD),
        (false, UNLICENSED_ROW, UNLICENSED_RECORD),
    ] {
        let (row, record) = stored_as(licensed);
        assert_eq!(
            String::from_utf8_lossy(&row.body),
            String::from_utf8_lossy(row_file),
            "licensed {licensed}: the row's body"
        );
        assert_eq!(
            String::from_utf8_lossy(&record),
            String::from_utf8_lossy(record_file),
            "licensed {licensed}: the record's bytes"
        );
        assert_eq!(
            member(&row, "licensed"),
            mandate_canon::Value::Bool(licensed),
            "the fixture's two cases differ in licensing"
        );
        assert!(parse(record_file).is_ok(), "the record is canonical JSON");
    }
}
