//! The recent one-minute IEX bars read (E7-7, DEC-469): the request it builds, what it reads, and
//! what it refuses, over scripted replies with no network (ADR-0001 ES-19).
//!
//! Each expectation is written from Alpaca's bars answer shape and from DEC-469's rules, never from
//! the code. Every refusal case first reads a good answer and asserts its exact values, so an
//! implementation that refuses everything passes none of them (`AGENTS.md` rule 3).

mod common;

use std::time::Duration;

use common::{FakeClock, FakeDataTransport, inline};
use mandate_accounting::InstrumentId;
use mandate_alpaca::data::{BarsRequest, DataClient, MAX_BARS_WINDOW};
use mandate_alpaca::error::{ReadError, TransportError};
use mandate_alpaca::http::Response;
use mandate_alpaca::read::{MinuteBar, MinuteBars};
use mandate_num::Qty;
use mandate_time::UtcNanos;

const NOW: &str = "2026-09-28T17:00:00Z";
const FIVE_MINUTES: Duration = Duration::from_secs(300);

fn instrument(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol).unwrap_or_else(|e| panic!("{symbol}: {e:?}"))
}

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

fn path(symbol: &str, now: &str, window: Duration) -> Result<String, TransportError> {
    BarsRequest::recent_minutes(&instrument(symbol), instant(now), window)
        .map(|request| request.path_and_query().to_owned())
}

fn bars_path(symbol: &str, start: &str, end: &str, limit: u32) -> String {
    format!(
        "/v2/stocks/{symbol}/bars?timeframe=1Min&start={start}&end={end}&limit={limit}&adjustment=raw&feed=iex&sort=asc"
    )
}

/// At 17:00:00 a five-minute window asks for 16:55 to 16:59 inclusive, five bars. Half a minute
/// later 16:55:30.5 rounds up to 16:56, and the last closed minute is still 16:59: four. A one-minute
/// window at 17:00 is the 16:59 bar alone, and fifteen minutes is the widest.
#[test]
fn the_request_asks_for_exactly_the_closed_minutes_in_the_window() {
    let cases = [
        (
            NOW,
            300,
            bars_path("AAPL", "2026-09-28T16:55:00Z", "2026-09-28T16:59:00Z", 5),
        ),
        (
            "2026-09-28T17:00:30.5Z",
            300,
            bars_path("AAPL", "2026-09-28T16:56:00Z", "2026-09-28T16:59:00Z", 4),
        ),
        (
            NOW,
            60,
            bars_path("AAPL", "2026-09-28T16:59:00Z", "2026-09-28T16:59:00Z", 1),
        ),
        (
            NOW,
            900,
            bars_path("AAPL", "2026-09-28T16:45:00Z", "2026-09-28T16:59:00Z", 15),
        ),
    ];
    for (now, window_s, expected) in cases {
        assert_eq!(
            path("AAPL", now, Duration::from_secs(window_s)),
            Ok(expected),
            "{window_s}s at {now}"
        );
    }
    assert_eq!(
        path("BRK.B", NOW, FIVE_MINUTES),
        Ok(bars_path(
            "BRK.B",
            "2026-09-28T16:55:00Z",
            "2026-09-28T16:59:00Z",
            5
        ))
    );
    let request = BarsRequest::recent_minutes(&instrument("AAPL"), instant(NOW), FIVE_MINUTES)
        .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(request.instrument(), &instrument("AAPL"));
    assert_eq!(request.first_start(), instant("2026-09-28T16:55:00Z"));
    assert_eq!(request.last_start(), instant("2026-09-28T16:59:00Z"));
    assert_eq!(
        request.url(),
        format!("https://data.alpaca.markets{}", request.path_and_query())
    );
    assert_eq!(MAX_BARS_WINDOW, Duration::from_secs(900));
}

/// No request exists for a pair, a window that is not whole minutes from one to fifteen, a window
/// with no closed minute in it, or one reaching back before New York midnight. 04:02Z is 00:02 in
/// New York (EDT): two minutes back is still today, five is yesterday.
#[test]
fn a_request_outside_its_bounds_is_never_built() {
    let refused = Err(TransportError::RefusedPath);
    assert_eq!(path("BTC/USD", NOW, FIVE_MINUTES), refused);
    for window in [
        Duration::ZERO,
        Duration::from_secs(59),
        Duration::from_secs(61),
        Duration::from_millis(300_500),
        Duration::from_secs(960),
    ] {
        assert_eq!(path("AAPL", NOW, window), refused, "{window:?}");
    }
    assert_eq!(
        path("AAPL", "2026-09-28T17:00:30Z", Duration::from_secs(60)),
        refused,
        "the minute 16:59:30 to 17:00:30 holds no bar that both starts inside it and has closed"
    );
    assert_eq!(
        path("AAPL", "2026-09-28T04:02:00Z", Duration::from_secs(120)),
        Ok(bars_path(
            "AAPL",
            "2026-09-28T04:00:00Z",
            "2026-09-28T04:01:00Z",
            2
        ))
    );
    assert_eq!(path("AAPL", "2026-09-28T04:02:00Z", FIVE_MINUTES), refused);
}

fn bar(t: &str, v: &str) -> String {
    format!(r#"{{"c":255.2,"h":255.3,"l":255.1,"n":12,"o":255.2,"t":"{t}","v":{v},"vw":255.21}}"#)
}

/// Five closed bars, 16:55 to 16:59, volumes 101 to 105.
fn good_bars() -> Vec<String> {
    (5..10)
        .map(|minute| {
            bar(
                &format!("2026-09-28T16:5{minute}:00Z"),
                &format!("10{}", minute - 4),
            )
        })
        .collect()
}

fn answer(symbol: &str, bars: &str, page: &str) -> String {
    format!(r#"{{"bars":{bars},"next_page_token":{page},"symbol":"{symbol}"}}"#)
}

fn good() -> String {
    answer("AAPL", &format!("[{}]", good_bars().join(",")), "null")
}

fn read_at(
    replies: Vec<Result<Response, TransportError>>,
    now: &str,
) -> (Result<MinuteBars, ReadError>, FakeDataTransport) {
    let transport = FakeDataTransport::serving(replies);
    let client = DataClient::new(transport.clone(), FakeClock::at(instant(now)));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap_or_else(|e| panic!("{e}"));
    let read = runtime.block_on(client.recent_minute_bars(&instrument("AAPL"), FIVE_MINUTES));
    (read, transport)
}

fn read_body(body: &str) -> Result<MinuteBars, &'static str> {
    read_at(vec![inline(200, body)], NOW)
        .0
        .map_err(|e| e.code())
}

fn expected_good() -> MinuteBars {
    MinuteBars {
        instrument: instrument("AAPL"),
        bars: (5..10)
            .map(|minute| MinuteBar {
                start: instant(&format!("2026-09-28T16:5{minute}:00Z")),
                volume: qty(&format!("10{}", minute - 4)),
            })
            .collect(),
    }
}

/// The answer is read bar for bar, the request sent is the one built from the clock, and it is
/// sent once.
#[test]
fn a_good_answer_reads_every_bar_exactly() {
    let (read, transport) = read_at(vec![inline(200, &good())], NOW);
    assert_eq!(read, Ok(expected_good()));
    let sent = transport.sent_bars();
    assert_eq!(sent.len(), 1);
    assert_eq!(
        sent.first().map(BarsRequest::path_and_query),
        Some(bars_path("AAPL", "2026-09-28T16:55:00Z", "2026-09-28T16:59:00Z", 5).as_str())
    );
    assert!(transport.sent().is_empty(), "no quote read was sent");
}

/// A volume keeps the digits its token has, and only a plain decimal token is a volume.
#[test]
fn a_volume_is_the_tokens_own_digits() {
    let one = |v: &str| {
        answer(
            "AAPL",
            &format!("[{}]", bar("2026-09-28T16:59:00Z", v)),
            "null",
        )
    };
    assert_eq!(
        read_body(&one("123456789012.000000001")).map(|read| read.bars),
        Ok(vec![MinuteBar {
            start: instant("2026-09-28T16:59:00Z"),
            volume: qty("123456789012.000000001"),
        }])
    );
    assert_eq!(read_body(&good()), Ok(expected_good()));
    assert_eq!(read_body(&one("1e3")), Err("exponent_form"));
    assert_eq!(read_body(&one(r#""100""#)), Err("wrong_type"));
    assert_eq!(read_body(&one("0.0000000001")), Err("too_many_places"));
    assert_eq!(read_body(&one("-1")), Err("arithmetic"));
}

/// Another symbol, a second page, and no bars are each refused; so is a missing field.
#[test]
fn another_symbol_a_second_page_or_no_bars_is_refused() {
    assert_eq!(read_body(&good()), Ok(expected_good()));
    let bars = format!("[{}]", good_bars().join(","));
    let cases = [
        (answer("MSFT", &bars, "null"), "other_instrument"),
        (answer("AAPL", &bars, r#""QUFQTHwyMDI2""#), "paginated"),
        (answer("AAPL", "[]", "null"), "absent"),
        (answer("AAPL", "null", "null"), "absent"),
        (
            format!(r#"{{"bars":{bars},"symbol":"AAPL"}}"#),
            "missing_field",
        ),
        (
            r#"{"next_page_token":null,"symbol":"AAPL"}"#.to_owned(),
            "missing_field",
        ),
        (
            format!(r#"{{"bars":{bars},"next_page_token":null}}"#),
            "missing_field",
        ),
        (answer("AAPL", r#"{"t":1}"#, "null"), "wrong_type"),
        ("not json".to_owned(), "not_json"),
    ];
    for (body, expected) in cases {
        assert_eq!(read_body(&body), Err(expected), "{body}");
    }
}

/// A bar before the window, after its last closed minute, off the minute grid, repeated, or out
/// of order is refused; bars exactly at the first and last starts are inside.
#[test]
fn a_bar_outside_the_window_or_out_of_order_is_refused() {
    let with = |bars: &[String]| answer("AAPL", &format!("[{}]", bars.join(",")), "null");
    assert_eq!(read_body(&with(&good_bars())), Ok(expected_good()));
    let edges = [
        bar("2026-09-28T16:55:00Z", "1"),
        bar("2026-09-28T16:59:00Z", "2"),
    ];
    assert_eq!(read_body(&with(&edges)).map(|read| read.bars.len()), Ok(2));
    let cases = [
        vec![bar("2026-09-28T16:54:00Z", "1")],
        vec![bar("2026-09-28T17:00:00Z", "1")],
        vec![bar("2026-09-28T16:57:30Z", "1")],
        vec![bar("2026-09-28T16:57:00.000000001Z", "1")],
        vec![
            bar("2026-09-28T16:57:00Z", "1"),
            bar("2026-09-28T16:57:00Z", "1"),
        ],
        vec![
            bar("2026-09-28T16:58:00Z", "1"),
            bar("2026-09-28T16:57:00Z", "1"),
        ],
    ];
    for bars in cases {
        assert_eq!(read_body(&with(&bars)), Err("out_of_window"), "{bars:?}");
    }
    let unstamped = r#"{"v":1}"#.to_owned();
    assert_eq!(read_body(&with(&[unstamped])), Err("missing_field"));
    let numeric = r#"{"t":1,"v":1}"#.to_owned();
    assert_eq!(read_body(&with(&[numeric])), Err("wrong_type"));
    let unparsed = bar("yesterday", "1");
    assert_eq!(read_body(&with(&[unparsed])), Err("time"));
}

/// A status other than success is the read's refusal, and a transport failure is its own.
#[test]
fn a_failed_status_or_transport_is_refused() {
    assert_eq!(read_body(&good()), Ok(expected_good()));
    for (status, expected) in [
        (404, "absent"),
        (429, "overloaded"),
        (503, "overloaded"),
        (403, "unexpected_status"),
    ] {
        let (read, _) = read_at(vec![inline(status, &good())], NOW);
        assert_eq!(read.map_err(|e| e.code()), Err(expected), "{status}");
    }
    let (read, _) = read_at(vec![Err(TransportError::Timeout)], NOW);
    assert_eq!(read.map_err(|e| e.code()), Err("timeout"));
}

/// A clock whose window would start yesterday builds no request, so nothing is sent.
#[test]
fn a_refused_request_sends_nothing() {
    let (read, transport) = read_at(vec![inline(200, &good())], "2026-09-28T04:02:00Z");
    assert_eq!(read.map_err(|e| e.code()), Err("refused_path"));
    assert!(transport.sent_bars().is_empty());
}
