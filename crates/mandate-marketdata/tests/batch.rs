//! DEC-2283 item 5, data-plane spec §3.8: one request carries every instrument of a generation.
//! The bars of a span or window and the corporate actions of a span are asked for several
//! symbols at once, split by symbol on receipt, and refused whole when an answer names a symbol not
//! asked for, repeats one, or holds a record outside the asked span. The oracles are the requests
//! and records each case writes out by hand, and, for the property, the generated bars themselves.

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{FakeTransport, RecordingPause, day, ok};
use mandate_marketdata::alpaca::WireError;
use mandate_marketdata::batch::{
    BarsBatch, BatchError, actions_path, bars_path, parse_actions_page, parse_bars_page,
};
use mandate_marketdata::client::{Client, FetchError};
use mandate_marketdata::http::{AlpacaDataHttp, Credentials, CredentialsError};
use mandate_marketdata::model::{DayRange, Symbol, TimeUnit, Timeframe};
use mandate_time::UtcNanos;
use proptest::prelude::*;

fn symbol(text: &str) -> Symbol {
    Symbol::parse(text).unwrap()
}

fn symbols(texts: &[&str]) -> BTreeSet<Symbol> {
    texts.iter().map(|text| symbol(text)).collect()
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn daily() -> Timeframe {
    Timeframe::new(1, TimeUnit::Day).unwrap()
}

fn minute() -> Timeframe {
    Timeframe::new(1, TimeUnit::Minute).unwrap()
}

/// SPY and QQQ's daily bars from 2026-09-01 up to 2026-10-01, both UTC midnights.
fn september() -> BarsBatch {
    BarsBatch::new(
        symbols(&["SPY", "QQQ"]),
        daily(),
        at("2026-09-01T00:00:00Z"),
        at("2026-10-01T00:00:00Z"),
    )
    .unwrap()
}

fn bar(t: &str, close: &str) -> String {
    format!(
        r#"{{"c":{close},"h":{close},"l":{close},"n":3,"o":{close},"t":"{t}","v":100,"vw":{close}}}"#
    )
}

fn bars_body(entries: &[(&str, &[String])], token: Option<&str>) -> Vec<u8> {
    let members: Vec<String> = entries
        .iter()
        .map(|(sym, bars)| format!(r#""{sym}":[{}]"#, bars.join(",")))
        .collect();
    let token = token.map_or("null".to_owned(), |t| format!(r#""{t}""#));
    format!(
        r#"{{"bars":{{{}}},"next_page_token":{token}}}"#,
        members.join(",")
    )
    .into_bytes()
}

#[test]
fn a_batch_asks_every_symbol_in_one_request_in_symbol_order() {
    assert_eq!(
        bars_path(&september(), 10_000, None),
        "/v2/stocks/bars?symbols=QQQ,SPY&timeframe=1Day&start=2026-09-01T00:00:00Z\
         &end=2026-10-01T00:00:00Z&limit=10000&feed=iex&adjustment=raw&sort=asc"
    );
    let window = BarsBatch::new(
        symbols(&["BRK.B"]),
        minute(),
        at("2026-10-27T14:25:00Z"),
        at("2026-10-27T14:31:00Z"),
    )
    .unwrap();
    assert_eq!(
        bars_path(&window, 500, Some("Q1BZ/fA==")),
        "/v2/stocks/bars?symbols=BRK.B&timeframe=1Min&start=2026-10-27T14:25:00Z\
         &end=2026-10-27T14:31:00Z&limit=500&feed=iex&adjustment=raw&sort=asc\
         &page_token=Q1BZ%2FfA%3D%3D"
    );
}

#[test]
fn a_batch_is_refused_before_it_could_be_asked() {
    let start = at("2026-09-01T00:00:00Z");
    let end = at("2026-10-01T00:00:00Z");
    assert_eq!(
        BarsBatch::new(BTreeSet::new(), daily(), start, end),
        Err(BatchError::NoSymbols)
    );
    let many: BTreeSet<Symbol> = (0..51).map(|n| symbol(&format!("S{n}"))).collect();
    assert_eq!(
        BarsBatch::new(many, daily(), start, end),
        Err(BatchError::TooManySymbols)
    );
    assert_eq!(
        BarsBatch::new(symbols(&["SPY"]), daily(), end, start),
        Err(BatchError::EmptySpan)
    );
    assert_eq!(
        BarsBatch::new(symbols(&["SPY"]), daily(), start, start),
        Err(BatchError::EmptySpan)
    );
    assert_eq!(
        BarsBatch::new(
            symbols(&["SPY"]),
            daily(),
            UtcNanos::from_parts(start.secs(), 1).unwrap(),
            end
        ),
        Err(BatchError::NotWholeSecond)
    );
    assert_eq!(
        BarsBatch::new(
            symbols(&["SPY"]),
            daily(),
            start,
            UtcNanos::from_parts(end.secs(), 1).unwrap()
        ),
        Err(BatchError::NotWholeSecond)
    );
    let codes = [
        BatchError::NoSymbols,
        BatchError::TooManySymbols,
        BatchError::EmptySpan,
        BatchError::NotWholeSecond,
    ]
    .map(BatchError::code);
    assert_eq!(
        codes,
        [
            "no_symbols",
            "too_many_symbols",
            "empty_span",
            "not_whole_second"
        ]
    );
    let fifty: BTreeSet<Symbol> = (0..50).map(|n| symbol(&format!("S{n}"))).collect();
    assert!(BarsBatch::new(fifty, daily(), start, end).is_ok());
}

#[test]
fn a_page_is_split_by_symbol() {
    let spy = [
        bar("2026-09-01T04:00:00Z", "650.1"),
        bar("2026-09-02T04:00:00Z", "651"),
    ];
    let qqq = [bar("2026-09-01T04:00:00Z", "570")];
    let page = parse_bars_page(
        &september(),
        &bars_body(&[("SPY", &spy), ("QQQ", &qqq)], Some("next")),
    )
    .unwrap();
    assert_eq!(page.next_page_token.as_deref(), Some("next"));
    let closes: BTreeMap<String, Vec<String>> = page
        .bars
        .iter()
        .map(|(sym, bars)| {
            (
                sym.as_str().to_owned(),
                bars.iter().map(|b| b.close.as_str().to_owned()).collect(),
            )
        })
        .collect();
    assert_eq!(
        closes,
        BTreeMap::from([
            ("QQQ".to_owned(), vec!["570".to_owned()]),
            ("SPY".to_owned(), vec!["650.1".to_owned(), "651".to_owned()]),
        ])
    );
}

#[test]
fn an_answer_the_batch_did_not_ask_for_refuses_the_page() {
    let one = [bar("2026-09-01T04:00:00Z", "1")];
    assert_eq!(
        parse_bars_page(&september(), &bars_body(&[("IWM", &one)], None)).unwrap_err(),
        WireError::UnexpectedSymbol("IWM".to_owned())
    );
    assert_eq!(
        parse_bars_page(
            &september(),
            &bars_body(&[("SPY", &one), ("SPY", &one)], None)
        )
        .unwrap_err(),
        WireError::DuplicateSymbol("SPY".to_owned())
    );
    let before = [bar("2026-08-31T23:59:59Z", "1")];
    assert_eq!(
        parse_bars_page(&september(), &bars_body(&[("SPY", &before)], None))
            .unwrap_err()
            .code(),
        "outside_span"
    );
    let after = [bar("2026-10-01T00:00:01Z", "1")];
    assert_eq!(
        parse_bars_page(&september(), &bars_body(&[("SPY", &after)], None))
            .unwrap_err()
            .code(),
        "outside_span"
    );
}

#[test]
fn a_record_at_the_spans_end_is_dropped_and_one_at_its_start_kept() {
    let edges = [
        bar("2026-09-01T00:00:00Z", "1"),
        bar("2026-10-01T00:00:00Z", "2"),
    ];
    let page = parse_bars_page(&september(), &bars_body(&[("SPY", &edges)], None)).unwrap();
    let spy = page.bars.get(&symbol("SPY")).unwrap();
    assert_eq!(spy.len(), 1);
    assert_eq!(spy[0].start, at("2026-09-01T00:00:00Z"));
}

#[tokio::test(flavor = "current_thread")]
async fn pages_are_merged_by_symbol_and_every_asked_symbol_answered() {
    let first = [
        bar("2026-09-01T04:00:00Z", "1"),
        bar("2026-09-02T04:00:00Z", "2"),
    ];
    let second = [bar("2026-09-03T04:00:00Z", "3")];
    let transport = FakeTransport::serving([
        ok(&bars_body(&[("SPY", &first)], Some("p2"))),
        ok(&bars_body(&[("SPY", &second)], None)),
    ]);
    let client = Client::new(transport.clone(), RecordingPause::default());
    let bars = client.fetch_bars_batch(&september()).await.unwrap();
    assert_eq!(
        bars.keys().map(Symbol::as_str).collect::<Vec<_>>(),
        ["QQQ", "SPY"],
        "QQQ, answered by no page, is answered empty"
    );
    assert!(bars[&symbol("QQQ")].is_empty());
    assert_eq!(
        bars[&symbol("SPY")]
            .iter()
            .map(|b| b.close.as_str().to_owned())
            .collect::<Vec<_>>(),
        ["1", "2", "3"]
    );
    assert_eq!(transport.requested().len(), 2);
    assert!(transport.requested()[1].ends_with("&page_token=p2"));
    assert_eq!(transport.unused(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn a_bar_repeated_or_out_of_order_across_pages_refuses_the_batch() {
    for (first, second) in [
        ("2026-09-02T04:00:00Z", "2026-09-02T04:00:00Z"),
        ("2026-09-02T04:00:00Z", "2026-09-01T04:00:00Z"),
    ] {
        let transport = FakeTransport::serving([
            ok(&bars_body(&[("SPY", &[bar(first, "1")])], Some("p2"))),
            ok(&bars_body(&[("SPY", &[bar(second, "1")])], None)),
        ]);
        let client = Client::new(transport, RecordingPause::default());
        let refused = client.fetch_bars_batch(&september()).await.unwrap_err();
        assert_eq!(refused.code(), "out_of_order", "{first} then {second}");
    }
    let transport = FakeTransport::serving([
        ok(&bars_body(
            &[("SPY", &[bar("2026-09-02T04:00:00Z", "1")])],
            Some("p2"),
        )),
        ok(&bars_body(
            &[("QQQ", &[bar("2026-09-01T04:00:00Z", "1")])],
            None,
        )),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    assert!(
        client.fetch_bars_batch(&september()).await.is_ok(),
        "order is per symbol: QQQ's earlier bar on a later page is in order"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn a_repeated_page_token_refuses_the_batch() {
    let one = [bar("2026-09-01T04:00:00Z", "1")];
    let two = [bar("2026-09-02T04:00:00Z", "1")];
    let three = [bar("2026-09-03T04:00:00Z", "1")];
    let transport = FakeTransport::serving([
        ok(&bars_body(&[("SPY", &one)], Some("p"))),
        ok(&bars_body(&[("SPY", &two)], Some("p"))),
        ok(&bars_body(&[("SPY", &three)], None)),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    assert!(matches!(
        client.fetch_bars_batch(&september()).await,
        Err(FetchError::RepeatedPageToken { page: 2 })
    ));
}

fn span() -> DayRange {
    DayRange::new(day("2026-09-01"), day("2026-09-30")).unwrap()
}

#[test]
fn actions_are_asked_for_every_symbol_in_one_request() {
    assert_eq!(
        actions_path(&symbols(&["SPY", "QQQ"]), span(), 1000, None).unwrap(),
        "/v1/corporate-actions?symbols=QQQ,SPY&start=2026-08-01&end=2027-10-01&limit=1000&sort=asc"
    );
}

fn actions_body(members: &str, token: Option<&str>) -> Vec<u8> {
    let token = token.map_or("null".to_owned(), |t| format!(r#""{t}""#));
    format!(r#"{{"corporate_actions":{{{members}}},"next_page_token":{token}}}"#).into_bytes()
}

fn split(id: &str, sym: &str, ex_date: &str) -> String {
    format!(
        r#"{{"id":"{id}","symbol":"{sym}","ex_date":"{ex_date}","new_rate":2,"old_rate":1,"process_date":"{ex_date}","cusip":"x"}}"#
    )
}

#[test]
fn actions_are_split_by_the_symbol_they_name() {
    let body = actions_body(
        &format!(
            r#""forward_splits":[{},{}],"cash_dividends":[{{"id":"d1","symbol":"SPY","ex_date":"2026-09-19","record_date":null,"payable_date":null,"rate":1.5,"special":false,"foreign":false,"process_date":"2026-09-19"}}],"name_changes":[{{"id":"o1","old_symbol":"QQQ","new_symbol":"QQQM","process_date":"2026-09-10"}},{{"id":"o2","process_date":"2026-09-11"}}]"#,
            split("s1", "SPY", "2026-09-15"),
            split("s2", "QQQ", "2026-09-16"),
        ),
        None,
    );
    let page = parse_actions_page(&symbols(&["SPY", "QQQ"]), &body).unwrap();
    let ids = |sym: &str| {
        let actions = &page.actions[&symbol(sym)];
        assert_eq!(actions.symbol, symbol(sym));
        actions.ids().map(str::to_owned).collect::<Vec<_>>()
    };
    assert_eq!(
        ids("SPY"),
        ["s1", "d1", "o2"],
        "o2 names no asked symbol, so it goes to each"
    );
    assert_eq!(
        ids("QQQ"),
        ["s2", "o1", "o2"],
        "o1 names QQQ as its old symbol"
    );
}

#[test]
fn an_action_for_a_symbol_not_asked_or_repeated_refuses_the_page() {
    let foreign = actions_body(
        &format!(r#""forward_splits":[{}]"#, split("s1", "IWM", "2026-09-15")),
        None,
    );
    assert_eq!(
        parse_actions_page(&symbols(&["SPY"]), &foreign).unwrap_err(),
        WireError::UnexpectedSymbol("IWM".to_owned())
    );
    let twice = actions_body(
        &format!(
            r#""forward_splits":[{},{}]"#,
            split("s1", "SPY", "2026-09-15"),
            split("s1", "SPY", "2026-09-15")
        ),
        None,
    );
    assert_eq!(
        parse_actions_page(&symbols(&["SPY"]), &twice).unwrap_err(),
        WireError::DuplicateAction("s1".to_owned())
    );
}

#[tokio::test(flavor = "current_thread")]
async fn actions_across_pages_are_merged_dated_in_the_span_and_never_repeated() {
    let transport = FakeTransport::serving([
        ok(&actions_body(
            &format!(r#""forward_splits":[{}]"#, split("s1", "SPY", "2026-09-15")),
            Some("p2"),
        )),
        ok(&actions_body(
            &format!(r#""forward_splits":[{}]"#, split("s2", "SPY", "2026-11-02")),
            None,
        )),
    ]);
    let client = Client::new(transport.clone(), RecordingPause::default());
    let actions = client
        .fetch_corporate_actions_batch(&symbols(&["SPY", "QQQ"]), span())
        .await
        .unwrap();
    assert_eq!(
        actions[&symbol("SPY")].ids().collect::<Vec<_>>(),
        ["s1"],
        "s2's ex-date is past the span"
    );
    assert!(actions[&symbol("QQQ")].is_empty());
    assert!(transport.requested()[1].ends_with("&page_token=p2"));

    let transport = FakeTransport::serving([
        ok(&actions_body(
            &format!(r#""forward_splits":[{}]"#, split("s1", "SPY", "2026-09-15")),
            Some("p2"),
        )),
        ok(&actions_body(
            &format!(r#""forward_splits":[{}]"#, split("s1", "SPY", "2026-09-15")),
            None,
        )),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    assert_eq!(
        client
            .fetch_corporate_actions_batch(&symbols(&["SPY"]), span())
            .await
            .unwrap_err()
            .code(),
        "wire"
    );
}

#[test]
fn the_bars_transport_sends_the_key_to_bars_and_corporate_actions_only() {
    let key = Credentials::from_stored(b"PKTEST123:secretvalue\n").unwrap();
    let http = AlpacaDataHttp::bars_only(key).unwrap();
    assert!(http.permits("/v2/stocks/bars?symbols=SPY&timeframe=1Day"));
    assert!(http.permits("/v1/corporate-actions?symbols=SPY&start=2026-08-01"));
    for refused in [
        "/v2/stocks/trades?symbols=SPY",
        "/v2/stocks/quotes?symbols=SPY",
        "/v1beta3/crypto/us/bars?symbols=BTC",
        "/v1beta1/news?symbols=SPY",
        "/v2/stocks/bars?symbols=SPY#x",
        "/v2/stocks/bars?symbols=SPY/../trades",
        "/v2/account",
    ] {
        assert!(!http.permits(refused), "{refused}");
    }
    let wide = AlpacaDataHttp::new(Credentials::new("k".into(), "s".into())).unwrap();
    assert!(
        wide.permits("/v2/stocks/trades?symbols=SPY"),
        "the CLI's transport is unchanged"
    );
}

#[test]
fn a_stored_key_is_one_paper_line_and_shows_nothing_of_itself() {
    let key = Credentials::from_stored(b"PKABC:s3cr3t").unwrap();
    let shown = format!("{key:?}");
    assert_eq!(shown, "Credentials(redacted)");
    assert!(
        !shown.contains("PKABC") && !shown.contains("s3cr3t"),
        "{shown}"
    );
    for (stored, refusal) in [
        (&b"AKLIVE:s3cr3t"[..], CredentialsError::NotPaper),
        (b"PKABC", CredentialsError::Malformed),
        (b"PKABC:", CredentialsError::Malformed),
        (b":s3cr3t", CredentialsError::Malformed),
        (b"PKABC:s3 cr3t", CredentialsError::Malformed),
        (b"PKABC:a:b", CredentialsError::Malformed),
        (b"PKABC:s3cr3t\n\n", CredentialsError::Malformed),
        (b"\xffPK:s", CredentialsError::Malformed),
        (b"AK:", CredentialsError::Malformed),
    ] {
        assert_eq!(
            Credentials::from_stored(stored).unwrap_err(),
            refusal,
            "{:?}",
            String::from_utf8_lossy(stored)
        );
    }
    assert_eq!(CredentialsError::NotPaper.code(), "not_paper");
    assert_eq!(CredentialsError::Malformed.code(), "malformed");
}

proptest! {
    /// Bars generated for each symbol, ascending, dealt onto pages in any order of symbols and
    /// page breaks, come back per symbol exactly as generated: the generated map is the oracle.
    #[test]
    fn any_paging_of_a_batch_returns_each_symbols_bars(
        counts in proptest::collection::vec(0usize..6, 1..4),
        breaks in proptest::collection::vec(any::<bool>(), 0..24),
    ) {
        let names = ["AAA", "BBB", "CCC"];
        let mut expected: BTreeMap<String, Vec<String>> = BTreeMap::new();
        let mut stream: Vec<(String, String, String)> = Vec::new();
        for (index, count) in counts.iter().enumerate() {
            let name = names[index].to_owned();
            let mut closes = Vec::new();
            for n in 0..*count {
                let t = format!("2026-09-{:02}T04:00:00Z", n + 1);
                let close = format!("{}.{}", index + 1, n + 1);
                stream.push((name.clone(), t, close.clone()));
                closes.push(close);
            }
            expected.insert(name, closes);
        }
        let mut pages: Vec<Vec<(String, Vec<String>)>> = vec![Vec::new()];
        for (position, (name, t, close)) in stream.iter().enumerate() {
            if breaks.get(position).copied().unwrap_or(false) && !pages.last().unwrap().is_empty() {
                pages.push(Vec::new());
            }
            let page = pages.last_mut().unwrap();
            match page.last_mut() {
                Some((last, bars)) if last == name => bars.push(bar(t, close)),
                _ => page.push((name.clone(), vec![bar(t, close)])),
            }
        }
        let total = pages.len();
        let bodies: Vec<Vec<u8>> = pages
            .iter()
            .enumerate()
            .map(|(n, page)| {
                let entries: Vec<(&str, &[String])> =
                    page.iter().map(|(name, bars)| (name.as_str(), bars.as_slice())).collect();
                let token = (n + 1 < total).then(|| format!("t{n}"));
                bars_body(&entries, token.as_deref())
            })
            .collect();
        let asked: BTreeSet<Symbol> = expected.keys().map(|name| symbol(name)).collect();
        let batch = BarsBatch::new(asked, daily(), at("2026-09-01T00:00:00Z"), at("2026-10-01T00:00:00Z")).unwrap();
        let transport = FakeTransport::serving(bodies.iter().map(|b| ok(b)));
        let client = Client::new(transport, RecordingPause::default());
        let runtime = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let got = runtime.block_on(client.fetch_bars_batch(&batch)).unwrap();
        let got: BTreeMap<String, Vec<String>> = got
            .iter()
            .map(|(sym, bars)| (sym.as_str().to_owned(), bars.iter().map(|b| b.close.as_str().to_owned()).collect()))
            .collect();
        prop_assert_eq!(got, expected);
    }
}
