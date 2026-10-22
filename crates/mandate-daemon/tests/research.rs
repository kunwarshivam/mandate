//! Research in the runtime process (DEC-2208): the research driver behind a deny-all default, over
//! the real in-memory journal, with a worker that records what it is handed and answers what a
//! test queues. The control and clock streams are rows a test lays down, as in `runtime.rs`.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Key, Object, Value, parse};
use mandate_daemon::cell::{self, CellDeployment, CellResearch, opt_in};
use mandate_daemon::recorded::Bundle;
use mandate_daemon::worker::{RecordedWorker, WorkerFor};
use mandate_daemon::{
    Alert, ArtifactSink, Config, Refs, ResearchParts, ResearchSource, ResearchWiring,
    ResearchWorker, RuntimeProcess, Wiring, fixture,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::{CallRecord, Outcome as CallOutcome, Purpose};
use mandate_journal::{AppendOutcome, ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_num::Usd;
use mandate_process::{Exit, State};
use mandate_research::{
    AdmissionFacts, AllowlistVersion, AssetId, AutonomyDecision, LineageState, MandateEnvelope,
    PolicyOverlay, ResearchEnvelope, SourceAllowlist, SourceId, ValidatedMandate, WorkingUniverse,
};
use mandate_research_run::agentic::{LoopBounds, OutputMode, Tool};
use mandate_research_run::{ModelPin, OutputBounds, RetrievalPlan, Symbol};
use mandate_runtime::{
    AgentId, AgenticConfig, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment,
    DryRunVerdict, EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest, GateDryRun,
    IdGen, JournalCatalog, MandateView, Mode, OrderPlan, Preconditions, Proposal, ResearchConfig,
    ResearchInput, ResearchRequest, RiskClock, RuntimeState, Seq, SignalInputs, TurnRequest,
    WorkspaceId, WriterEpoch,
};
use mandate_spec::document::Mandate;
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

const WORKSPACE: &str = "ws1";
const AGENT: &str = "agent-a";
const AGENT_STREAM: &str = "agent:ws1:agent-a";
const CONTROL: &str = "ctl:ws1";
const CLOCK: &str = "clock:ws1";
const ACCOUNT: &str = "acct-1";
const ACCOUNT_STREAM: &str = "acct:ws1:acct-1";
const AAPL: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const MSFT: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
const NOW: i64 = 1_792_155_600;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// ULID-shaped ids, as every journal id is (journal spec §3): a run's id is one too, and its
/// records carry it as their `correlation_id`. `0`, the epoch in 4 places, the head in 14, and the
/// ordinal in 7 of Crockford's base 32, which holds every `u32`.
struct Ids;
impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        const CROCKFORD: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
        let mut letters = [b'0'; 7];
        let mut rest = ordinal;
        for slot in letters.iter_mut().rev() {
            *slot = CROCKFORD[usize::try_from(rest % 32).unwrap()];
            rest /= 32;
        }
        let ordinal = String::from_utf8(letters.to_vec()).unwrap();
        EventId(format!("0{:04}{:014}{ordinal}", epoch.0, head.0))
    }
}

struct Deny;
impl GateDryRun for Deny {
    fn check(&self, _: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Deny {
            reason_code: "fixture".into(),
        }
    }
}

struct Nothing;
impl OrderPlan for Nothing {
    fn plan(&self, _: &MandateView, _: &SignalInputs) -> Option<Proposal> {
        None
    }
    fn classify(&self, _: &MandateView, _: &Proposal) -> Classified {
        Classified {
            autonomy: Autonomy::Deny,
            decided_by: None,
        }
    }
}

struct Empty;
impl FlattenPlanner for Empty {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        FlattenPlan {
            cancel_client_order_ids: Vec::new(),
            sells: Vec::new(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}

struct MandateRef;
impl Refs for MandateRef {
    fn refs(&self, _: &EventDraft) -> Option<(u64, Object)> {
        let version = format!("sha256:{}", Digest::of(b"the mandate version").to_hex());
        let mut refs = Object::new();
        refs.insert(Key::new("mandate_version").ok()?, Value::Str(version));
        Some((1, refs))
    }
}

/// The research entry's agentic configuration: `final_output`, with the v0.2 bounds.
fn agentic() -> AgenticConfig {
    AgenticConfig {
        output: OutputMode::FinalOutput,
        bounds: LoopBounds::v0_2(),
        ideate_menu: BTreeSet::from([
            Tool::MarketBars,
            Tool::NewsSearch,
            Tool::NewsGet,
            Tool::Note,
        ]),
        monitor_menu: BTreeSet::from([Tool::NewsSearch, Tool::NewsGet, Tool::Note]),
        cost_ceiling: Usd6::from_micros(500_000),
    }
}

/// The references the deployment's records carry: the mandate version, and the research entry's
/// model version, which the run records and the model's records need (journal spec §9).
struct RunRefs;
impl Refs for RunRefs {
    fn refs(&self, _: &EventDraft) -> Option<(u64, Object)> {
        let digest = |text: &[u8]| Value::Str(format!("sha256:{}", Digest::of(text).to_hex()));
        let mut refs = Object::new();
        refs.insert(
            Key::new("mandate_version").ok()?,
            digest(b"the mandate version"),
        );
        refs.insert(
            Key::new("model_version").ok()?,
            digest(b"the research entry"),
        );
        Some((1, refs))
    }
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("`{text}` is USD: {e}"))
}

fn config_entry() -> ResearchConfig {
    ResearchConfig {
        pin: ModelPin {
            model_id: "llm.research_agent".to_owned(),
            model_version: "0.1.0".to_owned(),
            content_hash: Digest::of(b"the research entry"),
        },
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        plan: RetrievalPlan::v0_1(),
        bounds: OutputBounds::v0_1(),
        description: "Long-only US large caps, on public news and filings.".to_owned(),
        revisions_enabled: false,
        agentic: None,
    }
}

fn envelope() -> MandateEnvelope {
    MandateEnvelope {
        universe_pinned: false,
        max_instruments: 5,
        asset_classes: BTreeSet::from([mandate_research::AssetClass::UsEquity]),
        leveraged_etps_enabled: false,
        leveraged_etp_disclosure_version: None,
        admits_instruments: true,
        research: Some(ResearchEnvelope {
            interval_s: 3_600,
            cost_cap_usd_per_day: usd("5"),
            max_revisions_per_lineage: 3,
        }),
        admission: AutonomyDecision::Ask,
    }
}

fn facts() -> AdmissionFacts {
    AdmissionFacts {
        allowlist: SourceAllowlist {
            version: AllowlistVersion(3),
            sources: ["src.newswire", "src.wire_two"]
                .into_iter()
                .map(|source| SourceId::new(source).unwrap())
                .collect(),
        },
        eligibility_failures: BTreeSet::new(),
        instrument_groups: BTreeMap::new(),
        claimed_by_other_agents: BTreeSet::new(),
        halted_instruments: BTreeSet::new(),
        disclosures_accepted: BTreeSet::new(),
        data_universe: Some(
            [AAPL, MSFT]
                .iter()
                .map(|id| AssetId::new(id).unwrap())
                .collect(),
        ),
        research_spend_usd_today: usd("0"),
    }
}

/// A source that gives every pass the open research context, unless switched off, and counts the
/// passes it was asked.
#[derive(Clone, Default)]
struct Open {
    asked: Rc<Cell<u32>>,
    off: Rc<Cell<bool>>,
}

impl ResearchSource for Open {
    fn parts(&self, _: &RuntimeState) -> Option<ResearchParts> {
        self.asked.set(self.asked.get() + 1);
        if self.off.get() {
            return None;
        }
        Some(ResearchParts {
            preconditions: Preconditions::read(&JournalCatalog::JOURNAL),
            mandate: ValidatedMandate::from_validated_envelope(envelope()),
            overlay: PolicyOverlay::internal_research_profile(),
            universe: WorkingUniverse::Known {
                instruments: BTreeSet::new(),
                pinned: false,
            },
            facts: facts(),
            lineages: LineageState::default(),
            active: Vec::new(),
            last_reservation: None,
            data_symbols: BTreeSet::from([
                Symbol::new("AAPL").unwrap(),
                Symbol::new("MSFT").unwrap(),
            ]),
        })
    }
}

/// A worker that records the requests it is handed and answers what the test queued, counting
/// how often it was asked for answers.
#[derive(Clone, Default)]
struct Hands {
    handed: Rc<RefCell<Vec<ResearchRequest>>>,
    queued: Rc<RefCell<Vec<ResearchInput>>>,
    asked: Rc<Cell<u32>>,
}

impl ResearchWorker for Hands {
    fn hand(&mut self, request: &ResearchRequest, _: &ResearchParts) {
        self.handed.borrow_mut().push(request.clone());
    }
    fn answers(&mut self, _: RiskClock) -> Vec<ResearchInput> {
        self.asked.set(self.asked.get() + 1);
        self.queued.borrow_mut().drain(..).collect()
    }
}

/// An artifact store that keeps what it is given, logs each put, gives it back by its digest, and
/// can refuse.
#[derive(Clone, Default)]
struct Shelf {
    put: Rc<RefCell<Vec<Vec<u8>>>>,
    refuse: Rc<Cell<bool>>,
    log: Log,
}

impl ArtifactSink for Shelf {
    fn put(&mut self, bytes: &[u8]) -> bool {
        if self.refuse.get() {
            return false;
        }
        self.log.borrow_mut().push("put".to_owned());
        self.put.borrow_mut().push(bytes.to_vec());
        true
    }

    fn get(&self, digest: &Digest) -> Option<Vec<u8>> {
        self.put
            .borrow()
            .iter()
            .find(|bytes| Digest::of(bytes) == *digest)
            .cloned()
    }
}

/// What the store and the sink did, in order.
type Log = Rc<RefCell<Vec<String>>>;

/// The research a test wires, and the handles it reads back.
#[derive(Clone, Default)]
struct Wired {
    source: Open,
    worker: Hands,
    shelf: Shelf,
    agentic: bool,
}

impl Wired {
    /// An agentic entry's research, logging into the rig's log.
    fn agentic(rig: &Rig) -> Self {
        let mut wired = Self {
            agentic: true,
            ..Self::default()
        };
        wired.shelf.log = Rc::clone(&rig.log);
        wired
    }

    fn wiring(&self) -> ResearchWiring {
        let mut config = config_entry();
        if self.agentic {
            config.agentic = Some(agentic());
        }
        ResearchWiring {
            config,
            source: Box::new(self.source.clone()),
            worker: Box::new(self.worker.clone()),
            artifacts: Box::new(self.shelf.clone()),
        }
    }
}

fn wiring(research: Option<ResearchWiring>) -> Wiring {
    wiring_with(research, Box::new(MandateRef))
}

fn wiring_with(research: Option<ResearchWiring>, refs: Box<dyn Refs>) -> Wiring {
    Wiring {
        ids: Box::new(Ids),
        gate: Box::new(Deny),
        plan: Box::new(Nothing),
        flatten: Box::new(Empty),
        refs,
        view: MandateView {
            version: "v1".into(),
            working_universe: Default::default(),
            restricted_instruments: Default::default(),
            approval: ApprovalSettings {
                approvers: ["user-owner".to_owned()].into(),
                author: "user-author".into(),
                timeout_s: 300,
                environment: Environment::Paper,
                live_cli_confirm: None,
            },
        },
        research,
    }
}

fn config() -> Config {
    Config {
        deployment: Deployment {
            agent: AgentId(AGENT.into()),
            connection: ConnectionId("conn-1".into()),
            workspace: WorkspaceId(WORKSPACE.into()),
        },
        environment: Environment::Paper,
        liveness: Duration::from_secs(10),
    }
}

/// What the store answers the next research append: one whose records carry a run as their
/// `correlation_id`.
#[derive(Clone, Copy, Debug)]
enum Fault {
    /// Commits it, then answers `Unavailable`, as a connection lost after the commit would.
    LoseAnswer,
    /// Writes nothing and answers `Unavailable`.
    Down,
    Fenced,
    HeadMismatch,
}

/// The agent stream on the in-memory journal; the control, account and clock streams as laid-down
/// rows; a fault for the next research append; and a log of reads, appends, and artifact puts.
#[derive(Clone, Default)]
struct Rig {
    journal: MemoryStore,
    control: Rc<RefCell<Vec<StoredEvent>>>,
    account: Rc<RefCell<Vec<StoredEvent>>>,
    clock: Rc<RefCell<Vec<StoredEvent>>>,
    fault: Rc<Cell<Option<Fault>>>,
    log: Log,
}

impl Store for Rig {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.journal.take_ownership(stream)
    }
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.log
            .borrow_mut()
            .push(format!("read {}", stream.as_str()));
        match stream.as_str() {
            CONTROL => Some(self.control.borrow().clone()),
            CLOCK => Some(self.clock.borrow().clone()),
            ACCOUNT_STREAM => Some(self.account.borrow().clone()),
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
        let research = drafts
            .iter()
            .any(|draft| !String::from_utf8_lossy(draft).contains(r#""correlation_id":null"#));
        self.log.borrow_mut().push(if research {
            "append research".to_owned()
        } else {
            "append".to_owned()
        });
        let fault = if research { self.fault.take() } else { None };
        match fault {
            Some(Fault::Down) => AppendOutcome::Unavailable,
            Some(Fault::Fenced) => AppendOutcome::Fenced {
                current_epoch: epoch + 1,
            },
            Some(Fault::HeadMismatch) => AppendOutcome::HeadMismatch {
                actual_seq: head + 1,
                actual_hash: Digest::of(b"another writer"),
            },
            Some(Fault::LoseAnswer) => {
                self.journal
                    .append(stream, head, epoch, recorded_at, drafts);
                AppendOutcome::Unavailable
            }
            None => self
                .journal
                .append(stream, head, epoch, recorded_at, drafts),
        }
    }
}

fn row(stream: &str, seq: u64, event_type: &str, actor: &str, payload: &str) -> StoredEvent {
    versioned(stream, seq, event_type, 1, actor, payload)
}

fn versioned(
    stream: &str,
    seq: u64,
    event_type: &str,
    schema_version: u64,
    actor: &str,
    payload: &str,
) -> StoredEvent {
    let space = match stream {
        CONTROL => 'C',
        CLOCK => 'K',
        _ => 'A',
    };
    let event_id = format!("01J8Z3M{space}{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":{schema_version},"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"scheduler","causation_id":null,"correlation_id":null,"actor":{{"kind":"{actor}","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: stream.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version,
        environment: "paper".into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

fn kill_switch(rig: &Rig) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"agent":"{AGENT}","command":"kill_switch","scope":"agent","subject":"{AGENT}","bid":null,"bid_size":null,"floor":null,"submitted_at":{NOW},"step_up":null,"user":"user-owner"}}"#
    );
    rig.control
        .borrow_mut()
        .push(row(CONTROL, seq, "OwnerCommandIssued", "user", &payload));
}

fn tick(rig: &Rig, secs: i64) {
    let seq = u64::try_from(rig.clock.borrow().len()).unwrap() + 1;
    let payload = format!(r#"{{"clock":"{}"}}"#, at(secs));
    rig.clock
        .borrow_mut()
        .push(row(CLOCK, seq, "ClockAdvanced", "system", &payload));
}

fn kinds(rig: &Rig) -> Vec<String> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .iter()
        .map(|r| r.event_type.clone())
        .collect()
}

fn count(rig: &Rig, kind: &str) -> usize {
    kinds(rig).iter().filter(|k| *k == kind).count()
}

fn start(rig: &Rig, research: Option<ResearchWiring>) -> RuntimeProcess<Rig> {
    RuntimeProcess::start(rig.clone(), wiring(research), &config(), at(NOW))
}

fn keys(alerts: &[Alert]) -> Vec<&'static str> {
    alerts.iter().map(|a| a.key).collect()
}

/// A worker answer naming a run the core has not started.
fn stray() -> ResearchInput {
    ResearchInput::Retrieved {
        run_id: "01STRAYRUN".to_owned(),
        items: Vec::new(),
    }
}

/// DEC-2208 item 1: the binary's fixture wiring runs no research.
#[test]
fn the_fixture_wiring_runs_no_research() {
    assert!(fixture::wiring("user-owner").research.is_none());
}

/// DEC-2208 item 1: with no research wiring, the process journals exactly what it journals without
/// one, ticks and the kill switch included.
#[test]
fn no_research_wiring_journals_what_a_process_without_research_journals() {
    let plain = Rig::default();
    let wired = Rig::default();
    let wires = Wired::default();
    let mut without = start(&plain, None);
    let mut with = start(&wired, Some(wires.wiring()));
    for rig in [&plain, &wired] {
        tick(rig, NOW + 1);
        kill_switch(rig);
        tick(rig, NOW + 2);
    }
    without.step(at(NOW + 1));
    with.step(at(NOW + 1));
    without.step(at(NOW + 2));
    with.step(at(NOW + 2));
    assert_eq!(kinds(&plain), kinds(&wired));
    assert_eq!(count(&wired, "KillSwitchActivated"), 1);
}

/// DEC-2208 item 3, DEC-1012 item 4: until the core's risk clock is set, the research pass asks
/// the worker nothing and hands nothing.
#[test]
fn no_tick_asks_the_worker_nothing() {
    let rig = Rig::default();
    let wires = Wired::default();
    let mut process = start(&rig, Some(wires.wiring()));
    wires.worker.queued.borrow_mut().push(stray());
    let alerts = process.step(at(NOW + 1));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert_eq!(wires.worker.asked.get(), 0);
    assert_eq!(wires.source.asked.get(), 0);
    assert_eq!(wires.worker.queued.borrow().len(), 1, "the answer waits");
}

/// The core's own gate: a runtime that has folded no clean reconciliation is paused, so a due tick
/// starts no research and the worker is handed nothing (research spec §5.3: research runs in
/// `normal` only, or `exits_only` for an agentic monitor).
#[test]
fn a_paused_runtime_starts_no_research_however_often_it_ticks() {
    let rig = Rig::default();
    let wires = Wired::default();
    let mut process = start(&rig, Some(wires.wiring()));
    let before = kinds(&rig);
    for second in 1..=3 {
        tick(&rig, NOW + second * 3_600);
        let alerts = process.step(at(NOW + second * 3_600));
        assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    }
    assert_eq!(process.state().effective_mode(), Mode::Paused);
    assert!(wires.source.asked.get() >= 3, "each pass reads the context");
    assert!(wires.worker.handed.borrow().is_empty());
    assert!(wires.shelf.put.borrow().is_empty());
    assert_eq!(kinds(&rig), before, "nothing journaled");
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 7: an answer the core refuses is dropped with an alert, the next pass does not
/// hand it again, and it stops nothing: the kill switch after it is journaled.
#[test]
fn a_refused_worker_answer_is_dropped_with_an_alert_and_stops_nothing() {
    let rig = Rig::default();
    let wires = Wired::default();
    let mut process = start(&rig, Some(wires.wiring()));
    tick(&rig, NOW + 1);
    wires.worker.queued.borrow_mut().extend([stray(), stray()]);
    let alerts = process.step(at(NOW + 1));
    assert_eq!(
        keys(&alerts),
        ["research_input_refused", "research_input_refused"],
        "each answer once, in arrival order"
    );
    assert_eq!(wires.worker.asked.get(), 1);
    let alerts = process.step(at(NOW + 2));
    assert!(alerts.is_empty(), "dropped, not kept: {:?}", keys(&alerts));
    kill_switch(&rig);
    process.step(at(NOW + 3));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// A source with nothing to give runs no research on that pass, and the worker's answers wait for
/// one that has: the answer is handed once the context is back, and only then.
#[test]
fn a_source_with_nothing_to_give_keeps_the_worker_s_answers_waiting() {
    struct Sometimes(Rc<Cell<bool>>);
    impl ResearchSource for Sometimes {
        fn parts(&self, state: &RuntimeState) -> Option<ResearchParts> {
            if self.0.get() {
                Open::default().parts(state)
            } else {
                None
            }
        }
    }
    let rig = Rig::default();
    let wires = Wired::default();
    let given = Rc::new(Cell::new(false));
    let mut research = wires.wiring();
    research.source = Box::new(Sometimes(Rc::clone(&given)));
    let mut process = start(&rig, Some(research));
    tick(&rig, NOW + 1);
    wires.worker.queued.borrow_mut().push(stray());
    for second in 1..=2 {
        let alerts = process.step(at(NOW + second));
        assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    }
    given.set(true);
    let alerts = process.step(at(NOW + 3));
    assert_eq!(keys(&alerts), ["research_input_refused"]);
    let alerts = process.step(at(NOW + 4));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// The control stream's version-2 `ConnectionEstablished` of the deployment's connection, which names
/// the account the process follows (DEC-1014 item 3).
fn established(rig: &Rig) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"connection_id":"conn-1","broker":"alpaca","environment":"paper","scopes":[],"account_ref":"{ACCOUNT}"}}"#
    );
    rig.control.borrow_mut().push(versioned(
        CONTROL,
        seq,
        "ConnectionEstablished",
        2,
        "system",
        &payload,
    ));
}

/// The executor's clean reconciliation of the deployment's account, so the core leaves its startup
/// hold (DEC-1014).
fn reconciled(rig: &Rig) {
    let seq = u64::try_from(rig.account.borrow().len()).unwrap() + 1;
    let payload = format!(r#"{{"result":"clean","risk_clock":{NOW}}}"#);
    rig.account.borrow_mut().push(row(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        "system",
        &payload,
    ));
}

/// A reconciled process with an agentic entry's research, and the references its records need.
fn running(rig: &Rig, wired: &Wired) -> RuntimeProcess<Rig> {
    reconciled(rig);
    running_with(rig, wired, Box::new(RunRefs))
}

fn running_with(rig: &Rig, wired: &Wired, refs: Box<dyn Refs>) -> RuntimeProcess<Rig> {
    established(rig);
    RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(wired.wiring()), refs),
        &config(),
        at(NOW),
    )
}

/// The run ids of the starts the worker was handed.
fn starts(wired: &Wired) -> Vec<String> {
    wired
        .worker
        .handed
        .borrow()
        .iter()
        .filter_map(|request| match request {
            ResearchRequest::Start { run_id, .. } => Some(run_id.clone()),
            _ => None,
        })
        .collect()
}

/// The stored `RunStarted` rows' bodies.
fn run_started(rig: &Rig) -> Vec<Value> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .iter()
        .filter(|row| row.event_type == "RunStarted")
        .map(|row| parse(&row.body).unwrap())
        .collect()
}

/// DEC-2208 items 3 and 5, DEC-2206 items 3 and 4: a reconciled runtime's due tick starts an
/// agentic run. The data universe is stored before the batch that names it, `RunStarted` carries the
/// run as its `correlation_id`, and only then is the worker handed the start.
#[test]
fn a_due_tick_journals_run_started_under_its_run_with_the_universe_stored_first() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    rig.log.borrow_mut().clear();
    let alerts = process.step(at(NOW + 60));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    let started = run_started(&rig);
    assert_eq!(started.len(), 1, "{:?}", kinds(&rig));
    let run_ids = starts(&wired);
    assert_eq!(run_ids.len(), 1);
    assert_eq!(
        started[0].get("correlation_id").and_then(Value::as_str),
        Some(run_ids[0].as_str())
    );
    let universe_ref = started[0]
        .get("payload")
        .and_then(|p| p.get("data_universe_ref"))
        .and_then(Value::as_str)
        .unwrap()
        .to_owned();
    assert!(
        wired
            .shelf
            .put
            .borrow()
            .iter()
            .any(|bytes| format!("sha256:{}", Digest::of(bytes)) == universe_ref),
        "the universe the record names was stored"
    );
    let log = rig.log.borrow().clone();
    let put = log.iter().position(|l| l == "put").unwrap();
    let append = log.iter().position(|l| l == "append research").unwrap();
    assert!(put < append, "{log:?}");
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 5: an artifact the sink cannot store appends nothing and halts research with an
/// alert, and holds nothing else: the kill switch laid in the same pass is journaled.
#[test]
fn an_artifact_the_sink_cannot_store_halts_research_and_holds_nothing_else() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    wired.shelf.refuse.set(true);
    tick(&rig, NOW + 60);
    let alerts = process.step(at(NOW + 60));
    assert_eq!(keys(&alerts), ["research_halted"]);
    assert!(run_started(&rig).is_empty());
    assert!(starts(&wired).is_empty());
    wired.shelf.refuse.set(false);
    kill_switch(&rig);
    tick(&rig, NOW + 7_200);
    process.step(at(NOW + 7_200));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert!(run_started(&rig).is_empty(), "research stays halted");
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 4, journal spec §5.1: a research batch whose answer was lost is in doubt; the next
/// pass offers it again before reading the control stream, it is written once, the worker is handed
/// the start only then, and the kill switch follows in the same pass.
#[test]
fn a_research_batch_in_doubt_is_retried_first_and_written_once() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert_eq!(run_started(&rig).len(), 1, "committed");
    assert!(starts(&wired).is_empty(), "but not known to be");
    kill_switch(&rig);
    rig.log.borrow_mut().clear();
    process.step(at(NOW + 61));
    let log = rig.log.borrow().clone();
    let retried = log.iter().position(|l| l == "append research").unwrap();
    let control = log.iter().position(|l| l == "read ctl:ws1").unwrap();
    assert!(retried < control, "{log:?}");
    assert_eq!(run_started(&rig).len(), 1, "written once");
    assert_eq!(starts(&wired).len(), 1);
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
}

/// DEC-2208 item 6: a research batch the journal refuses as invalid halts research with an alert,
/// and trading goes on: the kill switch is journaled, and no later tick starts a run.
#[test]
fn an_invalid_research_batch_halts_research_and_the_kill_switch_goes_on() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    reconciled(&rig);
    let mut process = running_with(&rig, &wired, Box::new(MandateRef));
    tick(&rig, NOW + 60);
    let alerts = process.step(at(NOW + 60));
    assert_eq!(keys(&alerts), ["research_halted"], "{:?}", kinds(&rig));
    kill_switch(&rig);
    tick(&rig, NOW + 7_200);
    process.step(at(NOW + 7_200));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert!(run_started(&rig).is_empty());
    assert!(starts(&wired).is_empty());
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 6: a research append fenced by a newer writer stops the process `Fenced`.
#[test]
fn a_fenced_research_append_stops_the_process_fenced() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::Fenced));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert_eq!(process.lifecycle().state(), State::Exited(Exit::Fenced));
    assert!(starts(&wired).is_empty());
}

/// DEC-2208 item 6: a research append whose head no longer matches stops the process `Failed`.
#[test]
fn a_research_append_at_a_stale_head_stops_the_process() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::HeadMismatch));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    assert!(starts(&wired).is_empty());
}

/// DEC-2208 item 3: the worker's seed for the run reaches the core on a later pass, and the core's
/// next request, the first turn, is handed after its batch.
#[test]
fn the_worker_s_seed_reaches_the_core_and_the_first_turn_follows() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    let run_id = starts(&wired).pop().unwrap();
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Retrieved {
            run_id,
            items: Vec::new(),
        });
    let alerts = process.step(at(NOW + 61));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert!(
        wired
            .worker
            .handed
            .borrow()
            .iter()
            .any(|request| matches!(request, ResearchRequest::Turn(_))),
        "{:?}",
        wired.worker.handed.borrow()
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 4: the retry of a research batch in doubt offers the core the same input, which it
/// answers from the batch it remembers, so a source with nothing to give on that pass holds nothing:
/// the batch is settled and the kill switch is journaled in the same pass.
#[test]
fn a_source_with_nothing_never_holds_the_retry_of_a_batch_in_doubt() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    assert!(starts(&wired).is_empty(), "in doubt");
    wired.source.off.set(true);
    kill_switch(&rig);
    process.step(at(NOW + 61));
    assert_eq!(starts(&wired).len(), 1, "settled");
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(run_started(&rig).len(), 1);
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 5: an artifact the sink cannot store on the retry of a batch that may already be
/// committed leaves the batch in doubt rather than abandoning it, so the core's head never goes
/// stale; once the sink is back the batch settles, written once, and the process stays ready.
#[test]
fn an_unstored_artifact_on_a_retry_keeps_the_batch_in_doubt() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    wired.shelf.refuse.set(true);
    let alerts = process.step(at(NOW + 61));
    assert!(alerts.is_empty(), "not halted: {:?}", keys(&alerts));
    assert!(starts(&wired).is_empty(), "still in doubt");
    wired.shelf.refuse.set(false);
    kill_switch(&rig);
    process.step(at(NOW + 62));
    assert_eq!(starts(&wired).len(), 1, "settled");
    assert_eq!(run_started(&rig).len(), 1, "written once");
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// DEC-2208 item 3: an answer queued behind a batch that goes into doubt waits in the inbox, and is
/// handed once the batch settles, on the next pass, never dropped.
#[test]
fn an_answer_queued_behind_a_batch_in_doubt_waits_for_it() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Retrieved {
            run_id: "01J9ZZZZZZZZZZZZZZZZZZZZZY".to_owned(),
            items: Vec::new(),
        });
    tick(&rig, NOW + 60);
    let alerts = process.step(at(NOW + 60));
    assert!(alerts.is_empty(), "behind the doubt: {:?}", keys(&alerts));
    let alerts = process.step(at(NOW + 61));
    assert_eq!(
        keys(&alerts),
        ["research_input_refused"],
        "handed once settled"
    );
    assert_eq!(starts(&wired).len(), 1);
    let alerts = process.step(at(NOW + 62));
    assert!(alerts.is_empty(), "handed once: {:?}", keys(&alerts));
}

/// DEC-2208 item 4, journal spec §5.1: a research batch still in doubt after its retry holds every
/// other input, the kill switch among them, since the agent stream has one writer; the process stays
/// ready, and once the journal answers the batch settles and the kill switch follows.
#[test]
fn a_research_batch_still_in_doubt_holds_the_pass_until_it_settles() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    rig.fault.set(Some(Fault::LoseAnswer));
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    rig.fault.set(Some(Fault::Down));
    kill_switch(&rig);
    process.step(at(NOW + 61));
    assert_eq!(
        count(&rig, "KillSwitchActivated"),
        0,
        "held behind the doubt"
    );
    assert_eq!(process.lifecycle().state(), State::Ready);
    process.step(at(NOW + 62));
    assert_eq!(starts(&wired).len(), 1);
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// The gateway's record of a turn's call that ended `provider_unavailable`, with no output.
fn unavailable(turn: &TurnRequest) -> CallRecord {
    let plan = &turn.call;
    let prompt = b"the turn's prompt".to_vec();
    CallRecord {
        call_id: plan.call_id.clone(),
        agent_id: Some(plan.agent_id.clone()),
        purpose: Purpose::Research,
        model: plan.model.clone(),
        endpoint: "openrouter:anthropic".to_owned(),
        request_digest: Some(Digest::of(b"the turn's request")),
        guard_rule: None,
        seed: None,
        prompt_ref: Some(Digest::of(&prompt)),
        response_ref: None,
        reported_identity: None,
        provider_request_id: None,
        outcome: CallOutcome::ProviderUnavailable,
        attempts: 1,
        tokens: Usage {
            input: 0,
            output: 0,
            cached: 0,
        },
        cost: Usd6::from_micros(0),
        price_table_ref: Digest::of(b"the price table"),
        cache_hit: false,
        deadline: plan.as_of,
        completed_at: plan.as_of,
        reservation: Some("01J8Z3MR000000000000000001".to_owned()),
        observations: plan.observations.clone(),
        output: None,
        tool_calls: Vec::new(),
        prompt: Some(prompt),
        response: None,
    }
}

/// DEC-2208 item 3: an answer whose own batch goes into doubt is the driver's to retry, never the
/// inbox's to hand again: once its retry settles, the run has ended once and nothing is refused.
#[test]
fn an_answer_whose_batch_goes_into_doubt_is_retried_never_handed_twice() {
    let rig = Rig::default();
    let wired = Wired::agentic(&rig);
    let mut process = running(&rig, &wired);
    tick(&rig, NOW + 60);
    process.step(at(NOW + 60));
    let run_id = starts(&wired).pop().unwrap();
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Retrieved {
            run_id: run_id.clone(),
            items: Vec::new(),
        });
    process.step(at(NOW + 61));
    let turn = wired
        .worker
        .handed
        .borrow()
        .iter()
        .find_map(|request| match request {
            ResearchRequest::Turn(turn) => Some(turn.clone()),
            _ => None,
        })
        .unwrap();
    rig.fault.set(Some(Fault::LoseAnswer));
    wired
        .worker
        .queued
        .borrow_mut()
        .push(ResearchInput::Called {
            run_id,
            at: RiskClock::from_secs(NOW + 60),
            record: Box::new(unavailable(&turn)),
        });
    let alerts = process.step(at(NOW + 62));
    assert!(alerts.is_empty(), "{:?}", keys(&alerts));
    assert_eq!(count(&rig, "RunEnded"), 1, "committed: {:?}", kinds(&rig));
    for second in 63..66 {
        let alerts = process.step(at(NOW + second));
        assert!(alerts.is_empty(), "handed once: {:?}", keys(&alerts));
    }
    assert_eq!(count(&rig, "RunEnded"), 1, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// The references a run over the recorded bundle carries: the mandate version, and the bundle's
/// research entry as the model version its records name.
struct BundleRefs(Digest);
impl Refs for BundleRefs {
    fn refs(&self, _: &EventDraft) -> Option<(u64, Object)> {
        let mut refs = Object::new();
        refs.insert(
            Key::new("mandate_version").ok()?,
            Value::Str(format!(
                "sha256:{}",
                Digest::of(b"the mandate version").to_hex()
            )),
        );
        refs.insert(
            Key::new("model_version").ok()?,
            Value::Str(format!("sha256:{}", self.0.to_hex())),
        );
        Some((1, refs))
    }
}

/// DEC-2210 item 3 and `AGENTS.md` rule 4: the cell's worker, wired into the process, takes a run
/// through the recorded fake to its judgment, and nothing the model said reaches the order path. The
/// run journals its call and the model's thesis as a proposal, and no stream holds an order event.
#[test]
fn the_recorded_worker_s_answers_never_reach_the_order_path() {
    let rig = Rig::default();
    reconciled(&rig);
    established(&rig);
    let bundle = Bundle::load(std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/research"
    )))
    .unwrap();
    let pin = bundle.entry.content().model_ref();
    let mut entry = config_entry();
    entry.pin = pin.clone();
    let clock = Rc::new(Cell::new(NOW));
    let read = Rc::clone(&clock);
    let who = WorkerFor {
        workspace: WORKSPACE.to_owned(),
        agent: AGENT.to_owned(),
        description: entry.description.clone(),
    };
    let worker = RecordedWorker::new(
        &bundle,
        who,
        Box::new(Shelf::default()),
        Box::new(move || at(read.get())),
    )
    .unwrap();
    let research = ResearchWiring {
        config: entry,
        source: Box::new(Open::default()),
        worker: Box::new(worker),
        artifacts: Box::new(Shelf::default()),
    };
    let mut process = RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(research), Box::new(BundleRefs(pin.content_hash))),
        &config(),
        at(NOW),
    );
    for second in 60..70 {
        tick(&rig, NOW + second);
        clock.set(NOW + second);
        process.step(at(NOW + second));
    }
    assert_eq!(
        count(&rig, "ModelInvocationRecorded"),
        1,
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(count(&rig, "ThesisProposed"), 1, "{:?}", kinds(&rig));
    for stream in [AGENT_STREAM, ACCOUNT_STREAM, CONTROL] {
        let rows = rig.journal.rows(&StreamId::parse(stream).unwrap()).unwrap();
        let orders: Vec<_> = rows
            .iter()
            .map(|row| row.event_type.as_str())
            .filter(|kind| kind.starts_with("Order"))
            .collect();
        assert!(orders.is_empty(), "{stream}: {orders:?}");
    }
}

/// The daemon's agentic fixture bundle (DEC-2266, DEC-2268).
const AGENTIC_BUNDLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/research-agentic"
);

/// The agentic fixture's scripted calls, counted from its `research.json` here rather than through
/// the loader: one per call of every scripted turn.
fn scripted_calls() -> (usize, usize) {
    let file: serde_json::Value = serde_json::from_slice(
        &std::fs::read(Path::new(AGENTIC_BUNDLE).join("research.json")).unwrap(),
    )
    .unwrap();
    let turns = file["turns"].as_array().unwrap();
    let calls = turns
        .iter()
        .map(|turn| turn["calls"].as_array().unwrap().len())
        .sum();
    (turns.len(), calls)
}

/// DEC-2268 item 5, agent harness spec §6.9 and HI-24, and `AGENTS.md` rule 4: the cell's worker
/// over an agentic bundle, wired into the process with the bundle's loop, runs the scripted turns
/// to the final output. Every turn is journaled: each reply's `ModelInvocationRecorded` heads its
/// calls' `ToolCallRecorded`, each call's `ToolResultRecorded` follows before the next turn, and the
/// run ends `done` after the final reply is judged. No stream holds an order event.
#[test]
fn an_agentic_run_journals_every_turn_through_to_its_end() {
    let rig = Rig::default();
    reconciled(&rig);
    established(&rig);
    let bundle = Bundle::load(Path::new(AGENTIC_BUNDLE)).unwrap();
    let pin = bundle.entry.content().model_ref();
    let mut entry = config_entry();
    entry.pin = pin.clone();
    entry.agentic = Some(cell::agentic_config(&bundle).unwrap_or_else(|e| panic!("{e:?}")));
    let clock = Rc::new(Cell::new(NOW));
    let read = Rc::clone(&clock);
    let who = WorkerFor {
        workspace: WORKSPACE.to_owned(),
        agent: AGENT.to_owned(),
        description: entry.description.clone(),
    };
    let shelf = Shelf::default();
    let worker = RecordedWorker::new(
        &bundle,
        who,
        Box::new(shelf.clone()),
        Box::new(move || at(read.get())),
    )
    .unwrap();
    let research = ResearchWiring {
        config: entry,
        source: Box::new(Open::default()),
        worker: Box::new(worker),
        artifacts: Box::new(shelf),
    };
    let mut process = RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(research), Box::new(BundleRefs(pin.content_hash))),
        &config(),
        at(NOW),
    );
    for second in 60..90 {
        tick(&rig, NOW + second);
        clock.set(NOW + second);
        process.step(at(NOW + second));
    }
    let (scripted, calls) = scripted_calls();
    let journaled = kinds(&rig);
    let run: Vec<&str> = journaled
        .iter()
        .map(String::as_str)
        .filter(|kind| {
            matches!(
                *kind,
                "RunStarted"
                    | "ModelInvocationRecorded"
                    | "ToolCallRecorded"
                    | "ToolResultRecorded"
                    | "RunSummaryRecorded"
                    | "RunEnded"
            )
        })
        .collect();
    assert_eq!(run.first(), Some(&"RunStarted"), "{journaled:?}");
    assert_eq!(
        count(&rig, "ModelInvocationRecorded"),
        scripted + 1,
        "every scripted turn and the final one: {journaled:?}"
    );
    assert_eq!(count(&rig, "ToolCallRecorded"), calls, "{journaled:?}");
    assert_eq!(count(&rig, "ToolResultRecorded"), calls, "{journaled:?}");
    let invocations: Vec<usize> = run
        .iter()
        .enumerate()
        .filter(|(_, kind)| **kind == "ModelInvocationRecorded")
        .map(|(index, _)| index)
        .collect();
    for pair in invocations.windows(2) {
        let between = &run[pair[0] + 1..pair[1]];
        let asked = between.iter().filter(|k| **k == "ToolCallRecorded").count();
        let answered = between
            .iter()
            .filter(|k| **k == "ToolResultRecorded")
            .count();
        assert!(asked > 0, "a scripted turn asks for calls: {run:?}");
        assert_eq!(
            asked, answered,
            "every call answered before the next turn: {run:?}"
        );
    }
    assert_eq!(
        run[run.len() - 2..],
        ["RunSummaryRecorded", "RunEnded"],
        "{run:?}"
    );
    let ended = rig
        .journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .into_iter()
        .rfind(|row| row.event_type == "RunEnded")
        .unwrap();
    let body = parse(&ended.body).unwrap();
    assert_eq!(
        body.get("payload")
            .and_then(|payload| payload.get("reason"))
            .and_then(Value::as_str),
        Some("done"),
        "{body:?}"
    );
    for stream in [AGENT_STREAM, ACCOUNT_STREAM, CONTROL] {
        let rows = rig.journal.rows(&StreamId::parse(stream).unwrap()).unwrap();
        let orders: Vec<_> = rows
            .iter()
            .map(|row| row.event_type.as_str())
            .filter(|kind| kind.starts_with("Order"))
            .collect();
        assert!(orders.is_empty(), "{stream}: {orders:?}");
    }
}

/// The cell's demo research bundle, which every runtime pod is handed when the cell sets
/// `runtime.research` (DEC-2246 item 4).
const DEMO_BUNDLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../deploy/cell/demo/research"
);
/// The demo's own agent's mandate, as its owner confirms it (`deploy/cell/demo/mandate.json`).
const DEMO_MANDATE: &str = include_str!("../../../deploy/cell/demo/mandate.json");

/// The demo's mandate, edited by `edit`, stored in an artifact store of its own for test `name`
/// and deployed for the agent on the rig's control stream; and the research the cell's opt-in
/// wires over the demo bundle for that deployment.
fn opted_in(rig: &Rig, name: &str, edit: impl FnOnce(&mut serde_json::Value)) -> CellResearch {
    let root = std::env::temp_dir().join(format!("daemon-{}-demo-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut store = FsArtifactStore::open(&root).unwrap();
    let mut document: serde_json::Value = serde_json::from_str(DEMO_MANDATE).unwrap();
    edit(&mut document);
    let mandate = Mandate::parse(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
    let version = store
        .put_artifact(&mandate.canonical_bytes().unwrap())
        .unwrap()
        .to_string();
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"agent_id":"{AGENT}","mandate_version":"{version}","record_ref":"{version}"}}"#
    );
    rig.control
        .borrow_mut()
        .push(row(CONTROL, seq, "AgentDeployed", "user", &payload));
    let deployment = CellDeployment {
        workspace: WORKSPACE.to_owned(),
        agent: AGENT.to_owned(),
        environment: Environment::Paper,
        resolved: true,
    };
    let opted = opt_in(Path::new(DEMO_BUNDLE), &deployment).unwrap_or_else(|e| panic!("{e:?}"));
    cell::research(opted, Box::new(rig.clone()), &root, Box::new(|| at(NOW)))
        .unwrap_or_else(|e| panic!("{e:?}"))
}

/// A worker that counts the runs it is handed, then hands every request on.
struct Counted {
    inner: Box<dyn ResearchWorker>,
    starts: Rc<Cell<usize>>,
}

impl ResearchWorker for Counted {
    fn hand(&mut self, request: &ResearchRequest, parts: &ResearchParts) {
        if matches!(request, ResearchRequest::Start { .. }) {
            self.starts.set(self.starts.get() + 1);
        }
        self.inner.hand(request, parts);
    }

    fn answers(&mut self, at: RiskClock) -> Vec<ResearchInput> {
        self.inner.answers(at)
    }
}

/// The runs a reconciled process starts, and the research records it journals, over a day of
/// hourly ticks with `research` wired: past every interval its envelope could set, each tick
/// stepped until the worker's answers are in.
fn a_day_of_research(rig: &Rig, research: CellResearch) -> (usize, usize) {
    reconciled(rig);
    established(rig);
    let starts = Rc::new(Cell::new(0));
    let mut wiring = research.wiring;
    wiring.worker = Box::new(Counted {
        inner: wiring.worker,
        starts: Rc::clone(&starts),
    });
    let mut process = RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(wiring), research.refs),
        &config(),
        at(NOW),
    );
    for hour in 1..=24 {
        tick(rig, NOW + hour * 3_600);
        for _ in 0..4 {
            process.step(at(NOW + hour * 3_600));
        }
    }
    let records = rig
        .log
        .borrow()
        .iter()
        .filter(|line| *line == "append research")
        .count();
    (starts.get(), records)
}

/// DEC-2246 item 4: the flag only makes the recorded bundle available; the confirmed envelope
/// decides research. `demo-spy`'s mandate pins its universe and has no `behavior.research`, so
/// opted in over the demo bundle it starts no run and journals no research record, so nothing is
/// called, metered or spent, however long it runs. The same opt-in under an envelope that
/// researches (unpinned, a `behavior.research`, and one `llm.` model admitting instruments, as
/// V-036 asks) starts runs and journals their records, so the first answer is the envelope's, not
/// the harness's.
#[test]
fn the_demo_agent_opted_in_runs_no_research_and_spends_nothing() {
    let rig = Rig::default();
    let research = opted_in(&rig, "spy", |_| {});
    assert_eq!(
        a_day_of_research(&rig, research),
        (0, 0),
        "{:?}",
        kinds(&rig)
    );
    let researching = Rig::default();
    let research = opted_in(&researching, "researching", |document| {
        document["universe"]["pinned"] = serde_json::json!(false);
        document["universe"]["pinned_instruments"] = serde_json::json!([]);
        document["behavior"]["signal_models"]
            .as_array_mut()
            .unwrap()
            .push(serde_json::json!({
                "id": "llm.research_agent",
                "version": "0.1.0",
                "content_hash": format!("sha256:{}", "4".repeat(64)),
                "params": [],
                "weight": "0.5",
                "max_output_age_s": 3600,
                "admits_instruments": true
            }));
        document["behavior"]["research"] = serde_json::json!({
            "interval_s": 3600,
            "cost_cap_usd_per_day": "5",
            "max_revisions_per_lineage": 3
        });
    });
    let (starts, records) = a_day_of_research(&researching, research);
    assert!(
        starts > 0 && records > 0,
        "{starts} {records} {:?}",
        kinds(&researching)
    );
}
