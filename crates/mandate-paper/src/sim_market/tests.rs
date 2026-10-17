//! The rehearsal's market source (DEC-961) under `mandate-alpaca`'s own clients: what it answers
//! is what the fixture scripts, and every rule the clients hold a read to still refuses.

use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_alpaca::{
    DataClient, DataTransport, Exchange, Feed, HttpRequest, Method, Pause, QuoteRequest, ReadError,
    RetryPolicy, TradingClient, TradingTransport, TransportError,
};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;

use super::{FixtureRefusal, MARKET_FILE, SimMarket};

const SYMBOL: &str = "RHETF";
const ASSET_ID: &str = "5f0c3c1e-7d1a-4c8e-9b0a-2e6f1d3a9c41";
const FIXTURE: &str = r#"{"ask":"85.01","ask_size":"1","asset_id":"5f0c3c1e-7d1a-4c8e-9b0a-2e6f1d3a9c41","bid":"84.99","bid_size":"2","exchange":"ARCA","minute_volume":"20000","symbol":"RHETF"}"#;
/// Monday 2026-10-19, 10:00:30 in New York: inside the regular session.
const MONDAY: &str = "2026-10-19T14:00:30.000000000Z";
const MAX_AGE: Duration = Duration::from_secs(10);

/// A clock that always reads one instant and never waits.
#[derive(Debug, Clone, Copy)]
struct Fixed(UtcNanos);

impl Pause for Fixed {
    fn now(&self) -> UtcNanos {
        self.0
    }

    async fn pause(&self, _: Duration) {}
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

/// `at(MONDAY)` moved by `millis`.
fn monday_plus(millis: i64) -> UtcNanos {
    let base = at(MONDAY);
    let nanos = i128::from(base.secs()) * 1_000_000_000 + i128::from(millis) * 1_000_000;
    let secs = i64::try_from(nanos.div_euclid(1_000_000_000)).unwrap();
    let rest = u32::try_from(nanos.rem_euclid(1_000_000_000)).unwrap();
    UtcNanos::from_parts(secs, rest).unwrap()
}

fn instrument(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol).unwrap()
}

/// A fresh directory holding `text` as the fixture's file, or none at all.
fn dir(name: &str, text: Option<&str>) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sim-market-{name}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    fs::create_dir_all(&dir).unwrap();
    if let Some(text) = text {
        fs::write(dir.join(MARKET_FILE), text).unwrap();
    }
    dir
}

fn market(name: &str, clock: Fixed) -> SimMarket<Fixed> {
    SimMarket::load(&dir(name, Some(FIXTURE)), clock).expect("the fixture loads")
}

fn block<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap()
        .block_on(future)
}

#[test]
fn the_quote_is_the_fixtures_stamped_at_its_clock() {
    let clock = Fixed(at(MONDAY));
    let client = DataClient::new(market("quote", clock), clock);
    let quote = block(client.latest_quote(&instrument(SYMBOL), MAX_AGE)).unwrap();
    assert_eq!(quote.instrument, instrument(SYMBOL));
    assert_eq!(quote.at, at(MONDAY), "stamped with the fixture's clock");
    assert_eq!(quote.bid, Price::parse("84.99").unwrap());
    assert_eq!(quote.ask, Price::parse("85.01").unwrap());
    assert_eq!(quote.bid_size, Qty::parse("2").unwrap());
    assert_eq!(quote.ask_size, Qty::parse("1").unwrap());
    assert_eq!(quote.feed, Feed::Iex);
}

#[test]
fn a_fixture_quote_older_than_the_bound_is_stale() {
    let reader = Fixed(at(MONDAY));
    let fresh = DataClient::new(market("fresh", Fixed(monday_plus(-9_999))), reader);
    assert!(block(fresh.latest_quote(&instrument(SYMBOL), MAX_AGE)).is_ok());
    let stale = DataClient::new(market("stale", Fixed(monday_plus(-10_001))), reader);
    assert_eq!(
        block(stale.latest_quote(&instrument(SYMBOL), MAX_AGE)).unwrap_err(),
        ReadError::Stale,
        "the age bound applies to the fixture's quote unchanged"
    );
}

#[test]
fn a_fixture_quote_stamped_past_the_skew_is_refused() {
    let reader = Fixed(at(MONDAY));
    let within = DataClient::new(market("within", Fixed(monday_plus(250))), reader);
    assert!(block(within.latest_quote(&instrument(SYMBOL), MAX_AGE)).is_ok());
    let ahead = DataClient::new(market("ahead", Fixed(monday_plus(251))), reader);
    assert_eq!(
        block(ahead.latest_quote(&instrument(SYMBOL), MAX_AGE)).unwrap_err(),
        ReadError::AheadOfClock,
        "DEC-936's skew tolerance applies to the fixture's quote unchanged"
    );
}

#[test]
fn another_symbols_quote_is_refused_with_nothing_answered() {
    let clock = Fixed(at(MONDAY));
    let sim = market("other-quote", clock);
    let request = QuoteRequest::latest(&instrument("SPY")).unwrap();
    assert_eq!(
        block(DataTransport::send(&sim, &request)).unwrap_err(),
        TransportError::RefusedPath
    );
    let client = DataClient::new(sim, clock);
    assert!(block(client.latest_quote(&instrument("SPY"), MAX_AGE)).is_err());
}

#[test]
fn the_bars_are_every_complete_minute_of_the_window() {
    let clock = Fixed(at(MONDAY));
    let client = DataClient::new(market("bars", clock), clock);
    let window = Duration::from_secs(300);
    let bars = block(client.recent_minute_bars(&instrument(SYMBOL), window)).unwrap();
    assert_eq!(bars.instrument, instrument(SYMBOL));
    let starts: Vec<UtcNanos> = bars.bars.iter().map(|bar| bar.start).collect();
    let expected: Vec<UtcNanos> = ["55", "56", "57", "58", "59"]
        .iter()
        .map(|minute| at(&format!("2026-10-19T13:{minute}:00.000000000Z")))
        .collect();
    assert_eq!(starts, expected);
    assert!(
        bars.bars
            .iter()
            .all(|bar| bar.volume == Qty::parse("20000").unwrap())
    );
}

#[test]
fn a_one_minute_window_is_one_bar() {
    let clock = Fixed(at("2026-10-19T14:00:00.000000000Z"));
    let client = DataClient::new(market("one-bar", clock), clock);
    let window = Duration::from_secs(60);
    let bars = block(client.recent_minute_bars(&instrument(SYMBOL), window)).unwrap();
    let starts: Vec<UtcNanos> = bars.bars.iter().map(|bar| bar.start).collect();
    assert_eq!(starts, [at("2026-10-19T13:59:00.000000000Z")]);
}

#[test]
fn another_symbols_bars_are_refused_with_nothing_answered() {
    let clock = Fixed(at(MONDAY));
    let sim = market("other-bars", clock);
    let request = mandate_alpaca::BarsRequest::recent_minutes(
        &instrument("SPY"),
        at(MONDAY),
        Duration::from_secs(300),
    )
    .unwrap();
    assert_eq!(
        block(sim.send_bars(&request)).unwrap_err(),
        TransportError::RefusedPath
    );
}

#[test]
fn the_asset_record_is_the_fixtures() {
    let clock = Fixed(at(MONDAY));
    let client = TradingClient::new(market("asset", clock), clock, RetryPolicy::default());
    let snapshot = block(client.asset(&instrument(SYMBOL))).unwrap();
    let asset = snapshot.asset;
    assert_eq!(asset.asset_id, ASSET_ID);
    assert_eq!(asset.instrument, instrument(SYMBOL));
    assert_eq!(asset.class, AssetClass::UsEquity);
    assert_eq!(asset.exchange, Exchange::Arca);
    assert!(asset.active && asset.tradable);
    assert!(!asset.fractionable && !asset.ipo && !asset.ptp_no_exception);
    assert_eq!(snapshot.loaded_at, at(MONDAY));
}

#[test]
fn every_other_trading_request_is_refused_with_nothing_answered() {
    let sim = market("trading", Fixed(at(MONDAY)));
    for request in [
        HttpRequest::new(Method::Get, "/v2/account", None).unwrap(),
        HttpRequest::new(Method::Get, "/v2/positions", None).unwrap(),
        HttpRequest::asset(&instrument("SPY")).unwrap(),
    ] {
        assert_eq!(
            block(TradingTransport::send(&sim, &request)).unwrap_err(),
            TransportError::RefusedPath,
            "{}",
            request.path_and_query()
        );
    }
}

#[test]
fn a_fixture_that_cannot_be_read_gives_no_market() {
    let clock = Fixed(at(MONDAY));
    assert_eq!(
        SimMarket::load(&dir("missing", None), clock).unwrap_err(),
        FixtureRefusal::Unreadable
    );
    let members = [
        "ask",
        "ask_size",
        "asset_id",
        "bid",
        "bid_size",
        "exchange",
        "minute_volume",
        "symbol",
    ];
    for member in members {
        let without = FIXTURE.replace(&format!("\"{member}\":"), "\"dropped\":");
        assert_eq!(
            SimMarket::load(&dir(&format!("no-{member}"), Some(&without)), clock).unwrap_err(),
            FixtureRefusal::Unreadable,
            "{member} is required"
        );
    }
    for (name, broken) in [
        ("float", FIXTURE.replace("\"84.99\"", "84.99")),
        ("bid", FIXTURE.replace("\"84.99\"", "\"cheap\"")),
        ("ask", FIXTURE.replace("\"85.01\"", "\"-1\"")),
        ("bid-size", FIXTURE.replace("\"2\"", "\"two\"")),
        (
            "ask-size",
            FIXTURE.replace("\"ask_size\":\"1\"", "\"ask_size\":\"x\""),
        ),
        ("volume", FIXTURE.replace("\"20000\"", "\"lots\"")),
        ("symbol", FIXTURE.replace("\"RHETF\"", "\"\"")),
        ("asset-id", FIXTURE.replace(ASSET_ID, "")),
        ("exchange", FIXTURE.replace("\"ARCA\"", "\"\"")),
        ("text", "not json".to_owned()),
    ] {
        assert_eq!(
            SimMarket::load(&dir(&format!("broken-{name}"), Some(&broken)), clock).unwrap_err(),
            FixtureRefusal::Unreadable,
            "{name}"
        );
    }
}
