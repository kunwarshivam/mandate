//! The cell's demo bundle, `deploy/cell/demo/research/` (DEC-2210 item 1, DEC-2246): it loads,
//! records only the demo's one instrument, and runs through the recorded worker to the recorded
//! reply, whose thesis names that instrument and cites only items retrieval returns from allowlisted
//! sources. The fixture rule: the bundle passes every rule it is not meant to fail.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use mandate_daemon::recorded::Bundle;
use mandate_daemon::worker::{RecordedWorker, WorkerFor};
use mandate_daemon::{ArtifactSink, ResearchParts, ResearchWorker};
use mandate_gateway::Outcome as CallOutcome;
use mandate_num::Usd;
use mandate_research::{
    AdmissionFacts, AssetClass, AssetId, AutonomyDecision, LineageState, MandateEnvelope,
    PolicyOverlay, ResearchEnvelope, ValidatedMandate, WorkingUniverse,
};
use mandate_research_run::{CallPlan, Symbol};
use mandate_runtime::{JournalCatalog, Preconditions, ResearchInput, ResearchRequest, RiskClock};
use mandate_time::UtcNanos;
use serde_json::Value;

const DEMO: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../deploy/cell/demo/research"
);
const DEMO_INSTRUMENT: &str = "b28f4066-5c6d-479b-a2af-85dc1a8f16fb";
const CUT_OFF: i64 = 1_792_180_800;
const RUN: &str = "01RUNDEMOBUNDLE";

/// A sink that keeps nothing and accepts everything.
struct Accept;

impl ArtifactSink for Accept {
    fn put(&mut self, _: &[u8]) -> bool {
        true
    }
}

fn bundle() -> Bundle {
    Bundle::load(Path::new(DEMO)).unwrap_or_else(|error| panic!("the demo bundle: {error:?}"))
}

fn json(file: &str) -> Value {
    let text = std::fs::read_to_string(Path::new(DEMO).join(file)).unwrap();
    serde_json::from_str(&text).unwrap()
}

/// A pass's parts for the demo's researching mandate: one instrument, the bundle's allowlist.
fn parts(bundle: &Bundle) -> ResearchParts {
    let envelope = MandateEnvelope {
        universe_pinned: false,
        max_instruments: 1,
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
    ResearchParts {
        preconditions: Preconditions::read(&JournalCatalog::JOURNAL),
        mandate: ValidatedMandate::from_validated_envelope(envelope),
        overlay: PolicyOverlay::internal_research_profile(),
        universe: WorkingUniverse::Known {
            instruments: BTreeSet::new(),
            pinned: false,
        },
        facts: AdmissionFacts {
            allowlist: bundle.allowlist.clone(),
            eligibility_failures: BTreeSet::new(),
            instrument_groups: BTreeMap::new(),
            claimed_by_other_agents: BTreeSet::new(),
            halted_instruments: BTreeSet::new(),
            disclosures_accepted: BTreeSet::new(),
            data_universe: Some(BTreeSet::from([AssetId::new(DEMO_INSTRUMENT).unwrap()])),
            research_spend_usd_today: Usd::parse("0").unwrap(),
        },
        lineages: LineageState::default(),
        active: Vec::new(),
        last_reservation: None,
        data_symbols: BTreeSet::from([Symbol::new("SPY").unwrap()]),
        event_triggers: None,
        events: Vec::new(),
    }
}

#[test]
fn the_demo_bundle_records_only_the_demo_instrument() {
    let bundle = bundle();
    let instruments: Vec<_> = bundle
        .plane
        .instruments
        .iter()
        .map(|fact| fact.asset_id.as_str().to_owned())
        .collect();
    assert_eq!(instruments, [DEMO_INSTRUMENT]);
    assert!(
        bundle.plane.filings.is_empty(),
        "the demo records no filings"
    );
    assert!(bundle.halted.is_empty(), "the demo records no halts");
    let instrument = json("../instrument.json");
    assert_eq!(instrument["instrument_id"], DEMO_INSTRUMENT);
    assert_eq!(instrument["symbol"], "SPY");
}

#[test]
fn the_demo_reply_cites_only_allowlisted_items_on_the_demo_instrument() {
    let research = json("research.json");
    let allowed: BTreeSet<_> = research["allowlist"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect();
    let cited: BTreeSet<_> = json("news.json")
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| allowed.contains(item["source"].as_str().unwrap()))
        .map(|item| item["item_id"].as_str().unwrap().to_owned())
        .collect();
    let reply: Value = serde_json::from_str(research["reply"]["output"].as_str().unwrap()).unwrap();
    let theses = reply["theses"].as_array().unwrap();
    assert_eq!(theses.len(), 1);
    assert_eq!(theses[0]["instrument"], "SPY");
    for evidence in theses[0]["evidence"].as_array().unwrap() {
        assert!(
            cited.contains(evidence.as_str().unwrap()),
            "{evidence} is an allowlisted item"
        );
    }
}

#[test]
fn the_demo_bundle_runs_through_the_recorded_fake_to_its_reply() {
    let bundle = bundle();
    let who = WorkerFor {
        workspace: "ws1".to_owned(),
        agent: "demo-research".to_owned(),
        description: "The demo's researching agent, on recorded data.".to_owned(),
    };
    let now = || UtcNanos::from_parts(CUT_OFF + 5, 0).unwrap();
    let mut worker = RecordedWorker::new(&bundle, who, Box::new(Accept), Box::new(now)).unwrap();
    let parts = parts(&bundle);
    worker.hand(
        &ResearchRequest::Start {
            run_id: RUN.to_owned(),
            cut_off: RiskClock::from_secs(CUT_OFF),
        },
        &parts,
    );
    let retrieved = worker.answers(RiskClock::from_secs(CUT_OFF + 1));
    let Some(ResearchInput::Retrieved { items, .. }) = retrieved.first() else {
        panic!("the run retrieved: {retrieved:?}");
    };
    assert!(!items.is_empty(), "the demo retrieves something");
    let plan = CallPlan {
        run_id: RUN.to_owned(),
        call_id: "01J8Z3MR000000000000000001".to_owned(),
        workspace_id: "ws1".to_owned(),
        agent_id: "demo-research".to_owned(),
        model: bundle.entry.content().model_ref(),
        as_of: UtcNanos::from_parts(CUT_OFF, 0).unwrap(),
        deadline_ms: 120_000,
        max_output_tokens: 4_096,
        observations: (0..items.len()).map(|n| format!("obs-{n}")).collect(),
    };
    worker.hand(&ResearchRequest::Call(plan), &parts);
    let answers = worker.answers(RiskClock::from_secs(CUT_OFF + 2));
    let Some(ResearchInput::Called { record, .. }) = answers.first() else {
        panic!("the call answered: {answers:?}");
    };
    assert_eq!(record.outcome, CallOutcome::Ok);
    assert_eq!(record.endpoint.as_str(), "recorded:demo");
}
