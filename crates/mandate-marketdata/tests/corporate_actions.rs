//! Corporate actions (backlog E2-4, trading domain spec §4.5, §8.5): the Alpaca wire format against
//! recorded responses, the paged fetch, and point-in-time split adjustment checked by hand and
//! against an oracle that tests the rounding definition instead of recomputing it.

mod common;

use common::{FakeTransport, RecordingPause, day, fixtures_dir, ok, scenario};
use mandate_canon::DecStr;
use mandate_marketdata::alpaca::{WireError, corporate_actions_path, parse_corporate_actions};
use mandate_marketdata::client::{Client, FetchError};
use mandate_marketdata::model::{
    ADJUSTED_PRICE_SCALE, AdjustmentError, Bar, CorporateActions, DayRange, Split, SplitRatio,
    Symbol,
};
use mandate_marketdata::number::{NumberError, from_units};
use mandate_time::{Date, TimeError, UtcNanos};
use proptest::prelude::*;

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

fn recorded(name: &str, sym: &str) -> CorporateActions {
    let s = scenario(name);
    assert_eq!(s.bodies.len(), 1, "{name} is one page");
    parse_corporate_actions(&symbol(sym), &s.bodies[0])
        .unwrap()
        .actions
}

fn ratio(new: u64, old: u64) -> SplitRatio {
    SplitRatio::new(new, old).unwrap()
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
fn the_request_names_the_symbol_the_process_dates_and_the_page() {
    assert_eq!(
        corporate_actions_path(&symbol("AAPL"), range("2020-01-01", "2021-12-31"), 5, None),
        "/v1/corporate-actions?symbols=AAPL&start=2020-01-01&end=2021-12-31&limit=5&sort=asc"
    );
    assert_eq!(
        corporate_actions_path(
            &symbol("BRK.B"),
            range("2024-01-01", "2024-01-01"),
            1000,
            Some("QUF=")
        ),
        "/v1/corporate-actions?symbols=BRK.B&start=2024-01-01&end=2024-01-01&limit=1000&sort=asc&page_token=QUF%3D"
    );
}

#[test]
fn a_reverse_split_dividends_and_spin_offs_parse_from_the_recorded_ge_response() {
    let ge = recorded("corporate-actions-ge-2018-2026", "GE");
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
    let other: Vec<(&str, &str, Date)> = ge
        .other
        .iter()
        .map(|o| (o.kind.as_str(), o.id.as_str(), o.process_date))
        .collect();
    assert_eq!(
        other,
        vec![
            (
                "spin_offs",
                "15a5c363-f85e-467f-81be-1f818982e45e",
                day("2019-02-25")
            ),
            (
                "spin_offs",
                "0b2e940e-e28d-4944-9e08-9e9761769d92",
                day("2023-01-04")
            ),
            (
                "spin_offs",
                "27cadc74-26f2-43fe-b5ef-8cc7e5affc30",
                day("2024-04-02")
            ),
        ]
    );
}

#[test]
fn forward_splits_and_dated_dividends_parse_from_the_recorded_nvda_response() {
    let nvda = recorded("corporate-actions-nvda-2021-2024", "NVDA");
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
    assert_eq!(aapl.cash_dividends.len(), 8);
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
            source: AdjustmentError::ZeroRatio { new: 0, old: 1 }
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
                source: AdjustmentError::ZeroRatio { new: 0, old: 1 },
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
        (AdjustmentError::ZeroRatio { new: 1, old: 0 }, "zero_ratio"),
        (AdjustmentError::RatioOverflow, "ratio_overflow"),
        (AdjustmentError::Overflow, "overflow"),
        (AdjustmentError::Number(NumberError::NotANumber), "number"),
        (AdjustmentError::Time(TimeError::OutOfRange), "time"),
    ];
    for (error, code) in adjustment {
        assert_eq!(error.code(), code, "{error}");
    }
}

#[test]
fn split_ratios_are_positive_and_kept_in_lowest_terms() {
    assert_eq!(
        SplitRatio::new(0, 3),
        Err(AdjustmentError::ZeroRatio { new: 0, old: 3 })
    );
    assert_eq!(
        SplitRatio::new(3, 0),
        Err(AdjustmentError::ZeroRatio { new: 3, old: 0 })
    );
    assert_eq!(ratio(4, 2), ratio(2, 1));
    assert_eq!(ratio(3, 3), SplitRatio::ONE);
    let r = ratio(6, 4);
    assert_eq!((r.new_shares(), r.old_shares()), (3, 2));
    assert_eq!(ratio(4, 1).then(ratio(1, 8)), Ok(ratio(1, 2)));
    assert_eq!(ratio(4, 1).then(ratio(10, 1)), Ok(ratio(40, 1)));
    let big = ratio(u64::MAX, 1);
    assert!(big.then(big).is_ok());
    assert_eq!(
        big.then(big).and_then(|r| r.then(big)),
        Err(AdjustmentError::RatioOverflow)
    );
    let small = ratio(1, u64::MAX);
    assert_eq!(
        small.then(small).and_then(|r| r.then(small)),
        Err(AdjustmentError::RatioOverflow)
    );
}

#[test]
fn prices_divide_by_the_ratio_to_12_places_half_even_and_quantities_multiply() {
    let cases = [
        (ratio(4, 1), "499.23", "124.8075"),
        (ratio(1, 8), "12.94", "103.52"),
        (ratio(3, 2), "100", "66.666666666667"),
        (ratio(3, 1), "1", "0.333333333333"),
        (ratio(6, 1), "1", "0.166666666667"),
        (ratio(2, 1), "0.000000000001", "0"),
        (ratio(2, 1), "0.000000000003", "0.000000000002"),
        (ratio(2, 1), "0.000000000005", "0.000000000002"),
        (ratio(2, 1), "0.000000000007", "0.000000000004"),
        (ratio(2, 1), "0.0000000000031", "0.000000000002"),
        (ratio(2, 1), "0.0000000000029", "0.000000000001"),
        (ratio(2, 1), "-0.000000000003", "-0.000000000002"),
        (ratio(2, 1), "-0.000000000005", "-0.000000000002"),
        (ratio(2, 1), "-0.0000000000029", "-0.000000000001"),
        (SplitRatio::ONE, "1.1234567890125", "1.123456789012"),
        (SplitRatio::ONE, "1.1234567890135", "1.123456789014"),
    ];
    for (r, price, adjusted) in cases {
        assert_eq!(r.adjust_price(&dec(price)), Ok(dec(adjusted)), "{price}");
    }
    let quantities = [
        (ratio(4, 1), "1000", 18, "4000"),
        (ratio(1, 8), "800", 18, "100"),
        (ratio(1, 8), "3", 18, "0.375"),
        (ratio(1, 3), "1", 18, "0.333333333333333333"),
        (ratio(1, 3), "2", 0, "1"),
        (ratio(1, 2), "5", 0, "2"),
        (ratio(1, 2), "7", 0, "4"),
    ];
    for (r, quantity, scale, adjusted) in quantities {
        assert_eq!(
            r.adjust_quantity(&dec(quantity), scale),
            Ok(dec(adjusted)),
            "{quantity} at {scale}"
        );
    }
    assert_eq!(
        ratio(1, u64::MAX).adjust_price(&dec("99999999999999999999")),
        Err(AdjustmentError::Overflow)
    );
    assert_eq!(
        SplitRatio::ONE.adjust_quantity(&dec("1"), 29),
        Err(AdjustmentError::Number(NumberError::UnsupportedScale {
            scale: 29
        }))
    );
    let huge = ratio(1, u64::MAX).then(ratio(1, u64::MAX)).unwrap();
    assert_eq!(huge.adjust_price(&dec("1")), Err(AdjustmentError::Overflow));
    assert_eq!(
        huge.adjust_quantity(&dec("1"), 0),
        Err(AdjustmentError::Overflow)
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
    let nvda = recorded("corporate-actions-nvda-2021-2024", "NVDA");
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
        Ok(SplitRatio::ONE)
    );

    let ge = recorded("corporate-actions-ge-2018-2026", "GE");
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
        price_units in 0_i64..=i64::MAX,
        price_scale in 0_u32..=18,
        new in 1_u64..=1_000,
        old in 1_u64..=1_000,
    ) {
        let price = from_units(
            i128::from(price_units),
            u8::try_from(price_scale).unwrap(),
        ).unwrap();
        let r = ratio(new, old).adjust_price(&price).unwrap();
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
        let forward = ratio(new, 1).adjust_quantity(&quantity, 18).unwrap();
        prop_assert_eq!(ratio(1, new).adjust_quantity(&forward, 18).unwrap(), quantity.clone());
        let actions = with_splits(vec![split("2024-06-10", new, 1)]);
        let b = Bar { volume: quantity, ..bar("2024-06-07T15:00:00Z", "1", "0") };
        let adjusted = actions.adjust_bar(&b, day("2024-06-10"), 18).unwrap();
        prop_assert_eq!(adjusted.start, b.start);
        prop_assert_eq!(adjusted.trade_count, b.trade_count);
        prop_assert_eq!(adjusted.volume, forward);
    }
}
