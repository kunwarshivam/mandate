//! E21-30, DEC-2359: the executor's quote reads. Each instrument whose exit waits on a price is
//! read at once and then at most once per `exit_step_s`; a quote that reads and is sane (trading
//! spec §8.2) is handed; and one that cannot be read, is stale or is insane hands nothing.

mod support;

use std::collections::BTreeSet;
use std::time::Duration;

use mandate_accounting::InstrumentId;
use mandate_alpaca::{
    BarsRequest, DataClient, DataTransport, Pause, QuoteRequest, Response, TransportError,
};
use mandate_executor::{MarketObservation, RiskClock};
use mandate_executord::quotes::{AlpacaQuotes, QuoteBounds, QuoteFeed, QuotePoll};
use mandate_num::{Fraction, Price, Qty};
use mandate_time::UtcNanos;
use support::{AAPL, Error, NOON, at};

fn aapl() -> Result<InstrumentId, Error> {
    Ok(InstrumentId::new(AAPL)?)
}

fn secs(n: i64) -> Result<UtcNanos, Error> {
    Ok(UtcNanos::from_parts(n, 0)?)
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

/// A data host that answers every latest-quote read with one scripted response.
struct Scripted(Result<Response, TransportError>);

impl DataTransport for Scripted {
    async fn send(&self, _request: &QuoteRequest) -> Result<Response, TransportError> {
        self.0.clone()
    }

    async fn send_bars(&self, _request: &BarsRequest) -> Result<Response, TransportError> {
        Err(TransportError::RefusedPath)
    }
}

/// An AAPL latest-quote answer stamped `t`.
fn answer(bid: &str, ask: &str, t: &str) -> Result<Response, TransportError> {
    Ok(Response {
        status: 200,
        body: format!(
            r#"{{"quote":{{"ap":{ask},"as":3,"bp":{bid},"bs":2,"t":"{t}"}},"symbol":"AAPL"}}"#
        )
        .into_bytes(),
    })
}

/// The paper profile's bounds as these cases use them: a quote no older than ten seconds, and a
/// spread of at most 1 % of the bid.
fn bounds() -> Result<QuoteBounds, Error> {
    Ok(QuoteBounds {
        max_age: Duration::from_secs(10),
        max_spread: Fraction::parse("0.01")?,
    })
}

fn feed(
    response: Result<Response, TransportError>,
    now: UtcNanos,
) -> Result<AlpacaQuotes<Scripted, Fixed>, Error> {
    Ok(AlpacaQuotes::new(
        DataClient::new(Scripted(response), Fixed(now)),
        bounds()?,
    )?)
}

/// What a sane read of AAPL at `bid` and `ask` observed at `observed` must be.
fn observed(bid: &str, ask: &str, observed: i64) -> Result<MarketObservation, Error> {
    Ok(MarketObservation {
        instrument: aapl()?,
        bid: Some(Price::parse(bid)?),
        bid_size: Some(Qty::parse("2")?),
        ask: Some(Price::parse(ask)?),
        last_trade: None,
        mark: None,
        sane: true,
        observed_at: RiskClock::from_secs(observed),
    })
}

/// A wanted instrument is read at once, then not again until `every` has passed since its last
/// read; one no longer wanted is forgotten, so it is read at once when it is wanted again.
#[test]
#[ignore = "pending E21-30"]
fn a_wanted_instrument_is_read_at_once_then_once_per_step() -> Result<(), Error> {
    let msft = InstrumentId::new("MSFT")?;
    let both = BTreeSet::from([aapl()?, msft.clone()]);
    let mut poll = QuotePoll::new(Duration::from_secs(5));
    let t = 1_790_000_000;
    assert_eq!(poll.due(&both, secs(t)?), vec![aapl()?, msft.clone()]);
    assert_eq!(poll.due(&both, secs(t + 4)?), Vec::<InstrumentId>::new());
    assert_eq!(poll.due(&both, secs(t + 5)?), vec![aapl()?, msft.clone()]);
    let only_aapl = BTreeSet::from([aapl()?]);
    assert_eq!(
        poll.due(&only_aapl, secs(t + 6)?),
        Vec::<InstrumentId>::new()
    );
    assert_eq!(poll.due(&both, secs(t + 7)?), vec![msft]);
    assert_eq!(
        poll.due(&BTreeSet::new(), secs(t + 20)?),
        Vec::<InstrumentId>::new()
    );
    Ok(())
}

/// Trading spec §8.2: a quote with `0 < bid ≤ ask` and a spread within the profile's bound is
/// sane, a locked one too, and is observed at its own stamp, or at the clock's now when stamped
/// ahead. A wider or a crossed one hands nothing (DEC-2359), so the core prices nothing from it.
#[test]
#[ignore = "pending E21-30"]
fn a_quote_is_handed_only_when_sane() -> Result<(), Error> {
    let now = at(NOON)?;
    let noon = now.secs();
    let before = "2026-09-21T14:59:58Z";
    for (bid, ask) in [("150", "150.2"), ("150", "150"), ("150", "151.5")] {
        let quotes = feed(answer(bid, ask, before), now)?;
        let observation = observed(bid, ask, noon - 2)?;
        assert_eq!(
            quotes.read(&aapl()?, now),
            Ok(observation.clone()),
            "{bid} × {ask}"
        );
        assert_eq!(
            quotes.observe(&aapl()?, now),
            Some(observation),
            "{bid} × {ask}"
        );
    }
    for (bid, ask) in [("150", "151.6"), ("150.2", "150")] {
        let quotes = feed(answer(bid, ask, before), now)?;
        assert_eq!(
            quotes.read(&aapl()?, now),
            Err("quote_insane"),
            "{bid} × {ask}"
        );
        assert_eq!(quotes.observe(&aapl()?, now), None, "{bid} × {ask}");
    }
    let ahead = "2026-09-21T15:00:00.200Z";
    assert_eq!(
        feed(answer("150", "150.2", ahead), now)?.read(&aapl()?, now),
        Ok(observed("150", "150.2", noon)?),
        "a quote stamped ahead of the clock is observed at the clock's now"
    );
    Ok(())
}

/// `AGENTS.md` rule 3: a quote older than the profile's bound, an answer that is not a quote, a
/// refused or failed read, or a quote of another instrument hands nothing, and the refusal names
/// neither the instrument nor a price.
#[test]
#[ignore = "pending E21-30"]
fn a_quote_that_cannot_be_read_hands_nothing() -> Result<(), Error> {
    let now = at(NOON)?;
    let not_quote = |body: &str| {
        Ok(Response {
            status: 200,
            body: body.as_bytes().to_vec(),
        })
    };
    for (case, response) in [
        ("stale", answer("150", "150.2", "2026-09-21T14:59:49Z")),
        (
            "refused",
            Ok(Response {
                status: 403,
                body: Vec::new(),
            }),
        ),
        (
            "unavailable",
            Ok(Response {
                status: 503,
                body: Vec::new(),
            }),
        ),
        ("not sent", Err(TransportError::RefusedPath)),
        ("no quote", not_quote(r#"{"quote":null,"symbol":"AAPL"}"#)),
        (
            "another instrument",
            not_quote(
                r#"{"quote":{"ap":1,"as":1,"bp":1,"bs":1,"t":"2026-09-21T14:59:59Z"},"symbol":"MSFT"}"#,
            ),
        ),
        ("one-sided", answer("0", "150.2", "2026-09-21T14:59:58Z")),
        ("garbage", not_quote("not json")),
    ] {
        let quotes = feed(response, now)?;
        let read = quotes.read(&aapl()?, now);
        assert_eq!(read, Err("quote_unread"), "{case}");
        assert_eq!(quotes.observe(&aapl()?, now), None, "{case}");
    }
    Ok(())
}
