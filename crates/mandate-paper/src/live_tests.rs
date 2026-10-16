//! The live run's start, its admission and its Robinhood session (E7-26; DEC-947 items 1 to 5,
//! the R0 rehearsal's R2, R3 and R4). `live_start` is driven in the default build over a
//! recorded live deployment, a stored document and ports whose session answers a scripted
//! snapshot, so no test logs in, holds a credential or reaches a host (ES-19, LT-1, `AGENTS.md`
//! rules 7 and 8). The Robinhood session is driven over a scripted tool seam on its own runtime.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;

use mandate_accounting::InstrumentId;
use mandate_alpaca::AlpacaPaperHttp;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::Environment;
use mandate_executor::BrokerPosition;
use mandate_journal::{ArtifactStore, StreamId};
use mandate_mcp::{CallClass, McpError, Tools};
use mandate_num::{Price, SignedQty, Usd};
use mandate_robinhood::{AccountSnapshot, RobinhoodConnector};
use mandate_shell::adapters::Sources;
use mandate_shell::control::{
    Build as BuiltWith, Confirmation, ConfirmedVersion, ControlRecord, DeploymentRefusal,
    DoorChecks, LiveAdmission, McpHost, RunFacts,
};
use mandate_shell::paper::{Artifacts, BrokerFacts};
use mandate_shell::robinhood::{ACCOUNT_UNREAD, RobinhoodBridge};
use mandate_shell::{Cause, ProductionCycle, Setup, ShellError, Stage};
use mandate_spec::Violation;
use mandate_spec::context::{AgentId, Membership};
use mandate_time::Date;
use serde_json::json;

use crate::{
    Broker, Connection, Dial, LapseRefusal, Linked, PaperError, Ports, REHEARSAL_HOST,
    RobinhoodEndpoint, Session, SystemClock, live_admission, live_start,
};

const MANDATE: &str = include_str!("../../mandate-shell/tests/fixtures/tracer/mandate.json");
const AGENT: &str = "agent_spy";
const CONNECTION: &str = "conn_alpaca_paper_01";
const ACCOUNT: &str = "acct_live_probe";
const NUMBER: &str = "RHSIM00000001";

/// The four records of a deployed, confirmed version (journal spec §9.2's shapes, as the shell's
/// own control tests write them): `@V` is the version, `@P` the provenance, `@C` the confirmed
/// paths.
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

fn canonical(text: &str) -> Value {
    mandate_canon::parse(text.as_bytes()).expect("the fixture parses")
}

fn record(seq: u64, event_type: &str, payload: &str) -> ControlRecord {
    ControlRecord {
        seq,
        event_type: event_type.to_owned(),
        payload: canonical(payload),
    }
}

/// A live deployment of the tracer mandate on a live Robinhood connection, its document stored
/// under `name`, and `extra` records after it.
fn deployment(name: &str, extra: &[(&str, &str)]) -> (Vec<ControlRecord>, FsArtifactStore) {
    let live = MANDATE.replace(r#""environment": "paper""#, r#""environment": "live""#);
    let bytes = to_canonical(&canonical(&live));
    let version = Digest::of(&bytes);
    let root = std::env::temp_dir().join(format!("live-start-{name}-{}", std::process::id()));
    if root.exists() {
        fs::remove_dir_all(&root).expect("the old store is removed");
    }
    let mut store = FsArtifactStore::open(&root).expect("the store opens");
    store.put_artifact(&bytes).expect("the document is stored");
    let paths: Vec<&str> = ENVELOPE.split(' ').collect();
    let provenance: Vec<String> = paths
        .iter()
        .map(|p| format!(r#"{{"path":"/{p}","source":"user_entered"}}"#))
        .collect();
    let confirmed: Vec<String> = paths.iter().map(|p| format!(r#""/{p}""#)).collect();
    let fill = |payload: &str| {
        payload
            .replace("@V", &format!("sha256:{version}"))
            .replace("@P", &provenance.join(","))
            .replace("@C", &confirmed.join(","))
    };
    let established = format!(
        r#"{{"account_ref":"{ACCOUNT}","broker":"robinhood","connection_id":"{CONNECTION}","environment":"live","scopes":["trading"]}}"#
    );
    let mut records: Vec<ControlRecord> = (1..)
        .zip(RECORDS)
        .map(|(seq, (event_type, payload))| record(seq, event_type, &fill(payload)))
        .collect();
    records.push(record(5, "ConnectionEstablished", &established));
    for (seq, (event_type, payload)) in (6..).zip(extra) {
        records.push(record(seq, event_type, payload));
    }
    (records, store)
}

fn endpoint() -> RobinhoodEndpoint {
    RobinhoodEndpoint {
        host: REHEARSAL_HOST.to_owned(),
        url: format!("http://{REHEARSAL_HOST}:8080/mcp"),
    }
}

fn the_dial() -> Dial {
    Dial {
        endpoint: endpoint(),
        account_number: NUMBER.to_owned(),
    }
}

/// The run's facts before any credential: every condition of DEC-924 item 1 but the door's.
fn before() -> RunFacts {
    RunFacts {
        agent: AgentId::new(AGENT),
        validation_date: Date::parse("2026-10-07").expect("the date parses"),
        membership: Membership {
            workspace_users: 1,
            approver_users: 1,
        },
        live: LiveAdmission {
            build: BuiltWith::Live,
            confirmation: Confirmation::ConfirmLive,
            door: DoorChecks::NotPassed,
            host: McpHost {
                dialed: Some(REHEARSAL_HOST.to_owned()),
                pinned: Some(REHEARSAL_HOST),
            },
        },
    }
}

fn live_robinhood() -> Connection {
    Connection {
        broker: Broker::Robinhood,
        environment: Environment::Live,
        account_ref: ACCOUNT.to_owned(),
    }
}

fn stream() -> StreamId {
    StreamId::parse(&format!("acct:ws_live:{ACCOUNT}")).expect("the stream parses")
}

fn usd(text: &str) -> Usd {
    Usd::parse(text).expect("the amount parses")
}

fn flat() -> AccountSnapshot {
    AccountSnapshot {
        cash: usd("100"),
        buying_power: usd("100"),
        positions: Vec::new(),
        open_orders: Vec::new(),
    }
}

/// A session that reads only the scripted snapshot, or its refusal.
#[derive(Clone)]
struct Snapshotting(Result<Option<AccountSnapshot>, &'static str>);

impl Session for Snapshotting {
    fn preflight<P: mandate_alpaca::Pause + Clone>(
        &self,
        _artifacts: &Artifacts,
        _pause: P,
    ) -> Result<BrokerFacts, Cause> {
        Err(Cause::Absent { what: "unread" })
    }

    fn cycle(self, _sources: Sources<()>, _setup: Setup) -> Result<ProductionCycle, Cause> {
        Err(Cause::Absent { what: "unread" })
    }

    fn broker_ask(&self, _facts: &BrokerFacts) -> Option<Price> {
        None
    }

    fn account_snapshot(&self) -> Result<Option<AccountSnapshot>, Cause> {
        self.0.clone().map_err(|what| Cause::Absent { what })
    }
}

/// Ports whose `connect` counts itself, keeps the dial it was handed and answers the scripted
/// session, or `Credentials` when there is none.
struct LivePorts {
    session: Option<Snapshotting>,
    connects: Cell<usize>,
    dials: RefCell<Vec<Option<Dial>>>,
}

impl LivePorts {
    fn answering(snapshot: Result<Option<AccountSnapshot>, &'static str>) -> Self {
        Self {
            session: Some(Snapshotting(snapshot)),
            connects: Cell::new(0),
            dials: RefCell::new(Vec::new()),
        }
    }
}

impl Ports for LivePorts {
    type Session = Snapshotting;
    type Pause = SystemClock;

    fn control_stream(&mut self, _workspace: &str) -> Result<Vec<ControlRecord>, PaperError> {
        Err(PaperError::Control)
    }

    fn connect(
        &mut self,
        _connection: &Connection,
        dial: Option<&Dial>,
    ) -> Result<Snapshotting, PaperError> {
        self.connects.set(self.connects.get().saturating_add(1));
        self.dials.borrow_mut().push(dial.cloned());
        self.session.clone().ok_or(PaperError::Credentials)
    }

    fn harden(&mut self) -> Result<(), PaperError> {
        Ok(())
    }

    fn pause(&self) -> SystemClock {
        SystemClock
    }
}

fn start(
    name: &str,
    extra: &[(&str, &str)],
    facts: &RunFacts,
    ports: &mut LivePorts,
) -> Result<ConfirmedVersion, PaperError> {
    let (records, store) = deployment(name, extra);
    live_start(
        &records,
        &store,
        facts,
        &live_robinhood(),
        &stream(),
        None,
        &the_dial(),
        ports,
    )
    .map(|(confirmed, _)| confirmed)
}

/// R2 and R3 (DEC-947 items 1 and 2): a live deployment that passes every check needing no
/// credential connects once, with the run's own dial, reads the agentic account's snapshot,
/// passes the lapse on it, and is confirmed by the shell's full fold with the door's condition
/// now held.
#[test]
fn a_flat_live_account_starts_after_one_connect_with_the_door_passed() {
    let mut ports = LivePorts::answering(Ok(Some(flat())));
    let confirmed = start("flat", &[], &before(), &mut ports).expect("the live run starts");
    assert_eq!(confirmed.mandate().environment, Environment::Live);
    assert_eq!(ports.connects.get(), 1);
    assert_eq!(ports.dials.borrow().as_slice(), [Some(the_dial())]);
}

/// FT-3 as DEC-947 item 2 keeps it: every check the fold makes without a credential refuses
/// before the one connect — a condition of the admission missing, a V-rule, a stopped agent.
#[test]
fn every_check_without_a_credential_refuses_before_the_connect() {
    let unconfirmed = RunFacts {
        live: LiveAdmission {
            confirmation: Confirmation::Absent,
            ..before().live
        },
        ..before()
    };
    let other_host = RunFacts {
        live: LiveAdmission {
            host: McpHost {
                dialed: Some("127.1.2.3".to_owned()),
                pinned: Some(REHEARSAL_HOST),
            },
            ..before().live
        },
        ..before()
    };
    for (name, facts) in [("unconfirmed", unconfirmed), ("other-host", other_host)] {
        let mut ports = LivePorts::answering(Ok(Some(flat())));
        let refused = start(name, &[], &facts, &mut ports).err();
        assert!(
            matches!(
                refused,
                Some(PaperError::Deployment(DeploymentRefusal::Live))
            ),
            "{name}: {refused:?}"
        );
        assert_eq!(ports.connects.get(), 0, "{name}");
    }
    let revoked = format!(r#"{{"connection_id":"{CONNECTION}"}}"#);
    let mut ports = LivePorts::answering(Ok(Some(flat())));
    let refused = start(
        "revoked",
        &[("ConnectionRevoked", &revoked)],
        &before(),
        &mut ports,
    )
    .err();
    let v001 = DeploymentRefusal::Violations([Violation::V001].into_iter().collect());
    assert!(
        matches!(&refused, Some(PaperError::Deployment(found)) if *found == v001),
        "{refused:?}"
    );
    assert_eq!(ports.connects.get(), 0);
    let stopped = format!(r#"{{"agent_id":"{AGENT}"}}"#);
    let mut ports = LivePorts::answering(Ok(Some(flat())));
    let refused = start(
        "stopped",
        &[("AgentStopped", &stopped)],
        &before(),
        &mut ports,
    )
    .err();
    assert!(
        matches!(
            refused,
            Some(PaperError::Deployment(DeploymentRefusal::Stopped))
        ),
        "{refused:?}"
    );
    assert_eq!(ports.connects.get(), 0);
}

/// DEC-529 item 3 on the real snapshot (DEC-947 item 1): an account holding a position refuses
/// `account_not_flat`, a session that reads no snapshot refuses `lapse_evidence_not_supplied`
/// (DEC-919), an unreadable snapshot refuses at the reconcile stage, and a failed connect is the
/// credentials' refusal; each after one connect at most, before anything is sent.
#[test]
fn the_lapse_refuses_on_the_snapshot_the_session_read() {
    let held = AccountSnapshot {
        positions: vec![BrokerPosition {
            instrument: InstrumentId::new("SPY").expect("the instrument parses"),
            qty: SignedQty::parse("1").expect("the quantity parses"),
            avg_entry_price: Price::parse("85").expect("the price parses"),
        }],
        ..flat()
    };
    let mut ports = LivePorts::answering(Ok(Some(held)));
    let refused = start("held", &[], &before(), &mut ports).err();
    assert!(
        matches!(
            refused,
            Some(PaperError::Lapse(LapseRefusal::NotFlat {
                positions: 1,
                open_orders: 0
            }))
        ),
        "{refused:?}"
    );
    let mut ports = LivePorts::answering(Ok(None));
    let refused = start("unsupplied", &[], &before(), &mut ports).err();
    assert!(
        matches!(refused, Some(PaperError::Lapse(LapseRefusal::NotSupplied))),
        "{refused:?}"
    );
    let mut ports = LivePorts::answering(Err(ACCOUNT_UNREAD));
    let refused = start("unread", &[], &before(), &mut ports).err();
    assert!(
        matches!(
            refused,
            Some(PaperError::Shell(ShellError::Refused {
                stage: Stage::Reconcile,
                cause: Cause::Absent { what: ACCOUNT_UNREAD }
            }))
        ),
        "{refused:?}"
    );
    let mut ports = LivePorts {
        session: None,
        ..LivePorts::answering(Ok(None))
    };
    let refused = start("no-session", &[], &before(), &mut ports).err();
    assert!(
        matches!(refused, Some(PaperError::Credentials)),
        "{refused:?}"
    );
    assert_eq!(ports.connects.get(), 1);
}

/// R3 (DEC-947 item 3): the admission is the run's own facts, one each — the build, the
/// acknowledgement, the door's answer, and the host dialed beside the build's pin — and nothing
/// else; the shell's `admits` then judges them.
#[test]
fn the_admission_is_the_runs_own_four_facts() {
    let dialed = endpoint();
    let all = live_admission(
        BuiltWith::Live,
        Environment::Live,
        DoorChecks::Passed,
        Some(&dialed),
        Some(REHEARSAL_HOST),
    );
    let expected = LiveAdmission {
        build: BuiltWith::Live,
        confirmation: Confirmation::ConfirmLive,
        door: DoorChecks::Passed,
        host: McpHost {
            dialed: Some(REHEARSAL_HOST.to_owned()),
            pinned: Some(REHEARSAL_HOST),
        },
    };
    assert_eq!(all, expected);
    assert!(all.admits());
    let paper = live_admission(
        BuiltWith::Default,
        Environment::Paper,
        DoorChecks::NotPassed,
        None,
        None,
    );
    assert_eq!(paper, LiveAdmission::refusing());
    let paper_acknowledged = live_admission(
        BuiltWith::Live,
        Environment::Paper,
        DoorChecks::Passed,
        Some(&dialed),
        Some(REHEARSAL_HOST),
    );
    assert_eq!(paper_acknowledged.confirmation, Confirmation::Absent);
    assert!(!paper_acknowledged.admits());
    let unpinned = live_admission(
        BuiltWith::Live,
        Environment::Live,
        DoorChecks::Passed,
        Some(&dialed),
        None,
    );
    assert_eq!(unpinned.host.pinned, None);
    assert!(!unpinned.admits());
}

type Calls = Rc<RefCell<Vec<&'static str>>>;

/// The tool seam with one scripted answer per tool, or a refusal for a tool it holds none for.
struct Scripted {
    answers: BTreeMap<&'static str, serde_json::Value>,
    calls: Calls,
}

impl Tools for Scripted {
    async fn call_tool(
        &self,
        _class: CallClass,
        tool: &'static str,
        _arguments: &serde_json::Value,
    ) -> Result<String, McpError> {
        self.calls.borrow_mut().push(tool);
        let content = self.answers.get(tool).ok_or(McpError::Network)?;
        Ok(json!({"structuredContent": content, "isError": false}).to_string())
    }
}

fn linked(answers: BTreeMap<&'static str, serde_json::Value>) -> (Linked<Scripted>, Calls) {
    let calls = Calls::default();
    let tools = Scripted {
        answers,
        calls: Rc::clone(&calls),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("the runtime builds");
    let bridge = RobinhoodBridge::new(runtime, RobinhoodConnector::new(tools, NUMBER.to_owned()));
    (Linked::new(bridge, None::<AlpacaPaperHttp>), calls)
}

/// R4 (DEC-947 item 4): the Robinhood session reads the agentic account's snapshot through its
/// one connector, the account reads alone; a refused read is the account's unread refusal.
#[test]
fn the_robinhood_session_reads_the_agentic_accounts_snapshot() {
    let answers = BTreeMap::from([
        (
            "get_accounts",
            json!({"accounts": [{"account_number": NUMBER, "agentic_allowed": true}]}),
        ),
        (
            "get_portfolio",
            json!({"account_number": NUMBER, "cash": "100", "buying_power": "100"}),
        ),
        ("get_equity_positions", json!({"positions": []})),
        ("get_equity_orders", json!({"orders": []})),
    ]);
    let (session, calls) = linked(answers);
    let snapshot = session.account_snapshot().expect("the snapshot reads");
    assert_eq!(snapshot, Some(flat()));
    assert!(
        calls.borrow().iter().all(|tool| tool.starts_with("get_")),
        "{:?}",
        calls.borrow()
    );
    assert!(calls.borrow().contains(&"get_portfolio"));
    let (session, calls) = linked(BTreeMap::new());
    let refused = session.account_snapshot();
    assert!(
        matches!(refused, Err(Cause::Absent { what }) if what == ACCOUNT_UNREAD),
        "{refused:?}"
    );
    assert_eq!(calls.borrow().as_slice(), ["get_accounts"]);
}
