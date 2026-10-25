//! The harness the deployment manager's tests share: the control stream in a [`MemoryJournal`]
//! with the journal's own draft checks, control services' drafts, the accounts a test's agents run
//! on, an orchestrator that checks the journal before it acts, and a clock.

#![allow(dead_code, reason = "each test file uses part of the harness")]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::rc::Rc;

use mandate_canon::{Digest, parse};
use mandate_deploy_manager::fake::Fake;
use mandate_deploy_manager::{
    Alerts, ControlJournal, Deployer, Identity, Instance, Launch, OperatorAlert, Orchestrator,
    Slot, Unavailable,
};
use mandate_journal::{AppendOutcome, MemoryJournal, StoredEvent, StreamId};
use mandate_time::UtcNanos;

pub const WORKSPACE: &str = "ws_01J8Z2";
pub const STREAM: &str = "ctl:ws_01J8Z2";
pub const START: i64 = 1_792_249_200;

pub fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(START + secs, 0).unwrap()
}

pub fn block_on<F: Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(future)
}

pub fn identity() -> Identity {
    Identity {
        workspace: WORKSPACE.to_owned(),
        actor_id: "deploy_manager".to_owned(),
        version: "0.0.0".to_owned(),
        build: Digest::of(b"mandate-deploy-manager test build"),
    }
}

pub type Shared = Rc<RefCell<MemoryJournal>>;

/// The control stream, shared by the control services' appends and the manager's.
#[derive(Clone)]
pub struct Journal {
    pub inner: Shared,
    pub unavailable: Rc<RefCell<bool>>,
    /// How many of the manager's next appends fail as unavailable, its reads still answered.
    pub failing_appends: Rc<RefCell<u32>>,
    /// Control services' drafts committed right after the manager's next append, as a writer
    /// racing it would.
    pub after_append: Rc<RefCell<Vec<Vec<u8>>>>,
    /// The account each test agent runs on, so its deployment names that account's connection.
    pub agents: Rc<RefCell<BTreeMap<String, String>>>,
}

impl Journal {
    pub fn new() -> Self {
        let journal = Self {
            inner: Rc::new(RefCell::new(MemoryJournal::new())),
            unavailable: Rc::new(RefCell::new(false)),
            failing_appends: Rc::new(RefCell::new(0)),
            after_append: Rc::new(RefCell::new(Vec::new())),
            agents: Rc::new(RefCell::new(BTreeMap::new())),
        };
        journal.services(&opened(), at(0));
        journal
    }

    /// A version-2 deployment of `agent` on the account [`manager`] was given for it.
    pub fn deployed(&self, agent: &str) -> Vec<u8> {
        let account = self.agents.borrow().get(agent).cloned().unwrap();
        deployed(agent, &account)
    }

    fn stream() -> StreamId {
        StreamId::parse(STREAM).unwrap()
    }

    /// Appends one control services' draft, which must commit.
    pub fn services(&self, draft: &[u8], now: UtcNanos) {
        let outcome = append(&self.inner, draft, now);
        assert!(
            matches!(outcome, AppendOutcome::Committed(_)),
            "{outcome:?}"
        );
    }

    pub fn rows(&self) -> Vec<StoredEvent> {
        self.inner.borrow().rows(&Self::stream()).to_vec()
    }

    /// The manager's records, as (event type, payload) in order.
    pub fn records(&self) -> Vec<(String, mandate_canon::Value)> {
        self.rows()
            .into_iter()
            .filter(|r| r.event_type.starts_with("Process"))
            .map(|r| {
                let body = parse(&r.body).unwrap();
                (r.event_type.clone(), body.get("payload").cloned().unwrap())
            })
            .collect()
    }

    /// Whether a manager record of `event_type` for `slot` and `generation` is committed.
    pub fn holds(&self, event_type: &str, slot: &Slot, generation: u64) -> bool {
        self.records().iter().any(|(t, p)| {
            t == event_type
                && p.get("process").and_then(|v| v.as_str()) == Some(slot.process.code())
                && p.get("subject").and_then(|v| v.as_str()) == Some(slot.subject.as_str())
                && p.get("generation").and_then(|v| v.as_int()) == Some(generation)
        })
    }
}

fn append(journal: &Shared, draft: &[u8], now: UtcNanos) -> AppendOutcome {
    let mut journal = journal.borrow_mut();
    let stream = StreamId::parse(STREAM).unwrap();
    let epoch = journal.take_ownership(&stream);
    let head = journal.head(&stream).seq;
    journal.append(&stream, head, epoch, now, &[draft])
}

impl ControlJournal for Journal {
    async fn rows_after(&mut self, seq: u64) -> Result<Vec<StoredEvent>, Unavailable> {
        if *self.unavailable.borrow() {
            return Err(Unavailable);
        }
        Ok(self.rows().into_iter().filter(|r| r.seq > seq).collect())
    }

    async fn append(
        &mut self,
        draft: &[u8],
        recorded_at: UtcNanos,
    ) -> Result<AppendOutcome, Unavailable> {
        if *self.unavailable.borrow() {
            return Err(Unavailable);
        }
        let mut failing = self.failing_appends.borrow_mut();
        if *failing > 0 {
            *failing -= 1;
            return Err(Unavailable);
        }
        drop(failing);
        let outcome = append(&self.inner, draft, recorded_at);
        let racing: Vec<Vec<u8>> = self.after_append.borrow_mut().drain(..).collect();
        for other in racing {
            self.services(&other, recorded_at);
        }
        Ok(outcome)
    }
}

/// An orchestrator over the [`Fake`] that refuses, as a violation, any start or stop whose record
/// is not already committed on the control stream, and any start while the slot has a live
/// instance: the oracle's own view, read from the journal and the fake, not from the manager.
pub struct Checked {
    pub fake: Fake,
    pub journal: Journal,
    pub violations: Vec<String>,
    /// A bug seeded into the orchestrator, so each of the oracle's checks is shown to catch one.
    pub seed: Seed,
}

/// The seeded bugs: each breaks one property the oracle checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seed {
    None,
    /// A start is acknowledged and never carried out, as a manager that delayed it would.
    DropStarts,
    /// A start leaves a second live instance beside the one asked for.
    SecondInstance,
    /// A start also starts a runtime nobody deployed.
    Unwanted,
    /// A start is carried out three times over, as a manager that ignored its bound would.
    Repeated,
    /// A start also starts the next generation before any record of it.
    Unjournaled,
    /// A start also brings back every slot whose crash loop is journaled.
    Revive,
}

impl Checked {
    pub fn new(journal: Journal) -> Self {
        Self {
            fake: Fake::new(),
            journal,
            violations: Vec::new(),
            seed: Seed::None,
        }
    }

    fn check_start(&mut self, slot: &Slot, generation: u64) {
        if !self.journal.holds("ProcessStartIssued", slot, generation) {
            self.violations
                .push(format!("start of {slot:?} g{generation} before its record"));
        }
        let live_other = self
            .fake
            .instances()
            .iter()
            .any(|i| i.slot == *slot && i.generation != generation && i.phase.live());
        if live_other {
            self.violations.push(format!(
                "start of {slot:?} g{generation} beside a live instance"
            ));
        }
    }
}

impl Orchestrator for Checked {
    async fn observe(&mut self) -> Result<Vec<Instance>, Unavailable> {
        self.fake.observe().await
    }

    async fn start(
        &mut self,
        slot: &Slot,
        generation: u64,
        launch: &Launch,
    ) -> Result<(), Unavailable> {
        if self.fake.unavailable || self.fake.refuse_effects {
            return self.fake.start(slot, generation, launch).await;
        }
        self.check_start(slot, generation);
        match self.seed {
            Seed::None => {}
            Seed::DropStarts => return Ok(()),
            Seed::SecondInstance => self.fake.start(slot, generation + 1000, launch).await?,
            Seed::Unwanted => {
                self.fake
                    .start(&Slot::runtime("agent_nobody"), 1, &Launch::default())
                    .await?
            }
            Seed::Repeated => {
                self.fake.start(slot, generation, launch).await?;
                self.fake.start(slot, generation, launch).await?;
            }
            Seed::Unjournaled => {
                self.check_start(slot, generation + 1);
            }
            Seed::Revive => {
                let looped: Vec<Slot> = self
                    .journal
                    .records()
                    .iter()
                    .filter(|(t, _)| t == "ProcessCrashLooped")
                    .filter_map(|(_, p)| {
                        Some(Slot {
                            process: mandate_deploy_manager::Process::parse(
                                p.get("process")?.as_str()?,
                            )?,
                            subject: p.get("subject")?.as_str()?.to_owned(),
                        })
                    })
                    .collect();
                for other in looped {
                    if self.fake.live(&other) == 0 {
                        self.fake.start(&other, 1000, &Launch::default()).await?;
                    }
                }
            }
        }
        self.fake.start(slot, generation, launch).await
    }

    async fn stop(
        &mut self,
        slot: &Slot,
        generation: u64,
        connection: Option<&str>,
    ) -> Result<(), Unavailable> {
        if !self.fake.unavailable
            && !self.fake.refuse_effects
            && !self.journal.holds("ProcessStopIssued", slot, generation)
        {
            self.violations
                .push(format!("stop of {slot:?} g{generation} before its record"));
        }
        self.fake.stop(slot, generation, connection).await
    }
}

#[derive(Debug, Default)]
pub struct Captured(pub Vec<OperatorAlert>);

impl Alerts for Captured {
    fn raise(&mut self, alert: OperatorAlert) {
        self.0.push(alert);
    }
}

/// The connections a test's agents run on, each to its account, established on the control stream
/// before the manager starts.
#[derive(Debug, Clone, Default)]
pub struct Accounts {
    pub agents: BTreeMap<String, String>,
    pub connections: BTreeMap<String, String>,
}

/// The mandate version an agent is deployed with in these tests.
pub fn version_of(agent: &str) -> String {
    format!("sha256:{}", Digest::of(agent.as_bytes()).to_hex())
}

/// The connection of an account in these tests.
pub fn connection_of(account: &str) -> String {
    format!("conn_{account}")
}

/// The connections of `agents`, each `(agent, account)`.
pub fn accounts(agents: &[(&str, &str)]) -> Accounts {
    let mut out = Accounts::default();
    for (agent, account) in agents {
        out.agents
            .insert((*agent).to_owned(), (*account).to_owned());
        out.connections
            .insert(connection_of(account), (*account).to_owned());
    }
    out
}

pub type Manager = Deployer<Journal, Checked, Captured>;

/// A new manager over the ports of `old`, as after the manager's own restart: it keeps nothing in
/// memory, so it rebuilds everything from the journal and the orchestrator.
pub fn restart(old: Manager) -> Manager {
    let (journal, orchestrator, alerts) = old.into_ports();
    Deployer::new(identity(), journal, orchestrator, alerts)
}

pub fn manager(journal: &Journal, accounts: Accounts) -> Manager {
    journal.agents.borrow_mut().extend(accounts.agents);
    for (connection, account) in &accounts.connections {
        let outcome = append(&journal.inner, &established(connection, account), at(0));
        assert!(
            matches!(
                outcome,
                AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_)
            ),
            "{outcome:?}"
        );
    }
    Deployer::new(
        identity(),
        journal.clone(),
        Checked::new(journal.clone()),
        Captured::default(),
    )
}

fn event_id(seed: &str) -> String {
    let digest = Digest::of(seed.as_bytes());
    let mut high = [0_u8; 16];
    high.copy_from_slice(&digest.as_bytes()[..16]);
    mandate_canon::encode_ulid(u128::from_be_bytes(high))
}

const SERVICES: &str = r#"{"kind":"system","id":"control_services","version":"0.1.0","build":"sha256:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd"}"#;
const OWNER: &str = r#"{"kind":"user","id":"user_owner_01","version":"1","build":null}"#;

fn envelope(
    event_id: &str,
    event_type: &str,
    actor: &str,
    refs: &str,
    payload: &str,
    artifacts: &str,
) -> Vec<u8> {
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{STREAM}",
        "event_type":"{event_type}","schema_version":1,"event_time":"2026-10-17T15:00:00.000000000Z",
        "clock_source":"local","causation_id":null,"correlation_id":null,"actor":{actor},
        "config_refs":{refs},"payload":{payload},"artifact_refs":{artifacts},"pii_refs":[]}}"#
    );
    mandate_canon::to_canonical(&parse(body.as_bytes()).unwrap())
}

pub fn opened() -> Vec<u8> {
    envelope(
        &event_id("opened"),
        "StreamOpened",
        SERVICES,
        "{}",
        r#"{"stream_type":"control","workspace_id":"ws_01J8Z2"}"#,
        "[]",
    )
}

/// A connection established on the control stream, as control services write it (journal spec
/// §9.8), naming its account.
pub fn established(connection: &str, account: &str) -> Vec<u8> {
    established_again(connection, account, 0)
}

/// The `nth` establishment of `connection`, each its own event; the first is [`established`].
pub fn established_again(connection: &str, account: &str, nth: u32) -> Vec<u8> {
    let seed = match nth {
        0 => format!("established {connection}"),
        _ => format!("established {connection} {nth}"),
    };
    let draft = envelope(
        &event_id(&seed),
        "ConnectionEstablished",
        SERVICES,
        "{}",
        &format!(
            r#"{{"connection_id":"{connection}","broker":"alpaca","environment":"paper","scopes":["data","trading"],"account_ref":"{account}","user":"user_owner_01","step_up":{{"assertion_id":"assert_owner_09","authenticated_at":"2026-10-17T14:59:50.000000000Z","method":"cli_confirm"}},"margin_attestation":null}}"#
        ),
        "[]",
    );
    let draft = at_version_2(&draft);
    let mandate_canon::Value::Object(mut body) = parse(&draft).unwrap() else {
        unreachable!("an envelope is an object")
    };
    body.insert(
        mandate_canon::Key::new("causation_id").unwrap(),
        mandate_canon::Value::Str(event_id(&format!("checked {connection}"))),
    );
    mandate_canon::to_canonical(&mandate_canon::Value::Object(body))
}

/// `draft` at schema version 2.
fn at_version_2(draft: &[u8]) -> Vec<u8> {
    let mandate_canon::Value::Object(mut body) = parse(draft).unwrap() else {
        unreachable!("an envelope is an object")
    };
    body.insert(
        mandate_canon::Key::new("schema_version").unwrap(),
        mandate_canon::Value::Int(mandate_canon::Int::new(2).unwrap()),
    );
    mandate_canon::to_canonical(&mandate_canon::Value::Object(body))
}

/// A version-2 deployment of `agent` on the connection of `account` (journal spec §9.24).
pub fn deployed(agent: &str, account: &str) -> Vec<u8> {
    deployment(&format!("deployed {agent}"), agent, Some(account), "")
}

/// A version-1 deployment of `agent`, which names no connection.
pub fn deployed_v1(agent: &str) -> Vec<u8> {
    deployment(&format!("deployed v1 {agent}"), agent, None, "")
}

/// A second version-2 `AgentDeployed` for an agent, with its own event id.
pub fn redeployed(agent: &str, account: &str, nth: u32) -> Vec<u8> {
    deployment(
        &format!("redeployed {agent} {nth}"),
        agent,
        Some(account),
        &format!(" {nth}"),
    )
}

fn deployment(seed: &str, agent: &str, account: Option<&str>, nth: &str) -> Vec<u8> {
    let version = version_of(agent);
    let record = format!(
        "sha256:{}",
        Digest::of(format!("record {agent}{nth}").as_bytes()).to_hex()
    );
    let mut refs = [record.clone(), version.clone()];
    refs.sort();
    let connection = account.map_or(String::new(), |account| {
        format!(r#","connection_id":"{}""#, connection_of(account))
    });
    let draft = envelope(
        &event_id(seed),
        "AgentDeployed",
        OWNER,
        &format!(r#"{{"mandate_version":"{version}"}}"#),
        &format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{record}"{connection}}}"#
        ),
        &format!(r#"["{}","{}"]"#, refs[0], refs[1]),
    );
    match account {
        Some(_) => at_version_2(&draft),
        None => draft,
    }
}

/// A connection's revocation on the control stream, as control services write it (journal spec §9.10).
pub fn revoked(connection: &str) -> Vec<u8> {
    revoked_again(connection, 0)
}

/// The `nth` revocation of `connection`, each its own event; the first is [`revoked`].
pub fn revoked_again(connection: &str, nth: u32) -> Vec<u8> {
    let seed = match nth {
        0 => format!("revoked {connection}"),
        _ => format!("revoked {connection} {nth}"),
    };
    envelope(
        &event_id(&seed),
        "ConnectionRevoked",
        SERVICES,
        "{}",
        &format!(r#"{{"connection_id":"{connection}"}}"#),
        "[]",
    )
}

pub fn stopped(agent: &str, account: &str) -> Vec<u8> {
    envelope(
        &event_id(&format!("stopped {agent}")),
        "AgentStopped",
        SERVICES,
        &format!(r#"{{"mandate_version":"{}"}}"#, version_of(agent)),
        &format!(
            r#"{{"agent_id":"{agent}","connection_id":"{}","reason":"owner_stop","retired_on":"2026-10-17","loss_added":"0"}}"#,
            connection_of(account)
        ),
        "[]",
    )
}

/// The owner's kill switch of `scope` (`agent`, `connection`, `workspace`) on `subject`; `nth`
/// keeps each one its own event.
pub fn kill(scope: &str, subject: &str, nth: u32) -> (String, Vec<u8>) {
    owner_command("kill_switch", scope, subject, nth)
}

/// The owner's `command` of `scope` on `subject`.
pub fn owner_command(command: &str, scope: &str, subject: &str, nth: u32) -> (String, Vec<u8>) {
    let id = event_id(&format!("{command} {scope} {subject} {nth}"));
    let agent = if scope == "agent" {
        format!(r#""{subject}""#)
    } else {
        "null".to_owned()
    };
    let draft = envelope(
        &id,
        "OwnerCommandIssued",
        OWNER,
        "{}",
        &format!(
            r#"{{"agent":{agent},"command":"{command}","scope":"{scope}","subject":"{subject}","release":null,"warning_shown":null,"bid":null,"bid_size":null,"floor":null,"user":"user_owner_01"}}"#
        ),
        "[]",
    );
    (id, draft)
}

/// A connect's request on the control stream (journal spec §9.8): at version 2 with `credential`
/// `oauth` (and its breadth) or `api_key`, or at version 1, which names no credential.
pub fn requested(connection: &str, account: &str, credential: Option<&str>) -> Vec<u8> {
    let step_up = r#"{"assertion_id":"assert_owner_09","authenticated_at":"2026-10-17T14:59:50.000000000Z","method":"cli_confirm"}"#;
    let tail = match credential {
        Some("oauth") => r#","credential":"oauth","breadth":{"other_environment":"may_reach","disclosure":{"document":"alpaca_oauth_token_breadth","version":"1"}}"#.to_owned(),
        Some(other) => format!(r#","credential":"{other}","breadth":null"#),
        None => String::new(),
    };
    let draft = envelope(
        &event_id(&format!("requested {connection}")),
        "ConnectionRequested",
        SERVICES,
        "{}",
        &format!(
            r#"{{"connection_id":"{connection}","account_ref":"{account}","broker":"alpaca","environment":"paper","user":"user_owner_01","step_up":{step_up}{tail}}}"#
        ),
        "[]",
    );
    match credential {
        Some(_) => at_version_2(&draft),
        None => draft,
    }
}

/// The role provisioner's actor (journal spec §9.33 rule 261: a system actor).
const PROVISIONER: &str = r#"{"kind":"system","id":"role_provisioner","version":"0.1.0","build":"sha256:eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee"}"#;

/// The event id of [`requested`]'s request of `connection`.
pub fn request_id(connection: &str) -> String {
    event_id(&format!("requested {connection}"))
}

/// A role provisioner's answer to the request whose event id is `request` (journal spec §9.33):
/// `ConnectionRolesMade` of `connection` when `refused` is `None`, else `ConnectionRolesRefused`
/// with that reason, each caused by the request.
fn roles(connection: &str, request: &str, refused: Option<&str>) -> Vec<u8> {
    let (event_type, payload) = match refused {
        None => (
            "ConnectionRolesMade",
            format!(r#"{{"connection_id":"{connection}"}}"#),
        ),
        Some(reason) => (
            "ConnectionRolesRefused",
            format!(r#"{{"connection_id":"{connection}","reason":"{reason}"}}"#),
        ),
    };
    let draft = envelope(
        &event_id(&format!("{event_type} {connection} {request}")),
        event_type,
        PROVISIONER,
        "{}",
        &payload,
        "[]",
    );
    let mandate_canon::Value::Object(mut body) = parse(&draft).unwrap() else {
        unreachable!("an envelope is an object")
    };
    body.insert(
        mandate_canon::Key::new("causation_id").unwrap(),
        mandate_canon::Value::Str(request.to_owned()),
    );
    mandate_canon::to_canonical(&mandate_canon::Value::Object(body))
}

/// The provisioner's `ConnectionRolesMade` for [`requested`]'s request of `connection`: what a
/// connect's token exchange waits for (journal spec §9.21, DEC-2350).
pub fn roles_made(connection: &str) -> Vec<u8> {
    roles(connection, &request_id(connection), None)
}

/// A `ConnectionRolesMade` of `connection` that answers the request `request`, which need not be
/// that connection's latest.
pub fn roles_made_for(connection: &str, request: &str) -> Vec<u8> {
    roles(connection, request, None)
}

/// The provisioner's `ConnectionRolesRefused` for [`requested`]'s request of `connection`.
pub fn roles_refused(connection: &str, reason: &str) -> Vec<u8> {
    roles(connection, &request_id(connection), Some(reason))
}

/// A teardown's `ConnectionRefused` of `connection` with `occasion` (journal spec §9.8): with
/// occasion `connect` it closes the connection's request.
pub fn refused(connection: &str, occasion: &str) -> Vec<u8> {
    envelope(
        &event_id(&format!("refused {connection} {occasion}")),
        "ConnectionRefused",
        SERVICES,
        "{}",
        &format!(
            r#"{{"connection_id":"{connection}","broker":"alpaca","environment":"paper","occasion":"{occasion}","check":null,"reason":"timeout","existing_connection_id":null,"user":"user_owner_01","step_up":{{"assertion_id":"assert_owner_09","authenticated_at":"2026-10-17T14:59:50.000000000Z","method":"cli_confirm"}}}}"#
        ),
        "[]",
    )
}
