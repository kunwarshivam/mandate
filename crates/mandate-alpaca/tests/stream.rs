//! E2-5 slice 1 (DEC-1174; data-plane spec v0.3 §3.1, §3.2): the stream protocol over recorded
//! frames, with no network. The frames are the vendor's documented examples, and each expected
//! value is written out from the frame's own text, never from the reader.

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_alpaca::WireError;
use mandate_alpaca::stream::{
    BarKind, CancelAction, Channels, Disposition, Fault, STREAM_HOST, StreamFeed, StreamMessage,
    auth_frame, read_frame, subscribe_frame,
};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use proptest::prelude::*;
use secrecy::{ExposeSecret, SecretString};

const QUOTE: &str = r#"{"T":"q","S":"AMD","bx":"U","bp":87.66,"bs":1,"ax":"Q","ap":87.68,"as":4,"t":"2021-02-22T15:51:45.335689322Z","c":["R"],"z":"C"}"#;
const TRADE: &str = r#"{"T":"t","i":96921,"S":"AAPL","x":"D","p":126.55,"s":1,"t":"2021-02-22T15:51:44.208Z","c":["@","I"],"z":"C"}"#;
const BAR: &str = r#"{"T":"b","S":"SPY","o":388.985,"h":389.13,"l":388.975,"c":389.12,"v":49378,"t":"2021-02-22T19:15:00Z","n":461,"vw":389.062639}"#;
const STATUS: &str = r#"{"T":"s","S":"AAPL","sc":"H","sm":"Trading Halt","rc":"T12","rm":"Trading Halted; For information requested by NASDAQ","t":"2021-02-22T19:15:00Z","z":"C"}"#;
const LULD: &str =
    r#"{"T":"l","S":"IONM","u":5.68,"d":4.65,"i":"B","t":"2021-02-22T19:15:00Z","z":"C"}"#;
const CORRECTION: &str = r#"{"T":"c","S":"EEM","x":"M","oi":52983525033527,"op":39.1582,"os":440000,"oc":[" ","7","V"],"ci":52983525033540,"cp":39.1809,"cs":440000,"cc":[" ","7","V"],"z":"B","t":"2023-04-06T14:24:08Z"}"#;
const CANCEL: &str = r#"{"T":"x","S":"TSLA","i":52983525033800,"x":"V","p":180.2,"s":100,"a":"C","z":"C","t":"2023-04-06T14:24:08Z"}"#;

fn id(symbol: &str) -> InstrumentId {
    InstrumentId::new(symbol).unwrap()
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn price(text: &str) -> Price {
    Price::parse(text).unwrap()
}

fn qty(text: &str) -> Qty {
    Qty::parse(text).unwrap()
}

fn one(message: &str) -> StreamMessage {
    let frame = read_frame(&format!("[{message}]")).unwrap();
    assert!(frame.refused.is_empty(), "{:?}", frame.refused);
    assert_eq!(frame.messages.len(), 1);
    frame.messages.into_iter().next().unwrap()
}

fn refusal(message: &str) -> WireError {
    let frame = read_frame(&format!("[{message}]")).unwrap();
    assert!(frame.messages.is_empty(), "{:?}", frame.messages);
    assert_eq!(frame.refused.len(), 1);
    frame.refused[0].reason.clone()
}

fn set(symbols: &[&str]) -> BTreeSet<InstrumentId> {
    symbols.iter().map(|s| id(s)).collect()
}

#[test]
fn each_feed_is_read_on_the_stream_host() {
    assert_eq!(STREAM_HOST, "wss://stream.data.alpaca.markets");
    assert_eq!(
        StreamFeed::Iex.url(),
        "wss://stream.data.alpaca.markets/v2/iex"
    );
    assert_eq!(
        StreamFeed::Sip.url(),
        "wss://stream.data.alpaca.markets/v2/sip"
    );
}

#[test]
fn the_auth_frame_carries_the_credential_and_stays_secret() {
    let key = SecretString::from("PKTEST".to_owned());
    let secret = SecretString::from("s3cr\"et".to_owned());
    let frame = auth_frame(&key, &secret);
    let sent: serde_json::Value = serde_json::from_str(frame.expose_secret()).unwrap();
    assert_eq!(
        sent,
        serde_json::json!({"action":"auth","key":"PKTEST","secret":"s3cr\"et"})
    );
    assert!(!format!("{frame:?}").contains("PKTEST"), "never printed");
}

#[test]
fn the_subscribe_frame_names_each_channel_in_symbol_order() {
    let channels = Channels {
        quotes: set(&["SPY", "AAPL"]),
        trades: set(&["AAPL"]),
        bars: set(&["SPY"]),
        updated_bars: set(&["SPY"]),
        daily_bars: BTreeSet::new(),
        statuses: set(&["SPY", "AAPL"]),
        lulds: set(&["SPY"]),
    };
    let frame = subscribe_frame(&channels).unwrap();
    assert_eq!(
        frame,
        r#"{"action":"subscribe","trades":["AAPL"],"quotes":["AAPL","SPY"],"bars":["SPY"],"updatedBars":["SPY"],"dailyBars":[],"statuses":["AAPL","SPY"],"lulds":["SPY"]}"#
    );
}

#[test]
fn a_subscription_naming_nothing_or_a_non_equity_is_refused() {
    assert!(matches!(
        subscribe_frame(&Channels::default()),
        Err(WireError::WrongType { .. })
    ));
    for symbol in ["BTC/USD", "*", "a b"] {
        let channels = Channels {
            quotes: InstrumentId::new(symbol).into_iter().collect(),
            ..Channels::default()
        };
        if channels.quotes.is_empty() {
            continue;
        }
        assert!(
            matches!(subscribe_frame(&channels), Err(WireError::WrongType { .. })),
            "{symbol}"
        );
    }
}

#[test]
fn the_control_messages_are_read() {
    let frame = read_frame(
        r#"[{"T":"success","msg":"connected"},{"T":"success","msg":"authenticated"},{"T":"subscription","trades":["AAPL"],"quotes":["AMD","CLDR"],"bars":[],"updatedBars":[],"dailyBars":["VOO"],"statuses":["AMD"],"lulds":[],"corrections":["AAPL"],"cancelErrors":["AAPL"]}]"#,
    )
    .unwrap();
    assert!(frame.refused.is_empty(), "{:?}", frame.refused);
    assert_eq!(
        frame.messages,
        vec![
            StreamMessage::Connected,
            StreamMessage::Authenticated,
            StreamMessage::Subscribed(Channels {
                quotes: set(&["AMD", "CLDR"]),
                trades: set(&["AAPL"]),
                daily_bars: set(&["VOO"]),
                statuses: set(&["AMD"]),
                ..Channels::default()
            }),
        ]
    );
}

#[test]
fn a_subscription_reads_each_channel_into_its_own_set() {
    let StreamMessage::Subscribed(channels) = one(
        r#"{"T":"subscription","trades":["A"],"quotes":["B"],"bars":["C"],"updatedBars":["D"],"dailyBars":["E"],"statuses":["F"],"lulds":["G"]}"#,
    ) else {
        panic!("not a subscription");
    };
    assert_eq!(
        channels,
        Channels {
            trades: set(&["A"]),
            quotes: set(&["B"]),
            bars: set(&["C"]),
            updated_bars: set(&["D"]),
            daily_bars: set(&["E"]),
            statuses: set(&["F"]),
            lulds: set(&["G"]),
        }
    );
}

#[test]
fn an_unknown_success_or_a_wildcard_subscription_is_refused() {
    assert!(matches!(
        refusal(r#"{"T":"success","msg":"maybe"}"#),
        WireError::WrongType { field: "msg" }
    ));
    refusal(r#"{"T":"subscription","quotes":["*"]}"#);
}

#[test]
fn each_error_code_says_what_the_connection_does() {
    let table = [
        (402, Disposition::Pause),
        (409, Disposition::Pause),
        (405, Disposition::Overflow),
        (400, Disposition::Down),
        (401, Disposition::Down),
        (403, Disposition::Down),
        (404, Disposition::Down),
        (406, Disposition::Down),
        (407, Disposition::Down),
        (410, Disposition::Narrow),
        (500, Disposition::Down),
        (599, Disposition::Down),
    ];
    for (code, disposition) in table {
        assert_eq!(Fault::of(code), Fault { code, disposition }, "{code}");
    }
    assert_eq!(
        one(r#"{"T":"error","code":409,"msg":"insufficient subscription"}"#),
        StreamMessage::Error(Fault {
            code: 409,
            disposition: Disposition::Pause
        })
    );
}

/// A feed that does not carry a channel answers 410. Going down and reconnecting would only ask
/// again and take the quotes down with it, so the subscription is narrowed to the channels every
/// feed carries: the statuses and LULD bands go, and the quotes, trades and bars stay.
#[test]
fn a_rejected_subscribe_action_narrows_to_the_channels_every_feed_carries() {
    let channels = Channels {
        quotes: set(&["SPY"]),
        trades: set(&["SPY"]),
        bars: set(&["SPY"]),
        updated_bars: set(&["SPY"]),
        daily_bars: set(&["SPY"]),
        statuses: set(&["SPY"]),
        lulds: set(&["SPY"]),
    };
    let narrowed = channels.narrowed();
    assert_eq!(
        narrowed,
        Channels {
            statuses: BTreeSet::new(),
            lulds: BTreeSet::new(),
            ..channels.clone()
        }
    );
    assert_eq!(
        subscribe_frame(&narrowed).unwrap(),
        r#"{"action":"subscribe","trades":["SPY"],"quotes":["SPY"],"bars":["SPY"],"updatedBars":["SPY"],"dailyBars":["SPY"],"statuses":[],"lulds":[]}"#
    );
    assert_eq!(
        narrowed.narrowed(),
        narrowed,
        "narrowing twice changes nothing"
    );
}

#[test]
fn a_quote_is_read_exactly() {
    let StreamMessage::Quote(quote) = one(QUOTE) else {
        panic!("not a quote");
    };
    assert_eq!(quote.instrument, id("AMD"));
    assert_eq!(quote.vendor_time, at("2021-02-22T15:51:45.335689322Z"));
    let bid = quote.bid.unwrap();
    assert_eq!(
        (bid.price, bid.size, bid.exchange.as_str()),
        (price("87.66"), qty("1"), "U")
    );
    let ask = quote.ask.unwrap();
    assert_eq!(
        (ask.price, ask.size, ask.exchange.as_str()),
        (price("87.68"), qty("4"), "Q")
    );
    assert_eq!(quote.conditions, ["R"]);
    assert_eq!(quote.tape, "C");
}

#[test]
fn a_zero_priced_side_is_absent_not_a_price() {
    let empty_bid = QUOTE.replace(r#""bp":87.66,"bs":1"#, r#""bp":0,"bs":0"#);
    let StreamMessage::Quote(quote) = one(&empty_bid) else {
        panic!("not a quote");
    };
    assert_eq!(quote.bid, None);
    assert!(quote.ask.is_some());
    let empty_ask = QUOTE.replace(r#""ap":87.68"#, r#""ap":0.0"#);
    let StreamMessage::Quote(quote) = one(&empty_ask) else {
        panic!("not a quote");
    };
    assert!(quote.bid.is_some());
    assert_eq!(quote.ask, None);
}

#[test]
fn a_negative_zero_side_is_absent_too() {
    for zero in ["-0", "-0.0"] {
        let text = QUOTE.replace(r#""bp":87.66"#, &format!(r#""bp":{zero}"#));
        let StreamMessage::Quote(quote) = one(&text) else {
            panic!("not a quote");
        };
        assert_eq!(quote.bid, None, "{zero}");
    }
}

#[test]
fn a_subscription_that_omits_the_statuses_and_bands_reads_them_as_empty() {
    let StreamMessage::Subscribed(channels) =
        one(r#"{"T":"subscription","quotes":["SPY"],"trades":["SPY"]}"#)
    else {
        panic!("not a subscription");
    };
    assert!(channels.statuses.is_empty());
    assert!(channels.lulds.is_empty());
    assert_eq!(channels.quotes, set(&["SPY"]));
}

#[test]
fn a_refusal_names_the_type_it_was_sent_as_when_it_can() {
    let frame = read_frame(&format!(
        "[{},{},17]",
        STATUS.replace(r#""sc":"H""#, r#""sc":7"#),
        QUOTE.replace("87.66", "8.766e1")
    ))
    .unwrap();
    let kinds: Vec<Option<&str>> = frame.refused.iter().map(|r| r.kind.as_deref()).collect();
    assert_eq!(kinds, [Some("s"), Some("q"), None]);
}

#[test]
fn a_trade_a_bar_a_status_and_a_band_are_read_exactly() {
    let StreamMessage::Trade(trade) = one(TRADE) else {
        panic!("not a trade");
    };
    assert_eq!(
        (
            trade.instrument,
            trade.vendor_time,
            trade.id,
            trade.exchange.as_str(),
            trade.price,
            trade.size,
            trade.tape.as_str()
        ),
        (
            id("AAPL"),
            at("2021-02-22T15:51:44.208Z"),
            96921,
            "D",
            price("126.55"),
            qty("1"),
            "C"
        )
    );
    assert_eq!(trade.conditions, ["@", "I"]);

    for (kind_tag, kind) in [
        ("b", BarKind::Minute),
        ("u", BarKind::Updated),
        ("d", BarKind::Daily),
    ] {
        let text = BAR.replace(r#""T":"b""#, &format!(r#""T":"{kind_tag}""#));
        let StreamMessage::Bar(read, bar) = one(&text) else {
            panic!("not a bar");
        };
        assert_eq!(read, kind);
        assert_eq!(bar.instrument, id("SPY"));
        assert_eq!(bar.start, at("2021-02-22T19:15:00Z"));
        assert_eq!(
            [bar.open, bar.high, bar.low, bar.close, bar.vwap],
            [
                price("388.985"),
                price("389.13"),
                price("388.975"),
                price("389.12"),
                price("389.062639")
            ]
        );
        assert_eq!((bar.volume, bar.trades), (qty("49378"), 461));
    }

    let StreamMessage::Status(status) = one(STATUS) else {
        panic!("not a status");
    };
    assert_eq!(
        (
            status.status_code.as_str(),
            status.status_message.as_str(),
            status.reason_code.as_str(),
            status.tape.as_str()
        ),
        ("H", "Trading Halt", "T12", "C")
    );
    assert_eq!(
        status.reason_message,
        "Trading Halted; For information requested by NASDAQ"
    );

    let StreamMessage::Luld(band) = one(LULD) else {
        panic!("not a band");
    };
    assert_eq!(
        (band.instrument, band.up, band.down, band.indicator.as_str()),
        (id("IONM"), price("5.68"), price("4.65"), "B")
    );
}

#[test]
fn a_correction_and_a_cancel_name_their_trade() {
    let StreamMessage::Correction(correction) = one(CORRECTION) else {
        panic!("not a correction");
    };
    assert_eq!(correction.instrument, id("EEM"));
    assert_eq!(
        (
            correction.original.id,
            correction.original.price,
            correction.corrected.id,
            correction.corrected.price
        ),
        (
            52_983_525_033_527,
            price("39.1582"),
            52_983_525_033_540,
            price("39.1809")
        )
    );
    assert_eq!(correction.original.size, qty("440000"));
    assert_eq!(correction.corrected.conditions, [" ", "7", "V"]);

    let StreamMessage::Cancel(cancel) = one(CANCEL) else {
        panic!("not a cancel");
    };
    assert_eq!(
        (
            cancel.id,
            cancel.price,
            cancel.size,
            cancel.action,
            cancel.exchange.as_str()
        ),
        (
            52_983_525_033_800,
            price("180.2"),
            qty("100"),
            CancelAction::Cancel,
            "V"
        )
    );
    let error = CANCEL.replace(r#""a":"C""#, r#""a":"E""#);
    let StreamMessage::Cancel(cancel) = one(&error) else {
        panic!("not a cancel");
    };
    assert_eq!(cancel.action, CancelAction::Error);
}

#[test]
fn a_number_that_went_through_a_float_or_is_not_exact_is_refused() {
    assert!(matches!(
        refusal(&QUOTE.replace("87.66", "8.766e1")),
        WireError::ExponentForm { field: "bp" }
    ));
    assert!(matches!(
        refusal(&QUOTE.replace("87.66", r#""87.66""#)),
        WireError::WrongType { field: "bp" }
    ));
    refusal(&QUOTE.replace("87.66", "87.6600000001"));
    refusal(&TRADE.replace("126.55", "-126.55"));
    refusal(&TRADE.replace("96921", "-1"));
    refusal(&LULD.replace("5.68", "0"));
}

#[test]
fn a_message_missing_a_field_or_with_a_bad_time_or_symbol_is_refused() {
    assert!(matches!(
        refusal(&QUOTE.replace(r#""S":"AMD","#, "")),
        WireError::MissingField { field: "S" }
    ));
    refusal(&QUOTE.replace("2021-02-22T15:51:45.335689322Z", "yesterday"));
    refusal(&TRADE.replace(r#""S":"AAPL""#, r#""S":"""#));
    refusal(&STATUS.replace(r#""sc":"H""#, r#""sc":7"#));
    refusal(&CANCEL.replace(r#""a":"C""#, r#""a":"Z""#));
    refusal(r#"{"T":"i","S":"INAQU","p":1,"z":"C","t":"2021-02-22T19:15:00Z"}"#);
    refusal(r#"{"S":"AMD"}"#);
    refusal("17");
}

#[test]
fn a_frame_that_is_not_an_array_is_not_read() {
    assert_eq!(read_frame("not json"), Err(WireError::NotJson));
    assert!(matches!(
        read_frame(QUOTE),
        Err(WireError::WrongType { .. })
    ));
    assert_eq!(read_frame("[]").unwrap().messages, []);
}

/// A message's text, readable or not, and whether it is readable: the oracle is how it was built.
fn message() -> impl Strategy<Value = (String, bool)> {
    let good = prop_oneof![
        Just(QUOTE),
        Just(TRADE),
        Just(BAR),
        Just(STATUS),
        Just(LULD),
        Just(CORRECTION),
        Just(CANCEL),
    ]
    .prop_map(|text| (text.to_owned(), true));
    let bad = prop_oneof![
        Just(QUOTE.replace("87.66", "8.766e1")),
        Just(TRADE.replace(r#""S":"AAPL","#, "")),
        Just(r#"{"T":"i","S":"X"}"#.to_owned()),
        Just("null".to_owned()),
        Just(BAR.replace("49378", "4.9e4")),
    ]
    .prop_map(|text| (text, false));
    prop_oneof![good, bad]
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 256, failure_persistence: None, ..ProptestConfig::default() })]

    /// Spec §3.1: every message of a frame is either read or refused, never both and never
    /// dropped, the refusals name their places, and a refusal never stops the rest of the frame.
    #[test]
    fn every_message_is_read_or_refused_by_its_place(messages in prop::collection::vec(message(), 0..12)) {
        let text = format!(
            "[{}]",
            messages.iter().map(|(m, _)| m.as_str()).collect::<Vec<_>>().join(",")
        );
        let frame = read_frame(&text).unwrap();
        let refused: Vec<usize> = messages
            .iter()
            .enumerate()
            .filter(|(_, (_, good))| !good)
            .map(|(i, _)| i)
            .collect();
        prop_assert_eq!(frame.refused.iter().map(|r| r.index).collect::<Vec<_>>(), refused);
        prop_assert_eq!(
            frame.messages.len(),
            messages.iter().filter(|(_, good)| *good).count()
        );
    }
}
