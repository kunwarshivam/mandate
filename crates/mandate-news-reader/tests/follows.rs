#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "tests: a broken fixture or a wrong answer should stop the test"
)]
//! What the reader polls (DEC-2278 item 2, DEC-2280 item 1): the agents it follows, each checked
//! against the workspace; each asset's ticker from the latest registered instrument snapshot; and
//! a followed agent's active theses and held instruments, through the research core's and the
//! gate context's own folds. The oracle is each test's own reading of the rows it wrote.

mod common;

use std::collections::BTreeSet;

use mandate_journal::{ArtifactRef, StoredEvent};
use mandate_news_reader::follows::check;
use mandate_news_reader::tickers::registered_tickers;
use mandate_news_reader::watch::followed;
use mandate_news_reader::{Follow, parse_follows};

use common::*;

fn rows(journal: &Journal, stream: &str) -> Vec<StoredEvent> {
    journal
        .others
        .borrow()
        .get(stream)
        .cloned()
        .unwrap_or_default()
}

fn follow(agent: &str, account: &str) -> Follow {
    Follow {
        agent: agent.to_owned(),
        account: account.to_owned(),
    }
}

#[test]
fn each_follow_is_read_as_written_and_a_flag_with_no_value_is_an_empty_follow() {
    let args = |text: &str| -> Vec<String> { text.split(' ').map(str::to_owned).collect() };
    assert_eq!(
        parse_follows(&args(
            "run --follow a1=acct1 --workspace ws --follow a2=acct2"
        )),
        [follow("a1", "acct1"), follow("a2", "acct2")]
    );
    assert_eq!(parse_follows(&args("run --workspace ws")), []);
    assert_eq!(
        parse_follows(&args("run --follow a1 --follow =acct --follow")),
        [follow("", ""), follow("", "acct"), follow("", "")]
    );
}

#[test]
fn every_follow_must_name_the_workspace_s_streams_and_there_must_be_one() {
    let checked = check(WS, &[follow(AGENT, ACCOUNT), follow("agent_b", "acct_b")]).unwrap();
    let named: Vec<(&str, &str, &str)> = checked
        .iter()
        .map(|f| {
            (
                f.agent.as_str(),
                f.agent_stream.as_str(),
                f.account_stream.as_str(),
            )
        })
        .collect();
    assert_eq!(
        named,
        [
            (
                AGENT,
                "agent:ws_1:agent_a",
                "acct:ws_1:01KDER00000000000000000001"
            ),
            ("agent_b", "agent:ws_1:agent_b", "acct:ws_1:acct_b"),
        ]
    );
    assert_eq!(check(WS, &[]), Err("no_follows"));
    for malformed in [
        follow("", ACCOUNT),
        follow(AGENT, ""),
        follow("agent:x", ACCOUNT),
        follow(AGENT, "acct x"),
    ] {
        assert_eq!(
            check(WS, &[follow(AGENT, ACCOUNT), malformed.clone()]),
            Err("follow_malformed"),
            "one malformed follow refuses them all: {malformed:?}"
        );
    }
}

#[test]
fn an_asset_s_ticker_is_its_latest_readable_snapshot_s_and_an_unreadable_one_is_counted() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    register(&journal, &shelf, SPY, "SPY");
    register(&journal, &shelf, QQQ, "QQQ");
    register(&journal, &shelf, QQQ, "not a ticker");
    register(&journal, &shelf, AAPL, "AAPL");
    register(&journal, &shelf, AAPL, "AAPL.B");
    register_ref(&journal, &ArtifactRef::of(b"no such artifact"));
    let control = rows(&journal, &format!("ctl:{WS}"));
    let tickers = registered_tickers(&control, &*shelf.objects.borrow());
    assert_eq!(tickers.ticker(SPY).map(|t| t.as_str()), Some("SPY"));
    assert_eq!(
        tickers.ticker(QQQ),
        None,
        "a later registration wins, even one whose symbol is not a ticker"
    );
    assert_eq!(tickers.ticker(AAPL).map(|t| t.as_str()), Some("AAPL.B"));
    assert_eq!(tickers.ticker("never-registered"), None);
    assert_eq!(tickers.unreadable(), 1);
    let mut reversed = control.clone();
    reversed.reverse();
    let tickers = registered_tickers(&reversed, &*shelf.objects.borrow());
    assert_eq!(
        tickers.ticker(AAPL).map(|t| t.as_str()),
        Some("AAPL.B"),
        "the latest by sequence, whatever order the rows are handed in"
    );
}

#[test]
fn only_instrument_snapshot_registrations_are_read() {
    let (journal, shelf) = (Journal::default(), Shelf::default());
    let reference = register(&journal, &shelf, SPY, "SPY");
    journal.push(
        &format!("ctl:{WS}"),
        "ConfigSnapshotRegistered",
        object(&[
            ("kind", s("rule_set")),
            ("content_hash", s(&ArtifactRef::of(b"absent").to_string())),
        ]),
    );
    journal.push(
        &format!("ctl:{WS}"),
        "MandateConfirmed",
        object(&[("content_hash", s(&reference.to_string()))]),
    );
    let tickers = registered_tickers(
        &rows(&journal, &format!("ctl:{WS}")),
        &*shelf.objects.borrow(),
    );
    assert_eq!(tickers.ticker(SPY).map(|t| t.as_str()), Some("SPY"));
    assert_eq!(
        tickers.unreadable(),
        0,
        "another kind's missing artifact is not read"
    );
}

#[test]
fn an_agent_s_active_theses_and_held_instruments_are_followed() {
    let journal = Journal::default();
    thesis(&journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
    thesis(&journal, "01THESISAAPL", AAPL, "2026-10-26T15:00:30Z");
    bought(&journal, "o1", QQQ, "2");
    let agent = rows(&journal, &format!("agent:{WS}:{AGENT}"));
    let account_name = format!("acct:{WS}:{ACCOUNT}");
    let account = rows(&journal, &account_name);
    let watched = followed(AGENT, &agent, &account_name, &account, t0());
    assert_eq!(
        watched.assets,
        BTreeSet::from([AAPL.to_owned(), QQQ.to_owned(), SPY.to_owned()])
    );
    assert!(!watched.theses_unreadable && !watched.held_unreadable);
    let later = followed(AGENT, &agent, &account_name, &account, plus(t0(), 60));
    assert_eq!(
        later.assets,
        BTreeSet::from([QQQ.to_owned(), SPY.to_owned()]),
        "an expired thesis is no longer followed"
    );
    invalidated(&journal, "01THESISSPY");
    let agent = rows(&journal, &format!("agent:{WS}:{AGENT}"));
    let ended = followed(AGENT, &agent, &account_name, &account, plus(t0(), 60));
    assert_eq!(ended.assets, BTreeSet::from([QQQ.to_owned()]));
    let others = followed("agent_b", &agent, &account_name, &account, t0());
    assert!(
        !others.assets.contains(QQQ),
        "another agent's fills are not its holdings"
    );
}

#[test]
fn an_unreadable_thesis_stream_gives_none_of_its_instruments_and_an_unreadable_account_row_is_passed_over()
 {
    let journal = Journal::default();
    thesis(&journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
    bought(&journal, "o1", QQQ, "2");
    let account_name = format!("acct:{WS}:{ACCOUNT}");
    journal.push(
        &account_name,
        "FillApplied",
        object(&[("fill_id", s("broken"))]),
    );
    let agent = rows(&journal, &format!("agent:{WS}:{AGENT}"));
    let watched = followed(
        AGENT,
        &agent,
        &account_name,
        &rows(&journal, &account_name),
        t0(),
    );
    assert_eq!(
        watched.assets,
        BTreeSet::from([QQQ.to_owned(), SPY.to_owned()]),
        "a row the fold cannot read is passed over, and what is held still counts"
    );
    assert!(watched.held_unreadable && !watched.theses_unreadable);
    journal.push(
        &format!("agent:{WS}:{AGENT}"),
        "ThesisInvalidated",
        object(&[]),
    );
    let agent = rows(&journal, &format!("agent:{WS}:{AGENT}"));
    let watched = followed(AGENT, &agent, &account_name, &[], t0());
    assert!(watched.theses_unreadable && !watched.held_unreadable);
    assert!(watched.assets.is_empty());
}

/// The gate context's fold refuses every corporate action (DEC-2378 item 7), so a split on a held
/// instrument is reported and passed over: the instrument held before it, and one bought after it,
/// are still polled, when news on a held position matters most.
#[test]
fn a_split_on_a_held_instrument_never_stops_its_polling() {
    let journal = Journal::default();
    thesis(&journal, "01THESISSPY", SPY, "2026-11-30T00:00:00Z");
    bought(&journal, "o1", QQQ, "2");
    let account_name = format!("acct:{WS}:{ACCOUNT}");
    journal.push(
        &account_name,
        "CorporateActionApplied",
        object(&[("instrument_id", s(QQQ)), ("kind", s("split"))]),
    );
    let agent = rows(&journal, &format!("agent:{WS}:{AGENT}"));
    let watched = followed(
        AGENT,
        &agent,
        &account_name,
        &rows(&journal, &account_name),
        t0(),
    );
    assert_eq!(
        watched.assets,
        BTreeSet::from([QQQ.to_owned(), SPY.to_owned()]),
        "the thesis's and the held instrument both stay watched after a split"
    );
    assert!(watched.held_unreadable && !watched.theses_unreadable);
    bought(&journal, "o2", AAPL, "1");
    let after = followed(
        AGENT,
        &agent,
        &account_name,
        &rows(&journal, &account_name),
        t0(),
    );
    assert_eq!(
        after.assets,
        BTreeSet::from([AAPL.to_owned(), QQQ.to_owned(), SPY.to_owned()]),
        "the fold goes on past the split"
    );
}

/// An account row whose body does not parse is reported and passed over, as a refused one is: the
/// instrument the agent buys after it is still polled.
#[test]
fn an_account_row_whose_body_does_not_parse_never_stops_a_held_instrument_s_polling() {
    let journal = Journal::default();
    let account_name = format!("acct:{WS}:{ACCOUNT}");
    journal.push(&account_name, "FeesCharged", object(&[]));
    journal
        .others
        .borrow_mut()
        .get_mut(&account_name)
        .unwrap()
        .last_mut()
        .unwrap()
        .body
        .clone_from(&b"{not canonical".to_vec());
    bought(&journal, "o1", QQQ, "2");
    let watched = followed(
        AGENT,
        &[],
        &account_name,
        &rows(&journal, &account_name),
        t0(),
    );
    assert_eq!(watched.assets, BTreeSet::from([QQQ.to_owned()]));
    assert!(watched.held_unreadable);
}
