//! The runtime-process rig research tests run on (DEC-2208): the in-memory journal and its
//! streams, the fixture wiring with a deny-all dry-run gate, a recording worker and sink, and the
//! cell's bundles. Shared by `tests/research.rs` and the daemon's own bench (DEC-2271).

#![allow(dead_code, reason = "each user takes a different subset")]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;
use std::time::Duration;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Key, Object, Value, parse};
use mandate_daemon::cell::{self, CellDeployment, CellResearch, opt_in};
use mandate_daemon::{
    Alert, ArtifactSink, Config, Refs, ResearchParts, ResearchSource, ResearchWiring,
    ResearchWorker, RuntimeProcess, Wiring,
};
use mandate_gateway::money::{Usage, Usd6};
use mandate_gateway::{CallRecord, Outcome as CallOutcome, Purpose};
use mandate_journal::{AppendOutcome, ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_num::Usd;
use mandate_research::{
    AdmissionFacts, AllowlistVersion, AssetId, AutonomyDecision, LineageState, MandateEnvelope,
    PolicyOverlay, ResearchEnvelope, SourceAllowlist, SourceId, ValidatedMandate, WorkingUniverse,
};
use mandate_research_run::agentic::{LoopBounds, OutputMode, Tool};
use mandate_research_run::{ModelPin, OutputBounds, RetrievalPlan, RetrievedItem, Symbol};
use mandate_runtime::{
    ActiveThesis, AgentId, AgenticConfig, ApprovalSettings, Autonomy, Classified, ConnectionId,
    Deployment, DryRunVerdict, EventDraft, EventId, EventTriggers, FlattenPlan, FlattenPlanner,
    FlattenRequest, GateDryRun, IdGen, JournalCatalog, MandateView, OrderPlan, Preconditions,
    Proposal, ResearchConfig, ResearchInput, ResearchRequest, RiskClock, RuntimeState, Seq,
    SignalInputs, TurnRequest, WorkspaceId, WriterEpoch,
};
use mandate_spec::document::Mandate;
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;

pub const WORKSPACE: &str = "ws1";
pub const AGENT: &str = "agent-a";
pub const AGENT_STREAM: &str = "agent:ws1:agent-a";
pub const CONTROL: &str = "ctl:ws1";
pub const CLOCK: &str = "clock:ws1";
pub const ACCOUNT: &str = "acct-1";
pub const ACCOUNT_STREAM: &str = "acct:ws1:acct-1";
pub const AAPL: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
pub const MSFT: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";
pub const NOW: i64 = 1_792_155_600;

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

/// ULID-shaped ids, as every journal id is (journal spec §3): a run's id is one too, and its
/// records carry it as their `correlation_id`. `0`, the epoch in 4 places, the head in 14, and the
/// ordinal in 7 of Crockford's base 32, which holds every `u32`.
pub struct Ids;
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

pub struct Deny;
impl GateDryRun for Deny {
    fn check(&self, _: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Deny {
            reason_code: "fixture".into(),
        }
    }
}

pub struct Nothing;
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

pub struct Empty;
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

pub struct MandateRef;
impl Refs for MandateRef {
    fn refs(&self, _: &EventDraft) -> Option<(u64, Object)> {
        let version = format!("sha256:{}", Digest::of(b"the mandate version").to_hex());
        let mut refs = Object::new();
        refs.insert(Key::new("mandate_version").ok()?, Value::Str(version));
        Some((1, refs))
    }
}

/// The research entry's agentic configuration: `final_output`, with the v0.2 bounds.
pub fn agentic() -> AgenticConfig {
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
pub struct RunRefs;
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

pub fn usd(text: &str) -> Usd {
    Usd::parse(text).unwrap_or_else(|e| panic!("`{text}` is USD: {e}"))
}

pub fn config_entry() -> ResearchConfig {
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

pub fn envelope() -> MandateEnvelope {
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

pub fn facts() -> AdmissionFacts {
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
pub struct Open {
    pub asked: Rc<Cell<u32>>,
    pub off: Rc<Cell<bool>>,
    /// Event triggers, the items arrived, and the active theses each pass gives, when set.
    pub evented: Rc<RefCell<Option<Evented>>>,
}

/// What an evented pass adds to the open context (DEC-2392).
#[derive(Clone)]
pub struct Evented {
    pub triggers: Option<EventTriggers>,
    pub events: Vec<RetrievedItem>,
    pub active: Vec<ActiveThesis>,
}

impl ResearchSource for Open {
    fn parts(&self, _: &RuntimeState) -> Option<ResearchParts> {
        self.asked.set(self.asked.get() + 1);
        if self.off.get() {
            return None;
        }
        let evented = self.evented.borrow().clone();
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
            active: evented
                .as_ref()
                .map(|e| e.active.clone())
                .unwrap_or_default(),
            last_reservation: None,
            data_symbols: BTreeSet::from([
                Symbol::new("AAPL").unwrap(),
                Symbol::new("MSFT").unwrap(),
            ]),
            event_triggers: evented.as_ref().and_then(|e| e.triggers.clone()),
            events: evented.map(|e| e.events).unwrap_or_default(),
        })
    }
}

/// A worker that records the requests it is handed and answers what the test queued, counting
/// how often it was asked for answers.
#[derive(Clone, Default)]
pub struct Hands {
    pub handed: Rc<RefCell<Vec<ResearchRequest>>>,
    pub queued: Rc<RefCell<Vec<ResearchInput>>>,
    pub asked: Rc<Cell<u32>>,
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
pub struct Shelf {
    pub put: Rc<RefCell<Vec<Vec<u8>>>>,
    pub refuse: Rc<Cell<bool>>,
    pub log: Log,
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
pub type Log = Rc<RefCell<Vec<String>>>;

/// The research a test wires, and the handles it reads back.
#[derive(Clone, Default)]
pub struct Wired {
    pub source: Open,
    pub worker: Hands,
    pub shelf: Shelf,
    pub agentic: bool,
}

impl Wired {
    /// An agentic entry's research, logging into the rig's log.
    pub fn agentic(rig: &Rig) -> Self {
        let mut wired = Self {
            agentic: true,
            ..Self::default()
        };
        wired.shelf.log = Rc::clone(&rig.log);
        wired
    }

    pub fn wiring(&self) -> ResearchWiring {
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

pub fn wiring(research: Option<ResearchWiring>) -> Wiring {
    wiring_with(research, Box::new(MandateRef))
}

pub fn wiring_with(research: Option<ResearchWiring>, refs: Box<dyn Refs>) -> Wiring {
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
        models: None,
        facts: None,
        approvers: None,
    }
}

pub fn config() -> Config {
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
pub enum Fault {
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
pub struct Rig {
    pub journal: MemoryStore,
    pub control: Rc<RefCell<Vec<StoredEvent>>>,
    pub account: Rc<RefCell<Vec<StoredEvent>>>,
    pub clock: Rc<RefCell<Vec<StoredEvent>>>,
    pub fault: Rc<Cell<Option<Fault>>>,
    pub log: Log,
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

pub fn row(stream: &str, seq: u64, event_type: &str, actor: &str, payload: &str) -> StoredEvent {
    versioned(stream, seq, event_type, 1, actor, payload)
}

pub fn versioned(
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

pub fn kill_switch(rig: &Rig) {
    let seq = u64::try_from(rig.control.borrow().len()).unwrap() + 1;
    let payload = format!(
        r#"{{"agent":"{AGENT}","command":"kill_switch","scope":"agent","subject":"{AGENT}","bid":null,"bid_size":null,"floor":null,"submitted_at":{NOW},"step_up":null,"user":"user-owner"}}"#
    );
    rig.control
        .borrow_mut()
        .push(row(CONTROL, seq, "OwnerCommandIssued", "user", &payload));
}

pub fn tick(rig: &Rig, secs: i64) {
    let seq = u64::try_from(rig.clock.borrow().len()).unwrap() + 1;
    let payload = format!(r#"{{"clock":"{}"}}"#, at(secs));
    rig.clock
        .borrow_mut()
        .push(row(CLOCK, seq, "ClockAdvanced", "system", &payload));
}

pub fn kinds(rig: &Rig) -> Vec<String> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .iter()
        .map(|r| r.event_type.clone())
        .collect()
}

pub fn count(rig: &Rig, kind: &str) -> usize {
    kinds(rig).iter().filter(|k| *k == kind).count()
}

pub fn start(rig: &Rig, research: Option<ResearchWiring>) -> RuntimeProcess<Rig> {
    RuntimeProcess::start(rig.clone(), wiring(research), &config(), at(NOW))
}

pub fn keys(alerts: &[Alert]) -> Vec<&'static str> {
    alerts.iter().map(|a| a.key).collect()
}

/// A worker answer naming a run the core has not started.
pub fn stray() -> ResearchInput {
    ResearchInput::Retrieved {
        run_id: "01STRAYRUN".to_owned(),
        items: Vec::new(),
    }
}

/// The control stream's version-2 `ConnectionEstablished` of the deployment's connection, which names
/// the account the process follows (DEC-1014 item 3).
pub fn established(rig: &Rig) {
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
pub fn reconciled(rig: &Rig) {
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
pub fn running(rig: &Rig, wired: &Wired) -> RuntimeProcess<Rig> {
    reconciled(rig);
    running_with(rig, wired, Box::new(RunRefs))
}

pub fn running_with(rig: &Rig, wired: &Wired, refs: Box<dyn Refs>) -> RuntimeProcess<Rig> {
    established(rig);
    RuntimeProcess::start(
        rig.clone(),
        wiring_with(Some(wired.wiring()), refs),
        &config(),
        at(NOW),
    )
}

/// The run ids of the starts the worker was handed.
pub fn starts(wired: &Wired) -> Vec<String> {
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
pub fn run_started(rig: &Rig) -> Vec<Value> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .iter()
        .filter(|row| row.event_type == "RunStarted")
        .map(|row| parse(&row.body).unwrap())
        .collect()
}

/// The gateway's record of a turn's call that ended `provider_unavailable`, with no output.
pub fn unavailable(turn: &TurnRequest) -> CallRecord {
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

/// The references a run over the recorded bundle carries: the mandate version, and the bundle's
/// research entry as the model version its records name.
pub struct BundleRefs(pub Digest);
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

/// The daemon's agentic fixture bundle (DEC-2266, DEC-2268).
pub const AGENTIC_BUNDLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/research-agentic"
);

/// The agentic fixture's scripted calls, counted from its `research.json` here rather than through
/// the loader: one per call of every scripted turn.
pub fn scripted_calls() -> (usize, usize) {
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

/// The cell's demo research bundle, which every runtime pod is handed when the cell sets
/// `runtime.research` (DEC-2246 item 4).
pub const DEMO_BUNDLE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../deploy/cell/demo/research"
);
/// The demo's own agent's mandate, as its owner confirms it (`deploy/cell/demo/mandate.json`).
pub const DEMO_MANDATE: &str = include_str!("../../../../deploy/cell/demo/mandate.json");

/// The demo's mandate, edited by `edit`, stored in an artifact store of its own for test `name`
/// and deployed for the agent on the rig's control stream; and the research the cell's opt-in
/// wires over the demo bundle for that deployment.
pub fn opted_in(rig: &Rig, name: &str, edit: impl FnOnce(&mut serde_json::Value)) -> CellResearch {
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
pub struct Counted {
    pub inner: Box<dyn ResearchWorker>,
    pub starts: Rc<Cell<usize>>,
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
pub fn a_day_of_research(rig: &Rig, research: CellResearch) -> (usize, usize) {
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
