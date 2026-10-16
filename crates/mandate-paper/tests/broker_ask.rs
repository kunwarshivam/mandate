//! The asked opening's broker ask at the check (DEC-945; DEC-529 item 12; DEC-940 item 4): a
//! Robinhood session's ask is read through the connector's `get_equity_quotes` when the
//! cross-check runs, held to the opening's quote age bound by the run's own clock, and compared
//! with the IEX ask within the registered collar. A missing, stale, garbled or wrong-instrument
//! quote refuses with nothing sent. The broker is a fixture seam answering literal JSON; nothing
//! reaches a host (`AGENTS.md` rule 8). Oracles: the band is recomputed here in whole
//! nanodollars, and the age from the fixture clock's own instants.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{Feed, LatestQuote, Pause};
use mandate_domain::AssetClass;
use mandate_mcp::{CallClass, McpError};
use mandate_num::{Fraction, Price, Qty, Usd};
use mandate_paper::{
    BrokerAsk, CollarMembers, PaperError, QuoteRefusal, broker_cross_check, fresh_ask,
};
use mandate_robinhood::{Quote, RobinhoodConnector, Tools};
use mandate_shell::robinhood::RobinhoodBridge;
use mandate_time::UtcNanos;
use proptest::prelude::*;
use serde_json::{Value, json};

const MAX_AGE: Duration = Duration::from_secs(10);
const T0: i64 = 1_792_162_800;

/// The broker seam: every call it saw, and the one answer it gives each call.
#[derive(Clone)]
struct Fixture {
    answer: Rc<RefCell<Result<String, McpError>>>,
    seen: Rc<RefCell<Vec<(CallClass, &'static str, Value)>>>,
}

impl Tools for Fixture {
    async fn call_tool(
        &self,
        class: CallClass,
        tool: &'static str,
        arguments: &Value,
    ) -> Result<String, McpError> {
        self.seen
            .borrow_mut()
            .push((class, tool, arguments.clone()));
        match &*self.answer.borrow() {
            Ok(text) => Ok(text.clone()),
            Err(_) => Err(McpError::Network),
        }
    }
}

/// An MCP tool result whose `structuredContent` is `content`.
fn answered(content: Value) -> Result<String, McpError> {
    Ok(json!({
        "content": [{"type": "text", "text": content.to_string()}],
        "structuredContent": content,
    })
    .to_string())
}

/// `get_equity_quotes`' answer of one record for `symbol` at `bid` and `ask`.
fn quoted(symbol: &str, bid: &str, ask: &str) -> Result<String, McpError> {
    answered(json!({"quotes": [{"symbol": symbol, "bid_price": bid, "ask_price": ask}]}))
}

impl Fixture {
    fn answering(answer: Result<String, McpError>) -> Self {
        Self {
            answer: Rc::new(RefCell::new(answer)),
            seen: Rc::default(),
        }
    }

    fn ask(&self) -> BrokerAsk {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime builds");
        let connector = RobinhoodConnector::new(self.clone(), "5QR00001".to_owned());
        BrokerAsk::robinhood(RobinhoodBridge::new(runtime, connector))
    }
}

/// A clock that answers its instants in turn, in nanoseconds past [`T0`], and counts its reads.
struct Steps {
    instants: RefCell<VecDeque<u64>>,
    reads: Cell<usize>,
}

impl Steps {
    fn of(instants: &[u64]) -> Self {
        Self {
            instants: RefCell::new(instants.iter().copied().collect()),
            reads: Cell::new(0),
        }
    }
}

fn at(nanos: u64) -> UtcNanos {
    let secs = i64::try_from(nanos / 1_000_000_000).expect("the offset fits");
    let sub = u32::try_from(nanos % 1_000_000_000).expect("the remainder fits");
    UtcNanos::from_parts(T0 + secs, sub).expect("the instant parses")
}

impl Pause for Steps {
    fn now(&self) -> UtcNanos {
        self.reads.set(self.reads.get() + 1);
        at(self
            .instants
            .borrow_mut()
            .pop_front()
            .expect("the check reads the clock no more often than the fixture plans"))
    }

    async fn pause(&self, _duration: Duration) {
        panic!("the check never waits");
    }
}

fn price(text: &str) -> Price {
    Price::parse(text).expect("the price parses")
}

fn spy() -> InstrumentId {
    InstrumentId::new("SPY").expect("the symbol parses")
}

fn collar() -> CollarMembers {
    CollarMembers {
        liquid_x: Fraction::parse("0.01").expect("the liquid collar parses"),
        other_x: Fraction::parse("0.02").expect("the other collar parses"),
        crypto_x: Fraction::parse("0.005").expect("the crypto collar parses"),
        liquid_threshold_usd: Usd::parse("50000000").expect("the threshold parses"),
    }
}

/// The IEX quote of SPY at `ask`.
fn iex(ask: &str) -> LatestQuote {
    LatestQuote {
        instrument: spy(),
        at: at(0),
        bid: price("1"),
        bid_size: Qty::parse("1").expect("the size parses"),
        ask: price(ask),
        ask_size: Qty::parse("1").expect("the size parses"),
        feed: Feed::Iex,
        clock_skew: None,
    }
}

fn liquid() -> Option<Usd> {
    Some(Usd::parse("60000000").expect("the volume parses"))
}

/// The cross-check of `broker` against the IEX ask `independent`, the clock reading `instants`.
fn check(broker: &BrokerAsk, independent: &str, instants: &[u64]) -> Result<(), PaperError> {
    broker_cross_check(
        &collar(),
        AssetClass::UsEquity,
        liquid(),
        broker,
        &iex(independent),
        &Steps::of(instants),
        MAX_AGE,
    )
}

fn refused(result: Result<(), PaperError>) -> Option<QuoteRefusal> {
    match result {
        Err(PaperError::CrossCheck(refusal)) => Some(refusal),
        _ => None,
    }
}

/// DEC-945 items 1 and 3: the ask is read when the check runs, never before, by one
/// `get_equity_quotes` on the `Ordinary` budget for the IEX quote's instrument alone; Robinhood's
/// ask, not its bid, is what must sit within the liquid collar's 1% of the IEX ask.
#[test]
fn robinhoods_ask_is_read_at_the_check_and_compared_within_the_collar() {
    let broker = Fixture::answering(quoted("SPY", "90", "100.5"));
    let ask = broker.ask();
    assert!(broker.seen.borrow().is_empty(), "nothing is read before the check");
    assert!(matches!(check(&ask, "100", &[0, 1]), Ok(())));
    assert_eq!(
        *broker.seen.borrow(),
        [(
            CallClass::Ordinary,
            "get_equity_quotes",
            json!({"symbols": ["SPY"]})
        )]
    );
    *broker.answer.borrow_mut() = quoted("SPY", "100", "101.5");
    assert_eq!(
        refused(check(&ask, "100", &[0, 1])),
        Some(QuoteRefusal::Disagreement)
    );
    *broker.answer.borrow_mut() = quoted("SPY", "98", "98.5");
    assert_eq!(
        refused(check(&ask, "100", &[0, 1])),
        Some(QuoteRefusal::Disagreement)
    );
    assert_eq!(broker.seen.borrow().len(), 3, "one read per check");
}

/// DEC-945 item 1: a failed call, a refused one, and every answer that is not exactly one
/// uncrossed record of the instrument refuse `quote_absent`, never a pass.
#[test]
fn a_missing_garbled_or_wrong_instrument_quote_refuses_absent() {
    let answers = [
        ("failed", Err(McpError::Network)),
        ("not json", Ok("{".to_owned())),
        (
            "refused",
            Ok(json!({"content": [], "isError": true}).to_string()),
        ),
        ("no quote", answered(json!({"quotes": []}))),
        (
            "two quotes",
            answered(json!({"quotes": [
                {"symbol": "SPY", "bid_price": "100", "ask_price": "100"},
                {"symbol": "SPY", "bid_price": "100", "ask_price": "100"},
            ]})),
        ),
        ("another instrument", quoted("QQQ", "100", "100")),
        ("crossed", quoted("SPY", "100.01", "100")),
        (
            "a float ask",
            answered(json!({"quotes": [{"symbol": "SPY", "bid_price": "100", "ask_price": 100}]})),
        ),
        (
            "a time field",
            answered(json!({"quotes": [
                {"symbol": "SPY", "bid_price": "100", "ask_price": "100", "updated_at": "now"},
            ]})),
        ),
        (
            "another member",
            answered(json!({"quotes": [
                {"symbol": "SPY", "bid_price": "100", "ask_price": "100"},
            ], "next": null})),
        ),
    ];
    for (name, answer) in answers {
        let broker = Fixture::answering(answer);
        assert_eq!(
            refused(check(&broker.ask(), "100", &[0, 1])),
            Some(QuoteRefusal::Absent),
            "{name}"
        );
        assert_eq!(broker.seen.borrow().len(), 1, "{name}");
    }
}

/// DEC-945 item 2: the read's age is the run clock's after the call less its before, so a read
/// that took the whole bound passes and one a nanosecond longer refuses `quote_stale`, with no
/// skew tolerance; the clock is read exactly twice.
#[test]
fn a_read_slower_than_the_bound_refuses_stale() {
    let broker = Fixture::answering(quoted("SPY", "100", "100"));
    let ask = broker.ask();
    let bound = u64::try_from(MAX_AGE.as_nanos()).expect("the bound fits");
    let clock = Steps::of(&[5, 5 + bound]);
    assert_eq!(ask.ask(&spy(), &clock, MAX_AGE), Ok(price("100")));
    assert_eq!(clock.reads.get(), 2);
    assert_eq!(
        refused(check(&ask, "100", &[5, 6 + bound])),
        Some(QuoteRefusal::Stale)
    );
    assert_eq!(
        refused(check(&ask, "100", &[0, 250_000_000 + bound])),
        Some(QuoteRefusal::Stale),
        "no 250 ms skew tolerance applies to Robinhood's quote"
    );
}

/// DEC-945 item 2: a clock that runs backwards across the read leaves the quote's age unknown,
/// so it refuses `quote_stale` rather than read as fresh.
#[test]
fn a_clock_that_runs_backwards_across_the_read_refuses_stale() {
    let broker = Fixture::answering(quoted("SPY", "100", "100"));
    assert_eq!(
        refused(check(&broker.ask(), "100", &[2, 1])),
        Some(QuoteRefusal::Stale)
    );
}

/// DEC-940 item 4, kept: a session that reads no broker quote refuses `quote_absent` and an
/// Alpaca session's ask is the preflight's own IEX ask, read from no broker and no clock.
#[test]
fn an_absent_ask_refuses_and_a_quoted_ask_is_the_preflights() {
    assert_eq!(
        refused(check(&BrokerAsk::Absent, "100", &[])),
        Some(QuoteRefusal::Absent)
    );
    let clock = Steps::of(&[]);
    assert_eq!(
        BrokerAsk::Quoted(price("100.25")).ask(&spy(), &clock, MAX_AGE),
        Ok(price("100.25"))
    );
    assert_eq!(clock.reads.get(), 0);
    assert!(matches!(
        check(&BrokerAsk::Quoted(price("101")), "100", &[]),
        Ok(())
    ));
    assert_eq!(
        refused(check(&BrokerAsk::Quoted(price("101.01")), "100", &[])),
        Some(QuoteRefusal::Disagreement)
    );
}

/// DEC-945 item 2, the pure judge: no quote and another instrument are absent; the age bound is
/// closed at `max_age`; an answer before its send is stale.
#[test]
fn the_judge_reads_the_instrument_and_the_reads_own_age() {
    let quote = Quote {
        symbol: spy(),
        bid: price("99"),
        ask: price("100"),
    };
    let bound = u64::try_from(MAX_AGE.as_nanos()).expect("the bound fits");
    assert_eq!(
        fresh_ask(Some(&quote), &spy(), at(7), at(7 + bound), MAX_AGE),
        Ok(price("100"))
    );
    assert_eq!(
        fresh_ask(Some(&quote), &spy(), at(7), at(7), MAX_AGE),
        Ok(price("100"))
    );
    assert_eq!(
        fresh_ask(Some(&quote), &spy(), at(7), at(8 + bound), MAX_AGE),
        Err(QuoteRefusal::Stale)
    );
    assert_eq!(
        fresh_ask(Some(&quote), &spy(), at(8), at(7), MAX_AGE),
        Err(QuoteRefusal::Stale)
    );
    assert_eq!(
        fresh_ask(None, &spy(), at(7), at(7), MAX_AGE),
        Err(QuoteRefusal::Absent)
    );
    let qqq = InstrumentId::new("QQQ").expect("the symbol parses");
    assert_eq!(
        fresh_ask(Some(&quote), &qqq, at(7), at(7), MAX_AGE),
        Err(QuoteRefusal::Absent)
    );
    assert_eq!(
        QuoteRefusal::Stale.code(),
        "quote_stale",
        "the stable reason code"
    );
}

/// Whole nanodollars of `cents` cents.
fn nanodollars(cents: i128) -> i128 {
    cents * 10_000_000
}

/// `nanos` nanodollars as decimal text.
fn text(nanos: i128) -> String {
    format!("{}.{:09}", nanos / 1_000_000_000, nanos % 1_000_000_000)
}

proptest! {
    /// DEC-529 item 12 around its tolerance: for an IEX ask and a Robinhood ask a few
    /// nanodollars either side of either end of the liquid collar's band, the whole path — the
    /// read at the check, the age judge, the collar — agrees exactly when the Robinhood ask is
    /// inside the band this test computes for itself (the ceiling truncated, the floor rounded
    /// up, at 9 places), and refuses `quote_disagreement` otherwise.
    #[test]
    fn broker_asks_around_the_collar_agree_exactly_inside_the_band(
        cents in 100_i128..=10_000_000,
        upper in any::<bool>(),
        nudge in -3_i128..=3,
        read_ns in 0_u64..=10_000_000_000,
    ) {
        let independent = nanodollars(cents);
        let ceiling = independent * 101 / 100;
        let floor = (independent * 99 + 99) / 100;
        let broker_nanos = if upper { ceiling } else { floor } + nudge;
        let inside = floor <= broker_nanos && broker_nanos <= ceiling;
        let broker = Fixture::answering(quoted("SPY", "0.01", &text(broker_nanos)));
        let result = check(&broker.ask(), &text(independent), &[0, read_ns]);
        if inside {
            prop_assert!(matches!(result, Ok(())), "{result:?}");
        } else {
            prop_assert_eq!(refused(result), Some(QuoteRefusal::Disagreement));
        }
    }
}
