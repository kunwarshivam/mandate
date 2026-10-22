//! The deployment's journaled meter stream (journal spec §9.19, DEC-2207, E21-62) over the
//! in-memory journal, which validates every draft: it opens as the deployment's, what it writes
//! reads back, the deployment's cap holds across a restart with a copy still open, a newer writer
//! fences an older one, and a deployment that names no stream fails closed.

use std::rc::Rc;

use mandate_canon::{Digest, parse};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::meter::{
    Caps, DeploymentMeter, Meter, MeterRecord, MeterRefusal, MeterStore, Reservation, Settlement,
    StoreError,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::provider::SequentialIds;
use mandate_gateway::registry::ModelRef;
use mandate_journal::{Environment, StreamId};
use mandate_store::meter::JournalMeter;
use mandate_store::{MemoryStore, Store};
use mandate_time::{Date, UtcNanos};

/// 2026-10-16T13:00:00Z.
const T: i64 = 1_792_155_600;
const DEPLOYMENT: &str = "dep_01J8Z0";

fn now() -> Rc<dyn Fn() -> UtcNanos> {
    Rc::new(|| UtcNanos::from_parts(T, 0).unwrap())
}

fn deployment_stream() -> StreamId {
    StreamId::parse(&format!("dmeter:{DEPLOYMENT}")).unwrap()
}

fn open(store: MemoryStore, ids_from: u128) -> JournalMeter<MemoryStore> {
    JournalMeter::open_deployment(
        store,
        DEPLOYMENT,
        Environment::Paper,
        now(),
        Box::new(SequentialIds::starting_at(ids_from)),
    )
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
        risk_day: Date::parse("2026-10-16").unwrap(),
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

fn event_types(store: &MemoryStore, stream: &StreamId) -> Vec<String> {
    store
        .rows(stream)
        .unwrap()
        .into_iter()
        .map(|row| row.event_type)
        .collect()
}

/// Journal spec §9.19: the deployment's stream is `dmeter:{deployment_id}`, opened in the first
/// record's batch with `{stream_type: "deployment_meter", deployment_id}`, and each record the
/// journal accepts reads back as itself, in order.
#[test]
fn the_deployment_s_stream_opens_as_its_own_and_reads_back() {
    let store = MemoryStore::default();
    let mut meter = open(store.clone(), 1 << 70);
    assert_eq!(meter.read(), Ok(Vec::new()));
    let reserved = MeterRecord::Reserved(reservation(10, 250_000));
    let settled = MeterRecord::Settled(settlement(10, 120_000));
    meter.append(reserved.clone()).unwrap();
    meter.append(settled.clone()).unwrap();
    assert_eq!(meter.read(), Ok(vec![reserved, settled]));
    assert_eq!(
        event_types(&store, &deployment_stream()),
        ["StreamOpened", "MeterReserved", "MeterSettled"]
    );
    let rows = store.rows(&deployment_stream()).unwrap();
    let opened = parse(&rows[0].body).unwrap();
    let payload = opened.get("payload").unwrap();
    assert_eq!(
        payload.get("stream_type").and_then(|v| v.as_str()),
        Some("deployment_meter")
    );
    assert_eq!(
        payload.get("deployment_id").and_then(|v| v.as_str()),
        Some(DEPLOYMENT)
    );
    assert_eq!(payload.get("workspace_id"), None, "no workspace owns it");
    assert_eq!(
        rows[0].event_id,
        mandate_canon::encode_ulid(1 << 70),
        "the opening record takes the writer's own id"
    );
}

/// DEC-2207, E21-62: the deployment's cap holds across a restart of the meter writers, with the
/// copy of an open reservation still counted at its maximum, and the workspace's stream and the
/// deployment's are each written once per admitted call.
#[test]
fn the_deployment_s_cap_holds_across_a_restart() {
    let store = MemoryStore::default();
    let meter = |workspace_ids: u128, deployment_ids: u128| {
        Meter::new(
            JournalMeter::open(
                store.clone(),
                "ws_1",
                Environment::Paper,
                now(),
                Box::new(SequentialIds::starting_at(workspace_ids)),
            ),
            Caps::new(Usd6::from_micros(50_000_000)),
            DeploymentMeter::new(
                Box::new(open(store.clone(), deployment_ids)),
                Usd6::from_micros(1_000_000),
                Box::new(SequentialIds::starting_at(deployment_ids + 1)),
            ),
        )
    };
    let mut before = meter(1 << 70, 1 << 80);
    before.reserve(reservation(10, 600_000)).unwrap();
    drop(before);

    let mut after = meter(1 << 71, 1 << 81);
    assert_eq!(
        after.reserve(reservation(20, 600_000)),
        Err(MeterRefusal::BudgetExhausted),
        "the open copy still counts at its maximum against the deployment's cap"
    );
    assert!(after.reserve(reservation(30, 400_000)).is_ok());
    let workspace = StreamId::parse("meter:ws_1").unwrap();
    assert_eq!(
        event_types(&store, &workspace),
        ["StreamOpened", "MeterReserved", "MeterReserved"]
    );
    assert_eq!(
        event_types(&store, &deployment_stream()),
        ["StreamOpened", "MeterReserved", "MeterReserved"],
        "the restarted writer opens nothing again and copies only what it admitted"
    );
}

/// DEC-2258 item 1 on the deployment's stream: a newer writer fences an older one, whose appends
/// then fail `Fenced` and write nothing.
#[test]
fn a_newer_deployment_writer_fences_an_older_one() {
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
        event_types(&store, &deployment_stream()),
        ["StreamOpened", "MeterReserved", "MeterSettled"]
    );
}

/// DEC-2258 item 4: a deployment id that names no stream gives a meter whose every read and append
/// fails `Unavailable`, writing nothing.
#[test]
fn a_deployment_that_names_no_stream_fails_closed() {
    let mut unnamed = JournalMeter::open_deployment(
        MemoryStore::default(),
        "no such deployment!",
        Environment::Paper,
        now(),
        Box::new(SequentialIds::starting_at(1)),
    );
    assert_eq!(unnamed.read(), Err(StoreError::Unavailable));
    assert_eq!(
        unnamed.append(MeterRecord::Reserved(reservation(10, 5))),
        Err(StoreError::Unavailable)
    );
}
