#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! The reader's start (DEC-2278 item 1, DEC-2280 item 6; journal spec §9.37's "What the writer
//! owes"): a reader that cannot run takes no epoch, writes nothing, fetches nothing, never
//! crashes, and stays not ready with its reason; one that can takes a newer epoch, fencing every
//! earlier reader, and folds its stream for the items it holds and the last known time.

mod common;

use std::collections::BTreeSet;

use mandate_journal::{ArtifactRef, Environment, StreamId};
use mandate_news_reader::reader::exit;
use mandate_news_reader::records::{Record, Stored, Writer};
use mandate_news_reader::{Follow, KeyRefusal, NewsReader};
use mandate_process::{Exit, State};
use mandate_sources::allowlist::Allowlist;
use mandate_sources::fetch::NewsAuth;
use mandate_store::Store;

use common::*;

/// A reader that cannot run: not ready with `code`, alive, and doing nothing in five steps.
fn holds_still(mut reader: Reader, journal: &Journal, transport: &Recorded, code: &str) {
    for second in 0..5 {
        block_on(reader.step(plus(t0(), second)));
    }
    assert_eq!(reader.not_ready(), Some(code));
    assert_eq!(
        reader.lifecycle().state(),
        State::Replaying,
        "{code}: started, holding no epoch"
    );
    assert!(reader.lifecycle().live(plus(t0(), 5)), "{code}: still live");
    assert!(transport.sent().is_empty(), "{code}: nothing fetched");
    assert!(journal.news().is_empty(), "{code}: nothing journaled");
    reader.drain(plus(t0(), 6));
    assert_eq!(
        exit(&reader),
        Some(Exit::Drained),
        "{code}: it drains, never crashes"
    );
}

fn ready_workspace() -> (Journal, Shelf, Recorded) {
    let (journal, shelf, transport) = (Journal::default(), Shelf::default(), Recorded::default());
    spy(&journal, &shelf);
    transport.answer(page(
        &[item(
            1,
            "2026-10-26T14:00:00Z",
            "2026-10-26T14:00:00Z",
            &["SPY"],
        )],
        None,
    ));
    (journal, shelf, transport)
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
        let (journal, shelf, transport) = ready_workspace();
        let reader = NewsReader::start(
            journal.clone(),
            shelf,
            transport.clone(),
            &config(),
            key,
            t0(),
        );
        holds_still(reader, &journal, &transport, code);
    }
}

#[test]
fn with_no_follows_or_a_malformed_one_it_is_not_ready_whatever_its_key() {
    let malformed = vec![
        Follow {
            agent: AGENT.to_owned(),
            account: ACCOUNT.to_owned(),
        },
        Follow {
            agent: "agent b".to_owned(),
            account: ACCOUNT.to_owned(),
        },
    ];
    for (follows, code) in [(Vec::new(), "no_follows"), (malformed, "follow_malformed")] {
        for key in [Ok(paper_auth()), Err(KeyRefusal::NoKey)] {
            let (journal, shelf, transport) = ready_workspace();
            let mut config = config();
            config.follows = follows.clone();
            let reader = NewsReader::start(
                journal.clone(),
                shelf,
                transport.clone(),
                &config,
                key,
                t0(),
            );
            holds_still(reader, &journal, &transport, code);
        }
    }
}

#[test]
fn a_source_the_allowlist_does_not_have_in_effect_is_not_ready_and_fetches_nothing() {
    let (journal, shelf, transport) = ready_workspace();
    let mut config = config();
    config.allowlist = Allowlist::parse(&ALLOWLIST.replace(
        "approved_by = \"founder\"\napproved_in = \"DEC-2394\"\n",
        "",
    ))
    .unwrap();
    let reader = started_with(&journal, &shelf, &transport, &config, t0());
    holds_still(reader, &journal, &transport, "news_source_not_in_effect");
}

#[test]
fn only_a_paper_workspace_s_reader_starts() {
    let (journal, shelf, transport) = ready_workspace();
    let mut config = config();
    config.environment = Environment::Live;
    let mut reader = started_with(&journal, &shelf, &transport, &config, t0());
    block_on(reader.step(t0()));
    assert_eq!(exit(&reader), Some(Exit::Refused));
    assert!(transport.sent().is_empty());
    assert!(journal.news().is_empty());
}

#[test]
fn a_ready_reader_holds_a_newer_epoch_and_folds_its_stream_at_start() {
    let (journal, shelf, transport) = ready_workspace();
    let empty = started(&journal, &shelf, &transport, t0());
    assert_eq!(empty.lifecycle().state(), State::Ready);
    assert_eq!(empty.not_ready(), None);
    assert_eq!(empty.last_known(), None, "an empty stream");
    assert_eq!(exit(&empty), None);

    let stream = StreamId::parse(&format!("news:{WS}")).unwrap();
    let mut writer_store = journal.clone();
    let epoch = writer_store.take_ownership(&stream).unwrap();
    let writer = Writer {
        workspace: WS.to_owned(),
        environment: Environment::Paper,
        epoch,
    };
    let record = |n: u64, known: &str| {
        Record::Stored(Stored {
            source: "alpaca.news".to_owned(),
            allowlist_version: 3,
            item_id: format!("{n}:2026-10-26T12:00:00.000000000Z"),
            symbols: BTreeSet::from(["SPY".to_owned()]),
            published_at: at("2026-10-26T12:00:00Z"),
            knowledge_at: at(known),
            item_ref: ArtifactRef::of(&n.to_be_bytes()),
            licensed: true,
        })
    };
    let records = [
        Record::Opened,
        record(1, "2026-10-26T14:00:00Z"),
        record(2, "2026-10-26T14:30:00Z"),
    ];
    let drafts = writer.drafts(0, t0(), &records).unwrap();
    let appended = writer_store.append(&stream, 0, epoch, t0(), &drafts);
    assert!(
        matches!(appended, mandate_journal::AppendOutcome::Committed(_)),
        "{appended:?}"
    );
    let restarted = started(&journal, &shelf, &transport, plus(t0(), 1));
    assert_eq!(restarted.last_known(), Some(at("2026-10-26T14:30:00Z")));
    let later = writer
        .drafts(3, t0(), &[record(3, "2026-10-26T14:45:00Z")])
        .unwrap();
    let fenced = writer_store.append(&stream, 3, epoch, t0(), &later);
    assert!(
        matches!(fenced, mandate_journal::AppendOutcome::Fenced { .. }),
        "the reader's start fenced the earlier writer: {fenced:?}"
    );
}
