//! Corporate actions (backlog E2-4, trading domain spec §4.5, §8.5): the Alpaca wire format against
//! recorded responses, the paged fetch and its ex-date window, and point-in-time split adjustment
//! checked by hand, against the trading-domain reference cases, and against an oracle that tests
//! the rounding definition instead of recomputing it.

mod common;

use common::{FakeTransport, RecordingPause, day, fixtures_dir, ok, scenario};
use mandate_canon::DecStr;
use mandate_marketdata::alpaca::{WireError, corporate_actions_path, parse_corporate_actions};
use mandate_marketdata::client::{Client, FetchError};
use mandate_marketdata::model::{
    ADJUSTED_PRICE_SCALE, AdjustmentError, Bar, CorporateActions, DayRange, Split, SplitRatio,
    Symbol, adjust_price, adjust_quantity, compose, split_ratio,
};
use mandate_marketdata::number::{NumberError, from_units};
use mandate_marketdata::timestamp::TimestampError;
use mandate_num::NumError;
use mandate_time::{Date, TimeError, UtcNanos};
use proptest::prelude::*;
use serde_json::Value;

fn symbol(s: &str) -> Symbol {
    Symbol::parse(s).unwrap()
}

fn dec(s: &str) -> DecStr {
    DecStr::parse(s).unwrap()
}

fn at(s: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(s).unwrap()
}

fn range(first: &str, last: &str) -> DayRange {
    DayRange::new(day(first), day(last)).unwrap()
}

fn recorded_page(name: &str, sym: &str) -> CorporateActions {
    let s = scenario(name);
    assert_eq!(s.bodies.len(), 1, "{name} is one page");
    parse_corporate_actions(&symbol(sym), &s.bodies[0])
        .unwrap()
        .actions
}

fn recorded(name: &str, sym: &str, ex_dates: DayRange) -> CorporateActions {
    recorded_page(name, sym).dated_in(ex_dates)
}

fn ge() -> CorporateActions {
    recorded(
        "corporate-actions-ge-2018-2026",
        "GE",
        range("2018-01-01", "2026-09-25"),
    )
}

fn nvda() -> CorporateActions {
    recorded(
        "corporate-actions-nvda-2021-2024",
        "NVDA",
        range("2021-01-01", "2024-12-31"),
    )
}

fn ratio(new: u64, old: u64) -> SplitRatio {
    split_ratio(new, old).unwrap()
}

fn split(ex_date: &str, new: u64, old: u64) -> Split {
    Split {
        id: format!("split-{ex_date}"),
        ex_date: day(ex_date),
        ratio: ratio(new, old),
    }
}

fn with_splits(splits: Vec<Split>) -> CorporateActions {
    CorporateActions {
        splits,
        ..CorporateActions::none(symbol("XYZ"))
    }
}

fn bar(start: &str, price: &str, volume: &str) -> Bar {
    Bar {
        start: at(start),
        open: dec(price),
        high: dec(price),
        low: dec(price),
        close: dec(price),
        volume: dec(volume),
        vwap: dec(price),
        trade_count: 7,
    }
}

#[test]
fn the_request_covers_process_dates_from_31_days_before_to_366_days_after_the_ex_dates() {
    assert_eq!(
        corporate_actions_path(&symbol("AAPL"), range("2020-01-01", "2021-12-31"), 5, None),
        Ok(
            "/v1/corporate-actions?symbols=AAPL&start=2019-12-01&end=2023-01-01&limit=5&sort=asc"
                .to_owned()
        )
    );
    assert_eq!(
        corporate_actions_path(
            &symbol("BRK.B"),
            range("2024-03-01", "2024-03-01"),
            1000,
            Some("QUF=")
        ),
        Ok("/v1/corporate-actions?symbols=BRK.B&start=2024-01-30&end=2025-03-02&limit=1000&sort=asc&page_token=QUF%3D".to_owned())
    );
    assert_eq!(
        corporate_actions_path(&symbol("XYZ"), range("1970-01-31", "1970-01-31"), 5, None),
        Err(TimestampError::Time(TimeError::OutOfRange)),
        "a window starting before 1970 is out of range"
    );
    assert_eq!(
        corporate_actions_path(&symbol("XYZ"), range("1970-02-01", "1970-02-01"), 5, None),
        Ok(
            "/v1/corporate-actions?symbols=XYZ&start=1970-01-01&end=1971-02-02&limit=5&sort=asc"
                .to_owned()
        )
    );
    assert_eq!(
        corporate_actions_path(&symbol("XYZ"), range("9999-01-01", "9999-01-01"), 5, None),
        Err(TimestampError::Time(TimeError::OutOfRange))
    );
}

#[test]
fn a_reverse_split_dividends_and_spin_offs_parse_from_the_recorded_ge_response() {
    let ge = ge();
    assert_eq!(ge.symbol, symbol("GE"));
    assert_eq!(
        ge.splits,
        vec![Split {
            id: "74199d56-24db-4226-9678-6363edc1531d".to_owned(),
            ex_date: day("2021-08-02"),
            ratio: ratio(1, 8),
        }]
    );
    assert_eq!(ge.cash_dividends.len(), 35);
    let first = &ge.cash_dividends[0];
    assert_eq!(first.id, "5e4b4491-5059-4dd3-bc30-46605c041b24");
    assert_eq!(first.ex_date, day("2018-02-23"));
    assert_eq!((first.record_date, first.payable_date), (None, None));
    assert_eq!(first.rate, dec("0.12"));
    assert!(!first.special && !first.foreign);
    assert_eq!(
        ge.cash_dividends.last().map(|d| d.ex_date),
        Some(day("2026-07-06"))
    );
    let other: Vec<(&str, &str, Option<Date>, Date)> = ge
        .other
        .iter()
        .map(|o| (o.kind.as_str(), o.id.as_str(), o.ex_date, o.process_date))
        .collect();
    assert_eq!(
        other,
        vec![
            (
                "spin_offs",
                "15a5c363-f85e-467f-81be-1f818982e45e",
                Some(day("2019-02-25")),
                day("2019-02-25")
            ),
            (
                "spin_offs",
                "0b2e940e-e28d-4944-9e08-9e9761769d92",
                Some(day("2023-01-04")),
                day("2023-01-04")
            ),
            (
                "spin_offs",
                "27cadc74-26f2-43fe-b5ef-8cc7e5affc30",
                Some(day("2024-04-02")),
                day("2024-04-02")
            ),
        ]
    );
    let page = recorded_page("corporate-actions-ge-2018-2026", "GE");
    let outside: Vec<(Date, Option<Date>)> = page
        .cash_dividends
        .iter()
        .filter(|d| !ge.cash_dividends.contains(d))
        .map(|d| (d.ex_date, d.payable_date))
        .collect();
    assert_eq!(
        outside,
        vec![
            (day("2017-12-26"), None),
            (day("2026-10-05"), Some(day("2026-10-26")))
        ],
        "the wider process-date window also returns dividends dated outside the range"
    );
}

#[test]
fn forward_splits_and_dated_dividends_parse_from_the_recorded_nvda_response() {
    let nvda = nvda();
    let splits: Vec<(Date, SplitRatio)> =
        nvda.splits.iter().map(|s| (s.ex_date, s.ratio)).collect();
    assert_eq!(
        splits,
        vec![
            (day("2021-07-20"), ratio(4, 1)),
            (day("2024-06-10"), ratio(10, 1))
        ]
    );
    assert_eq!(nvda.cash_dividends.len(), 16);
    let first = &nvda.cash_dividends[0];
    assert_eq!(first.ex_date, day("2021-03-09"));
    assert_eq!(first.record_date, Some(day("2021-03-10")));
    assert_eq!(first.payable_date, Some(day("2021-03-31")));
    assert_eq!(first.rate, dec("0.16"));
    assert!(nvda.other.is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn the_client_follows_corporate_action_pages_to_the_end() {
    let s = scenario("corporate-actions-aapl-2020-2021-paged");
    assert!(s.requests.len() > 1, "the scenario must span several pages");
    let transport = FakeTransport::serving(s.bodies.iter().map(|b| ok(b)));
    let client = Client::new(transport.clone(), RecordingPause::default()).with_page_limit(5);
    let aapl = client
        .fetch_corporate_actions(&symbol("AAPL"), range("2020-01-01", "2021-12-31"))
        .await
        .unwrap();
    assert_eq!(transport.requested(), s.requests);
    assert_eq!(transport.unused(), 0);
    assert_eq!(
        aapl.cash_dividends.len(),
        8,
        "the 2022 dividends are dropped"
    );
    assert_eq!(
        aapl.cash_dividends.last().map(|d| d.ex_date),
        Some(day("2021-11-05"))
    );
    assert_eq!(
        aapl.splits,
        vec![Split {
            id: "d209b6c2-9231-474c-a80e-1a79dacadbb2".to_owned(),
            ex_date: day("2020-08-31"),
            ratio: ratio(4, 1),
        }]
    );
    let ex_dates: Vec<Date> = aapl.cash_dividends.iter().map(|d| d.ex_date).collect();
    assert!(ex_dates.windows(2).all(|w| w[0] < w[1]), "{ex_dates:?}");
}

const EMPTY: &str = r#"{"corporate_actions":{},"next_page_token":null}"#;

fn page_with(dividend_ids: &[&str], token: Option<&str>) -> Vec<u8> {
    let dividends: Vec<String> = dividend_ids
        .iter()
        .map(|id| {
            format!(
                r#"{{"id":"{id}","symbol":"XYZ","ex_date":"2024-01-05","rate":0.1,"special":false,"foreign":false,"process_date":"2024-01-05"}}"#
            )
        })
        .collect();
    let token = token.map_or("null".to_owned(), |t| format!("\"{t}\""));
    format!(
        r#"{{"corporate_actions":{{"cash_dividends":[{}]}},"next_page_token":{token}}}"#,
        dividends.join(",")
    )
    .into_bytes()
}

#[tokio::test(flavor = "current_thread")]
async fn corporate_action_pages_are_at_most_1000_and_an_action_seen_twice_is_an_error() {
    let transport = FakeTransport::serving([ok(EMPTY.as_bytes())]);
    let client = Client::new(transport.clone(), RecordingPause::default());
    let none = client
        .fetch_corporate_actions(&symbol("XYZ"), range("2024-01-01", "2024-12-31"))
        .await
        .unwrap();
    assert!(none.is_empty());
    assert!(
        transport.requested()[0].contains("&limit=1000&"),
        "{:?}",
        transport.requested()
    );

    let transport = FakeTransport::serving([
        ok(&page_with(&["a", "b"], Some("next"))),
        ok(&page_with(&["b"], None)),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    assert_eq!(
        client
            .fetch_corporate_actions(&symbol("XYZ"), range("2024-01-01", "2024-12-31"))
            .await,
        Err(FetchError::Wire {
            page: 2,
            source: WireError::DuplicateAction("b".to_owned())
        })
    );

    let transport = FakeTransport::serving([
        ok(&page_with(&["a"], Some("same"))),
        ok(&page_with(&["b"], Some("same"))),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    assert_eq!(
        client
            .fetch_corporate_actions(&symbol("XYZ"), range("2024-01-01", "2024-12-31"))
            .await,
        Err(FetchError::RepeatedPageToken { page: 2 })
    );
}

fn dated_page(actions: &[(&str, &str, &str)]) -> Vec<u8> {
    let mut dividends = Vec::new();
    let mut splits = Vec::new();
    let mut other = Vec::new();
    for (id, ex_date, process_date) in actions {
        let dated = |extra: &str| {
            format!(r#"{{"id":"{id}","symbol":"XYZ",{extra}"process_date":"{process_date}"}}"#)
        };
        let ex = if ex_date.is_empty() {
            String::new()
        } else {
            format!(r#""ex_date":"{ex_date}","#)
        };
        match id.split_once('-').map(|(kind, _)| kind) {
            Some("dividend") => dividends.push(dated(&format!(
                r#"{ex}"rate":0.1,"special":false,"foreign":false,"#
            ))),
            Some("split") => splits.push(dated(&format!(r#"{ex}"new_rate":2,"old_rate":1,"#))),
            _ => other.push(dated(&ex)),
        }
    }
    format!(
        r#"{{"corporate_actions":{{"cash_dividends":[{}],"forward_splits":[{}],"name_changes":[{}]}},"next_page_token":null}}"#,
        dividends.join(","),
        splits.join(","),
        other.join(",")
    )
    .into_bytes()
}

#[tokio::test(flavor = "current_thread")]
async fn actions_are_kept_by_ex_date_whatever_their_process_date() {
    let transport = FakeTransport::serving([ok(&dated_page(&[
        ("dividend-before", "2024-02-29", "2024-03-05"),
        ("dividend-first", "2024-03-01", "2024-04-15"),
        ("dividend-last", "2024-03-29", "2024-05-20"),
        ("dividend-after", "2024-04-01", "2024-04-01"),
        ("split-before", "2024-02-29", "2024-03-01"),
        ("split-first", "2024-03-01", "2024-03-01"),
        ("split-last", "2024-03-29", "2024-03-29"),
        ("split-after", "2024-04-01", "2024-03-29"),
        ("other-before", "2024-02-29", "2024-03-01"),
        ("other-last", "2024-03-29", "2024-04-01"),
        ("other-undated-first", "", "2024-03-01"),
        ("other-undated-before", "", "2024-02-29"),
        ("other-undated-after", "", "2024-04-01"),
    ]))]);
    let client = Client::new(transport.clone(), RecordingPause::default());
    let march = client
        .fetch_corporate_actions(&symbol("XYZ"), range("2024-03-01", "2024-03-29"))
        .await
        .unwrap();
    assert_eq!(
        transport.requested(),
        vec![
            "/v1/corporate-actions?symbols=XYZ&start=2024-01-30&end=2025-03-30&limit=1000&sort=asc"
                .to_owned()
        ]
    );
    let ids: Vec<&str> = march.ids().collect();
    assert_eq!(
        ids,
        [
            "split-first",
            "split-last",
            "dividend-first",
            "dividend-last",
            "other-last",
            "other-undated-first"
        ],
        "an action processed after the range but dated in it is kept, and one processed in the range but dated outside it is dropped"
    );

    let s = scenario("corporate-actions-nvda-2021-2024");
    let transport = FakeTransport::serving(s.bodies.iter().map(|b| ok(b)));
    let client = Client::new(transport.clone(), RecordingPause::default());
    let fetched = client
        .fetch_corporate_actions(&symbol("NVDA"), range("2021-01-01", "2024-12-31"))
        .await
        .unwrap();
    assert_eq!(transport.requested(), s.requests);
    assert_eq!(fetched, nvda());
    let page = recorded_page("corporate-actions-nvda-2021-2024", "NVDA");
    let dropped: Vec<Date> = page
        .cash_dividends
        .iter()
        .filter(|d| !fetched.cash_dividends.contains(d))
        .map(|d| d.ex_date)
        .collect();
    assert_eq!(
        dropped,
        [
            day("2020-12-03"),
            day("2025-03-12"),
            day("2025-06-11"),
            day("2025-09-11"),
            day("2025-12-04")
        ],
        "the wider window returns the dividends dated just outside the range, and the ex-date filter drops them"
    );
}

fn parse(body: &str) -> Result<CorporateActions, WireError> {
    parse_corporate_actions(&symbol("XYZ"), body.as_bytes()).map(|p| p.actions)
}

fn splits_page(new_rate: &str, old_rate: &str, sym: &str, ex_date: &str) -> String {
    format!(
        r#"{{"corporate_actions":{{"forward_splits":[{{"id":"s","symbol":"{sym}","ex_date":"{ex_date}","new_rate":{new_rate},"old_rate":{old_rate},"process_date":"2024-01-05"}}]}},"next_page_token":null}}"#
    )
}

#[test]
fn malformed_corporate_action_pages_are_rejected_with_their_reason() {
    assert_eq!(
        parse(r#"{"next_page_token":null}"#),
        Err(WireError::MissingMember("corporate_actions"))
    );
    assert_eq!(
        parse(r#"{"corporate_actions":{}}"#),
        Err(WireError::MissingMember("next_page_token"))
    );
    assert!(matches!(parse("null"), Err(WireError::Json(_))));
    assert!(matches!(
        parse(r#"{"corporate_actions":[],"next_page_token":null}"#),
        Err(WireError::Json(_))
    ));
    assert_eq!(
        parse(&splits_page("4", "1", "ABC", "2024-01-05")),
        Err(WireError::UnexpectedSymbol("ABC".to_owned()))
    );
    assert_eq!(
        parse(&splits_page("0", "1", "XYZ", "2024-01-05")),
        Err(WireError::Split {
            id: "s".to_owned(),
            source: AdjustmentError::Ratio {
                new: 0,
                old: 1,
                source: NumError::NotPositive
            }
        })
    );
    assert_eq!(
        parse(&splits_page("1.5", "1", "XYZ", "2024-01-05")),
        Err(WireError::Number {
            field: "new_rate",
            raw: "1.5".to_owned(),
            source: NumberError::NotAnInteger
        })
    );
    assert_eq!(
        parse(&splits_page("3", "\"2\"", "XYZ", "2024-01-05")),
        Err(WireError::Number {
            field: "old_rate",
            raw: "\"2\"".to_owned(),
            source: NumberError::NotAnInteger
        })
    );
    assert!(matches!(
        parse(&splits_page("3", "2", "XYZ", "2024-02-30")),
        Err(WireError::Date {
            field: "ex_date",
            ..
        })
    ));
    let dividend = |rate: &str, payable: &str| {
        format!(
            r#"{{"corporate_actions":{{"cash_dividends":[{{"id":"d","symbol":"XYZ","ex_date":"2024-01-05","payable_date":{payable},"rate":{rate},"special":true,"foreign":false,"process_date":"2024-01-05"}}]}},"next_page_token":null}}"#
        )
    };
    assert_eq!(
        parse(&dividend("-0.1", "\"2024-01-20\"")),
        Err(WireError::Negative {
            field: "rate",
            raw: "-0.1".to_owned()
        })
    );
    assert!(matches!(
        parse(&dividend("0.1", "\"soon\"")),
        Err(WireError::Date {
            field: "payable_date",
            ..
        })
    ));
    let special = parse(&dividend("0.1", "\"2024-01-20\"")).unwrap();
    assert!(special.cash_dividends[0].special);
    assert_eq!(
        special.cash_dividends[0].payable_date,
        Some(day("2024-01-20"))
    );
    assert!(matches!(
        parse(r#"{"corporate_actions":{"name_changes":[{"id":"n"}]},"next_page_token":null}"#),
        Err(WireError::Json(_))
    ));
    assert!(matches!(
        parse(
            r#"{"corporate_actions":{"name_changes":[{"id":"n","process_date":"May 1"}]},"next_page_token":null}"#
        ),
        Err(WireError::Date {
            field: "process_date",
            ..
        })
    ));
    assert_eq!(
        parse(
            r#"{"corporate_actions":{"name_changes":[{"id":"n","process_date":"2024-01-05"}],"cash_mergers":[{"id":"n","process_date":"2024-01-06"}]},"next_page_token":null}"#
        ),
        Err(WireError::DuplicateAction("n".to_owned()))
    );
}

#[test]
fn wire_error_codes_are_stable() {
    let cases = [
        (
            WireError::Date {
                field: "ex_date",
                raw: "x".to_owned(),
                source: TimeError::Syntax,
            },
            "date",
        ),
        (
            WireError::Split {
                id: "s".to_owned(),
                source: AdjustmentError::Ratio {
                    new: 0,
                    old: 1,
                    source: NumError::NotPositive,
                },
            },
            "split",
        ),
        (
            WireError::DuplicateAction("a".to_owned()),
            "duplicate_action",
        ),
    ];
    for (error, code) in cases {
        assert_eq!(error.code(), code, "{error}");
    }
    let adjustment = [
        (
            AdjustmentError::Ratio {
                new: 1,
                old: 0,
                source: NumError::NotPositive,
            },
            "ratio",
        ),
        (AdjustmentError::RatioOverflow, "ratio_overflow"),
        (
            AdjustmentError::Price {
                price: dec("1"),
                source: NumError::TooPrecise,
            },
            "price",
        ),
        (
            AdjustmentError::AdjustedPrice("x".to_owned(), mandate_canon::DecError::Syntax),
            "adjusted_price",
        ),
        (AdjustmentError::Overflow, "overflow"),
        (AdjustmentError::Number(NumberError::NotANumber), "number"),
        (AdjustmentError::Time(TimeError::OutOfRange), "time"),
    ];
    for (error, code) in adjustment {
        assert_eq!(error.code(), code, "{error}");
    }
}

#[test]
fn split_ratios_are_positive_and_compose_in_lowest_terms() {
    for (new, old) in [(0, 3), (3, 0)] {
        assert_eq!(
            split_ratio(new, old),
            Err(AdjustmentError::Ratio {
                new,
                old,
                source: NumError::NotPositive
            })
        );
    }
    let r = ratio(6, 4);
    assert_eq!((r.new_shares(), r.old_shares()), (6, 4));
    assert_eq!(compose(r, ratio(1, 1)), Ok(ratio(3, 2)));
    assert_eq!(compose(ratio(3, 3), ratio(5, 5)), Ok(ratio(1, 1)));
    assert_eq!(compose(ratio(4, 1), ratio(1, 8)), Ok(ratio(1, 2)));
    assert_eq!(compose(ratio(4, 1), ratio(10, 1)), Ok(ratio(40, 1)));
    assert_eq!(compose(ratio(1, 10), ratio(4, 1)), Ok(ratio(2, 5)));
    let big = ratio(u64::MAX, 1);
    assert_eq!(compose(big, ratio(1, u64::MAX)), Ok(ratio(1, 1)));
    assert_eq!(
        compose(big, ratio(2, 1)),
        Err(AdjustmentError::RatioOverflow)
    );
    let small = ratio(1, u64::MAX);
    assert_eq!(
        compose(small, ratio(1, 2)),
        Err(AdjustmentError::RatioOverflow)
    );
    assert_eq!(compose(big, big), Err(AdjustmentError::RatioOverflow));
}

#[test]
fn prices_divide_by_the_ratio_to_12_places_half_even_and_quantities_multiply() {
    let cases = [
        (ratio(4, 1), "499.23", "124.8075"),
        (ratio(1, 8), "12.94", "103.52"),
        (ratio(3, 2), "100", "66.666666666667"),
        (ratio(3, 1), "1", "0.333333333333"),
        (ratio(6, 1), "1", "0.166666666667"),
        (ratio(2, 1), "0.000000000003", "0.000000000002"),
        (ratio(2, 1), "0.000000000005", "0.000000000002"),
        (ratio(2, 1), "0.000000000007", "0.000000000004"),
        (ratio(1, 1), "1.123456789012", "1.123456789012"),
    ];
    for (r, price, adjusted) in cases {
        assert_eq!(adjust_price(r, &dec(price)), Ok(dec(adjusted)), "{price}");
    }
    let rejected = [
        (ratio(2, 1), "0.000000000001", NumError::NotPositive),
        (ratio(2, 1), "0.0000000000031", NumError::TooPrecise),
        (ratio(2, 1), "-1", NumError::NotPositive),
        (ratio(2, 1), "0", NumError::NotPositive),
        (
            ratio(1, u64::MAX),
            "99999999999999999999",
            NumError::Overflow,
        ),
    ];
    for (r, price, source) in rejected {
        assert_eq!(
            adjust_price(r, &dec(price)),
            Err(AdjustmentError::Price {
                price: dec(price),
                source
            }),
            "{price}"
        );
    }
    let quantities = [
        (ratio(4, 1), "1000", 18, "4000"),
        (ratio(1, 8), "800", 18, "100"),
        (ratio(1, 8), "3", 18, "0.375"),
        (ratio(1, 3), "1", 18, "0.333333333333333333"),
        (ratio(1, 3), "2", 0, "1"),
        (ratio(1, 2), "5", 0, "2"),
        (ratio(1, 2), "7", 0, "4"),
        (
            ratio(2, 1),
            "-0.0000000000000000005",
            18,
            "-0.000000000000000001",
        ),
        (ratio(1, 2), "-5", 0, "-2"),
        (ratio(1, 2), "-7", 0, "-4"),
    ];
    for (r, quantity, scale, adjusted) in quantities {
        assert_eq!(
            adjust_quantity(r, &dec(quantity), scale),
            Ok(dec(adjusted)),
            "{quantity} at {scale}"
        );
    }
    assert_eq!(
        adjust_quantity(ratio(1, 1), &dec("1"), 29),
        Err(AdjustmentError::Number(NumberError::UnsupportedScale {
            scale: 29
        }))
    );
    assert_eq!(
        adjust_quantity(ratio(u64::MAX, 1), &dec("100"), 18),
        Err(AdjustmentError::Overflow)
    );
    assert_eq!(
        adjust_quantity(ratio(1, u64::MAX), &dec("1"), 28),
        Ok(dec("0.0000000000000000000542101086"))
    );
}

/// Every split in the trading-domain reference cases (`fixtures/refcases/trading-domain.json`,
/// variants included) with an expected mark: a bar at the last mark before the split, adjusted as
/// of the ex-date, closes at the mark the accounting fold is expected to store.
#[test]
fn adjusted_prices_match_the_marks_of_the_reference_case_splits() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/refcases/trading-domain.json"
    );
    let suite: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let text = |v: &Value| v.as_str().unwrap().to_owned();
    let mut checked = Vec::new();
    for case in suite["cases"].as_array().unwrap() {
        let mut runs = vec![(text(&case["id"]), case["steps"].clone())];
        for variant in case["variants"].as_array().into_iter().flatten() {
            if let Some(steps) = variant["overrides"].get("steps") {
                runs.push((
                    format!("{} {}", text(&case["id"]), text(&variant["name"])),
                    steps.clone(),
                ));
            }
        }
        for (name, steps) in runs {
            let mut last_mark = std::collections::BTreeMap::new();
            for step in steps.as_array().into_iter().flatten() {
                let data = &step["data"];
                match step["event"].as_str() {
                    Some("mark") => {
                        last_mark.insert(text(&data["instrument"]), text(&data["price"]));
                    }
                    Some("corporate_action_applied") if data["type"] == "split" => {
                        let instrument = text(&data["instrument"]);
                        let Some(expected) = step["expect"]["marks"].get(&instrument) else {
                            continue;
                        };
                        let price = &last_mark[&instrument];
                        let new = data["ratio"]["new"].as_u64().unwrap();
                        let old = data["ratio"]["old"].as_u64().unwrap();
                        let ex_date = day(&text(&data["ex_date"]));
                        let actions = with_splits(vec![Split {
                            id: name.clone(),
                            ex_date,
                            ratio: ratio(new, old),
                        }]);
                        let before = at("2020-01-02T15:00:00Z");
                        let adjusted = actions
                            .adjust_bar(&bar("2020-01-02T15:00:00Z", price, "1"), ex_date, 18)
                            .unwrap();
                        assert_eq!(adjusted.start, before);
                        assert_eq!(adjusted.close, dec(&text(expected)), "{name}");
                        checked.push(name.clone());
                    }
                    _ => {}
                }
            }
        }
    }
    assert_eq!(
        checked,
        [
            "RC-04",
            "RC-05",
            "RC-23",
            "RC-23 forward_3_for_1_non_terminating_mark"
        ]
    );
}

#[test]
fn a_split_takes_effect_at_20_00_new_york_time_the_evening_before_its_ex_date() {
    let cases = [
        ("2020-08-31", "2020-08-31T00:00:00Z"),
        ("2024-01-08", "2024-01-08T01:00:00Z"),
        ("2024-03-11", "2024-03-11T00:00:00Z"),
        ("2024-11-04", "2024-11-04T01:00:00Z"),
        ("2024-03-10", "2024-03-10T01:00:00Z"),
        ("2024-11-03", "2024-11-03T00:00:00Z"),
    ];
    for (ex_date, effective) in cases {
        assert_eq!(
            split(ex_date, 2, 1).effective_at(),
            Ok(at(effective)),
            "{ex_date}"
        );
    }
    assert!(matches!(
        split("9999-12-31", 2, 1).effective_at(),
        Err(AdjustmentError::Time(_))
    ));
}

#[test]
fn bars_before_a_split_are_adjusted_only_once_its_ex_date_is_known() {
    let aapl = with_splits(vec![split("2020-08-31", 4, 1)]);
    let friday = bar("2020-08-28T19:00:00Z", "499.23", "1000");
    assert_eq!(
        aapl.adjust_bar(&friday, day("2020-08-28"), 18),
        Ok(friday.clone()),
        "the split is not known on the day before its ex-date"
    );
    assert_eq!(
        aapl.adjust_bar(&friday, day("2020-08-30"), 18),
        Ok(friday.clone())
    );
    let adjusted = aapl.adjust_bar(&friday, day("2020-08-31"), 18).unwrap();
    assert_eq!(adjusted, bar("2020-08-28T19:00:00Z", "124.8075", "4000"));
    assert_eq!(adjusted.trade_count, friday.trade_count);
    assert_eq!(
        aapl.adjust_bar(&friday, day("2026-01-01"), 18),
        Ok(adjusted)
    );
    let before = bar("2020-08-30T23:59:59Z", "500", "1");
    assert_eq!(
        aapl.adjust_bar(&before, day("2020-08-31"), 18),
        Ok(bar("2020-08-30T23:59:59Z", "125", "4"))
    );
    let precise = bar("2020-08-28T19:00:00Z", "0.123456789012345678", "1");
    for (actions, as_of) in [
        (&aapl, day("2020-08-30")),
        (
            &CorporateActions::none(symbol("BTC/USD")),
            day("2026-01-01"),
        ),
    ] {
        assert_eq!(
            actions.adjust_bar(&precise, as_of, 18),
            Ok(precise.clone()),
            "a bar no split applies to is kept as stored, even beyond a mark's 12 places"
        );
    }
    assert!(matches!(
        aapl.adjust_bar(&precise, day("2020-08-31"), 18),
        Err(AdjustmentError::Price {
            source: NumError::TooPrecise,
            ..
        })
    ));
    for post in ["2020-08-31T00:00:00Z", "2020-08-31T13:30:00Z"] {
        let b = bar(post, "125", "4");
        assert_eq!(
            aapl.adjust_bar(&b, day("2026-01-01"), 18),
            Ok(b.clone()),
            "{post}: the overnight session into the ex-date trades post-split"
        );
    }
}

#[test]
fn splits_compound_point_in_time_and_other_actions_never_adjust() {
    let nvda = nvda();
    let early = bar("2021-07-16T19:00:00Z", "745", "10");
    assert_eq!(
        nvda.adjust_bar(&early, day("2021-07-19"), 18),
        Ok(early.clone())
    );
    assert_eq!(
        nvda.adjust_bar(&early, day("2024-06-07"), 18),
        Ok(bar("2021-07-16T19:00:00Z", "186.25", "40"))
    );
    assert_eq!(
        nvda.adjust_bar(&early, day("2024-06-10"), 18),
        Ok(bar("2021-07-16T19:00:00Z", "18.625", "400"))
    );
    let between = bar("2022-06-01T15:00:00Z", "180", "1");
    assert_eq!(
        nvda.adjust_bar(&between, day("2024-12-31"), 18),
        Ok(bar("2022-06-01T15:00:00Z", "18", "10"))
    );
    assert_eq!(
        nvda.split_ratio_after(at("2024-06-10T13:30:00Z"), day("2024-12-31")),
        Ok(ratio(1, 1))
    );

    let ge = ge();
    let before_spin_offs = bar("2019-01-02T15:00:00Z", "8", "80");
    assert_eq!(
        ge.adjust_bar(&before_spin_offs, day("2026-01-01"), 18),
        Ok(bar("2019-01-02T15:00:00Z", "64", "10")),
        "only the reverse split adjusts; the spin-offs are kept, not applied"
    );
    assert_eq!(ge.other.len(), 3);
    let none = CorporateActions::none(symbol("GE"));
    assert!(none.is_empty());
    let only_splits = CorporateActions {
        splits: ge.splits.clone(),
        ..none.clone()
    };
    let only_dividends = CorporateActions {
        cash_dividends: ge.cash_dividends.clone(),
        ..none.clone()
    };
    let only_other = CorporateActions {
        other: ge.other.clone(),
        ..none
    };
    for actions in [only_splits, only_dividends, only_other] {
        assert!(!actions.is_empty(), "{actions:?}");
    }
    let order_free = with_splits(vec![split("2024-06-10", 10, 1), split("2021-07-20", 4, 1)]);
    assert_eq!(
        order_free.split_ratio_after(at("2021-07-16T19:00:00Z"), day("2024-12-31")),
        Ok(ratio(40, 1))
    );
}

#[test]
fn a_price_adjuster_adjusts_a_price_as_adjust_bar_does_and_keeps_unadjusted_prices_as_stored() {
    let nvda = nvda();
    let adjuster = nvda.price_adjuster(day("2024-06-10")).unwrap();
    assert_eq!(
        adjuster.adjust(&dec("745"), at("2021-07-16T19:00:00Z")),
        Ok(dec("18.625"))
    );
    assert_eq!(
        adjuster.adjust(&dec("180"), at("2024-06-09T23:59:59Z")),
        Ok(dec("18"))
    );
    assert_eq!(
        adjuster.adjust(&dec("120.5"), at("2024-06-10T00:00:00Z")),
        Ok(dec("120.5"))
    );
    let precise = dec("0.123456789012345678");
    assert_eq!(
        adjuster.adjust(&precise, at("2024-06-10T13:30:00Z")),
        Ok(precise.clone()),
        "no split applies, so the price keeps its 18 places"
    );
    assert!(matches!(
        adjuster.adjust(&precise, at("2024-06-07T13:30:00Z")),
        Err(AdjustmentError::Price {
            source: NumError::TooPrecise,
            ..
        })
    ));
    let before_both = nvda.price_adjuster(day("2021-07-19")).unwrap();
    assert_eq!(
        before_both.adjust(&dec("745"), at("2021-07-16T19:00:00Z")),
        Ok(dec("745")),
        "neither split is known yet"
    );
}

#[test]
fn the_recorded_corporate_action_fixtures_are_listed_in_the_recording_script() {
    let script = std::fs::read_to_string(fixtures_dir().join("record.sh")).unwrap();
    for name in [
        "corporate-actions-aapl-2020-2021-paged",
        "corporate-actions-ge-2018-2026",
        "corporate-actions-nvda-2021-2024",
    ] {
        assert!(script.contains(&format!("record {name} ")), "{name}");
        let first = &scenario(name).requests[0];
        assert!(script.contains(first.as_str()), "{name}: {first}");
    }
}

fn fraction_digits(value: &DecStr) -> u32 {
    value
        .as_str()
        .split_once('.')
        .map_or(0, |(_, f)| u32::try_from(f.len()).unwrap())
}

fn units(value: &DecStr) -> i128 {
    value.as_str().replace('.', "").parse().unwrap()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(2_000))]

    /// The adjusted price r (12 places) is the nearest multiple of 10⁻¹² to p × old ÷ new, and the
    /// even one on a tie: |p·old·10¹² − r·new·10^s| ≤ new·10^s ÷ 2, where s is p's scale.
    #[test]
    fn adjusted_prices_are_the_nearest_12_place_value_with_ties_to_even(
        price_units in 1_i64..=10_000_000_000_000,
        price_scale in 0_u32..=12,
        new in 1_u64..=1_000,
        old in 1_u64..=1_000,
    ) {
        let price = from_units(
            i128::from(price_units),
            u8::try_from(price_scale).unwrap(),
        ).unwrap();
        let scaled = i128::from(price_units) * i128::from(old) * 10_i128.pow(12 - price_scale);
        if 2 * scaled <= i128::from(new) {
            prop_assert_eq!(
                adjust_price(ratio(new, old), &price),
                Err(AdjustmentError::Price { price: price.clone(), source: NumError::NotPositive })
            );
            return Ok(());
        }
        let r = adjust_price(ratio(new, old), &price).unwrap();
        prop_assert!(fraction_digits(&r) <= u32::from(ADJUSTED_PRICE_SCALE));
        let r_units = units(&r) * 10_i128.pow(u32::from(ADJUSTED_PRICE_SCALE) - fraction_digits(&r));
        let p_units = units(&price);
        let s = fraction_digits(&price);
        let exact = p_units * i128::from(old) * 10_i128.pow(12);
        let approx = r_units * i128::from(new) * 10_i128.pow(s);
        let half_step = i128::from(new) * 10_i128.pow(s);
        let twice_error = 2 * (exact - approx).abs();
        prop_assert!(twice_error <= half_step, "{price} → {r}");
        if twice_error == half_step {
            prop_assert!(r_units % 2 == 0, "tie {price} → {r} is not even");
        }
    }

    /// Adjusting by a ratio and then by its inverse returns a quantity that the first step left
    /// exact, and never changes a bar's start or trade count.
    #[test]
    fn forward_then_reverse_restores_an_exactly_adjusted_quantity(
        quantity_units in 0_i64..=1_000_000_000_000,
        new in 1_u64..=1_000,
    ) {
        let quantity = from_units(i128::from(quantity_units), 9).unwrap();
        let forward = adjust_quantity(ratio(new, 1), &quantity, 18).unwrap();
        prop_assert_eq!(adjust_quantity(ratio(1, new), &forward, 18).unwrap(), quantity.clone());
        let actions = with_splits(vec![split("2024-06-10", new, 1)]);
        let b = Bar { volume: quantity, ..bar("2024-06-07T15:00:00Z", "1", "0") };
        let adjusted = actions.adjust_bar(&b, day("2024-06-10"), 18).unwrap();
        prop_assert_eq!(adjusted.start, b.start);
        prop_assert_eq!(adjusted.trade_count, b.trade_count);
        prop_assert_eq!(adjusted.volume, forward);
    }

    /// A price adjuster built once for `as_of` adjusts every price exactly as [`adjust_bar`]
    /// adjusts a bar's close at the same instant.
    #[test]
    fn a_price_adjuster_agrees_with_adjust_bar(
        secs in at("2021-01-01T00:00:00Z").secs()..at("2025-01-01T00:00:00Z").secs(),
        as_of_days in 0_i64..1_461,
        price_units in 1_i64..=10_000_000_000,
    ) {
        let nvda = nvda();
        let as_of = UtcNanos::from_parts(at("2021-01-01T12:00:00Z").secs() + as_of_days * 86_400, 0)
            .unwrap()
            .date();
        let price = from_units(i128::from(price_units), 4).unwrap();
        let observed = UtcNanos::from_parts(secs, 0).unwrap();
        let b = Bar { start: observed, ..bar("2021-01-01T00:00:00Z", price.as_str(), "1") };
        let expected = nvda.adjust_bar(&b, as_of, 18).unwrap().close;
        prop_assert_eq!(nvda.price_adjuster(as_of).unwrap().adjust(&price, observed).unwrap(), expected);
    }
}
