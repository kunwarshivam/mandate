//! The journaled meter stream (DEC-2258, E21-61) over the in-memory journal, which validates every
//! draft against journal spec §9.19: what it writes reads back, a restart keeps the caps with a
//! reservation still open, a newer writer fences an older one, and anything it cannot read fails
//! closed.

use std::rc::Rc;

use mandate_canon::{Digest, parse};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::meter::{
    Caps, DeploymentMeter, MemoryMeter, Meter, MeterRecord, MeterRefusal, MeterStore, Reservation,
    Settlement, StoreError,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::provider::SequentialIds;
use mandate_gateway::registry::ModelRef;
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_store::meter::JournalMeter;
use mandate_store::{MemoryStore, Store};
use mandate_time::{Date, UtcNanos};

/// 2026-10-16T13:00:00Z.
const T: i64 = 1_792_155_600;

fn now() -> Rc<dyn Fn() -> UtcNanos> {
    Rc::new(|| UtcNanos::from_parts(T, 0).unwrap())
}

fn meter_stream() -> StreamId {
    StreamId::parse("meter:ws_1").unwrap()
}

fn open<S: Store>(store: S, ids_from: u128) -> JournalMeter<S> {
    JournalMeter::open(
        store,
        "ws_1",
        Environment::Paper,
        now(),
        Box::new(SequentialIds::starting_at(ids_from)),
    )
}

fn day() -> Date {
    Date::parse("2026-10-16").unwrap()
}

fn reservation(n: u128, max_micros: u64) -> Reservation {
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
        max_cost: Usd6::from_micros(max_micros),
        risk_day: day(),
    }
}

fn settlement(n: u128, cost_micros: u64) -> Settlement {
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
        cost: Usd6::from_micros(cost_micros),
    }
}

fn event_types(store: &MemoryStore) -> Vec<String> {
    store
        .rows(&meter_stream())
        .unwrap()
        .into_iter()
        .map(|row| row.event_type)
        .collect()
}

/// DEC-2258 items 1 to 3: the first record opens the stream in the same batch, every record is a
/// draft at its own event id that §9.19 accepts, written by a `system` actor, and each reads back,
/// in stream order, as itself.
#[test]
fn what_it_writes_reads_back_and_the_first_record_opens_the_stream() {
    let store = MemoryStore::default();
    let mut meter = open(store.clone(), 1 << 70);
    assert_eq!(meter.read(), Ok(Vec::new()));
    let reserved = MeterRecord::Reserved(reservation(10, 250_000));
    meter.append(reserved.clone()).unwrap();
    assert_eq!(meter.read(), Ok(vec![reserved.clone()]));
    assert_eq!(event_types(&store), ["StreamOpened", "MeterReserved"]);
    let settled = MeterRecord::Settled(settlement(10, 120_000));
    meter.append(settled.clone()).unwrap();
    assert_eq!(meter.read(), Ok(vec![reserved, settled.clone()]));
    let rows = store.rows(&meter_stream()).unwrap();
    assert_eq!(
        rows.iter()
            .map(|row| row.event_id.as_str())
            .collect::<Vec<_>>(),
        [
            mandate_canon::encode_ulid(1 << 70).as_str(),
            mandate_canon::encode_ulid(10).as_str(),
            settled.event_id()
        ]
    );
    for row in &rows {
        let body = parse(&row.body).unwrap();
        assert_eq!(
            body.get("actor")
                .and_then(|a| a.get("kind"))
                .and_then(|k| k.as_str()),
            Some("system"),
            "{}",
            row.event_type
        );
    }
    let opened = parse(&rows[0].body).unwrap();
    assert_eq!(
        opened
            .get("payload")
            .and_then(|p| p.get("workspace_id"))
            .and_then(|w| w.as_str()),
        Some("ws_1")
    );
}

/// DEC-2258, E21-61's acceptance: a gateway restarted over the same journal folds the reservation
/// its predecessor left open at its maximum, so the day's research cap holds across the restart.
#[test]
fn a_restart_keeps_the_cap_with_a_reservation_still_open() {
    let store = MemoryStore::default();
    let caps = || {
        Caps::new(Usd6::from_micros(50_000_000))
            .with_research_daily("agent-a", Usd6::from_micros(1_000_000))
            .unwrap()
    };
    let deployment = || {
        DeploymentMeter::new(
            Box::new(MemoryMeter::new()),
            Usd6::from_micros(50_000_000),
            Box::new(SequentialIds::starting_at(1 << 90)),
        )
    };
    let mut before = Meter::new(open(store.clone(), 1 << 70), caps(), deployment());
    before.reserve(reservation(10, 600_000)).unwrap();
    drop(before);

    let mut after = Meter::new(open(store.clone(), 1 << 71), caps(), deployment());
    assert_eq!(
        after.reserve(reservation(20, 600_000)),
        Err(MeterRefusal::BudgetExhausted),
        "the open reservation still counts at its maximum"
    );
    assert!(after.reserve(reservation(30, 400_000)).is_ok());
    assert_eq!(
        event_types(&store),
        ["StreamOpened", "MeterReserved", "MeterReserved"],
        "the restarted writer opens nothing again and writes only what it admitted"
    );
}

/// DEC-2258 item 1: a newer writer fences an older one, whose appends then fail `Fenced` and leave
/// the stream as the newer writer wrote it.
#[test]
fn a_newer_writer_fences_an_older_one() {
    let store = MemoryStore::default();
    let mut older = open(store.clone(), 1 << 70);
    older
        .append(MeterRecord::Reserved(reservation(10, 1)))
        .unwrap();
    let mut newer = open(store.clone(), 1 << 71);
    assert_eq!(
        older.append(MeterRecord::Settled(settlement(10, 1))),
        Err(StoreError::Fenced)
    );
    newer
        .append(MeterRecord::Settled(settlement(10, 1)))
        .unwrap();
    assert_eq!(
        event_types(&store),
        ["StreamOpened", "MeterReserved", "MeterSettled"]
    );
}

/// DEC-2258 item 5: rule 158 is checked before anything is written: a second reservation of one
/// call, a settlement of no reservation, and a second settlement are refused and write nothing.
#[test]
fn rule_158_refuses_before_anything_is_written() {
    let store = MemoryStore::default();
    let mut meter = open(store.clone(), 1 << 70);
    assert!(matches!(
        meter.append(MeterRecord::Settled(settlement(10, 1))),
        Err(StoreError::Rule158(_))
    ));
    assert_eq!(store.rows(&meter_stream()), Some(Vec::new()));
    meter
        .append(MeterRecord::Reserved(reservation(10, 5)))
        .unwrap();
    let mut again = reservation(10, 5);
    again.event_id = mandate_canon::encode_ulid(99);
    assert!(matches!(
        meter.append(MeterRecord::Reserved(again)),
        Err(StoreError::Rule158(_))
    ));
    meter
        .append(MeterRecord::Settled(settlement(10, 3)))
        .unwrap();
    let mut twice = settlement(10, 3);
    twice.event_id = mandate_canon::encode_ulid(98);
    assert!(matches!(
        meter.append(MeterRecord::Settled(twice)),
        Err(StoreError::Rule158(_))
    ));
    assert_eq!(
        event_types(&store),
        ["StreamOpened", "MeterReserved", "MeterSettled"]
    );
}

/// A store over a shared in-memory journal that can stop answering, grant no epoch, or hand back
/// a meter record whose payload no longer reads.
#[derive(Clone, Default)]
struct Faulty {
    inner: MemoryStore,
    down: Rc<std::cell::Cell<bool>>,
    no_epoch: Rc<std::cell::Cell<bool>>,
    garbled: Rc<std::cell::Cell<bool>>,
}

impl Store for Faulty {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        if self.no_epoch.get() {
            return None;
        }
        self.inner.take_ownership(stream)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if self.down.get() {
            return None;
        }
        let mut rows = self.inner.rows(stream)?;
        if self.garbled.get() {
            for row in &mut rows {
                if row.event_type == "MeterReserved" {
                    row.body = String::from_utf8(row.body.clone())
                        .unwrap()
                        .replace("\"max_cost_usd\"", "\"max_cost\"")
                        .into_bytes();
                }
            }
        }
        Some(rows)
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        if self.down.get() {
            return AppendOutcome::Unavailable;
        }
        self.inner
            .append(stream, expected_head, epoch, recorded_at, drafts)
    }
}

/// DEC-2258 items 4 and 6: a stream the store cannot read, a meter record it cannot read back, a
/// writer the store granted no epoch, and an append the store cannot confirm all fail
/// `Unavailable`, so the gateway refuses `meter_unavailable` and sends nothing.
#[test]
fn anything_it_cannot_read_or_confirm_fails_closed() {
    let store = Faulty::default();
    let mut meter = open(store.clone(), 1 << 70);
    meter
        .append(MeterRecord::Reserved(reservation(10, 5)))
        .unwrap();

    store.garbled.set(true);
    assert_eq!(
        meter.read(),
        Err(StoreError::Unavailable),
        "a garbled record"
    );
    assert_eq!(
        meter.append(MeterRecord::Settled(settlement(10, 1))),
        Err(StoreError::Unavailable)
    );
    store.garbled.set(false);

    store.down.set(true);
    assert_eq!(
        meter.read(),
        Err(StoreError::Unavailable),
        "a store that cannot answer"
    );
    assert_eq!(
        meter.append(MeterRecord::Settled(settlement(10, 1))),
        Err(StoreError::Unavailable)
    );
    store.down.set(false);

    let ungranted = Faulty::default();
    ungranted.no_epoch.set(true);
    let mut meter = open(ungranted.clone(), 1 << 70);
    assert_eq!(meter.read(), Err(StoreError::Unavailable), "no epoch");
    assert_eq!(
        meter.append(MeterRecord::Reserved(reservation(10, 5))),
        Err(StoreError::Unavailable)
    );
    assert_eq!(ungranted.inner.rows(&meter_stream()), Some(Vec::new()));

    let mut unnamed = JournalMeter::open(
        MemoryStore::default(),
        "no such workspace!",
        Environment::Paper,
        now(),
        Box::new(SequentialIds::starting_at(1)),
    );
    assert_eq!(unnamed.read(), Err(StoreError::Unavailable), "no stream");
    assert_eq!(
        unnamed.append(MeterRecord::Reserved(reservation(10, 5))),
        Err(StoreError::Unavailable)
    );
}
