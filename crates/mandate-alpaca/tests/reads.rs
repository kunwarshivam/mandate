//! The instrument snapshot and the latest quote (backlog E7-8, DEC-168): hand-calculated cases
//! over the scenarios in `tests/fixtures/alpaca-trading/asset_*` and `tests/fixtures/alpaca-data/`,
//! with no network (ADR-0001 ES-19).
//!
//! Each case's expectation is written from trading-domain spec §3.1, §4.1 and §4.2 and from the
//! fixture's own bytes, never from the code. Every refusal case reads a good answer first and
//! asserts its exact values, so an implementation that refuses everything passes none of them,
//! and every reading case asserts exact values, so one that answers a constant passes none either
//! (`AGENTS.md` rule 3: a missing, unreadable, or stale answer is refused, and a good one is read).
//!
//! The cases whose subject is a read are `pending E7-8`. The request-building cases at the end are
//! live: the allowlist and the data host's request type are this PR's, not a stub's.

mod common;

use std::time::Duration;

use common::{FakeClock, FakeDataTransport, FakeTransport, body, data_scenario, inline, reply};
use mandate_accounting::{AssetClass, InstrumentId};
use mandate_alpaca::client::{RetryPolicy, TradingClient};
use mandate_alpaca::data::{DataClient, QuoteRequest};
use mandate_alpaca::error::{ReadError, TransportError, WireError};
use mandate_alpaca::http::{HttpRequest, Method, Response, asset_path, symbol_segment};
use mandate_alpaca::read::{self, Asset, AssetSnapshot, Exchange, Feed, LatestQuote};
use mandate_alpaca::{DATA_HOST, endpoint_for, is_paper_trading_path};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use proptest::prelude::*;

/// A read's answer, with the stub's own refusal turned into the failure it is: a pending case
/// must stop at the crate's `Unimplemented` and nowhere else, so an expected refusal is never
/// satisfied by a stub's.
fn read<T: core::fmt::Debug>(result: Result<T, ReadError>) -> Result<T, ReadError> {
    if let Err(error @ ReadError::Unimplemented { .. }) = &result {
        panic!("{error}");
    }
    result
}

/// A read's refusal code, or its value.
fn code<T>(result: Result<T, ReadError>) -> Result<T, &'static str> {
    match result {
        Err(error @ ReadError::Unimplemented { .. }) => panic!("{error}"),
        other => other.map_err(|error| error.code()),
    }
}

fn instrument(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol).unwrap_or_else(|e| panic!("{symbol}: {e:?}"))
}

fn instant(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

/// `at` moved by a signed number of nanoseconds, computed on whole nanoseconds since the epoch
/// rather than by any method of the crate under test.
fn shifted(at: UtcNanos, nanos: i128) -> UtcNanos {
    let total = i128::from(at.secs()) * 1_000_000_000 + i128::from(at.nanos()) + nanos;
    let secs = i64::try_from(total.div_euclid(1_000_000_000)).expect("in range");
    let rest = u32::try_from(total.rem_euclid(1_000_000_000)).expect("in range");
    UtcNanos::from_parts(secs, rest).expect("in range")
}

fn price(text: &str) -> Price {
    Price::parse(text).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap_or_else(|e| panic!("{text}: {e:?}"))
}

fn text_of(name: &str) -> String {
    String::from_utf8(body(name, 0)).expect("a fixture is UTF-8")
}

fn data_text_of(name: &str) -> String {
    let recorded = data_scenario(name);
    let first = recorded
        .exchanges
        .first()
        .unwrap_or_else(|| panic!("{name} has no exchange"));
    String::from_utf8(first.response.clone()).expect("a fixture is UTF-8")
}

/// `text` with `from` replaced by `to` exactly once, so a mutation that misses its target is a
/// failure of the case rather than a case that checks the unmutated body.
fn mutated(text: &str, from: &str, to: &str) -> Vec<u8> {
    assert_eq!(text.matches(from).count(), 1, "`{from}` occurs once in {text}");
    text.replacen(from, to, 1).into_bytes()
}

/// `AAPL` as the `asset_equity` fixture describes it (trading-domain spec §3.1).
fn aapl() -> Asset {
    Asset {
        asset_id: "b0b6dd9d-8b9b-48a9-ba46-b9d54906e415".to_owned(),
        instrument: instrument("AAPL"),
        class: AssetClass::UsEquity,
        exchange: Exchange::Nasdaq,
        active: true,
        tradable: true,
        fractionable: true,
        ipo: false,
        ptp_no_exception: false,
        min_order_size: None,
        min_trade_increment: None,
        price_increment: None,
    }
}

/// `BTC/USD` as the `asset_crypto` fixture describes it: a pair carries its order constraints.
fn btc() -> Asset {
    Asset {
        asset_id: "276e2673-764b-4ab6-a611-caf665ca6340".to_owned(),
        instrument: instrument("BTC/USD"),
        class: AssetClass::Crypto,
        exchange: Exchange::Crypto,
        active: true,
        tradable: true,
        fractionable: true,
        ipo: false,
        ptp_no_exception: false,
        min_order_size: Some(qty("0.000018")),
        min_trade_increment: Some(qty("0.000000001")),
        price_increment: Some(price("1")),
    }
}

/// The instant the equity quote fixture is stamped with.
const EQUITY_QUOTE_AT: &str = "2026-09-25T19:59:59.826431742Z";
/// The instant the crypto quote fixture is stamped with.
const CRYPTO_QUOTE_AT: &str = "2026-09-26T23:30:00.123456789Z";
/// The bound the quote cases pass, standing for the data profile's configured staleness threshold
/// (trading-domain spec §4.2). The read holds none of its own.
const MAX_AGE: Duration = Duration::from_secs(60);
/// The bound the asset cases pass, standing for spec §3.1's daily refresh.
const ASSET_MAX_AGE: Duration = Duration::from_secs(24 * 60 * 60);

/// The `AAPL` quote in `quote_equity`, on the `iex` feed.
fn aapl_quote() -> LatestQuote {
    LatestQuote {
        instrument: instrument("AAPL"),
        at: instant(EQUITY_QUOTE_AT),
        bid: price("227.44"),
        bid_size: qty("2"),
        ask: price("227.46"),
        ask_size: qty("1"),
        feed: Feed::Iex,
    }
}

/// The `BTC/USD` quote in `quote_crypto`, on the crypto feed.
fn btc_quote() -> LatestQuote {
    LatestQuote {
        instrument: instrument("BTC/USD"),
        at: instant(CRYPTO_QUOTE_AT),
        bid: price("65101.25"),
        bid_size: qty("0.51"),
        ask: price("65123.4"),
        ask_size: qty("0.2791"),
        feed: Feed::Crypto,
    }
}

fn trading(
    replies: impl IntoIterator<Item = Result<Response, TransportError>>,
    now: UtcNanos,
) -> (TradingClient<FakeTransport, FakeClock>, FakeTransport) {
    let transport = FakeTransport::serving(replies);
    let client = TradingClient::new(transport.clone(), FakeClock::at(now), RetryPolicy::default());
    (client, transport)
}

fn data(
    replies: impl IntoIterator<Item = Result<Response, TransportError>>,
    now: UtcNanos,
) -> (DataClient<FakeDataTransport, FakeClock>, FakeDataTransport) {
    let transport = FakeDataTransport::serving(replies);
    (DataClient::new(transport.clone(), FakeClock::at(now)), transport)
}

/// One latest-quote scenario's recorded reply.
fn data_reply(name: &str) -> Result<Response, TransportError> {
    let recorded = data_scenario(name);
    let first = recorded
        .exchanges
        .first()
        .unwrap_or_else(|| panic!("{name} has no exchange"));
    Ok(Response {
        status: first.status,
        body: first.response.clone(),
    })
}

/// The method, path and body of everything the trading client sent.
fn sent_lines(transport: &FakeTransport) -> Vec<(Method, String, Option<String>)> {
    transport
        .sent()
        .iter()
        .map(|r| {
            (
                r.method(),
                r.path_and_query().to_owned(),
                r.body().map(str::to_owned),
            )
        })
        .collect()
}

/// The URL of everything the data client sent.
fn sent_urls(transport: &FakeDataTransport) -> Vec<String> {
    transport.sent().iter().map(QuoteRequest::url).collect()
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn an_equity_asset_is_read_into_its_eligibility_fields_and_stamped_with_the_clock() {
    let now = instant("2026-09-28T13:00:00Z");
    let (client, transport) = trading([reply(200, "asset_equity", 0)], now);
    let snapshot = read(client.asset(&instrument("AAPL")).await);
    assert_eq!(
        snapshot,
        Ok(AssetSnapshot {
            asset: aapl(),
            loaded_at: now,
        }),
        "every §3.1 field the gate reads, exactly, and the instant it was read"
    );
    assert_eq!(
        sent_lines(&transport),
        vec![(Method::Get, "/v2/assets/AAPL".to_owned(), None)],
        "one read, on the asset endpoint, with no body"
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn a_crypto_asset_is_read_on_its_slash_free_path_with_its_order_constraints() {
    let now = instant("2026-09-28T13:00:00Z");
    let (client, transport) = trading([reply(200, "asset_crypto", 0)], now);
    let snapshot = read(client.asset(&instrument("BTC/USD")).await);
    assert_eq!(
        snapshot,
        Ok(AssetSnapshot {
            asset: btc(),
            loaded_at: now,
        })
    );
    assert_eq!(
        sent_lines(&transport),
        vec![(Method::Get, "/v2/assets/BTCUSD".to_owned(), None)],
        "the pair is sent without its slash, as the positions read sends it"
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn an_asset_the_broker_does_not_have_is_absent() {
    let now = instant("2026-09-28T13:00:00Z");
    let (client, transport) = trading(
        [reply(200, "asset_equity", 0), reply(404, "asset_absent", 0)],
        now,
    );
    assert_eq!(
        read(client.asset(&instrument("AAPL")).await).map(|s| s.asset),
        Ok(aapl()),
        "a listed instrument is read"
    );
    assert_eq!(
        code(client.asset(&instrument("XYZQ")).await),
        Err("absent"),
        "and one the broker does not list is absent, never a default record (rule 3)"
    );
    assert_eq!(
        sent_lines(&transport)
            .into_iter()
            .map(|(_, path, _)| path)
            .collect::<Vec<_>>(),
        vec!["/v2/assets/AAPL".to_owned(), "/v2/assets/XYZQ".to_owned()]
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn an_asset_record_about_another_instrument_is_refused() {
    let now = instant("2026-09-28T13:00:00Z");
    let (client, _) = trading(
        [reply(200, "asset_equity", 0), reply(200, "asset_equity", 0)],
        now,
    );
    assert_eq!(
        read(client.asset(&instrument("AAPL")).await).map(|s| s.asset),
        Ok(aapl())
    );
    assert_eq!(
        code(client.asset(&instrument("MSFT")).await),
        Err("other_instrument"),
        "AAPL's record is never MSFT's snapshot"
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn an_overloaded_or_failing_broker_leaves_the_asset_unread() {
    let now = instant("2026-09-28T13:00:00Z");
    let (client, _) = trading(
        [
            inline(429, "{\"message\":\"too many requests\"}"),
            inline(500, "{\"message\":\"internal error\"}"),
            inline(503, ""),
            Err(TransportError::Timeout),
            Err(TransportError::Connect),
            inline(403, "{\"code\":40310000,\"message\":\"forbidden\"}"),
            reply(200, "asset_equity", 0),
        ],
        now,
    );
    let aapl_id = instrument("AAPL");
    for expected in [
        "overloaded",
        "overloaded",
        "overloaded",
        "timeout",
        "connect",
        "unexpected_status",
    ] {
        assert_eq!(
            code(client.asset(&aapl_id).await).map(|s| s.asset),
            Err(expected),
            "no answer about the instrument is no snapshot"
        );
    }
    assert_eq!(
        read(client.asset(&aapl_id).await).map(|s| s.asset),
        Ok(aapl()),
        "and the next good answer is read, with nothing carried over"
    );
}

#[test]
#[ignore = "pending E7-8"]
fn an_uninterpretable_equity_asset_record_is_refused_field_by_field() {
    let base = text_of("asset_equity");
    let aapl_id = instrument("AAPL");
    assert_eq!(
        read(read::asset(&aapl_id, base.as_bytes())),
        Ok(aapl()),
        "the recorded body reads"
    );
    for (from, to, expected) in [
        ("\"tradable\":true", "\"tradable\":\"true\"", "wrong_type"),
        (",\"tradable\":true", "", "missing_field"),
        ("\"fractionable\":true,", "", "missing_field"),
        ("\"fractionable\":true", "\"fractionable\":1", "wrong_type"),
        ("\"status\":\"active\",", "", "missing_field"),
        ("\"status\":\"active\"", "\"status\":\"halted\"", "wrong_type"),
        ("\"class\":\"us_equity\"", "\"class\":\"us_option\"", "wrong_type"),
        ("\"class\":\"us_equity\",", "", "missing_field"),
        ("\"exchange\":\"NASDAQ\",", "", "missing_field"),
        ("\"exchange\":\"NASDAQ\"", "\"exchange\":7", "wrong_type"),
        (
            "\"attributes\":[\"fractional_eh_enabled\",\"has_options\"],",
            "",
            "missing_field",
        ),
        (
            "\"attributes\":[\"fractional_eh_enabled\",\"has_options\"]",
            "\"attributes\":\"ipo\"",
            "wrong_type",
        ),
        (
            "\"attributes\":[\"fractional_eh_enabled\",\"has_options\"]",
            "\"attributes\":[1]",
            "wrong_type",
        ),
        (
            "\"id\":\"b0b6dd9d-8b9b-48a9-ba46-b9d54906e415\"",
            "\"id\":\"../x\"",
            "wrong_type",
        ),
        (",\"symbol\":\"AAPL\"", "", "missing_field"),
        ("\"symbol\":\"AAPL\"", "\"symbol\":\"AAPL/..\"", "wrong_type"),
    ] {
        assert_eq!(
            code(read::asset(&aapl_id, &mutated(&base, from, to))),
            Err(expected),
            "`{from}` as `{to}` is not a record this crate can read, so there is no snapshot"
        );
    }
    assert_eq!(
        code(read::asset(&aapl_id, b"<html>maintenance</html>")),
        Err("not_json")
    );
    assert_eq!(code(read::asset(&aapl_id, b"[]")), Err("wrong_type"));
}

#[test]
#[ignore = "pending E7-8"]
fn an_uninterpretable_crypto_asset_record_is_refused_field_by_field() {
    let base = text_of("asset_crypto");
    let pair = instrument("BTC/USD");
    assert_eq!(read(read::asset(&pair, base.as_bytes())), Ok(btc()));
    for (from, to, expected) in [
        ("\"min_order_size\":\"0.000018\",", "", "missing_field"),
        (
            "\"min_trade_increment\":\"0.000000001\",",
            "",
            "missing_field",
        ),
        (",\"price_increment\":\"1\"", "", "missing_field"),
        (
            "\"min_order_size\":\"0.000018\"",
            "\"min_order_size\":0.000018",
            "float_number",
        ),
        (
            "\"price_increment\":\"1\"",
            "\"price_increment\":\"1e0\"",
            "exponent_form",
        ),
        (
            "\"min_trade_increment\":\"0.000000001\"",
            "\"min_trade_increment\":\"0.0000000001\"",
            "too_many_places",
        ),
        (
            "\"min_order_size\":\"0.000018\"",
            "\"min_order_size\":\"-0.000018\"",
            "arithmetic",
        ),
        (
            "\"price_increment\":\"1\"",
            "\"price_increment\":\"0\"",
            "arithmetic",
        ),
    ] {
        assert_eq!(
            code(read::asset(&pair, &mutated(&base, from, to))),
            Err(expected),
            "`{from}` as `{to}`: a pair's order constraints are read exactly or not at all"
        );
    }
}

#[test]
#[ignore = "pending E7-8"]
fn the_exchange_codes_the_spec_names_are_read_and_any_other_is_other() {
    let base = text_of("asset_equity");
    let aapl_id = instrument("AAPL");
    for (wire, exchange) in [
        ("NASDAQ", Exchange::Nasdaq),
        ("NYSE", Exchange::Nyse),
        ("ARCA", Exchange::Arca),
        ("AMEX", Exchange::Amex),
        ("BATS", Exchange::Bats),
        ("OTC", Exchange::Otc),
        ("CRYPTO", Exchange::Crypto),
        ("IEX", Exchange::Other),
        ("nasdaq", Exchange::Other),
        ("", Exchange::Other),
    ] {
        let body = mutated(
            &base,
            "\"exchange\":\"NASDAQ\"",
            &format!("\"exchange\":\"{wire}\""),
        );
        assert_eq!(
            read(read::asset(&aapl_id, &body)).map(|asset| asset.exchange),
            Ok(exchange),
            "`{wire}` (trading-domain spec §3.1: anything unnamed is ineligible, so it is Other)"
        );
    }
}

#[test]
#[ignore = "pending E7-8"]
fn the_status_and_the_ipo_and_ptp_attributes_are_read() {
    let base = text_of("asset_equity");
    let aapl_id = instrument("AAPL");
    let attributes = "\"attributes\":[\"fractional_eh_enabled\",\"has_options\"]";
    for (to, ipo, ptp_no_exception) in [
        ("\"attributes\":[]", false, false),
        ("\"attributes\":[\"ipo\"]", true, false),
        ("\"attributes\":[\"ptp_no_exception\"]", false, true),
        ("\"attributes\":[\"ptp_with_exception\"]", false, false),
        (
            "\"attributes\":[\"has_options\",\"ipo\",\"ptp_no_exception\"]",
            true,
            true,
        ),
    ] {
        let asset = read(read::asset(&aapl_id, &mutated(&base, attributes, to)));
        assert_eq!(
            asset.map(|a| (a.ipo, a.ptp_no_exception)),
            Ok((ipo, ptp_no_exception)),
            "{to} (spec §3.2 item 3)"
        );
    }
    let inactive = mutated(&base, "\"status\":\"active\"", "\"status\":\"inactive\"");
    assert_eq!(
        read(read::asset(&aapl_id, &inactive)).map(|a| (a.active, a.tradable)),
        Ok((false, true)),
        "an inactive asset reads as inactive; `tradable` is its own field"
    );
    let untradable = mutated(&base, "\"tradable\":true", "\"tradable\":false");
    assert_eq!(
        read(read::asset(&aapl_id, &untradable)).map(|a| (a.active, a.tradable)),
        Ok((true, false))
    );
    let whole = mutated(&base, "\"fractionable\":true", "\"fractionable\":false");
    assert_eq!(
        read(read::asset(&aapl_id, &whole)).map(|a| a.fractionable),
        Ok(false)
    );
}

#[test]
#[ignore = "pending E7-8"]
fn an_asset_snapshot_older_than_its_bound_is_refused() {
    let loaded_at = instant("2026-09-28T13:00:00.5Z");
    let snapshot = AssetSnapshot {
        asset: aapl(),
        loaded_at,
    };
    let bound = i128::try_from(ASSET_MAX_AGE.as_nanos()).expect("fits");
    let expected = aapl();
    assert_eq!(
        read(snapshot.current(loaded_at, ASSET_MAX_AGE)),
        Ok(&expected),
        "a snapshot is current the instant it is read"
    );
    assert_eq!(
        read(snapshot.current(shifted(loaded_at, bound), ASSET_MAX_AGE)),
        Ok(&expected),
        "and still at exactly its bound"
    );
    assert_eq!(
        code(snapshot.current(shifted(loaded_at, bound + 1), ASSET_MAX_AGE)),
        Err("stale"),
        "one nanosecond past it, it is refused (rule 3)"
    );
    assert_eq!(
        code(snapshot.current(shifted(loaded_at, -1), ASSET_MAX_AGE)),
        Err("ahead_of_clock"),
        "and a snapshot read after the clock's now has no age to judge"
    );
    assert_eq!(
        code(snapshot.current(loaded_at, Duration::ZERO)),
        Ok(&expected),
        "a zero bound admits only the instant of the read"
    );
    assert_eq!(
        code(snapshot.current(shifted(loaded_at, 1), Duration::ZERO)),
        Err("stale")
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn an_equity_latest_quote_is_read_exactly_on_the_iex_feed() {
    let now = shifted(instant(EQUITY_QUOTE_AT), 5_000_000_000);
    let (client, transport) = data([data_reply("quote_equity")], now);
    assert_eq!(
        read(client.latest_quote(&instrument("AAPL"), MAX_AGE).await),
        Ok(aapl_quote()),
        "both sides, exactly as the data host sent them, and the instant it stamped"
    );
    assert_eq!(
        sent_urls(&transport),
        vec!["https://data.alpaca.markets/v2/stocks/AAPL/quotes/latest?feed=iex".to_owned()],
        "one read, on the data host, on the paper profile's feed (spec §4.2)"
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn a_crypto_latest_quote_is_read_exactly_on_the_crypto_feed() {
    let now = shifted(instant(CRYPTO_QUOTE_AT), 1);
    let (client, transport) = data([data_reply("quote_crypto")], now);
    assert_eq!(
        read(client.latest_quote(&instrument("BTC/USD"), MAX_AGE).await),
        Ok(btc_quote())
    );
    assert_eq!(
        sent_urls(&transport),
        vec![
            "https://data.alpaca.markets/v1beta3/crypto/us/latest/quotes?symbols=BTC%2FUSD"
                .to_owned()
        ]
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn a_missing_quote_is_absent() {
    let now = shifted(instant(EQUITY_QUOTE_AT), 1);
    let (client, _) = data(
        [data_reply("quote_equity"), data_reply("quote_equity_absent")],
        now,
    );
    assert_eq!(
        read(client.latest_quote(&instrument("AAPL"), MAX_AGE).await),
        Ok(aapl_quote())
    );
    assert_eq!(
        code(client.latest_quote(&instrument("XYZQ"), MAX_AGE).await),
        Err("absent"),
        "a symbol the data host does not know has no quote, never a default one"
    );

    let now = shifted(instant(CRYPTO_QUOTE_AT), 1);
    let (client, _) = data(
        [data_reply("quote_crypto"), data_reply("quote_crypto_absent")],
        now,
    );
    assert_eq!(
        read(client.latest_quote(&instrument("BTC/USD"), MAX_AGE).await),
        Ok(btc_quote())
    );
    assert_eq!(
        code(client.latest_quote(&instrument("ETH/BTC"), MAX_AGE).await),
        Err("absent"),
        "a pair missing from `quotes` has no quote"
    );

    let aapl_id = instrument("AAPL");
    for body in [
        "{\"symbol\":\"AAPL\"}",
        "{\"quote\":null,\"symbol\":\"AAPL\"}",
    ] {
        assert_eq!(
            code(read::latest_quote(&aapl_id, body.as_bytes())),
            Err("absent"),
            "{body}"
        );
    }
    assert_eq!(
        code(read::latest_quote(
            &instrument("BTC/USD"),
            b"{\"quotes\":{\"BTC/USD\":null}}"
        )),
        Err("absent")
    );
}

#[test]
#[ignore = "pending E7-8"]
fn an_uninterpretable_equity_quote_is_refused_field_by_field() {
    let base = data_text_of("quote_equity");
    let aapl_id = instrument("AAPL");
    assert_eq!(
        read(read::latest_quote(&aapl_id, base.as_bytes())),
        Ok(aapl_quote())
    );
    for (from, to, expected) in [
        ("\"bp\":227.44", "\"bp\":\"227.44\"", "wrong_type"),
        ("\"bp\":227.44", "\"bp\":2.2744e2", "exponent_form"),
        ("\"ap\":227.46", "\"ap\":22746E-2", "exponent_form"),
        ("\"bp\":227.44", "\"bp\":227.4400000001", "too_many_places"),
        ("\"bp\":227.44,", "", "missing_field"),
        ("\"ap\":227.46,", "", "missing_field"),
        ("\"as\":1,", "", "missing_field"),
        ("\"bs\":2,", "", "missing_field"),
        ("\"bs\":2", "\"bs\":-2", "arithmetic"),
        ("\"ap\":227.46", "\"ap\":-227.46", "arithmetic"),
        ("\"bs\":2", "\"bs\":null", "wrong_type"),
        (
            "\"t\":\"2026-09-25T19:59:59.826431742Z\",",
            "",
            "missing_field",
        ),
        (
            "\"t\":\"2026-09-25T19:59:59.826431742Z\"",
            "\"t\":\"yesterday\"",
            "time",
        ),
        (
            "\"t\":\"2026-09-25T19:59:59.826431742Z\"",
            "\"t\":1790366399",
            "wrong_type",
        ),
        (",\"symbol\":\"AAPL\"", "", "missing_field"),
    ] {
        assert_eq!(
            code(read::latest_quote(&aapl_id, &mutated(&base, from, to))),
            Err(expected),
            "`{from}` as `{to}` is not a quote this crate can read, so there is none"
        );
    }
    assert_eq!(
        code(read::latest_quote(
            &aapl_id,
            b"{\"quote\":[],\"symbol\":\"AAPL\"}"
        )),
        Err("wrong_type")
    );
    assert_eq!(
        code(read::latest_quote(&aapl_id, b"upstream timeout")),
        Err("not_json")
    );
}

#[test]
#[ignore = "pending E7-8"]
fn an_uninterpretable_crypto_quote_is_refused_field_by_field() {
    let base = data_text_of("quote_crypto");
    let pair = instrument("BTC/USD");
    assert_eq!(
        read(read::latest_quote(&pair, base.as_bytes())),
        Ok(btc_quote())
    );
    for (from, to, expected) in [
        ("\"ap\":65123.4", "\"ap\":\"65123.4\"", "wrong_type"),
        ("\"as\":0.2791", "\"as\":2.791e-1", "exponent_form"),
        ("\"bs\":0.51", "\"bs\":0.5100000000001", "too_many_places"),
        ("\"bp\":65101.25,", "", "missing_field"),
        (
            ",\"t\":\"2026-09-26T23:30:00.123456789Z\"",
            "",
            "missing_field",
        ),
        (
            "\"t\":\"2026-09-26T23:30:00.123456789Z\"",
            "\"t\":\"2026-09-26 23:30:00\"",
            "time",
        ),
    ] {
        assert_eq!(
            code(read::latest_quote(&pair, &mutated(&base, from, to))),
            Err(expected),
            "`{from}` as `{to}`"
        );
    }
    assert_eq!(
        code(read::latest_quote(&pair, b"{\"quotes\":[]}")),
        Err("wrong_type")
    );
    assert_eq!(code(read::latest_quote(&pair, b"{}")), Err("missing_field"));
}

#[test]
#[ignore = "pending E7-8"]
fn a_quote_with_a_side_with_no_price_is_refused() {
    let equity = data_text_of("quote_equity");
    let aapl_id = instrument("AAPL");
    assert_eq!(
        read(read::latest_quote(&aapl_id, equity.as_bytes())),
        Ok(aapl_quote())
    );
    for (from, to) in [
        ("\"bp\":227.44", "\"bp\":0"),
        ("\"ap\":227.46", "\"ap\":0"),
        ("\"bp\":227.44", "\"bp\":0.0"),
    ] {
        assert_eq!(
            code(read::latest_quote(&aapl_id, &mutated(&equity, from, to))),
            Err("one_sided"),
            "`{to}`: a side with no price gives no mark and no collar (spec §8.2)"
        );
    }
    let crypto = data_text_of("quote_crypto");
    assert_eq!(
        code(read::latest_quote(
            &instrument("BTC/USD"),
            &mutated(&crypto, "\"ap\":65123.4", "\"ap\":0")
        )),
        Err("one_sided")
    );
    let crossed = mutated(&equity, "\"bp\":227.44", "\"bp\":227.50");
    assert_eq!(
        read(read::latest_quote(&aapl_id, &crossed)).map(|q| (q.bid, q.ask)),
        Ok((price("227.5"), price("227.46"))),
        "a crossed quote is kept as sent: whether it is sane is its reader's (spec §4.1, §8.2)"
    );
}

#[test]
#[ignore = "pending E7-8"]
fn a_quote_about_another_instrument_is_refused() {
    let equity = data_text_of("quote_equity");
    assert_eq!(
        read(read::latest_quote(&instrument("AAPL"), equity.as_bytes())),
        Ok(aapl_quote())
    );
    assert_eq!(
        code(read::latest_quote(&instrument("MSFT"), equity.as_bytes())),
        Err("other_instrument"),
        "AAPL's quote is never MSFT's"
    );
    let crypto = data_text_of("quote_crypto");
    assert_eq!(
        code(read::latest_quote(&instrument("ETH/USD"), crypto.as_bytes())),
        Err("absent"),
        "a pair's quote is found by its key, and BTC/USD's key is not ETH/USD's"
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn a_quote_older_than_the_bound_is_refused() {
    let stamped = instant(EQUITY_QUOTE_AT);
    let bound = i128::try_from(MAX_AGE.as_nanos()).expect("fits");
    for (offset, expected) in [
        (0, Ok(aapl_quote())),
        (bound, Ok(aapl_quote())),
        (bound + 1, Err("stale")),
        (bound * 60, Err("stale")),
    ] {
        let (client, _) = data([data_reply("quote_equity")], shifted(stamped, offset));
        assert_eq!(
            code(client.latest_quote(&instrument("AAPL"), MAX_AGE).await),
            expected,
            "a quote {offset} ns old against a {MAX_AGE:?} bound (spec §4.2, rule 3)"
        );
    }
    let (client, _) = data([data_reply("quote_equity")], shifted(stamped, 1));
    assert_eq!(
        code(client.latest_quote(&instrument("AAPL"), Duration::ZERO).await),
        Err("stale"),
        "the bound is the caller's, and a zero one refuses any quote not stamped this instant"
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn a_quote_stamped_after_the_clock_is_refused() {
    let stamped = instant(CRYPTO_QUOTE_AT);
    let (client, _) = data([data_reply("quote_crypto")], stamped);
    assert_eq!(
        read(client.latest_quote(&instrument("BTC/USD"), MAX_AGE).await),
        Ok(btc_quote()),
        "a quote stamped at the clock's now is current"
    );
    let (client, _) = data([data_reply("quote_crypto")], shifted(stamped, -1));
    assert_eq!(
        code(client.latest_quote(&instrument("BTC/USD"), MAX_AGE).await),
        Err("ahead_of_clock"),
        "one stamped a nanosecond later than now has no age anyone can judge"
    );
}

#[tokio::test]
#[ignore = "pending E7-8"]
async fn an_overloaded_or_failing_data_host_leaves_the_quote_unread() {
    let now = shifted(instant(EQUITY_QUOTE_AT), 1);
    let (client, transport) = data(
        [
            inline(429, "{\"message\":\"too many requests\"}"),
            inline(500, ""),
            Err(TransportError::Timeout),
            Err(TransportError::Request),
            inline(
                403,
                "{\"message\":\"subscription does not permit querying recent SIP data\"}",
            ),
            inline(422, "{\"message\":\"invalid symbol\"}"),
            data_reply("quote_equity"),
        ],
        now,
    );
    let aapl_id = instrument("AAPL");
    for expected in [
        "overloaded",
        "overloaded",
        "timeout",
        "request",
        "unexpected_status",
        "unexpected_status",
    ] {
        assert_eq!(
            code(client.latest_quote(&aapl_id, MAX_AGE).await),
            Err(expected)
        );
    }
    assert_eq!(
        read(client.latest_quote(&aapl_id, MAX_AGE).await),
        Ok(aapl_quote()),
        "and the next good answer is read"
    );
    assert_eq!(transport.sent().len(), 7, "one request per read, never a retry");
}

/// The canonical text `mandate-num` parses: trailing fractional zeros and a bare point dropped,
/// and nothing else, so `"100"` stays `"100"`.
fn canonical(text: &str) -> String {
    match text.split_once('.') {
        Some((whole, places)) => match places.trim_end_matches('0') {
            "" => whole.to_owned(),
            kept => format!("{whole}.{kept}"),
        },
        None => text.to_owned(),
    }
}

/// A JSON number token of up to 9 places with its own exact value's text, or zero.
fn number_token() -> impl Strategy<Value = String> {
    (0u64..200_000, prop::option::of("[0-9]{1,9}")).prop_map(|(whole, places)| match places {
        Some(places) => format!("{whole}.{places}"),
        None => whole.to_string(),
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Any latest-quote body with prices and sizes of up to 9 places reads back as exactly those
    /// numbers, or is refused for the one reason the oracle gives: a zero price is one-sided, and
    /// an age past the bound or before the clock is refused. The oracle reads each number with
    /// `mandate-num` from its own canonical text and judges the age on whole nanoseconds since
    /// the epoch, not with anything the crate computes.
    #[test]
    #[ignore = "pending E7-8"]
    fn a_quote_reads_as_its_own_numbers_or_is_refused_for_the_oracles_reason(
        bid in number_token(),
        ask in number_token(),
        bid_size in number_token(),
        ask_size in number_token(),
        age in -2_000_000_000i128..130_000_000_000,
    ) {
        let stamped = instant(EQUITY_QUOTE_AT);
        let body = format!(
            "{{\"quote\":{{\"ap\":{ask},\"as\":{ask_size},\"ax\":\"V\",\"bp\":{bid},\"bs\":{bid_size},\
             \"bx\":\"V\",\"c\":[\"R\"],\"t\":\"{EQUITY_QUOTE_AT}\",\"z\":\"C\"}},\"symbol\":\"AAPL\"}}"
        );
        let (client, _) = data(
            [Ok(Response { status: 200, body: body.into_bytes() })],
            shifted(stamped, age),
        );
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .map_err(|e| TestCaseError::fail(e.to_string()))?;
        let answer = runtime.block_on(client.latest_quote(&instrument("AAPL"), MAX_AGE));
        let bound = i128::try_from(MAX_AGE.as_nanos()).expect("fits");
        let expected = match (Price::parse(&canonical(&bid)), Price::parse(&canonical(&ask))) {
            (Err(_), _) | (_, Err(_)) => Err("one_sided"),
            (Ok(_), Ok(_)) if age < 0 => Err("ahead_of_clock"),
            (Ok(_), Ok(_)) if age > bound => Err("stale"),
            (Ok(bid), Ok(ask)) => Ok(LatestQuote {
                instrument: instrument("AAPL"),
                at: stamped,
                bid,
                bid_size: qty(&canonical(&bid_size)),
                ask,
                ask_size: qty(&canonical(&ask_size)),
                feed: Feed::Iex,
            }),
        };
        prop_assert_eq!(code(answer), expected);
    }
}

#[test]
fn the_asset_read_is_built_on_the_slash_free_symbol_path() {
    for (symbol, path) in [
        ("AAPL", "/v2/assets/AAPL"),
        ("BRK.B", "/v2/assets/BRK.B"),
        ("BTC/USD", "/v2/assets/BTCUSD"),
        ("ETH/USDT", "/v2/assets/ETHUSDT"),
    ] {
        let id = instrument(symbol);
        assert_eq!(asset_path(&id), Ok(path.to_owned()), "{symbol}");
        let request = HttpRequest::asset(&id).unwrap_or_else(|e| panic!("{symbol}: {e}"));
        assert_eq!(request.method(), Method::Get);
        assert_eq!(request.path_and_query(), path);
        assert_eq!(request.body(), None);
        assert_eq!(
            request.url(),
            format!("https://paper-api.alpaca.markets{path}"),
            "the asset read goes to the paper trading host and nowhere else"
        );
        assert!(is_paper_trading_path(path), "{path} is on the allowlist");
    }
    assert_eq!(
        symbol_segment(&instrument("BTC/USD")),
        Ok("BTCUSD".to_owned())
    );
    assert_eq!(symbol_segment(&instrument("BRK.B")), Ok("BRK.B".to_owned()));
    for method in [Method::Post, Method::Delete] {
        assert_eq!(
            endpoint_for(method, "/v2/assets/AAPL"),
            None,
            "the asset endpoint is a read and nothing else"
        );
    }
    assert!(
        !is_paper_trading_path("/v2/assets"),
        "the whole asset list is not a read this crate makes"
    );
    assert!(!is_paper_trading_path("/v2/assets/BTC/USD"));
}

#[test]
fn a_hostile_symbol_never_becomes_an_asset_or_a_quote_request() {
    for symbol in [
        "../AAPL", "AAPL/..", "A/B/C", "BTC/", "/USD", "/", "BTC/US.D", "BTC//USD", ".", "..",
        "%2e%2e", ".A", "A%2FB", "AAPL?x=1", "AAPL#x", "B TC", "BTC/USD?x=1", "AAPL&feed=sip",
    ] {
        let id = instrument(symbol);
        assert_eq!(
            HttpRequest::asset(&id).err(),
            Some(TransportError::RefusedPath),
            "{symbol:?} is not a symbol, so no asset read is built for it"
        );
        assert_eq!(asset_path(&id), Err(TransportError::RefusedPath));
        assert_eq!(symbol_segment(&id), Err(TransportError::RefusedPath));
        assert_eq!(
            QuoteRequest::latest(&id).err(),
            Some(TransportError::RefusedPath),
            "and no quote read either"
        );
    }
}

#[test]
fn a_latest_quote_request_is_built_for_the_data_host_only() {
    assert_eq!(DATA_HOST, "https://data.alpaca.markets");
    for (symbol, path) in [
        ("AAPL", "/v2/stocks/AAPL/quotes/latest?feed=iex"),
        ("BRK.B", "/v2/stocks/BRK.B/quotes/latest?feed=iex"),
        (
            "BTC/USD",
            "/v1beta3/crypto/us/latest/quotes?symbols=BTC%2FUSD",
        ),
        (
            "ETH/BTC",
            "/v1beta3/crypto/us/latest/quotes?symbols=ETH%2FBTC",
        ),
    ] {
        let id = instrument(symbol);
        let request = QuoteRequest::latest(&id).unwrap_or_else(|e| panic!("{symbol}: {e}"));
        assert_eq!(request.instrument(), &id);
        assert_eq!(request.path_and_query(), path);
        let url = format!("https://data.alpaca.markets{path}");
        assert_eq!(request.url(), url);
        assert_eq!(
            reqwest::Url::parse(&url).map(|u| u.to_string()),
            Ok(url.clone()),
            "a URL parser sends it exactly as it was built"
        );
        assert!(
            !is_paper_trading_path(path),
            "{path} is not a trading path, so no trading request can be built on it"
        );
    }
}

#[test]
fn every_read_error_code_is_stable_and_unique() {
    let samples = [
        (ReadError::Unimplemented { story: "E7-8" }, "unimplemented"),
        (ReadError::Absent, "absent"),
        (ReadError::OtherInstrument, "other_instrument"),
        (ReadError::OneSided, "one_sided"),
        (ReadError::Stale, "stale"),
        (ReadError::AheadOfClock, "ahead_of_clock"),
        (ReadError::Overloaded, "overloaded"),
        (ReadError::UnexpectedStatus { status: 403 }, "unexpected_status"),
        (ReadError::Wire(WireError::NotJson), "not_json"),
        (
            ReadError::Wire(WireError::MissingField { field: "t" }),
            "missing_field",
        ),
        (ReadError::Transport(TransportError::Timeout), "timeout"),
        (
            ReadError::Transport(TransportError::RefusedPath),
            "refused_path",
        ),
    ];
    for (error, expected) in &samples {
        assert_eq!(error.code(), *expected, "{error:?}");
    }
    let own = ReadError::CODES;
    assert_eq!(
        own.to_vec(),
        samples
            .iter()
            .take(8)
            .map(|(_, code)| *code)
            .collect::<Vec<_>>(),
        "the declared set is the variants' own codes, in order"
    );
    let mut seen = std::collections::BTreeSet::new();
    for code in own {
        assert!(seen.insert(code), "`{code}` appears twice");
    }
    for code in own.iter().skip(1) {
        assert!(
            !WireError::CODES.contains(code),
            "`{code}` would read as a wire failure"
        );
        assert!(
            ![
                TransportError::Timeout,
                TransportError::Connect,
                TransportError::Request,
                TransportError::RefusedPath,
            ]
            .iter()
            .any(|t| t.code() == *code),
            "`{code}` would read as a transport failure"
        );
    }
    assert!(
        ReadError::Stale.to_string().contains("older")
            && !ReadError::UnexpectedStatus { status: 403 }
                .to_string()
                .contains("http"),
        "the messages carry no URL"
    );
}
