//! Top-of-book quotes (backlog E2-3): recorded Alpaca quote pages parse with every number exact,
//! pages are followed and retried like bars and trades, the feed of the dataset picks the
//! request, locked, crossed, and one-sided quotes are kept as sent, and Parquet partitions hold
//! every value exactly and are never rewritten.

mod common;

use std::fs::{self, File};
use std::time::Duration;

use arrow_schema::{DataType, TimeUnit};
use common::{
    FakeTransport, RecordingPause, Scratch, dataset, day, ok, scenario, shy_iex_trades, status,
};
use mandate_canon::DecStr;
use mandate_marketdata::alpaca::{self, WireError};
use mandate_marketdata::client::{Client, FetchError, TransportError};
use mandate_marketdata::dataset::{self, DatasetError, Status, Store};
use mandate_marketdata::http::is_market_data_path;
use mandate_marketdata::inspect::{self, InspectError};
use mandate_marketdata::model::{
    AssetClass, DatasetId, Feed, Kind, ModelError, Quote, Records, Symbol,
};
use mandate_time::UtcNanos;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

const SIP_PAGED: &str = "stock-quotes-sip-cphc-2026-09-24-paged";
const IEX_DAY: &str = "stock-quotes-iex-cphc-2026-09-24";
const SIP_SATURDAY: &str = "stock-quotes-sip-cphc-2026-09-19";
const CRYPTO_FIRST_PAGE: &str = "crypto-quotes-btcusd-2026-09-24-first-page";

fn dec(s: &str) -> DecStr {
    DecStr::parse(s).unwrap()
}

fn at(s: &str) -> UtcNanos {
    UtcNanos::parse(s).unwrap()
}

fn text(s: &str) -> Option<String> {
    Some(s.to_owned())
}

fn cphc(feed: Feed) -> DatasetId {
    dataset(AssetClass::UsEquity, feed, Kind::Quotes, "CPHC")
}

fn btc_quotes() -> DatasetId {
    dataset(AssetClass::Crypto, Feed::CryptoUs, Kind::Quotes, "BTC/USD")
}

fn quotes_of(records: Records) -> Vec<Quote> {
    match records {
        Records::Quotes(quotes) => quotes,
        other => panic!("quotes expected, got {other:?}"),
    }
}

fn parse(dataset: &DatasetId, on: &str, body: &[u8]) -> Result<Vec<Quote>, WireError> {
    alpaca::parse_page(dataset, day(on), body).map(|p| quotes_of(p.records))
}

/// Every quote of a recorded scenario, page after page.
fn recorded(name: &str, dataset: &DatasetId, on: &str) -> Vec<Quote> {
    scenario(name)
        .bodies
        .iter()
        .flat_map(|body| parse(dataset, on, body).unwrap())
        .collect()
}

fn stock_quote(t: &str, bid: &str, ask: &str) -> String {
    format!(
        r#"{{"ap":{ask},"as":200,"ax":"Q","bp":{bid},"bs":100,"bx":"P","c":["R"],"t":"{t}","z":"C"}}"#
    )
}

fn quotes_page(symbol: &str, quotes: &[String], token: Option<&str>) -> Vec<u8> {
    let token = token.map_or("null".to_owned(), |t| format!("\"{t}\""));
    format!(
        r#"{{"next_page_token":{token},"quotes":{{"{symbol}":[{}]}}}}"#,
        quotes.join(",")
    )
    .into_bytes()
}

fn cphc_page(quotes: &[String], token: Option<&str>) -> Vec<u8> {
    quotes_page("CPHC", quotes, token)
}

fn good_quote() -> String {
    stock_quote("2026-09-24T14:00:00.5Z", "12.5", "12.51")
}

fn parse_one(fields: &str) -> Result<Vec<Quote>, WireError> {
    let body = format!(r#"{{"next_page_token":null,"quotes":{{"CPHC":[{{{fields}}}]}}}}"#);
    parse(&cphc(Feed::Sip), "2026-09-24", body.as_bytes())
}

fn good_fields() -> String {
    let quote = good_quote();
    quote[1..quote.len() - 1].to_owned()
}

#[test]
fn recorded_sip_quotes_parse_with_exact_prices_sizes_exchanges_conditions_and_tape() {
    let s = scenario(SIP_PAGED);
    let page = alpaca::parse_page(&cphc(Feed::Sip), day("2026-09-24"), &s.bodies[0]).unwrap();
    assert_eq!(
        page.next_page_token.as_deref(),
        Some("Q1BIQ3wxNzkwMjU4ODUwMzk3MDI0ODU4fFF8MTUuNDV8MTAwfFB8MTUuOTJ8MTAwfFI=")
    );
    let quotes = quotes_of(page.records);
    assert_eq!(quotes.len(), 50);
    assert_eq!(
        quotes[0],
        Quote {
            time: at("2026-09-24T08:00:00.045789138Z"),
            bid_price: dec("12.8"),
            bid_size: dec("300"),
            ask_price: dec("0"),
            ask_size: dec("0"),
            bid_exchange: text("K"),
            ask_exchange: text(" "),
            conditions: Some(vec!["Y".to_owned()]),
            tape: text("C"),
        },
        "a one-sided quote keeps its zero ask and blank exchange"
    );
    assert_eq!(
        quotes[1],
        Quote {
            time: at("2026-09-24T08:01:51.173802256Z"),
            bid_price: dec("12.8"),
            bid_size: dec("300"),
            ask_price: dec("24.83"),
            ask_size: dec("100"),
            bid_exchange: text("K"),
            ask_exchange: text("Q"),
            conditions: Some(vec!["R".to_owned()]),
            tape: text("C"),
        }
    );
}

#[test]
fn recorded_iex_quotes_parse_with_exact_values() {
    let quotes = recorded(IEX_DAY, &cphc(Feed::Iex), "2026-09-24");
    assert_eq!(quotes.len(), 37);
    assert_eq!(
        quotes[0],
        Quote {
            time: at("2026-09-24T13:19:58.799254121Z"),
            bid_price: dec("11"),
            bid_size: dec("100"),
            ask_price: dec("28.49"),
            ask_size: dec("100"),
            bid_exchange: text("V"),
            ask_exchange: text("V"),
            conditions: Some(vec!["R".to_owned()]),
            tape: text("C"),
        }
    );
    assert_eq!(quotes[36].ask_price, dec("36.51"));
    assert_eq!(quotes[36].time, at("2026-09-24T20:00:01.300918208Z"));
}

#[test]
fn recorded_crypto_quotes_parse_without_exchanges_conditions_or_tape() {
    let s = scenario(CRYPTO_FIRST_PAGE);
    let page = alpaca::parse_page(&btc_quotes(), day("2026-09-24"), &s.bodies[0]).unwrap();
    assert!(page.next_page_token.is_some(), "a first page of a long day");
    let quotes = quotes_of(page.records);
    assert_eq!(quotes.len(), 20);
    assert_eq!(
        quotes[0],
        Quote {
            time: at("2026-09-24T00:00:11.834308116Z"),
            bid_price: dec("84387.73"),
            bid_size: dec("0.00017775"),
            ask_price: dec("84396.24"),
            ask_size: dec("0.001002"),
            bid_exchange: None,
            ask_exchange: None,
            conditions: None,
            tape: None,
        }
    );
    assert_eq!(quotes[19].bid_size, dec("0.0010012"));
}

#[test]
fn fractional_seconds_of_every_width_parse_exactly() {
    let recorded = recorded(SIP_PAGED, &cphc(Feed::Sip), "2026-09-24");
    let eight_digits: Vec<&Quote> = recorded
        .iter()
        .filter(|q| q.time == at("2026-09-24T12:14:27.163106570Z"))
        .collect();
    assert_eq!(eight_digits.len(), 1, "Alpaca trims the trailing zero");
    let widths = [
        ("2026-09-24T14:00:00Z", "2026-09-24T14:00:00.000000000Z"),
        ("2026-09-24T14:00:00.5Z", "2026-09-24T14:00:00.500000000Z"),
        ("2026-09-24T14:00:00.05Z", "2026-09-24T14:00:00.050000000Z"),
        (
            "2026-09-24T14:00:00.000001Z",
            "2026-09-24T14:00:00.000001000Z",
        ),
        (
            "2026-09-24T14:00:00.12345678Z",
            "2026-09-24T14:00:00.123456780Z",
        ),
        (
            "2026-09-24T14:00:00.123456789Z",
            "2026-09-24T14:00:00.123456789Z",
        ),
    ];
    for (sent, canonical) in widths {
        let quotes = parse(
            &cphc(Feed::Sip),
            "2026-09-24",
            &cphc_page(&[stock_quote(sent, "1", "2")], None),
        )
        .unwrap();
        assert_eq!(quotes[0].time, at(canonical), "{sent}");
    }
    let ten_digits = stock_quote("2026-09-24T14:00:00.1234567891Z", "1", "2");
    assert!(matches!(
        parse(
            &cphc(Feed::Sip),
            "2026-09-24",
            &cphc_page(&[ten_digits], None)
        ),
        Err(WireError::Timestamp { .. })
    ));
}

#[test]
fn locked_crossed_and_one_sided_quotes_are_kept_in_vendor_order() {
    let sent = [
        stock_quote("2026-09-24T14:00:00Z", "12.50", "12.51"),
        stock_quote("2026-09-24T14:00:01Z", "12.51", "12.51"),
        stock_quote("2026-09-24T14:00:02Z", "12.52", "12.51"),
        stock_quote("2026-09-24T14:00:03Z", "0", "12.51"),
        stock_quote("2026-09-24T14:00:03Z", "12.52", "12.51"),
    ];
    let quotes = parse(&cphc(Feed::Sip), "2026-09-24", &cphc_page(&sent, None)).unwrap();
    let sides: Vec<(DecStr, DecStr)> = quotes
        .iter()
        .map(|q| (q.bid_price.clone(), q.ask_price.clone()))
        .collect();
    assert_eq!(
        sides,
        vec![
            (dec("12.50"), dec("12.51")),
            (dec("12.51"), dec("12.51")),
            (dec("12.52"), dec("12.51")),
            (dec("0"), dec("12.51")),
            (dec("12.52"), dec("12.51")),
        ]
    );
}

#[test]
fn malformed_quote_numbers_are_rejected() {
    let good = good_fields();
    assert_eq!(parse_one(&good).map(|q| q.len()), Ok(1));
    let cases: [(&str, &str, &str); 5] = [
        ("\"bp\":12.5", "\"bp\":\"12.5\"", "bp"),
        ("\"ap\":12.51", "\"ap\":1e999", "ap"),
        ("\"bs\":100", "\"bs\":null", "bs"),
        ("\"as\":200", "\"as\":[200]", "as"),
        ("\"ap\":12.51", "\"ap\":true", "ap"),
    ];
    for (from, to, field) in cases {
        let fields = good.replace(from, to);
        assert_ne!(fields, good);
        match parse_one(&fields) {
            Err(WireError::Number { field: f, .. }) => assert_eq!(f, field, "{to}"),
            other => panic!("{to}: expected a number error, got {other:?}"),
        }
    }
    for (from, to, field) in [
        ("\"bp\":12.5", "\"bp\":-12.5", "bp"),
        ("\"as\":200", "\"as\":-200", "as"),
        ("\"ap\":12.51", "\"ap\":-12.51", "ap"),
        ("\"bs\":100", "\"bs\":-100", "bs"),
    ] {
        assert_eq!(
            parse_one(&good.replace(from, to)),
            Err(WireError::Negative {
                field,
                raw: to.split(':').nth(1).unwrap().to_owned()
            })
        );
    }
    let missing = good.replace("\"bs\":100,", "");
    assert!(matches!(parse_one(&missing), Err(WireError::Json(_))));
    let repeated = format!("{good},\"bp\":13");
    assert!(matches!(parse_one(&repeated), Err(WireError::Json(_))));
}

#[test]
fn quote_pages_are_checked_like_bar_and_trade_pages() {
    let sip = cphc(Feed::Sip);
    let saturday = &scenario(SIP_SATURDAY).bodies[0];
    let page = alpaca::parse_page(&sip, day("2026-09-19"), saturday).unwrap();
    assert_eq!(page.records, Records::Quotes(Vec::new()));
    assert_eq!(page.next_page_token, None);
    let null_map = br#"{"quotes":null,"next_page_token":null}"#;
    assert_eq!(parse(&sip, "2026-09-19", null_map), Ok(Vec::new()));
    assert_eq!(
        parse(
            &sip,
            "2026-09-24",
            br#"{"trades":{},"next_page_token":null}"#
        ),
        Err(WireError::MissingMember("quotes"))
    );
    assert_eq!(
        parse(
            &sip,
            "2026-09-24",
            &quotes_page("SHY", &[good_quote()], None)
        ),
        Err(WireError::UnexpectedSymbol("SHY".to_owned()))
    );
    let twice = br#"{"quotes":{"CPHC":[],"CPHC":[]},"next_page_token":null}"#;
    assert_eq!(
        parse(&sip, "2026-09-24", twice),
        Err(WireError::DuplicateSymbol("CPHC".to_owned()))
    );
}

#[test]
fn a_quote_at_the_next_midnight_belongs_to_the_next_day_and_earlier_ones_are_errors() {
    let sip = cphc(Feed::Sip);
    let edge = [
        stock_quote("2026-09-24T00:00:00Z", "1", "2"),
        stock_quote("2026-09-24T23:59:59.999999999Z", "1", "2"),
        stock_quote("2026-09-25T00:00:00Z", "1", "2"),
    ];
    let quotes = parse(&sip, "2026-09-24", &cphc_page(&edge, None)).unwrap();
    assert_eq!(
        quotes.iter().map(|q| q.time).collect::<Vec<_>>(),
        vec![
            at("2026-09-24T00:00:00.000000000Z"),
            at("2026-09-24T23:59:59.999999999Z")
        ]
    );
    let before = stock_quote("2026-09-23T23:59:59.999999999Z", "1", "2");
    assert_eq!(
        parse(&sip, "2026-09-24", &cphc_page(&[before], None)),
        Err(WireError::OutsideDay {
            time: at("2026-09-23T23:59:59.999999999Z"),
            day: day("2026-09-24"),
        })
    );
}

#[test]
fn the_feed_of_the_dataset_picks_the_quote_request() {
    for (name, id, limit) in [
        (SIP_PAGED, cphc(Feed::Sip), 50),
        (IEX_DAY, cphc(Feed::Iex), 10_000),
        (SIP_SATURDAY, cphc(Feed::Sip), 10_000),
        (CRYPTO_FIRST_PAGE, btc_quotes(), 20),
    ] {
        let s = scenario(name);
        let on = if name == SIP_SATURDAY {
            "2026-09-19"
        } else {
            "2026-09-24"
        };
        let path = alpaca::page_path(&id, day(on), limit, None).unwrap();
        assert_eq!(path, s.requests[0], "{name}");
        assert!(is_market_data_path(&path), "{path}");
    }
    let sip = scenario(SIP_PAGED);
    let first = alpaca::parse_page(&cphc(Feed::Sip), day("2026-09-24"), &sip.bodies[0]).unwrap();
    let next = first.next_page_token.as_deref();
    assert_eq!(
        alpaca::page_path(&cphc(Feed::Sip), day("2026-09-24"), 50, next).unwrap(),
        sip.requests[1]
    );
    assert_ne!(
        cphc(Feed::Sip).relative_dir(),
        cphc(Feed::Iex).relative_dir()
    );
    assert_eq!(
        cphc(Feed::Iex).relative_dir(),
        ["alpaca", "iex", "quotes", "CPHC"]
            .iter()
            .collect::<std::path::PathBuf>()
    );
    assert_eq!(Kind::Quotes.dir_name(), "quotes");
    let symbol = || Symbol::parse("CPHC").unwrap();
    assert_eq!(
        DatasetId::new(AssetClass::UsEquity, Feed::CryptoUs, Kind::Quotes, symbol()),
        Err(ModelError::FeedMismatch {
            asset_class: AssetClass::UsEquity,
            feed: Feed::CryptoUs
        })
    );
    assert!(!is_market_data_path(
        "/v2/stocks/quotes/latest?symbols=CPHC"
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn quote_pages_are_followed_to_the_end_in_time_order() {
    let s = scenario(SIP_PAGED);
    assert_eq!(s.requests.len(), 3);
    let transport = FakeTransport::serving(s.bodies.iter().map(|b| ok(b)));
    let pause = RecordingPause::default();
    let client = Client::new(transport.clone(), pause.clone()).with_page_limit(50);
    let records = client
        .fetch_day(&cphc(Feed::Sip), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(records.len(), 147);
    assert_eq!(transport.requested(), s.requests);
    assert_eq!(transport.unused(), 0);
    assert!(pause.pauses().is_empty());
    let times = records.times();
    assert!(times.windows(2).all(|w| w[0] <= w[1]));
    assert_eq!(
        quotes_of(records),
        recorded(SIP_PAGED, &cphc(Feed::Sip), "2026-09-24")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn rate_limited_quote_requests_retry_with_backoff_and_order_is_checked() {
    let body = cphc_page(&[good_quote()], None);
    let transport = FakeTransport::serving([
        status(429),
        Err(TransportError::Timeout),
        status(503),
        ok(&body),
    ]);
    let pause = RecordingPause::default();
    let client = Client::new(transport.clone(), pause.clone());
    let records = client
        .fetch_day(&cphc(Feed::Iex), day("2026-09-24"))
        .await
        .unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(pause.pauses(), [1, 2, 4].map(Duration::from_secs).to_vec());
    let requested = transport.requested();
    assert_eq!(requested.len(), 4);
    assert!(requested.iter().all(|r| r == &requested[0]));
    assert!(requested[0].contains("&feed=iex&"));

    let transport = FakeTransport::serving([
        ok(&cphc_page(
            &[stock_quote("2026-09-24T14:00:01Z", "1", "2")],
            Some("A"),
        )),
        ok(&cphc_page(
            &[stock_quote("2026-09-24T14:00:00Z", "1", "2")],
            None,
        )),
    ]);
    let client = Client::new(transport, RecordingPause::default());
    assert_eq!(
        client.fetch_day(&cphc(Feed::Sip), day("2026-09-24")).await,
        Err(FetchError::OutOfOrder {
            time: at("2026-09-24T14:00:00.000000000Z")
        })
    );
}

fn scripted_edge_quotes() -> Vec<Quote> {
    let sent = [
        stock_quote("2026-09-24T14:00:00.000000001Z", "12.51", "12.51"),
        stock_quote("2026-09-24T14:00:01Z", "12.52", "12.51"),
        stock_quote("2026-09-24T14:00:02Z", "0", "0.000000001"),
        stock_quote("2026-09-24T14:00:03Z", "999999.999999999", "12.51"),
    ];
    parse(&cphc(Feed::Sip), "2026-09-24", &cphc_page(&sent, None)).unwrap()
}

fn every_recording() -> Vec<(DatasetId, Vec<Quote>)> {
    vec![
        (
            cphc(Feed::Sip),
            recorded(SIP_PAGED, &cphc(Feed::Sip), "2026-09-24"),
        ),
        (
            cphc(Feed::Iex),
            recorded(IEX_DAY, &cphc(Feed::Iex), "2026-09-24"),
        ),
        (
            btc_quotes(),
            recorded(CRYPTO_FIRST_PAGE, &btc_quotes(), "2026-09-24"),
        ),
        (cphc(Feed::Sip), scripted_edge_quotes()),
    ]
}

#[test]
fn quote_partitions_round_trip_every_value_exactly() {
    let scratch = Scratch::new("quotes-round-trip");
    for (n, (id, quotes)) in every_recording().into_iter().enumerate() {
        assert!(!quotes.is_empty());
        let records = Records::Quotes(quotes);
        let bytes = dataset::encode(&id, &records).unwrap();
        assert_eq!(bytes, dataset::encode(&id, &records.clone()).unwrap());
        let path = scratch.path().join(format!("{n}.parquet"));
        fs::write(&path, &bytes).unwrap();
        assert_eq!(
            dataset::read(&path, Kind::Quotes).unwrap(),
            records,
            "{id:?}"
        );
    }
}

#[test]
fn quote_columns_are_decimal128_at_scale_9_and_the_manifest_records_them() {
    let scratch = Scratch::new("quotes-scales");
    let store = Store::new(scratch.path());
    let id = cphc(Feed::Sip);
    let quotes = recorded(SIP_PAGED, &id, "2026-09-24");
    store
        .put_day(&id, day("2026-09-24"), &Records::Quotes(quotes))
        .unwrap();
    let dir = store.dataset_dir(&id);
    let manifest = mandate_canon::parse(&fs::read(dir.join(dataset::MANIFEST)).unwrap()).unwrap();
    let described = manifest.get("dataset").unwrap();
    let member = |name: &str| described.get(name).and_then(|v| v.as_str());
    assert_eq!(member("kind"), Some("quotes"));
    assert_eq!(member("feed"), Some("sip"));
    assert_eq!(member("symbol"), Some("CPHC"));
    assert_eq!(member("timeframe"), None);
    let scales: Vec<(String, u64)> = manifest
        .get("scales")
        .and_then(|s| s.as_object())
        .unwrap()
        .iter()
        .map(|(k, v)| (k.as_str().to_owned(), v.as_int().unwrap()))
        .collect();
    let expected = ["ask_price", "ask_size", "bid_price", "bid_size"].map(|c| (c.to_owned(), 9));
    assert_eq!(scales, expected.to_vec());
    let file = File::open(dir.join("2026-09-24.parquet")).unwrap();
    let schema = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .schema()
        .clone();
    let columns: Vec<(&str, &DataType, bool)> = schema
        .fields()
        .iter()
        .map(|f| (f.name().as_str(), f.data_type(), f.is_nullable()))
        .collect();
    let decimal = DataType::Decimal128(38, 9);
    let time = DataType::Timestamp(TimeUnit::Nanosecond, Some("UTC".into()));
    assert_eq!(
        columns[..6],
        [
            ("symbol", &DataType::Utf8, false),
            ("time", &time, false),
            ("bid_price", &decimal, false),
            ("bid_size", &decimal, false),
            ("ask_price", &decimal, false),
            ("ask_size", &decimal, false),
        ]
    );
    let names: Vec<&str> = columns.iter().map(|c| c.0).collect();
    assert_eq!(
        names[6..],
        ["bid_exchange", "ask_exchange", "conditions", "tape"]
    );
    assert!(
        columns[6..].iter().all(|c| c.2),
        "vendor fields are optional"
    );
    let (listed, days) = dataset::read_manifest(&dir).unwrap();
    assert_eq!(listed, id);
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].rows, 147);
}

#[test]
fn storing_a_quote_day_again_changes_nothing_and_different_quotes_conflict() {
    let scratch = Scratch::new("quotes-idempotent");
    let store = Store::new(scratch.path());
    let id = cphc(Feed::Iex);
    let quotes = recorded(IEX_DAY, &id, "2026-09-24");
    let records = Records::Quotes(quotes.clone());
    let first = store.put_day(&id, day("2026-09-24"), &records).unwrap();
    assert_eq!(first.status, Status::Written);
    assert_eq!(first.rows, 37);
    let dir = store.dataset_dir(&id);
    let snapshot = || {
        let mut files: Vec<(String, Vec<u8>)> = fs::read_dir(&dir)
            .unwrap()
            .map(|e| {
                let path = e.unwrap().path();
                let name = path.file_name().unwrap().to_string_lossy().into_owned();
                (name, fs::read(&path).unwrap())
            })
            .collect();
        files.sort();
        files
    };
    let before = snapshot();
    let again = store.put_day(&id, day("2026-09-24"), &records).unwrap();
    assert_eq!(again.status, Status::Unchanged);
    assert_eq!(again.file, first.file);
    assert_eq!(snapshot(), before);

    let mut changed = quotes;
    changed[0].ask_size = dec("200");
    let err = store
        .put_day(&id, day("2026-09-24"), &Records::Quotes(changed))
        .unwrap_err();
    assert!(matches!(err, DatasetError::Conflict { .. }), "{err}");
    assert_eq!(snapshot(), before);

    let empty = store
        .put_day(&id, day("2026-09-19"), &Records::empty(Kind::Quotes))
        .unwrap();
    assert_eq!((empty.rows, empty.file), (0, None));
    assert!(!dir.join("2026-09-19.parquet").exists());
}

#[test]
fn a_quote_value_needing_a_tenth_fractional_digit_is_rejected_not_rounded() {
    let id = cphc(Feed::Sip);
    let columns = ["bid_price", "bid_size", "ask_price", "ask_size"];
    for (row, column) in columns.into_iter().enumerate() {
        let mut quotes = scripted_edge_quotes();
        let quote = &mut quotes[row];
        let field = match column {
            "bid_price" => &mut quote.bid_price,
            "bid_size" => &mut quote.bid_size,
            "ask_price" => &mut quote.ask_price,
            _ => &mut quote.ask_size,
        };
        *field = dec("12.5100000001");
        match dataset::encode(&id, &Records::Quotes(quotes)) {
            Err(DatasetError::Number {
                column: c, row: r, ..
            }) => assert_eq!((c, r), (column, row)),
            other => panic!("{column}: expected a number error, got {other:?}"),
        }
    }
}

#[test]
fn a_partition_of_another_kind_is_not_read_as_quotes() {
    let scratch = Scratch::new("quotes-schema");
    let s = scenario("stock-trades-iex-shy-2026-09-24-paged");
    let trades = alpaca::parse_page(&shy_iex_trades(), day("2026-09-24"), &s.bodies[0])
        .unwrap()
        .records;
    let path = scratch.path().join("trades.parquet");
    fs::write(&path, dataset::encode(&shy_iex_trades(), &trades).unwrap()).unwrap();
    assert!(matches!(
        dataset::read(&path, Kind::Quotes),
        Err(DatasetError::Parquet(_))
    ));
    let quotes = Records::Quotes(scripted_edge_quotes());
    let quotes_path = scratch.path().join("quotes.parquet");
    fs::write(
        &quotes_path,
        dataset::encode(&cphc(Feed::Sip), &quotes).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        dataset::read(&quotes_path, Kind::Trades),
        Err(DatasetError::Parquet(_))
    ));
}

#[test]
fn inspect_covers_stored_quote_days_but_does_not_summarize_quotes_yet() {
    let scratch = Scratch::new("quotes-inspect");
    let store = Store::new(scratch.path());
    let id = cphc(Feed::Sip);
    store
        .put_day(&id, day("2026-09-19"), &Records::empty(Kind::Quotes))
        .unwrap();
    let dir = store.dataset_dir(&id);
    let inspection = inspect::inspect(&dir).unwrap();
    assert_eq!(inspection.dataset, id);
    assert_eq!(inspection.coverage.listed, 1);
    assert_eq!(inspection.stats, None);

    let quotes = recorded(SIP_PAGED, &id, "2026-09-24");
    store
        .put_day(&id, day("2026-09-24"), &Records::Quotes(quotes))
        .unwrap();
    let err = inspect::inspect(&dir).unwrap_err();
    assert!(
        matches!(err, InspectError::Unsupported(Kind::Quotes)),
        "{err}"
    );
    assert_eq!(err.code(), "unsupported");
    assert_eq!(
        err.to_string(),
        "inspect does not summarize stored quotes yet"
    );
}
