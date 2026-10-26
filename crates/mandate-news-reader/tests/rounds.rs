#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The news reader's rounds end to end (DEC-2278 item 2, DEC-2280 item 1): what it asks follows
//! the theses and holdings as they change; an instrument with no ticker, a stream it cannot read,
//! and a failed request are counted and paused on; more tickers than one request takes are
//! chunked; and an endless continuation stops at the page cap. The oracle is each test's own
//! reading of the requests sent and the rows stored.

mod common;

use std::collections::BTreeSet;

use mandate_journal::ArtifactRef;
use mandate_news_reader::poll::MAX_PAGES;
use mandate_news_reader::{Config, Follow};
use mandate_process::State;
use mandate_sources::fetch::MAX_NEWS_SYMBOLS;

use common::*;

#[test]
fn it_polls_the_active_theses_and_held_instruments_and_drops_a_thesis_that_ends() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    register(&journal, &shelf, SPY, "SPY");
    register(&journal, &shelf, QQQ, "QQQ");
    register(&journal, &shelf, AAPL, "AAPL");
    thesis(&journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
    thesis(&journal, "01THESISAAPL", AAPL, "2026-10-26T15:00:30Z");
    bought(&journal, "o1", QQQ, "2");
    let transport = Recorded::default();
    for _ in 0..3 {
        transport.answer(page(&[], None));
    }
    let mut reader = started(&journal, &shelf, &transport, t0());
    block_on(reader.step(t0()));
    assert_eq!(
        asked(&transport.sent()[0]),
        "AAPL,QQQ,SPY",
        "each active thesis's and each held instrument's ticker, once"
    );
    block_on(reader.step(plus(t0(), 60)));
    assert_eq!(
        asked(&transport.sent()[1]),
        "QQQ,SPY",
        "a thesis past its expiry drops out of the next poll"
    );
    invalidated(&journal, "01THESISSPY");
    block_on(reader.step(plus(t0(), 61)));
    assert_eq!(
        asked(&transport.sent()[2]),
        "QQQ",
        "an invalidated thesis drops out; the held instrument stays"
    );
    assert!(reader.counts().unreadable_follows.is_empty());
}

#[test]
fn an_instrument_with_no_registered_ticker_is_not_polled_and_is_counted() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    thesis(&journal, "01THESISAAPL", AAPL, "2026-11-30T00:00:00Z");
    thesis(&journal, "01THESISQQQ", QQQ, "2026-11-30T00:00:00Z");
    register(&journal, &shelf, QQQ, "QQQ");
    register(&journal, &shelf, QQQ, "not a ticker");
    register_ref(&journal, &ArtifactRef::of(b"no such artifact"));
    let transport = Recorded::default();
    let mut reader = started(&journal, &shelf, &transport, t0());
    block_on(reader.step(t0()));
    assert!(
        transport.sent().is_empty(),
        "AAPL has no snapshot and QQQ's latest names no ticker, so nothing is asked"
    );
    assert_eq!(
        reader.counts().unregistered,
        BTreeSet::from([AAPL.to_owned(), QQQ.to_owned()])
    );
    assert_eq!(reader.counts().unreadable_snapshots, 1);

    register(&journal, &shelf, QQQ, "QQQ");
    transport.answer(page(&[], None));
    block_on(reader.step(plus(t0(), 30)));
    assert_eq!(
        asked(&transport.sent()[0]),
        "QQQ",
        "a later registration wins"
    );
}

#[test]
fn a_stream_it_cannot_read_polls_nothing_and_a_fold_it_cannot_read_is_counted() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let transport = Recorded::default();
    let mut reader = started(&journal, &shelf, &transport, t0());
    journal.others_down.set(true);
    block_on(reader.step(t0()));
    assert!(transport.sent().is_empty());
    journal.others_down.set(false);
    journal.push(
        &format!("acct:{WS}:{ACCOUNT}"),
        "FillApplied",
        object(&[("fill_id", s("broken"))]),
    );
    transport.answer(page(&[], None));
    block_on(reader.step(plus(t0(), 30)));
    assert_eq!(
        asked(&transport.sent()[0]),
        "SPY",
        "the theses still count when the account stream cannot be folded"
    );
    assert_eq!(
        reader.counts().unreadable_follows,
        BTreeSet::from([AGENT.to_owned()])
    );
}

#[test]
fn a_failed_request_ends_the_round_and_journals_nothing() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let transport = Recorded::default();
    transport
        .answers
        .borrow_mut()
        .push(mandate_sources::fetch::RawResponse {
            status: 500,
            body: Vec::new(),
        });
    let mut reader = started(&journal, &shelf, &transport, t0());
    block_on(reader.step(t0()));
    transport.answer(b"not json".to_vec());
    block_on(reader.step(plus(t0(), 30)));
    assert_eq!(reader.counts().fetch_failed.get("status"), Some(&1));
    assert_eq!(reader.counts().fetch_failed.get("malformed"), Some(&1));
    assert!(journal.news().is_empty());
    assert_eq!(reader.lifecycle().state(), State::Ready);
}

/// MAJOR 1 of #2758's review: more tickers than one request takes, across two follows, are asked
/// in chunks of at most the fetcher's limit, every ticker once, all in one round.
#[test]
fn more_tickers_than_one_request_takes_are_asked_in_chunks() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    let assets: Vec<String> = (0..MAX_NEWS_SYMBOLS + 5)
        .map(|n| format!("00000000-0000-4000-8000-{n:012}"))
        .collect();
    for (n, asset) in assets.iter().enumerate() {
        register(&journal, &shelf, asset, &format!("T{n:03}"));
        let agent = if n % 2 == 0 { AGENT } else { "agent_b" };
        thesis_of(
            &journal,
            agent,
            &format!("01THESIS{n:03}"),
            asset,
            "2026-11-30T00:00:00Z",
        );
    }
    let mut config: Config = config();
    config.follows.push(Follow {
        agent: "agent_b".to_owned(),
        account: "acct_b".to_owned(),
    });
    let transport = Recorded::default();
    for _ in 0..3 {
        transport.answer(page(&[], None));
    }
    let mut reader = started_with(&journal, &shelf, &transport, &config, t0());
    block_on(reader.step(t0()));
    block_on(reader.step(plus(t0(), 1)));
    let sent = transport.sent();
    assert_eq!(sent.len(), 2, "one round, two requests");
    let chunks: Vec<Vec<String>> = sent
        .iter()
        .map(|url| asked(url).split(',').map(str::to_owned).collect())
        .collect();
    assert_eq!(chunks[0].len(), MAX_NEWS_SYMBOLS);
    assert_eq!(chunks[1].len(), 5);
    let all: Vec<String> = chunks.concat();
    let expected: Vec<String> = (0..MAX_NEWS_SYMBOLS + 5)
        .map(|n| format!("T{n:03}"))
        .collect();
    assert_eq!(all, expected, "every ticker once, sorted");
    for url in &sent {
        assert!(
            url.contains("start=2026-10-24T15:00:00Z"),
            "one start for the round: {url}"
        );
    }
    assert!(reader.counts().fetch_failed.is_empty());
    block_on(reader.step(plus(t0(), 2)));
    assert_eq!(
        asked(&transport.sent()[2]),
        chunks[0].join(","),
        "the next round starts over"
    );
}

/// MINOR 4 of #2758's review: a chunk follows at most the page cap's pages, and says when it
/// stopped with one still to follow; the next round starts afresh.
#[test]
fn an_endless_continuation_stops_at_the_page_cap_and_is_counted() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let transport = Recorded::default();
    let pages = usize::try_from(MAX_PAGES).unwrap();
    for n in 0..=pages {
        transport.answer(page(&[], Some(&format!("again{n}"))));
    }
    let mut reader = started(&journal, &shelf, &transport, t0());
    for second in 0..=i64::try_from(pages).unwrap() {
        block_on(reader.step(plus(t0(), second)));
    }
    let sent = transport.sent();
    assert_eq!(sent.len(), pages + 1);
    assert!(
        sent[pages - 1].ends_with(&format!("page_token=again{}", pages - 2)),
        "the cap's last page: {}",
        sent[pages - 1]
    );
    assert!(
        !sent[pages].contains("page_token="),
        "the request after the cap starts a new round at its first page: {}",
        sent[pages]
    );
    assert_eq!(reader.counts().page_caps, 1);
}

/// MINOR 5 of #2758's review: a round with nothing to ask, or a failed one, pauses the reader for
/// the cooldown, so it does not read every stream each second; the pause ends on the clock.
#[test]
fn an_idle_or_failed_round_pauses_the_reader_for_the_cooldown() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    register(&journal, &shelf, SPY, "SPY");
    let transport = Recorded::default();
    let mut reader = started(&journal, &shelf, &transport, t0());
    block_on(reader.step(t0()));
    thesis(&journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
    block_on(reader.step(plus(t0(), 29)));
    assert!(
        transport.sent().is_empty(),
        "paused, it asks nothing though it now has a ticker"
    );
    block_on(reader.step(plus(t0(), 30)));
    assert_eq!(transport.sent().len(), 1, "the pause ends at its cooldown");
    assert_eq!(reader.counts().fetch_failed.get("transport"), Some(&1));
    transport.answer(page(&[], None));
    block_on(reader.step(plus(t0(), 59)));
    assert_eq!(transport.sent().len(), 1, "a failed request pauses it too");
    block_on(reader.step(plus(t0(), 60)));
    assert_eq!(transport.sent().len(), 2);
    assert_eq!(reader.lifecycle().state(), State::Ready);
}
