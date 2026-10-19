//! A dispatch's `filings.get` (agent harness spec v0.2 §6.10; DEC-2250 items 6 to 9; DEC-2252
//! items 2 to 4; backlog E19-13 slice 3b): it reads a range of one received filing's stored text
//! through the reader's `filing_text`, as of the run's cut-off and allowlist version, and shows it
//! with the filing's fields under `untrusted`. A range past the end is refused `unknown_item`; an
//! excerpt-only filing, any other reader error, and an answer that leaks or oversteps read nothing.

mod common;

use common::{BLOG, CUT_OFF, EDGAR, Fixture, at, filing, item, scope, source};
use mandate_canon::{Digest, Int, Value, to_canonical};
use mandate_research_run::agentic::{Code, LoopBounds, Tool, ToolCall, ToolOutcome};
use mandate_research_run::{
    DispatchScope, Dispatched, Filing, FilingReader, FilingText, ItemId, ReadError, Readers,
    RetrievedItem, TextQuery, dispatch,
};
use mandate_time::UtcNanos;

const TURN: u32 = 6;
const ASK: &str = r#"{"item":"f-1","length":500,"offset":100}"#;
const FAILED: (ToolOutcome, Option<Code>) = (ToolOutcome::Failed, Some(Code::Unavailable));
const OK: (ToolOutcome, Option<Code>) = (ToolOutcome::Ok, None);

fn run(readers: Readers<'_>, arguments: &str) -> Dispatched {
    let scope = scope();
    let bounds = LoopBounds::v0_2();
    let within = DispatchScope {
        scope: &scope,
        bounds: &bounds,
        result_bytes_left: 96_000,
    };
    let calls = [ToolCall {
        index: 0,
        tool: Tool::FilingsGet,
        arguments: arguments.to_owned(),
    }];
    let mut got = dispatch(TURN, &calls, within, readers).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(got.results.len(), 1, "one call, one result");
    got.results.remove(0)
}

fn answering(answer: Result<FilingText, ReadError>) -> Fixture {
    Fixture {
        filing_text: Some(answer),
        ..Fixture::default()
    }
}

fn range(record: Filing, offset: u64, text: &str) -> FilingText {
    FilingText {
        filing: record,
        stored_bytes: 12_345,
        offset,
        text: text.to_owned(),
    }
}

fn verdict(result: &Dispatched) -> (ToolOutcome, Option<Code>) {
    (result.outcome, result.code)
}

fn keys(value: &Value) -> Vec<&str> {
    let object = value
        .as_object()
        .unwrap_or_else(|| panic!("an object: {value:?}"));
    object.keys().map(|key| key.as_str()).collect()
}

fn read_nothing(result: &Dispatched, leaked: &str, why: &str) {
    assert_eq!(verdict(result), FAILED, "{why}");
    assert!(result.items.is_empty() && !result.untrusted, "{why}");
    let content = String::from_utf8_lossy(&result.content);
    assert!(!content.contains(leaked), "{why}: {content}");
}

#[test]
fn filings_get_reads_the_range_asked_and_shows_it_under_untrusted() {
    let record = filing("f-1", "AAPL", CUT_OFF - 60);
    let fixture = answering(Ok(range(record.clone(), 100, "The range.")));
    let result = run(fixture.readers(), ASK);
    assert_eq!(verdict(&result), OK);
    assert_eq!(
        *fixture.range_asks.borrow(),
        vec![(item("f-1"), at(CUT_OFF), 3, 100, 500)],
        "the id, the cut-off, the allowlist version, the offset and the length, and nothing else"
    );
    let value = mandate_canon::parse(&result.content).unwrap_or_else(|e| panic!("JSON: {e:?}"));
    assert_eq!(result.content, to_canonical(&value));
    assert_eq!(keys(&value), ["items", "truncated"]);
    let all = value
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_default();
    let [first] = all else {
        panic!("one item: {all:?}")
    };
    assert_eq!(keys(first), ["item_id", "source", "untrusted"]);
    let text = |s: &str| Some(Value::Str(s.to_owned()));
    assert_eq!(first.get("item_id").cloned(), text("f-1"));
    assert_eq!(first.get("source").cloned(), text(EDGAR));
    let wrapped = first
        .get("untrusted")
        .unwrap_or_else(|| panic!("untrusted"));
    let fields = [
        "content_hash",
        "filed_at",
        "form",
        "length_bytes",
        "offset",
        "stored_bytes",
        "text",
    ];
    assert_eq!(keys(wrapped), fields, "DEC-2252 item 3");
    let field = |name: &str| wrapped.get(name).cloned();
    assert_eq!(field("text"), text("The range."));
    assert_eq!(field("form"), text("8-K"));
    assert_eq!(field("filed_at"), text(&at(CUT_OFF - 60).to_string()));
    assert_eq!(
        field("content_hash"),
        text(&format!("sha256:{}", Digest::of(b"f-1")))
    );
    let record_length = u64::try_from("Excerpt of f-1.".len()).unwrap_or(u64::MAX);
    assert_eq!(
        field("length_bytes"),
        Int::new(record_length).map(Value::Int)
    );
    assert_eq!(field("offset"), Int::new(100).map(Value::Int));
    assert_eq!(field("stored_bytes"), Int::new(12_345).map(Value::Int));
    assert_eq!(
        result.items,
        vec![RetrievedItem::Filing(record)],
        "the filing is observed as its own record"
    );
    assert!(result.untrusted && !result.truncated);
}

#[test]
fn a_range_is_at_most_the_length_asked_and_never_cut_to_it() {
    let record = filing("f-1", "AAPL", CUT_OFF - 60);
    let ask = r#"{"item":"f-1","length":10,"offset":0}"#;
    let exact = answering(Ok(range(record.clone(), 0, &"é".repeat(5))));
    let result = run(exact.readers(), ask);
    assert_eq!(verdict(&result), OK, "exactly `length` bytes is within it");
    assert!(
        !result.truncated,
        "the length is the model's own bound, not a cap"
    );

    let over = answering(Ok(range(record, 0, &format!("{}x", "é".repeat(5)))));
    read_nothing(&run(over.readers(), ask), "é", "one byte over the length");
}

#[test]
fn the_readers_two_answers_are_kept_apart() {
    let result = run(answering(Err(ReadError::OutOfRange)).readers(), ASK);
    assert_eq!(
        verdict(&result),
        (ToolOutcome::Refused, Some(Code::UnknownItem)),
        "a range past the end is the model's mistake (DEC-2250 item 7)"
    );
    assert_eq!(result.content, br#"{"error":"unknown_item"}"#.to_vec());
    assert!(result.items.is_empty());

    for (error, why) in [
        (ReadError::ExcerptOnly, "an excerpt-only filing"),
        (ReadError::Unavailable, "an unavailable store"),
        (
            ReadError::Refused {
                reason: "leaked reason".to_owned(),
            },
            "a refused read",
        ),
    ] {
        let result = run(answering(Err(error)).readers(), ASK);
        read_nothing(&result, "leaked", why);
        assert_eq!(
            result.content,
            br#"{"error":"unavailable"}"#.to_vec(),
            "{why}"
        );
    }
}

#[test]
fn a_filings_get_answer_that_leaks_reads_nothing() {
    let mut licensed = filing("f-1", "AAPL", CUT_OFF - 60);
    licensed.licensed = true;
    let mut blog = filing("f-1", "AAPL", CUT_OFF - 60);
    blog.source = source(BLOG);
    for (answer, why) in [
        (
            range(filing("f-2", "AAPL", CUT_OFF - 60), 100, "leaked"),
            "another item",
        ),
        (
            range(filing("f-1", "AAPL", CUT_OFF + 1), 100, "leaked"),
            "after the cut-off",
        ),
        (range(blog, 100, "leaked"), "off the allowlist"),
        (range(licensed, 100, "leaked"), "licensed text"),
        (
            range(filing("f-1", "AAPL", CUT_OFF - 60), 0, "leaked"),
            "a range at another offset",
        ),
    ] {
        read_nothing(&run(answering(Ok(answer)).readers(), ASK), "leaked", why);
    }
    let at_cut_off = answering(Ok(range(filing("f-1", "AAPL", CUT_OFF), 100, "known")));
    assert_eq!(
        verdict(&run(at_cut_off.readers(), ASK)),
        OK,
        "a filing known at the cut-off is not look-ahead (DP-1)"
    );
}

#[test]
fn filings_get_arguments_outside_the_pinned_schema_ask_no_reader() {
    let record = filing("f-1", "AAPL", CUT_OFF - 60);
    let widest = answering(Ok(range(record, 4_000_000_000, "far in")));
    let far = r#"{"item":"f-1","length":8000,"offset":4000000000}"#;
    assert_eq!(verdict(&run(widest.readers(), far)), OK, "{far}");
    assert_eq!(
        *widest.range_asks.borrow(),
        vec![(item("f-1"), at(CUT_OFF), 3, 4_000_000_000, 8_000)]
    );

    let fixture = answering(Ok(range(filing("f-1", "AAPL", CUT_OFF - 60), 0, "text")));
    for arguments in [
        "{}",
        r#"{"item":"f-1","length":100}"#,
        r#"{"item":"f-1","offset":0}"#,
        r#"{"item":"","length":100,"offset":0}"#,
        r#"{"item":7,"length":100,"offset":0}"#,
        r#"{"item":"f 1","length":100,"offset":0}"#,
        r#"{"item":"f-1","length":100,"offset":-1}"#,
        r#"{"item":"f-1","length":100,"offset":"0"}"#,
        r#"{"item":"f-1","length":0,"offset":0}"#,
        r#"{"item":"f-1","length":8001,"offset":0}"#,
        "{",
    ] {
        assert_eq!(
            verdict(&run(fixture.readers(), arguments)),
            FAILED,
            "{arguments}"
        );
    }
    assert!(
        fixture.range_asks.borrow().is_empty(),
        "no reader is asked for what the schema forbids"
    );
}

#[test]
fn a_reader_with_no_stored_filing_text_reads_nothing() {
    let fixture = Fixture::default();
    let readers = Readers {
        filings: &Bare,
        ..fixture.readers()
    };
    assert_eq!(
        verdict(&run(readers, ASK)),
        FAILED,
        "the port's default reads nothing (rule 3)"
    );
}

/// A filings reader that serves only the seed's fixed read.
struct Bare;

impl FilingReader for Bare {
    fn filings(&self, _: &TextQuery) -> Result<Vec<Filing>, ReadError> {
        Ok(Vec::new())
    }
}

/// The port's default body, called directly rather than through the pending dispatch, so the
/// tests PR's own mutants run already holds it to reading nothing (rule 3).
#[test]
fn the_filing_text_default_answers_unavailable() {
    let id: ItemId = item("f-1");
    let cut_off: UtcNanos = at(CUT_OFF);
    assert_eq!(
        Bare.filing_text(&id, cut_off, 3, 0, 100),
        Err(ReadError::Unavailable)
    );
}
