//! The cell's research source (DEC-2214 items 2 to 4): each pass's parts from the recorded bundle,
//! the deployed mandate document, and the control and agent streams, and no context at all when a
//! fact cannot be read. The envelope's own reading is `envelope.rs`'s.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mandate_canon::{Digest, parse};
use mandate_daemon::ResearchSource;
use mandate_daemon::recorded::Bundle;
use mandate_daemon::source::{CellSource, Rows, SourceFor, envelope};
use mandate_journal::{AppendOutcome, ArtifactRef, Environment, StoredEvent, StreamId};
use mandate_num::Usd;
use mandate_research::{LineageState, WorkingUniverse};
use mandate_runtime::{AgentId, ConnectionId, Deployment, RuntimeState, WorkspaceId};
use mandate_spec::document::Mandate;
use mandate_store::Store;
use mandate_time::UtcNanos;
use serde_json::{Value as Json, json};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const MANDATE: &str = include_str!("fixtures/mandate.json");
const AGENT: &str = "agent-a";
const AAPL: &str = "7b4a1c2e-1111-4a2b-9c3d-000000000001";
const MSFT: &str = "7b4a1c2e-2222-4a2b-9c3d-000000000002";

/// A record of `stream`, with `payload` at `payload`.
fn row(stream: &str, seq: u64, event_type: &str, environment: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01J8Z3MC{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"{environment}","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"system","causation_id":null,"correlation_id":null,"actor":{{"kind":"user","id":"user-owner","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: stream.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 1,
        environment: environment.into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

/// The workspace's control and agent streams, either of which a test can make unreadable.
#[derive(Clone, Default)]
struct Journal {
    control: Rc<RefCell<Vec<StoredEvent>>>,
    agent: Rc<RefCell<Vec<StoredEvent>>>,
    unreadable: Rc<RefCell<BTreeSet<String>>>,
}

impl Rows for Journal {
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        if self.unreadable.borrow().contains(stream.as_str()) {
            return None;
        }
        match stream.as_str() {
            "ctl:ws1" => Some(self.control.borrow().clone()),
            "agent:ws1:agent-a" => Some(self.agent.borrow().clone()),
            _ => None,
        }
    }
}

impl Journal {
    fn control(&self, event_type: &str, environment: &str, payload: &str) {
        let seq = u64::try_from(self.control.borrow().len()).unwrap() + 1;
        self.control
            .borrow_mut()
            .push(row("ctl:ws1", seq, event_type, environment, payload));
    }

    fn agent(&self, event_type: &str, payload: &str) {
        let seq = u64::try_from(self.agent.borrow().len()).unwrap() + 1;
        self.agent
            .borrow_mut()
            .push(row("agent:ws1:agent-a", seq, event_type, "paper", payload));
    }

    fn deployed(&self, agent: &str, version: &str, environment: &str) {
        self.control(
            "AgentDeployed",
            environment,
            &format!(
                r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{version}"}}"#
            ),
        );
    }

    fn stopped(&self, agent: &str) {
        self.control(
            "AgentStopped",
            "paper",
            &format!(
                r#"{{"agent_id":"{agent}","connection_id":"conn_alpaca_paper_01","reason":"owner_stop","retired_on":"2026-10-16","loss_added":"0"}}"#
            ),
        );
    }
}

/// The fixture mandate made one the research agent may work under: an unpinned universe of up to
/// five instruments, an admitting signal model, and a research envelope.
fn researching() -> Json {
    let mut document: Json = serde_json::from_str(MANDATE).unwrap();
    document["universe"]["pinned"] = json!(false);
    document["universe"]["pinned_instruments"] = json!([]);
    document["universe"]["max_instruments"] = json!(5);
    document["behavior"]["signal_models"][0]["admits_instruments"] = json!(true);
    document["behavior"]["research"] = json!({
        "interval_s": 3600,
        "cost_cap_usd_per_day": "5",
        "max_revisions_per_lineage": 3
    });
    document
}

/// Stores `document` canonically, as the workspace services do, and gives its version.
fn store(documents: &mut BTreeMap<Digest, Vec<u8>>, document: &Json) -> String {
    let mandate = Mandate::parse(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
    let bytes = mandate.canonical_bytes().unwrap();
    let reference = ArtifactRef::of(&bytes);
    documents.insert(reference.digest(), bytes);
    reference.to_string()
}

fn state() -> RuntimeState {
    RuntimeState::new(Deployment {
        agent: AgentId(AGENT.into()),
        connection: ConnectionId("conn-1".into()),
        workspace: WorkspaceId("ws1".into()),
    })
}

fn who() -> SourceFor {
    SourceFor {
        workspace: "ws1".to_owned(),
        agent: AGENT.to_owned(),
        environment: Environment::Paper,
    }
}

/// A journal with `document` deployed for the agent, and the source over `bundle`.
fn deployed(bundle: &Bundle, document: &Json) -> (Journal, CellSource) {
    let journal = Journal::default();
    let mut documents = BTreeMap::new();
    let version = store(&mut documents, document);
    journal.deployed(AGENT, &version, "paper");
    let source = CellSource::new(
        bundle,
        who(),
        Box::new(journal.clone()),
        Box::new(documents),
    );
    (journal, source)
}

fn bundle() -> Bundle {
    Bundle::load(Path::new(FIXTURE)).unwrap()
}

/// A copy of the fixture bundle with `edit` applied to `file`'s JSON.
fn bundle_with(name: &str, file: &str, edit: impl FnOnce(&mut Json)) -> Bundle {
    let dir: PathBuf =
        std::env::temp_dir().join(format!("daemon-{}-source-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(FIXTURE).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
    }
    let mut value: Json = serde_json::from_slice(&fs::read(dir.join(file)).unwrap()).unwrap();
    edit(&mut value);
    fs::write(dir.join(file), value.to_string()).unwrap();
    Bundle::load(&dir).unwrap()
}

fn ids(ids: &[&str]) -> Vec<String> {
    ids.iter().map(|id| (*id).to_owned()).collect()
}

/// A store the source reads through, holding one row on every stream.
struct OneRow;

impl Store for OneRow {
    fn take_ownership(&mut self, _: &StreamId) -> Option<u64> {
        None
    }
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        Some(vec![row(
            stream.as_str(),
            1,
            "AgentDeployed",
            "paper",
            "{}",
        )])
    }
    fn append(
        &mut self,
        _: &StreamId,
        _: u64,
        _: u64,
        _: UtcNanos,
        _: &[Vec<u8>],
    ) -> AppendOutcome {
        AppendOutcome::Unavailable
    }
}

/// Any store is a source of rows, read as the store reads them.
#[test]
#[ignore = "pending E21-54"]
fn a_store_is_read_as_its_own_rows() {
    let stream = StreamId::parse("ctl:ws1").unwrap();
    assert_eq!(Rows::rows(&OneRow, &stream), Store::rows(&OneRow, &stream));
    assert_eq!(Rows::rows(&OneRow, &stream).unwrap().len(), 1);
}

/// On a fresh agent stream, the pass gets the deployed mandate's envelope and the bundle's
/// recorded facts, an empty working universe, and no theses, lineages or reservation.
#[test]
#[ignore = "pending E21-54"]
fn a_fresh_agent_gets_its_mandate_and_the_bundle_s_facts() {
    let bundle = bundle();
    let document = researching();
    let (_, source) = deployed(&bundle, &document);
    let parts = source.parts(&state()).unwrap();
    let expected = envelope(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
    assert_eq!(*parts.mandate.envelope(), expected);
    let facts = &parts.facts;
    assert_eq!(facts.allowlist, bundle.allowlist);
    assert_eq!(
        facts
            .data_universe
            .as_ref()
            .unwrap()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect::<Vec<_>>(),
        ids(&[AAPL, MSFT])
    );
    assert!(facts.eligibility_failures.is_empty());
    assert!(facts.halted_instruments.is_empty());
    assert!(facts.claimed_by_other_agents.is_empty());
    assert!(facts.disclosures_accepted.is_empty());
    assert!(facts.instrument_groups.is_empty());
    assert_eq!(facts.research_spend_usd_today, Usd::parse("0").unwrap());
    assert_eq!(
        parts.universe,
        WorkingUniverse::Known {
            instruments: BTreeSet::new(),
            pinned: false
        }
    );
    assert_eq!(parts.lineages, LineageState::default());
    assert!(parts.active.is_empty());
    assert_eq!(parts.last_reservation, None);
    assert_eq!(
        parts
            .data_symbols
            .iter()
            .map(|s| s.as_str().to_owned())
            .collect::<Vec<_>>(),
        ids(&["AAPL", "MSFT"])
    );
}

/// The bundle's halts and ineligible instruments reach the eligibility floor, by asset.
#[test]
#[ignore = "pending E21-54"]
fn the_bundle_s_halts_and_ineligible_instruments_reach_the_floor() {
    let halted = bundle_with("halted", "research.json", |r| {
        r["halted_instruments"] = json!(["MSFT"])
    });
    let (_, source) = deployed(&halted, &researching());
    let facts = source.parts(&state()).unwrap().facts;
    let halted: Vec<_> = facts
        .halted_instruments
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(halted, ids(&[MSFT]));

    let ineligible = bundle_with("ineligible", "instruments.json", |rows| {
        rows[0]["eligible"] = json!(false)
    });
    let (_, source) = deployed(&ineligible, &researching());
    let facts = source.parts(&state()).unwrap().facts;
    let failures: Vec<_> = facts
        .eligibility_failures
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect();
    assert_eq!(failures, ids(&[AAPL]));
}

/// Without a deployed mandate it can read, the pass has no context: the agent never deployed or
/// since stopped, its document missing or unparseable, or a stream unreadable.
#[test]
#[ignore = "pending E21-54"]
fn no_context_without_a_deployed_mandate_it_can_read() {
    let bundle = bundle();
    let journal = Journal::default();
    let source = CellSource::new(
        &bundle,
        who(),
        Box::new(journal.clone()),
        Box::new(BTreeMap::<Digest, Vec<u8>>::new()),
    );
    assert!(source.parts(&state()).is_none(), "never deployed");
    journal.deployed(AGENT, &format!("sha256:{}", "ab".repeat(32)), "paper");
    assert!(source.parts(&state()).is_none(), "document missing");

    let (journal, source) = deployed(&bundle, &researching());
    journal.stopped(AGENT);
    assert!(source.parts(&state()).is_none(), "stopped");

    let (journal, source) = deployed(&bundle, &researching());
    assert!(source.parts(&state()).is_some());
    for stream in ["ctl:ws1", "agent:ws1:agent-a"] {
        journal.unreadable.borrow_mut().insert(stream.to_owned());
        assert!(source.parts(&state()).is_none(), "{stream} unreadable");
        journal.unreadable.borrow_mut().clear();
    }
}

/// Another agent deployed in the environment may claim instruments this pass cannot see, so the
/// pass has no context until it stops (DEC-2214 item 2).
#[test]
#[ignore = "pending E21-54"]
fn no_context_while_another_agent_is_deployed() {
    let (journal, source) = deployed(&bundle(), &researching());
    journal.deployed("agent-b", &format!("sha256:{}", "cd".repeat(32)), "paper");
    assert!(source.parts(&state()).is_none());
    journal.stopped("agent-b");
    assert!(source.parts(&state()).is_some(), "agent-b stopped");
    journal.deployed("agent-b", &format!("sha256:{}", "cd".repeat(32)), "live");
    assert!(
        source.parts(&state()).is_some(),
        "another environment's deployment is not this one's"
    );
}

/// Once the agent stream holds a record whose facts the source cannot fold yet, research pauses
/// (DEC-2214 item 3, until E21-59): a thesis, a universe change, or a call the meter reserved. A
/// run's start and observations do not pause it, so the first run is never held mid-way.
#[test]
#[ignore = "pending E21-54"]
fn research_pauses_once_the_stream_holds_a_fact_it_cannot_fold() {
    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent("RunStarted", r#"{"run_id":"01RUN"}"#);
    journal.agent("ObservationRecorded", r#"{"source":"src.newswire"}"#);
    assert!(source.parts(&state()).is_some(), "a run under way");

    for kind in [
        "ThesisProposed",
        "ThesisRevised",
        "ThesisInvalidated",
        "UniverseChanged",
    ] {
        let (journal, source) = deployed(&bundle(), &researching());
        journal.agent(kind, r#"{}"#);
        assert!(source.parts(&state()).is_none(), "{kind}");
    }

    let (journal, source) = deployed(&bundle(), &researching());
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"0.25","reservation":"01J8Z3MR000000000000000001"}"#,
    );
    assert!(source.parts(&state()).is_none(), "a metered call");
}

/// Today's spend is at most every unmetered call's recorded cost, summed; a cost it cannot read
/// gives no context.
#[test]
#[ignore = "pending E21-54"]
fn spend_sums_the_recorded_costs_and_refuses_one_it_cannot_read() {
    let (journal, source) = deployed(&bundle(), &researching());
    for cost in ["0.25", "0.5"] {
        journal.agent(
            "ModelInvocationRecorded",
            &format!(r#"{{"cost_usd":"{cost}","reservation":null}}"#),
        );
    }
    let facts = source.parts(&state()).unwrap().facts;
    assert_eq!(facts.research_spend_usd_today, Usd::parse("0.75").unwrap());
    journal.agent(
        "ModelInvocationRecorded",
        r#"{"cost_usd":"a lot","reservation":null}"#,
    );
    assert!(source.parts(&state()).is_none());
}
