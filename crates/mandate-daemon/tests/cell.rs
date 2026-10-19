//! The cell's research opt-in (DEC-2210 item 5, DEC-2215): never for a deployment that is not a
//! paper one read from the journal, nor over a bundle that does not load, and, once opted in, the
//! bundle's entry over the recorded worker and the cell's source, with records naming the deployed
//! mandate and the entry. Without the opt-in the binary runs deny-all, and a refused opt-in stops
//! its start (`tests/process.rs`). An agentic bundle gives the core its loop: `final_output`, the
//! entry's bounds and declared tools, and the recorded run cap; one recording no run cap is refused
//! at the opt-in (DEC-2268).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, parse};
use mandate_daemon::cell::{
    CellDeployment, CellError, OptIn, agentic_config, meter_opened_ids, opt_in, research,
    research_daily_cap, research_metered,
};
use mandate_daemon::recorded::{Bundle, BundleError};
use mandate_daemon::source::Rows;
use mandate_gateway::money::Usd6;
use mandate_gateway::provider::{IdSource, SequentialIds};
use mandate_journal::{ArtifactStore, Environment, StoredEvent, StreamId};
use mandate_research_run::agentic::{LoopBounds, OutputMode, Tool, ToolClass};
use mandate_runtime::{AgenticConfig, EventDraft, EventId};
use mandate_spec::document::Mandate;
use mandate_store::MemoryStore;
use mandate_store::meter::JournalMeter;
use mandate_time::UtcNanos;
use serde_json::{Value as Json, json};

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/research");
const AGENTIC: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/research-agentic"
);
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

/// DEC-2254 items 1 to 3: the daily research cap is the mandate's own, tightened by the research
/// profile, which sets no ceiling of its own; zero for a mandate with no research envelope; and no
/// cap at all, so no start, for a document that cannot give one the gateway can hold.
#[test]
fn the_daily_research_cap_is_the_mandate_s_own_and_fails_closed() {
    let cap = |edit: &dyn Fn(&mut Json)| {
        let mut document: Json = serde_json::from_str(MANDATE).unwrap();
        document["behavior"]["research"] = json!({
            "interval_s": 3600,
            "cost_cap_usd_per_day": "5",
            "max_revisions_per_lineage": 3
        });
        edit(&mut document);
        research_daily_cap(&parse(document.to_string().as_bytes()).unwrap())
    };
    let with = |value: Json| {
        move |document: &mut Json| {
            document["behavior"]["research"]["cost_cap_usd_per_day"] = value.clone()
        }
    };
    assert_eq!(cap(&|_| {}), Ok(Usd6::from_micros(5_000_000)));
    assert_eq!(cap(&with(json!("2.5"))), Ok(Usd6::from_micros(2_500_000)));
    assert_eq!(cap(&with(json!("0.000001"))), Ok(Usd6::from_micros(1)));
    assert_eq!(
        cap(&|document| document["behavior"]["research"] = Json::Null),
        Ok(Usd6::ZERO),
        "no research envelope, no research spend"
    );
    let unavailable = Err(CellError::Mandate("mandate_document_unavailable"));
    for (case, edit) in [
        (
            "no behavior.research",
            Box::new(|document: &mut Json| {
                document["behavior"]
                    .as_object_mut()
                    .unwrap()
                    .remove("research");
            }) as Box<dyn Fn(&mut Json)>,
        ),
        (
            "no cap",
            Box::new(|document: &mut Json| {
                document["behavior"]["research"]
                    .as_object_mut()
                    .unwrap()
                    .remove("cost_cap_usd_per_day");
            }),
        ),
        ("a number", Box::new(with(json!(5)))),
        ("no amount", Box::new(with(json!("five")))),
        ("below a micro-dollar", Box::new(with(json!("0.0000001")))),
        ("negative", Box::new(with(json!("-1")))),
    ] {
        assert_eq!(cap(&*edit), unavailable, "{case}");
    }
}

/// DEC-2259 item 1: research over a meter the process gives, its journaled meter stream, is the
/// same research the in-memory meter wires: the same configuration and the same references.
#[test]
fn research_over_a_given_meter_is_the_same_research() {
    let (root, control, version, _) = deployed("metered");
    let meter = JournalMeter::open(
        MemoryStore::default(),
        "ws1",
        Environment::Paper,
        Rc::new(|| UtcNanos::from_parts(1_792_180_800, 0).unwrap()),
        Box::new(SequentialIds::starting_at(1 << 70)),
    );
    let metered = research_metered(
        opted(),
        Box::new(control.clone()),
        Box::new(meter),
        &root,
        now(),
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    let (plain_root, plain_control, _, _) = deployed("plain");
    let plain = research(opted(), Box::new(plain_control), &plain_root, now()).unwrap();
    assert_eq!(metered.wiring.config, plain.wiring.config);
    let draft = EventDraft {
        event_id: EventId("01J8Z3MC000000000000000001".to_owned()),
        event_type: "RunStarted".to_owned(),
        causation_id: None,
        payload: Value::Null,
    };
    let (_, refs) = metered.refs.refs(&draft).unwrap();
    assert_eq!(
        refs.get("mandate_version").and_then(Value::as_str),
        Some(version.as_str())
    );
}

/// DEC-2258 item 2: a workspace's meter stream opens with an id drawn from its own name, the same
/// at every start, and no two workspaces' streams open with one id.
#[test]
fn each_workspace_s_meter_stream_opens_with_an_id_of_its_own() {
    let first = |workspace: &str| meter_opened_ids(workspace).next_id();
    assert_eq!(first("ws1"), first("ws1"));
    assert_ne!(first("ws1"), first("ws2"));
    assert_ne!(first("ws2"), first("ws3"));
    let mut ids = meter_opened_ids("ws1");
    let (one, two) = (ids.next_id(), ids.next_id());
    assert_ne!(one, two);
}

/// A copy of the agentic fixture with `edit` applied to its `research.json`, at a directory of its
/// own for test `name`.
fn agentic_edited(name: &str, edit: impl FnOnce(&mut Json)) -> PathBuf {
    let dir =
        std::env::temp_dir().join(format!("daemon-{}-cell-agentic-{name}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    for entry in fs::read_dir(AGENTIC).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, dir.join(path.file_name().unwrap())).unwrap();
    }
    let file = dir.join("research.json");
    let mut value: Json = serde_json::from_slice(&fs::read(&file).unwrap()).unwrap();
    edit(&mut value);
    fs::write(&file, value.to_string()).unwrap();
    dir
}

/// The core's loop for the agentic fixture, typed out from the fixture and DEC-2262's tools.
fn expected_loop() -> AgenticConfig {
    let tools = BTreeSet::from([
        Tool::FilingsGet,
        Tool::FilingsSearch,
        Tool::MarketBars,
        Tool::MarketQuote,
        Tool::NewsGet,
        Tool::NewsSearch,
        Tool::Note,
        Tool::Plan,
    ]);
    AgenticConfig {
        output: OutputMode::FinalOutput,
        bounds: LoopBounds::v0_2(),
        ideate_menu: tools.clone(),
        monitor_menu: tools,
        cost_ceiling: Usd6::from_micros(1_500_000),
    }
}

/// DEC-2268 item 1: a recording's `caps.run_usd` is the workspace's run cap, absent when it records
/// none, and a malformed one refuses the load.
#[test]
fn a_recording_s_run_cap_is_read_when_it_records_one() {
    assert_eq!(
        Bundle::load(Path::new(AGENTIC)).unwrap().run_cap,
        Some(Usd6::from_micros(1_500_000))
    );
    assert_eq!(Bundle::load(Path::new(FIXTURE)).unwrap().run_cap, None);
    let dir = agentic_edited("bad-cap", |file| {
        file["caps"]["run_usd"] = json!("a dollar")
    });
    assert_eq!(
        Bundle::load(&dir).map(|_| ()),
        Err(BundleError::Malformed {
            file: "research.json",
            member: "run_usd"
        })
    );
}

/// DEC-2268 items 2 and 3: an agentic bundle's research is the core's loop over the entry, and the
/// single-call bundle's stays single-call.
#[test]
#[ignore = "pending E19-13"]
fn an_agentic_bundle_s_research_runs_the_entry_s_loop() {
    let bundle = Bundle::load(Path::new(AGENTIC)).unwrap();
    assert_eq!(agentic_config(&bundle), Ok(expected_loop()));
    let (root, control, _, _) = deployed("agentic");
    let opted = opt_in(Path::new(AGENTIC), &paper(true)).unwrap_or_else(|e| panic!("{e:?}"));
    let wired = research(opted, Box::new(control), &root, now())
        .unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(wired.wiring.config.agentic, Some(expected_loop()));
    assert_eq!(wired.wiring.config.pin, bundle.entry.content().model_ref());
}

/// DEC-2268 item 3: no menu holds a tool that acts, and only declared tools are offered.
#[test]
#[ignore = "pending E19-13"]
fn neither_menu_offers_an_action_or_an_undeclared_tool() {
    let bundle = Bundle::load(Path::new(AGENTIC)).unwrap();
    let config = agentic_config(&bundle).unwrap_or_else(|e| panic!("{e:?}"));
    let content = bundle.entry.content();
    let declared = content.agentic().unwrap();
    for tool in config.ideate_menu.iter().chain(&config.monitor_menu) {
        assert_ne!(tool.class(), ToolClass::Action, "{tool:?}");
        assert!(declared.tool(tool.name()).is_some(), "{tool:?} is declared");
    }
}

/// DEC-2268 item 4 and §6.11: an agentic bundle that records no run cap is refused at the opt-in,
/// before any store, worker or call, so no agentic run starts without one.
#[test]
#[ignore = "pending E19-13"]
fn an_agentic_bundle_without_a_run_cap_is_refused_at_the_opt_in() {
    let dir = agentic_edited("no-cap", |file| {
        file["caps"].as_object_mut().unwrap().remove("run_usd");
    });
    let refused = opt_in(&dir, &paper(true)).map(|_| ());
    assert_eq!(
        refused,
        Err(CellError::Bundle(BundleError::Malformed {
            file: "research.json",
            member: "run_usd"
        }))
    );
}
