//! A runtime's deployment read from the journal and the mandate document it names (DEC-1016):
//! the latest `AgentDeployed` of the agent names the version, the stored document gives the
//! connection and the `user:` approvers, and the version's `MandateVersionCreated` gives its author.

use std::collections::BTreeMap;

use mandate_canon::{Digest, parse};
use mandate_daemon::deployment::{Resolved, resolved};
use mandate_journal::{ArtifactRef, Environment, StoredEvent};
use mandate_spec::document::Mandate;

const AGENT: &str = "agent-a";
const MANDATE: &str = include_str!("fixtures/mandate.json");

/// A control-stream row as the workspace services commit it, by `actor` (kind and id).
fn row(
    seq: u64,
    event_type: &str,
    environment: &str,
    actor: (&str, &str),
    payload: &str,
) -> StoredEvent {
    let (kind, id) = actor;
    let event_id = format!("01J8Z3MC{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"{environment}","event_id":"{event_id}","stream_id":"ctl:ws1","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"system","causation_id":null,"correlation_id":null,"actor":{{"kind":"{kind}","id":"{id}","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
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

/// A control stream and the store it names documents in.
#[derive(Default)]
struct World {
    control: Vec<StoredEvent>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl World {
    fn next(&self) -> u64 {
        u64::try_from(self.control.len()).unwrap() + 1
    }

    /// Stores the fixture mandate changed by `edit`, and journals its creation by `author`.
    fn version(
        &mut self,
        author: (&str, &str),
        edit: impl FnOnce(&mut serde_json::Value),
    ) -> String {
        let mut document: serde_json::Value = serde_json::from_str(MANDATE).unwrap();
        edit(&mut document);
        let mandate = Mandate::parse(&parse(document.to_string().as_bytes()).unwrap()).unwrap();
        let bytes = mandate.canonical_bytes().unwrap();
        let version = ArtifactRef::of(&bytes).to_string();
        self.store.insert(ArtifactRef::of(&bytes).digest(), bytes);
        let payload = format!(
            r#"{{"mandate_version":"{version}","provenance":[],"record_ref":"{version}"}}"#
        );
        let seq = self.next();
        self.control
            .push(row(seq, "MandateVersionCreated", "paper", author, &payload));
        version
    }

    fn deployed(&mut self, agent: &str, version: &str, environment: &str) {
        let payload = format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{version}"}}"#
        );
        let seq = self.next();
        self.control.push(row(
            seq,
            "AgentDeployed",
            environment,
            ("user", "user-owner"),
            &payload,
        ));
    }

    fn stopped(&mut self, agent: &str) {
        let payload = format!(
            r#"{{"agent_id":"{agent}","connection_id":"conn_alpaca_paper_01","reason":"owner_stop","retired_on":"2026-10-16","loss_added":"0"}}"#
        );
        let seq = self.next();
        self.control.push(row(
            seq,
            "AgentStopped",
            "paper",
            ("user", "user-owner"),
            &payload,
        ));
    }

    fn resolved(&self) -> Result<Resolved, &'static str> {
        resolved(&self.control, AGENT, Environment::Paper, &self.store)
    }
}

fn approvers(list: &[&str]) -> impl FnOnce(&mut serde_json::Value) {
    let list: Vec<String> = list.iter().map(|a| (*a).to_owned()).collect();
    move |d| d["autonomy"]["approval"]["approvers"] = serde_json::json!(list)
}

fn connection(id: &str) -> impl FnOnce(&mut serde_json::Value) {
    let id = id.to_owned();
    move |d| d["connection_id"] = serde_json::json!(id)
}

#[test]
fn a_deployed_agent_s_connection_approvers_and_author_come_from_its_mandate_document() {
    let mut world = World::default();
    let version = world.version(
        ("user", "user-author"),
        approvers(&["role:approver", "user:u-1", "user:u-2"]),
    );
    world.deployed(AGENT, &version, "paper");
    assert_eq!(
        world.resolved(),
        Ok(Resolved {
            connection: "conn_alpaca_paper_01".into(),
            approvers: vec!["u-1".into(), "u-2".into()],
            author: "user-author".into(),
        })
    );
}

/// A `role:` approver is not resolved here: the deny-all wiring never asks, and an approver set
/// without it can only make an approval harder to grant, never easier.
#[test]
fn only_named_users_are_taken_as_approvers() {
    let mut world = World::default();
    let version = world.version(("user", "user-author"), approvers(&["role:owner"]));
    world.deployed(AGENT, &version, "paper");
    assert_eq!(world.resolved().unwrap().approvers, Vec::<String>::new());
}

#[test]
fn the_latest_deployment_of_the_agent_names_its_version() {
    let mut world = World::default();
    let first = world.version(("user", "user-author"), connection("conn-first"));
    let second = world.version(("user", "user-second"), connection("conn-second"));
    world.deployed(AGENT, &first, "paper");
    world.deployed("agent-b", &first, "paper");
    world.deployed(AGENT, &second, "paper");
    world.deployed("agent-b", &first, "paper");
    let found = world.resolved().unwrap();
    assert_eq!(found.connection, "conn-second");
    assert_eq!(found.author, "user-second");
}

#[test]
fn an_agent_never_deployed_or_since_stopped_is_refused() {
    let mut world = World::default();
    let version = world.version(("user", "user-author"), |_| {});
    world.deployed("agent-b", &version, "paper");
    assert_eq!(world.resolved(), Err("agent_not_deployed"));
    world.deployed(AGENT, &version, "paper");
    assert!(world.resolved().is_ok());
    world.stopped(AGENT);
    assert_eq!(world.resolved(), Err("agent_not_deployed"));
    world.deployed(AGENT, &version, "paper");
    assert!(world.resolved().is_ok(), "a redeployment runs again");
}

#[test]
fn a_document_missing_corrupt_or_invalid_is_refused() {
    let mut world = World::default();
    let version = world.version(("user", "user-author"), |_| {});
    world.deployed(AGENT, &version, "paper");
    let digest = ArtifactRef::parse(&version).unwrap().digest();
    let stored = world.store.remove(&digest).unwrap();
    assert_eq!(world.resolved(), Err("mandate_document_unavailable"));
    world.store.insert(digest, b"tampered".to_vec());
    assert_eq!(world.resolved(), Err("mandate_document_unavailable"));
    world.store.insert(digest, stored);
    assert!(world.resolved().is_ok());

    let mut invalid = World::default();
    let bytes = br#"{"name":"not a mandate"}"#.to_vec();
    let reference = ArtifactRef::of(&bytes);
    invalid.store.insert(reference.digest(), bytes);
    invalid.deployed(AGENT, &reference.to_string(), "paper");
    assert_eq!(invalid.resolved(), Err("mandate_document_unavailable"));
}

/// A paper runtime never takes a live deployment or a live document (`AGENTS.md` rule 8).
#[test]
fn a_paper_runtime_never_resolves_a_live_deployment_or_document() {
    let mut world = World::default();
    let version = world.version(("user", "user-author"), |_| {});
    world.deployed(AGENT, &version, "live");
    assert_eq!(world.resolved(), Err("agent_not_deployed"));

    let mut live = World::default();
    let version = live.version(("user", "user-author"), |d| {
        d["environment"] = serde_json::json!("live");
    });
    live.deployed(AGENT, &version, "paper");
    assert_eq!(live.resolved(), Err("mandate_environment_mismatch"));
}

#[test]
fn a_version_with_no_user_who_created_it_is_refused() {
    let mut world = World::default();
    let version = world.version(("system", "importer"), |_| {});
    world.deployed(AGENT, &version, "paper");
    assert_eq!(world.resolved(), Err("mandate_author_unknown"));

    let mut unrecorded = World::default();
    let version = unrecorded.version(("user", "user-author"), |_| {});
    unrecorded
        .control
        .retain(|r| r.event_type != "MandateVersionCreated");
    unrecorded.deployed(AGENT, &version, "paper");
    assert_eq!(unrecorded.resolved(), Err("mandate_author_unknown"));
}
