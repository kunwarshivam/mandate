//! The live run's start, its admission and its Robinhood session (E7-26; DEC-947 items 1 to 5,
//! the R0 rehearsal's R2, R3 and R4). `live_start` is driven in the default build over a
//! recorded live deployment, a stored document and ports whose session answers a scripted
//! snapshot, so no test logs in, holds a credential or reaches a host (ES-19, LT-1, `AGENTS.md`
//! rules 7 and 8). The Robinhood session is driven over a scripted tool seam on its own runtime.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::rc::Rc;

use mandate_accounting::{InstrumentId, Side};
use mandate_alpaca::Pause;
use mandate_artifacts_fs::FsArtifactStore;
use mandate_canon::{Digest, Value, to_canonical};
use mandate_domain::Environment;
use mandate_executor::{BrokerOrder, BrokerPosition};
use mandate_journal::{ArtifactStore, StreamId};
use mandate_mcp::{CallClass, McpError, Tools};
use mandate_num::{Price, Qty, SignedQty, Usd};
use mandate_robinhood::{AccountSnapshot, RobinhoodConnector, Tickers};
use mandate_shell::adapters::Sources;
use mandate_shell::control::{
    Build as BuiltWith, Confirmation, ConfirmedVersion, ControlRecord, DeploymentRefusal,
    DoorChecks, LiveAdmission, McpHost, Origin, RunFacts,
};
use mandate_shell::paper::{Artifacts, BrokerFacts};
use mandate_shell::robinhood::{ACCOUNT_UNREAD, RobinhoodBridge};
use mandate_shell::{Cause, ProductionCycle, Setup, ShellError, Stage};
use mandate_spec::Violation;
use mandate_spec::context::{AgentId, Membership};
use mandate_time::{Date, UtcNanos};
use serde_json::json;

use crate::{
    ACCOUNT_NUMBER_VAR, Args, Broker, BrokerAsk, Build, Built, Connected, Connection, Dial,
    LapseRefusal, LapseSnapshot, Linked, Market, PaperError, Ports, REHEARSAL_ENDPOINT_VAR,
    REHEARSAL_HOST, RobinhoodEndpoint, Session, SystemClock, live_admission, live_start,
    live_started, robinhood_reads,
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
        event_time: None,
        event_type: event_type.to_owned(),
        payload: canonical(payload),
        origin: Origin {
            stream_id: "ctl:ws_live".to_owned(),
            environment: Environment::Live,
        },
    }
}

/// A live deployment of the tracer mandate on a live Robinhood connection, with the stop-limit
/// offset V-008 asks for on its profile (DEC-1152 item 1), its document stored under `name`, and
/// `extra` records after it.
fn deployment(name: &str, extra: &[(&str, &str)]) -> (Vec<ControlRecord>, FsArtifactStore) {
    let live = MANDATE
        .replace(r#""environment": "paper""#, r#""environment": "live""#)
        .replace(
            r#""crypto_stop_limit_offset": null"#,
            r#""crypto_stop_limit_offset": "0.01""#,
        );
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
        market: None,
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
        _earlier: Option<&LapseSnapshot>,
    ) -> Result<BrokerFacts, Cause> {
        Err(Cause::Absent { what: "unread" })
    }

    fn cycle(&self, _sources: Sources<()>, _setup: Setup) -> Result<ProductionCycle, Cause> {
        Err(Cause::Absent { what: "unread" })
    }

    fn fresh_quote<P: mandate_alpaca::Pause>(
        &self,
        _artifacts: &Artifacts,
        _pause: P,
    ) -> Result<mandate_alpaca::LatestQuote, Cause> {
        Err(Cause::Absent { what: "unread" })
    }

    fn broker_ask(&self, _facts: &BrokerFacts) -> BrokerAsk {
        BrokerAsk::Absent
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
    .map(|started| started.confirmed)
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
/// before the one connect — a condition of the admission missing, a V-rule (the connection
/// re-established as paper under the live mandate, V-001), a stopped agent.
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
    let paper = format!(
        r#"{{"account_ref":"{ACCOUNT}","broker":"alpaca","connection_id":"{CONNECTION}","environment":"paper","scopes":["trading"]}}"#
    );
    let mut ports = LivePorts::answering(Ok(Some(flat())));
    let refused = start(
        "paper-connection",
        &[("ConnectionEstablished", &paper)],
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
                cause: Cause::Absent {
                    what: ACCOUNT_UNREAD
                }
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

fn linked(answers: BTreeMap<&'static str, serde_json::Value>) -> (Linked<Scripted, Market>, Calls) {
    let calls = Calls::default();
    let tools = Scripted {
        answers,
        calls: Rc::clone(&calls),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("the runtime builds");
    let bridge = RobinhoodBridge::new(
        runtime,
        RobinhoodConnector::new(tools, NUMBER.to_owned(), Tickers::default()),
    );
    (Linked::new(bridge, None::<Market>), calls)
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

fn args(acknowledged: Environment) -> Args {
    Args {
        workspace: "ws_live".to_owned(),
        agent: AGENT.to_owned(),
        account_ref: ACCOUNT.to_owned(),
        journal: None,
        store: std::env::temp_dir().join("live-start-unused-store"),
        bars: std::env::temp_dir().join("live-start-unused-bars"),
        place_one_order: false,
        reconcile_only: false,
        acknowledged,
    }
}

fn rehearsal_vars() -> Vec<(String, String)> {
    vec![
        (
            REHEARSAL_ENDPOINT_VAR.to_owned(),
            format!("http://{REHEARSAL_HOST}:8080/mcp"),
        ),
        (ACCOUNT_NUMBER_VAR.to_owned(), NUMBER.to_owned()),
    ]
}

/// DEC-961 item 1: the dial names the market fixture's directory only in the `rh-sim` build;
/// every other build, the `live` one included, reads no fixture whatever the environment holds.
#[test]
fn only_the_rehearsal_build_dials_a_market_fixture() {
    let mut vars = rehearsal_vars();
    vars.push(("MANDATE_RH_SIM_MARKET".to_owned(), "/fixture".to_owned()));
    let dialed = crate::dial(Build::Rehearsal, &vars).expect("the rehearsal dials");
    let expected = cfg!(feature = "rh-sim").then(|| std::path::PathBuf::from("/fixture"));
    assert_eq!(dialed.market, expected);
    let unset = crate::dial(Build::Rehearsal, &rehearsal_vars()).expect("the rehearsal dials");
    assert_eq!(unset.market, None);
}

/// The rehearsal build: the simulator's endpoint and pin, compiled with `live`.
const REHEARSAL: Built = Built {
    build: Build::Rehearsal,
    with: BuiltWith::Live,
};

fn refusing_facts() -> RunFacts {
    RunFacts {
        live: LiveAdmission::refusing(),
        ..before()
    }
}

/// R2 and R3 through the run's own start (DEC-947 items 2, 3 and 5): a live connection the door
/// chose dials the build's endpoint and the founder's account number from the process
/// environment, admits itself from the run's facts with the build's pin, and starts on the
/// connected session's snapshot, the dial handed to the one connect.
#[test]
fn a_live_connection_starts_through_the_live_arm_with_the_runs_own_dial() {
    let (records, store) = deployment("started", &[]);
    let door = (live_robinhood(), stream());
    let mut ports = LivePorts::answering(Ok(Some(flat())));
    let mut facts = refusing_facts();
    let started = live_started(
        Some(&door),
        &records,
        &store,
        &mut facts,
        &args(Environment::Live),
        &rehearsal_vars(),
        REHEARSAL,
        &mut ports,
    )
    .expect("the live run starts");
    let started = started.expect("a live connection starts through the live arm");
    assert_eq!(started.confirmed.mandate().environment, Environment::Live);
    assert_eq!(ports.dials.borrow().as_slice(), [Some(the_dial())]);
    assert_eq!(facts.live, before().live);
}

/// The live arm refuses before any connect when the run's own facts do not admit it: a paper
/// acknowledgement, a build compiled without `live`, a dial the build cannot make.
#[test]
fn the_live_arm_refuses_before_the_connect_on_the_runs_own_facts() {
    let (records, store) = deployment("started-refusals", &[]);
    let door = (live_robinhood(), stream());
    let unbuilt = Built {
        with: BuiltWith::Default,
        ..REHEARSAL
    };
    let cases = [
        (Environment::Paper, REHEARSAL, rehearsal_vars(), "live"),
        (Environment::Live, unbuilt, rehearsal_vars(), "live"),
        (
            Environment::Live,
            REHEARSAL,
            Vec::new(),
            "rehearsal_endpoint_unset",
        ),
    ];
    for (acknowledged, built, vars, code) in cases {
        let mut ports = LivePorts::answering(Ok(Some(flat())));
        let refused = live_started(
            Some(&door),
            &records,
            &store,
            &mut refusing_facts(),
            &args(acknowledged),
            &vars,
            built,
            &mut ports,
        )
        .err();
        let found = match &refused {
            Some(PaperError::Deployment(refusal)) => refusal.code(),
            Some(PaperError::Endpoint(refusal)) => refusal.code(),
            _ => "other",
        };
        assert_eq!(found, code, "{acknowledged:?} {built:?}: {refused:?}");
        assert_eq!(ports.connects.get(), 0, "{code}");
    }
}

/// The binary's session routes the live door's snapshot to its Robinhood arm (DEC-947 item 1).
#[test]
fn the_binarys_robinhood_session_reads_its_snapshot() {
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
    let (linked, _) = linked(answers);
    let session: Connected<Scripted> = Connected::Robinhood(linked);
    assert_eq!(session.account_snapshot().ok(), Some(Some(flat())));
}

/// A paper connection, or none, is not the live arm's: it reads nothing and connects nothing.
#[test]
fn a_paper_connection_or_none_is_left_to_the_run() {
    let (records, store) = deployment("started-paper", &[]);
    let paper = (
        Connection {
            broker: Broker::Alpaca,
            environment: Environment::Paper,
            account_ref: ACCOUNT.to_owned(),
        },
        stream(),
    );
    for door in [Some(&paper), None] {
        let mut ports = LivePorts::answering(Ok(Some(flat())));
        let started = live_started(
            door,
            &records,
            &store,
            &mut refusing_facts(),
            &args(Environment::Paper),
            &rehearsal_vars(),
            REHEARSAL,
            &mut ports,
        );
        assert!(matches!(started, Ok(None)), "{:?}", started.err());
        assert_eq!(ports.connects.get(), 0);
    }
}

/// R11 (DEC-959 item 1): the live start hands the run the snapshot its lapse counted, read
/// through the session it returns, with the run clock read before the read was sent.
#[test]
fn the_live_start_hands_the_run_the_snapshot_its_lapse_counted() {
    let (records, store) = deployment("lapse-kept", &[]);
    let mut ports = LivePorts::answering(Ok(Some(flat())));
    let before_start = SystemClock.now();
    let started = live_start(
        &records,
        &store,
        &before(),
        &live_robinhood(),
        &stream(),
        None,
        &the_dial(),
        &mut ports,
    )
    .expect("the live run starts");
    let after_start = SystemClock.now();
    let lapse = started.lapse.expect("the lapse's snapshot is kept");
    assert_eq!(lapse.account, flat());
    assert!(
        before_start <= lapse.sent && lapse.sent <= after_start,
        "{lapse:?}"
    );
}

fn at(secs: i64, nanos: u32) -> UtcNanos {
    UtcNanos::from_parts(secs, nanos).expect("a valid instant")
}

/// A lapse snapshot stands for the account at most its bound after it was sent, the bound
/// included; one nanosecond older, across a clock that ran backwards, or listing any position
/// record or working order (whose instrument it may name by a ticker registered only later,
/// DEC-956), it does not.
#[test]
fn a_lapse_snapshot_is_fresh_within_its_bound_only() {
    let kept = LapseSnapshot {
        account: flat(),
        sent: at(1_000, 500),
    };
    let bound = std::time::Duration::from_secs(10);
    assert_eq!(kept.fresh(at(1_000, 500), bound), Some(&flat()));
    assert_eq!(kept.fresh(at(1_010, 500), bound), Some(&flat()));
    assert_eq!(kept.fresh(at(1_010, 501), bound), None);
    assert_eq!(
        kept.fresh(at(1_000, 499), bound),
        None,
        "the clock ran back"
    );
    assert_eq!(
        kept.fresh(at(1_000, 500), std::time::Duration::ZERO),
        Some(&flat())
    );
    let position = BrokerPosition {
        instrument: InstrumentId::new("SPY").expect("a symbol"),
        qty: SignedQty::parse("0").expect("a quantity"),
        avg_entry_price: Price::parse("1").expect("a price"),
    };
    let holding = LapseSnapshot {
        account: AccountSnapshot {
            positions: vec![position],
            ..flat()
        },
        sent: at(1_000, 500),
    };
    assert_eq!(
        holding.fresh(at(1_000, 500), bound),
        None,
        "a position record"
    );
    let order = BrokerOrder {
        broker_order_id: "o1".to_owned(),
        client_order_id: None,
        instrument: InstrumentId::new("SPY").expect("a symbol"),
        side: Side::Buy,
        qty: Qty::parse("1").expect("a quantity"),
        filled_qty: Qty::parse("0").expect("a quantity"),
        limit_price: None,
        stop_price: None,
        status: "accepted".to_owned(),
        reject_code: None,
        replaced_by_broker_order_id: None,
        legs: Vec::new(),
        created_on: None,
    };
    let ordering = LapseSnapshot {
        account: AccountSnapshot {
            open_orders: vec![order],
            ..flat()
        },
        sent: at(1_000, 500),
    };
    assert_eq!(
        ordering.fresh(at(1_000, 500), bound),
        None,
        "a working order"
    );
}

/// The scripted agentic account, its tradability and its quote, on a bridge of its own.
fn preflight_bridge() -> (Rc<RobinhoodBridge<Scripted>>, Calls) {
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
        (
            "get_equity_tradability",
            json!({"account_number": NUMBER, "symbol": "SPY", "tradable": true}),
        ),
        (
            "get_equity_quotes",
            json!({"quotes": [{"symbol": "SPY", "bid_price": "85", "ask_price": "85"}]}),
        ),
    ]);
    let calls = Calls::default();
    let tools = Scripted {
        answers,
        calls: Rc::clone(&calls),
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("the runtime builds");
    let asset = InstrumentId::new("b28f4066-5c6d-479b-a2af-85dc1a8f16fb").expect("an asset id");
    let tickers = Tickers::registered([(asset, "SPY".to_owned())]).expect("one ticker");
    let connector = RobinhoodConnector::new(tools, NUMBER.to_owned(), tickers);
    (RobinhoodBridge::new(runtime, connector), calls)
}

/// R11 (DEC-959 item 1): the preflight stands on a fresh lapse snapshot and reads only the
/// tradability and the quote; a stale one, one across a clock that ran back, or none is never
/// reused, and the account is read again in full.
#[test]
fn the_preflight_reuses_a_fresh_lapse_snapshot_and_rereads_a_stale_one() {
    let symbol = InstrumentId::new("SPY").expect("a symbol");
    let handed = AccountSnapshot {
        cash: usd("99"),
        ..flat()
    };
    let kept = LapseSnapshot {
        account: handed.clone(),
        sent: at(1_000, 0),
    };
    let bound = std::time::Duration::from_secs(10);
    let (bridge, calls) = preflight_bridge();
    let facts = robinhood_reads(&bridge, &symbol, Some(&kept), at(1_010, 0), bound)
        .expect("the preflight reads");
    assert_eq!(facts.account, handed);
    assert_eq!(
        calls.borrow().as_slice(),
        [
            "get_accounts",
            "get_equity_tradability",
            "get_equity_quotes"
        ]
    );
    let full = [
        "get_accounts",
        "get_portfolio",
        "get_accounts",
        "get_equity_positions",
        "get_accounts",
        "get_equity_orders",
        "get_accounts",
        "get_equity_tradability",
        "get_equity_quotes",
    ];
    for (earlier, now) in [
        (Some(&kept), at(1_010, 1)),
        (Some(&kept), at(999, 0)),
        (None, at(1_000, 0)),
    ] {
        let (bridge, calls) = preflight_bridge();
        let facts =
            robinhood_reads(&bridge, &symbol, earlier, now, bound).expect("the preflight reads");
        assert_eq!(facts.account, flat(), "{now:?}");
        assert_eq!(calls.borrow().as_slice(), full, "{now:?}");
    }
}
