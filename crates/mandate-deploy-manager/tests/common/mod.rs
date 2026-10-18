//! The harness the deployment manager's tests share: the control stream in a [`MemoryJournal`]
//! with the journal's own draft checks, control services' drafts, a resolver, an orchestrator that
//! checks the journal before it acts, and a clock.

#![allow(dead_code, reason = "each test file uses part of the harness")]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::future::Future;
use std::rc::Rc;

use mandate_canon::{Digest, parse};
use mandate_deploy_manager::fake::Fake;
use mandate_deploy_manager::{
    Alerts, ControlJournal, Deployer, Identity, Instance, OperatorAlert, Orchestrator, Resolver,
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
    /// Control services' drafts committed right after the manager's next append, as a writer
    /// racing it would.
    pub after_append: Rc<RefCell<Vec<Vec<u8>>>>,
}

impl Journal {
    pub fn new() -> Self {
        let journal = Self {
            inner: Rc::new(RefCell::new(MemoryJournal::new())),
            unavailable: Rc::new(RefCell::new(false)),
            after_append: Rc::new(RefCell::new(Vec::new())),
        };
        journal.services(&opened(), at(0));
        journal
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

    async fn start(&mut self, slot: &Slot, generation: u64) -> Result<(), Unavailable> {
        if self.fake.unavailable || self.fake.refuse_effects {
            return self.fake.start(slot, generation).await;
        }
        self.check_start(slot, generation);
        match self.seed {
            Seed::None => {}
            Seed::DropStarts => return Ok(()),
            Seed::SecondInstance => self.fake.start(slot, generation + 1000).await?,
            Seed::Unwanted => self.fake.start(&Slot::runtime("agent_nobody"), 1).await?,
            Seed::Repeated => {
                self.fake.start(slot, generation).await?;
                self.fake.start(slot, generation).await?;
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
                        self.fake.start(&other, 1000).await?;
                    }
                }
            }
        }
        self.fake.start(slot, generation).await
    }

    async fn stop(&mut self, slot: &Slot, generation: u64) -> Result<(), Unavailable> {
        if !self.fake.unavailable
            && !self.fake.refuse_effects
            && !self.journal.holds("ProcessStopIssued", slot, generation)
        {
            self.violations
                .push(format!("stop of {slot:?} g{generation} before its record"));
        }
        self.fake.stop(slot, generation).await
    }
}

#[derive(Debug, Default)]
pub struct Captured(pub Vec<OperatorAlert>);

impl Alerts for Captured {
    fn raise(&mut self, alert: OperatorAlert) {
        self.0.push(alert);
    }
}

/// Mandate versions and connections to accounts, as the stored documents would say.
#[derive(Debug, Clone, Default)]
pub struct Accounts {
    pub versions: BTreeMap<String, String>,
    pub connections: BTreeMap<String, String>,
    /// While set, the store cannot answer.
    pub unavailable: Rc<RefCell<bool>>,
}

impl Resolver for Accounts {
    fn account_of_version(&self, version: &str) -> Result<Option<String>, Unavailable> {
        if *self.unavailable.borrow() {
            return Err(Unavailable);
        }
        Ok(self.versions.get(version).cloned())
    }
}

/// The mandate version an agent is deployed with in these tests.
pub fn version_of(agent: &str) -> String {
    format!("sha256:{}", Digest::of(agent.as_bytes()).to_hex())
}

/// The connection of an account in these tests.
pub fn connection_of(account: &str) -> String {
    format!("conn_{account}")
}

/// A resolver for `agents`, each `(agent, account)`.
pub fn accounts(agents: &[(&str, &str)]) -> Accounts {
    let mut out = Accounts::default();
    for (agent, account) in agents {
        out.versions
            .insert(version_of(agent), (*account).to_owned());
        out.connections
            .insert(connection_of(account), (*account).to_owned());
    }
    out
}

pub type Manager = Deployer<Journal, Checked, Captured, Accounts>;

/// A new manager over the ports of `old`, as after the manager's own restart: it keeps nothing in
/// memory, so it rebuilds everything from the journal and the orchestrator.
pub fn restart(old: Manager) -> Manager {
    let (journal, orchestrator, alerts, resolver) = old.into_ports();
    Deployer::new(identity(), journal, orchestrator, alerts, resolver)
}

pub fn manager(journal: &Journal, resolver: Accounts) -> Manager {
    for (connection, account) in &resolver.connections {
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
        resolver,
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
    let draft = envelope(
        &event_id(&format!("established {connection}")),
        "ConnectionEstablished",
        SERVICES,
        "{}",
        &format!(
            r#"{{"connection_id":"{connection}","broker":"alpaca","environment":"paper","scopes":["data","trading"],"account_ref":"{account}","user":"user_owner_01","step_up":{{"assertion_id":"assert_owner_09","authenticated_at":"2026-10-17T14:59:50.000000000Z","method":"cli_confirm"}},"margin_attestation":null}}"#
        ),
        "[]",
    );
    let mandate_canon::Value::Object(mut body) = parse(&draft).unwrap() else {
        unreachable!("an envelope is an object")
    };
    body.insert(
        mandate_canon::Key::new("schema_version").unwrap(),
        mandate_canon::Value::Int(mandate_canon::Int::new(2).unwrap()),
    );
    body.insert(
        mandate_canon::Key::new("causation_id").unwrap(),
        mandate_canon::Value::Str(event_id(&format!("checked {connection}"))),
    );
    mandate_canon::to_canonical(&mandate_canon::Value::Object(body))
}

pub fn deployed(agent: &str) -> Vec<u8> {
    let version = version_of(agent);
    let record = format!(
        "sha256:{}",
        Digest::of(format!("record {agent}").as_bytes()).to_hex()
    );
    let mut refs = [record.clone(), version.clone()];
    refs.sort();
    envelope(
        &event_id(&format!("deployed {agent}")),
        "AgentDeployed",
        OWNER,
        &format!(r#"{{"mandate_version":"{version}"}}"#),
        &format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{record}"}}"#
        ),
        &format!(r#"["{}","{}"]"#, refs[0], refs[1]),
    )
}

/// A second `AgentDeployed` for an agent, with its own event id.
pub fn redeployed(agent: &str, nth: u32) -> Vec<u8> {
    let version = version_of(agent);
    let record = format!(
        "sha256:{}",
        Digest::of(format!("record {agent} {nth}").as_bytes()).to_hex()
    );
    let mut refs = [record.clone(), version.clone()];
    refs.sort();
    envelope(
        &event_id(&format!("redeployed {agent} {nth}")),
        "AgentDeployed",
        OWNER,
        &format!(r#"{{"mandate_version":"{version}"}}"#),
        &format!(
            r#"{{"agent_id":"{agent}","mandate_version":"{version}","record_ref":"{record}"}}"#
        ),
        &format!(r#"["{}","{}"]"#, refs[0], refs[1]),
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
