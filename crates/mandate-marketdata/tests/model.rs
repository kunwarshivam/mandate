//! Datasets name exactly what they hold: asset class, feed, bars or trades, and symbol, with every
//! combination the vendor cannot serve refused (backlog E2-1, DEC-89).

use std::path::PathBuf;

use mandate_marketdata::model::{
    AssetClass, DatasetId, DayRange, Feed, Kind, ModelError, Symbol, TimeUnit, Timeframe,
};
use mandate_time::Date;

fn day(s: &str) -> Date {
    Date::parse(s).unwrap()
}

#[test]
fn timeframes_use_alpaca_notation_and_its_bounds() {
    for (text, amount, unit) in [
        ("1Min", 1, TimeUnit::Minute),
        ("59Min", 59, TimeUnit::Minute),
        ("1Hour", 1, TimeUnit::Hour),
        ("23Hour", 23, TimeUnit::Hour),
        ("1Day", 1, TimeUnit::Day),
    ] {
        let tf: Timeframe = text.parse().unwrap();
        assert_eq!((tf.amount(), tf.unit()), (amount, unit), "{text}");
        assert_eq!(tf.to_string(), text);
        assert_eq!(Timeframe::new(amount, unit), Ok(tf));
    }
    for text in [
        "0Min", "60Min", "24Hour", "2Day", "01Min", "1min", "Min", "1", "", "1Minute", "256Min",
        "+1Min",
    ] {
        assert_eq!(
            text.parse::<Timeframe>(),
            Err(ModelError::Timeframe(text.to_owned())),
            "{text:?}"
        );
    }
}

#[test]
fn symbols_are_upper_case_tickers_or_pairs() {
    for ok in ["SPY", "BRK.B", "A", "BTC/USD", "ETH/BTC", "ABCDEFGHIJKLMNO"] {
        assert_eq!(Symbol::parse(ok).map(|s| s.to_string()), Ok(ok.to_owned()));
    }
    for bad in [
        "",
        "spy",
        "SP Y",
        ".SPY",
        "SPY.",
        "BTC/",
        "/USD",
        "BTC/USD/EUR",
        "BT.C/USD",
        "ABCDEFGHIJKLMNOP",
        "SPY\n",
        "../SPY",
    ] {
        assert_eq!(
            Symbol::parse(bad),
            Err(ModelError::Symbol(bad.to_owned())),
            "{bad:?}"
        );
    }
    let pair = Symbol::parse("BTC/USD").unwrap();
    assert!(pair.is_pair());
    assert_eq!(pair.slug(), "BTC-USD");
    assert!(!Symbol::parse("BRK.B").unwrap().is_pair());
}

#[test]
fn asset_classes_and_feeds_parse_and_pair_up() {
    for (text, class) in [
        ("us-equity", AssetClass::UsEquity),
        ("crypto", AssetClass::Crypto),
    ] {
        assert_eq!(text.parse(), Ok(class));
        assert_eq!(class.to_string(), text);
    }
    assert!("equity".parse::<AssetClass>().is_err());
    for (text, feed, class) in [
        ("sip", Feed::Sip, AssetClass::UsEquity),
        ("iex", Feed::Iex, AssetClass::UsEquity),
        ("crypto-us", Feed::CryptoUs, AssetClass::Crypto),
    ] {
        assert_eq!(text.parse(), Ok(feed));
        assert_eq!(feed.to_string(), text);
        assert!(feed.serves(class));
        let other = match class {
            AssetClass::UsEquity => AssetClass::Crypto,
            AssetClass::Crypto => AssetClass::UsEquity,
        };
        assert!(!feed.serves(other));
    }
    assert!("otc".parse::<Feed>().is_err());
}

#[test]
fn a_dataset_refuses_combinations_the_vendor_cannot_serve() {
    let symbol = |s: &str| Symbol::parse(s).unwrap();
    let bars: Kind = Kind::Bars("1Min".parse().unwrap());
    assert_eq!(
        DatasetId::new(AssetClass::Crypto, Feed::Sip, bars, symbol("BTC/USD")),
        Err(ModelError::FeedMismatch {
            asset_class: AssetClass::Crypto,
            feed: Feed::Sip
        })
    );
    assert_eq!(
        DatasetId::new(AssetClass::UsEquity, Feed::Iex, bars, symbol("BTC/USD")),
        Err(ModelError::SymbolMismatch {
            asset_class: AssetClass::UsEquity,
            symbol: symbol("BTC/USD")
        })
    );
    assert!(DatasetId::new(AssetClass::Crypto, Feed::CryptoUs, bars, symbol("BTC")).is_err());

    let id = DatasetId::new(AssetClass::Crypto, Feed::CryptoUs, bars, symbol("BTC/USD")).unwrap();
    assert_eq!(
        id.relative_dir(),
        ["alpaca", "crypto-us", "bars-1Min", "BTC-USD"]
            .iter()
            .collect::<PathBuf>()
    );
    let id = DatasetId::new(
        AssetClass::UsEquity,
        Feed::Sip,
        Kind::Trades,
        symbol("BRK.B"),
    )
    .unwrap();
    assert_eq!(
        id.relative_dir(),
        ["alpaca", "sip", "trades", "BRK.B"]
            .iter()
            .collect::<PathBuf>()
    );
}

#[test]
fn day_ranges_are_inclusive_and_follow_the_calendar() {
    let days = DayRange::new(day("2026-12-30"), day("2027-01-02"))
        .unwrap()
        .days()
        .unwrap();
    assert_eq!(
        days,
        ["2026-12-30", "2026-12-31", "2027-01-01", "2027-01-02"].map(day)
    );
    let one = DayRange::new(day("2028-02-29"), day("2028-02-29")).unwrap();
    assert_eq!(one.days().unwrap(), [day("2028-02-29")]);
    assert_eq!(
        DayRange::new(day("2026-09-25"), day("2026-09-24")),
        Err(ModelError::EmptyRange {
            first: day("2026-09-25"),
            last: day("2026-09-24")
        })
    );
}
