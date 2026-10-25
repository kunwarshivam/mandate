//! E21-30, DEC-2359: the executor process's quote feed. A held exit beside resting protection is
//! held for want of a price until the feed reads its instrument's quote, and then the ladder
//! prices it; the feed is asked only about instruments whose exit waits.

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_canon::Value;
use mandate_executor::{BrokerRequest, MarketObservation, OrderType, RiskClock, SubmitOrder};
use mandate_executord::quotes::QuoteFeed;
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use support::{
    AAPL, ACCOUNT_STREAM, AGENT, Allowing, Error, Journal, NOON, OPENED, OPENING, Paper,
    TwoPercentStop, at, deployed, propose_market_exit, start_wired, wired,
};

const EXIT: &str = "01JINTENT0000000000000000C";
/// The recorded opening's protective order, as its journaled leg names it.
const PROTECTION: &str = "md-01JINTENT0000000000000000A-pe1h11o8";

/// A sane AAPL quote at `bid` and `ask`, observed at `observed`.
fn observed(bid: &str, ask: &str, observed: i64) -> Result<MarketObservation, Error> {
    Ok(MarketObservation {
        instrument: InstrumentId::new(AAPL)?,
        bid: Some(Price::parse(bid)?),
        bid_size: Some(Qty::parse("2")?),
        ask: Some(Price::parse(ask)?),
        last_trade: None,
        mark: None,
        sane: true,
        observed_at: RiskClock::from_secs(observed),
    })
}

/// The quotes a test feed answers, and every instrument it was asked about, in order.
#[derive(Clone, Default)]
struct Recorded {
    quote: Option<MarketObservation>,
    asked: Rc<RefCell<Vec<InstrumentId>>>,
}

impl QuoteFeed for Recorded {
    fn observe(&self, instrument: &InstrumentId, _now: UtcNanos) -> Option<MarketObservation> {
        self.asked.borrow_mut().push(instrument.clone());
        self.quote
            .clone()
            .filter(|quote| &quote.instrument == instrument)
    }
}

/// The agent's ten AAPL, bought by the recorded opening and filled, its protection resting at the
/// broker, and then the agent's risk exit at the market, whose sequence (§5.4) cancels the
/// protection first and the broker confirms, with `quotes` as the process's feed.
fn exit_beside_its_protection(quotes: Recorded) -> Result<(Journal, Paper), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    broker.resting(OPENING, AAPL, "10", "150")?;
    broker.fill(OPENING, "150")?;
    broker.resting(PROTECTION, AAPL, "10", "147")?;
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(quotes);
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    propose_market_exit(&journal, AGENT, EXIT, "risk_exit", "decision")?;
    for _ in 0..4 {
        process.step(at(NOON)?);
        for seen in broker.seen() {
            if let BrokerRequest::Cancel { client_order_id } = seen.request {
                broker.canceled(client_order_id.as_str())?;
            }
        }
    }
    Ok((journal, broker))
}

fn exit_sells(broker: &Paper) -> Vec<SubmitOrder> {
    broker
        .seen()
        .into_iter()
        .filter_map(|seen| match seen.request {
            BrokerRequest::Submit(order) if order.client_order_id.as_str().ends_with(EXIT) => {
                Some(order)
            }
            _ => None,
        })
        .collect()
}

/// DEC-2037 item 5, §5.6: with no feed, the exit beside the bracket has nothing to price from
/// and sends no sell. With a fresh, sane quote read for it, the ladder prices it: one limit sell
/// below the bid. The feed is asked about AAPL only once its exit waits, and about nothing else.
#[test]
#[ignore = "pending E21-30"]
fn a_held_exit_beside_its_protection_is_priced_once_its_quote_is_read() -> Result<(), Error> {
    let (journal, unquoted) = exit_beside_its_protection(Recorded::default())?;
    let held = journal
        .payloads("GateDecided")
        .last()
        .is_some_and(|decided| {
            decided.get("verdict").and_then(Value::as_str) == Some("hold")
                && decided.get("reason_code").and_then(Value::as_str) == Some("exit_unpriced")
        });
    assert!(held, "the case builds an exit held for want of a price");
    assert!(
        exit_sells(&unquoted).is_empty(),
        "with no quote the exit is held"
    );
    let noon = at(NOON)?.secs();
    let quotes = Recorded {
        quote: Some(observed("150", "150.2", noon)?),
        asked: Rc::default(),
    };
    let asked = Rc::clone(&quotes.asked);
    let (_, quoted) = exit_beside_its_protection(quotes)?;
    let sells = exit_sells(&quoted);
    let bid = Price::parse("150")?;
    assert!(
        matches!(
            sells.as_slice(),
            [sell] if sell.order_type == OrderType::Limit
                && sell.limit_price.is_some_and(|limit| limit < bid)
        ),
        "{sells:?}"
    );
    let asked = asked.borrow();
    assert!(!asked.is_empty(), "the feed was asked");
    assert!(
        asked.iter().all(|asked| asked.as_str() == AAPL),
        "{asked:?}"
    );
    Ok(())
}
