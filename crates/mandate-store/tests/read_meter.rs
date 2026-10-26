//! The meter stream read without its writer epoch (DEC-2397 item 4): the runtime reads the
//! model-gateway service's stream for its own checks and never fences the service. What the writer
//! wrote reads back as itself; opening and reading it takes no epoch, so the writer's next append
//! still commits; every append is refused and writes nothing; and a stream it cannot read fails
//! closed. The oracle is the writer's own records and the journal's rows.

use std::rc::Rc;

use mandate_canon::Digest;
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::meter::{MeterRecord, MeterStore, Reservation, Settlement, StoreError};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::provider::SequentialIds;
use mandate_gateway::registry::ModelRef;
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_store::meter::{JournalMeter, ReadMeter};
use mandate_store::{MemoryStore, Store};
use mandate_time::{Date, UtcNanos};

fn writer(store: MemoryStore) -> JournalMeter<MemoryStore> {
    JournalMeter::open(
        store,
        "ws_1",
        Environment::Paper,
        Rc::new(|| UtcNanos::from_parts(1_792_155_600, 0).unwrap()),
        Box::new(SequentialIds::starting_at(1 << 70)),
    )
}

fn reservation(n: u128) -> Reservation {
    Reservation {
        event_id: mandate_canon::encode_ulid(n),
        call_id: mandate_canon::encode_ulid(n + 1),
        agent_id: Some("agent-a".to_owned()),
        purpose: Purpose::Research,
        model: ModelRef {
            model_id: "llm.research_agent".to_owned(),
            model_version: "0.1.0".to_owned(),
            content_hash: Digest::of(b"model"),
        },
        endpoint: "openrouter".to_owned(),
        key_owner: KeyOwner::Platform,
        price_table_ref: Digest::of(b"prices"),
        max_cost: Usd6::from_micros(500),
        risk_day: Date::parse("2026-10-16").unwrap(),
    }
}

fn settlement(n: u128) -> Settlement {
    Settlement {
        event_id: mandate_canon::encode_ulid(n + 2),
        call_id: mandate_canon::encode_ulid(n + 1),
        reservation: mandate_canon::encode_ulid(n),
        outcome: Outcome::Ok,
        attempts: 1,
        tokens: Usage {
            input: 100,
            output: 20,
            cached: 0,
        },
        cost: Usd6::from_micros(300),
    }
}

/// What the writer wrote reads back through the reader as itself, in stream order.
#[test]
#[ignore = "pending E21-62"]
fn what_the_writer_wrote_reads_back() {
    let store = MemoryStore::default();
    let mut gateway = writer(store.clone());
    let reader = ReadMeter::open(store.clone(), "ws_1");
    assert_eq!(reader.read(), Ok(vec![]));
    let written = [
        MeterRecord::Reserved(reservation(10)),
        MeterRecord::Settled(settlement(10)),
    ];
    for record in &written {
        gateway.append(record.clone()).unwrap();
    }
    assert_eq!(reader.read(), Ok(written.to_vec()));
    assert_eq!(gateway.read(), reader.read());
}

/// DEC-2397 item 4: a reader opened and read between the writer's appends takes no epoch, so the
/// writer is never fenced.
#[test]
#[ignore = "pending E21-62"]
fn reading_never_fences_the_writer() {
    let store = MemoryStore::default();
    let mut gateway = writer(store.clone());
    gateway
        .append(MeterRecord::Reserved(reservation(10)))
        .unwrap();
    let reader = ReadMeter::open(store.clone(), "ws_1");
    let _ = reader.read().unwrap();
    assert_eq!(
        gateway.append(MeterRecord::Settled(settlement(10))),
        Ok(()),
        "the writer still holds the stream"
    );
}

/// Every append through the reader is refused and writes nothing.
#[test]
#[ignore = "pending E21-62"]
fn every_append_is_refused_and_writes_nothing() {
    let store = MemoryStore::default();
    let mut reader = ReadMeter::open(store.clone(), "ws_1");
    assert_eq!(
        reader.append(MeterRecord::Reserved(reservation(10))),
        Err(StoreError::Unavailable)
    );
    let stream = StreamId::parse("meter:ws_1").unwrap();
    assert_eq!(store.rows(&stream), Some(vec![]));
}

/// A journal that cannot answer.
#[derive(Clone)]
struct Down;

impl Store for Down {
    fn take_ownership(&mut self, _: &StreamId) -> Option<u64> {
        None
    }

    fn rows(&self, _: &StreamId) -> Option<Vec<StoredEvent>> {
        None
    }

    fn append(
        &mut self,
        _: &StreamId,
        _: u64,
        _: u64,
        _: UtcNanos,
        _: &[Vec<u8>],
    ) -> AppendOutcome {
        AppendOutcome::Unavailable
    }
}

/// A journal that cannot answer, or a workspace that names no stream, reads `Unavailable`.
#[test]
#[ignore = "pending E21-62"]
fn a_stream_it_cannot_read_fails_closed() {
    assert_eq!(
        ReadMeter::open(Down, "ws_1").read(),
        Err(StoreError::Unavailable)
    );
    assert_eq!(
        ReadMeter::open(MemoryStore::default(), "").read(),
        Err(StoreError::Unavailable)
    );
}
