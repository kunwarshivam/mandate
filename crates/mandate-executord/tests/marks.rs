//! The executor's risk marks (DEC-2380 item 3): `marks::mark_draft` writes journal spec §9.30's
//! `MarkUpdated` version 2 from a quote, sane only with a positive bid (rule 244), with the
//! session the US-equities calendar places the tick in, or `crypto`, and no mark while an equity's
//! market is closed or for a quote with no price; every mark it writes, the journal accepts.

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::{Object, Value};
use mandate_executor::{EventDraft, EventId, MarketObservation, RiskClock};
use mandate_executord::marks::mark_draft;
use mandate_num::Price;
use mandate_time::ExchangeCalendar;

/// 2026-10-19, a Monday: 14:30:01 UTC is 10:30:01 in New York, in the regular session.
const REGULAR: i64 = 1_792_420_201;
/// 2026-10-19 08:00:01 UTC is 04:00:01 in New York, in the pre-market session.
const PRE_MARKET: i64 = 1_792_396_801;
/// 2026-10-17, a Saturday, 15:00 UTC: the market is closed.
const CLOSED: i64 = 1_792_249_200;

fn price(text: &str) -> Option<Price> {
    Some(Price::parse(text).unwrap())
}

fn observation(bid: Option<Price>, sane: bool) -> MarketObservation {
    MarketObservation {
        instrument: InstrumentId::new("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415").unwrap(),
        bid,
        bid_size: None,
        ask: price("187.45"),
        last_trade: price("187.44"),
        mark: None,
        sane,
        observed_at: RiskClock::from_secs(REGULAR),
    }
}

fn calendar() -> ExchangeCalendar {
    ExchangeCalendar::us_equities().unwrap()
}

fn draft(observation: &MarketObservation, class: AssetClass, at: i64) -> Option<EventDraft> {
    mark_draft(
        observation,
        class,
        &calendar(),
        "fixture",
        EventId("01JAB0M0A000000000000000M1".to_owned()),
        RiskClock::from_secs(at),
    )
}

fn member<'a>(draft: &'a EventDraft, name: &str) -> Option<&'a Value> {
    draft.payload.get(name)
}

fn text(value: &str) -> Option<Value> {
    Some(Value::Str(value.to_owned()))
}

/// A sane quote in the regular session is a sane version-2 mark at the tick, priced at its
/// bid, with exactly §9.30's members.
#[test]
fn a_sane_regular_quote_is_a_sane_regular_mark() {
    let made = draft(
        &observation(price("187.42"), true),
        AssetClass::UsEquity,
        REGULAR,
    )
    .expect("a mark");
    assert_eq!(made.event_type, "MarkUpdated");
    assert_eq!(made.schema_version, 2);
    assert_eq!(made.causation_id, None);
    assert_eq!(made.config_refs, Object::new());
    assert_eq!(made.event_id.0, "01JAB0M0A000000000000000M1");
    let expect = [
        (
            "instrument_id",
            text("b0b6dd9d-8b9b-48a9-ba46-b9d54906e415"),
        ),
        ("price", text("187.42")),
        ("source", text("quote")),
        ("feed", text("fixture")),
        ("bid", text("187.42")),
        ("sane", Some(Value::Bool(true))),
        ("session", text("regular")),
        ("risk_clock", text("2026-10-19T14:30:01.000000000Z")),
    ];
    for (name, want) in &expect {
        assert_eq!(member(&made, name).cloned(), *want, "{name}");
    }
    assert_eq!(
        made.payload.as_object().map(Object::len),
        Some(expect.len())
    );
}

/// The session is the calendar's at the tick: pre-market here, and `crypto` for a crypto
/// instrument at any instant, a weekend's included.
#[test]
fn the_session_is_the_calendar_s_or_crypto() {
    let quote = observation(price("187.42"), true);
    let pre = draft(&quote, AssetClass::UsEquity, PRE_MARKET).expect("a mark");
    assert_eq!(member(&pre, "session").cloned(), text("pre_market"));
    for at in [REGULAR, CLOSED] {
        let crypto = draft(&quote, AssetClass::Crypto, at).expect("a crypto mark");
        assert_eq!(member(&crypto, "session").cloned(), text("crypto"), "{at}");
    }
}

/// An equity's quote while the market is closed is no mark: no session names it.
#[test]
fn an_equity_quote_while_closed_is_no_mark() {
    assert!(
        draft(
            &observation(price("187.42"), true),
            AssetClass::UsEquity,
            CLOSED
        )
        .is_none()
    );
}

/// A quote that is not sane is recorded as such, its bid kept; one with no bid is never sane
/// (rule 244; a bid is a `Price`, positive by its type), and is priced at its last trade.
#[test]
fn no_mark_is_sane_without_a_positive_bid() {
    let insane = draft(
        &observation(price("187.42"), false),
        AssetClass::UsEquity,
        REGULAR,
    )
    .expect("a mark");
    assert_eq!(member(&insane, "sane").cloned(), Some(Value::Bool(false)));
    assert_eq!(member(&insane, "bid").cloned(), text("187.42"));
    let no_bid = draft(&observation(None, true), AssetClass::UsEquity, REGULAR).expect("a mark");
    assert_eq!(member(&no_bid, "sane").cloned(), Some(Value::Bool(false)));
    assert_eq!(member(&no_bid, "bid").cloned(), Some(Value::Null));
    assert_eq!(member(&no_bid, "price").cloned(), text("187.44"));
}

/// A quote with no bid, no last trade and no mark has no price, and is no mark.
#[test]
fn a_quote_with_no_price_is_no_mark() {
    let mut quote = observation(None, false);
    quote.last_trade = None;
    assert!(draft(&quote, AssetClass::UsEquity, REGULAR).is_none());
    quote.mark = price("187.4");
    let marked = draft(&quote, AssetClass::UsEquity, REGULAR).expect("a mark");
    assert_eq!(member(&marked, "price").cloned(), text("187.4"));
}

/// Every draft the module writes is one the journal accepts as a version-2 mark.
#[test]
fn every_mark_is_one_the_journal_accepts() {
    for (bid, sane, class, at) in [
        (price("187.42"), true, AssetClass::UsEquity, REGULAR),
        (price("187.42"), false, AssetClass::UsEquity, PRE_MARKET),
        (None, true, AssetClass::UsEquity, REGULAR),
        (price("0.00000001"), true, AssetClass::Crypto, CLOSED),
    ] {
        let made = draft(&observation(bid, sane), class, at).expect("a mark");
        let body = mandate_canon::to_canonical(&envelope(&made));
        let parsed = mandate_journal::Draft::parse(&body);
        assert!(parsed.is_ok(), "{parsed:?}");
    }
}

/// `made` in the journal's envelope on an account stream, as the process would append it.
fn envelope(made: &EventDraft) -> Value {
    let payload = String::from_utf8(mandate_canon::to_canonical(&made.payload)).unwrap();
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{}","stream_id":"acct:ws_01J8Z2:01J8Z2ACCT00000000000000A1","event_type":"{}","schema_version":2,"event_time":"2026-10-19T14:30:00.250000000Z","clock_source":"local","causation_id":null,"correlation_id":null,"actor":{{"kind":"system","id":"executor","version":"0.1.0","build":"sha256:3333333333333333333333333333333333333333333333333333333333333333"}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#,
        made.event_id.0, made.event_type
    );
    mandate_canon::parse(body.as_bytes()).unwrap()
}
