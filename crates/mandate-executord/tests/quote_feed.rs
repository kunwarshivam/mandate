//! E21-30, DEC-2359: the executor process's quote feed. A held exit beside resting protection is
//! held for want of a price until the feed reads its instrument's quote, and then the ladder
//! prices it; the feed is asked only about instruments whose exit waits.

mod support;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    BarsRequest, DataTransport, HttpRequest, Pause, QuoteRequest, Response, TradingTransport,
    TransportError,
};
use mandate_canon::Value;
use mandate_executor::{BrokerRequest, MarketObservation, OrderType, RiskClock, SubmitOrder};
use mandate_executord::alpaca::AlpacaBroker;
use mandate_executord::credential::paper_wiring;
use mandate_executord::quotes::QuoteFeed;
use mandate_executord::{ExecutorProcess, fixture};
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use support::{
    AAPL, ACCOUNT_STREAM, AGENT, Allowing, Error, Journal, NOON, OPENED, OPENING, Paper,
    TwoPercentStop, agent_stream, at, deployed, object, propose_market_exit, start_wired, wired,
};

const MSFT: &str = "MSFT";
/// The agent's opening in MSFT, which waits on no exit price.
const OPENING_MSFT: &str = "01JINTENT0000000000000000M";
const EXIT: &str = "01JINTENT0000000000000000C";
/// The recorded opening's protective order, as its journaled leg names it.
const PROTECTION: &str = "md-01JINTENT0000000000000000A-pe1h11o8";

/// A canonical string value.
fn string(value: &str) -> Value {
    Value::Str(value.to_owned())
}

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
    fn observe(
        &self,
        instrument: &InstrumentId,
        _now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        self.asked.borrow_mut().push(instrument.clone());
        self.quote
            .clone()
            .filter(|quote| &quote.instrument == instrument)
            .ok_or("no_quote")
    }
}

/// The executor of a held exit beside its protection, its journal, its broker, and the count of
/// exit sells the broker had seen after each of the four passes after the exit's proposal.
type Exiting = (Journal, Paper, ExecutorProcess<Journal, Paper>, Vec<usize>);

/// The agent's ten AAPL, bought by the recorded opening and filled, its protection resting at the
/// broker, and then the agent's risk exit at the market, whose sequence (§5.4) cancels the
/// protection first and the broker confirms, with `quotes` as the process's feed. The agent's
/// MSFT opening is proposed beside them, so the account holds an instrument whose exit waits on
/// nothing.
fn exit_beside_its_protection(quotes: Recorded) -> Result<Exiting, Error> {
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
    journal.write(
        &agent_stream(AGENT),
        OPENING_MSFT,
        "IntentProposed",
        1,
        Some("decision"),
        object(vec![
            ("instrument_id", string(MSFT)),
            ("limit_price", string("400")),
            ("purpose", string("open")),
            ("qty", string("1")),
            ("side", string("buy")),
            ("tif", string("day")),
            ("type", string("limit")),
        ])?,
    )?;
    propose_market_exit(&journal, AGENT, EXIT, "risk_exit", "decision")?;
    let mut sold = Vec::new();
    for _ in 0..4 {
        process.step(at(NOON)?);
        sold.push(exit_sells(&broker).len());
        for seen in broker.seen() {
            if let BrokerRequest::Cancel { client_order_id } = seen.request {
                broker.canceled(client_order_id.as_str())?;
            }
        }
    }
    Ok((journal, broker, process, sold))
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

/// DEC-2037 item 5, §5.6: with no feed, the exit beside its protection has nothing to price from
/// and sends no sell. With a fresh, sane quote read for it, the ladder prices it in the pass that
/// reads it, whose tick follows the read: one limit sell below the bid. The feed is asked about
/// AAPL only once its exit waits, and never about the agent's MSFT, whose exit waits on nothing.
#[test]
fn a_held_exit_beside_its_protection_is_priced_once_its_quote_is_read() -> Result<(), Error> {
    let (journal, unquoted, _, _) = exit_beside_its_protection(Recorded::default())?;
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
    let (_, quoted, _, sold) = exit_beside_its_protection(quotes)?;
    assert_eq!(
        sold.iter().position(|sells| *sells > 0),
        Some(1),
        "the pass that first reads the quote, the one after the protection's cancel is confirmed, \
         prices the exit: {sold:?}"
    );
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

/// While the exit waits, its instrument is read once when first wanted and then once per
/// `exit_step_s`, five seconds here: not again a second short of the step, and again at it.
#[test]
fn a_waiting_exits_quote_is_read_once_per_step() -> Result<(), Error> {
    let quotes = Recorded::default();
    let asked = Rc::clone(&quotes.asked);
    let (journal, _, mut process, _) = exit_beside_its_protection(quotes)?;
    assert!(
        journal
            .received()
            .iter()
            .any(|intent| intent == OPENING_MSFT),
        "the case builds an account with an MSFT intent whose exit waits on nothing"
    );
    let noon = at(NOON)?.secs();
    let mut counts = vec![asked.borrow().len()];
    for later in [4, 5] {
        process.step(UtcNanos::from_parts(noon + later, 0)?);
        counts.push(asked.borrow().len());
    }
    assert_eq!(counts, [1, 1, 2]);
    Ok(())
}

/// A clock fixed at one instant, which never waits.
#[derive(Clone, Copy)]
struct Fixed(UtcNanos);

impl Pause for Fixed {
    fn now(&self) -> UtcNanos {
        self.0
    }

    async fn pause(&self, _duration: Duration) {}
}

/// A transport that answers every latest-quote read with one fresh AAPL quote, and refuses every
/// trading call.
#[derive(Clone)]
struct Quoting(String);

impl TradingTransport for Quoting {
    async fn send(&self, _request: &HttpRequest) -> Result<Response, TransportError> {
        Err(TransportError::RefusedPath)
    }
}

impl DataTransport for Quoting {
    async fn send(&self, _request: &QuoteRequest) -> Result<Response, TransportError> {
        Ok(Response {
            status: 200,
            body: format!(
                r#"{{"quote":{{"ap":150.2,"as":3,"bp":150,"bs":2,"t":"{}"}},"symbol":"AAPL"}}"#,
                self.0
            )
            .into_bytes(),
        })
    }

    async fn send_bars(&self, _request: &BarsRequest) -> Result<Response, TransportError> {
        Err(TransportError::RefusedPath)
    }
}

/// DEC-2359 item 2: the paper serve path's wiring reads quotes over its broker's own transport, so
/// a quote that transport answers is handed, where the fixture's wiring hands none.
#[test]
fn the_paper_serve_path_reads_quotes_over_its_broker() -> Result<(), Error> {
    let now = at(NOON)?;
    let broker = AlpacaBroker::new(Quoting(now.to_string()))?;
    let aapl = InstrumentId::new(AAPL)?;
    let read = paper_wiring(&broker, Fixed(now))?
        .quotes
        .observe(&aapl, now)?;
    assert_eq!(
        (read.bid, read.ask, read.sane),
        (
            Some(Price::parse("150")?),
            Some(Price::parse("150.2")?),
            true
        )
    );
    assert!(fixture::wiring()?.quotes.observe(&aapl, now).is_err());
    Ok(())
}
