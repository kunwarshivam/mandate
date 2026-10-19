//! A dispatch's filings search (agent harness spec v0.2 §6.10; DEC-2250 items 1 to 5 and 9;
//! backlog E19-13 slice 3b): `filings.search` takes its scope from the harness and only its forms
//! and terms from the model, every answer is filtered again on knowledge time, the window, the
//! allowlist, licensing, scope, and form, and entries come newest first with their stored length,
//! each excerpt cut to 4,000 bytes and wrapped as untrusted. `filings.get` follows in the next pair.

mod common;

use common::{BLOG, CUT_OFF, DAY, EDGAR, Fixture, at, filing, scope, source, symbol};
use mandate_canon::{Digest, Int, Value, to_canonical};
use mandate_research_run::agentic::{Code, LoopBounds, Tool, ToolCall, ToolOutcome};
use mandate_research_run::{
    Dispatch, DispatchScope, Dispatched, Filing, FilingEntry, FilingReader, ReadError, Readers,
    RetrievedItem, TextQuery, dispatch,
};

const TURN: u32 = 5;
/// DEC-2250 item 1's pinned list, in its order.
const LIST: [&str; 6] = ["10-K", "10-K/A", "10-Q", "10-Q/A", "8-K", "8-K/A"];
const AAPL_FILINGS: &str = r#"{"forms":[],"limit":10,"symbols":["AAPL"],"terms":[],"window_d":30}"#;
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

fn run(readers: Readers<'_>, calls: &[ToolCall], bounds: LoopBounds) -> Dispatch {
    let scope = scope();
    let within = DispatchScope {
        scope: &scope,
        bounds: &bounds,
        result_bytes_left: 96_000,
    };
    dispatch(TURN, calls, within, readers).unwrap_or_else(|e| panic!("{e:?}"))
}

fn one(fixture: &Fixture, arguments: &str) -> Dispatched {
    let calls = [call(0, Tool::FilingsSearch, arguments)];
    let mut dispatched = run(fixture.readers(), &calls, LoopBounds::v0_2());
    assert_eq!(dispatched.results.len(), 1, "one call, one result");
    dispatched.results.remove(0)
}

fn filed(id: &str, form: &str, about: &str, knowledge_at: i64) -> FilingEntry {
    let mut record = filing(id, about, knowledge_at);
    record.form = form.to_owned();
    FilingEntry {
        filing: record,
        stored_bytes: 12_345,
    }
}

fn with_filings(filing_entries: Vec<FilingEntry>) -> Fixture {
    Fixture {
        filing_entries,
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

fn excerpt(result: &Dispatched) -> String {
    let all = items(&parsed(result));
    let text = all
        .first()
        .and_then(|item| item.get("untrusted"))
        .and_then(|fields| fields.get("excerpt"))
        .and_then(Value::as_str);
    text.unwrap_or_default().to_owned()
}

#[test]
fn filings_search_takes_its_scope_from_the_harness_and_its_forms_and_terms_from_the_model() {
    let fixture = Fixture::default();
    let arguments = r#"{"forms":["8-K","10-Q"],"limit":4,"symbols":["AAPL","MSFT"],"terms":["guidance"],"window_d":30}"#;
    assert_eq!(verdict(&one(&fixture, arguments)), OK);
    let queries = fixture.queries.borrow();
    let [query] = queries.as_slice() else {
        panic!("one filings query: {queries:?}")
    };
    assert_eq!(query.workspace, "ws1");
    assert_eq!(
        (query.cut_off, query.window_start),
        (at(CUT_OFF), at(CUT_OFF - 30 * DAY))
    );
    assert_eq!(query.allowlist_version, 3);
    assert_eq!(
        query.symbols,
        [symbol("AAPL"), symbol("MSFT")].into_iter().collect()
    );
    assert_eq!(query.max_items, 4);
    assert_eq!(
        *fixture.forms.borrow(),
        vec![ids(&["10-Q", "8-K"])],
        "the forms reach the reader in the pinned list's order"
    );
    assert_eq!(*fixture.terms.borrow(), vec![ids(&["guidance"])]);

    let fixture = Fixture::default();
    assert_eq!(verdict(&one(&fixture, AAPL_FILINGS)), OK);
    assert_eq!(
        *fixture.forms.borrow(),
        vec![ids(&LIST)],
        "naming no form passes the whole list (DEC-2250 item 1)"
    );
}

#[test]
fn filings_search_filters_every_answer_again() {
    let mut licensed = filed("f-licensed", "8-K", "AAPL", CUT_OFF - 60);
    licensed.filing.licensed = true;
    let mut blog = filed("f-blog", "8-K", "AAPL", CUT_OFF - 60);
    blog.filing.source = source(BLOG);
    let fixture = with_filings(vec![
        filed("f-kept-old", "10-K", "AAPL", CUT_OFF - 10 * DAY),
        filed("f-kept-new", "8-K", "AAPL", CUT_OFF - 60),
        filed("f-future", "8-K", "AAPL", CUT_OFF + 1),
        filed("f-stale", "8-K", "AAPL", CUT_OFF - 31 * DAY),
        blog,
        filed("f-msft", "8-K", "MSFT", CUT_OFF - 60),
        filed("bar:AAPL:1", "8-K", "AAPL", CUT_OFF - 60),
        filed("f-s1", "S-1", "AAPL", CUT_OFF - 60),
        filed("f-kept-new", "8-K", "AAPL", CUT_OFF - 30),
        licensed,
    ]);
    let result = one(&fixture, AAPL_FILINGS);
    assert_eq!(
        shown(&result),
        ids(&["f-kept-new", "f-kept-old"]),
        "after the cut-off, before the window, off the allowlist, out of scope, a reserved id, a \
         form off the list, a second answer for one id, and licensed text are all dropped"
    );
    assert_eq!(observed(&result), shown(&result));
    let Some(RetrievedItem::Filing(first)) = result.items.first() else {
        panic!("a filing")
    };
    assert_eq!(
        first.knowledge_at,
        at(CUT_OFF - 60),
        "the first answer for an id is kept"
    );
    assert!(result.untrusted, "a filing is third-party text");

    let fixture = with_filings(vec![
        filed("f-8k", "8-K", "AAPL", CUT_OFF - 60),
        filed("f-10q", "10-Q", "AAPL", CUT_OFF - 120),
    ]);
    let arguments = r#"{"forms":["10-Q"],"limit":10,"symbols":["AAPL"],"terms":[],"window_d":30}"#;
    assert_eq!(
        shown(&one(&fixture, arguments)),
        ids(&["f-10q"]),
        "a form the call did not name is dropped"
    );
}

#[test]
fn filings_are_newest_first_then_by_id_up_to_the_models_limit() {
    let fixture = with_filings(vec![
        filed("f-b", "8-K", "AAPL", CUT_OFF - 60),
        filed("f-a", "8-K", "AAPL", CUT_OFF - 60),
        filed("f-c", "8-K", "AAPL", CUT_OFF - 600),
        filed("f-d", "8-K", "AAPL", CUT_OFF - 6_000),
    ]);
    let arguments = r#"{"forms":[],"limit":3,"symbols":["AAPL"],"terms":[],"window_d":30}"#;
    let result = one(&fixture, arguments);
    assert_eq!(
        shown(&result),
        ids(&["f-a", "f-b", "f-c"]),
        "a tie goes by item id"
    );
    assert!(
        !result.truncated,
        "the model's own limit is not a cap (rule 179)"
    );
}

#[test]
fn filing_entries_carry_their_stored_length_under_untrusted() {
    let entry = filed("f-1", "8-K", "AAPL", CUT_OFF - 60);
    let record: Filing = entry.filing.clone();
    let result = one(&with_filings(vec![entry]), AAPL_FILINGS);
    let value = parsed(&result);
    assert_eq!(result.content, to_canonical(&value));
    assert_eq!(keys(&value), ["items", "truncated"]);
    let all = items(&value);
    let [first] = all.as_slice() else {
        panic!("one entry: {all:?}")
    };
    assert_eq!(keys(first), ["item_id", "source", "untrusted"]);
    assert_eq!(
        (first.get("item_id").cloned(), first.get("source").cloned()),
        (
            Some(Value::Str("f-1".to_owned())),
            Some(Value::Str(EDGAR.to_owned()))
        )
    );
    let wrapped = first
        .get("untrusted")
        .unwrap_or_else(|| panic!("untrusted"));
    let fields = [
        "content_hash",
        "excerpt",
        "filed_at",
        "form",
        "length_bytes",
        "stored_bytes",
    ];
    assert_eq!(keys(wrapped), fields, "DEC-2250 item 5");
    let field = |name: &str| wrapped.get(name).cloned();
    let text = |s: &str| Some(Value::Str(s.to_owned()));
    assert_eq!(field("excerpt"), text("Excerpt of f-1."));
    assert_eq!(field("form"), text("8-K"));
    assert_eq!(field("filed_at"), text(&at(CUT_OFF - 60).to_string()));
    assert_eq!(
        field("content_hash"),
        text(&format!("sha256:{}", Digest::of(b"f-1")))
    );
    let length = u64::try_from("Excerpt of f-1.".len()).unwrap_or(u64::MAX);
    assert_eq!(field("length_bytes"), Int::new(length).map(Value::Int));
    assert_eq!(field("stored_bytes"), Int::new(12_345).map(Value::Int));
    assert_eq!(
        result.items,
        vec![RetrievedItem::Filing(record)],
        "the filing is observed as its own record, without the stored length"
    );
    assert!(result.untrusted && !result.truncated);
}

#[test]
fn a_filing_excerpt_is_cut_to_four_thousand_bytes() {
    let mut long = filed("f-1", "8-K", "AAPL", CUT_OFF - 60);
    long.filing.excerpt = "é".repeat(3_000);
    let result = one(&with_filings(vec![long.clone()]), AAPL_FILINGS);
    assert_eq!(excerpt(&result), "é".repeat(2_000), "§6.10's 4,000 bytes");
    assert!(result.truncated, "a harness cap removed text");

    long.filing.excerpt = format!("a{}", "é".repeat(3_000));
    let result = one(&with_filings(vec![long.clone()]), AAPL_FILINGS);
    assert_eq!(
        excerpt(&result),
        format!("a{}", "é".repeat(1_999)),
        "the 4,000th byte is inside a character, so the cut keeps 3,999 bytes"
    );

    long.filing.excerpt = "s".repeat(4_000);
    let result = one(&with_filings(vec![long]), AAPL_FILINGS);
    assert_eq!(excerpt(&result).len(), 4_000);
    assert!(!result.truncated, "4,000 bytes is within the cap");
}

#[test]
fn the_byte_caps_drop_the_oldest_filings_first() {
    let entries = (0..10)
        .map(|n| {
            let mut entry = filed(&format!("f-{n:02}"), "8-K", "AAPL", CUT_OFF - 600 + n);
            entry.filing.excerpt = "x".repeat(1_500);
            entry
        })
        .collect();
    let fixture = with_filings(entries);
    let bounds = LoopBounds {
        max_result_bytes: 5_000,
        ..LoopBounds::v0_2()
    };
    let dispatched = run(
        fixture.readers(),
        &[call(0, Tool::FilingsSearch, AAPL_FILINGS)],
        bounds,
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
        .map(|n| format!("f-{n:02}"))
        .collect();
    assert!(!kept.is_empty() && kept == newest, "{kept:?}");
    assert_eq!(observed(result), kept);
}

#[test]
fn filings_search_arguments_outside_the_pinned_schema_read_nothing() {
    let fixture = Fixture::default();
    let search = |symbols: &str, forms: &str, window: u32, limit: u32, terms: &str| {
        format!(
            r#"{{"forms":{forms},"limit":{limit},"symbols":{symbols},"terms":{terms},"window_d":{window}}}"#
        )
    };
    let aapl = r#"["AAPL"]"#;
    let five = r#"["AAPL","AAPL","AAPL","AAPL","AAPL"]"#;
    let six = r#"["AAPL","AAPL","AAPL","AAPL","AAPL","AAPL"]"#;
    let five_forms = r#"["10-K","10-K/A","10-Q","10-Q/A","8-K"]"#;
    let every_form = r#"["10-K","10-K/A","10-Q","10-Q/A","8-K","8-K/A"]"#;
    for (arguments, read) in [
        (search(five, "[]", 30, 5, "[]"), true),
        (search(aapl, five_forms, 30, 5, "[]"), true),
        (search(aapl, r#"["8-K/A"]"#, 1, 1, r#"["Q3 2026"]"#), true),
        (search(aapl, "[]", 90, 10, "[]"), true),
        (search("[]", "[]", 30, 5, "[]"), false),
        (search(six, "[]", 30, 5, "[]"), false),
        (search(aapl, every_form, 30, 5, "[]"), false),
        (search(aapl, r#"["S-1"]"#, 30, 5, "[]"), false),
        (search(aapl, r#"["8-k"]"#, 30, 5, "[]"), false),
        (search(aapl, r#"["8-K","8-K"]"#, 30, 5, "[]"), false),
        (search(aapl, "[7]", 30, 5, "[]"), false),
        (search(aapl, "[]", 0, 5, "[]"), false),
        (search(aapl, "[]", 91, 5, "[]"), false),
        (search(aapl, "[]", 30, 0, "[]"), false),
        (search(aapl, "[]", 30, 11, "[]"), false),
        (search(aapl, "[]", 30, 5, r#"[""]"#), false),
        (
            search(aapl, "[]", 30, 5, r#"["a","b","c","d","e","f"]"#),
            false,
        ),
        (
            r#"{"limit":5,"symbols":["AAPL"],"terms":[],"window_d":30}"#.to_owned(),
            false,
        ),
        ("{".to_owned(), false),
    ] {
        let before = fixture.calls.get();
        let result = one(&fixture, &arguments);
        assert_eq!(
            verdict(&result),
            if read { OK } else { FAILED },
            "{arguments}"
        );
        assert_eq!(fixture.calls.get() - before, u32::from(read), "{arguments}");
    }
}

#[test]
fn a_filings_search_outside_the_universe_or_unanswered_reads_nothing() {
    let fixture = Fixture::default();
    let tsla = r#"{"forms":[],"limit":5,"symbols":["AAPL","TSLA"],"terms":[],"window_d":30}"#;
    let result = one(&fixture, tsla);
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
    let result = one(&failing, AAPL_FILINGS);
    assert_eq!(verdict(&result), FAILED);
    assert_eq!(result.content, br#"{"error":"unavailable"}"#.to_vec());
    assert!(result.items.is_empty() && !result.untrusted && !result.truncated);

    let readers = Readers {
        filings: &Bare,
        ..fixture.readers()
    };
    let got = run(
        readers,
        &[call(0, Tool::FilingsSearch, AAPL_FILINGS)],
        LoopBounds::v0_2(),
    );
    let verdicts: Vec<_> = got.results.iter().map(verdict).collect();
    assert_eq!(
        verdicts,
        [FAILED],
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
fn the_filings_search_default_answers_unavailable() {
    let query = TextQuery {
        workspace: "ws1".to_owned(),
        symbols: [symbol("AAPL")].into_iter().collect(),
        window_start: at(CUT_OFF - DAY),
        cut_off: at(CUT_OFF),
        allowlist_version: 3,
        max_items: 10,
    };
    let answered = Bare.search(&query, &ids(&LIST), &[]);
    assert_eq!(answered, Err(ReadError::Unavailable));
}

#[test]
fn terms_too_long_or_off_their_alphabet_read_nothing() {
    let fixture = Fixture::default();
    let longer = "t".repeat(33);
    for term in [longer.as_str(), "é", "!", "a_b"] {
        let arguments = format!(
            r#"{{"forms":[],"limit":5,"symbols":["AAPL"],"terms":["{term}"],"window_d":30}}"#
        );
        assert_eq!(verdict(&one(&fixture, &arguments)), FAILED, "{term}");
    }
    assert_eq!(fixture.calls.get(), 0, "no reader is asked");
}

#[test]
fn a_filing_known_at_the_cut_off_or_the_windows_start_is_kept() {
    let fixture = with_filings(vec![
        filed("f-cut-off", "8-K", "AAPL", CUT_OFF),
        filed("f-window-start", "8-K", "AAPL", CUT_OFF - 30 * DAY),
        filed("f-before", "8-K", "AAPL", CUT_OFF - 30 * DAY - 1),
    ]);
    assert_eq!(
        shown(&one(&fixture, AAPL_FILINGS)),
        ids(&["f-cut-off", "f-window-start"]),
        "both edges are inside the window, and a second before it is not (DEC-2252 item 4)"
    );
}

#[test]
fn forms_match_exactly_and_reach_the_reader_in_the_lists_order() {
    let fixture = Fixture::default();
    let case = r#"{"forms":["8-K","8-k"],"limit":5,"symbols":["AAPL"],"terms":[],"window_d":30}"#;
    assert_eq!(
        verdict(&one(&fixture, case)),
        FAILED,
        "a form in another case is off the list"
    );
    assert_eq!(fixture.calls.get(), 0);

    let pair =
        r#"{"forms":["8-K/A","10-K","8-K"],"limit":5,"symbols":["AAPL"],"terms":[],"window_d":30}"#;
    assert_eq!(
        verdict(&one(&fixture, pair)),
        OK,
        "a form and its amendment are two forms"
    );
    assert_eq!(
        *fixture.forms.borrow(),
        vec![ids(&["10-K", "8-K", "8-K/A"])],
        "the list's order, not the model's (DEC-2252 item 1)"
    );
}
