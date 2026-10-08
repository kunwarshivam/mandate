//! E12-6 slice A1: journal pages (workspace API §4.8, API-15, DEC-770 items 1 and 2). Every
//! oracle here reads what the fixture appended and recomputes hashes and links itself; none calls
//! the code under test to decide what a page should have held.

mod common;

use common::{Fixture, acct, limit, ws};
use mandate_audit::{AuditError, JournalEvent, JournalRead, MemoryRead, PageLimit};
use mandate_canon::{Digest, parse};

/// The chain check a client runs over concatenated pages (API-15), from the bytes alone: each
/// body re-hashes to its `hash`, names its own `seq`, `event_id`, stream and `prev_hash`, and
/// links to the event before it, the first to 64 zeros.
fn assert_chain(stream: &str, events: &[JournalEvent]) {
    let mut prev = Digest::ZERO;
    for (i, event) in events.iter().enumerate() {
        let seq = i as u64 + 1;
        assert_eq!(event.seq, seq, "pages skip or repeat a seq");
        assert_eq!(event.stream_id, stream);
        assert_eq!(Digest::of(&event.body), event.hash, "seq {seq} re-hash");
        assert_eq!(
            event.prev_hash, prev,
            "seq {seq} does not join its predecessor"
        );
        let body = parse(&event.body).unwrap();
        assert_eq!(body.get("seq").and_then(|v| v.as_int()), Some(seq));
        assert_eq!(
            body.get("event_id").and_then(|v| v.as_str()),
            Some(event.event_id.as_str())
        );
        assert_eq!(body.get("stream_id").and_then(|v| v.as_str()), Some(stream));
        assert_eq!(
            body.get("prev_hash").and_then(|v| v.as_str()),
            Some(prev.to_hex().as_str())
        );
        prev = event.hash;
    }
}

/// The errors' texts name no stream, event or workspace (API-9, rule 6), so the one `NotFound`
/// reads the same however it arose.
#[test]
fn the_error_texts_name_nothing_they_were_asked_for() {
    assert_eq!(AuditError::NotFound.to_string(), "not found");
    assert_eq!(
        AuditError::LimitOutOfRange { limit: 501 }.to_string(),
        "page limit 501 is outside 1..=500"
    );
}

#[test]
#[ignore = "pending E12-6"]
fn a_page_serves_the_events_after_the_cursor_in_seq_order() {
    let fx = Fixture::new(5);
    let stream = acct("ws_a", "ACCT1");
    let read = MemoryRead::new(&fx.journal);
    let page = read.page(&ws("ws_a"), &stream, 2, limit(3)).unwrap();
    let ids: Vec<&str> = page.events.iter().map(|e| e.event_id.as_str()).collect();
    let expected = &fx.appended[&stream].event_ids[2..5];
    assert_eq!(ids, expected);
    assert_eq!(
        page.events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [3, 4, 5]
    );
    assert_eq!(page.cursor, 5, "the cursor is the last seq served");
    let whole = read.page(&ws("ws_a"), &stream, 0, limit(500)).unwrap();
    assert_eq!(whole.events.len(), 6);
    assert_eq!(&whole.events[2..5], page.events.as_slice());
    assert_chain(&stream, &whole.events);
}

#[test]
#[ignore = "pending E12-6"]
fn the_default_limit_is_one_hundred_within_one_to_five_hundred() {
    let mut fx = Fixture::new(0);
    let stream = acct("ws_a", "ACCT1");
    fx.marks(&stream, 600);
    let read = MemoryRead::new(&fx.journal);
    let page = |l: PageLimit| read.page(&ws("ws_a"), &stream, 0, l).unwrap().events.len();
    assert_eq!(page(PageLimit::new(None).unwrap()), 100);
    assert_eq!(page(limit(1)), 1);
    assert_eq!(page(limit(500)), 500);
    assert_eq!(page(limit(37)), 37);
    for out in [0, 501, u64::MAX] {
        assert_eq!(
            PageLimit::new(Some(out)),
            Err(AuditError::LimitOutOfRange { limit: out }),
            "a limit outside 1..=500 is refused, never clamped"
        );
    }
}

#[test]
#[ignore = "pending E12-6"]
fn a_cursor_at_or_past_the_head_gives_an_empty_page_and_keeps_the_cursor() {
    let fx = Fixture::new(3);
    let stream = acct("ws_a", "ACCT2");
    let read = MemoryRead::new(&fx.journal);
    for after in [4, 5, 1000, u64::MAX] {
        let page = read.page(&ws("ws_a"), &stream, after, limit(10)).unwrap();
        assert!(page.events.is_empty(), "after {after}");
        assert_eq!(page.cursor, after);
    }
    let last = read.page(&ws("ws_a"), &stream, 3, limit(10)).unwrap();
    assert_eq!(last.events.iter().map(|e| e.seq).collect::<Vec<_>>(), [4]);
    assert_eq!(last.cursor, 4);
}
