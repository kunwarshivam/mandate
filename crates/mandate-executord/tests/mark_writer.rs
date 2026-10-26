//! DEC-2380 item 3: the executor process marks each instrument its wiring names. At each pass
//! that reads a tick, an instrument due a mark whose quote the feed hands is journaled on the
//! account stream as a `MarkUpdated` version 2 at that tick, at most once every
//! [`marks::MARK_EVERY`]; an instrument the feed has no quote for, or the snapshot no asset class
//! for, is not marked, and a wiring that names no instrument marks nothing.

mod support;

use std::collections::BTreeSet;

use mandate_accounting::{AssetClass, InstrumentId};
use mandate_canon::Value;
use mandate_executor::{ExitTier, InstrumentSnapshot, MarketObservation, RiskClock};
use mandate_executord::marks::{self, Marked};
use mandate_executord::quotes::QuoteFeed;
use mandate_num::{Price, Qty, ShareIncrement};
use mandate_process::State;
use mandate_time::UtcNanos;
use support::{
    AAPL, ACCOUNT_STREAM, AGENT, Allowing, Error, Fault, Journal, NOON, OPENED, Paper,
    TwoPercentStop, at, deployed, propose_market_exit, start_wired, tick, wired,
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

/// A feed whose every quote is crossed, 150.2 by 150, and judged not sane.
struct Crossed;

impl QuoteFeed for Crossed {
    fn observe(
        &self,
        instrument: &InstrumentId,
        now: UtcNanos,
    ) -> Result<MarketObservation, &'static str> {
        Ok(MarketObservation {
            instrument: instrument.clone(),
            bid: Price::parse("150.2").ok(),
            bid_size: Qty::parse("2").ok(),
            ask: Price::parse("150").ok(),
            last_trade: None,
            mark: None,
            sane: false,
            observed_at: RiskClock::from_secs(now.secs()),
        })
    }

    fn feed(&self) -> &'static str {
        "fixture"
    }
}

/// The support snapshot's AAPL, and BTCUSD as a crypto pair.
struct WithCrypto;

impl InstrumentSnapshot for WithCrypto {
    fn asset_class(&self, instrument: &InstrumentId) -> Option<AssetClass> {
        match instrument.as_str() {
            AAPL => Some(AssetClass::UsEquity),
            BTC => Some(AssetClass::Crypto),
            _ => None,
        }
    }

    fn increment(&self, _instrument: &InstrumentId) -> Option<ShareIncrement> {
        None
    }

    fn exit_tier(&self, _instrument: &InstrumentId) -> Option<ExitTier> {
        None
    }
}

const BTC: &str = "BTCUSD";

/// The agent's risk exit.
const EXIT: &str = "01JINTENT0000000000000000C";

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
/// outage, none while it goes on; the mark is kept and offered again, unchanged, first on each pass,
/// so once appends succeed it commits at its own tick, and the instrument is marked again when due.
/// Each pass's tick is handed before its marks, so a mark going in doubt never holds that tick.
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
    journal.fail_next_of("MarkUpdated", &[Fault::Unavailable; 2]);
    let mut alerts = Vec::new();
    let mut clocks_handed = Vec::new();
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
        clocks_handed.push(process.state().risk_clock());
    }
    assert_eq!(alerts, vec![1, 0, 0]);
    let noon = at(NOON)?.secs();
    assert_eq!(
        clocks_handed,
        [15, 15, 45]
            .iter()
            .map(|later| Some(RiskClock::from_secs(noon + later)))
            .collect::<Vec<_>>(),
        "the pass whose mark goes in doubt has handed its tick first (`AGENTS.md` rule 13); a pass \
         whose retried mark is still in doubt appends nothing after it, as with any input in doubt"
    );
    let clocks: Vec<Option<String>> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| member(payload, "risk_clock"))
        .collect();
    assert_eq!(
        clocks,
        vec![
            Some("2026-09-21T15:00:00.000000000Z".to_owned()),
            Some("2026-09-21T15:00:15.000000000Z".to_owned()),
            Some("2026-09-21T15:00:45.000000000Z".to_owned()),
        ]
    );
    Ok(())
}

/// An append whose answer is lost after it committed does not stop the process: the next pass
/// offers the journal the same mark again, which it already holds, before anything else, and goes
/// on to mark at its own tick; each mark is journaled once.
#[test]
fn a_mark_committed_with_its_answer_lost_is_resent_unchanged() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Quoting(vec![AAPL]));
    wiring.marked = Box::new(Naming(vec![AAPL]));
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    journal.fail_next_of("MarkUpdated", &[Fault::LostAnswer]);
    let mut alerts = Vec::new();
    for (index, clock) in [
        "2026-09-21T15:00:15Z",
        "2026-09-21T15:00:30Z",
        "2026-09-21T15:00:45Z",
    ]
    .iter()
    .enumerate()
    {
        tick(&journal, &format!("a{index}"), clock)?;
        alerts.push(
            process
                .step(at(clock)?)
                .iter()
                .filter(|alert| alert.key == "journal_unavailable")
                .count(),
        );
    }
    assert_eq!(alerts, vec![1, 0, 0]);
    assert!(
        !matches!(process.lifecycle().state(), State::Exited(_)),
        "an ambiguous mark never stops the process: {:?}",
        process.lifecycle().state()
    );
    let clocks: Vec<Option<String>> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| member(payload, "risk_clock"))
        .collect();
    assert_eq!(
        clocks,
        vec![
            Some("2026-09-21T15:00:00.000000000Z".to_owned()),
            Some("2026-09-21T15:00:15.000000000Z".to_owned()),
            Some("2026-09-21T15:00:30.000000000Z".to_owned()),
            Some("2026-09-21T15:00:45.000000000Z".to_owned()),
        ]
    );
    Ok(())
}

/// No tick, no mark: passes whose wall clock moves on past [`marks::MARK_EVERY`] with no new
/// `ClockAdvanced` write none, and a mark taken at a later tick is stamped with that tick's clock,
/// not with the wall clock of the pass that read it.
#[test]
fn a_pass_without_a_new_tick_marks_nothing() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Quoting(vec![AAPL]));
    wiring.marked = Box::new(Naming(vec![AAPL]));
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    for wall in [
        "2026-09-21T15:00:20Z",
        "2026-09-21T15:00:40Z",
        "2026-09-21T15:01:00Z",
    ] {
        process.step(at(wall)?);
    }
    assert_eq!(marks_of(&journal).len(), 1, "no tick, no mark");
    tick(&journal, "t2", "2026-09-21T15:01:15Z")?;
    process.step(at("2026-09-21T15:01:42Z")?);
    let clocks: Vec<Option<String>> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| member(payload, "risk_clock"))
        .collect();
    assert_eq!(
        clocks,
        vec![
            Some("2026-09-21T15:00:00.000000000Z".to_owned()),
            Some("2026-09-21T15:01:15.000000000Z".to_owned()),
        ]
    );
    Ok(())
}

/// One pass marks each named instrument once, each with its own session: an equity in the regular
/// session and a crypto pair in `crypto`.
#[test]
fn one_pass_marks_each_instrument_once() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Quoting(vec![AAPL, BTC]));
    wiring.marked = Box::new(Naming(vec![AAPL, BTC]));
    wiring.instruments = Box::new(WithCrypto);
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    let marked: Vec<(Option<String>, Option<String>)> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| (member(payload, "instrument_id"), member(payload, "session")))
        .collect();
    assert_eq!(
        marked,
        vec![
            (Some(AAPL.to_owned()), Some("regular".to_owned())),
            (Some(BTC.to_owned()), Some("crypto".to_owned())),
        ]
    );
    Ok(())
}

/// A quote the feed judges not sane still marks the instrument, `sane` false at its bid, so a held
/// position is restricted at once (journal spec §9.30, DEC-2380 item 3).
#[test]
fn a_quote_judged_not_sane_marks_the_instrument_not_sane() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Crossed);
    wiring.marked = Box::new(Naming(vec![AAPL]));
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    let marks = marks_of(&journal);
    assert_eq!(marks.len(), 1, "{marks:?}");
    let (_, payload) = &marks[0];
    assert_eq!(member(payload, "sane").as_deref(), Some("false"));
    assert_eq!(member(payload, "bid").as_deref(), Some("150.2"));
    Ok(())
}

/// A mark whose append goes in doubt never holds the pass's proposals: the agent's risk exit,
/// proposed before a tick whose mark goes in doubt, is received in that same pass
/// (`AGENTS.md` rule 13).
#[test]
fn a_mark_in_doubt_never_holds_an_exit_in_its_pass() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Quoting(vec![AAPL]));
    wiring.marked = Box::new(Naming(vec![AAPL]));
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    let received = |journal: &Journal| journal.received().iter().any(|intent| intent == EXIT);
    assert!(!received(&journal));
    propose_market_exit(&journal, AGENT, EXIT, "risk_exit", "decision")?;
    journal.fail_next_of("MarkUpdated", &[Fault::Unavailable]);
    let later = "2026-09-21T15:00:15Z";
    tick(&journal, "e1", later)?;
    let alerts = process.step(at(later)?);
    assert!(
        alerts
            .iter()
            .any(|alert| alert.key == "journal_unavailable"),
        "the case builds a mark in doubt: {alerts:?}"
    );
    assert!(received(&journal), "the exit was taken in the same pass");
    Ok(())
}

/// The instruments after one whose mark went in doubt were not marked, and are due again at
/// once: with AAPL's mark in doubt at 15:00:15, BTCUSD is marked at the next tick, a second later,
/// not a full interval after.
#[test]
fn instruments_left_unmarked_by_a_stopped_pass_stay_due() -> Result<(), Error> {
    let journal = deployed()?;
    journal.seed(ACCOUNT_STREAM, OPENED)?;
    let broker = Paper::new(&journal);
    let mut wiring = wired(Box::new(Allowing), Box::new(TwoPercentStop))?;
    wiring.quotes = Box::new(Quoting(vec![AAPL, BTC]));
    wiring.marked = Box::new(Naming(vec![AAPL, BTC]));
    wiring.instruments = Box::new(WithCrypto);
    let mut process = start_wired(&journal, &broker, wiring)?;
    process.step(at(NOON)?);
    journal.fail_next_of("MarkUpdated", &[Fault::Unavailable]);
    for (id, clock) in [
        ("u1", "2026-09-21T15:00:15Z"),
        ("u2", "2026-09-21T15:00:16Z"),
    ] {
        tick(&journal, id, clock)?;
        process.step(at(clock)?);
    }
    let marked: Vec<(Option<String>, Option<String>)> = marks_of(&journal)
        .iter()
        .map(|(_, payload)| {
            (
                member(payload, "instrument_id"),
                member(payload, "risk_clock"),
            )
        })
        .collect();
    let mark =
        |instrument: &str, clock: &str| (Some(instrument.to_owned()), Some(clock.to_owned()));
    assert_eq!(
        marked,
        vec![
            mark(AAPL, "2026-09-21T15:00:00.000000000Z"),
            mark(BTC, "2026-09-21T15:00:00.000000000Z"),
            mark(AAPL, "2026-09-21T15:00:15.000000000Z"),
            mark(BTC, "2026-09-21T15:00:16.000000000Z"),
        ]
    );
    Ok(())
}
