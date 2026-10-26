//! The deployment input from the control stream (E19-11, DEC-505; the brief's slice D3, part a):
//! each refusal E19-11 lists, the model registry present for V-007 (X-7), the connection fact the
//! run supplies, and V-002 left to the second phase, on the preflight's equity. The stream is built
//! here, record by record, in the shapes of journal spec §9.2's vectors, around the E7-7 paper
//! mandate; every expected value is read from those records.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::Environment;
use mandate_journal::{ArtifactError, ArtifactRef, ArtifactSource};
use mandate_num::Usd;
use mandate_shell::control::{
    Build, Confirmation, ConfirmedVersion, ControlRecord, DeploymentRefusal as Refusal, DoorChecks,
    LiveAdmission, McpHost, Origin, RunFacts, checked_before_credentials, confirmed_version,
};
use mandate_spec::context::{AgentId, Membership};
use mandate_spec::{Mandate, Violation};
use mandate_time::Date;
use proptest::prelude::*;

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
        event_time: None,
        event_type,
        payload: json(payload),
        origin: Origin {
            stream_id: "ctl:ws_spy".to_owned(),
            environment: Environment::Paper,
        },
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

    /// Phase 1 for a run that supplies `live` as DEC-924 item 1's admission.
    fn read_admitting(&self, live: LiveAdmission) -> Result<ConfirmedVersion, Refusal> {
        let run = RunFacts { live, ..run() };
        confirmed_version(&self.records, &self.store, &run)
    }

    fn refused(&self) -> Option<Refusal> {
        self.read().err()
    }

    /// The fold's checks before any credential, for a run that supplies `live` (DEC-947 item 2).
    fn checked(&self, live: LiveAdmission) -> Result<(), Refusal> {
        let run = RunFacts { live, ..run() };
        checked_before_credentials(&self.records, &self.store, &run)
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
        live: LiveAdmission::refusing(),
    }
}

/// An `AgentStopped` for `agent` (journal spec §9.2's shape).
fn stop(agent: &str) -> String {
    format!(
        r#"{{"agent_id":"{agent}","connection_id":"{CONNECTION}","loss_added":"0","reason":"owner_stop","retired_on":"2026-10-06"}}"#
    )
}

/// An artifact store that cannot be read.
struct Unavailable;
impl ArtifactSource for Unavailable {
    fn read_artifact(&self, _: &ArtifactRef) -> Result<Vec<u8>, ArtifactError> {
        Err(ArtifactError::Unavailable)
    }
}

fn violations(rules: &[Violation]) -> Option<Refusal> {
    Some(Refusal::Violations(rules.iter().copied().collect()))
}

/// The input is the deployed version, with no equity given: its digest, the stored document
/// parsed, and a context with the registry present (X-7), every envelope path confirmed, and `paper`
/// for a connection the stream says nothing about (DEC-505 items 1 and 3).
#[test]
fn the_input_is_the_deployed_confirmed_version() {
    let stream = Stream::deployed();
    let input = stream.read();
    let version = input.as_ref().map(|i| i.version());
    assert_eq!(version, Ok(stream.version), "the AgentDeployed version");
    let input = input.unwrap();
    assert_eq!(*input.mandate(), Mandate::parse(&json(MANDATE)).unwrap());
    let registered = input.context().registry.as_ref().map(BTreeMap::len);
    assert_eq!(registered, Some(1), "the one model_version registration");
    let paper = Some(mandate_domain::Environment::Paper);
    assert_eq!(input.context().connection_environment, paper);
}

#[test]
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
fn a_stopped_agent_refuses_until_it_is_deployed_again() {
    let others = Stream::deployed().then("AgentStopped", &stop("agent_other"));
    assert_eq!(others.refused(), None, "another agent's stop");
    let stopped = Stream::deployed().then("AgentStopped", &stop(AGENT));
    assert_eq!(stopped.refused(), Some(Refusal::Stopped));
    let deploy = stopped.text(3);
    let again = stopped.then("AgentDeployed", &deploy);
    assert_eq!(again.read().map(|i| i.version()), Ok(again.version));
}

/// The latest `AgentDeployed` names the version: a later one naming a document the store lacks
/// refuses rather than falling back to the earlier version.
#[test]
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

/// The records are read in `seq` order, not slice order: a stop at seq 5 listed before the
/// deployment at seq 4 still follows it.
#[test]
fn the_stream_is_read_in_seq_order() {
    let mut reordered = Stream::deployed().then("AgentStopped", &stop(AGENT));
    reordered.records.swap(3, 4);
    assert_eq!(reordered.refused(), Some(Refusal::Stopped));
}

/// A stored document that re-hashes but is no mandate, and a store that cannot be read, refuse.
#[test]
fn an_unreadable_document_or_store_refuses() {
    let unreadable = Stream::of(&json(r#"{"name":"not a mandate"}"#), &envelope());
    assert_eq!(unreadable.refused(), Some(Refusal::DocumentUnreadable));
    let stream = Stream::deployed();
    let refused = confirmed_version(&stream.records, &Unavailable, &run()).err();
    assert_eq!(refused, Some(Refusal::StoreUnavailable));
}

/// A path the owner did not confirm (V-020; `/name`, which no other rule reads), a confirmation of
/// another version or none (V-020 and V-022), a model registered under another hash or not at all
/// (V-007), and a connection revoked or established as `live` (V-001) each refuse.
#[test]
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
    assert_eq!(
        revoked.refused(),
        violations(&[Violation::V001, Violation::V008]),
        "a revoked connection's environment and profile are both unknown, and V-008 fails closed \
         on the profile (DEC-1152 item 1)"
    );
    let live = format!(
        r#"{{"broker":"alpaca","connection_id":"{CONNECTION}","environment":"live","scopes":["trading"]}}"#
    );
    let live = Stream::deployed().then("ConnectionEstablished", &live);
    assert_eq!(live.refused(), violations(&[Violation::V001]));
}

/// A `live` mandate is refused before anything else is read about it.
#[test]
fn a_live_mandate_is_refused() {
    let paper = Stream::deployed();
    assert_eq!(paper.refused(), None, "the paper document");
    let live = json(&live_document());
    let stream = Stream::of(&live, &envelope());
    assert_ne!(stream.version, paper.version, "the edit changed it");
    assert_eq!(stream.refused(), Some(Refusal::Live));
}

/// V-002 is phase 2's (DEC-505 item 3): the version phase 1 confirmed with no equity is refused on
/// a preflight equity below the allocation, and carries an equity that covers it.
#[test]
fn v_002_refuses_only_on_the_preflight_equity() {
    let input = Stream::deployed().read().unwrap();
    let usd = |text| Usd::parse(text).unwrap();
    let short = input.clone().with_equity(usd("999.99")).err();
    assert_eq!(short, violations(&[Violation::V002]), "below 1000");
    let input = input.with_equity(usd("1000")).unwrap();
    assert_eq!(input.context().account_equity_usd, usd("1000"));
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

/// A stand-in for the pinned Robinhood host constant the runner hands the shell (DEC-924 item 3):
/// a fixture name that reaches nothing, never Robinhood's own host.
const PINNED: &str = "mcp.pinned.example";

/// Every one of DEC-924 item 1's four conditions holding.
fn admitted() -> LiveAdmission {
    LiveAdmission {
        build: Build::Live,
        confirmation: Confirmation::ConfirmLive,
        door: DoorChecks::Passed,
        host: McpHost {
            dialed: Some(PINNED.to_owned()),
            pinned: Some(PINNED),
        },
    }
}

/// The E7-7 mandate made `live`, with the stop-limit offset V-008 asks for on the Robinhood profile
/// its live connection records (DEC-539 item 2, DEC-1152 item 1).
fn live_document() -> String {
    MANDATE
        .replace(r#""environment": "paper""#, r#""environment": "live""#)
        .replace(
            r#""crypto_stop_limit_offset": null"#,
            r#""crypto_stop_limit_offset": "0.01""#,
        )
}

/// The E7-7 mandate deployed as `live`, its connection established as a live Robinhood one, so
/// V-001 agrees and the live refusal is the only one the stream can draw.
fn live_deployment() -> Stream {
    let live = json(&live_document());
    let established = format!(
        r#"{{"broker":"robinhood","connection_id":"{CONNECTION}","environment":"live","scopes":["trading"]}}"#
    );
    Stream::of(&live, &envelope()).then("ConnectionEstablished", &established)
}

/// DEC-1152 item 2: a live mandate whose connection the stream does not record reads the profile
/// absent, so with no stop-limit offset the admitted fold refuses it V-008 beside V-001, never
/// reading it as Alpaca's paper row; with the offset, V-001 alone.
#[test]
fn a_live_mandate_on_an_unrecorded_connection_needs_the_stop_limit_offset() {
    let without = MANDATE.replace(r#""environment": "paper""#, r#""environment": "live""#);
    let stream = Stream::of(&json(&without), &envelope());
    assert_eq!(
        stream.read_admitting(admitted()).err(),
        violations(&[Violation::V001, Violation::V008])
    );
    let with = Stream::of(&json(&live_document()), &envelope());
    assert_eq!(
        with.read_admitting(admitted()).err(),
        violations(&[Violation::V001])
    );
}

/// DEC-924 item 1: a live deployment whose run holds all four conditions is admitted to the fold,
/// which confirms it on its live connection; the default every caller passes still refuses `Live`.
#[test]
fn a_live_deployment_holding_all_four_conditions_is_admitted() {
    let stream = live_deployment();
    assert_eq!(
        stream.refused(),
        Some(Refusal::Live),
        "the refusing default"
    );
    let input = stream.read_admitting(admitted());
    let input = input.expect("all four conditions admit");
    assert_eq!(input.version(), stream.version);
    assert_eq!(input.mandate().environment, Environment::Live);
    assert_eq!(
        input.context().connection_environment,
        Some(Environment::Live)
    );
}

/// DEC-924 item 2: any one condition missing refuses `Live`, each in turn, the host by absence,
/// by a different or empty name, by case and by a build that pins none.
#[test]
fn any_one_condition_missing_refuses_live() {
    let stream = live_deployment();
    let host = |dialed: Option<&str>, pinned: Option<&'static str>| McpHost {
        dialed: dialed.map(str::to_owned),
        pinned,
    };
    let missing = [
        (
            "the default build",
            LiveAdmission {
                build: Build::Default,
                ..admitted()
            },
        ),
        (
            "no --confirm-live",
            LiveAdmission {
                confirmation: Confirmation::Absent,
                ..admitted()
            },
        ),
        (
            "the door not passed",
            LiveAdmission {
                door: DoorChecks::NotPassed,
                ..admitted()
            },
        ),
        (
            "no host dialed",
            LiveAdmission {
                host: host(None, Some(PINNED)),
                ..admitted()
            },
        ),
        (
            "another host",
            LiveAdmission {
                host: host(Some("mcp.other.example"), Some(PINNED)),
                ..admitted()
            },
        ),
        (
            "the host in capitals",
            LiveAdmission {
                host: host(Some("MCP.PINNED.EXAMPLE"), Some(PINNED)),
                ..admitted()
            },
        ),
        (
            "no host pinned",
            LiveAdmission {
                host: host(Some(PINNED), None),
                ..admitted()
            },
        ),
        (
            "an empty pin",
            LiveAdmission {
                host: host(Some(""), Some("")),
                ..admitted()
            },
        ),
        ("every one", LiveAdmission::refusing()),
    ];
    for (case, live) in missing {
        assert!(!live.admits(), "{case}");
        assert_eq!(
            stream.read_admitting(live).err(),
            Some(Refusal::Live),
            "{case}"
        );
    }
    assert!(admitted().admits());
}

/// The dialed hosts the property draws: index 1 is the pinned name itself, the others differ from
/// every non-empty pin.
const DIALED: [Option<&str>; 6] = [
    None,
    Some(PINNED),
    Some(""),
    Some("MCP.PINNED.EXAMPLE"),
    Some("mcp.pinned.example."),
    Some("mcp.other.example"),
];
/// The pins the property draws: index 1 is the only usable one.
const PINS: [Option<&'static str>; 3] = [None, Some(PINNED), Some("")];

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 1024,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// DEC-924 items 1 and 2 over the 144 combinations, drawn 1024 times so the one admitted
    /// combination is all but certain to be drawn: a live deployment is admitted exactly when the
    /// independent oracle — the drawn indices alone, never `admits` or a string comparison — finds
    /// no condition missing, and refused `Live` otherwise; a paper deployment's answer is the
    /// same whatever the run supplies.
    #[test]
    fn no_combination_missing_a_condition_is_admitted(
        live_build in any::<bool>(),
        confirmed in any::<bool>(),
        passed in any::<bool>(),
        dialed in 0..DIALED.len(),
        pinned in 0..PINS.len(),
    ) {
        let live = LiveAdmission {
            build: if live_build { Build::Live } else { Build::Default },
            confirmation: if confirmed { Confirmation::ConfirmLive } else { Confirmation::Absent },
            door: if passed { DoorChecks::Passed } else { DoorChecks::NotPassed },
            host: McpHost { dialed: DIALED[dialed].map(str::to_owned), pinned: PINS[pinned] },
        };
        let missing = [!live_build, !confirmed, !passed, dialed != 1 || pinned != 1];
        let expected_admitted = missing.iter().all(|m| !m);
        prop_assert_eq!(live.admits(), expected_admitted);
        let answer = live_deployment().read_admitting(live.clone());
        if expected_admitted {
            prop_assert!(answer.is_ok(), "{:?}", answer.err());
        } else {
            prop_assert_eq!(answer.err(), Some(Refusal::Live));
        }
        let paper = Stream::deployed();
        prop_assert_eq!(paper.read_admitting(live), paper.read());
    }
}

/// DEC-947 item 2: before any credential, the fold runs every check it makes and admits a live
/// deployment on the three conditions a run holds before it reads one (the `live` build,
/// `--confirm-live`, and the dialed host equal to the pin); the door's own condition is the one
/// it leaves for after the snapshot. It confirms nothing: it answers no version, only whether the
/// run may go on to read the credential.
#[test]
fn before_credentials_the_fold_admits_on_every_condition_but_the_door() {
    let stream = live_deployment();
    let before = LiveAdmission {
        door: DoorChecks::NotPassed,
        ..admitted()
    };
    assert_eq!(stream.checked(before.clone()), Ok(()));
    assert_eq!(stream.checked(admitted()), Ok(()), "the door passed too");
    assert_eq!(
        stream.read_admitting(before.clone()).err(),
        Some(Refusal::Live),
        "the full fold still needs the door"
    );
    let host = |dialed: &str| McpHost {
        dialed: Some(dialed.to_owned()),
        pinned: Some(PINNED),
    };
    let missing = [
        (
            "the default build",
            LiveAdmission {
                build: Build::Default,
                ..before.clone()
            },
        ),
        (
            "no --confirm-live",
            LiveAdmission {
                confirmation: Confirmation::Absent,
                ..before.clone()
            },
        ),
        (
            "another host",
            LiveAdmission {
                host: host("mcp.other.example"),
                ..before.clone()
            },
        ),
        (
            "no host dialed",
            LiveAdmission {
                host: McpHost {
                    dialed: None,
                    pinned: Some(PINNED),
                },
                ..before.clone()
            },
        ),
        (
            "no host pinned",
            LiveAdmission {
                host: McpHost {
                    dialed: Some(PINNED.to_owned()),
                    pinned: None,
                },
                ..before.clone()
            },
        ),
        (
            "an empty pin",
            LiveAdmission {
                host: McpHost {
                    dialed: Some(String::new()),
                    pinned: Some(""),
                },
                ..before.clone()
            },
        ),
        ("every one", LiveAdmission::refusing()),
    ];
    for (case, live) in missing {
        assert_eq!(stream.checked(live), Err(Refusal::Live), "{case}");
    }
}

/// DEC-947 item 2: the check before credentials refuses exactly what the full fold refuses beyond
/// the door, each V-rule included, and a paper deployment answers as the full fold does.
#[test]
fn before_credentials_the_fold_refuses_every_v_rule_and_the_walk() {
    let before = LiveAdmission {
        door: DoorChecks::NotPassed,
        ..admitted()
    };
    let live = json(&live_document());
    let but_name: Vec<&str> = envelope().into_iter().filter(|p| *p != "name").collect();
    let established = format!(
        r#"{{"broker":"robinhood","connection_id":"{CONNECTION}","environment":"live","scopes":["trading"]}}"#
    );
    let unconfirmed = Stream::of(&live, &but_name).then("ConnectionEstablished", &established);
    assert_eq!(
        unconfirmed.checked(before.clone()).err(),
        violations(&[Violation::V020])
    );
    let paper_live = format!(
        r#"{{"broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","scopes":["trading"]}}"#
    );
    let mismatched = Stream::of(&live, &envelope()).then("ConnectionEstablished", &paper_live);
    assert_eq!(
        mismatched.checked(before.clone()).err(),
        violations(&[Violation::V001])
    );
    let stopped = live_deployment().then("AgentStopped", &stop(AGENT));
    assert_eq!(stopped.checked(before.clone()), Err(Refusal::Stopped));
    let mut undeployed = live_deployment();
    undeployed
        .records
        .retain(|r| r.event_type != "AgentDeployed");
    assert_eq!(
        undeployed.checked(before.clone()),
        Err(Refusal::NotDeployed)
    );
    let refused = checked_before_credentials(&live_deployment().records, &Unavailable, &run());
    assert_eq!(refused, Err(Refusal::StoreUnavailable));
    let paper = Stream::deployed();
    assert_eq!(paper.checked(before), Ok(()));
    assert_eq!(paper.checked(LiveAdmission::refusing()), Ok(()));
}
