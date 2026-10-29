//! The cell's research worker's shared test fixtures: the recorded bundle, a sink that keeps and
//! gives back what it is handed and can refuse, a pass's parts, and the bundle naming the agentic
//! loop (DEC-2263).

#![allow(dead_code, reason = "each test binary uses a different subset")]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::rc::Rc;

use mandate_canon::{Digest, Key, Value, parse};
use mandate_daemon::recorded::Bundle;
use mandate_daemon::worker::WorkerFor;
use mandate_daemon::{ArtifactSink, ResearchParts};
use mandate_num::Usd;
use mandate_research::{
    AdmissionFacts, AllowlistVersion, AssetClass, AssetId, AutonomyDecision, LineageState,
    MandateEnvelope, PolicyOverlay, ResearchEnvelope, SourceAllowlist, SourceId, ValidatedMandate,
    WorkingUniverse,
};
use mandate_research_run::{ResearchEntry, Symbol};
use mandate_runtime::{JournalCatalog, Preconditions};

pub const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
pub const CUT_OFF: i64 = 1_792_180_800;
pub const RUN: &str = "01RUNRECORDED";
pub const SOURCES: [&str; 2] = ["src.newswire", "src.wire_two"];

pub fn bundle() -> Bundle {
    Bundle::load(Path::new(FIXTURE)).unwrap()
}

/// An artifact store that keeps what it is given, gives it back by its digest, and can refuse.
#[derive(Clone, Default)]
pub struct Shelf {
    pub put: Rc<RefCell<Vec<Vec<u8>>>>,
    pub refuse: Rc<Cell<bool>>,
}

impl ArtifactSink for Shelf {
    fn put(&mut self, bytes: &[u8]) -> bool {
        if self.refuse.get() {
            return false;
        }
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

/// A pass's parts: the AAPL and MSFT data universe, both recorded wires allowlisted at version 3.
pub fn parts() -> ResearchParts {
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
        event_triggers: None,
        events: Vec::new(),
        stored: mandate_daemon::StoredNews::default(),
    }
}

pub const RECORDING: &str = include_str!("../fixtures/research/research.json");

/// The recorded bundle with its entry naming the `v0_2` loop, read as `research.json` would be.
pub fn agentic_bundle() -> Bundle {
    let Some(Value::Object(mut entry)) = parse(RECORDING.as_bytes()).unwrap().get("entry").cloned()
    else {
        panic!("the recording has an entry")
    };
    entry.insert(Key::new("agentic").unwrap(), Value::Str("v0_2".to_owned()));
    let mut bundle = bundle();
    bundle.entry =
        ResearchEntry::recorded(&Value::Object(entry)).unwrap_or_else(|e| panic!("{e:?}"));
    bundle
}

pub fn who() -> WorkerFor {
    WorkerFor {
        workspace: "ws1".to_owned(),
        agent: "agent-a".to_owned(),
        description: "Long-only US large caps, on public news and filings.".to_owned(),
    }
}
