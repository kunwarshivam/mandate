//! E12-1 slice A2: the causal trace's walk (workspace API §4.8.1 "Causal trace", AU-1, AU-3,
//! AU-4, DEC-761, DEC-762, DEC-772). Most graphs are `ModelOutputRecorded` events whose
//! `causation_id` and `payload.evidence[]` name any event of any workspace. Every expectation is
//! written out here from the fixture's own record; none calls the code under test.

mod common;

use std::collections::BTreeSet;
use std::slice::from_ref;

use common::{Fixture, T, WS_A, WS_B, actor, event_id, tenant, text};
use mandate_audit::{AuditError, Hop, HopStatus, MemoryRead, Trace, TraceRead};
use mandate_canon::Digest;
use mandate_identity::WorkspaceId;
use mandate_journal::StreamId;

const AGENTS: [&str; 2] = ["AG1", "AG2"];
const EV: &str = "payload.evidence[]";

fn agent(workspace: WorkspaceId, name: &str) -> String {
    format!("agent:{}:{name}", text(workspace))
}

fn quote(text: Option<&str>) -> String {
    text.map_or("null".to_owned(), |t| format!("\"{t}\""))
}

fn draft(id: &str, stream: &str, event_type: &str, caused: Option<&str>, payload: &str) -> Vec<u8> {
    let refs: BTreeSet<&str> = payload
        .split('"')
        .filter(|s| s.starts_with("sha256:"))
        .collect();
    format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{id}","stream_id":"{stream}",
        "event_type":"{event_type}","schema_version":1,"event_time":"{T}","clock_source":"local",
        "causation_id":{},"correlation_id":null,"actor":{},"config_refs":{{"mandate_version":
        "sha256:{}"}},"payload":{payload},"artifact_refs":[{}],"pii_refs":[]}}"#,
        quote(caused),
        actor(),
        "4".repeat(64),
        refs.iter()
            .map(|r| quote(Some(r)))
            .collect::<Vec<_>>()
            .join(",")
    )
    .into_bytes()
}

/// A `ModelOutputRecorded` payload; its model-authored members are `invalidation` and `thesis_ref`.
fn output(evidence: &[String], invalidation: Option<&str>, thesis_ref: Option<&str>) -> String {
    let evidence: Vec<String> = evidence.iter().map(|e| quote(Some(e))).collect();
    format!(
        r#"{{"model_id":"llm.research_agent","model_version":"1.0.0","content_hash":"sha256:{}",
        "instrument_id":"inst","as_of":"{T}","expires_at":"{T}","direction":"long",
        "conviction":"1","confidence":"1","horizon_s":60,"thesis_ref":{},"evidence":[{}],
        "invalidation":{},"thesis_id":null,"lineage_id":null,"ignored":null}}"#,
        "6".repeat(64),
        quote(thesis_ref),
        evidence.join(","),
        quote(invalidation)
    )
}

/// A journal of [`Fixture`]'s streams and an agent stream per [`AGENTS`] name in each workspace.
struct Graph {
    fx: Fixture,
}

impl Graph {
    fn new() -> Self {
        let mut fx = Fixture::new(0);
        for workspace in [WS_A, WS_B] {
            for name in AGENTS {
                let opened = format!(
                    r#"{{"stream_type":"agent","workspace_id":"{}","agent_id":"{name}"}}"#,
                    text(workspace)
                );
                fx.open(&agent(workspace, name), &opened);
            }
        }
        Self { fx }
    }

    fn add(&mut self, stream: &str, id: &str, causation: Option<&str>, evidence: &[String]) {
        let payload = output(evidence, None, None);
        let event = draft(id, stream, "ModelOutputRecorded", causation, &payload);
        self.fx.append_draft(stream, id.to_owned(), &event);
    }
}

fn hop(from: &str, link: &str, to: Option<&str>, status: HopStatus) -> Hop {
    Hop {
        from: from.to_owned(),
        link: link.to_owned(),
        to: to.map(str::to_owned),
        status,
    }
}

/// Traces `start` as `workspace` and checks what does not depend on the links: each node is the
/// journal's row, `as_of` holds the heads of the nodes' streams and of `also`, the streams the walk
/// read that hold no node (DEC-772 item 6), and no other workspace's stream id appears (AU-1).
fn traced(g: &Graph, workspace: WorkspaceId, start: &str, also: &[&str]) -> Trace {
    let read = MemoryRead::new(&g.fx.journal);
    let trace = read.trace(&tenant(workspace), start).unwrap();
    assert_eq!(trace.start, start);
    let mut streams: BTreeSet<String> = also.iter().map(|s| s.to_string()).collect();
    for node in &trace.nodes {
        let row = g.fx.journal.event(&node.event.event_id).unwrap();
        assert_eq!(node.event.stream_id, row.stream_id);
        assert_eq!(node.event.body, row.body);
        streams.insert(row.stream_id.clone());
    }
    let heads: Vec<(String, u64, Digest)> = streams
        .into_iter()
        .map(|s| {
            let rows = g.fx.journal.rows(&StreamId::parse(&s).unwrap());
            let last = Digest::of(&rows.last().unwrap().body);
            (s, rows.len() as u64, last)
        })
        .collect();
    let as_of: Vec<(String, u64, Digest)> = trace
        .as_of
        .iter()
        .map(|w| (w.stream_id.clone(), w.seq, w.hash))
        .collect();
    assert_eq!(as_of, heads);
    let shown = format!("{trace:?}");
    for stream in g.fx.appended.keys() {
        if stream.split(':').nth(1) != Some(text(workspace).as_str()) {
            assert!(!shown.contains(&format!("{stream:?}")), "{stream} leaks");
        }
    }
    trace
}

/// AU-1: links forged to name another workspace's events, the prefixed segment's, and absent ids
/// all read `not_recorded` with no `to`; a start outside the workspace is the one `NotFound`.
#[test]
#[ignore = "pending E12-1"]
fn forged_links_read_as_absent_ones_and_a_foreign_start_is_not_found() {
    let mut g = Graph::new();
    let (mine, theirs) = (event_id(710_001), event_id(710_002));
    let prefixed = g.fx.appended[&format!("ctl:{}0", text(WS_A))].event_ids[0].clone();
    g.add(&agent(WS_B, "AG1"), &theirs, Some(&mine), from_ref(&mine));
    let forged = [theirs.clone(), prefixed, event_id(999_999)];
    g.add(&agent(WS_A, "AG1"), &mine, Some(&theirs), &forged);
    let trace = traced(&g, WS_A, &mine, &[]);
    let lost = |link: &str| hop(&mine, link, None, HopStatus::NotRecorded);
    let expected = [lost("causation_id"), lost(EV), lost(EV), lost(EV)];
    assert_eq!((trace.nodes.len(), trace.hops), (1, expected.to_vec()));
    let read = MemoryRead::new(&g.fx.journal);
    for start in [theirs, event_id(999_999), "x".to_owned(), String::new()] {
        assert_eq!(read.trace(&tenant(WS_A), &start), Err(AuditError::NotFound));
    }
}
