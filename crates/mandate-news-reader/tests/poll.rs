#![allow(
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "tests: a wrong answer should stop the test"
)]
//! A round of requests (DEC-2278 item 2): the tickers sorted and in chunks the fetcher takes, each
//! chunk followed through its pages from one start up to the page cap, and the pause after an
//! idle or failed round. The oracle walks the round and counts what it asked.

mod common;

use std::collections::BTreeSet;

use mandate_news_reader::poll::{
    COOLDOWN_S, FIRST_LOOKBACK_S, MAX_PAGES, POLL_OVERLAP_S, Round, cooldown_until, round_start,
};
use mandate_sources::fetch::MAX_NEWS_SYMBOLS;
use mandate_sources::ingest::PageToken;
use mandate_sources::symbols::Symbol;

use common::{plus, t0};

fn tickers(n: usize) -> BTreeSet<Symbol> {
    (0..n)
        .map(|i| Symbol::parse(&format!("T{i:03}")).unwrap())
        .collect()
}

fn token(n: u32) -> PageToken {
    PageToken::parse(&format!("page{n}")).unwrap()
}

/// One request: its tickers and the page it asks for.
type Asked = (Vec<String>, Option<String>);

/// Every request a round makes when each chunk's pages run `pages` deep: each request's tickers
/// and page.
fn walk(mut round: Round, pages: u32) -> (Vec<Asked>, u32) {
    let (mut asked, mut capped, mut deep) = (Vec::new(), 0, 0);
    loop {
        let (symbols, _, page) = round.request();
        asked.push((
            symbols
                .iter()
                .map(|s| s.as_str().to_owned())
                .collect::<Vec<_>>(),
            page.map(|p| p.as_str().to_owned()),
        ));
        deep += 1;
        let next = (deep < pages).then(|| token(deep));
        let after = round.after(next);
        capped += u32::from(after.capped);
        if after
            .round
            .as_ref()
            .is_some_and(|r| r.request().2.is_none())
        {
            deep = 0;
        }
        match after.round {
            Some(next) => round = next,
            None => return (asked, capped),
        }
    }
}

#[test]
fn the_tickers_go_out_sorted_in_chunks_the_fetcher_takes() {
    let all = tickers(2 * MAX_NEWS_SYMBOLS + 3);
    let round = Round::new(all.clone(), t0()).unwrap();
    let (asked, capped) = walk(round, 1);
    assert_eq!(capped, 0);
    let sizes: Vec<usize> = asked.iter().map(|(symbols, _)| symbols.len()).collect();
    assert_eq!(sizes, [MAX_NEWS_SYMBOLS, MAX_NEWS_SYMBOLS, 3]);
    let flat: Vec<String> = asked.into_iter().flat_map(|(symbols, _)| symbols).collect();
    let expected: Vec<String> = all.iter().map(|s| s.as_str().to_owned()).collect();
    assert_eq!(flat, expected, "each ticker once, in order");
    assert_eq!(
        Round::new(BTreeSet::new(), t0()),
        None,
        "nothing to ask is no round"
    );
}

#[test]
fn each_chunk_follows_its_pages_from_the_round_s_start() {
    let round = Round::new(tickers(MAX_NEWS_SYMBOLS + 1), t0()).unwrap();
    let (asked, capped) = walk(round.clone(), 3);
    assert_eq!(capped, 0);
    let pages: Vec<(usize, Option<String>)> = asked
        .iter()
        .map(|(symbols, page)| (symbols.len(), page.clone()))
        .collect();
    assert_eq!(
        pages,
        [
            (MAX_NEWS_SYMBOLS, None),
            (MAX_NEWS_SYMBOLS, Some("page1".to_owned())),
            (MAX_NEWS_SYMBOLS, Some("page2".to_owned())),
            (1, None),
            (1, Some("page1".to_owned())),
            (1, Some("page2".to_owned())),
        ]
    );
    let (_, start, _) = round.after(Some(token(1))).round.unwrap().request();
    assert_eq!(start, t0(), "a later page asks from the same start");
}

#[test]
fn a_chunk_stops_at_the_page_cap_and_says_so() {
    let round = Round::new(tickers(MAX_NEWS_SYMBOLS + 1), t0()).unwrap();
    let (asked, capped) = walk(round, u32::MAX);
    assert_eq!(capped, 2, "each chunk hit the cap once");
    let max_pages = usize::try_from(MAX_PAGES).unwrap();
    assert_eq!(asked.len(), 2 * max_pages);
    assert_eq!(
        asked[max_pages].1, None,
        "the next chunk starts at its first page"
    );
    let (asked, capped) = walk(Round::new(tickers(1), t0()).unwrap(), MAX_PAGES);
    assert_eq!(
        (asked.len(), capped),
        (max_pages, 0),
        "a chunk whose last page is the cap's is not capped"
    );
}

#[test]
fn a_round_asks_from_the_last_known_time_less_the_overlap_or_the_lookback_and_a_pause_lasts_its_cooldown()
 {
    assert_eq!(
        round_start(Some(plus(t0(), -5)), t0()),
        Some(plus(t0(), -5 - POLL_OVERLAP_S)),
        "the last known time, less the overlap"
    );
    assert_eq!(POLL_OVERLAP_S, 15 * 60);
    assert_eq!(round_start(None, t0()), Some(plus(t0(), -FIRST_LOOKBACK_S)));
    assert_eq!(FIRST_LOOKBACK_S, 48 * 3_600);
    assert_eq!(cooldown_until(t0()), Some(plus(t0(), COOLDOWN_S)));
    assert_eq!(COOLDOWN_S, 30);
}
