//! The cell's recorded plane serves `news.search` and `news.get` (backlog E19-16; data-plane spec
//! §4.6; agent harness spec §6.10 and HI-28). A search keeps exactly the items whose stored headline
//! or summary holds each term, case-folded, as a whole word or phrase within that one field, and
//! never anything else. A
//! get answers one recorded item's stored text, stamped at its age before the cut-off. The oracle
//! reads the fixture's files and matches terms with its own scanner, not the plane's.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use mandate_daemon::recorded::Bundle;
use mandate_research_run::{ItemId, NewsReader, ReadError, TextQuery};
use mandate_time::UtcNanos;
use serde_json::Value as Json;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const CUT_OFF: i64 = 1_792_155_600;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

fn query() -> TextQuery {
    TextQuery {
        workspace: "ws1".to_owned(),
        symbols: BTreeSet::new(),
        window_start: at(CUT_OFF - 400_000),
        cut_off: at(CUT_OFF),
        allowlist_version: 3,
        max_items: 50,
    }
}

/// The recorded news rows, as the file holds them.
fn rows() -> Vec<Json> {
    let bytes = fs::read(Path::new(FIXTURE).join("news.json")).unwrap();
    serde_json::from_slice::<Json>(&bytes)
        .unwrap()
        .as_array()
        .unwrap()
        .clone()
}

fn text_of(row: &Json, member: &str) -> String {
    row[member].as_str().unwrap().to_owned()
}

/// `term` in `text` as data-plane spec §4.6 reads it, by this oracle's own scan: case-folded, and
/// bounded at each end by the text's edge or a character that is not an ASCII letter or digit, so
/// it is a whole word or, with spaces, a whole phrase.
fn holds(text: &str, term: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let term = term.to_ascii_lowercase();
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    let mut from = 0;
    while let Some(found) = text[from..].find(&term) {
        let start = from + found;
        let end = start + term.len();
        if !word(text[..start].chars().next_back()) && !word(text[end..].chars().next()) {
            return true;
        }
        from = start + 1;
    }
    false
}

/// The item ids the fixture holds for which every term is in the headline or in the summary, a
/// phrase inside one field and never across the two (DEC-2327 item 1), in file order.
fn expected(terms: &[&str]) -> Vec<String> {
    rows()
        .iter()
        .filter(|row| {
            terms.iter().all(|term| {
                holds(&text_of(row, "headline"), term) || holds(&text_of(row, "summary"), term)
            })
        })
        .map(|row| text_of(row, "item_id"))
        .collect()
}

fn searched(bundle: &Bundle, terms: &[&str]) -> Vec<String> {
    let terms: Vec<String> = terms.iter().map(|term| (*term).to_owned()).collect();
    let mut found: Vec<String> = bundle
        .plane
        .search(&query(), &terms)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .into_iter()
        .map(|item| item.item_id.as_str().to_owned())
        .collect();
    let order: Vec<String> = rows().iter().map(|row| text_of(row, "item_id")).collect();
    found.sort_by_key(|id| order.iter().position(|o| o == id));
    found
}

#[test]
fn a_search_keeps_exactly_the_items_every_term_matches() {
    let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
    for terms in [
        vec!["services"],
        vec!["SERVICES"],
        vec!["serv"],
        vec!["services revenue"],
        vec!["revenue services"],
        vec!["record", "quarter"],
        vec!["apple", "orders"],
        vec!["triple"],
        vec!["triple."],
        vec!["region."],
        vec!["five-days"],
        vec!["record the"],
        vec!["triple an"],
        vec!["ervices"],
        vec!["vices"],
        vec![],
    ] {
        assert_eq!(
            searched(&bundle, &terms),
            expected(&terms),
            "terms {terms:?}"
        );
    }
    assert_eq!(
        searched(&bundle, &["services"]),
        ["nw-aapl-services"],
        "the oracle itself agrees with the fixture's text"
    );
    for part in ["serv", "ervices", "vices"] {
        assert!(
            searched(&bundle, &[part]).is_empty(),
            "a part of a word, at either end, is no match: {part}"
        );
    }
    assert!(
        searched(&bundle, &["record the"]).is_empty(),
        "a phrase across the headline and the summary is no match"
    );
    assert_eq!(
        searched(&bundle, &["region."]),
        ["nw-aapl-services"],
        "a term may hold a period"
    );
}

/// Every word of the recorded text, and words it does not hold, to draw terms from.
fn vocabulary() -> Vec<String> {
    let mut words: BTreeSet<String> = rows()
        .iter()
        .flat_map(|row| {
            format!("{} {}", text_of(row, "headline"), text_of(row, "summary"))
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|word| !word.is_empty())
                .map(str::to_ascii_lowercase)
                .collect::<Vec<_>>()
        })
        .collect();
    words.extend(["tesla", "quart", "apples", "ervices", "vices"].map(str::to_owned));
    words.into_iter().collect()
}

/// Every word the recording holds and five it does not (suffixes and stems among them), alone,
/// every ordered pair of them, and every two adjacent words of a headline or of a summary as one
/// phrase: each search keeps what the oracle keeps.
#[test]
fn every_word_pair_and_phrase_of_the_recording_keeps_what_the_oracle_keeps() {
    let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
    let words = vocabulary();
    let mut cases: Vec<Vec<String>> = words.iter().map(|word| vec![word.clone()]).collect();
    for first in &words {
        for second in &words {
            cases.push(vec![first.clone(), second.clone()]);
        }
    }
    for row in rows() {
        for field in ["headline", "summary"] {
            let stored = text_of(&row, field);
            let spoken: Vec<&str> = stored
                .split(|c: char| !c.is_ascii_alphanumeric())
                .filter(|word| !word.is_empty())
                .collect();
            cases.extend(spoken.windows(2).map(|pair| vec![pair.join(" ")]));
        }
    }
    let mut matched = 0;
    for terms in &cases {
        let terms: Vec<&str> = terms.iter().map(String::as_str).collect();
        let kept = expected(&terms);
        matched += usize::from(!kept.is_empty());
        assert_eq!(searched(&bundle, &terms), kept, "terms {terms:?}");
    }
    assert!(
        matched > 0 && matched < cases.len(),
        "some searches match and some do not: {matched} of {}",
        cases.len()
    );
}

#[test]
fn a_get_answers_a_recorded_item_s_stored_text_at_its_age() {
    let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
    for row in rows() {
        let id = ItemId::new(&text_of(&row, "item_id")).unwrap();
        let answered = bundle
            .plane
            .news_text(&id, at(CUT_OFF), 3)
            .unwrap_or_else(|e| panic!("{id:?}: {e:?}"));
        let age = row["age_s"].as_i64().unwrap();
        assert_eq!(answered.item.item_id, id);
        assert_eq!(answered.item.knowledge_at, at(CUT_OFF - age));
        assert_eq!(
            answered.text,
            format!(
                "{}\n\n{}",
                text_of(&row, "headline"),
                text_of(&row, "summary")
            ),
            "the stored text is the headline and summary the store holds"
        );
    }
}

#[test]
fn a_get_for_an_item_the_recording_lacks_or_another_allowlist_answers_nothing() {
    let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
    let missing = ItemId::new("nw-not-recorded").unwrap();
    assert_eq!(
        bundle
            .plane
            .news_text(&missing, at(CUT_OFF), 3)
            .map(|t| t.text),
        Err(ReadError::Unavailable)
    );
    let recorded = ItemId::new("nw-aapl-services").unwrap();
    assert_eq!(
        bundle
            .plane
            .news_text(&recorded, at(CUT_OFF), 4)
            .map(|t| t.text),
        Err(ReadError::Unavailable),
        "the recording holds allowlist version 3 alone"
    );
    assert!(
        bundle.plane.news_text(&recorded, at(CUT_OFF), 3).is_ok(),
        "and answers it"
    );
}

/// Every recorded bundle the repository holds loads as it did before the news reads were served:
/// the reads add no member a recording must carry (DEC-2327 item 3).
#[test]
fn every_recorded_bundle_still_loads_unchanged() {
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");
    let demo = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../deploy/cell/demo/research"
    );
    for dir in [
        format!("{fixtures}/research"),
        format!("{fixtures}/research-agentic"),
        format!("{fixtures}/research-adversarial"),
        demo.to_owned(),
    ] {
        Bundle::load(Path::new(&dir)).unwrap_or_else(|e| panic!("{dir}: {e:?}"));
    }
}
