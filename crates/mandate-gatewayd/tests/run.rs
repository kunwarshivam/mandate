//! The binary's start (DEC-2326 items 1 to 3, DEC-2257 item 2, DEC-2258, E21-62): its arguments,
//! where a live start is refused in every form; the recording read strictly; and the recorded
//! service over the journaled meter streams, which journals every call it reserves, refuses a
//! foreign workspace and an over-budget call with nothing sent, and does not start while a meter
//! stream or a deployed agent's cap does not read. The oracle reads the journal's own rows.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::Duration;

use mandate_canon::{Digest, decode_ulid, parse};
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::provider::{IdSource, ManualClock, SequentialIds};
use mandate_gateway::{CallRequest, RequestError};
use mandate_gatewayd::cap::CapError;
use mandate_gatewayd::config::Config;
use mandate_gatewayd::recorded::Replies;
use mandate_gatewayd::run::{
    self, ArgsError, Command, Parts, RecordedService, RunArgs, StartFailure, WriteLog,
    answer_recorded, heartbeat, host_now, opened_ids, read_recorded, recorded_service, started_ids,
};
use mandate_gatewayd::serve::Log;
use mandate_gatewayd::service::Control;
use mandate_gatewayd::start::StartError;
use mandate_journal::{
    AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, StreamId,
};
use mandate_process::lifecycle::{Input, Kind, Lifecycle};
use mandate_process::probe::{self, Probe, Status};
use mandate_research_run::recorded::RecordedModel;
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

const WORKSPACE: &str = "ws_01J8Z2";
const DEPLOYMENT: &str = "dep_01J8Z2";
const AGENT: &str = "agent_a";
const DEMO: &str = "../../deploy/cell/demo/research/research.json";
const MANDATE: &str = include_str!("../../mandate-daemon/tests/fixtures/mandate.json");

fn args(text: &str) -> Vec<String> {
    text.split_whitespace().map(str::to_owned).collect()
}

fn at(text: &str) -> UtcNanos {
    UtcNanos::parse(text).unwrap()
}

/// DEC-2326 item 3: `run` names its recording, its address and its probe socket, and `probe` its
/// word and the socket.
#[test]
fn run_and_probe_read_their_flags() {
    assert_eq!(
        run::command(&args(
            "run --recorded /b/research.json --listen 0.0.0.0:8080 --probe /tmp/p.sock"
        )),
        Ok(Command::Run(RunArgs {
            recorded: PathBuf::from("/b/research.json"),
            listen: "0.0.0.0:8080".parse::<SocketAddr>().unwrap(),
            probe: PathBuf::from("/tmp/p.sock"),
        }))
    );
    assert_eq!(
        run::command(&args("probe readyz --probe /tmp/p.sock")),
        Ok(Command::Probe {
            asked: Probe::Ready,
            socket: PathBuf::from("/tmp/p.sock"),
        })
    );
}

/// DEC-2326 items 1 and 3, DEC-79: a live start is refused `live_not_built` in every form,
/// whatever else is named, so no argument makes the binary build a provider client.
#[test]
fn a_live_start_is_refused_in_every_form() {
    for text in [
        "run --registry /r.json --listen 0.0.0.0:8080 --probe /p",
        "run --recorded /b.json --registry /r.json --listen 0.0.0.0:8080 --probe /p",
        "run --registry=/r.json --recorded /b.json --listen 0.0.0.0:8080 --probe /p",
        "run --recorded /b.json --listen 0.0.0.0:8080 --probe /p --registry",
        "probe livez --probe /p --registry /r.json",
    ] {
        assert_eq!(
            run::command(&args(text)),
            Err(ArgsError::LiveNotBuilt),
            "{text}"
        );
    }
    assert_eq!(ArgsError::LiveNotBuilt.code(), "live_not_built");
}

/// A flag that is missing, empty, written `--name=value`, or followed by another flag reads as
/// missing, and each refusal names its own code.
#[test]
fn each_missing_or_malformed_flag_refuses_with_its_code() {
    for (text, refused, code) in [
        (
            "run --recorded /b --listen 0.0.0.0:1",
            ArgsError::ProbeSocketMissing,
            "probe_socket_missing",
        ),
        (
            "run --recorded /b --listen 0.0.0.0:1 --probe --x",
            ArgsError::ProbeSocketMissing,
            "probe_socket_missing",
        ),
        (
            "run --listen 0.0.0.0:1 --probe /p",
            ArgsError::RecordedMissing,
            "recorded_bundle_missing",
        ),
        (
            "run --recorded=/b --listen 0.0.0.0:1 --probe /p",
            ArgsError::RecordedMissing,
            "recorded_bundle_missing",
        ),
        (
            "run --recorded --listen 0.0.0.0:1 --probe /p",
            ArgsError::RecordedMissing,
            "recorded_bundle_missing",
        ),
        (
            "run --recorded /b --probe /p",
            ArgsError::ListenMissing,
            "listen_missing",
        ),
        (
            "run --recorded /b --listen nowhere --probe /p",
            ArgsError::ListenMissing,
            "listen_missing",
        ),
        (
            "probe ready --probe /p",
            ArgsError::ProbeUnknown,
            "probe_unknown",
        ),
        ("serve --probe /p", ArgsError::Usage, "usage"),
        ("--probe /p", ArgsError::Usage, "usage"),
    ] {
        assert_eq!(run::command(&args(text)), Err(refused), "{text}");
        assert_eq!(refused.code(), code);
    }
}

/// DEC-2210 item 1: the recording is read strictly; a file that is missing or not a recording
/// refuses the start with its code.
#[test]
fn the_recording_is_read_strictly() {
    let demo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEMO);
    assert!(read_recorded(&demo).is_ok());
    let missing = std::env::temp_dir().join("mandate-gatewayd-no-such-recording.json");
    assert_eq!(
        read_recorded(&missing).map(|_| ()),
        Err(StartFailure::RecordedUnreadable)
    );
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    assert_eq!(
        read_recorded(&manifest).map(|_| ()),
        Err(StartFailure::RecordedMalformed)
    );
    assert_eq!(
        StartFailure::RecordedUnreadable.code(),
        "recorded_bundle_unreadable"
    );
    assert_eq!(
        StartFailure::RecordedMalformed.code(),
        "recorded_bundle_malformed"
    );
}

fn demo() -> RecordedModel {
    read_recorded(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEMO)).unwrap()
}

fn config(workspace_monthly: &str) -> Config {
    let vars = BTreeMap::from([
        ("MANDATE_GATEWAY_WORKSPACE", WORKSPACE.to_owned()),
        ("MANDATE_GATEWAY_DEPLOYMENT", DEPLOYMENT.to_owned()),
        ("MANDATE_JOURNAL_DSN", "postgres://unused".to_owned()),
        ("MANDATE_ARTIFACTS", "/unused".to_owned()),
        (
            "MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD",
            workspace_monthly.to_owned(),
        ),
        ("MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD", "20".to_owned()),
    ]);
    Config::from_env(&|name| vars.get(name).cloned()).unwrap()
}

/// The control stream and the artifact store a test sets up.
#[derive(Clone, Default)]
struct World {
    control: Rc<RefCell<Vec<StoredEvent>>>,
    store: Rc<RefCell<BTreeMap<Digest, Vec<u8>>>>,
}

impl Control for World {
    fn rows(&self) -> Option<Vec<StoredEvent>> {
        Some(self.control.borrow().clone())
    }
}

impl ArtifactSource for World {
    fn read_artifact(&self, reference: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        self.store.borrow().read_artifact(reference)
    }
}

impl World {
    /// Stores the fixture mandate with this daily research cap and returns its version.
    fn version(&self, cap: &str) -> String {
        let text = MANDATE.trim_end().replace(
            r#""research":null"#,
            &format!(r#""research":{{"cost_cap_usd_per_day":"{cap}"}}"#),
        );
        let bytes = mandate_canon::to_canonical(&parse(text.as_bytes()).unwrap());
        let reference = ArtifactRef::of(&bytes);
        self.store.borrow_mut().insert(reference.digest(), bytes);
        reference.to_string()
    }

    fn deploy(&self, agent: &str, version: &str) {
        let payload = format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{version}"}}"#
        );
        let mut rows = self.control.borrow_mut();
        let seq = u64::try_from(rows.len()).unwrap() + 1;
        let event_id = format!("01J8Z3MC{seq:018}");
        let body = format!(
            r#"{{"actor":{{"build":null,"id":"user-owner","kind":"user","version":"1"}},"artifact_refs":[],"causation_id":null,"clock_source":"system","config_refs":{{}},"correlation_id":null,"envelope_version":1,"environment":"paper","event_id":"{event_id}","event_time":"2026-10-16T13:00:00.000000000Z","event_type":"AgentDeployed","payload":{payload},"pii_refs":[],"schema_version":1,"stream_id":"ctl:{WORKSPACE}"}}"#
        );
        rows.push(StoredEvent {
            stream_id: format!("ctl:{WORKSPACE}"),
            seq,
            event_id,
            event_type: "AgentDeployed".into(),
            schema_version: 1,
            environment: "paper".into(),
            recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
            prev_hash: Digest::of(b"prev"),
            hash: Digest::of(body.as_bytes()),
            body: body.into_bytes(),
        });
    }
}

/// A journal that cannot answer a read of `unreadable`, and answers every other call as the
/// in-memory journal does.
#[derive(Clone)]
struct Blind {
    inner: MemoryStore,
    unreadable: &'static str,
}

impl Store for Blind {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.inner.take_ownership(stream)
    }

    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if stream.as_str().starts_with(self.unreadable) {
            None
        } else {
            self.inner.rows(stream)
        }
    }

    fn append(
        &mut self,
        stream: &StreamId,
        expected_head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.inner
            .append(stream, expected_head, epoch, recorded_at, drafts)
    }
}

const NOW: &str = "2026-10-16T14:00:00.000000000Z";

fn parts<S: Store + Clone>(store: S, world: &World) -> Parts<S, ManualClock> {
    Parts {
        workspace_store: store.clone(),
        deployment_store: store,
        clock: ManualClock::at(at(NOW)),
        now: Rc::new(|| at(NOW)),
        ids: [
            Box::new(started_ids(at(NOW), 0)),
            Box::new(opened_ids(&format!("meter:{WORKSPACE}"))),
            Box::new(opened_ids(&format!("dmeter:{DEPLOYMENT}"))),
            Box::new(started_ids(at(NOW), 1)),
        ],
        control: Box::new(world.clone()),
        artifacts: Box::new(world.clone()),
    }
}

fn request(model: &RecordedModel, workspace: &str, n: u128) -> CallRequest {
    let content = model.entry.content();
    CallRequest {
        call_id: mandate_canon::encode_ulid((1 << 90) + n),
        workspace_id: workspace.to_owned(),
        agent_id: Some(AGENT.to_owned()),
        purpose: Purpose::Research,
        model: content.model_ref(),
        inputs: content
            .template
            .inputs
            .iter()
            .map(|name| (name.clone(), format!("recorded {name}")))
            .collect(),
        as_of: at("2026-10-16T13:55:00.000000000Z"),
        deadline: at("2026-10-16T14:02:00.000000000Z"),
        observations: vec![],
        key_owner: KeyOwner::Platform,
        run_kind: None,
        turns: vec![],
        tools: vec![],
        spend_ceiling: None,
    }
}

fn stream_rows(store: &MemoryStore, stream: &str) -> Vec<StoredEvent> {
    store.rows(&StreamId::parse(stream).unwrap()).unwrap()
}

fn kinds(rows: &[StoredEvent]) -> Vec<String> {
    rows.iter().map(|row| row.event_type.clone()).collect()
}

fn started(
    store: &MemoryStore,
    world: &World,
    monthly: &str,
) -> RecordedService<MemoryStore, ManualClock> {
    recorded_service(&config(monthly), &demo(), parts(store.clone(), world)).unwrap()
}

/// DEC-2257 item 2, DEC-2258, inference spec §3.6: a research call is reserved on the workspace's
/// journaled meter stream and copied to the deployment's before it is sent, settled after, and
/// answered with the recorded reply, through the one recorded endpoint and without a key.
#[test]
fn a_call_is_journaled_on_both_meter_streams_and_answered_with_the_recording() {
    let store = MemoryStore::default();
    let world = World::default();
    world.deploy(AGENT, &world.version("5"));
    let model = demo();
    let mut service = started(&store, &world, "50");
    let record = answer_recorded(
        &mut service,
        &Replies::new(&model),
        &request(&model, WORKSPACE, 1),
    )
    .unwrap();
    assert_eq!(record.outcome, Outcome::Ok, "{record:?}");
    assert_eq!(record.endpoint, model.endpoint.id);
    assert!(record.reservation.is_some(), "the call was reserved");
    let workspace = kinds(&stream_rows(&store, &format!("meter:{WORKSPACE}")));
    let deployment = kinds(&stream_rows(&store, &format!("dmeter:{DEPLOYMENT}")));
    for rows in [&workspace, &deployment] {
        let reserved = rows.iter().position(|kind| kind == "MeterReserved");
        let settled = rows.iter().position(|kind| kind == "MeterSettled");
        assert!(
            reserved.is_some() && reserved < settled,
            "reserved, then settled: {rows:?}"
        );
    }
    let sent = service.gateway().transport().sent();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].host, model.endpoint.host);
}

/// DEC-2265 items 2 and 7, INF-9: a request naming another workspace is refused before anything is
/// reserved or sent.
#[test]
fn a_foreign_workspace_is_refused_with_nothing_reserved_or_sent() {
    let store = MemoryStore::default();
    let world = World::default();
    world.deploy(AGENT, &world.version("5"));
    let model = demo();
    let mut service = started(&store, &world, "50");
    let before = stream_rows(&store, &format!("meter:{WORKSPACE}")).len();
    assert_eq!(
        answer_recorded(
            &mut service,
            &Replies::new(&model),
            &request(&model, "ws_other", 1)
        ),
        Err(RequestError::ForeignWorkspace)
    );
    assert_eq!(
        stream_rows(&store, &format!("meter:{WORKSPACE}")).len(),
        before
    );
    assert!(service.gateway().transport().sent().is_empty());
}

/// DEC-997 item 3, DEC-2326 item 2, DEC-2272: a budget the environment and the deployed mandate
/// name binds; a call whose maximum passes it is refused `budget_exhausted` and nothing is
/// reserved or sent.
#[test]
fn a_named_budget_below_the_call_s_maximum_binds_with_nothing_sent() {
    let store = MemoryStore::default();
    let world = World::default();
    world.deploy(AGENT, &world.version("0.000001"));
    let model = demo();
    let mut service = started(&store, &world, "0.000001");
    let record = answer_recorded(
        &mut service,
        &Replies::new(&model),
        &request(&model, WORKSPACE, 1),
    )
    .unwrap();
    assert_eq!(record.outcome, Outcome::BudgetExhausted);
    assert!(service.gateway().transport().sent().is_empty());
    assert!(
        !kinds(&stream_rows(&store, &format!("meter:{WORKSPACE}")))
            .iter()
            .any(|kind| kind == "MeterReserved"),
        "nothing reserved"
    );
}

/// DEC-2257 item 2: the service does not start while either meter stream cannot be read, and
/// names the one that does not.
#[test]
fn the_start_refuses_while_a_meter_stream_does_not_read() {
    let world = World::default();
    for (unreadable, refused, code) in [
        (
            "meter:",
            StartError::WorkspaceMeter,
            "workspace_meter_unavailable",
        ),
        (
            "dmeter:",
            StartError::DeploymentMeter,
            "deployment_meter_unavailable",
        ),
    ] {
        let store = Blind {
            inner: MemoryStore::default(),
            unreadable,
        };
        let started = recorded_service(&config("50"), &demo(), parts(store, &world));
        assert_eq!(
            started.err(),
            Some(StartFailure::Meter(refused)),
            "{unreadable}"
        );
        assert_eq!(StartFailure::Meter(refused).code(), code);
    }
}

/// DEC-2272 item 4: the service does not start while a deployed agent's cap cannot be read.
#[test]
fn the_start_refuses_while_a_deployed_agent_s_cap_does_not_read() {
    let world = World::default();
    world.deploy(AGENT, &ArtifactRef::of(b"never stored").to_string());
    let started = recorded_service(
        &config("50"),
        &demo(),
        parts(MemoryStore::default(), &world),
    );
    assert_eq!(
        started.err(),
        Some(StartFailure::Cap(CapError::DocumentUnavailable))
    );
}

/// DEC-2258 item 2: a meter stream opens with an id drawn from its own name, the same on every
/// start; the started ids are ULIDs at the start's millisecond, each lane apart, and a later
/// start's sort after an earlier one's.
#[test]
fn the_ids_never_meet_and_sort_by_start() {
    let first = |mut ids: SequentialIds| ids.next_id();
    assert_eq!(
        first(opened_ids("meter:ws_1")),
        first(opened_ids("meter:ws_1"))
    );
    assert_ne!(
        first(opened_ids("meter:ws_1")),
        first(opened_ids("dmeter:ws_1"))
    );
    let start = at("2026-10-16T14:00:00.123456789Z");
    let mut calls = started_ids(start, 0);
    let mut copies = started_ids(start, 1);
    let call = calls.next_id();
    let copy = copies.next_id();
    assert_ne!(call, copy);
    assert_ne!(calls.next_id(), call);
    let ms = |id: &str| decode_ulid(id).unwrap() >> 80;
    assert_eq!(ms(&call), 1_792_159_200_123);
    assert_eq!(ms(&copy), 1_792_159_200_123);
    let later = started_ids(at("2026-10-16T14:00:00.124000000Z"), 0).next_id();
    assert!(later > copies.next_id(), "a later start sorts after");
}

/// ES-05: the host clock reads now, not a default.
#[test]
fn the_host_clock_reads_now() {
    assert!(host_now() > at("2026-01-01T00:00:00.000000000Z"));
}

/// DEC-2265 item 9: each log line is written whole, with its newline, in order.
#[test]
fn the_log_writes_each_line_whole() {
    let log = WriteLog::new(Vec::new());
    log.line("call answered: ok");
    log.line("call refused: foreign_workspace");
    assert_eq!(
        String::from_utf8(log.into_inner()).unwrap(),
        "call answered: ok\ncall refused: foreign_workspace\n"
    );
}

/// Design §3.4: while the service serves, the heartbeat keeps its liveness bound moving past the
/// start's; once it stops serving, the bound stops moving.
#[test]
fn the_heartbeat_moves_liveness_while_serving_and_stops_after() {
    fn ticking() -> UtcNanos {
        static TICKS: AtomicI64 = AtomicI64::new(0);
        let tick = TICKS.fetch_add(1, Ordering::SeqCst) + 1;
        UtcNanos::from_parts(1_792_159_200 + tick, 0).unwrap()
    }
    let start = at("2026-10-16T14:00:00.000000000Z");
    let mut lifecycle = Lifecycle::new(Kind::Service, Duration::from_secs(10), start);
    lifecycle.observe(start, Input::StartupPassed);
    let status = Status::new(lifecycle.snapshot());
    let live_at = |secs: i64| {
        let when = UtcNanos::from_parts(1_792_159_200 + secs, 0).unwrap();
        probe::answer(Probe::Live, status.snapshot(), when).ok
    };
    assert!(!live_at(15), "the start's bound alone ends before");
    let serving = Arc::new(AtomicBool::new(true));
    heartbeat(
        lifecycle,
        status.clone(),
        Arc::clone(&serving),
        Duration::from_millis(5),
        ticking,
    );
    let moved = (0..400).any(|_| {
        std::thread::sleep(Duration::from_millis(5));
        live_at(15)
    });
    assert!(moved, "the heartbeat moved the bound");
    serving.store(false, Ordering::SeqCst);
    std::thread::sleep(Duration::from_millis(50));
    let stopped = status.snapshot();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(status.snapshot(), stopped, "it stops with the loop");
}
