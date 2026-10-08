//! The deployment input from the control stream (E19-11, DEC-505; the brief's slice D3, part a):
//! each refusal E19-11 lists, the model registry present for V-007 (X-7), and the two facts the run
//! supplies. The stream is built here, record by record, in the shapes of journal spec §9.2's
//! vectors, around the E7-7 paper mandate; every expected value is read from those records.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value, to_canonical};
use mandate_num::Usd;
use mandate_shell::control::{ControlRecord, DeploymentRefusal, RunFacts, confirmed_version};
use mandate_spec::context::{AgentId, Membership};
use mandate_spec::{Mandate, Violation};
use mandate_time::Date;

const MANDATE: &str = include_str!("fixtures/tracer/mandate.json");
const AGENT: &str = "agent_spy";
const CONNECTION: &str = "conn_alpaca_paper_01";
const MODEL: &str = r#""content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","model_id":"quant.ma_crossover","model_version":"1.0.0""#;
const ENVELOPE: [&str; 11] = [
    "autonomy",
    "behavior",
    "capital",
    "connection_id",
    "environment",
    "goal",
    "name",
    "notifications",
    "protection",
    "risk",
    "universe",
];

fn json(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).unwrap()
}

fn record(seq: u64, event_type: &str, payload: &str) -> ControlRecord {
    let event_type = event_type.to_owned();
    ControlRecord {
        seq,
        event_type,
        payload: json(payload),
    }
}

/// A deployed, confirmed version of `document`: its model registered, its version created with
/// every envelope path entered by the user and confirmed, and the agent deployed with it.
struct Stream {
    version: Digest,
    records: Vec<ControlRecord>,
    store: BTreeMap<Digest, Vec<u8>>,
}

impl Stream {
    fn of(document: &Value, confirmed: &[&str]) -> Self {
        let bytes = to_canonical(document);
        let version = Digest::of(&bytes);
        let paths = |source: &str| -> Vec<String> {
            let path = |p: &&str| format!(r#"{{"path":"/{p}","source":"{source}"}}"#);
            ENVELOPE.iter().map(path).collect()
        };
        let confirmed: Vec<String> = confirmed.iter().map(|p| format!(r#""/{p}""#)).collect();
        let v = format!("sha256:{version}");
        let records = vec![
            record(
                1,
                "ConfigSnapshotRegistered",
                &format!(
                    r#"{{"admits_instruments":false,{MODEL},"kind":"model_version","params":["fast_periods","slow_periods"]}}"#,
                ),
            ),
            record(
                2,
                "MandateVersionCreated",
                &format!(
                    r#"{{"mandate_version":"{v}","provenance":[{}],"record_ref":"sha256:{}"}}"#,
                    paths("user_entered").join(","),
                    "7".repeat(64),
                ),
            ),
            record(
                3,
                "MandateConfirmed",
                &format!(
                    r#"{{"confirmed_paths":[{}],"mandate_version":"{v}"}}"#,
                    confirmed.join(","),
                ),
            ),
            record(
                4,
                "AgentDeployed",
                &format!(
                    r#"{{"agent_id":"{AGENT}","mandate_version":"{v}","record_ref":"sha256:{}"}}"#,
                    "6".repeat(64),
                ),
            ),
        ];
        Self {
            version,
            records,
            store: BTreeMap::from([(version, bytes)]),
        }
    }

    fn deployed() -> Self {
        Self::of(&json(MANDATE), &ENVELOPE)
    }

    fn then(mut self, event_type: &str, payload: &str) -> Self {
        let seq = self.records.last().map_or(1, |r| r.seq + 1);
        self.records.push(record(seq, event_type, payload));
        self
    }

    fn read(&self) -> Result<mandate_shell::control::ConfirmedVersion, DeploymentRefusal> {
        confirmed_version(&self.records, &self.store, &run())
    }
}

fn run() -> RunFacts {
    RunFacts {
        agent: AgentId::new(AGENT),
        validation_date: Date::parse("2026-10-07").unwrap(),
        account_equity_usd: Usd::parse("100000").unwrap(),
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
    }
}

fn violations(rules: &[Violation]) -> Result<(), DeploymentRefusal> {
    Err(DeploymentRefusal::Violations(
        rules.iter().copied().collect(),
    ))
}

/// The input is the deployed version: its digest, the stored document parsed, and a context with
/// the registry present (X-7), every envelope path confirmed, the run's equity, and `paper` for a
/// connection the stream says nothing about (DEC-505 items 1 and 3).
#[test]
#[ignore = "pending E19-11"]
fn the_input_is_the_deployed_confirmed_version() {
    let stream = Stream::deployed();
    let input = stream.read();
    let version = input.as_ref().map(|i| i.version);
    assert_eq!(version, Ok(stream.version), "the AgentDeployed version");
    let input = input.unwrap();
    assert_eq!(input.mandate, Mandate::parse(&json(MANDATE)).unwrap());
    let registered = input.context.registry.as_ref().map(BTreeMap::len);
    assert_eq!(registered, Some(1), "the one model_version registration");
    assert_eq!(input.context.account_equity_usd, run().account_equity_usd);
    let paper = Some(mandate_domain::Environment::Paper);
    assert_eq!(input.context.connection_environment, paper);
}

#[test]
#[ignore = "pending E19-11"]
fn without_a_deployment_of_this_agent_there_is_no_input() {
    let mut stream = Stream::deployed();
    stream.records.retain(|r| r.event_type != "AgentDeployed");
    assert_eq!(
        stream.read().map(|_| ()),
        Err(DeploymentRefusal::NotDeployed)
    );
    let other = Stream::deployed();
    let other = other.records[3].payload.clone();
    let text = String::from_utf8(to_canonical(&other))
        .unwrap()
        .replace(AGENT, "agent_other");
    let stream = Stream::deployed().then("AgentDeployed", &text);
    let mut only_other = stream;
    only_other.records.remove(3);
    assert_eq!(
        only_other.read().map(|_| ()),
        Err(DeploymentRefusal::NotDeployed)
    );
}

/// An `AgentStopped` after the latest `AgentDeployed` refuses, another agent's does not, and a
/// deployment after the stop counts.
#[test]
#[ignore = "pending E19-11"]
fn a_stopped_agent_refuses_until_it_is_deployed_again() {
    let stop = format!(
        r#"{{"agent_id":"{AGENT}","connection_id":"{CONNECTION}","loss_added":"0","reason":"owner_stop","retired_on":"2026-10-06"}}"#
    );
    let other = stop.replace(AGENT, "agent_other");
    let others = Stream::deployed().then("AgentStopped", &other);
    let deployed = Ok(others.version);
    assert_eq!(
        others.read().map(|i| i.version),
        deployed,
        "another agent's stop"
    );
    let stopped = Stream::deployed().then("AgentStopped", &stop);
    assert_eq!(stopped.read().map(|_| ()), Err(DeploymentRefusal::Stopped));
    let deploy = to_canonical(&stopped.records[3].payload);
    let again = stopped.then("AgentDeployed", &String::from_utf8(deploy).unwrap());
    assert_eq!(again.read().map(|i| i.version), Ok(again.version));
}

/// The latest `AgentDeployed` names the version: a later one naming a document the store lacks
/// refuses rather than falling back to the earlier version.
#[test]
#[ignore = "pending E19-11"]
fn the_latest_deployment_counts_and_its_document_must_be_stored_intact() {
    let later = format!(
        r#"{{"agent_id":"{AGENT}","mandate_version":"sha256:{}","record_ref":"sha256:{}"}}"#,
        "8".repeat(64),
        "6".repeat(64),
    );
    let missing = Stream::deployed().then("AgentDeployed", &later);
    assert_eq!(
        missing.read().map(|_| ()),
        Err(DeploymentRefusal::DocumentMissing)
    );
    let mut corrupt = Stream::deployed();
    corrupt
        .store
        .values_mut()
        .for_each(|bytes| bytes.push(b' '));
    assert_eq!(
        corrupt.read().map(|_| ()),
        Err(DeploymentRefusal::DocumentCorrupt)
    );
}

/// A path the owner did not confirm (V-020; `/name`, which no other rule reads), a model
/// registered under another hash or not at all (V-007), and a connection revoked or established as
/// `live` (V-001) each refuse.
#[test]
#[ignore = "pending E19-11"]
fn every_v_rule_the_stream_decides_refuses() {
    let but_name: Vec<&str> = ENVELOPE.into_iter().filter(|p| *p != "name").collect();
    let unconfirmed = Stream::of(&json(MANDATE), &but_name);
    assert_eq!(
        unconfirmed.read().map(|_| ()),
        violations(&[Violation::V020])
    );
    let mut unregistered = Stream::deployed();
    unregistered.records.remove(0);
    assert_eq!(
        unregistered.read().map(|_| ()),
        violations(&[Violation::V007])
    );
    let mut mismatched = Stream::deployed();
    let other = to_canonical(&mismatched.records[0].payload);
    let other = String::from_utf8(other)
        .unwrap()
        .replace("4f3559", "000000");
    mismatched.records[0].payload = json(&other);
    assert_eq!(
        mismatched.read().map(|_| ()),
        violations(&[Violation::V007])
    );
    let revoked = format!(r#"{{"connection_id":"{CONNECTION}"}}"#);
    let revoked = Stream::deployed().then("ConnectionRevoked", &revoked);
    assert_eq!(revoked.read().map(|_| ()), violations(&[Violation::V001]));
    let live = format!(
        r#"{{"broker":"alpaca","connection_id":"{CONNECTION}","environment":"live","scopes":["trading"]}}"#
    );
    let live = Stream::deployed().then("ConnectionEstablished", &live);
    assert_eq!(live.read().map(|_| ()), violations(&[Violation::V001]));
}

/// A `live` mandate is refused before anything else is read about it.
#[test]
#[ignore = "pending E19-11"]
fn a_live_mandate_is_refused() {
    let live = json(&MANDATE.replace(r#""environment": "paper""#, r#""environment": "live""#));
    let stream = Stream::of(&live, &ENVELOPE);
    assert_ne!(
        stream.version,
        Stream::deployed().version,
        "the edit changed the document"
    );
    assert_eq!(stream.read().map(|_| ()), Err(DeploymentRefusal::Live));
}

/// Each refusal has its own stable code (ADR-0001 ES-09); a live test for the mutation gate.
#[test]
fn every_deployment_refusal_has_its_own_code() {
    let codes = [
        DeploymentRefusal::Unimplemented { story: "E19-11" },
        DeploymentRefusal::NotDeployed,
        DeploymentRefusal::Stopped,
        DeploymentRefusal::DocumentMissing,
        DeploymentRefusal::DocumentCorrupt,
        DeploymentRefusal::StoreUnavailable,
        DeploymentRefusal::DocumentUnreadable,
        DeploymentRefusal::Live,
        DeploymentRefusal::Fold,
        DeploymentRefusal::Violations(BTreeSet::new()),
    ]
    .map(|r| r.code());
    let want = [
        "unimplemented",
        "not_deployed",
        "stopped",
        "document_missing",
        "document_corrupt",
        "store_unavailable",
        "document_unreadable",
        "live",
        "fold",
        "violations",
    ];
    assert_eq!(codes, want);
}
