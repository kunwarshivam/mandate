//! The cell's recorded plane serves `filings.search` and `filings.get` (backlog E19-16, DEC-2328;
//! data-plane spec §4.6; agent harness spec §6.10). A search keeps exactly the filings of an asked
//! form whose form and excerpt hold every term, case-folded, as a whole word or phrase, each with the
//! byte length of the text the store holds. A get answers one range of a filing's stored text, cut on
//! UTF-8 boundaries: `out_of_range` past the end, and `excerpt_only` for a filing recorded without
//! its text. A recording's `text` member is optional. The oracle reads the rows it wrote and matches
//! and slices with its own code, not the plane's.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use mandate_daemon::recorded::{Bundle, BundleError};
use mandate_research_run::{FilingReader, ItemId, ReadError, Symbol, TextQuery};
use mandate_time::UtcNanos;
use serde_json::{Value as Json, json};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const FILES: [&str; 5] = [
    "research.json",
    "bars.json",
    "news.json",
    "filings.json",
    "instruments.json",
];
const CUT_OFF: i64 = 1_792_180_800;

/// A filing's full text, with characters of two, three and four bytes so a range can end inside one.
const TEXT: &str = "Item 2. Net sales rose 8% — services reached a record; Café revenue 🍎 grew.";

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// The recorded filings: a 10-Q with its full text, an 8-K recorded as an excerpt alone, and a 10-K
/// of another symbol.
fn rows() -> Vec<Json> {
    vec![
        json!({"age_s": 86_400, "excerpt": "Net sales rose in every segment; services set a record.",
               "filed_age_s": 90_000, "form": "10-Q", "item_id": "fl-aapl-10q", "licensed": false,
               "source": "src.newswire", "symbol": "AAPL", "text": TEXT}),
        json!({"age_s": 43_200, "excerpt": "The board named a new chief financial officer.",
               "filed_age_s": 43_200, "form": "8-K", "item_id": "fl-aapl-8k", "licensed": false,
               "source": "src.wire_two", "symbol": "AAPL"}),
        json!({"age_s": 200_000, "excerpt": "Cloud revenue grew; the annual report records it.",
               "filed_age_s": 200_000, "form": "10-K", "item_id": "fl-msft-10k", "licensed": false,
               "source": "src.newswire", "symbol": "MSFT"}),
    ]
}

/// A copy of the fixture bundle in its own directory, with `filings` as its filings.json.
fn bundle_with(name: &str, filings: &[Json]) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("daemon-{}-filings-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for file in FILES {
        let bytes = if file == "filings.json" {
            serde_json::to_vec(&Json::Array(filings.to_vec())).unwrap()
        } else {
            fs::read(Path::new(FIXTURE).join(file)).unwrap()
        };
        fs::write(dir.join(file), bytes).unwrap();
    }
    dir
}

fn recorded() -> Bundle {
    Bundle::load(&bundle_with("rows", &rows())).unwrap_or_else(|e| panic!("{e:?}"))
}

fn query() -> TextQuery {
    TextQuery {
        workspace: "ws1".to_owned(),
        symbols: BTreeSet::from([Symbol::new("AAPL").unwrap(), Symbol::new("MSFT").unwrap()]),
        window_start: at(CUT_OFF - 2_592_000),
        cut_off: at(CUT_OFF),
        allowlist_version: 3,
        max_items: 10,
    }
}

fn member(row: &Json, name: &str) -> String {
    row[name].as_str().unwrap().to_owned()
}

/// `term` in `text`, case-folded, bounded at each end by the edge or a character that is not an
/// ASCII letter or digit: this oracle's own scan of data-plane spec §4.6.
fn holds(text: &str, term: &str) -> bool {
    let text = text.to_ascii_lowercase();
    let term = term.to_ascii_lowercase();
    let word = |c: Option<char>| c.is_some_and(|c| c.is_ascii_alphanumeric());
    (0..text.len()).any(|start| {
        text.is_char_boundary(start)
            && text[start..].starts_with(&term)
            && !word(text[..start].chars().next_back())
            && !word(text[start + term.len()..].chars().next())
    })
}

/// The filings of an asked form whose form and excerpt hold every term, with the byte length of
/// the text the store holds: the full text when recorded, else the excerpt.
fn expected(forms: &[&str], terms: &[&str]) -> Vec<(String, u64)> {
    let mut kept: Vec<(String, u64)> = rows()
        .iter()
        .filter(|row| forms.contains(&member(row, "form").as_str()))
        .filter(|row| {
            let stored = format!("{} {}", member(row, "form"), member(row, "excerpt"));
            terms.iter().all(|term| holds(&stored, term))
        })
        .map(|row| {
            let held = row["text"]
                .as_str()
                .map_or_else(|| member(row, "excerpt"), str::to_owned);
            (member(row, "item_id"), u64::try_from(held.len()).unwrap())
        })
        .collect();
    kept.sort();
    kept
}

fn searched(bundle: &Bundle, forms: &[&str], terms: &[&str]) -> Vec<(String, u64)> {
    let forms: Vec<String> = forms.iter().map(|form| (*form).to_owned()).collect();
    let terms: Vec<String> = terms.iter().map(|term| (*term).to_owned()).collect();
    let mut found: Vec<(String, u64)> = bundle
        .plane
        .search(&query(), &forms, &terms)
        .unwrap_or_else(|e| panic!("{e:?}"))
        .into_iter()
        .map(|entry| (entry.filing.item_id.as_str().to_owned(), entry.stored_bytes))
        .collect();
    found.sort();
    found
}

#[test]
#[ignore = "pending E19-16"]
fn a_search_keeps_the_asked_forms_whose_text_holds_every_term() {
    let bundle = recorded();
    let all = ["10-K", "10-K/A", "10-Q", "10-Q/A", "8-K", "8-K/A"];
    for (forms, terms) in [
        (all.as_slice(), vec![]),
        (&["10-Q"][..], vec![]),
        (&["8-K", "10-K"][..], vec![]),
        (&all[..], vec!["record"]),
        (&all[..], vec!["RECORD"]),
        (&all[..], vec!["rec"]),
        (&all[..], vec!["10-q"]),
        (&all[..], vec!["services", "segment"]),
        (&all[..], vec!["chief financial officer"]),
        (&all[..], vec!["financial chief"]),
        (&["10-Q"][..], vec!["officer"]),
        (&all[..], vec!["revenue", "annual report"]),
    ] {
        assert_eq!(
            searched(&bundle, forms, &terms),
            expected(forms, &terms),
            "forms {forms:?} terms {terms:?}"
        );
    }
    assert_eq!(
        searched(&bundle, &all, &["record"]),
        [("fl-aapl-10q".to_owned(), 81)],
        "the oracle agrees with the rows it wrote: the 10-Q's text is held, and the 10-K's \
         \"records\" is not the word \"record\""
    );
}

/// What a range of `text` from `offset` of at most `length` bytes must answer, by this oracle's
/// own slicing: nothing from inside a character, `out_of_range` past the end, else the longest run
/// of whole characters from `offset` within `length` bytes.
fn range(text: &str, offset: usize, length: usize) -> Result<String, &'static str> {
    if offset > text.len() {
        return Err("out_of_range");
    }
    if !text.is_char_boundary(offset) {
        return Err("inside a character");
    }
    let mut shown = String::new();
    for c in text[offset..].chars() {
        if shown.len() + c.len_utf8() > length {
            break;
        }
        shown.push(c);
    }
    Ok(shown)
}

#[test]
#[ignore = "pending E19-16"]
fn a_get_answers_each_range_of_the_stored_text_cut_on_character_boundaries() {
    let bundle = recorded();
    let item = ItemId::new("fl-aapl-10q").unwrap();
    let mut cut = 0;
    for offset in 0..=TEXT.len() + 2 {
        for length in [1, 2, 3, 5, 8, 40, 8_000] {
            let answered = bundle.plane.filing_text(
                &item,
                at(CUT_OFF),
                3,
                u64::try_from(offset).unwrap(),
                u32::try_from(length).unwrap(),
            );
            match (range(TEXT, offset, length), answered) {
                (Ok(shown), Ok(text)) => {
                    assert_eq!(text.text, shown, "offset {offset} length {length}");
                    assert_eq!(text.offset, u64::try_from(offset).unwrap());
                    assert_eq!(text.stored_bytes, u64::try_from(TEXT.len()).unwrap());
                    assert_eq!(text.filing.item_id, item);
                    assert_eq!(text.filing.knowledge_at, at(CUT_OFF - 86_400));
                    cut += usize::from(shown.len() < length.min(TEXT.len() - offset));
                }
                (Err("out_of_range"), Err(ReadError::OutOfRange)) => {}
                (Err("inside a character"), Err(error)) => {
                    assert_ne!(error, ReadError::OutOfRange, "offset {offset}");
                }
                (expected, answered) => {
                    panic!("offset {offset} length {length}: {expected:?} but {answered:?}")
                }
            }
        }
    }
    assert!(
        cut > 0,
        "some ranges end before a character they would split"
    );
}

#[test]
#[ignore = "pending E19-16"]
fn a_get_answers_excerpt_only_without_the_text_and_nothing_for_another_version_or_item() {
    let bundle = recorded();
    let get = |id: &str, version: u32| {
        bundle
            .plane
            .filing_text(&ItemId::new(id).unwrap(), at(CUT_OFF), version, 0, 100)
    };
    assert_eq!(
        get("fl-aapl-8k", 3).map(|t| t.text),
        Err(ReadError::ExcerptOnly)
    );
    assert_eq!(
        get("fl-not-recorded", 3).map(|t| t.text),
        Err(ReadError::Unavailable)
    );
    assert_eq!(
        get("fl-aapl-10q", 4).map(|t| t.text),
        Err(ReadError::Unavailable),
        "the recording holds allowlist version 3 alone"
    );
    assert!(get("fl-aapl-10q", 3).is_ok());
}

#[test]
#[ignore = "pending E19-16"]
fn a_filing_s_text_is_optional_and_must_be_text_when_recorded() {
    let without: Vec<Json> = rows()
        .into_iter()
        .map(|mut row| {
            row.as_object_mut().unwrap().remove("text");
            row
        })
        .collect();
    Bundle::load(&bundle_with("no-text", &without)).unwrap_or_else(|e| panic!("{e:?}"));
    let mut wrong = rows();
    wrong[0]["text"] = json!(42);
    assert_eq!(
        Bundle::load(&bundle_with("text-not-text", &wrong)),
        Err(BundleError::Malformed {
            file: "filings.json",
            member: "text"
        })
    );
}
