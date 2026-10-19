//! A dispatch's `news.get` (agent harness spec v0.2 §6.10; DEC-2213 items 6 to 8; backlog E19-13
//! slice 3b): it reads one received item's stored text through the reader's `news_text`, as of
//! the run's cut-off and allowlist version, cut to 8,000 bytes and wrapped as untrusted. An answer
//! that leaks (another item, a later one, off the allowlist, licensed) reads nothing, and so does a
//! reader that stores no text.

mod common;

use common::{BLOG, CUT_OFF, Fixture, NEWSWIRE, at, item, news, scope};
use mandate_canon::{Digest, Int, Value, to_canonical};
use mandate_research_run::agentic::{Code, LoopBounds, Tool, ToolCall, ToolCallId, ToolOutcome};
use mandate_research_run::{
    Dispatch, DispatchScope, Dispatched, ItemId, NewsItem, NewsReader, NewsText, ReadError,
    Readers, RetrievedItem, TextQuery, dispatch,
};
use mandate_time::UtcNanos;

const TURN: u32 = 4;
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

fn run(readers: Readers<'_>, calls: &[ToolCall]) -> Dispatch {
    let scope = scope();
    let bounds = LoopBounds::v0_2();
    let within = DispatchScope {
        scope: &scope,
        bounds: &bounds,
        result_bytes_left: 96_000,
    };
    dispatch(TURN, calls, within, readers).unwrap_or_else(|e| panic!("{e:?}"))
}

fn one(fixture: &Fixture, tool: Tool, arguments: &str) -> Dispatched {
    let mut dispatched = run(fixture.readers(), &[call(0, tool, arguments)]);
    assert_eq!(dispatched.results.len(), 1, "one call, one result");
    dispatched.results.remove(0)
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

fn with_text(item: NewsItem, text: &str) -> Fixture {
    let texts = vec![NewsText {
        item,
        text: text.to_owned(),
    }];
    Fixture {
        texts,
        ..Fixture::default()
    }
}

#[test]
fn news_get_reads_the_stored_text_of_the_item_asked() {
    let fixture = with_text(
        news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60),
        "The full text.",
    );
    let result = one(&fixture, Tool::NewsGet, r#"{"item":"n-1"}"#);
    assert_eq!(verdict(&result), OK);
    assert_eq!(
        *fixture.text_asks.borrow(),
        vec![(item("n-1"), at(CUT_OFF), 3)]
    );
    let value = parsed(&result);
    assert_eq!(result.content, to_canonical(&value));
    assert_eq!(keys(&value), ["items", "truncated"]);
    let all = items(&value);
    let [first] = all.as_slice() else {
        panic!("one item: {all:?}")
    };
    assert_eq!(keys(first), ["item_id", "source", "untrusted"]);
    let wrapped = first
        .get("untrusted")
        .unwrap_or_else(|| panic!("untrusted"));
    let fields = [
        "content_hash",
        "headline",
        "length_bytes",
        "published_at",
        "text",
    ];
    assert_eq!(keys(wrapped), fields);
    let field = |name: &str| wrapped.get(name).cloned();
    assert_eq!(field("text"), Some(Value::Str("The full text.".to_owned())));
    assert_eq!(
        field("headline"),
        Some(Value::Str("Headline n-1".to_owned()))
    );
    assert_eq!(
        field("published_at"),
        Some(Value::Str(at(CUT_OFF - 60).to_string()))
    );
    assert_eq!(
        field("content_hash"),
        Some(Value::Str(format!("sha256:{}", Digest::of(b"n-1"))))
    );
    let record = "Headline n-1".len() + "Summary of n-1.".len();
    let record = u64::try_from(record).unwrap_or(u64::MAX);
    assert_eq!(
        field("length_bytes"),
        Int::new(record).map(Value::Int),
        "the length is the observed record's, as the search reports it"
    );
    assert_eq!(
        (first.get("item_id").cloned(), first.get("source").cloned()),
        (
            Some(Value::Str("n-1".to_owned())),
            Some(Value::Str(NEWSWIRE.to_owned()))
        )
    );
    assert_eq!(
        observed(&result),
        ids(&["n-1"]),
        "the item is observed as its own record"
    );
    assert!(result.untrusted && !result.truncated);
}

#[test]
fn news_get_text_is_cut_to_eight_thousand_bytes() {
    let item = news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60);
    let result = one(
        &with_text(item.clone(), &"é".repeat(5_000)),
        Tool::NewsGet,
        r#"{"item":"n-1"}"#,
    );
    let value = parsed(&result);
    let all = items(&value);
    let text = all
        .first()
        .and_then(|i| i.get("untrusted"))
        .and_then(|u| u.get("text"));
    let text = text.and_then(Value::as_str).unwrap_or_default().to_owned();
    assert_eq!(text, "é".repeat(4_000), "§6.10's 8,000 bytes");
    assert!(result.truncated);

    let offset = format!("a{}", "é".repeat(5_000));
    let result = one(
        &with_text(item.clone(), &offset),
        Tool::NewsGet,
        r#"{"item":"n-1"}"#,
    );
    let value = parsed(&result);
    let shown = items(&value)
        .first()
        .and_then(|i| i.get("untrusted"))
        .and_then(|u| u.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned();
    assert_eq!(
        shown,
        format!("a{}", "é".repeat(3_999)),
        "the 8,000th byte is inside a character, so the cut keeps 7,999 bytes"
    );
    assert!(result.truncated);

    let exact = "t".repeat(8_000);
    let result = one(&with_text(item, &exact), Tool::NewsGet, r#"{"item":"n-1"}"#);
    assert!(!result.truncated, "8,000 bytes is within the cap");
}

#[test]
fn news_get_reads_nothing_from_a_reader_that_leaks_or_from_bad_arguments() {
    let mut licensed = news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60);
    licensed.licensed = true;
    for (answered, why) in [
        (
            news("n-1", NEWSWIRE, "AAPL", CUT_OFF + 1),
            "after the cut-off",
        ),
        (news("n-1", BLOG, "AAPL", CUT_OFF - 60), "off the allowlist"),
        (licensed, "licensed text"),
        (news("n-2", NEWSWIRE, "AAPL", CUT_OFF - 60), "another item"),
    ] {
        let fixture = with_text(answered, "leaked");
        let result = one(&fixture, Tool::NewsGet, r#"{"item":"n-1"}"#);
        assert_eq!(verdict(&result), FAILED, "{why}");
        assert!(result.items.is_empty(), "{why}");
        assert!(
            !String::from_utf8_lossy(&result.content).contains("leaked"),
            "{why}"
        );
    }
    let at_cut_off = with_text(
        news("n-1", NEWSWIRE, "AAPL", CUT_OFF),
        "known at the cut-off",
    );
    let result = one(&at_cut_off, Tool::NewsGet, r#"{"item":"n-1"}"#);
    assert_eq!(
        verdict(&result),
        OK,
        "an item known at the cut-off is not look-ahead (DP-1)"
    );
    let fixture = with_text(news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60), "text");
    for arguments in [
        "{}",
        r#"{"item":""}"#,
        r#"{"item":7}"#,
        r#"{"item":"n 1"}"#,
        "{",
    ] {
        assert_eq!(
            verdict(&one(&fixture, Tool::NewsGet, arguments)),
            FAILED,
            "{arguments}"
        );
    }
    assert!(
        fixture.text_asks.borrow().is_empty(),
        "no reader is asked for what the schema forbids"
    );
}

#[test]
fn a_reader_with_no_stored_text_answers_unavailable() {
    struct Bare;
    impl NewsReader for Bare {
        fn news(&self, _: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
            Ok(Vec::new())
        }
    }
    let fixture = Fixture::default();
    let readers = Readers {
        news: &Bare,
        ..fixture.readers()
    };
    let got = run(readers, &[call(2, Tool::NewsGet, r#"{"item":"n-1"}"#)]);
    let result = got.results.first().unwrap_or_else(|| panic!("one result"));
    assert_eq!(
        verdict(result),
        FAILED,
        "the port's default reads nothing (rule 3)"
    );
    assert_eq!(
        result.call,
        ToolCallId {
            turn: TURN,
            index: 2
        }
    );
    assert_eq!(
        got.result_bytes_left,
        96_000 - 23,
        "its error counts as the read's result"
    );
}

#[test]
fn a_reader_that_refuses_the_text_reads_nothing() {
    struct Refusing;
    impl NewsReader for Refusing {
        fn news(&self, _: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
            Ok(Vec::new())
        }
        fn news_text(&self, _: &ItemId, _: UtcNanos, _: u32) -> Result<NewsText, ReadError> {
            Err(ReadError::Refused {
                reason: "leaked reason".to_owned(),
            })
        }
    }
    let fixture = Fixture::default();
    let readers = Readers {
        news: &Refusing,
        ..fixture.readers()
    };
    let got = run(readers, &[call(0, Tool::NewsGet, r#"{"item":"n-1"}"#)]);
    let result = got.results.first().unwrap_or_else(|| panic!("one result"));
    assert_eq!(verdict(result), FAILED, "a refused read is an empty read");
    assert!(result.items.is_empty());
    assert_eq!(result.content, br#"{"error":"unavailable"}"#.to_vec());
}

/// The port's default body, called directly rather than through the pending dispatch, so the
/// tests PR's own mutants run already holds it to reading nothing (rule 3).
#[test]
fn the_news_text_default_answers_unavailable() {
    struct Bare;
    impl NewsReader for Bare {
        fn news(&self, _: &TextQuery) -> Result<Vec<NewsItem>, ReadError> {
            Ok(Vec::new())
        }
    }
    let answered = Bare.news_text(&item("n-1"), at(CUT_OFF), 3);
    assert_eq!(answered, Err(ReadError::Unavailable));
}
