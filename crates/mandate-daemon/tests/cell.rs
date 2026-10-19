//! The cell's research opt-in (DEC-2210 item 5, DEC-2215): never for a deployment that is not a
//! paper one read from the journal, nor over a bundle that does not load, and, once opted in, the
//! bundle's entry over the recorded worker and the cell's source, with records naming the deployed
//! mandate and the entry. Without the opt-in the binary runs deny-all, and a refused opt-in stops
//! its start (`tests/process.rs`).

use std::cell::RefCell;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, parse};
use mandate_daemon::cell::{CellDeployment, CellError, OptIn, opt_in, research};
use mandate_daemon::recorded::{Bundle, BundleError};
use mandate_daemon::source::Rows;
use mandate_journal::{ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_runtime::{EventDraft, EventId};
use mandate_spec::document::Mandate;
use mandate_time::UtcNanos;
use serde_json::{Value as Json, json};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const MANDATE: &str = include_str!("fixtures/mandate.json");
const AGENT: &str = "agent-a";

/// A control row, as the workspace services commit it.
fn row(seq: u64, event_type: &str, environment: &str, payload: &str) -> StoredEvent {
    let event_id = format!("01J8Z3MC{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"{environment}","event_id":"{event_id}","stream_id":"ctl:ws1","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"system","causation_id":null,"correlation_id":null,"actor":{{"kind":"user","id":"user-owner","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: "ctl:ws1".into(),
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

/// The workspace's control stream; every other stream is empty.
#[derive(Clone, Default)]
struct Control(Rc<RefCell<Vec<StoredEvent>>>);

impl Rows for Control {
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        Some(if stream.as_str() == "ctl:ws1" {
            self.0.borrow().clone()
        } else {
            Vec::new()
        })
    }
}

/// An artifact store of its own for test `name`, holding the researching mandate, and the control
/// stream that deploys it for the agent.
fn deployed(name: &str) -> (PathBuf, Control, String, Json) {
    let root = std::env::temp_dir().join(format!("daemon-{}-cell-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let mut store = FsArtifactStore::open(&root).unwrap();
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
    let mandate = Mandate::parse(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
    let bytes = mandate.canonical_bytes().unwrap();
    let version = store.put_artifact(&bytes).unwrap().to_string();
    let control = Control::default();
    control.0.borrow_mut().push(row(
        1,
        "AgentDeployed",
        "paper",
        &format!(
            r#"{{"agent_id":"{AGENT}","mandate_version":"{version}","record_ref":"{version}"}}"#
        ),
    ));
    (root, control, version, document)
}

fn paper(resolved: bool) -> CellDeployment {
    CellDeployment {
        workspace: "ws1".to_owned(),
        agent: AGENT.to_owned(),
        environment: Environment::Paper,
        resolved,
    }
}

fn now() -> Box<dyn FnMut() -> UtcNanos> {
    Box::new(|| UtcNanos::from_parts(1_792_180_800, 0).unwrap())
}

/// The opt-in over the test fixture's bundle for a resolved paper deployment.
fn opted() -> OptIn {
    opt_in(Path::new(FIXTURE), &paper(true)).unwrap_or_else(|error| panic!("{error:?}"))
}

/// Research is asked for only on a paper deployment the process read from the journal; a live one
/// is refused first, read from the journal or not.
#[test]
#[ignore = "pending E21-54"]
fn the_opt_in_refuses_any_but_a_resolved_paper_deployment() {
    let live = |resolved| CellDeployment {
        environment: Environment::Live,
        ..paper(resolved)
    };
    let refused =
        |deployment: &CellDeployment| opt_in(Path::new(FIXTURE), deployment).err().unwrap();
    assert_eq!(refused(&live(true)), CellError::NotPaper);
    assert_eq!(refused(&live(false)), CellError::NotPaper);
    assert_eq!(refused(&paper(false)), CellError::Unresolved);
    opted();
}

/// A bundle that does not load refuses the opt-in.
#[test]
#[ignore = "pending E21-54"]
fn the_opt_in_refuses_a_bundle_it_cannot_read() {
    let missing = std::env::temp_dir().join(format!("daemon-{}-no-bundle", std::process::id()));
    assert_eq!(
        opt_in(&missing, &paper(true)).err().unwrap(),
        CellError::Bundle(BundleError::Unreadable {
            file: "research.json"
        })
    );
}

/// What opted-in research needs and cannot read refuses it: the deployment, or the artifact store.
#[test]
#[ignore = "pending E21-54"]
fn the_research_refuses_what_it_cannot_read() {
    let (root, control, _, _) = deployed("unreadable");
    let nobody = Control::default();
    assert_eq!(
        research(opted(), Box::new(nobody), &root, now())
            .err()
            .unwrap(),
        CellError::Mandate("agent_not_deployed")
    );
    let nowhere = std::env::temp_dir().join(format!("daemon-{}-no-store", std::process::id()));
    assert_eq!(
        research(opted(), Box::new(control), &nowhere, now())
            .err()
            .unwrap(),
        CellError::ArtifactsUnavailable
    );
}

/// Asked for on a resolved paper deployment, research is the bundle's entry, single-call and with
/// revisions off, told the mandate's own description, and its records name the deployed mandate
/// version and the entry's content.
#[test]
#[ignore = "pending E21-54"]
fn the_opted_in_research_is_the_bundle_s_entry_for_the_deployed_mandate() {
    let (root, control, version, document) = deployed("wired");
    let wired = research(opted(), Box::new(control), &root, now())
        .unwrap_or_else(|error| panic!("{error:?}"));
    let bundle = Bundle::load(Path::new(FIXTURE)).unwrap();
    let config = &wired.wiring.config;
    assert_eq!(config.pin, bundle.entry.content().model_ref());
    assert_eq!(u64::from(config.deadline_ms), bundle.entry.deadline_ms);
    assert_eq!(config.max_output_tokens, bundle.entry.max_output_tokens);
    assert_eq!(config.plan, bundle.entry.plan);
    assert_eq!(config.bounds, bundle.entry.bounds);
    assert_eq!(
        config.description,
        document["behavior"]["description"].as_str().unwrap()
    );
    assert!(!config.revisions_enabled);
    assert!(config.agentic.is_none());

    let draft = EventDraft {
        event_id: EventId("01J8Z3MC000000000000000001".to_owned()),
        event_type: "RunStarted".to_owned(),
        causation_id: None,
        payload: Value::Null,
    };
    let (refs_version, refs) = wired.refs.refs(&draft).unwrap();
    assert_eq!(refs_version, 1);
    assert_eq!(
        refs.get("mandate_version").and_then(Value::as_str),
        Some(version.as_str())
    );
    let hash = bundle.entry.content().model_ref().content_hash;
    assert_eq!(
        refs.get("model_version").and_then(Value::as_str),
        Some(format!("sha256:{}", hash.to_hex()).as_str())
    );
}

/// Each refusal reads as the code the binary prints.
#[test]
fn each_refusal_is_a_code() {
    let codes = [
        (CellError::NotPaper, "research_needs_paper"),
        (CellError::Unresolved, "research_needs_resolved_deployment"),
        (
            CellError::ArtifactsUnavailable,
            "artifact_store_unavailable",
        ),
        (
            CellError::Bundle(BundleError::Unreadable {
                file: "research.json",
            }),
            "research_bundle_unreadable",
        ),
        (
            CellError::Mandate("agent_not_deployed"),
            "agent_not_deployed",
        ),
    ];
    for (error, code) in codes {
        assert_eq!(error.code(), code);
        assert_eq!(error.to_string(), code);
        let as_error: &dyn std::error::Error = &error;
        assert!(as_error.source().is_none());
    }
}
