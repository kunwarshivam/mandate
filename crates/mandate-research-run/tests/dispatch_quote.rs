//! A dispatch's `market.quote` (agent harness spec v0.2 §6.10; DEC-2255; backlog E19-13 slice
//! 3b): one last quote per data-universe symbol at or before the cut-off, asked once per symbol in
//! symbol order, filtered again on its symbol and time, and shown with the worker's own structural
//! sanity flag. The quote item's identity and record land here too, with the `quote:` namespace no
//! text reader may use.

mod common;

use common::{CUT_OFF, Fixture, NEWSWIRE, at, news, scope, symbol};
use mandate_canon::{Digest, Value, to_canonical};
use mandate_num::{Price, Qty};
use mandate_research_run::agentic::{Code, LoopBounds, Tool, ToolCall, ToolOutcome};
use mandate_research_run::{
    Bar, BarReader, DispatchScope, Dispatched, ItemKind, Quote, ReadError, Readers, RetrievedItem,
    SourceId, Symbol, dispatch,
};
use mandate_time::UtcNanos;

const TURN: u32 = 7;
const FAILED: (ToolOutcome, Option<Code>) = (ToolOutcome::Failed, Some(Code::Unavailable));
const OK: (ToolOutcome, Option<Code>) = (ToolOutcome::Ok, None);

fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("{text} is a price: {e:?}"))
}

fn size(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("{text} is a size: {e:?}"))
}

fn quote(name: &str, secs: i64, bid: Option<&str>, ask: Option<&str>) -> Quote {
    Quote {
        symbol: symbol(name),
        knowledge_at: at(secs),
        bid: bid.map(price),
        bid_size: size("100"),
        ask: ask.map(price),
        ask_size: size("200"),
    }
}

fn answering(answers: &[(&str, Quote)]) -> Fixture {
    Fixture {
        quotes: answers
            .iter()
            .map(|(name, quote)| ((*name).to_owned(), quote.clone()))
            .collect(),
        ..Fixture::default()
    }
}

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
        tool: Tool::MarketQuote,
        arguments: arguments.to_owned(),
    }];
    let mut got = dispatch(TURN, &calls, within, readers).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(got.results.len(), 1, "one call, one result");
    got.results.remove(0)
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

fn shown(result: &Dispatched) -> Vec<Value> {
    let value = mandate_canon::parse(&result.content).unwrap_or_else(|e| panic!("JSON: {e:?}"));
    value
        .get("items")
        .and_then(Value::as_array)
        .unwrap_or_default()
        .to_vec()
}

fn sane(item: &Value) -> Option<bool> {
    match item.get("untrusted").and_then(|fields| fields.get("sane")) {
        Some(Value::Bool(flag)) => Some(*flag),
        _ => None,
    }
}

#[test]
fn a_quote_is_a_platform_item_with_a_derived_id() {
    let item = RetrievedItem::Quote(quote("AAPL", CUT_OFF - 5, Some("189.5"), Some("189.52")));
    assert_eq!(
        item.item_id().map(|id| id.as_str().to_owned()).ok(),
        Some(format!("quote:AAPL:{}", CUT_OFF - 5))
    );
    assert_eq!(
        item.source().map(|source| source.as_str().to_owned()).ok(),
        Some(SourceId::QUOTES.to_owned())
    );
    assert_eq!(SourceId::QUOTES, "platform.quotes");
    assert_eq!(item.kind(), ItemKind::Quote);
    assert_eq!(ItemKind::Quote.as_str(), "quote");
    assert_eq!(item.symbol(), Some(&symbol("AAPL")));
    assert_eq!(item.knowledge_at(), at(CUT_OFF - 5));
    assert_eq!((item.length_bytes(), item.content_hash()), (0, None));

    let record = item.record().unwrap_or_else(|e| panic!("{e:?}"));
    let value = mandate_canon::parse(&record).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(record, to_canonical(&value));
    let text = |s: &str| Some(Value::Str(s.to_owned()));
    let member = |name: &str| value.get(name).cloned();
    assert_eq!(member("kind"), text("quote"));
    assert_eq!(member("bid"), text(&price("189.5").to_string()));
    assert_eq!(member("ask"), text(&price("189.52").to_string()));
    assert_eq!(member("bid_size"), text(&size("100").to_string()));
    assert_eq!(member("knowledge_at"), text(&at(CUT_OFF - 5).to_string()));

    let one_sided = RetrievedItem::Quote(quote("AAPL", CUT_OFF - 5, None, Some("189.52")));
    let record = one_sided.record().unwrap_or_else(|e| panic!("{e:?}"));
    let value = mandate_canon::parse(&record).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        value.get("bid").cloned(),
        Some(Value::Null),
        "a side with no order"
    );
}

#[test]
fn only_a_text_item_carries_a_content_hash() {
    let news = RetrievedItem::News(news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60));
    assert_eq!(news.content_hash(), Some(Digest::of(b"n-1")));
    let quote = RetrievedItem::Quote(quote("AAPL", CUT_OFF - 5, Some("1"), Some("2")));
    assert_eq!(quote.content_hash(), None, "a quote is data, not text");
}

#[test]
fn a_text_reader_may_not_use_the_quote_namespace() {
    let fixture = Fixture {
        news: vec![
            news("quote:AAPL:1", NEWSWIRE, "AAPL", CUT_OFF - 60),
            news("n-1", NEWSWIRE, "AAPL", CUT_OFF - 60),
        ],
        ..Fixture::default()
    };
    let scope = scope();
    let bounds = LoopBounds::v0_2();
    let within = DispatchScope {
        scope: &scope,
        bounds: &bounds,
        result_bytes_left: 96_000,
    };
    let calls = [ToolCall {
        index: 0,
        tool: Tool::NewsSearch,
        arguments: r#"{"limit":5,"symbols":["AAPL"],"terms":[],"window_h":24}"#.to_owned(),
    }];
    let got = dispatch(TURN, &calls, within, fixture.readers()).unwrap_or_else(|e| panic!("{e:?}"));
    let ids: Vec<String> = got
        .results
        .iter()
        .flat_map(|result| result.items.iter())
        .filter_map(|item| item.item_id().ok().map(|id| id.as_str().to_owned()))
        .collect();
    assert_eq!(ids, ["n-1"], "`quote:` is reserved (DEC-2255 item 6)");
}

#[test]
fn market_quote_asks_each_symbol_once_in_order_as_of_the_cut_off() {
    let fixture = answering(&[
        (
            "AAPL",
            quote("AAPL", CUT_OFF - 5, Some("189.5"), Some("189.52")),
        ),
        (
            "MSFT",
            quote("MSFT", CUT_OFF - 9, Some("410.1"), Some("410.2")),
        ),
    ]);
    let result = run(fixture.readers(), r#"{"symbols":["MSFT","AAPL","AAPL"]}"#);
    assert_eq!(verdict(&result), OK);
    assert_eq!(
        *fixture.quote_asks.borrow(),
        vec![(symbol("AAPL"), at(CUT_OFF)), (symbol("MSFT"), at(CUT_OFF))],
        "once per symbol, in symbol order, as of the cut-off"
    );
    let symbols: Vec<Option<String>> = shown(&result)
        .iter()
        .map(|item| {
            item.get("untrusted")
                .and_then(|fields| fields.get("symbol"))
                .and_then(Value::as_str)
                .map(str::to_owned)
        })
        .collect();
    assert_eq!(symbols, [Some("AAPL".to_owned()), Some("MSFT".to_owned())]);
}

#[test]
fn a_quote_shows_its_time_sides_and_flag_under_untrusted() {
    let last = quote("AAPL", CUT_OFF - 5, Some("189.5"), Some("189.52"));
    let fixture = answering(&[("AAPL", last.clone())]);
    let result = run(fixture.readers(), r#"{"symbols":["AAPL"]}"#);
    let value = mandate_canon::parse(&result.content).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(result.content, to_canonical(&value));
    assert_eq!(keys(&value), ["items", "truncated"]);
    let all = shown(&result);
    let [first] = all.as_slice() else {
        panic!("one quote: {all:?}")
    };
    assert_eq!(keys(first), ["item_id", "source", "untrusted"]);
    let text = |s: &str| Some(Value::Str(s.to_owned()));
    assert_eq!(
        first.get("item_id").cloned(),
        text(&format!("quote:AAPL:{}", CUT_OFF - 5))
    );
    assert_eq!(first.get("source").cloned(), text("platform.quotes"));
    let wrapped = first
        .get("untrusted")
        .unwrap_or_else(|| panic!("untrusted"));
    let fields = [
        "ask",
        "ask_size",
        "bid",
        "bid_size",
        "quoted_at",
        "sane",
        "symbol",
    ];
    assert_eq!(keys(wrapped), fields, "DEC-2255 item 6");
    let field = |name: &str| wrapped.get(name).cloned();
    assert_eq!(field("bid"), text(&price("189.5").to_string()));
    assert_eq!(field("ask"), text(&price("189.52").to_string()));
    assert_eq!(field("bid_size"), text(&size("100").to_string()));
    assert_eq!(field("ask_size"), text(&size("200").to_string()));
    assert_eq!(field("quoted_at"), text(&at(CUT_OFF - 5).to_string()));
    assert_eq!(field("symbol"), text("AAPL"));
    assert_eq!(sane(first), Some(true));
    assert_eq!(
        result.items,
        vec![RetrievedItem::Quote(last)],
        "observed as its own record"
    );
    assert!(!result.untrusted, "a quote is platform data, as a bar is");
    assert!(!result.truncated);
}

#[test]
fn the_sanity_flag_is_both_sides_priced_with_the_bid_at_or_below_the_ask() {
    for (bid, ask, flag, why) in [
        (Some("189.5"), Some("189.52"), true, "a normal quote"),
        (Some("189.5"), Some("189.5"), true, "a locked quote"),
        (Some("189.53"), Some("189.52"), false, "a crossed quote"),
        (None, Some("189.52"), false, "no bid"),
        (Some("189.5"), None, false, "no ask"),
    ] {
        let fixture = answering(&[("AAPL", quote("AAPL", CUT_OFF - 5, bid, ask))]);
        let result = run(fixture.readers(), r#"{"symbols":["AAPL"]}"#);
        let all = shown(&result);
        assert_eq!(all.first().and_then(sane), Some(flag), "{why}");
        if bid.is_none() {
            let shown_bid = all
                .first()
                .and_then(|item| item.get("untrusted"))
                .and_then(|fields| fields.get("bid"))
                .cloned();
            assert_eq!(
                shown_bid,
                Some(Value::Null),
                "{why}: a side with no order is null"
            );
        }
    }
}

#[test]
fn every_quote_is_filtered_again_on_its_symbol_and_time() {
    let fixture = answering(&[
        ("AAPL", quote("MSFT", CUT_OFF - 5, Some("1"), Some("2"))),
        ("MSFT", quote("MSFT", CUT_OFF + 1, Some("1"), Some("2"))),
    ]);
    let result = run(fixture.readers(), r#"{"symbols":["AAPL","MSFT"]}"#);
    assert_eq!(verdict(&result), OK);
    assert!(
        shown(&result).is_empty() && result.items.is_empty(),
        "another symbol's quote and a later one are both dropped (HI-13)"
    );

    let at_cut_off = answering(&[("AAPL", quote("AAPL", CUT_OFF, Some("1"), Some("2")))]);
    assert_eq!(
        shown(&run(at_cut_off.readers(), r#"{"symbols":["AAPL"]}"#)).len(),
        1,
        "a quote at the cut-off is not look-ahead (DP-1)"
    );
    let none = Fixture::default();
    let result = run(none.readers(), r#"{"symbols":["AAPL"]}"#);
    assert_eq!(
        (verdict(&result), shown(&result).len()),
        (OK, 0),
        "no quote, no item"
    );
}

#[test]
fn market_quote_arguments_outside_the_pinned_schema_ask_no_reader() {
    let fixture = Fixture::default();
    let ten = format!(r#"{{"symbols":[{}]}}"#, [r#""AAPL""#; 10].join(","));
    assert_eq!(verdict(&run(fixture.readers(), &ten)), OK, "ten symbols");
    let asked = fixture.quote_asks.borrow().len();
    let eleven = format!(r#"{{"symbols":[{}]}}"#, [r#""AAPL""#; 11].join(","));
    for arguments in [
        eleven.as_str(),
        r#"{"symbols":[]}"#,
        r#"{"symbols":[7]}"#,
        r#"{"symbols":["aapl!"]}"#,
        r#"{"symbols":"AAPL"}"#,
        "{}",
        "{",
    ] {
        assert_eq!(
            verdict(&run(fixture.readers(), arguments)),
            FAILED,
            "{arguments}"
        );
    }
    assert_eq!(
        fixture.quote_asks.borrow().len(),
        asked,
        "no reader is asked"
    );
}

#[test]
fn a_quote_read_outside_the_universe_or_unanswered_reads_nothing() {
    let fixture = Fixture::default();
    let result = run(fixture.readers(), r#"{"symbols":["AAPL","TSLA"]}"#);
    assert_eq!(
        verdict(&result),
        (ToolOutcome::Refused, Some(Code::OutOfUniverse))
    );
    assert!(
        fixture.quote_asks.borrow().is_empty(),
        "refused whole, before any read"
    );

    let failing = Fixture {
        failing: true,
        ..answering(&[("AAPL", quote("AAPL", CUT_OFF - 5, Some("1"), Some("2")))])
    };
    let result = run(failing.readers(), r#"{"symbols":["AAPL","MSFT"]}"#);
    assert_eq!(
        verdict(&result),
        FAILED,
        "any reader error reads nothing for the whole call"
    );
    assert!(result.items.is_empty());

    let readers = Readers {
        bars: &Bare,
        ..fixture.readers()
    };
    assert_eq!(
        verdict(&run(readers, r#"{"symbols":["AAPL"]}"#)),
        FAILED,
        "the port's default reads nothing (rule 3)"
    );
}

/// A bar reader that serves bars only.
struct Bare;

impl BarReader for Bare {
    fn daily_bars(&self, _: &Symbol, _: UtcNanos, _: u32) -> Result<Vec<Bar>, ReadError> {
        Ok(Vec::new())
    }
}

/// The port's default body, called directly rather than through the pending dispatch, so the
/// tests PR's own mutants run already holds it to reading nothing (rule 3).
#[test]
fn the_last_quote_default_answers_unavailable() {
    assert_eq!(
        Bare.last_quote(&symbol("AAPL"), at(CUT_OFF)),
        Err(ReadError::Unavailable)
    );
}
