//! A dispatch's news search (agent harness spec v0.2 §6.10 and §6.11; DEC-2209, DEC-2213; backlog
//! E19-13 slice 3b): `news.search` takes its scope from the harness and only its terms from the
//! model, every answer is filtered again on knowledge time, the window, the allowlist, licensing,
//! and scope, and items come newest first, cut to their byte caps, each wrapped as untrusted with
//! no link. `news.get` follows in the next pair of PRs.

mod common;

use common::{BLOG, CUT_OFF, DAY, Fixture, NEWSWIRE, at, news, scope, symbol};
use mandate_canon::{Value, to_canonical};
use mandate_research_run::agentic::{Code, LoopBounds, Tool, ToolCall, ToolCallId, ToolOutcome};
use mandate_research_run::{
    Dispatch, DispatchScope, Dispatched, NewsItem, ResearchRunError, RetrievedItem, dispatch,
};
use proptest::strategy::{Strategy, ValueTree};
use proptest::test_runner::TestRunner;

const TURN: u32 = 3;
const AAPL_NEWS: &str = r#"{"limit":20,"symbols":["AAPL"],"terms":[],"window_h":24}"#;
const FAILED: (ToolOutcome, Option<Code>) = (ToolOutcome::Failed, Some(Code::Unavailable));
const OK: (ToolOutcome, Option<Code>) = (ToolOutcome::Ok, None);

fn call(index: u32, tool: Tool, arguments: &str) -> ToolCall {
    let arguments = arguments.to_owned();
    ToolCall {
        index,
        tool,
        arguments,
    }
}

fn try_run(
    fixture: &Fixture,
    calls: &[ToolCall],
    bounds: LoopBounds,
    left: u32,
) -> Result<Dispatch, ResearchRunError> {
    let scope = scope();
    let within = DispatchScope {
        scope: &scope,
        bounds: &bounds,
        result_bytes_left: left,
    };
    dispatch(TURN, calls, within, fixture.readers())
}

fn run(fixture: &Fixture, calls: &[ToolCall], bounds: LoopBounds, left: u32) -> Dispatch {
    try_run(fixture, calls, bounds, left).unwrap_or_else(|e| panic!("{e:?}"))
}

fn one(fixture: &Fixture, tool: Tool, arguments: &str) -> Dispatched {
    let calls = [call(0, tool, arguments)];
    let mut dispatched = run(fixture, &calls, LoopBounds::v0_2(), 96_000);
    assert_eq!(dispatched.results.len(), 1, "one call, one result");
    dispatched.results.remove(0)
}

fn with_news(news: Vec<NewsItem>) -> Fixture {
    Fixture {
        news,
        ..Fixture::default()
    }
}

fn verdict(result: &Dispatched) -> (ToolOutcome, Option<Code>) {
    (result.outcome, result.code)
}

fn parsed(result: &Dispatched) -> Value {
    mandate_canon::parse(&result.content).unwrap_or_else(|e| panic!("JSON: {e:?}"))
}

fn keys(value: &Value) -> Vec<&str> {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("an object: {value:?}"));
    object.keys().map(|key| key.as_str()).collect()
}

fn items(value: &Value) -> Vec<Value> {
    value
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .to_vec()
}

/// The `item_id` of every item the result shows the model, in its order.
fn shown(result: &Dispatched) -> Vec<String> {
    let id = |item: &Value| {
        item.get("item_id")
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    items(&parsed(result))
        .iter()
        .map(|item| id(item).unwrap_or_default())
        .collect()
}

/// The ids of the items the result will have observed, in their order.
fn observed(result: &Dispatched) -> Vec<String> {
    let id = |item: &RetrievedItem| item.item_id().map(|id| id.as_str().to_owned());
    result
        .items
        .iter()
        .map(|item| id(item).unwrap_or_default())
        .collect()
}

fn ids(names: &[&str]) -> Vec<String> {
    names.iter().map(|name| (*name).to_owned()).collect()
}

fn wide(n: i64) -> NewsItem {
    let mut item = news(&format!("n-{n:02}"), NEWSWIRE, "AAPL", CUT_OFF - 600 + n);
    item.summary = "x".repeat(1_500);
    item
}

#[test]
fn news_search_takes_its_scope_from_the_harness_and_its_terms_from_the_model() {
    let fixture = Fixture::default();
    let arguments =
        r#"{"limit":7,"symbols":["AAPL"],"terms":["guidance","share buyback"],"window_h":24}"#;
    assert_eq!(verdict(&one(&fixture, Tool::NewsSearch, arguments)), OK);
    let queries = fixture.queries.borrow();
    let [query] = queries.as_slice() else {
        panic!("one news query: {queries:?}")
    };
    assert_eq!(query.workspace, "ws1");
    assert_eq!(
        (query.cut_off, query.window_start),
        (at(CUT_OFF), at(CUT_OFF - 24 * 3_600))
    );
    assert_eq!(query.allowlist_version, 3);
    assert_eq!(query.symbols, [symbol("AAPL")].into_iter().collect());
    assert_eq!(query.max_items, 7);
    let terms = fixture.terms.borrow();
    assert_eq!(
        *terms,
        vec![ids(&["guidance", "share buyback"])],
        "the terms reach the reader"
    );
}

#[test]
fn news_search_filters_every_answer_again() {
    let mut licensed = news("n-licensed", NEWSWIRE, "AAPL", CUT_OFF - 60);
    licensed.licensed = true;
    let fixture = with_news(vec![
        news("n-kept-old", NEWSWIRE, "AAPL", CUT_OFF - 7_200),
        news("n-kept-new", NEWSWIRE, "AAPL", CUT_OFF - 60),
        news("n-future", NEWSWIRE, "AAPL", CUT_OFF + 1),
        news("n-stale", NEWSWIRE, "AAPL", CUT_OFF - 2 * DAY),
        news("n-blog", BLOG, "AAPL", CUT_OFF - 60),
        news("n-msft", NEWSWIRE, "MSFT", CUT_OFF - 60),
        news("bar:AAPL:1", NEWSWIRE, "AAPL", CUT_OFF - 60),
        news("n-kept-new", NEWSWIRE, "AAPL", CUT_OFF - 30),
        licensed,
    ]);
    let result = one(&fixture, Tool::NewsSearch, AAPL_NEWS);
    assert_eq!(
        shown(&result),
        ids(&["n-kept-new", "n-kept-old"]),
        "after the cut-off, before the window, off the allowlist, out of scope, a reserved id, \
         a second answer for one id, and licensed text are all dropped (HI-13, DEC-101, HI-23)"
    );
    assert_eq!(observed(&result), shown(&result));
    let Some(RetrievedItem::News(first)) = result.items.first() else {
        panic!("news")
    };
    assert_eq!(
        first.knowledge_at,
        at(CUT_OFF - 60),
        "the first answer for an id is kept"
    );
    assert!(result.untrusted, "news is third-party text");
}

#[test]
fn news_is_newest_first_then_by_id_up_to_the_models_limit() {
    let fixture = with_news(vec![
        news("n-b", NEWSWIRE, "AAPL", CUT_OFF - 60),
        news("n-a", NEWSWIRE, "AAPL", CUT_OFF - 60),
        news("n-c", NEWSWIRE, "AAPL", CUT_OFF - 600),
        news("n-d", NEWSWIRE, "AAPL", CUT_OFF - 6_000),
    ]);
    let arguments = r#"{"limit":3,"symbols":["AAPL"],"terms":[],"window_h":24}"#;
    let result = one(&fixture, Tool::NewsSearch, arguments);
    assert_eq!(
        shown(&result),
        ids(&["n-a", "n-b", "n-c"]),
        "a tie goes by item id"
    );
    assert!(
        !result.truncated,
        "the model's own limit is not a cap (rule 179)"
    );
}

#[test]
fn news_results_are_spotlighted_canonical_json_with_no_link() {
    let fixture = with_news(vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)]);
    let result = one(&fixture, Tool::NewsSearch, AAPL_NEWS);
    let value = parsed(&result);
    assert_eq!(
        result.content,
        to_canonical(&value),
        "canonical: its own re-serialisation"
    );
    assert_eq!(keys(&value), ["items", "truncated"]);
    let all = items(&value);
    let [first] = all.as_slice() else {
        panic!("one item: {all:?}")
    };
    assert_eq!(keys(first), ["item_id", "source", "untrusted"]);
    assert_eq!(first.get("source").and_then(Value::as_str), Some(NEWSWIRE));
    let wrapped = first
        .get("untrusted")
        .unwrap_or_else(|| panic!("untrusted"));
    let fields = [
        "content_hash",
        "headline",
        "length_bytes",
        "published_at",
        "summary",
    ];
    assert_eq!(
        keys(wrapped),
        fields,
        "§6.3's news fields without `link` (§6.10)"
    );
    assert_eq!(
        wrapped.get("headline").and_then(Value::as_str),
        Some("Headline n-1")
    );
    let text = String::from_utf8(result.content.clone()).unwrap_or_default();
    assert!(!text.contains("https://"), "{text}");
}

#[test]
fn a_news_item_is_cut_to_two_thousand_bytes() {
    let mut long = news("n-long", NEWSWIRE, "AAPL", CUT_OFF - 60);
    long.summary = "é".repeat(3_000);
    let result = one(&with_news(vec![long]), Tool::NewsSearch, AAPL_NEWS);
    let Some(RetrievedItem::News(kept)) = result.items.first() else {
        panic!("one item")
    };
    assert!(
        kept.headline.len() + kept.summary.len() <= 2_000,
        "§6.10's 2,000 bytes per item"
    );
    assert!(
        kept.headline.len() + kept.summary.len() > 1_990,
        "cut only as far as the cap"
    );
    assert!(
        kept.summary.chars().all(|c| c == 'é'),
        "cut on a character boundary"
    );
    assert!(result.truncated, "a harness cap removed text");

    let mut exact = news("n-exact", NEWSWIRE, "AAPL", CUT_OFF - 60);
    exact.summary = "s".repeat(2_000 - exact.headline.len());
    let result = one(&with_news(vec![exact.clone()]), Tool::NewsSearch, AAPL_NEWS);
    assert_eq!(
        result.items,
        vec![RetrievedItem::News(exact)],
        "2,000 bytes is within the cap"
    );
    assert!(!result.truncated);
}

#[test]
fn the_byte_caps_drop_the_oldest_news_first() {
    let fixture = with_news((0..10).map(wide).collect());
    let bounds = LoopBounds {
        max_result_bytes: 5_000,
        ..LoopBounds::v0_2()
    };
    let dispatched = run(
        &fixture,
        &[call(0, Tool::NewsSearch, AAPL_NEWS)],
        bounds,
        96_000,
    );
    let result = dispatched
        .results
        .first()
        .unwrap_or_else(|| panic!("one result"));
    assert!(result.content.len() <= 5_000 && result.truncated);
    let kept = shown(result);
    let newest: Vec<String> = (0..10)
        .rev()
        .take(kept.len())
        .map(|n| format!("n-{n:02}"))
        .collect();
    assert!(!kept.is_empty() && kept == newest, "{kept:?}");
    assert_eq!(observed(result), kept);

    let calls = [
        call(0, Tool::NewsSearch, AAPL_NEWS),
        call(1, Tool::NewsSearch, AAPL_NEWS),
    ];
    let dispatched = run(&fixture, &calls, LoopBounds::v0_2(), 4_000);
    let [first, second] = dispatched.results.as_slice() else {
        panic!("two results")
    };
    assert!(first.content.len() <= 4_000 && first.truncated);
    assert_eq!(second.content, br#"{"items":[],"truncated":true}"#.to_vec());
    let spent = u32::try_from(first.content.len() + second.content.len()).unwrap_or(u32::MAX);
    assert_eq!(
        dispatched.result_bytes_left,
        4_000_u32.saturating_sub(spent)
    );
}

#[test]
fn news_search_arguments_outside_the_pinned_schema_read_nothing() {
    let fixture = Fixture::default();
    let search = |symbols: &str, terms: &str, window: u32, limit: u32| {
        format!(r#"{{"limit":{limit},"symbols":{symbols},"terms":{terms},"window_h":{window}}}"#)
    };
    let aapl = r#"["AAPL"]"#;
    let five = r#"["AAPL","AAPL","AAPL","AAPL","AAPL"]"#;
    let six = r#"["AAPL","AAPL","AAPL","AAPL","AAPL","AAPL"]"#;
    let widest = format!(r#"["{}"]"#, "t".repeat(32));
    let longer = format!(r#"["{}"]"#, "t".repeat(33));
    for (arguments, read) in [
        (search(five, "[]", 24, 5), true),
        (search(aapl, r#"["a","b","c","d","e"]"#, 24, 5), true),
        (search(aapl, &widest, 24, 5), true),
        (search(aapl, r#"["Q3 2026 v1.2-a"]"#, 24, 5), true),
        (search(aapl, "[]", 1, 1), true),
        (search(aapl, "[]", 168, 20), true),
        (search("[]", "[]", 24, 5), false),
        (search(six, "[]", 24, 5), false),
        (search(aapl, r#"["a","b","c","d","e","f"]"#, 24, 5), false),
        (search(aapl, &longer, 24, 5), false),
        (search(aapl, r#"[""]"#, 24, 5), false),
        (search(aapl, r#"["!"]"#, 24, 5), false),
        (search(aapl, r#"["é"]"#, 24, 5), false),
        (search(aapl, "[]", 0, 5), false),
        (search(aapl, "[]", 169, 5), false),
        (search(aapl, "[]", 24, 0), false),
        (search(aapl, "[]", 24, 21), false),
        ("{".to_owned(), false),
    ] {
        let before = fixture.calls.get();
        let result = one(&fixture, Tool::NewsSearch, &arguments);
        assert_eq!(
            verdict(&result),
            if read { OK } else { FAILED },
            "{arguments}"
        );
        assert_eq!(fixture.calls.get() - before, u32::from(read), "{arguments}");
    }
}

#[test]
fn a_news_search_outside_the_universe_or_unanswered_reads_nothing() {
    let fixture = Fixture::default();
    let tsla = r#"{"limit":5,"symbols":["AAPL","TSLA"],"terms":[],"window_h":24}"#;
    let result = one(&fixture, Tool::NewsSearch, tsla);
    assert_eq!(
        verdict(&result),
        (ToolOutcome::Refused, Some(Code::OutOfUniverse))
    );
    assert_eq!(result.content, br#"{"error":"out_of_universe"}"#.to_vec());
    assert_eq!(fixture.calls.get(), 0, "refused whole, before any read");

    let failing = Fixture {
        failing: true,
        ..Fixture::default()
    };
    let result = one(&failing, Tool::NewsSearch, AAPL_NEWS);
    assert_eq!(verdict(&result), FAILED);
    assert_eq!(result.content, br#"{"error":"unavailable"}"#.to_vec());
    assert!(result.items.is_empty() && !result.untrusted && !result.truncated);
}

#[test]
fn a_reader_that_cannot_match_terms_reads_nothing() {
    struct Bare;
    impl mandate_research_run::NewsReader for Bare {
        fn news(
            &self,
            _: &mandate_research_run::TextQuery,
        ) -> Result<Vec<NewsItem>, mandate_research_run::ReadError> {
            Ok(Vec::new())
        }
    }
    let fixture = Fixture::default();
    let readers = mandate_research_run::Readers {
        news: &Bare,
        ..fixture.readers()
    };
    let scope = scope();
    let bounds = LoopBounds::v0_2();
    let within = DispatchScope {
        scope: &scope,
        bounds: &bounds,
        result_bytes_left: 96_000,
    };
    let calls = [call(0, Tool::NewsSearch, AAPL_NEWS)];
    let got = dispatch(TURN, &calls, within, readers);
    let got = got.unwrap_or_else(|e| panic!("{e:?}"));
    let verdicts: Vec<_> = got.results.iter().map(verdict).collect();
    assert_eq!(
        verdicts,
        [FAILED],
        "the port's default reads nothing (rule 3)"
    );
    let result = got.results.first().unwrap_or_else(|| panic!("one result"));
    assert_eq!(
        result.call,
        ToolCallId {
            turn: TURN,
            index: 0
        }
    );
}

/// The property's oracle, from the inputs alone: within budget, the newest eligible items newest
/// first, observed as shown, wrapped as untrusted, `truncated` exactly when a cap removed
/// something, and the run's bytes lowered by exactly the result's length. `None`, or the breach.
fn breach(rows: &[(i64, usize)], cap: u32, left: u32, got: &Dispatch) -> Option<String> {
    let [result] = got.results.as_slice() else {
        return Some(format!("{} results for one call", got.results.len()));
    };
    let kept = shown(result);
    let budget = usize::try_from(cap.min(left)).unwrap_or(usize::MAX);
    let mut eligible: Vec<(i64, String, usize)> = (0..)
        .zip(rows)
        .filter(|(_, (offset, _))| *offset <= 0 && *offset >= -86_400)
        .map(|(n, (offset, size)): (u32, _)| (*offset, format!("n-{n:02}"), *size))
        .collect();
    eligible.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    eligible.truncate(20);
    let cut = eligible
        .iter()
        .any(|(_, _, size)| "Headline n-00".len() + size > 2_000);
    let newest: Vec<String> = eligible.iter().map(|(_, id, _)| id.clone()).collect();
    let spent = u32::try_from(result.content.len()).unwrap_or(u32::MAX);
    let wrapped = items(&parsed(result))
        .iter()
        .all(|item| item.as_object().is_some() && keys(item) == ["item_id", "source", "untrusted"]);
    if !kept.is_empty() && result.content.len() > budget {
        Some(format!("{} bytes over {budget}", result.content.len()))
    } else if kept.as_slice() != newest.get(..kept.len()).unwrap_or_default() {
        Some(format!("{kept:?} is not the newest of {newest:?}"))
    } else if observed(result) != kept {
        Some("observes other than it shows".to_owned())
    } else if !wrapped {
        Some("an item is not wrapped as untrusted".to_owned())
    } else if result.truncated != (kept.len() < newest.len() || cut) {
        Some(format!(
            "truncated {} with {} of {}",
            result.truncated,
            kept.len(),
            newest.len()
        ))
    } else if got.result_bytes_left != left.saturating_sub(spent) {
        Some(format!(
            "{} bytes left, not {}",
            got.result_bytes_left,
            left.saturating_sub(spent)
        ))
    } else {
        None
    }
}

fn answers(rows: &[(i64, usize)]) -> Vec<NewsItem> {
    (0..)
        .zip(rows)
        .map(|(n, (offset, size)): (u32, _)| {
            let mut item = news(&format!("n-{n:02}"), NEWSWIRE, "AAPL", CUT_OFF + offset);
            item.summary = "s".repeat(*size);
            item
        })
        .collect()
}

/// Random news answers and byte budgets against [`breach`]'s oracle.
#[test]
fn random_news_reads_hold_the_caps_the_window_and_the_cut_off() {
    let strategy = (
        proptest::collection::btree_map(-90_000_i64..600, 0_usize..2_500, 0..40),
        64_u32..30_000,
        0_u32..40_000,
    );
    let mut runner = TestRunner::deterministic();
    for _ in 0..64 {
        let tree = strategy
            .new_tree(&mut runner)
            .unwrap_or_else(|e| panic!("{e}"));
        let (rows, cap, left) = tree.current();
        let rows: Vec<(i64, usize)> = rows.into_iter().collect();
        let bounds = LoopBounds {
            max_result_bytes: cap,
            ..LoopBounds::v0_2()
        };
        let calls = [call(0, Tool::NewsSearch, AAPL_NEWS)];
        let got = run(&with_news(answers(&rows)), &calls, bounds, left);
        assert_eq!(
            breach(&rows, cap, left, &got),
            None,
            "{rows:?} {cap} {left}"
        );
    }
}

/// A hand-built result showing the given rows' items, with `truncated` and bytes left as given.
fn seeded(rows: &[(i64, usize)], shown: &[u32], truncated: bool, left: u32) -> Dispatch {
    let all = answers(rows);
    let picked: Vec<NewsItem> = shown
        .iter()
        .filter_map(|n| all.get(usize::try_from(*n).unwrap_or(usize::MAX)))
        .cloned()
        .collect();
    let item = |news: &NewsItem| {
        let (id, from) = (news.item_id.as_str(), news.source.as_str());
        format!(r#"{{"item_id":"{id}","source":"{from}","untrusted":{{}}}}"#)
    };
    let body: Vec<String> = picked.iter().map(item).collect();
    let content = format!(
        r#"{{"items":[{}],"truncated":{truncated}}}"#,
        body.join(",")
    );
    let result = Dispatched {
        call: ToolCallId {
            turn: TURN,
            index: 0,
        },
        outcome: ToolOutcome::Ok,
        code: None,
        content: content.into_bytes(),
        truncated,
        untrusted: true,
        items: picked.into_iter().map(RetrievedItem::News).collect(),
    };
    Dispatch {
        results: vec![result],
        result_bytes_left: left,
    }
}

#[test]
fn the_news_oracle_catches_seeded_breaches() {
    let rows = [(-300, 10), (-200, 10), (-100, 10), (60, 10), (-100_000, 10)];
    let honest = seeded(&rows, &[2, 1, 0], false, 1_000);
    let length = honest
        .results
        .first()
        .map_or(0, |result| result.content.len());
    let left = 1_000 + u32::try_from(length).unwrap_or(0);
    assert_eq!(
        breach(&rows, 30_000, left, &honest),
        None,
        "the honest result passes"
    );
    let mut renamed = honest.clone();
    renamed.results.iter_mut().for_each(|result| {
        let text = String::from_utf8_lossy(&result.content).replace("untrusted", "data");
        result.content = text.into_bytes();
    });
    let long = [(-300, 2_500), (-200, 10)];
    let long_left = 1_000
        + u32::try_from(seeded(&long, &[1, 0], false, 0).results[0].content.len()).unwrap_or(0);
    for (what, got, rows, cap, left) in [
        (
            "after the cut-off",
            seeded(&rows, &[3, 2, 1], false, 1_000),
            &rows[..],
            30_000,
            left,
        ),
        (
            "before the window",
            seeded(&rows, &[2, 1, 0, 4], false, 1_000),
            &rows[..],
            30_000,
            left,
        ),
        (
            "oldest first",
            seeded(&rows, &[0, 1, 2], false, 1_000),
            &rows[..],
            30_000,
            left,
        ),
        (
            "not the newest",
            seeded(&rows, &[1, 0], true, 1_000),
            &rows[..],
            30_000,
            left,
        ),
        (
            "truncated unset",
            seeded(&rows, &[2, 1], false, 1_000),
            &rows[..],
            30_000,
            left,
        ),
        (
            "a cut text unmarked",
            seeded(&long, &[1, 0], false, 1_000),
            &long[..],
            30_000,
            long_left,
        ),
        (
            "bytes left wrong",
            seeded(&rows, &[2, 1, 0], false, 999),
            &rows[..],
            30_000,
            left,
        ),
        (
            "over its budget",
            seeded(&rows, &[2, 1, 0], false, 1_000),
            &rows[..],
            64,
            left,
        ),
        (
            "its data not under untrusted",
            renamed,
            &rows[..],
            30_000,
            left,
        ),
    ] {
        assert!(
            breach(rows, cap, left, &got).is_some(),
            "the oracle misses: {what}"
        );
    }
}

#[test]
fn the_news_reads_are_untrusted_only_when_they_hold_third_party_text() {
    let fixture = with_news(vec![news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60)]);
    let result = one(&fixture, Tool::NewsSearch, AAPL_NEWS);
    assert!(result.untrusted, "an ok news read holds third-party text");
    let refused = one(
        &fixture,
        Tool::NewsSearch,
        r#"{"limit":5,"symbols":["TSLA"],"terms":[],"window_h":24}"#,
    );
    assert!(!refused.untrusted, "a refusal holds no text");
}

/// A search with no `terms` member reads nothing, at once. It runs first among the news tests'
/// cheap checks, so a mutant that refuses every term set fails fast rather than near a timeout.
#[test]
fn a_news_search_with_no_terms_member_reads_nothing() {
    let fixture = Fixture::default();
    let result = one(
        &fixture,
        Tool::NewsSearch,
        r#"{"limit":5,"symbols":["AAPL"],"window_h":24}"#,
    );
    assert_eq!(verdict(&result), FAILED);
    assert_eq!(
        fixture.calls.get(),
        0,
        "nothing is read without the pinned arguments"
    );
    let empty = one(
        &fixture,
        Tool::NewsSearch,
        r#"{"limit":5,"symbols":["AAPL"],"terms":[],"window_h":24}"#,
    );
    assert_eq!(
        verdict(&empty),
        OK,
        "an empty terms list is §6.10's 0 to 5 terms (DEC-2213 item 1)"
    );
}
