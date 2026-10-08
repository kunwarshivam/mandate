//! The deployment input from the control stream (E19-11, DEC-505; the brief's slice D3, part a):
//! each refusal E19-11 lists, the model registry present for V-007 (X-7), the connection fact the
//! run supplies, and V-002 left to the second phase, on the preflight's equity. The stream is built
//! here, record by record, in the shapes of journal spec §9.2's vectors, around the E7-7 paper
//! mandate; every expected value is read from those records.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value, to_canonical};
use mandate_num::Usd;
use mandate_shell::control::{
    ConfirmedVersion, ControlRecord, DeploymentRefusal as Refusal, RunFacts, confirmed_version,
};
use mandate_spec::context::{AgentId, Membership};
use mandate_spec::{Mandate, Violation};
use mandate_time::Date;

const MANDATE: &str = include_str!("fixtures/tracer/mandate.json");
const AGENT: &str = "agent_spy";
const CONNECTION: &str = "conn_alpaca_paper_01";
/// The four records of a deployed, confirmed version: `@V` is the version, `@P` the provenance list
/// and `@C` the confirmed paths (journal spec §9.2's shapes).
const RECORDS: [(&str, &str); 4] = [
    (
        "ConfigSnapshotRegistered",
        r#"{"admits_instruments":false,"content_hash":"sha256:4f3559229f89b27c04b43ec773b0ff0b884895362622964b78dfd791d67e18fc","kind":"model_version","model_id":"quant.ma_crossover","model_version":"1.0.0","params":["fast_periods","slow_periods"]}"#,
    ),
    (
        "MandateVersionCreated",
        r#"{"mandate_version":"@V","provenance":[@P],"record_ref":"sha256:7777777777777777777777777777777777777777777777777777777777777777"}"#,
    ),
    (
        "MandateConfirmed",
        r#"{"confirmed_paths":[@C],"mandate_version":"@V"}"#,
    ),
    (
        "AgentDeployed",
        r#"{"agent_id":"agent_spy","mandate_version":"@V","record_ref":"sha256:6666666666666666666666666666666666666666666666666666666666666666"}"#,
    ),
];
const ENVELOPE: &str = "autonomy behavior capital connection_id environment goal name notifications protection risk universe";

fn envelope() -> Vec<&'static str> {
    ENVELOPE.split(' ').collect()
}

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
        let path = |p: &&str| format!(r#"{{"path":"/{p}","source":"user_entered"}}"#);
        let provenance: Vec<String> = envelope().iter().map(path).collect();
        let confirmed: Vec<String> = confirmed.iter().map(|p| format!(r#""/{p}""#)).collect();
        let fill = |payload: &str| {
            let payload = payload.replace("@V", &format!("sha256:{version}"));
            payload
                .replace("@P", &provenance.join(","))
                .replace("@C", &confirmed.join(","))
        };
        let records = (1..)
            .zip(RECORDS)
            .map(|(seq, (event_type, payload))| record(seq, event_type, &fill(payload)))
            .collect();
        Self {
            version,
            records,
            store: BTreeMap::from([(version, bytes)]),
        }
    }

    fn deployed() -> Self {
        Self::of(&json(MANDATE), &envelope())
    }

    /// Record `index`'s payload as canonical text.
    fn text(&self, index: usize) -> String {
        String::from_utf8(to_canonical(&self.records[index].payload)).unwrap()
    }

    /// The stream with `from` replaced by `to` in record `index`'s payload.
    fn edit(mut self, index: usize, from: &str, to: &str) -> Self {
        self.records[index].payload = json(&self.text(index).replace(from, to));
        self
    }

    fn then(mut self, event_type: &str, payload: &str) -> Self {
        let seq = self.records.last().map_or(1, |r| r.seq + 1);
        self.records.push(record(seq, event_type, payload));
        self
    }

    /// Phase 1, which takes no equity.
    fn read(&self) -> Result<ConfirmedVersion, Refusal> {
        confirmed_version(&self.records, &self.store, &run())
    }

    fn refused(&self) -> Option<Refusal> {
        self.read().err()
    }
}

fn run() -> RunFacts {
    RunFacts {
        agent: AgentId::new(AGENT),
        validation_date: Date::parse("2026-10-07").unwrap(),
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
    }
}

fn violations(rules: &[Violation]) -> Option<Refusal> {
    Some(Refusal::Violations(rules.iter().copied().collect()))
}

/// The input is the deployed version, with no equity given: its digest, the stored document
/// parsed, and a context with the registry present (X-7), every envelope path confirmed, and `paper`
/// for a connection the stream says nothing about (DEC-505 items 1 and 3).
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
    let paper = Some(mandate_domain::Environment::Paper);
    assert_eq!(input.context.connection_environment, paper);
}

#[test]
#[ignore = "pending E19-11"]
fn without_a_deployment_of_this_agent_there_is_no_input() {
    assert_eq!(Stream::deployed().refused(), None, "with it");
    let mut stream = Stream::deployed();
    stream.records.retain(|r| r.event_type != "AgentDeployed");
    assert_eq!(stream.refused(), Some(Refusal::NotDeployed));
    let only_other = Stream::deployed().edit(3, AGENT, "agent_other");
    assert_eq!(
        only_other.refused(),
        Some(Refusal::NotDeployed),
        "another's"
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
    assert_eq!(others.refused(), None, "another agent's stop");
    let stopped = Stream::deployed().then("AgentStopped", &stop);
    assert_eq!(stopped.refused(), Some(Refusal::Stopped));
    let deploy = stopped.text(3);
    let again = stopped.then("AgentDeployed", &deploy);
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
    assert_eq!(missing.refused(), Some(Refusal::DocumentMissing));
    let mut corrupt = Stream::deployed();
    corrupt.store.values_mut().for_each(|b| b.push(b' '));
    assert_eq!(corrupt.refused(), Some(Refusal::DocumentCorrupt));
}

/// A path the owner did not confirm (V-020; `/name`, which no other rule reads), a confirmation of
/// another version or none (V-020 and V-022), a model registered under another hash or not at all
/// (V-007), and a connection revoked or established as `live` (V-001) each refuse.
#[test]
#[ignore = "pending E19-11"]
fn every_v_rule_the_stream_decides_refuses() {
    let but_name: Vec<&str> = envelope().into_iter().filter(|p| *p != "name").collect();
    let unconfirmed = Stream::of(&json(MANDATE), &but_name);
    assert_eq!(unconfirmed.refused(), violations(&[Violation::V020]));
    let version = Stream::deployed().version.to_string();
    let elsewhere = Stream::deployed().edit(2, &version, &"9".repeat(64));
    let unconfirmed = violations(&[Violation::V020, Violation::V022]);
    assert_eq!(elsewhere.refused(), unconfirmed, "another version's");
    let mut never = Stream::deployed();
    never.records.remove(2);
    assert_eq!(never.refused(), unconfirmed, "no confirmation");
    let mut unregistered = Stream::deployed();
    unregistered.records.remove(0);
    let v007 = violations(&[Violation::V007]);
    assert_eq!(unregistered.refused(), v007, "unregistered");
    let mismatched = Stream::deployed().edit(0, "4f3559", "000000");
    assert_eq!(mismatched.refused(), v007, "another hash");
    let revoked = format!(r#"{{"connection_id":"{CONNECTION}"}}"#);
    let revoked = Stream::deployed().then("ConnectionRevoked", &revoked);
    assert_eq!(revoked.refused(), violations(&[Violation::V001]));
    let live = format!(
        r#"{{"broker":"alpaca","connection_id":"{CONNECTION}","environment":"live","scopes":["trading"]}}"#
    );
    let live = Stream::deployed().then("ConnectionEstablished", &live);
    assert_eq!(live.refused(), violations(&[Violation::V001]));
}

/// A `live` mandate is refused before anything else is read about it.
#[test]
#[ignore = "pending E19-11"]
fn a_live_mandate_is_refused() {
    let paper = Stream::deployed();
    assert_eq!(paper.refused(), None, "the paper document");
    let live = json(&MANDATE.replace(r#""environment": "paper""#, r#""environment": "live""#));
    let stream = Stream::of(&live, &envelope());
    assert_ne!(stream.version, paper.version, "the edit changed it");
    assert_eq!(stream.refused(), Some(Refusal::Live));
}

/// V-002 is phase 2's (DEC-505 item 3): the version phase 1 confirmed with no equity is refused on
/// a preflight equity below the allocation, and carries an equity that covers it.
#[test]
#[ignore = "pending E19-11"]
fn v_002_refuses_only_on_the_preflight_equity() {
    let input = Stream::deployed().read().unwrap();
    let usd = |text| Usd::parse(text).unwrap();
    let short = input.clone().with_equity(usd("999.99")).err();
    assert_eq!(short, violations(&[Violation::V002]), "below 1000");
    let input = input.with_equity(usd("1000")).unwrap();
    assert_eq!(input.context.account_equity_usd, usd("1000"));
}

/// Each refusal has its own stable code (ADR-0001 ES-09); a live test for the mutation gate.
#[test]
fn every_deployment_refusal_has_its_own_code() {
    let rows = [
        (Refusal::Unimplemented { story: "E19-11" }, "unimplemented"),
        (Refusal::NotDeployed, "not_deployed"),
        (Refusal::Stopped, "stopped"),
        (Refusal::DocumentMissing, "document_missing"),
        (Refusal::DocumentCorrupt, "document_corrupt"),
        (Refusal::StoreUnavailable, "store_unavailable"),
        (Refusal::DocumentUnreadable, "document_unreadable"),
        (Refusal::Live, "live"),
        (Refusal::Fold, "fold"),
        (Refusal::Violations(BTreeSet::new()), "violations"),
    ];
    for (refusal, code) in rows {
        assert_eq!(refusal.code(), code, "{refusal:?}");
    }
}
