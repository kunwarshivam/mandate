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
        block_on(reader.step(Clock::agreed(plus(t0(), second))));
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
    block_on(reader.step(Clock::agreed(t0())));
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

/// The workspace of `ready_workspace` with one stored item journaled by an earlier writer, at seq
/// 2 after the stream's `StreamOpened`.
fn workspace_with_one_item() -> (Journal, Shelf, Recorded) {
    let (journal, shelf, transport) = ready_workspace();
    let stream = StreamId::parse(&format!("news:{WS}")).unwrap();
    let mut writer_store = journal.clone();
    let epoch = writer_store.take_ownership(&stream).unwrap();
    let writer = Writer {
        workspace: WS.to_owned(),
        environment: Environment::Paper,
        epoch,
    };
    let records = [
        Record::Opened,
        Record::Stored(Stored {
            source: "alpaca.news".to_owned(),
            allowlist_version: 3,
            item_id: "1:2026-10-26T12:00:00.000000000Z".to_owned(),
            symbols: BTreeSet::from(["SPY".to_owned()]),
            published_at: at("2026-10-26T12:00:00Z"),
            knowledge_at: at("2026-10-26T14:30:00Z"),
            item_ref: ArtifactRef::of(&1_u64.to_be_bytes()),
            licensed: true,
        }),
    ];
    let drafts = writer.drafts(0, t0(), &records).unwrap();
    let appended = writer_store.append(&stream, 0, epoch, t0(), &drafts);
    assert!(
        matches!(appended, mandate_journal::AppendOutcome::Committed(_)),
        "{appended:?}"
    );
    (journal, shelf, transport)
}

/// The stored item's body with its payload member `name` replaced by `value`, or removed.
fn with_member(journal: &Journal, name: &str, value: Option<mandate_canon::Value>) -> Vec<u8> {
    let row = journal.news().into_iter().find(|row| row.seq == 2).unwrap();
    let mut body = mandate_canon::parse(&row.body).unwrap();
    let mandate_canon::Value::Object(members) = &mut body else {
        panic!("a body is an object")
    };
    let payload = members
        .get_mut(&mandate_canon::Key::new("payload").unwrap())
        .unwrap();
    let mandate_canon::Value::Object(payload) = payload else {
        panic!("a payload is an object")
    };
    let key = mandate_canon::Key::new(name).unwrap();
    match value {
        Some(value) => {
            payload.insert(key, value);
        }
        None => {
            payload.remove(&key);
        }
    }
    mandate_canon::to_canonical(&body)
}

/// DEC-2281 item 3, as amended: a stored item the reader cannot read at start would leave its
/// held ids or its last known time short, so it would store a held item again or ask from too far
/// back. It stops the start instead: not ready with `news_stream_unreadable`, nothing fetched,
/// nothing journaled, and a drain on stop. The journal holds the row unchanged.
#[test]
fn a_stored_item_it_cannot_read_at_start_stops_it_with_nothing_fetched_or_journaled() {
    let (journal, shelf, transport) = workspace_with_one_item();
    let readable = started(&journal, &shelf, &transport, plus(t0(), 1));
    assert_eq!(readable.not_ready(), None, "the stored item reads");
    assert_eq!(readable.lifecycle().state(), State::Ready);
    let cases = [
        ("an unparseable body", b"{not canonical".to_vec()),
        ("no item_id", with_member(&journal, "item_id", None)),
        (
            "a garbled knowledge_at",
            with_member(
                &journal,
                "knowledge_at",
                Some(mandate_canon::Value::Str("yesterday".to_owned())),
            ),
        ),
        (
            "no knowledge_at",
            with_member(&journal, "knowledge_at", None),
        ),
    ];
    for (case, body) in cases {
        let (journal, shelf, transport) = workspace_with_one_item();
        journal.news_bodies.borrow_mut().insert(2, body);
        let before = journal.news();
        let mut reader = started(&journal, &shelf, &transport, plus(t0(), 1));
        for second in 2..7 {
            block_on(reader.step(Clock::agreed(plus(t0(), second))));
        }
        assert_eq!(reader.not_ready(), Some("news_stream_unreadable"), "{case}");
        assert_ne!(reader.lifecycle().state(), State::Ready, "{case}");
        assert!(transport.sent().is_empty(), "{case}: nothing fetched");
        assert_eq!(journal.news(), before, "{case}: nothing journaled");
        reader.drain(plus(t0(), 8));
        assert_eq!(exit(&reader), Some(Exit::Drained), "{case}: it drains");
    }
}

/// E2-10d: a reader that finds a stored item it cannot read fences nothing. It reads and folds
/// its stream before it takes the writer epoch, so it refuses `news_stream_unreadable` holding no
/// epoch, and the earlier writer's next append still commits at its own epoch.
#[test]
fn a_reader_that_cannot_read_its_stream_fences_no_earlier_writer() {
    let (journal, shelf, transport) = workspace_with_one_item();
    let stream = StreamId::parse(&format!("news:{WS}")).unwrap();
    let mut earlier = journal.clone();
    let epoch = earlier.take_ownership(&stream).unwrap();
    let writer = Writer {
        workspace: WS.to_owned(),
        environment: Environment::Paper,
        epoch,
    };
    journal
        .news_bodies
        .borrow_mut()
        .insert(2, b"{not canonical".to_vec());
    let refused = started(&journal, &shelf, &transport, plus(t0(), 1));
    assert_eq!(refused.not_ready(), Some("news_stream_unreadable"));
    assert_eq!(
        refused.lifecycle().state(),
        State::Replaying,
        "started, holding no epoch"
    );
    let next = Record::Stored(Stored {
        source: "alpaca.news".to_owned(),
        allowlist_version: 3,
        item_id: "2:2026-10-26T12:30:00.000000000Z".to_owned(),
        symbols: BTreeSet::from(["SPY".to_owned()]),
        published_at: at("2026-10-26T12:30:00Z"),
        knowledge_at: at("2026-10-26T14:45:00Z"),
        item_ref: ArtifactRef::of(&2_u64.to_be_bytes()),
        licensed: true,
    });
    let drafts = writer.drafts(2, t0(), &[next]).unwrap();
    let appended = earlier.append(&stream, 2, epoch, t0(), &drafts);
    assert!(
        matches!(appended, mandate_journal::AppendOutcome::Committed(_)),
        "the earlier writer still writes: {appended:?}"
    );
}

/// Has another writer store `item_id` at seq 3 as the reader's next `take_ownership` begins, after
/// its first read of the stream; with `unreadable`, the row reads back as a body that does not
/// parse.
fn stored_between_the_reads(journal: &Journal, item_id: &str, unreadable: bool) {
    let item_id = item_id.to_owned();
    let bodies = journal.news_bodies.clone();
    *journal.before_ownership.borrow_mut() = Some(Box::new(move |memory| {
        let stream = StreamId::parse(&format!("news:{WS}")).unwrap();
        let epoch = memory.take_ownership(&stream).unwrap();
        let writer = Writer {
            workspace: WS.to_owned(),
            environment: Environment::Paper,
            epoch,
        };
        let between = Record::Stored(Stored {
            source: "alpaca.news".to_owned(),
            allowlist_version: 3,
            item_id,
            symbols: BTreeSet::from(["SPY".to_owned()]),
            published_at: at("2026-10-26T14:00:00Z"),
            knowledge_at: at("2026-10-26T14:50:00Z"),
            item_ref: ArtifactRef::of(&3_u64.to_be_bytes()),
            licensed: true,
        });
        let drafts = writer.drafts(2, t0(), &[between]).unwrap();
        let appended = memory.append(&stream, 2, epoch, t0(), &drafts);
        assert!(
            matches!(appended, mandate_journal::AppendOutcome::Committed(_)),
            "{appended:?}"
        );
        if unreadable {
            bodies.borrow_mut().insert(3, b"{not canonical".to_vec());
        }
    }));
}

/// DEC-2287 item 2: the reader folds the rows it reads after it takes the epoch, not those of its
/// first read. An item another writer stored between the two reads is held, so the reader, offered
/// it again, stores nothing and stays ready.
#[test]
fn an_item_stored_between_the_reads_is_held_and_never_stored_again() {
    let (journal, shelf, transport) = workspace_with_one_item();
    let offered = "1:2026-10-26T14:00:00.000000000Z";
    stored_between_the_reads(&journal, offered, false);
    let mut reader = started(&journal, &shelf, &transport, plus(t0(), 1));
    assert_eq!(reader.not_ready(), None);
    assert_eq!(reader.lifecycle().state(), State::Ready);
    let before = journal.news();
    assert_eq!(before.len(), 3, "the item stored between the reads");
    for second in 2..7 {
        block_on(reader.step(Clock::agreed(plus(t0(), second))));
    }
    assert!(
        !transport.sent().is_empty(),
        "the page offering it was asked"
    );
    assert_eq!(journal.news(), before, "the held item is not stored again");
    assert_eq!(exit(&reader), None, "no refused append stopped it");
    assert_eq!(reader.lifecycle().state(), State::Ready);
}

/// DEC-2287 item 2: a row unreadable at the read after the epoch refuses the start as one at the
/// first read does: not ready with `news_stream_unreadable`, never replayed, nothing fetched and
/// nothing journaled.
#[test]
fn a_row_unreadable_at_the_read_after_the_epoch_stops_the_start() {
    let (journal, shelf, transport) = workspace_with_one_item();
    stored_between_the_reads(&journal, "1:2026-10-26T14:00:00.000000000Z", true);
    let mut reader = started(&journal, &shelf, &transport, plus(t0(), 1));
    let before = journal.news();
    for second in 2..7 {
        block_on(reader.step(Clock::agreed(plus(t0(), second))));
    }
    assert_eq!(reader.not_ready(), Some("news_stream_unreadable"));
    assert_eq!(
        reader.lifecycle().state(),
        State::Replaying,
        "it took the epoch and never replayed"
    );
    assert!(transport.sent().is_empty(), "nothing fetched");
    assert_eq!(journal.news(), before, "nothing journaled");
}
