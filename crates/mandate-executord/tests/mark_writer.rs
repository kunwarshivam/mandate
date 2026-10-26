//! DEC-2380 item 3: the executor process marks each instrument its wiring names. At each pass
//! that reads a tick, an instrument due a mark whose quote the feed hands is journaled on the
//! account stream as a `MarkUpdated` version 2 at that tick, at most once every
//! [`marks::MARK_EVERY`]; an instrument the feed has no quote for, or the snapshot no asset class
//! for, is not marked, and a wiring that names no instrument marks nothing.

mod support;

use std::collections::BTreeSet;

use mandate_accounting::InstrumentId;
use mandate_canon::Value;
use mandate_executor::{MarketObservation, RiskClock};
use mandate_executord::marks::{self, Marked};
use mandate_executord::quotes::QuoteFeed;
use mandate_num::{Price, Qty};
use mandate_time::UtcNanos;
use support::{
    AAPL, ACCOUNT_STREAM, Allowing, Error, Fault, Journal, NOON, OPENED, Paper, TwoPercentStop, at,
    deployed, start_wired, tick, wired,
};

/// The instruments a test marks.
struct Naming(Vec<&'static str>);

impl Marked for Naming {
    fn instruments(&self) -> BTreeSet<InstrumentId> {
        self.0
            .iter()
            .filter_map(|name| InstrumentId::new(name).ok())
            .collect()
    }
}

/// A feed that answers a sane quote at 150 by 150.02 for each instrument it names, observed at the
/// instant asked.
struct Quoting(Vec<&'static str>);

impl QuoteFeed for Quoting {
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        if !self.0.contains(&instrument.as_str()) {
            return Err("no_quote");
        }
        Ok(MarketObservation {
            instrument: instrument.clone(),
            bid: Price::parse("150").ok(),
            bid_size: Qty::parse("2").ok(),
            ask: Price::parse("150.02").ok(),
            last_trade: None,
            mark: None,
            sane: true,
            observed_at: RiskClock::from_secs(now.secs()),
        })
    }

    fn feed(&self) -> &'static str {
        "fixture"
    }
}

/// The account's journal after its process, marking `named` from a feed quoting `quoted`, has run
/// one pass at each of the scheduler's ticks in `ticks` (the first is noon's).
fn marked(
    named: Vec<&'static str>,
    quoted: Vec<&'static str>,
    ticks: &[&str],
) -> Result<Journal, Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Quoting(quoted));
    wiring.marked = Box::new(Naming(named));
    let mut process = start_wired(&journal, &broker, wiring)?;
    for (index, clock) in ticks.iter().enumerate() {
        if index > 0 {
            tick(&journal, &format!("t{}", index + 1), clock)?;
        }
        process.step(at(clock)?);
    }
    Ok(journal)
}

/// The account stream's marks: each one's schema version and payload.
fn marks_of(journal: &Journal) -> Vec<(u64, Value)> {
    journal
        .rows_of(ACCOUNT_STREAM)
        .into_iter()
        .filter(|row| row.event_type == "MarkUpdated")
        .filter_map(|row| {
            let body = mandate_canon::parse(&row.body).ok()?;
            Some((row.schema_version, body.get("payload")?.clone()))
        })
        .collect()
}

fn member(payload: &Value, name: &str) -> Option<String> {
    match payload.get(name)? {
        Value::Str(text) => Some(text.clone()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Null => Some("null".to_owned()),
        _ => None,
    }
}

/// Noon's pass marks AAPL from its quote: a sane version-2 mark at noon's tick, in the regular
/// session, from the fixture feed, at its bid.
#[test]
fn a_named_instrument_is_marked_at_the_tick() -> Result<(), Error> {
    let journal = marked(vec![AAPL], vec![AAPL], &[NOON])?;
    let marks = marks_of(&journal);
    assert_eq!(marks.len(), 1, "{marks:?}");
    let (version, payload) = &marks[0];
    assert_eq!(*version, 2);
    for (name, want) in [
        ("instrument_id", AAPL),
        ("price", "150"),
        ("bid", "150"),
        ("sane", "true"),
        ("session", "regular"),
        ("source", "quote"),
        ("feed", "fixture"),
        ("risk_clock", "2026-09-21T15:00:00.000000000Z"),
    ] {
        assert_eq!(member(payload, name).as_deref(), Some(want), "{name}");
    }
    Ok(())
}

/// A pass within [`marks::MARK_EVERY`] of the last mark writes none; one at that interval or later
/// marks again, at its own tick.
#[test]
fn an_instrument_is_marked_at_most_once_an_interval() -> Result<(), Error> {
    assert_eq!(marks::MARK_EVERY.as_secs(), 15);
    let journal = marked(
        vec![AAPL],
        vec![AAPL],
        &[
            NOON,
            "2026-09-21T15:00:05Z",
            "2026-09-21T15:00:14Z",
            "2026-09-21T15:00:15Z",
        ],
    )?;
    let clocks: Vec<Option<String>> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| member(payload, "risk_clock"))
        .collect();
    assert_eq!(
        clocks,
        vec![
            Some("2026-09-21T15:00:00.000000000Z".to_owned()),
            Some("2026-09-21T15:00:15.000000000Z".to_owned()),
        ]
    );
    Ok(())
}

/// An instrument the feed has no quote for, or the snapshot no asset class for, is not marked;
/// the others still are.
#[test]
fn an_instrument_without_a_quote_or_a_class_is_not_marked() -> Result<(), Error> {
    let journal = marked(vec![AAPL, "MSFT", "TSLA"], vec![AAPL, "TSLA"], &[NOON])?;
    let instruments: Vec<Option<String>> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| member(payload, "instrument_id"))
        .collect();
    assert_eq!(instruments, vec![Some(AAPL.to_owned())]);
    Ok(())
}

/// A wiring that names no instrument marks nothing, whatever its feed quotes.
#[test]
fn a_wiring_naming_no_instrument_marks_nothing() -> Result<(), Error> {
    let journal = marked(Vec::new(), vec![AAPL], &[NOON, "2026-09-21T15:00:30Z"])?;
    assert!(marks_of(&journal).is_empty());
    Ok(())
}

/// A mark whose append fails holds the pass and raises the journal's one opaque alert for the
/// outage, none while it goes on; once appends succeed the instrument is marked again.
#[test]
fn a_mark_in_doubt_raises_one_alert_an_outage() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Quoting(vec![AAPL]));
    wiring.marked = Box::new(Naming(vec![AAPL]));
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    journal.fail_next(&[Fault::Unavailable; 2]);
    let mut alerts = Vec::new();
    for (index, clock) in [
        "2026-09-21T15:00:15Z",
        "2026-09-21T15:00:30Z",
        "2026-09-21T15:00:45Z",
    ]
    .iter()
    .enumerate()
    {
        tick(&journal, &format!("d{index}"), clock)?;
        let raised = process
            .step(at(clock)?)
            .iter()
            .filter(|alert| alert.key == "journal_unavailable" && alert.subject.is_none())
            .count();
        alerts.push(raised);
    }
    assert_eq!(alerts, vec![1, 0, 0]);
    let clocks: Vec<Option<String>> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| member(payload, "risk_clock"))
        .collect();
    assert_eq!(
        clocks,
        vec![
            Some("2026-09-21T15:00:00.000000000Z".to_owned()),
            Some("2026-09-21T15:00:45.000000000Z".to_owned()),
        ]
    );
    Ok(())
}
