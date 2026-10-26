#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The news reader on recorded pages (DEC-2278 items 1 to 3, DEC-2280; journal spec §9.37's "What
//! the writer owes"). Every record it writes goes through the real in-memory journal, so rules 283
//! to 286 judge it. The oracle is each test's own reading of the recorded page and the stored rows.

mod common;

use std::collections::BTreeSet;

use mandate_canon::{Digest, Value, parse};
use mandate_journal::{ArtifactRef, StoredEvent, get_artifact};
use mandate_news_reader::{KeyRefusal, NewsReader};
use mandate_process::{Exit, State};
use mandate_sources::fetch::NewsAuth;

use common::*;

fn stored(journal: &Journal) -> Vec<StoredEvent> {
    journal
        .news()
        .into_iter()
        .filter(|row| row.event_type == "NewsItemStored")
        .collect()
}

fn ids(journal: &Journal) -> Vec<String> {
    stored(journal)
        .iter()
        .map(|row| member(row, "item_id").as_str().unwrap().to_owned())
        .collect()
}

/// A workspace whose agent holds a thesis on SPY, with SPY's ticker registered.
fn spy_thesis(journal: &Journal, shelf: &Shelf) {
    register(journal, shelf, SPY, "SPY");
    thesis(journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
}

#[test]
fn with_no_key_or_a_refused_key_it_writes_nothing_fetches_nothing_and_is_not_ready() {
    let cases: Vec<(Result<NewsAuth, KeyRefusal>, &str)> = vec![
        (Err(KeyRefusal::NoKey), "no_key"),
        (Err(KeyRefusal::VaultUnavailable), "vault_unavailable"),
        (
            NewsAuth::from_stored(b"AKLIVEKEY0001:livesecret").map_err(KeyRefusal::Refused),
            "not_paper",
        ),
        (
            NewsAuth::from_stored(b"PK one:two").map_err(KeyRefusal::Refused),
            "news_key_malformed",
        ),
    ];
    for (key, code) in cases {
        let (journal, shelf) = (Journal::default(), Shelf::default());
        spy_thesis(&journal, &shelf);
        let transport = Recorded::answering(&[page(
            &[item(
                1,
                "2026-10-26T14:00:00Z",
                "2026-10-26T14:00:00Z",
                &["SPY"],
            )],
            None,
        )]);
        let mut reader = NewsReader::start(
            journal.clone(),
            shelf.clone(),
            transport.clone(),
            &config(),
            key,
            t0(),
        );
        for second in 0..5 {
            block_on(reader.step(plus(t0(), second)));
        }
        assert_eq!(reader.not_ready(), Some(code));
        assert_ne!(reader.lifecycle().state(), State::Ready, "{code}");
        assert!(
            !matches!(reader.lifecycle().state(), State::Exited(_)),
            "{code}: it never crashes"
        );
        assert!(reader.lifecycle().live(plus(t0(), 5)), "{code}: still live");
        assert!(transport.sent().is_empty(), "{code}: nothing fetched");
        assert!(journal.news().is_empty(), "{code}: nothing journaled");
        reader.drain(plus(t0(), 6));
        assert_eq!(reader.lifecycle().state(), State::Exited(Exit::Drained));
    }
}

#[test]
fn a_source_the_allowlist_does_not_have_in_effect_is_not_ready_and_fetches_nothing() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let mut config = config();
    config.allowlist = mandate_sources::allowlist::Allowlist::parse(&ALLOWLIST.replace(
        "approved_by = \"founder\"\napproved_in = \"DEC-2394\"\n",
        "",
    ))
    .unwrap();
    let transport = Recorded::default();
    let mut reader = NewsReader::start(
        journal.clone(),
        shelf,
        transport.clone(),
        &config,
        Ok(paper_auth()),
        t0(),
    );
    block_on(reader.step(t0()));
    assert_eq!(reader.not_ready(), Some("news_source_not_in_effect"));
    assert!(transport.sent().is_empty());
    assert!(journal.news().is_empty());
}

#[test]
fn each_new_item_is_stored_as_its_typed_record_and_journaled_before_anything_reads_it() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let first = item(
        41000002,
        "2026-10-26T13:00:00Z",
        "2026-10-26T13:05:00.5Z",
        &["SPY"],
    );
    let second = item(
        41000001,
        "2026-10-26T12:00:00Z",
        "2026-10-26T12:00:00Z",
        &["SPY", "AAPL", "SPY"],
    );
    let transport = Recorded::answering(&[page(&[first.clone(), second.clone()], None)]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    assert_eq!(reader.lifecycle().state(), State::Ready);
    block_on(reader.step(t0()));

    assert_eq!(
        transport.sent(),
        [
            "https://data.alpaca.markets/v1beta1/news?symbols=SPY&start=2026-10-24T15:00:00Z&limit=50&sort=desc"
        ],
        "an empty stream asks from 48 hours back, for the followed thesis's ticker"
    );
    assert!(transport.sent.borrow().iter().all(|(_, auth)| *auth));
    let rows = journal.news();
    let types: Vec<&str> = rows.iter().map(|r| r.event_type.as_str()).collect();
    assert_eq!(types, ["StreamOpened", "NewsItemStored", "NewsItemStored"]);
    assert_eq!(member(&rows[0], "stream_type"), Value::Str("news".into()));
    assert_eq!(
        ids(&journal),
        [
            "41000001:2026-10-26T12:00:00.000000000Z",
            "41000002:2026-10-26T13:05:00.500000000Z"
        ],
        "the vendor's id and version, oldest version first"
    );
    let row = &stored(&journal)[0];
    assert_eq!(member(row, "source"), Value::Str("alpaca.news".into()));
    assert_eq!(
        member(row, "allowlist_version"),
        Value::Int(mandate_canon::Int::new(3).unwrap())
    );
    assert_eq!(
        member(row, "symbols"),
        Value::Array(vec![Value::Str("AAPL".into()), Value::Str("SPY".into())]),
        "the vendor's tickers, sorted and unique, the unregistered one kept"
    );
    assert_eq!(
        member(row, "published_at"),
        Value::Str("2026-10-26T12:00:00.000000000Z".into())
    );
    assert_eq!(
        member(row, "knowledge_at"),
        Value::Str("2026-10-26T15:00:00.000000000Z".into())
    );
    assert_eq!(member(row, "licensed"), Value::Bool(true));

    let reference = ArtifactRef::parse(member(row, "item_ref").as_str().unwrap()).unwrap();
    let body = parse(&row.body).unwrap();
    assert_eq!(
        body.get("artifact_refs"),
        Some(&Value::Array(vec![Value::Str(reference.to_string())]))
    );
    assert_eq!(body.get("clock_source"), Some(&Value::Str("local".into())));
    let record = parse(&get_artifact(&*shelf.objects.borrow(), &reference).unwrap()).unwrap();
    let text = |name: &str| record.get(name).cloned().unwrap_or(Value::Null);
    assert_eq!(text("kind"), Value::Str("news".into()));
    assert_eq!(text("item_id"), member(row, "item_id"));
    assert_eq!(text("source"), Value::Str("alpaca.news".into()));
    assert_eq!(text("symbols"), member(row, "symbols"));
    assert_eq!(text("knowledge_at"), member(row, "knowledge_at"));
    assert_eq!(text("published_at"), member(row, "published_at"));
    assert_eq!(text("licensed"), Value::Bool(true));
    for withheld in ["headline", "summary", "link"] {
        assert_eq!(
            text(withheld),
            Value::Null,
            "{withheld}: licensed text is never kept"
        );
    }
    assert_eq!(
        text("content_hash"),
        Value::Str(format!("sha256:{}", Digest::of(second.as_bytes()).to_hex())),
        "the record names the item's raw bytes by digest alone"
    );
    let shelved: Vec<Vec<u8>> = shelf.objects.borrow().values().cloned().collect();
    for bytes in shelved {
        let held = String::from_utf8_lossy(&bytes);
        assert!(
            !held.contains("Headline"),
            "no raw response and no text is kept: {held}"
        );
    }
    assert_eq!(reader.counts().stored, 2);
}

#[test]
fn known_time_is_the_latest_of_receipt_publication_update_and_the_stream_s_last() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let receipt = t0();
    let updated_after_receipt = item(
        7,
        "2026-10-26T14:00:00Z",
        "2026-10-26T15:00:00.75Z",
        &["SPY"],
    );
    let transport = Recorded::answering(&[page(&[updated_after_receipt], None)]);
    let mut reader = started(&journal, &shelf, &transport, receipt);
    block_on(reader.step(receipt));
    assert_eq!(
        member(&stored(&journal)[0], "knowledge_at"),
        Value::Str("2026-10-26T15:00:00.750000000Z".into()),
        "an update within the skew after receipt is the later"
    );

    let earlier_clock = plus(t0(), -60);
    let transport = Recorded::answering(&[page(
        &[item(
            8,
            "2026-10-26T13:00:00Z",
            "2026-10-26T13:00:00Z",
            &["SPY"],
        )],
        None,
    )]);
    let mut restarted = started(&journal, &shelf, &transport, earlier_clock);
    block_on(restarted.step(earlier_clock));
    assert_eq!(
        transport.sent()[0],
        "https://data.alpaca.markets/v1beta1/news?symbols=SPY&start=2026-10-26T15:00:00.750000000Z&limit=50&sort=desc",
        "a stream with items asks from its last known time"
    );
    assert_eq!(
        member(&stored(&journal)[1], "knowledge_at"),
        Value::Str("2026-10-26T15:00:00.750000000Z".into()),
        "a host clock behind the stream's last never puts an item before what was known"
    );
}

#[test]
fn an_item_is_stored_once_across_fetches_restarts_and_failovers() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let one = item(1, "2026-10-26T12:00:00Z", "2026-10-26T12:00:00Z", &["SPY"]);
    let two = item(2, "2026-10-26T12:30:00Z", "2026-10-26T12:30:00Z", &["SPY"]);
    let transport = Recorded::answering(&[
        page(&[one.clone(), one.clone()], None),
        page(&[two.clone(), one.clone()], None),
    ]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    block_on(reader.step(t0()));
    block_on(reader.step(plus(t0(), 1)));
    assert_eq!(
        ids(&journal),
        [
            "1:2026-10-26T12:00:00.000000000Z",
            "2:2026-10-26T12:30:00.000000000Z"
        ]
    );

    let again = Recorded::answering(&[page(&[two.clone(), one.clone()], None)]);
    let mut restarted = started(&journal, &shelf, &again, plus(t0(), 2));
    block_on(restarted.step(plus(t0(), 2)));
    assert_eq!(again.sent().len(), 1, "the restarted reader asked");
    assert_eq!(stored(&journal).len(), 2, "and stored nothing it held");

    let updated = item(1, "2026-10-26T12:00:00Z", "2026-10-26T12:45:00Z", &["SPY"]);
    let failover = Recorded::answering(&[page(&[updated], None)]);
    let mut standby = started(&journal, &shelf, &failover, plus(t0(), 3));
    again.answer(page(
        &[item(
            3,
            "2026-10-26T12:50:00Z",
            "2026-10-26T12:50:00Z",
            &["SPY"],
        )],
        None,
    ));
    block_on(restarted.step(plus(t0(), 3)));
    assert_eq!(
        restarted.lifecycle().state(),
        State::Exited(Exit::Fenced),
        "the standby's start fenced the earlier reader, which then exits on its next append"
    );
    assert_eq!(stored(&journal).len(), 2, "the fenced reader wrote nothing");
    block_on(standby.step(plus(t0(), 3)));
    assert_eq!(
        ids(&journal),
        [
            "1:2026-10-26T12:00:00.000000000Z",
            "2:2026-10-26T12:30:00.000000000Z",
            "1:2026-10-26T12:45:00.000000000Z"
        ],
        "a vendor's later version is a new item, stored by the reader that holds the stream"
    );
}

/// The URL's `symbols` query, as asked.
fn asked(url: &str) -> String {
    url.split("symbols=")
        .nth(1)
        .and_then(|rest| rest.split('&').next())
        .unwrap()
        .to_owned()
}

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
    block_on(reader.step(plus(t0(), 1)));
    assert_eq!(
        asked(&transport.sent()[0]),
        "QQQ",
        "a later registration wins"
    );
}

#[test]
fn a_stream_it_cannot_read_polls_nothing_and_a_fold_it_cannot_read_is_counted() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
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
    block_on(reader.step(plus(t0(), 1)));
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
fn an_item_with_no_ticker_or_one_the_parser_refuses_is_counted_and_never_journaled() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let tickers: Vec<String> = (0..33).map(|n| format!("T{n}")).collect();
    let tickers: Vec<&str> = tickers.iter().map(String::as_str).collect();
    let transport = Recorded::answering(&[page(
        &[
            item(1, "2026-10-26T12:00:00Z", "2026-10-26T12:00:00Z", &[]),
            item(2, "2026-10-26T12:00:00Z", "2026-10-26T12:00:00Z", &tickers),
            item(3, "2026-10-26T15:00:02Z", "2026-10-26T15:00:02Z", &["SPY"]),
            item(4, "2026-10-26T12:00:00Z", "2026-10-26T12:00:00Z", &["SPY"]),
        ],
        None,
    )]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    block_on(reader.step(t0()));
    assert_eq!(ids(&journal), ["4:2026-10-26T12:00:00.000000000Z"]);
    let counts = reader.counts();
    assert_eq!(
        counts.skipped.get("no_ticker"),
        Some(&BTreeSet::from([
            "1:2026-10-26T12:00:00.000000000Z".to_owned()
        ]))
    );
    assert_eq!(counts.refused.get("too_many_tags"), Some(&1));
    assert_eq!(counts.refused.get("future_dated"), Some(&1));
    assert_eq!(counts.stored, 1);
}

#[test]
fn it_follows_the_vendor_s_pages_one_request_a_step_at_the_allowlist_s_pace() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let transport = Recorded::answering(&[
        page(
            &[item(
                2,
                "2026-10-26T13:00:00Z",
                "2026-10-26T13:00:00Z",
                &["SPY"],
            )],
            Some("next-page_1"),
        ),
        page(
            &[item(
                1,
                "2026-10-26T12:00:00Z",
                "2026-10-26T12:00:00Z",
                &["SPY"],
            )],
            None,
        ),
    ]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    block_on(reader.step(t0()));
    block_on(reader.step(t0()));
    assert_eq!(
        transport.sent().len(),
        1,
        "a second request inside the allowlist's interval is not sent"
    );
    block_on(reader.step(plus(t0(), 1)));
    let sent = transport.sent();
    assert_eq!(
        sent[1],
        "https://data.alpaca.markets/v1beta1/news?symbols=SPY&start=2026-10-24T15:00:00Z&limit=50&sort=desc&page_token=next-page_1",
        "the next page of the same round, from the same start"
    );
    assert_eq!(stored(&journal).len(), 2);
}

#[test]
fn an_unanswered_append_is_retried_and_a_refused_one_stops_the_reader() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let transport = Recorded::answering(&[page(
        &[item(
            1,
            "2026-10-26T12:00:00Z",
            "2026-10-26T12:00:00Z",
            &["SPY"],
        )],
        None,
    )]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    journal.appends_down.set(true);
    block_on(reader.step(t0()));
    assert!(journal.news().is_empty());
    assert_eq!(reader.lifecycle().state(), State::Ready);
    journal.appends_down.set(false);
    block_on(reader.step(plus(t0(), 1)));
    assert_eq!(
        ids(&journal),
        ["1:2026-10-26T12:00:00.000000000Z"],
        "the retried batch is committed once, as the same bytes"
    );
    assert_eq!(
        reader.counts().fetch_failed.get("transport"),
        Some(&1),
        "then the step asks again, and the empty transport fails"
    );

    transport.answer(page(
        &[item(
            2,
            "2026-10-26T12:30:00Z",
            "2026-10-26T12:30:00Z",
            &["SPY"],
        )],
        None,
    ));
    journal.appends_refused.set(true);
    block_on(reader.step(plus(t0(), 2)));
    assert_eq!(
        reader.lifecycle().state(),
        State::Exited(Exit::CycleFailed),
        "a refused append stops the reader; the next process folds again"
    );
    transport.answer(page(&[], None));
    block_on(reader.step(plus(t0(), 3)));
    assert_eq!(transport.sent().len(), 3, "an exited reader asks nothing");
}

#[test]
fn an_artifact_store_that_cannot_take_a_record_stops_the_reader_before_it_journals() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
    let transport = Recorded::answering(&[page(
        &[item(
            1,
            "2026-10-26T12:00:00Z",
            "2026-10-26T12:00:00Z",
            &["SPY"],
        )],
        None,
    )]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    shelf.down.set(true);
    block_on(reader.step(t0()));
    assert_eq!(reader.lifecycle().state(), State::Exited(Exit::CycleFailed));
    assert!(
        journal.news().is_empty(),
        "no record names an item not stored"
    );
}

#[test]
fn a_failed_request_ends_the_round_and_journals_nothing() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy_thesis(&journal, &shelf);
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
    block_on(reader.step(plus(t0(), 1)));
    assert_eq!(reader.counts().fetch_failed.get("status"), Some(&1));
    assert_eq!(reader.counts().fetch_failed.get("malformed"), Some(&1));
    assert!(journal.news().is_empty());
    assert_eq!(reader.lifecycle().state(), State::Ready);
}

#[test]
fn each_follow_names_an_agent_and_its_account_and_a_malformed_one_is_refused() {
    let args = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    assert_eq!(
        mandate_news_reader::reader::parse_follows(&args(
            "run --follow a1=acct1 --workspace ws --follow a2=acct2"
        )),
        Ok(vec![
            mandate_news_reader::Follow {
                agent: "a1".to_owned(),
                account: "acct1".to_owned()
            },
            mandate_news_reader::Follow {
                agent: "a2".to_owned(),
                account: "acct2".to_owned()
            },
        ])
    );
    assert_eq!(
        mandate_news_reader::reader::parse_follows(&args("run --workspace ws")),
        Ok(vec![])
    );
    for malformed in [
        "run --follow a1",
        "run --follow =acct",
        "run --follow a1=",
        "run --follow",
    ] {
        assert_eq!(
            mandate_news_reader::reader::parse_follows(&args(malformed)),
            Err("follow_malformed"),
            "{malformed}"
        );
    }
}

#[test]
fn only_a_paper_workspace_s_reader_starts() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let mut config = config();
    config.environment = mandate_journal::Environment::Live;
    let transport = Recorded::default();
    let mut reader = NewsReader::start(
        journal.clone(),
        shelf,
        transport.clone(),
        &config,
        Ok(paper_auth()),
        t0(),
    );
    block_on(reader.step(t0()));
    assert_eq!(
        mandate_news_reader::reader::exit(&reader),
        Some(Exit::Refused)
    );
    assert!(transport.sent().is_empty());
    assert!(journal.news().is_empty());
}

#[test]
fn the_reader_reports_its_exit_and_its_stream_s_last_known_time() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    spy(&journal, &shelf);
    let transport = Recorded::answering(&[page(
        &[item(
            1,
            "2026-10-26T12:00:00Z",
            "2026-10-26T12:00:00Z",
            &["SPY"],
        )],
        None,
    )]);
    let mut reader = started(&journal, &shelf, &transport, t0());
    assert_eq!(reader.last_known(), None);
    assert_eq!(mandate_news_reader::reader::exit(&reader), None);
    block_on(reader.step(t0()));
    assert_eq!(reader.last_known(), Some(t0()));
    assert_eq!(reader.counts().unreadable_snapshots, 0);
    let restarted = started(&journal, &shelf, &transport, plus(t0(), 1));
    assert_eq!(restarted.last_known(), Some(t0()), "folded at start");
    reader.drain(plus(t0(), 2));
    assert_eq!(
        mandate_news_reader::reader::exit(&reader),
        Some(Exit::Drained)
    );
}
