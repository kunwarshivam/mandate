//! Recorded Alpaca pages parse with every number exact; malformed pages are rejected.

mod common;

use common::{btc_1hour, btc_trades, day, scenario, shy_iex_trades, spy_sip_1hour};
use mandate_canon::DecStr;
use mandate_marketdata::alpaca::{self, WireError};
use mandate_marketdata::model::{Bar, Records, Trade};
use mandate_marketdata::number::NumberError;
use mandate_time::UtcNanos;

fn dec(s: &str) -> DecStr {
    DecStr::parse(s).unwrap()
}

fn at(s: &str) -> UtcNanos {
    UtcNanos::parse(s).unwrap()
}

#[test]
fn recorded_stock_bars_parse_with_exact_decimals() {
    let s = scenario("stock-bars-sip-spy-1hour-2026-09-24");
    let page = alpaca::parse_page(&spy_sip_1hour(), day("2026-09-24"), &s.bodies[0]).unwrap();
    assert_eq!(page.next_page_token, None);
    let Records::Bars(bars) = page.records else {
        panic!("bars expected")
    };
    assert_eq!(bars.len(), 16);
    assert_eq!(
        bars[0],
        Bar {
            start: at("2026-09-24T08:00:00.000000000Z"),
            open: dec("764.53"),
            high: dec("766.48"),
            low: dec("762.73"),
            close: dec("762.85"),
            volume: dec("140858"),
            vwap: dec("763.933979"),
            trade_count: 8256,
        }
    );
    assert_eq!(bars[1].high, dec("763.5559"));
}

#[test]
fn recorded_trades_parse_with_exact_decimals_and_vendor_fields() {
    let s = scenario("stock-trades-iex-shy-2026-09-24-paged");
    let page = alpaca::parse_page(&shy_iex_trades(), day("2026-09-24"), &s.bodies[0]).unwrap();
    assert_eq!(
        page.next_page_token.as_deref(),
        Some("U0hZfDE3OTAyNjIzNTAyNTI1MTgzNDN8VnwzMDE=")
    );
    let Records::Trades(trades) = page.records else {
        panic!("trades expected")
    };
    assert_eq!(trades.len(), 300);
    assert_eq!(
        trades[0],
        Trade {
            time: at("2026-09-24T13:30:01.051079925Z"),
            price: dec("81.19"),
            size: dec("1300"),
            trade_id: 1,
            exchange: Some("V".to_owned()),
            conditions: Some(vec!["@".to_owned()]),
            tape: Some("C".to_owned()),
            taker_side: None,
        }
    );

    let s = scenario("crypto-trades-btcusd-2026-09-24-paged");
    let page = alpaca::parse_page(&btc_trades(), day("2026-09-24"), &s.bodies[0]).unwrap();
    let Records::Trades(trades) = page.records else {
        panic!("trades expected")
    };
    assert_eq!(
        trades[0],
        Trade {
            time: at("2026-09-24T00:01:03.754506982Z"),
            price: dec("84432.274"),
            size: dec("0.00100291"),
            trade_id: 528_371_953_027_465_522,
            exchange: None,
            conditions: None,
            tape: None,
            taker_side: Some("S".to_owned()),
        }
    );
}

#[test]
fn a_record_at_the_exclusive_end_belongs_to_the_next_day() {
    let s = scenario("crypto-bars-btcusd-1hour-2026-09-24");
    let recorded = String::from_utf8(s.bodies[0].clone()).unwrap();
    assert_eq!(
        recorded.matches("\"t\":").count(),
        25,
        "Alpaca's end is inclusive"
    );
    assert!(recorded.contains("\"t\":\"2026-09-25T00:00:00Z\""));
    let page = alpaca::parse_page(&btc_1hour(), day("2026-09-24"), &s.bodies[0]).unwrap();
    let times = page.records.times();
    assert_eq!(times.len(), 24);
    assert_eq!(times.last(), Some(&at("2026-09-24T23:00:00.000000000Z")));
}

#[test]
fn an_empty_day_is_an_empty_page() {
    let s = scenario("stock-bars-sip-spy-1hour-2026-09-19");
    let page = alpaca::parse_page(&spy_sip_1hour(), day("2026-09-19"), &s.bodies[0]).unwrap();
    assert!(page.records.is_empty());
    assert_eq!(page.next_page_token, None);
    let null_map = br#"{"bars":null,"next_page_token":null}"#;
    let page = alpaca::parse_page(&spy_sip_1hour(), day("2026-09-19"), null_map).unwrap();
    assert!(page.records.is_empty());
}

fn bar_json(fields: &str) -> Vec<u8> {
    format!(r#"{{"bars":{{"SPY":[{{{fields}}}]}},"next_page_token":null}}"#).into_bytes()
}

const GOOD_BAR: &str =
    r#""c":1.5,"h":2,"l":1,"n":3,"o":1.25,"t":"2026-09-24T08:00:00Z","v":10,"vw":1.4"#;

fn parse_bar(body: &[u8]) -> Result<Records, WireError> {
    alpaca::parse_page(&spy_sip_1hour(), day("2026-09-24"), body).map(|p| p.records)
}

#[test]
fn a_well_formed_bar_parses() {
    assert_eq!(parse_bar(&bar_json(GOOD_BAR)).map(|r| r.len()), Ok(1));
}

#[test]
fn malformed_numbers_in_records_are_rejected() {
    let cases: [(&str, &str, &str); 7] = [
        ("\"o\":1.25", "\"o\":\"1.25\"", "o"),
        ("\"v\":10", "\"v\":1e999", "v"),
        ("\"vw\":1.4", "\"vw\":null", "vw"),
        ("\"n\":3", "\"n\":3.0", "n"),
        ("\"n\":3", "\"n\":-3", "n"),
        ("\"h\":2", "\"h\":[2]", "h"),
        ("\"c\":1.5", "\"c\":true", "c"),
    ];
    for (good, bad, field) in cases {
        let fields = GOOD_BAR.replace(good, bad);
        assert_ne!(fields, GOOD_BAR);
        match parse_bar(&bar_json(&fields)) {
            Err(WireError::Number { field: f, .. }) => assert_eq!(f, field, "{bad}"),
            other => panic!("{bad}: expected a number error, got {other:?}"),
        }
    }
    let negative = GOOD_BAR.replace("\"l\":1", "\"l\":-1");
    assert_eq!(
        parse_bar(&bar_json(&negative)),
        Err(WireError::Negative {
            field: "l",
            raw: "-1".to_owned()
        })
    );
    let out_of_range = GOOD_BAR.replace("\"v\":10", "\"v\":1e400");
    assert!(matches!(
        parse_bar(&bar_json(&out_of_range)),
        Err(WireError::Number {
            field: "v",
            source: NumberError::Decimal(_),
            ..
        })
    ));
}

#[test]
fn missing_and_duplicate_members_are_rejected() {
    let missing_field = GOOD_BAR.replace(",\"vw\":1.4", "");
    assert!(matches!(
        parse_bar(&bar_json(&missing_field)),
        Err(WireError::Json(_))
    ));
    let duplicate_field = format!("{GOOD_BAR},\"o\":9");
    assert!(matches!(
        parse_bar(&bar_json(&duplicate_field)),
        Err(WireError::Json(_))
    ));
    let duplicate_symbol = br#"{"bars":{"SPY":[],"SPY":[]},"next_page_token":null}"#;
    assert_eq!(
        parse_bar(duplicate_symbol),
        Err(WireError::DuplicateSymbol("SPY".to_owned()))
    );
    assert_eq!(
        parse_bar(br#"{"message":"forbidden."}"#),
        Err(WireError::MissingMember("bars"))
    );
    assert_eq!(
        parse_bar(br#"{"bars":{}}"#),
        Err(WireError::MissingMember("next_page_token"))
    );
    assert!(matches!(parse_bar(b"not json"), Err(WireError::Json(_))));
}

#[test]
fn data_for_another_symbol_is_rejected() {
    let body = br#"{"bars":{"QQQ":[]},"next_page_token":null}"#;
    assert_eq!(
        parse_bar(body),
        Err(WireError::UnexpectedSymbol("QQQ".to_owned()))
    );
}

#[test]
fn records_outside_the_day_and_bad_timestamps_are_rejected() {
    let before = GOOD_BAR.replace("2026-09-24T08:00:00Z", "2026-09-23T23:59:59.999999999Z");
    assert_eq!(
        parse_bar(&bar_json(&before)),
        Err(WireError::OutsideDay {
            time: at("2026-09-23T23:59:59.999999999Z"),
            day: day("2026-09-24"),
        })
    );
    let after = GOOD_BAR.replace("2026-09-24T08:00:00Z", "2026-09-25T00:00:00.000000001Z");
    assert!(matches!(
        parse_bar(&bar_json(&after)),
        Err(WireError::OutsideDay { .. })
    ));
    let offset = GOOD_BAR.replace("2026-09-24T08:00:00Z", "2026-09-24T08:00:00+00:00");
    assert!(matches!(
        parse_bar(&bar_json(&offset)),
        Err(WireError::Timestamp { .. })
    ));
}
