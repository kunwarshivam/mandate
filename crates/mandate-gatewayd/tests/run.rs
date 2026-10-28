//! The binary's start (DEC-2326 items 1 to 3, DEC-2257 item 2, DEC-2258, E21-62): its arguments,
//! where a live start is refused in every form; the recording read strictly; and the recorded
//! service over the journaled meter streams, which journals every call it reserves, refuses a
//! foreign workspace and an over-budget call with nothing sent, and does not start while a meter
//! stream or a deployed agent's cap does not read. The oracle reads the journal's own rows.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mandate_canon::{Digest, decode_ulid, parse};
use mandate_gateway::fake::RecordedTransport;
use mandate_gateway::kinds::{KeyOwner, Outcome, Purpose};
use mandate_gateway::policy::EndpointPolicy;
use mandate_gateway::provider::NoKeys;
use mandate_gateway::provider::{IdSource, ManualClock, SequentialIds};
use mandate_gateway::registry::Locality;
use mandate_gateway::registry::RegistryError;
use mandate_gateway::{CallRequest, RequestError};
use mandate_gatewayd::cap::CapError;
use mandate_gatewayd::config::Config;
use mandate_gatewayd::live::{LiveStart, live_service, live_settings, read_registry, upstreams};
use mandate_gatewayd::recorded::Replies;
use mandate_gatewayd::recorded::{RATE_MARGIN, WORKSPACE_RATE};
use mandate_gatewayd::run::{
    self, ArgsError, Command, LiveArgs, Parts, RecordedService, RunArgs, StartFailure, WriteLog,
    answer_recorded, heartbeat, host_now, observe, opened_ids, read_recorded, recorded_service,
    started_ids,
};
use mandate_gatewayd::serve::Log;
use mandate_gatewayd::service::Control;
use mandate_gatewayd::start::StartError;
use mandate_journal::{
    AppendOutcome, ArtifactError, ArtifactRef, ArtifactSource, StoredEvent, StreamId,
};
use mandate_process::lifecycle::{Input, Kind, Lifecycle};
use mandate_process::probe::{self, Probe, Status};
use mandate_research_run::agentic::LoopBounds;
use mandate_research_run::live::{LiveEntry, Refused, read_live};
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

/// DEC-2216 item 4, DEC-2326 item 3: `run --registry` starts live from the registry file, with
/// the vault's address and mounted credentials; it never also replays a recording, and a live
/// start missing any of its flags, or naming the registry as `--registry=`, starts nothing.
#[test]
fn a_live_start_reads_its_registry_and_vault_flags() {
    assert_eq!(
        run::command(&args(
            "run --registry /r/registry.json --listen 0.0.0.0:8080 --probe /tmp/p.sock \
             --vault-address https://vault:8200 --vault-dir /var/run/vault"
        )),
        Ok(Command::Live(LiveArgs {
            registry: PathBuf::from("/r/registry.json"),
            listen: "0.0.0.0:8080".parse::<SocketAddr>().unwrap(),
            probe: PathBuf::from("/tmp/p.sock"),
            vault_address: "https://vault:8200".to_owned(),
            vault_dir: PathBuf::from("/var/run/vault"),
        }))
    );
    let vault = "--vault-address https://v:8200 --vault-dir /v";
    for (text, refused, code) in [
        (
            format!(
                "run --recorded /b.json --registry /r.json --listen 0.0.0.0:1 --probe /p {vault}"
            ),
            ArgsError::RecordedAndLive,
            "recorded_and_live",
        ),
        (
            format!("run --registry=/r.json --listen 0.0.0.0:1 --probe /p {vault}"),
            ArgsError::RegistryMissing,
            "registry_missing",
        ),
        (
            format!("run --registry --listen 0.0.0.0:1 --probe /p {vault}"),
            ArgsError::RegistryMissing,
            "registry_missing",
        ),
        (
            "run --registry /r.json --listen 0.0.0.0:1 --probe /p --vault-dir /v".to_owned(),
            ArgsError::VaultAddressMissing,
            "vault_address_missing",
        ),
        (
            "run --registry /r.json --listen 0.0.0.0:1 --probe /p --vault-address https://v:8200"
                .to_owned(),
            ArgsError::VaultDirMissing,
            "vault_dir_missing",
        ),
        (
            format!("run --registry /r.json --probe /p {vault}"),
            ArgsError::ListenMissing,
            "listen_missing",
        ),
    ] {
        assert_eq!(run::command(&args(&text)), Err(refused), "{text}");
        assert_eq!(refused.code(), code);
    }
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

/// The demo recording as a single-call one, whose one call is answered with its final reply: its
/// entry is agentic, and these tests are about the meters, which a call's run kind does not change.
fn demo() -> RecordedModel {
    let mut demo = read_recorded(&PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(DEMO)).unwrap();
    assert!(demo.entry.agentic.is_some(), "the demo entry is agentic");
    demo.entry.agentic = None;
    demo.turns.clear();
    demo.monitor_reply = None;
    demo
}

fn config(workspace_monthly: &str) -> Config {
    config_with(workspace_monthly, "20")
}

fn config_with(workspace_monthly: &str, deployment_monthly: &str) -> Config {
    let vars = BTreeMap::from([
        ("MANDATE_GATEWAY_WORKSPACE", WORKSPACE.to_owned()),
        ("MANDATE_GATEWAY_DEPLOYMENT", DEPLOYMENT.to_owned()),
        ("MANDATE_JOURNAL_DSN", "postgres://unused".to_owned()),
        ("MANDATE_ARTIFACTS", "/unused".to_owned()),
        (
            "MANDATE_GATEWAY_WORKSPACE_MONTHLY_USD",
            workspace_monthly.to_owned(),
        ),
        (
            "MANDATE_GATEWAY_DEPLOYMENT_MONTHLY_USD",
            deployment_monthly.to_owned(),
        ),
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
    parts_at(store, world, NOW)
}

fn parts_at<S: Store + Clone>(store: S, world: &World, now: &'static str) -> Parts<S, ManualClock> {
    Parts {
        workspace_store: store.clone(),
        deployment_store: store,
        clock: ManualClock::at(at(now)),
        now: Rc::new(move || at(now)),
        ids: run::ids(&config("50"), at(now)),
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
    let lifecycle = Lifecycle::new(Kind::Service, Duration::from_secs(10), start);
    let status = Status::new(lifecycle.snapshot());
    let lifecycle = Arc::new(Mutex::new(lifecycle));
    observe(&lifecycle, &status, start, Input::StartupPassed);
    let live_at = |secs: i64| {
        let when = UtcNanos::from_parts(1_792_159_200 + secs, 0).unwrap();
        probe::answer(Probe::Live, status.snapshot(), when).ok
    };
    assert!(!live_at(15), "the start's bound alone ends before");
    let serving = Arc::new(AtomicBool::new(true));
    heartbeat(
        Arc::clone(&lifecycle),
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

/// Design §3.4: a heartbeat after the process exited leaves it exited, so the probe never reads a
/// stopped service as live.
#[test]
fn a_heartbeat_after_the_exit_leaves_the_process_stopped() {
    let start = at("2026-10-16T14:00:00.000000000Z");
    let lifecycle = Lifecycle::new(Kind::Service, Duration::from_secs(10), start);
    let status = Status::new(lifecycle.snapshot());
    let lifecycle = Mutex::new(lifecycle);
    observe(&lifecycle, &status, start, Input::StartupPassed);
    assert!(probe::answer(Probe::Ready, status.snapshot(), start).ok);
    observe(&lifecycle, &status, start, Input::Failed);
    observe(&lifecycle, &status, start, Input::Heartbeat);
    let live = probe::answer(Probe::Live, status.snapshot(), start);
    assert_eq!((live.ok, live.word), (false, "stopped"));
}

/// The ids a stream is opened with: the first of the stream name's digest, as DEC-2258 item 2 has
/// the cell's runtime draw them, computed here from the digest itself.
fn opened_id(stream: &str) -> String {
    let digest = Digest::of(stream.as_bytes());
    let (high, _) = digest.as_bytes().split_at(16);
    mandate_canon::encode_ulid(u128::from_be_bytes(high.try_into().unwrap()))
}

fn payload(row: &StoredEvent) -> mandate_canon::Value {
    parse(&row.body).unwrap().get("payload").unwrap().clone()
}

/// DEC-2258, DEC-2207, journal spec §9.19: each meter stream opens once with its own name's id
/// and its own type, in paper; the deployment stream's copies carry the deployment meter's own ids,
/// lane 1 at the start's millisecond, never a call's.
#[test]
fn each_meter_stream_opens_with_its_own_name_s_id_and_type() {
    let store = MemoryStore::default();
    let world = World::default();
    world.deploy(AGENT, &world.version("5"));
    let model = demo();
    let mut service = started(&store, &world, "50");
    answer_recorded(
        &mut service,
        &Replies::new(&model),
        &request(&model, WORKSPACE, 1),
    )
    .unwrap();
    for (stream, stream_type, member, id) in [
        (
            format!("meter:{WORKSPACE}"),
            "meter",
            "workspace_id",
            WORKSPACE,
        ),
        (
            format!("dmeter:{DEPLOYMENT}"),
            "deployment_meter",
            "deployment_id",
            DEPLOYMENT,
        ),
    ] {
        let rows = stream_rows(&store, &stream);
        assert!(
            rows.iter().all(|row| row.environment == "paper"),
            "{stream}"
        );
        let opened: Vec<&StoredEvent> = rows
            .iter()
            .filter(|row| row.event_type == "StreamOpened")
            .collect();
        assert_eq!(opened.len(), 1, "{stream}");
        assert_eq!(opened[0].event_id, opened_id(&stream));
        let opened = payload(opened[0]);
        assert_eq!(
            opened.get("stream_type").and_then(|v| v.as_str()),
            Some(stream_type)
        );
        assert_eq!(opened.get(member).and_then(|v| v.as_str()), Some(id));
    }
    let lane = |id: &str| (decode_ulid(id).unwrap() >> 72) & 0xff;
    let ms = |id: &str| decode_ulid(id).unwrap() >> 80;
    for row in stream_rows(&store, &format!("dmeter:{DEPLOYMENT}"))
        .iter()
        .filter(|row| row.event_type != "StreamOpened")
    {
        assert_eq!(lane(&row.event_id), 1, "{}", row.event_type);
        assert_eq!(ms(&row.event_id), 1_792_159_200_000);
    }
}

/// DEC-2258 item 2, journal spec §5.1: a restart over the same journal reopens neither stream, and
/// no event id on either stream repeats across the two starts.
#[test]
fn a_restart_reopens_no_stream_and_repeats_no_id() {
    let store = MemoryStore::default();
    let world = World::default();
    world.deploy(AGENT, &world.version("5"));
    let model = demo();
    for (n, now) in [(1, NOW), (2, "2026-10-16T14:00:30.000000000Z")] {
        let mut service =
            recorded_service(&config("50"), &model, parts_at(store.clone(), &world, now)).unwrap();
        let record = answer_recorded(
            &mut service,
            &Replies::new(&model),
            &request(&model, WORKSPACE, n),
        )
        .unwrap();
        assert_eq!(record.outcome, Outcome::Ok, "start {n}");
    }
    let mut seen = BTreeSet::new();
    for stream in [format!("meter:{WORKSPACE}"), format!("dmeter:{DEPLOYMENT}")] {
        let rows = stream_rows(&store, &stream);
        assert_eq!(
            rows.iter()
                .filter(|row| row.event_type == "StreamOpened")
                .count(),
            1,
            "{stream}"
        );
        assert_eq!(
            rows.iter()
                .filter(|row| row.event_type == "MeterReserved")
                .count(),
            2,
            "{stream}"
        );
        for row in &rows {
            assert!(
                seen.insert(row.event_id.clone()),
                "{} repeats",
                row.event_id
            );
        }
    }
}

/// DEC-997 item 3, DEC-2326 item 2: the workspace's budget is the workspace meter's cap and the
/// deployment's is the deployment meter's, each as the environment names it; with the deployment's
/// alone below the call's maximum it binds alone, and a call refused reserves nothing on either
/// stream and sends nothing. (A daily cap above the workspace's budget does not read, DEC-2254
/// item 3, so the workspace's case sets the daily cap with it.)
#[test]
fn each_budget_is_its_own_meter_s_cap_and_binds_alone() {
    let model = demo();
    let named = |text: &str| mandate_gateway::money::Usd6::parse(text).unwrap();
    for (workspace, deployment, daily) in
        [("0.000001", "20", "0.000001"), ("50", "0.000001", "0.5")]
    {
        let world = World::default();
        world.deploy(AGENT, &world.version(daily));
        let store = MemoryStore::default();
        let mut service = recorded_service(
            &config_with(workspace, deployment),
            &model,
            parts(store.clone(), &world),
        )
        .unwrap();
        let meter = service.gateway().meter();
        assert_eq!(meter.caps().workspace_monthly(), named(workspace));
        assert_eq!(meter.deployment().monthly(), named(deployment));
        let record = answer_recorded(
            &mut service,
            &Replies::new(&model),
            &request(&model, WORKSPACE, 1),
        )
        .unwrap();
        assert_eq!(
            record.outcome,
            Outcome::BudgetExhausted,
            "{workspace}/{deployment}"
        );
        assert!(service.gateway().transport().sent().is_empty());
        for stream in [format!("meter:{WORKSPACE}"), format!("dmeter:{DEPLOYMENT}")] {
            assert!(
                !kinds(&stream_rows(&store, &stream))
                    .iter()
                    .any(|k| k == "MeterReserved"),
                "{stream}"
            );
        }
    }
}

/// The founder's vetted list as the chart renders it (DEC-2282 item 6): A and B, and C, whose
/// undated snapshot the registry refuses (item 7).
fn vetted_entry(id: &str, provider: &str, slug: &str, snapshot: &str, upstream: &str) -> String {
    format!(
        r#"{{"status":"offered",
            "entry":{{"deadline_ms":120000,"max_output_tokens":4096,"model_id":"{id}",
                      "model_name":"{slug}","model_version":"1.0.0","provider":"{provider}","seed":7,
                      "snapshot":"{snapshot}"}},
            "endpoint":{{"cached_per_million":"0.1","host":"openrouter.ai","id":"openrouter:{id}",
                         "input_per_million":"2","output_per_million":"10","price_table":"openrouter 2026-10-27",
                         "region":"global","reported_provider":"Google","upstream":"{upstream}"}}}}"#
    )
}

fn vetted_file() -> String {
    format!(
        r#"{{"entries":[{},{},{}]}}"#,
        vetted_entry(
            "llm.research_sonnet",
            "anthropic",
            "anthropic/claude-sonnet-5.5",
            "claude-sonnet-5-5",
            "google-vertex/global"
        ),
        vetted_entry(
            "llm.research_haiku",
            "anthropic",
            "anthropic/claude-haiku-5.5",
            "claude-haiku-5-5",
            "google-vertex/global"
        ),
        vetted_entry(
            "llm.research_terra",
            "openai",
            "openai/gpt-5.6-terra",
            "gpt-5.6-terra",
            "azure"
        ),
    )
}

fn vetted() -> Vec<LiveEntry> {
    read_live(&parse(vetted_file().as_bytes()).unwrap()).unwrap()
}

fn live_request(entry: &LiveEntry, n: u128) -> CallRequest {
    let content = entry.entry.content();
    CallRequest {
        model: content.model_ref(),
        inputs: content
            .template
            .inputs
            .iter()
            .map(|name| (name.clone(), format!("live {name}")))
            .collect(),
        ..request(&demo(), WORKSPACE, n)
    }
}

fn live_started(
    store: &MemoryStore,
    world: &World,
    entries: &[LiveEntry],
) -> Result<LiveStart<MemoryStore, ManualClock, RecordedTransport>, StartFailure> {
    live_service(
        &config("50"),
        entries,
        parts(store.clone(), world),
        RecordedTransport::new(),
        Box::new(NoKeys),
    )
}

/// DEC-2216 item 4: the live service starts with every vetted entry the registry takes registered
/// and none offered, since no gate has passed, and C refused while undated. A research call naming
/// an entry is answered `policy_denied`, with nothing reserved and nothing sent, so a live start
/// spends nothing until a gate passes.
#[test]
fn the_live_service_starts_offering_nothing_and_sends_nothing() {
    let store = MemoryStore::default();
    let world = World::default();
    world.deploy(AGENT, &world.version("5"));
    let entries = vetted();
    let (mut service, refused) = live_started(&store, &world, &entries).unwrap();
    assert_eq!(
        refused,
        vec![Refused {
            model_id: "llm.research_terra".to_owned(),
            error: RegistryError::FloatingAlias,
        }]
    );
    let meter = format!("meter:{WORKSPACE}");
    let before = stream_rows(&store, &meter).len();
    for (n, entry) in (1..).zip(&entries[..2]) {
        let record = service.answer(&live_request(entry, n)).unwrap();
        assert_eq!(record.outcome, Outcome::PolicyDenied, "{record:?}");
        assert!(record.reservation.is_none());
    }
    assert!(service.gateway().transport().sent().is_empty());
    assert_eq!(
        stream_rows(&store, &meter).len(),
        before,
        "nothing reserved"
    );
}

/// DEC-2216 item 4: an entry offered on a gate report needs the report's every binding re-read and
/// an identity probe, which the gate's build adds; until then a registry file naming a report
/// starts nothing, rather than offer on the file's say-so.
#[test]
fn a_registry_naming_a_gate_report_starts_nothing_yet() {
    let store = MemoryStore::default();
    let world = World::default();
    let mut entries = vetted();
    entries[0].gate_report = Some(format!("sha256:{}", "a".repeat(64)));
    assert_eq!(
        live_started(&store, &world, &entries).err(),
        Some(StartFailure::GateReportUnverifiable)
    );
    assert_eq!(
        StartFailure::GateReportUnverifiable.code(),
        "gate_report_unverifiable"
    );
}

/// DEC-2326 item 1: the registry file is read strictly, once: one that cannot be read, or is not a
/// live registry, starts nothing.
#[test]
fn the_registry_file_is_read_strictly() {
    let dir = std::env::temp_dir().join(format!("gatewayd-registry-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert_eq!(
        read_registry(&dir.join("absent.json")).err(),
        Some(StartFailure::RegistryUnreadable)
    );
    let malformed = dir.join("malformed.json");
    std::fs::write(&malformed, br#"{"entries":[]}"#).unwrap();
    assert_eq!(
        read_registry(&malformed).err(),
        Some(StartFailure::RegistryMalformed)
    );
    let good = dir.join("registry.json");
    std::fs::write(&good, vetted_file()).unwrap();
    assert_eq!(read_registry(&good), Ok(vetted()));
    assert_eq!(
        StartFailure::RegistryUnreadable.code(),
        "registry_unreadable"
    );
    assert_eq!(StartFailure::RegistryMalformed.code(), "registry_malformed");
}

/// DEC-2216 item 4, DEC-2020: the key is served for, and the endpoint policy admits, the upstreams
/// of the entries the registry takes alone, so C's `azure` is neither keyed nor allowed while C is
/// refused; and an agent's rate is the most turns any taken entry's run may take, plus the margin.
#[test]
fn the_settings_and_the_keyed_upstreams_are_the_taken_entries_alone() {
    let mut entries = vetted();
    assert_eq!(
        upstreams(&entries),
        BTreeSet::from(["google-vertex/global".to_owned()])
    );
    entries[1].entry.agentic = Some(LoopBounds::v0_2());
    let settings = live_settings(WORKSPACE, &entries);
    assert_eq!(settings.workspace_id, WORKSPACE);
    assert_eq!(
        settings.policy,
        EndpointPolicy::new(
            BTreeSet::from([Locality::Hosted]),
            Some(BTreeSet::from(["google-vertex/global".to_owned()])),
            None,
        )
    );
    assert_eq!(settings.rate.per_workspace, WORKSPACE_RATE);
    assert_eq!(
        settings.rate.per_agent,
        LoopBounds::v0_2().max_turns + RATE_MARGIN
    );
    entries[1].entry.agentic = None;
    assert_eq!(
        live_settings(WORKSPACE, &entries).rate.per_agent,
        1 + RATE_MARGIN,
        "single-call entries take one call a run"
    );
}

/// DEC-2216 item 4: a registry file naming a gate report is refused as it is read, before the
/// vault is touched or anything is built.
#[test]
fn a_registry_file_naming_a_gate_report_is_refused_as_it_is_read() {
    let dir = std::env::temp_dir().join(format!("gatewayd-gated-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let named = dir.join("registry.json");
    let text = vetted_file().replacen(
        r#""status":"offered","#,
        &format!(
            r#""status":"offered","gate_report":"sha256:{}","#,
            "a".repeat(64)
        ),
        1,
    );
    std::fs::write(&named, text).unwrap();
    assert_eq!(
        read_registry(&named).err(),
        Some(StartFailure::GateReportUnverifiable)
    );
}

/// DEC-2282 item 6, DEC-2396 item 3, DEC-2216 item 5: the live registry file the chart renders for a
/// hosted cell reads strictly; Sonnet 5.5 and Haiku 5.5 register, GPT-5.6 Terra is refused while its
/// snapshot is undated, and the key is served for `google-vertex/global` alone.
#[test]
fn the_committed_live_registry_file_reads_and_registers_a_and_b_alone() {
    let path = PathBuf::from("../../deploy/cell/model-gateway/registry.json");
    let entries = read_registry(&path).unwrap();
    let ids: Vec<&str> = entries.iter().map(|e| e.entry.model_id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "llm.research_sonnet",
            "llm.research_haiku",
            "llm.research_terra"
        ]
    );
    assert!(
        entries
            .iter()
            .all(|e| !e.offered && e.gate_report.is_none())
    );
    assert_eq!(
        upstreams(&entries),
        BTreeSet::from(["google-vertex/global".to_owned()])
    );
    let store = MemoryStore::default();
    let world = World::default();
    let (_, refused) = live_started(&store, &world, &entries).unwrap();
    assert_eq!(
        refused,
        vec![Refused {
            model_id: "llm.research_terra".to_owned(),
            error: RegistryError::FloatingAlias,
        }]
    );
}
