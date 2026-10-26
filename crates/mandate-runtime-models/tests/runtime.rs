//! The composer as the runtime process's models port (DEC-2383 items 2 and 7): on a scheduler
//! tick, the process journals the closes' observation and then the pinned model's output on the
//! real in-memory journal, which validates every draft, naming the closes object the data process
//! stored; with the object not held, it journals neither and the process stays ready.

#[allow(
    dead_code,
    reason = "the shared fixtures serve the composer's tests too, which use the rest"
)]
mod common;

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use common::*;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::Digest;
use mandate_daemon::{Config, RuntimeProcess, fixture};
use mandate_journal::{AppendOutcome, ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_process::State;
use mandate_runtime::{AgentId, ConnectionId, Deployment, WorkspaceId};
use mandate_runtime_models::wiring as models;
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

const AGENT_STREAM: &str = "agent:ws1:agent-a";
const CLOCK: &str = "clock:ws1";

/// The agent stream on the in-memory journal, and the scheduler's clock stream as laid-down rows.
#[derive(Clone, Default)]
struct Rig {
    journal: MemoryStore,
    clock: Rc<RefCell<Vec<StoredEvent>>>,
}

impl Store for Rig {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.journal.take_ownership(stream)
    }
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        match stream.as_str() {
            CLOCK => Some(self.clock.borrow().clone()),
            _ => self.journal.rows(stream),
        }
    }
    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.journal
            .append(stream, head, epoch, recorded_at, drafts)
    }
}

/// The scheduler's tick at `clock`.
fn advance(rig: &Rig, clock: &str) {
    let seq = u64::try_from(rig.clock.borrow().len()).unwrap() + 1;
    let event_id = format!("01J8Z3MK{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{CLOCK}","event_type":"ClockAdvanced","schema_version":1,"event_time":"{clock}","clock_source":"scheduler","causation_id":null,"correlation_id":null,"actor":{{"kind":"system","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{{"clock":"{}"}},"artifact_refs":[],"pii_refs":[]}}"#,
        at(clock)
    );
    rig.clock.borrow_mut().push(StoredEvent {
        stream_id: CLOCK.into(),
        seq,
        event_id,
        event_type: "ClockAdvanced".into(),
        schema_version: 1,
        environment: "paper".into(),
        recorded_at: clock.into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    });
}

fn config() -> Config {
    Config {
        deployment: Deployment {
            agent: AgentId("agent-a".into()),
            connection: ConnectionId("conn_demo_paper_01".into()),
            workspace: WorkspaceId("ws1".into()),
        },
        environment: Environment::Paper,
        liveness: Duration::from_secs(10),
    }
}

fn agent_rows(rig: &Rig) -> Vec<StoredEvent> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
}

fn kinds(rig: &Rig) -> Vec<String> {
    agent_rows(rig)
        .iter()
        .map(|row| row.event_type.clone())
        .collect()
}

/// A process on the fixture wiring with the composer's models over `root`, started at `NOW`.
fn started(rig: &Rig, root: &Root) -> RuntimeProcess<Rig> {
    let mut wiring = fixture::wiring("user-owner", AGENT_STREAM);
    wiring.models = models(
        &registered(),
        Environment::Paper,
        &document(),
        root.bars().to_str(),
        FsArtifactStore::open_existing(root.artifacts()).unwrap(),
    );
    assert!(wiring.models.is_some(), "the cell's deployment is wired");
    RuntimeProcess::start(rig.clone(), wiring, &config(), at(NOW))
}

fn body(row: &StoredEvent) -> String {
    String::from_utf8(row.body.clone()).unwrap()
}

#[test]
fn a_tick_journals_the_stored_closes_observation_then_the_pinned_model_s_output() {
    let root = Root::new("runtime");
    let series = rising(30);
    store_series(&root.bars(), "QQQ", &series, None);
    let closes = closes_json(&series);
    FsArtifactStore::open(root.artifacts())
        .unwrap()
        .put_artifact(&closes)
        .unwrap();
    let rig = Rig::default();
    let mut process = started(&rig, &root);
    advance(&rig, NOW);
    process.step(at(NOW));
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    let rows = agent_rows(&rig);
    let observed: Vec<&StoredEvent> = rows
        .iter()
        .filter(|row| row.event_type == "ObservationRecorded")
        .collect();
    let outputs: Vec<&StoredEvent> = rows
        .iter()
        .filter(|row| row.event_type == "ModelOutputRecorded")
        .collect();
    let ([observation], [output]) = (observed.as_slice(), outputs.as_slice()) else {
        panic!("one observation and one output: {:?}", kinds(&rig));
    };
    assert!(observation.seq < output.seq, "{:?}", kinds(&rig));
    let observation = body(observation);
    for member in [
        format!(r#""data_ref":"sha256:{}""#, Digest::of(&closes).to_hex()),
        r#""source":"cell_recorded_daily_bars""#.to_owned(),
        format!(r#""instrument_id":"{QQQ}""#),
    ] {
        assert!(observation.contains(&member), "{member} in {observation}");
    }
    let output = body(output);
    for member in [
        format!(r#""model_id":"{MODEL}""#),
        format!(r#""content_hash":"sha256:{PINNED_HASH}""#),
        format!(r#""instrument_id":"{QQQ}""#),
        r#""direction":"long""#.to_owned(),
    ] {
        assert!(output.contains(&member), "{member} in {output}");
    }
    advance(&rig, "2026-10-26T19:00:01Z");
    process.step(at("2026-10-26T19:00:01Z"));
    assert_eq!(
        kinds(&rig)
            .iter()
            .filter(|kind| *kind == "ModelOutputRecorded")
            .count(),
        1,
        "the next tick's same daily output is recorded once"
    );
}

#[test]
fn closes_the_store_does_not_hold_journal_neither_record_and_the_process_stays_ready() {
    let root = Root::new("runtime-unheld");
    store_series(&root.bars(), "QQQ", &rising(30), None);
    FsArtifactStore::open(root.artifacts()).unwrap();
    let rig = Rig::default();
    let mut process = started(&rig, &root);
    advance(&rig, NOW);
    process.step(at(NOW));
    assert_eq!(
        process.lifecycle().state(),
        State::Ready,
        "{:?}",
        kinds(&rig)
    );
    let kinds = kinds(&rig);
    assert!(
        !kinds
            .iter()
            .any(|kind| kind == "ObservationRecorded" || kind == "ModelOutputRecorded"),
        "{kinds:?}"
    );
}
