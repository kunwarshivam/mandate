//! E12-6 slice A1: journal pages (workspace API §4.8.1, API-15, AU-2, DEC-760 items 1 to 3). Every
//! oracle here reads what the fixture appended and recomputes hashes and links itself; none calls
//! the code under test to decide what a page should have held.

mod common;

use common::{Fixture, T, WS_A, acct, limit, tenant, text, workspaces};
use mandate_audit::{AuditError, Head, JournalEvent, JournalRead, MemoryRead, PageLimit};
use mandate_canon::{Digest, parse};
use mandate_journal::StreamId;
use proptest::prelude::*;

/// The largest canonical integer, 2^53 − 1 (journal spec §4 rule 4).
const MAX_SEQ: u64 = 9_007_199_254_740_991;

/// The head the journal's own rows give for `stream`: the last row's `seq`, the SHA-256 of its body,
/// and its `recorded_at`.
fn head_of(fx: &Fixture, stream: &str) -> Head {
    let rows = fx.journal.rows(&StreamId::parse(stream).unwrap());
    let last = rows.last().unwrap();
    Head {
        seq: rows.len() as u64,
        hash: Digest::of(&last.body),
        recorded_at: T.to_owned(),
    }
}

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
        assert_eq!(
            body.get("event_type").and_then(|v| v.as_str()),
            Some(event.event_type.as_str())
        );
        assert_eq!(
            body.get("recorded_at").and_then(|v| v.as_str()),
            Some(event.recorded_at.as_str())
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
        AuditError::LimitOutOfRange { limit: 1001 }.to_string(),
        "page limit 1001 is outside 1..=1000"
    );
    assert_eq!(
        AuditError::AfterSeqOutOfRange {
            after_seq: MAX_SEQ + 1
        }
        .to_string(),
        "after_seq 9007199254740992 is outside 0..=9007199254740991"
    );
}

#[test]
fn a_page_serves_the_events_after_the_cursor_in_seq_order_with_the_head() {
    let fx = Fixture::new(5);
    let stream = acct(&text(WS_A), "ACCT1");
    let read = MemoryRead::new(&fx.journal);
    let page = read.page(&tenant(WS_A), &stream, 2, limit(3)).unwrap();
    assert_eq!(page.stream_id, stream);
    assert_eq!(
        page.head,
        head_of(&fx, &stream),
        "the head read with the page"
    );
    let ids: Vec<&str> = page.events.iter().map(|e| e.event_id.as_str()).collect();
    assert_eq!(ids, &fx.appended[&stream].event_ids[2..5]);
    assert_eq!(
        page.events.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [3, 4, 5]
    );
    assert_eq!(page.next_after_seq, 5, "the cursor is the last seq served");
    assert!(!page.at_head, "seq 6 is still to read");
    let rest = read.page(&tenant(WS_A), &stream, 5, limit(3)).unwrap();
    assert_eq!(rest.events.iter().map(|e| e.seq).collect::<Vec<_>>(), [6]);
    assert!(rest.at_head);
    let whole = read.page(&tenant(WS_A), &stream, 0, limit(1000)).unwrap();
    assert_eq!(whole.events.len(), 6);
    assert!(whole.at_head);
    assert_eq!(&whole.events[2..5], page.events.as_slice());
    assert_eq!(whole.events[0].event_type, "StreamOpened");
    assert_eq!(whole.events[1].event_type, "MarkUpdated");
    assert_chain(&stream, &whole.events);
}

#[test]
fn the_default_limit_is_one_hundred_within_one_to_one_thousand() {
    let mut fx = Fixture::new(0);
    let stream = acct(&text(WS_A), "ACCT1");
    fx.marks(&stream, 1100);
    let read = MemoryRead::new(&fx.journal);
    let page = |l: PageLimit| {
        read.page(&tenant(WS_A), &stream, 0, l)
            .unwrap()
            .events
            .len()
    };
    assert_eq!(page(PageLimit::new(None).unwrap()), 100);
    assert_eq!(page(limit(1)), 1);
    assert_eq!(page(limit(1000)), 1000);
    assert_eq!(page(limit(37)), 37);
    for out in [0, 1001, u64::MAX] {
        assert_eq!(
            PageLimit::new(Some(out)),
            Err(AuditError::LimitOutOfRange { limit: out }),
            "a limit outside 1..=1000 is refused, never clamped"
        );
    }
}

#[test]
fn a_cursor_at_or_past_the_head_gives_an_empty_page_at_the_head() {
    let fx = Fixture::new(3);
    let stream = acct(&text(WS_A), "ACCT2");
    let read = MemoryRead::new(&fx.journal);
    for after in [4, 5, 1000, MAX_SEQ] {
        let page = read.page(&tenant(WS_A), &stream, after, limit(10)).unwrap();
        assert!(page.events.is_empty(), "after {after}");
        assert_eq!(page.next_after_seq, after);
        assert!(page.at_head);
        assert_eq!(page.head, head_of(&fx, &stream));
    }
    let last = read.page(&tenant(WS_A), &stream, 3, limit(10)).unwrap();
    assert_eq!(last.events.iter().map(|e| e.seq).collect::<Vec<_>>(), [4]);
    assert_eq!(last.next_after_seq, 4);
    assert!(last.at_head);
    for after in [MAX_SEQ + 1, u64::MAX] {
        assert_eq!(
            read.page(&tenant(WS_A), &stream, after, limit(10)),
            Err(AuditError::AfterSeqOutOfRange { after_seq: after }),
            "an after_seq above 2^53 - 1 is refused"
        );
    }
}

/// The streams the fuzz appends to, each an index into [`workspaces`] and an account: the one it
/// pages, of [`WS_A`], a sibling in the same workspace, and two of other workspaces, one whose
/// segment has the paged workspace's as a prefix.
const FUZZ_STREAMS: [(usize, &str); 4] = [(0, "ACCT1"), (0, "ACCT2"), (1, "ACCT1"), (2, "ACCT1")];

#[derive(Debug, Clone)]
enum Op {
    Append { stream: usize, count: u64 },
    Read { limit: u64 },
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        3 => (0..FUZZ_STREAMS.len(), 1..=3u64).prop_map(|(stream, count)| Op::Append { stream, count }),
        4 => (1..=8u64).prop_map(|limit| Op::Read { limit }),
        1 => (1..=1000u64).prop_map(|limit| Op::Read { limit }),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// E12-6's acceptance fuzz (API-15, AU-2): random page sizes, with appends to the paged stream
    /// and to others between page reads. Every page holds as many events as were there to serve,
    /// up to its limit, with the head as the journal's rows give it at that read; the concatenated
    /// pages are the stream's appended events exactly once in `seq` order, and the chain check
    /// passes across page boundaries from 64 zeros.
    #[test]
    fn paging_under_concurrent_appends_serves_the_stream_once_in_order_and_chains(
        initial in 0..12u64,
        ops in prop::collection::vec(op(), 1..40),
    ) {
        let mut fx = Fixture::new(initial);
        let segments = workspaces();
        let (workspace, account) = FUZZ_STREAMS[0];
        let target = acct(&segments[workspace], account);
        let me = tenant(WS_A);
        let mut cursor = 0u64;
        let mut served: Vec<JournalEvent> = Vec::new();
        let read_page = |fx: &Fixture, cursor: &mut u64, served: &mut Vec<JournalEvent>, l: u64| {
            let available = fx.appended[&target].event_ids.len() as u64 - *cursor;
            let page = MemoryRead::new(&fx.journal)
                .page(&me, &target, *cursor, limit(l))
                .unwrap();
            assert_eq!(page.events.len() as u64, available.min(l), "after {cursor}, limit {l}");
            let first = *cursor + 1;
            assert!(page.events.iter().map(|e| e.seq).eq(first..first + page.events.len() as u64));
            let next = page.events.last().map_or(*cursor, |e| e.seq);
            assert_eq!(page.next_after_seq, next);
            assert_eq!(page.head, head_of(fx, &target), "the head read with the page");
            assert_eq!(page.at_head, next == fx.appended[&target].event_ids.len() as u64);
            *cursor = page.next_after_seq;
            served.extend(page.events);
        };
        for op in ops {
            match op {
                Op::Append { stream, count } => {
                    let (w, a) = FUZZ_STREAMS[stream];
                    fx.marks(&acct(&segments[w], a), count);
                }
                Op::Read { limit } => read_page(&fx, &mut cursor, &mut served, limit),
            }
        }
        let total = fx.appended[&target].event_ids.len();
        while served.len() < total {
            read_page(&fx, &mut cursor, &mut served, 1000);
        }
        read_page(&fx, &mut cursor, &mut served, 1000);
        let ids: Vec<&String> = served.iter().map(|e| &e.event_id).collect();
        let expected: Vec<&String> = fx.appended[&target].event_ids.iter().collect();
        prop_assert_eq!(ids, expected);
        assert_chain(&target, &served);
    }
}
