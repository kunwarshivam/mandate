//! The cell's meter store (DEC-2249 item 3): the agent's research spend from the records the
//! worker's gateway reserves and settles through it, and a store held, so no spend and no write,
//! while the agent stream records a reserved call on the risk day it does not hold, as after a
//! restart. Every expected spend is summed by hand from the records the test lays down.

use std::cell::RefCell;
use std::rc::Rc;

use mandate_canon::Digest;
use mandate_daemon::meter::CellMeter;
use mandate_daemon::source::{Rows, SourceFor};
use mandate_gateway::Outcome;
use mandate_gateway::kinds::{KeyOwner, Purpose};
use mandate_gateway::meter::{
    MeterRecord, MeterStore, Reservation, Settlement, SharedMeter, StoreError,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::provider::SequentialIds;
use mandate_gateway::registry::ModelRef;
use mandate_journal::{Environment, StoredEvent, StreamId};
use mandate_num::Usd;
use mandate_store::MemoryStore;
use mandate_store::meter::JournalMeter;
use mandate_time::{Date, UtcNanos};

const AGENT: &str = "agent-a";
const AGENT_STREAM: &str = "agent:ws1:agent-a";
/// 11:00 in New York on 2026-10-16, whose risk day runs from 04:00Z that day to 04:00Z the next.
const NOW: &str = "2026-10-16T15:00:00Z";
const TODAY: (u16, u8, u8) = (2026, 10, 16);
const YESTERDAY: (u16, u8, u8) = (2026, 10, 15);

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse_rfc3339(text).unwrap()
}

fn day((year, month, day): (u16, u8, u8)) -> Date {
    Date::new(year, month, day).unwrap()
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap()
}

/// The agent stream, which a test can append to or make unreadable.
#[derive(Clone, Default)]
struct Stream {
    rows: Rc<RefCell<Vec<StoredEvent>>>,
    down: Rc<RefCell<bool>>,
}

impl Rows for Stream {
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if *self.down.borrow() {
            return None;
        }
        Some(if stream.as_str() == AGENT_STREAM {
            self.rows.borrow().clone()
        } else {
            Vec::new()
        })
    }
}

impl Stream {
    /// A `ModelInvocationRecorded` at `event_time` whose payload's `reservation` is `reservation`,
    /// written as JSON (a string, `null`, or anything else).
    fn call(&self, reservation: &str, event_time: &str) {
        self.recorded(
            &format!(r#"{{"cost_usd":"0.25","reservation":{reservation}}}"#),
            event_time,
        );
    }

    /// A `ModelInvocationRecorded` at `event_time` with `payload`, written as JSON.
    fn recorded(&self, payload: &str, event_time: &str) {
        let seq = u64::try_from(self.rows.borrow().len()).unwrap() + 1;
        let event_id = format!("01J8Z3MA{seq:018}");
        self.raw(format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{AGENT_STREAM}","event_type":"ModelInvocationRecorded","schema_version":1,"event_time":"{event_time}","clock_source":"system","causation_id":null,"correlation_id":"01RUN","actor":{{"kind":"agent","id":"{AGENT}","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
        ), event_time);
    }

    /// A `ModelInvocationRecorded` whose stored body is `body`, whatever it holds.
    fn raw(&self, body: String, event_time: &str) {
        let seq = u64::try_from(self.rows.borrow().len()).unwrap() + 1;
        let event_id = format!("01J8Z3MA{seq:018}");
        self.rows.borrow_mut().push(StoredEvent {
            stream_id: AGENT_STREAM.into(),
            seq,
            event_id,
            event_type: "ModelInvocationRecorded".into(),
            schema_version: 1,
            environment: "paper".into(),
            recorded_at: event_time.into(),
            prev_hash: Digest::of(b"prev"),
            hash: Digest::of(body.as_bytes()),
            body: body.into_bytes(),
        });
    }
}

/// A clock a test can move.
#[derive(Clone)]
struct Clock(Rc<RefCell<UtcNanos>>);

impl Clock {
    fn at(text: &str) -> Self {
        Self(Rc::new(RefCell::new(at(text))))
    }

    fn set(&self, text: &str) {
        *self.0.borrow_mut() = at(text);
    }

    fn reading(&self) -> Rc<dyn Fn() -> UtcNanos> {
        let clock = Rc::clone(&self.0);
        Rc::new(move || *clock.borrow())
    }
}

fn who() -> SourceFor {
    SourceFor {
        workspace: "ws1".to_owned(),
        agent: AGENT.to_owned(),
        environment: Environment::Paper,
    }
}

fn cell_meter(store: &SharedMeter, stream: &Stream, clock: &Clock) -> CellMeter {
    CellMeter::new(
        store.clone(),
        Rc::new(stream.clone()),
        who(),
        clock.reading(),
    )
}

/// A research reservation `id` of `agent` on `risk_day`, at most `max` dollars.
fn reserved(id: &str, agent: &str, max: &str, risk_day: Date) -> MeterRecord {
    MeterRecord::Reserved(Reservation {
        event_id: id.to_owned(),
        call_id: format!("call-{id}"),
        agent_id: Some(agent.to_owned()),
        purpose: Purpose::Research,
        model: ModelRef {
            model_id: "llm.research_agent".to_owned(),
            model_version: "0.1.0".to_owned(),
            content_hash: Digest::of(b"entry"),
        },
        endpoint: "openrouter:anthropic".to_owned(),
        key_owner: KeyOwner::Platform,
        price_table_ref: Digest::of(b"prices"),
        max_cost: Usd6::parse(max).unwrap(),
        risk_day,
    })
}

/// Reservation `id`'s settlement at `cost` dollars.
fn settled(id: &str, cost: &str) -> MeterRecord {
    MeterRecord::Settled(Settlement {
        event_id: format!("settled-{id}"),
        call_id: format!("call-{id}"),
        reservation: id.to_owned(),
        outcome: Outcome::Ok,
        attempts: 1,
        tokens: Usage {
            input: 100,
            output: 10,
            cached: 0,
        },
        cost: Usd6::parse(cost).unwrap(),
    })
}

/// The day's spend is the agent's own research records on that risk day: an open reservation at
/// its maximum, a settled one at its cost; another agent's and another day's count for nothing.
/// The records are written through the store, as the gateway writes them, and read back as written.
#[test]
fn the_spend_counts_open_reservations_at_most_and_settled_ones_at_cost() {
    let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
    let mut meter = cell_meter(&store, &stream, &clock);
    assert_eq!(meter.research_spend(at(NOW)), Some(usd("0")));
    let records = [
        reserved("R1", AGENT, "0.30", day(TODAY)),
        reserved("R2", AGENT, "0.50", day(TODAY)),
        settled("R2", "0.12"),
        reserved("R3", "agent-b", "1.00", day(TODAY)),
        reserved("R4", AGENT, "0.70", day(YESTERDAY)),
    ];
    for record in records.clone() {
        meter.append(record).unwrap();
    }
    assert_eq!(meter.read().unwrap(), records, "read back as written");
    assert_eq!(store.records(), records, "the shared store holds them");
    assert_eq!(
        meter.research_spend(at(NOW)),
        Some(usd("0.42")),
        "R1 open at 0.30 plus R2 settled at 0.12; R3 is agent-b's and R4 yesterday's"
    );
    assert_eq!(
        meter.research_spend(at("2026-10-15T20:00:00Z")),
        Some(usd("0.7")),
        "yesterday's spend is R4's alone"
    );
}

/// After a restart the store is empty while the agent stream records a reserved call earlier on
/// the risk day: the store is held, so it reads and writes nothing and gives no spend, through
/// 03:59:59Z the next morning, the day's last second in New York; from the next risk day it is
/// free. A store that holds the call's reservation is not held.
#[test]
fn a_reserved_call_today_the_store_does_not_hold_holds_it_until_the_next_risk_day() {
    let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
    stream.call(r#""R1""#, "2026-10-16T14:00:00.000000000Z");
    let mut meter = cell_meter(&store, &stream, &clock);
    assert!(meter.held(at(NOW)));
    assert_eq!(meter.read(), Err(StoreError::Unavailable));
    let record = reserved("R5", AGENT, "0.10", day(TODAY));
    assert_eq!(meter.append(record.clone()), Err(StoreError::Unavailable));
    assert!(store.records().is_empty(), "nothing written while held");
    assert_eq!(meter.research_spend(at(NOW)), None);
    clock.set("2026-10-17T03:59:59Z");
    assert!(meter.held(at("2026-10-17T03:59:59Z")));
    assert_eq!(meter.read(), Err(StoreError::Unavailable));
    clock.set("2026-10-17T04:00:00Z");
    assert!(!meter.held(at("2026-10-17T04:00:00Z")));
    assert_eq!(meter.read(), Ok(Vec::new()));
    meter.append(record.clone()).unwrap();
    assert_eq!(store.records(), [record]);
    assert_eq!(
        meter.research_spend(at("2026-10-17T04:00:00Z")),
        Some(usd("0")),
        "R5 is the 16th's"
    );

    let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
    store
        .clone()
        .append(reserved("R1", AGENT, "0.30", day(TODAY)))
        .unwrap();
    stream.call(r#""R1""#, "2026-10-16T14:00:00.000000000Z");
    let meter = cell_meter(&store, &stream, &clock);
    assert!(!meter.held(at(NOW)), "the store holds R1");
    assert_eq!(meter.research_spend(at(NOW)), Some(usd("0.3")));
}

/// A reserved call on an earlier risk day, and a call no reservation backed, hold nothing: the
/// store writes, reads back, and counts what it is given.
#[test]
fn an_earlier_day_s_reserved_call_and_an_unreserved_call_hold_nothing() {
    let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
    stream.call(r#""R0""#, "2026-10-16T03:59:59.000000000Z");
    stream.call("null", "2026-10-16T14:00:00.000000000Z");
    let mut meter = cell_meter(&store, &stream, &clock);
    assert!(!meter.held(at(NOW)));
    let record = reserved("R6", AGENT, "0.25", day(TODAY));
    meter.append(record.clone()).unwrap();
    assert_eq!(meter.read(), Ok(vec![record]));
    assert_eq!(meter.research_spend(at(NOW)), Some(usd("0.25")));
}

/// A call record whose reservation is neither a string nor `null`, or is missing, or whose time
/// cannot be read, holds the store, as does an agent stream that cannot be read: fail closed.
#[test]
fn a_call_record_or_a_stream_it_cannot_read_holds_the_store() {
    for (reservation, event_time) in [
        ("7", "2026-10-16T14:00:00.000000000Z"),
        (r#"{"id":"R1"}"#, "2026-10-16T14:00:00.000000000Z"),
        (r#""R1""#, "yesterday"),
    ] {
        let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
        stream.call(reservation, event_time);
        let meter = cell_meter(&store, &stream, &clock);
        assert!(meter.held(at(NOW)), "{reservation} at {event_time}");
        assert_eq!(meter.read(), Err(StoreError::Unavailable));
        assert_eq!(meter.research_spend(at(NOW)), None);
    }
    let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
    let meter = cell_meter(&store, &stream, &clock);
    *stream.down.borrow_mut() = true;
    assert!(meter.held(at(NOW)), "an unreadable stream");
    assert_eq!(meter.read(), Err(StoreError::Unavailable));
}

/// A call record whose body is no JSON, or whose payload has no `reservation` member, holds the
/// store as an unreadable reservation does: the meter cannot tell it was unreserved.
#[test]
fn a_call_record_with_no_body_or_no_reservation_member_holds_the_store() {
    let unparseable =
        |stream: &Stream| stream.raw("not json".to_owned(), "2026-10-16T14:00:00.000000000Z");
    let unreserved = |stream: &Stream| {
        stream.recorded(r#"{"cost_usd":"0.25"}"#, "2026-10-16T14:00:00.000000000Z")
    };
    for (case, record) in [
        ("no body", &unparseable as &dyn Fn(&Stream)),
        ("no reservation member", &unreserved),
    ] {
        let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
        record(&stream);
        let meter = cell_meter(&store, &stream, &clock);
        assert!(meter.held(at(NOW)), "{case}");
        assert_eq!(meter.read(), Err(StoreError::Unavailable), "{case}");
        assert_eq!(meter.research_spend(at(NOW)), None, "{case}");
    }
}

/// Every spend the gateway can hold is a `Usd`: one micro-dollar, whole dollars, and the largest
/// `Usd6` each come back as the same amount, so the spend is never refused for its size.
#[test]
fn every_spend_the_gateway_can_hold_converts_exactly() {
    for (max, spent) in [
        ("0.000001", "0.000001"),
        ("7", "7"),
        ("0.25", "0.25"),
        ("18446744073709.551615", "18446744073709.551615"),
    ] {
        let (store, stream, clock) = (SharedMeter::new(), Stream::default(), Clock::at(NOW));
        let mut meter = cell_meter(&store, &stream, &clock);
        meter
            .append(reserved("R9", AGENT, max, day(TODAY)))
            .unwrap();
        assert_eq!(meter.research_spend(at(NOW)), Some(usd(spent)), "{max}");
    }
}

/// A research reservation of the agent today at `max` dollars, its ids ULIDs from `n`, as the
/// journal takes them.
fn journaled_reservation(n: u128, max: &str) -> MeterRecord {
    let MeterRecord::Reserved(mut reservation) = reserved("", AGENT, max, day(TODAY)) else {
        unreachable!("reserved gives a reservation")
    };
    reservation.event_id = mandate_canon::encode_ulid(n);
    reservation.call_id = mandate_canon::encode_ulid(n + 1);
    MeterRecord::Reserved(reservation)
}

/// DEC-2259 items 1 and 2: over the journaled meter stream, a cell meter restarted after a
/// reserved call counts that reservation at its maximum, and the agent stream's record of the call
/// holds nothing. A call record whose reservation the journal lacks still holds the store.
#[test]
fn a_journaled_meter_restarted_counts_its_reservation_and_holds_nothing() {
    let (journal, stream, clock) = (MemoryStore::default(), Stream::default(), Clock::at(NOW));
    let journaled = |from: u128| {
        CellMeter::over(
            Box::new(JournalMeter::open(
                journal.clone(),
                "ws1",
                Environment::Paper,
                clock.reading(),
                Box::new(SequentialIds::starting_at(from)),
            )),
            Rc::new(stream.clone()),
            who(),
            clock.reading(),
        )
    };
    let mut before = journaled(1 << 70);
    let reservation = journaled_reservation(10, "0.25");
    before.append(reservation.clone()).unwrap();
    stream.call(
        &format!(r#""{}""#, reservation.event_id()),
        "2026-10-16T14:00:00.000000000Z",
    );
    drop(before);

    let after = journaled(1 << 71);
    assert!(!after.held(at(NOW)), "the journal holds the reservation");
    assert_eq!(after.read(), Ok(vec![reservation]));
    assert_eq!(after.research_spend(at(NOW)), Some(usd("0.25")));

    stream.call(
        r#""01J8Z3MR000000000000000777""#,
        "2026-10-16T14:30:00.000000000Z",
    );
    assert!(
        after.held(at(NOW)),
        "a reservation the journal lacks still holds"
    );
    assert_eq!(after.research_spend(at(NOW)), None);
}
