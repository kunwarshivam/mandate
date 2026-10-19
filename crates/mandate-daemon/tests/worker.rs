//! The cell's research worker (DEC-2210 item 3): a run's retrieval over the recorded plane and its
//! one call through the recorded fake, every artifact an answer names stored before the answer,
//! and every answer stamped at the risk clock of the pass it is handed in. The oracle is the
//! harness's own `retrieve` and `capture` over the same bundle, not the worker.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

use mandate_canon::Digest;
use mandate_daemon::meter::CellMeter;
use mandate_daemon::recorded::Bundle;
use mandate_daemon::source::{Rows, SourceFor};
use mandate_daemon::worker::{RecordedWorker, StoreSink, WorkerError, WorkerFor};
use mandate_daemon::{ArtifactSink, ResearchParts, ResearchWorker};
use mandate_gateway::Outcome as CallOutcome;
use mandate_gateway::meter::MeterStore;
use mandate_gateway::meter::{MeterRecord, SharedMeter};
use mandate_gateway::money::Usd6;
use mandate_gateway::provider::SequentialIds;
use mandate_journal::{ArtifactRef, ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_num::Usd;
use mandate_research::{
    AdmissionFacts, AllowlistVersion, AssetClass, AssetId, AutonomyDecision, LineageState,
    MandateEnvelope, PolicyOverlay, ResearchEnvelope, SourceAllowlist, SourceId, ValidatedMandate,
    WorkingUniverse,
};
use mandate_research_run::{
    Allowlist, CallPlan, Readers, RetrievalPlan, Scope, SourceId as RunSource, Symbol, retrieve,
};
use mandate_runtime::{
    JournalCatalog, Preconditions, ResearchInput, ResearchRequest, RetrievedObservation, RiskClock,
};
use mandate_store::MemoryStore;
use mandate_store::meter::JournalMeter;
use mandate_time::UtcNanos;
use proptest::prelude::*;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const CUT_OFF: i64 = 1_792_180_800;
const RUN: &str = "01RUNRECORDED";
const SOURCES: [&str; 2] = ["src.newswire", "src.wire_two"];

fn bundle() -> Bundle {
    Bundle::load(Path::new(FIXTURE)).unwrap()
}

/// An artifact store that keeps what it is given, and can refuse.
#[derive(Clone, Default)]
struct Shelf {
    put: Rc<RefCell<Vec<Vec<u8>>>>,
    refuse: Rc<Cell<bool>>,
}

impl ArtifactSink for Shelf {
    fn put(&mut self, bytes: &[u8]) -> bool {
        if self.refuse.get() {
            return false;
        }
        self.put.borrow_mut().push(bytes.to_vec());
        true
    }
}

/// A pass's parts: the AAPL and MSFT data universe, both recorded wires allowlisted.
fn parts() -> ResearchParts {
    let envelope = MandateEnvelope {
        universe_pinned: false,
        max_instruments: 5,
        asset_classes: BTreeSet::from([AssetClass::UsEquity]),
        leveraged_etps_enabled: false,
        leveraged_etp_disclosure_version: None,
        admits_instruments: true,
        research: Some(ResearchEnvelope {
            interval_s: 3_600,
            cost_cap_usd_per_day: Usd::parse("5").unwrap(),
            max_revisions_per_lineage: 3,
        }),
        admission: AutonomyDecision::Ask,
    };
    let instruments = bundle().plane.instruments;
    ResearchParts {
        preconditions: Preconditions::read(&JournalCatalog::JOURNAL),
        mandate: ValidatedMandate::from_validated_envelope(envelope),
        overlay: PolicyOverlay::internal_research_profile(),
        universe: WorkingUniverse::Known {
            instruments: BTreeSet::new(),
            pinned: false,
        },
        facts: AdmissionFacts {
            allowlist: SourceAllowlist {
                version: AllowlistVersion(3),
                sources: SOURCES.iter().map(|s| SourceId::new(s).unwrap()).collect(),
            },
            eligibility_failures: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            claimed_by_other_agents: BTreeSet::new(),
            halted_instruments: BTreeSet::new(),
            disclosures_accepted: BTreeSet::new(),
            data_universe: Some(
                instruments
                    .iter()
                    .map(|fact| AssetId::new(fact.asset_id.as_str()).unwrap())
                    .collect(),
            ),
            research_spend_usd_today: Usd::parse("0").unwrap(),
        },
        lineages: LineageState::default(),
        active: Vec::new(),
        last_reservation: None,
        data_symbols: BTreeSet::from([Symbol::new("AAPL").unwrap(), Symbol::new("MSFT").unwrap()]),
    }
}

fn worker(shelf: &Shelf) -> RecordedWorker {
    let who = WorkerFor {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        description: "Long-only US large caps, on public news and filings.".to_owned(),
    };
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    RecordedWorker::new(&bundle(), who, Box::new(shelf.clone()), Box::new(now)).unwrap()
}

/// What the run's retrieval must answer and store, computed without the worker: `retrieve` over the
/// bundle's plane in the same scope, each item captured at the cut-off.
fn expected() -> (Vec<RetrievedObservation>, Vec<Vec<u8>>) {
    let bundle = bundle();
    let scope = Scope {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        cut_off: UtcNanos::from_parts(CUT_OFF, 0).unwrap(),
        data_universe: parts().data_symbols,
        held: BTreeSet::new(),
        allowlist: Allowlist {
            version: 3,
            sources: SOURCES.iter().map(|s| RunSource::new(s).unwrap()).collect(),
        },
    };
    let readers = Readers {
        bars: &bundle.plane,
        news: &bundle.plane,
        filings: &bundle.plane,
        instruments: &bundle.plane,
    };
    let retrieval = retrieve(&RetrievalPlan::v0_1(), &scope, readers).unwrap();
    retrieval
        .items
        .iter()
        .map(|item| RetrievedObservation::capture(item, RiskClock::from_secs(CUT_OFF)).unwrap())
        .unzip()
}

fn start(worker: &mut RecordedWorker) {
    let request = ResearchRequest::Start {
        run_id: RUN.to_owned(),
        cut_off: RiskClock::from_secs(CUT_OFF),
    };
    worker.hand(&request, &parts());
}

fn call_plan(observations: usize) -> CallPlan {
    let bundle = bundle();
    CallPlan {
        run_id: RUN.to_owned(),
        call_id: "01J8Z3MR000000000000000001".to_owned(),
        workspace_id: "ws1".to_owned(),
        agent_id: "agent-a".to_owned(),
        model: bundle.entry.content().model_ref(),
        as_of: UtcNanos::from_parts(CUT_OFF, 0).unwrap(),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        observations: (0..observations).map(|n| format!("obs-{n}")).collect(),
    }
}

#[test]
fn a_run_retrieves_the_recorded_plane_and_stores_each_record_before_its_answer() {
    let shelf = Shelf::default();
    let mut worker = worker(&shelf);
    let (observations, records) = expected();
    assert!(!observations.is_empty(), "the fixture retrieves something");
    start(&mut worker);
    assert_eq!(*shelf.put.borrow(), records, "every record is stored first");
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 10));
    assert_eq!(
        answers,
        [ResearchInput::Retrieved {
            run_id: RUN.to_owned(),
            items: observations,
        }]
    );
    assert!(
        worker
            .answers(RiskClock::from_secs(CUT_OFF + 20))
            .is_empty()
    );
}

#[test]
fn an_item_whose_record_the_sink_refuses_is_never_answered() {
    let shelf = Shelf::default();
    shelf.refuse.set(true);
    let mut worker = worker(&shelf);
    start(&mut worker);
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 10));
    assert_eq!(
        answers,
        [ResearchInput::Retrieved {
            run_id: RUN.to_owned(),
            items: Vec::new(),
        }]
    );
}

#[test]
fn a_cancelled_run_before_its_call_says_nothing_was_dispatched() {
    let shelf = Shelf::default();
    let mut worker = worker(&shelf);
    start(&mut worker);
    worker.hand(
        &ResearchRequest::Cancel {
            run_id: RUN.to_owned(),
        },
        &parts(),
    );
    assert_eq!(
        worker.answers(RiskClock::from_secs(CUT_OFF + 10)),
        [ResearchInput::Cancelled {
            run_id: RUN.to_owned(),
            dispatched: false,
        }]
    );
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(16))]

    /// The call goes through the recorded fake and gives the recorded reply; its prompt and
    /// response are stored before the answer, which carries the risk clock of the pass it is
    /// handed in, whatever that clock reads.
    #[test]
    fn the_call_answers_the_recorded_reply_stamped_at_the_handed_risk_clock(
        later in 1_i64..86_400,
    ) {
        let shelf = Shelf::default();
        let mut worker = worker(&shelf);
        start(&mut worker);
        let retrieved = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
        let Some(ResearchInput::Retrieved { items, .. }) = retrieved.first() else {
            panic!("the run retrieved: {retrieved:?}");
        };
        let stored_before = shelf.put.borrow().len();
        worker.hand(&ResearchRequest::Call(call_plan(items.len())), &parts());
        let at = RiskClock::from_secs(CUT_OFF + later);
        let answers = worker.answers(at);
        prop_assert_eq!(answers.len(), 1);
        let Some(ResearchInput::Called { run_id, at: stamped, record }) = answers.first() else {
            panic!("the call answered: {answers:?}");
        };
        prop_assert_eq!(run_id.as_str(), RUN);
        prop_assert_eq!(*stamped, at);
        prop_assert_eq!(record.outcome, CallOutcome::Ok);
        prop_assert_eq!(record.endpoint.as_str(), "openrouter:anthropic");
        let shelf = shelf.put.borrow();
        let stored: BTreeSet<_> = shelf[stored_before..]
            .iter()
            .map(|bytes| ArtifactRef::of(bytes).digest())
            .collect();
        prop_assert!(record.prompt_ref.is_some_and(|d| stored.contains(&d)));
        prop_assert!(record.response_ref.is_some_and(|d| stored.contains(&d)));
    }
}

/// The agent stream: one call record at 19:00Z on the cut-off's risk day, or at `event_time`, its
/// `reservation` the JSON given, or nothing.
struct AgentRows(Vec<StoredEvent>);

impl AgentRows {
    fn with_call(reservation: Option<&str>) -> Self {
        Self::with_call_at(reservation, "2026-10-16T19:00:00.000000000Z")
    }

    fn with_call_at(reservation: Option<&str>, event_time: &str) -> Self {
        let Some(reservation) = reservation else {
            return Self(Vec::new());
        };
        let body = format!(
            r#"{{"envelope_version":1,"environment":"paper","event_id":"01J8Z3MA000000000000000001","stream_id":"agent:ws1:agent-a","event_type":"ModelInvocationRecorded","schema_version":1,"event_time":"{event_time}","clock_source":"system","causation_id":null,"correlation_id":"01RUN","actor":{{"kind":"agent","id":"agent-a","version":"1","build":null}},"config_refs":{{}},"payload":{{"cost_usd":"0.25","reservation":{reservation}}},"artifact_refs":[],"pii_refs":[]}}"#
        );
        Self(vec![StoredEvent {
            stream_id: "agent:ws1:agent-a".into(),
            seq: 1,
            event_id: "01J8Z3MA000000000000000001".into(),
            event_type: "ModelInvocationRecorded".into(),
            schema_version: 1,
            environment: "paper".into(),
            recorded_at: event_time.into(),
            prev_hash: Digest::of(b"prev"),
            hash: Digest::of(body.as_bytes()),
            body: body.into_bytes(),
        }])
    }
}

impl Rows for AgentRows {
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        Some(if stream.as_str() == "agent:ws1:agent-a" {
            self.0.clone()
        } else {
            Vec::new()
        })
    }
}

/// A worker whose gateway reserves through a meter over `store`, the agent stream holding one call
/// record whose reservation is `reservation`, if any, and its clock at the worker's own `now`.
fn metered(shelf: &Shelf, store: &SharedMeter, reservation: Option<&str>) -> RecordedWorker {
    metered_over(shelf, store, AgentRows::with_call(reservation))
}

/// As [`metered`], over the agent stream `rows`.
fn metered_over(shelf: &Shelf, store: &SharedMeter, rows: AgentRows) -> RecordedWorker {
    let who = WorkerFor {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        description: "Long-only US large caps, on public news and filings.".to_owned(),
    };
    let clock = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let meter = CellMeter::new(
        store.clone(),
        Rc::new(rows),
        SourceFor {
            workspace: "ws1".to_owned(),
            agent: "agent-a".to_owned(),
            environment: Environment::Paper,
        },
        Rc::new(clock),
    );
    RecordedWorker::metered(
        &bundle(),
        who,
        Box::new(shelf.clone()),
        Box::new(clock),
        meter,
    )
    .unwrap()
}

/// The call the run makes, through `worker`: its one answer's record.
fn called(worker: &mut RecordedWorker) -> mandate_gateway::CallRecord {
    start(worker);
    let retrieved = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let Some(ResearchInput::Retrieved { items, .. }) = retrieved.first() else {
        panic!("the run retrieved: {retrieved:?}");
    };
    worker.hand(&ResearchRequest::Call(call_plan(items.len())), &parts());
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 2));
    let [ResearchInput::Called { record, .. }] = answers.as_slice() else {
        panic!("the call answered once: {answers:?}");
    };
    (**record).clone()
}

/// DEC-2249 item 3: the worker's gateway reserves and settles through the shared meter, so the
/// source can count what it reserved. After a restart, while the agent stream records a reserved
/// call earlier on the risk day that the meter does not hold, the gateway refuses the call
/// `meter_unavailable`, reserves nothing, and sends nothing.
#[test]
fn a_held_meter_refuses_the_call_and_a_free_one_holds_its_reservation() {
    let (shelf, store) = (Shelf::default(), SharedMeter::new());
    let record = called(&mut metered(&shelf, &store, None));
    assert_eq!(record.outcome, CallOutcome::Ok);
    let reservation = record.reservation.expect("an ok call is reserved");
    let records = store.records();
    assert!(
        records
            .iter()
            .any(|r| matches!(r, MeterRecord::Reserved(x) if x.event_id == reservation)),
        "the shared store holds the call's reservation: {records:?}"
    );
    assert!(
        records
            .iter()
            .any(|r| matches!(r, MeterRecord::Settled(x) if x.reservation == reservation)),
        "and its settlement"
    );

    let (shelf, store) = (Shelf::default(), SharedMeter::new());
    let record = called(&mut metered(
        &shelf,
        &store,
        Some(r#""01J8Z3MR000000000000000777""#),
    ));
    assert_eq!(record.outcome, CallOutcome::MeterUnavailable);
    assert_eq!(record.reservation, None);
    assert!(store.records().is_empty(), "nothing reserved while held");
    assert_eq!(record.response_ref, None, "nothing was sent");
}

/// A clean restart is no outage: a worker restarted on an empty meter, its agent stream recording
/// a reserved call only on an earlier risk day (03:59:59Z on the 16th is the 15th in New York),
/// calls as before, and its reservation lands in the shared store.
#[test]
fn a_restart_with_no_reserved_call_on_the_risk_day_calls_as_before() {
    let (shelf, store) = (Shelf::default(), SharedMeter::new());
    let rows = AgentRows::with_call_at(
        Some(r#""01J8Z3MR000000000000000555""#),
        "2026-10-16T03:59:59.000000000Z",
    );
    let record = called(&mut metered_over(&shelf, &store, rows));
    assert_eq!(record.outcome, CallOutcome::Ok);
    let reservation = record.reservation.expect("an ok call is reserved");
    assert!(
        store
            .records()
            .iter()
            .any(|r| matches!(r, MeterRecord::Reserved(x) if x.event_id == reservation)),
        "the call's reservation is in the shared store"
    );
}

/// As [`metered`] over an agent stream with no call, the gateway's daily research cap at `daily`.
fn capped(shelf: &Shelf, store: &SharedMeter, daily: Usd6) -> Result<RecordedWorker, WorkerError> {
    let clock = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let meter = CellMeter::new(
        store.clone(),
        Rc::new(AgentRows::with_call(None)),
        SourceFor {
            workspace: "ws1".to_owned(),
            agent: "agent-a".to_owned(),
            environment: Environment::Paper,
        },
        Rc::new(clock),
    );
    RecordedWorker::capped(
        &bundle(),
        WorkerFor {
            workspace: "ws1".to_owned(),
            agent: "agent-a".to_owned(),
            description: "Long-only US large caps, on public news and filings.".to_owned(),
        },
        Box::new(shelf.clone()),
        Box::new(clock),
        meter,
        daily,
    )
}

/// DEC-2254 item 4: the gateway holds the agent's research calls to the daily cap. Under the
/// mandate's cap of five dollars the call goes out as before. Under a cap below the call's
/// reservation, far under the workspace's and the deployment's budgets, it is refused
/// `budget_exhausted`: nothing is reserved and nothing is sent. A cap above the workspace's
/// monthly budget builds no worker (item 3).
#[test]
fn a_call_over_the_daily_research_cap_is_refused_and_nothing_is_sent() {
    let (shelf, store) = (Shelf::default(), SharedMeter::new());
    let mut worker = capped(&shelf, &store, Usd6::from_micros(5_000_000)).unwrap();
    let record = called(&mut worker);
    assert_eq!(record.outcome, CallOutcome::Ok);
    assert!(record.reservation.is_some(), "an ok call is reserved");

    let (shelf, store) = (Shelf::default(), SharedMeter::new());
    let mut worker = capped(&shelf, &store, Usd6::from_micros(1)).unwrap();
    let record = called(&mut worker);
    assert_eq!(record.outcome, CallOutcome::BudgetExhausted);
    assert_eq!(record.reservation, None);
    assert!(store.records().is_empty(), "nothing reserved over the cap");
    assert_eq!(record.response_ref, None, "nothing was sent");

    let above = bundle().workspace_cap.micros() + 1;
    assert_eq!(
        capped(
            &Shelf::default(),
            &SharedMeter::new(),
            Usd6::from_micros(above)
        )
        .err(),
        Some(WorkerError::DailyCapAboveWorkspace)
    );
}

/// An artifact store that answers every put with a reference other than the bytes'.
struct Misfiled;

impl mandate_journal::ArtifactSource for Misfiled {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, mandate_journal::ArtifactError> {
        Err(mandate_journal::ArtifactError::Missing)
    }
}

impl ArtifactStore for Misfiled {
    fn put_artifact(&mut self, _: &[u8]) -> Result<ArtifactRef, mandate_journal::ArtifactError> {
        Ok(ArtifactRef::of(b"other bytes"))
    }
}

/// The store sink stores content-addressed: it says stored only when the store kept the bytes under
/// their own reference.
#[test]
fn the_store_sink_says_stored_only_for_the_bytes_own_reference() {
    let mut kept = StoreSink(BTreeMap::new());
    assert!(kept.put(b"a record"));
    assert_eq!(
        kept.0.get(&ArtifactRef::of(b"a record").digest()),
        Some(&b"a record".to_vec())
    );
    assert!(!StoreSink(Misfiled).put(b"a record"));
}

/// As [`called`], the call made as `call_id`: the core gives every call an id of its own, so a
/// call after a restart is never the first call again (journal spec rule 158).
fn called_as(worker: &mut RecordedWorker, call_id: &str) -> mandate_gateway::CallRecord {
    start(worker);
    let retrieved = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let Some(ResearchInput::Retrieved { items, .. }) = retrieved.first() else {
        panic!("the run retrieved: {retrieved:?}");
    };
    let plan = CallPlan {
        call_id: call_id.to_owned(),
        ..call_plan(items.len())
    };
    worker.hand(&ResearchRequest::Call(plan), &parts());
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 2));
    let [ResearchInput::Called { record, .. }] = answers.as_slice() else {
        panic!("the call answered once: {answers:?}");
    };
    (**record).clone()
}

/// The journal's meter stream for `ws1`, opened by a writer of its own.
fn journal_meter(journal: &MemoryStore, at: i64) -> JournalMeter<MemoryStore> {
    let clock = move || UtcNanos::from_parts(at, 0).unwrap();
    JournalMeter::open(
        journal.clone(),
        "ws1",
        Environment::Paper,
        Rc::new(clock),
        Box::new(SequentialIds::starting_at(
            u128::try_from(at).unwrap() << 32,
        )),
    )
}

/// A worker started at `at`, its gateway reserving through the journal's meter stream under the
/// daily research cap `daily`, over an agent stream recording `rows`.
fn journaled(journal: &MemoryStore, rows: AgentRows, at: i64, daily: Usd6) -> RecordedWorker {
    let clock = move || UtcNanos::from_parts(at, 0).unwrap();
    let meter = CellMeter::over(
        Box::new(journal_meter(journal, at)),
        Rc::new(rows),
        SourceFor {
            workspace: "ws1".to_owned(),
            agent: "agent-a".to_owned(),
            environment: Environment::Paper,
        },
        Rc::new(clock),
    );
    RecordedWorker::capped(
        &bundle(),
        WorkerFor {
            workspace: "ws1".to_owned(),
            agent: "agent-a".to_owned(),
            description: "Long-only US large caps, on public news and filings.".to_owned(),
        },
        Box::new(Shelf::default()),
        Box::new(clock),
        meter,
        daily,
    )
    .unwrap()
}

/// E21-61's acceptance in the cell (DEC-2259 items 1 and 2): a worker restarted over the journaled
/// meter, its agent stream recording the call it made before, is not held, and the day's cap counts
/// that call: a cap one micro-dollar short of its spend plus a new call's maximum refuses the new
/// call `budget_exhausted` and journals nothing, and the cap at exactly that sum admits it. The
/// sum is read from the journal's records, not from the worker.
#[test]
fn a_restarted_worker_over_the_journaled_meter_keeps_the_daily_cap() {
    let journal = MemoryStore::default();
    let first = called(&mut journaled(
        &journal,
        AgentRows::with_call(None),
        CUT_OFF + 5,
        Usd6::from_micros(5_000_000),
    ));
    assert_eq!(first.outcome, CallOutcome::Ok);
    let reservation = first.reservation.expect("an ok call is reserved");
    let records = journal_meter(&journal, CUT_OFF + 6).read().unwrap();
    let (mut max, mut cost) = (None, None);
    for record in &records {
        match record {
            MeterRecord::Reserved(r) if r.event_id == reservation => max = Some(r.max_cost),
            MeterRecord::Settled(s) if s.reservation == reservation => cost = Some(s.cost),
            _ => {}
        }
    }
    let (max, cost) = (max.unwrap(), cost.unwrap());
    let quoted = format!(r#""{reservation}""#);
    let after = |at: i64, daily: u64| {
        journaled(
            &journal,
            AgentRows::with_call(Some(&quoted)),
            at,
            Usd6::from_micros(daily),
        )
    };

    let short = cost.micros() + max.micros() - 1;
    let refused = called_as(
        &mut after(CUT_OFF + 65, short),
        "01J8Z3MR000000000000000002",
    );
    assert_eq!(refused.outcome, CallOutcome::BudgetExhausted);
    assert_eq!(refused.reservation, None);
    assert_eq!(
        journal_meter(&journal, CUT_OFF + 66).read().unwrap(),
        records,
        "nothing journaled over the cap"
    );

    let admitted = called_as(
        &mut after(CUT_OFF + 125, short + 1),
        "01J8Z3MR000000000000000003",
    );
    assert_eq!(
        admitted.outcome,
        CallOutcome::Ok,
        "not held after the restart"
    );
}
