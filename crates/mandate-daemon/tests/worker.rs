//! The cell's research worker (DEC-2210 item 3): a run's retrieval over the recorded plane and its
//! one call through the recorded fake, every artifact an answer names stored before the answer,
//! and every answer stamped at the risk clock of the pass it is handed in. The oracle is the
//! harness's own `retrieve` and `capture` over the same bundle, not the worker.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

use mandate_daemon::recorded::Bundle;
use mandate_daemon::worker::{RecordedWorker, StoreSink, WorkerFor};
use mandate_daemon::{ArtifactSink, ResearchParts, ResearchWorker};
use mandate_gateway::Outcome as CallOutcome;
use mandate_journal::{ArtifactRef, ArtifactStore};
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
