//! The runtime process and its deployment's account stream (DEC-2209): folded after the agent stream
//! and before `Input::Started`, as the shell folds it, then tailed each pass after the control stream
//! and before the clock stream, each row past the core's own head handed once. The agent stream is
//! the real in-memory journal, which validates every draft the process writes; the control, account
//! and clock streams are rows a test lays down, as their writers would have committed them.

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use mandate_canon::{Digest, Key, Object, Value};
use mandate_daemon::{Config, Refs, RuntimeProcess, Wiring, fixture};
use mandate_journal::{AppendOutcome, Environment, StoredEvent, StreamId};
use mandate_process::{Exit, State};
use mandate_runtime::{
    AgentId, ApprovalSettings, Autonomy, Classified, ConnectionId, Deployment, DryRunVerdict,
    EventDraft, EventId, FlattenPlan, FlattenPlanner, FlattenRequest, GateDryRun, IdGen,
    MandateView, Mode, OrderPlan, Proposal, Seq, SignalInputs, WorkspaceId, WriterEpoch,
};
use mandate_store::{MemoryStore, Store};
use mandate_time::UtcNanos;
use proptest::prelude::*;

const WORKSPACE: &str = "ws1";
const AGENT: &str = "agent-a";
const ACCOUNT: &str = "acct-1";
const AGENT_STREAM: &str = "agent:ws1:agent-a";
const ACCOUNT_STREAM: &str = "acct:ws1:acct-1";
const OTHER_ACCOUNT_STREAM: &str = "acct:ws1:acct-2";
const CONTROL: &str = "ctl:ws1";
const CLOCK: &str = "clock:ws1";
const NOW: i64 = 1_792_155_600;

fn at(secs: i64) -> UtcNanos {
    UtcNanos::from_parts(secs, 0).unwrap()
}

struct Ids;
impl IdGen for Ids {
    fn event_id(&self, epoch: WriterEpoch, head: Seq, ordinal: u32) -> EventId {
        EventId(format!("0{:05}{:014}{ordinal:06}", epoch.0, head.0))
    }
}

struct Deny;
impl GateDryRun for Deny {
    fn check(&self, _: &Proposal) -> DryRunVerdict {
        DryRunVerdict::Deny {
            reason_code: "fixture".into(),
        }
    }
}

struct Nothing;
impl OrderPlan for Nothing {
    fn plan(&self, _: &MandateView, _: &SignalInputs) -> Option<Proposal> {
        None
    }
    fn classify(&self, _: &MandateView, _: &Proposal) -> Classified {
        Classified {
            autonomy: Autonomy::Deny,
            decided_by: None,
        }
    }
}

struct Empty;
impl FlattenPlanner for Empty {
    fn plan(&self, request: &FlattenRequest) -> FlattenPlan {
        FlattenPlan {
            cancel_client_order_ids: Vec::new(),
            sells: Vec::new(),
            purpose: request.initiator.sell_purpose(),
            confirmation: request.confirmation.clone(),
        }
    }
}

struct MandateRef;
impl Refs for MandateRef {
    fn refs(&self, _: &EventDraft) -> Option<(u64, Object)> {
        let version = format!("sha256:{}", Digest::of(b"the mandate version").to_hex());
        let mut refs = Object::new();
        refs.insert(Key::new("mandate_version").ok()?, Value::Str(version));
        Some((1, refs))
    }
}

fn wiring(account: Option<&str>) -> Wiring {
    Wiring {
        ids: Box::new(Ids),
        gate: Box::new(Deny),
        plan: Box::new(Nothing),
        flatten: Box::new(Empty),
        refs: Box::new(MandateRef),
        view: MandateView {
            version: "v1".into(),
            working_universe: Default::default(),
            restricted_instruments: Default::default(),
            approval: ApprovalSettings {
                approvers: ["user-owner".to_owned()].into(),
                author: "user-author".into(),
                timeout_s: 300,
                environment: Environment::Paper,
                live_cli_confirm: None,
            },
        },
        account: account.map(str::to_owned),
    }
}

fn config() -> Config {
    Config {
        deployment: Deployment {
            agent: AgentId(AGENT.into()),
            connection: ConnectionId("conn-1".into()),
            workspace: WorkspaceId(WORKSPACE.into()),
        },
        environment: Environment::Paper,
        liveness: Duration::from_secs(10),
    }
}

/// The agent stream on the in-memory journal; the control, account and clock streams as laid-down
/// rows, with a switch that makes the account stream unreadable; and a log of which stream each read
/// was of.
#[derive(Clone, Default)]
struct Rig {
    journal: MemoryStore,
    control: Rc<RefCell<Vec<StoredEvent>>>,
    account: Rc<RefCell<Vec<StoredEvent>>>,
    clock: Rc<RefCell<Vec<StoredEvent>>>,
    account_down: Rc<Cell<bool>>,
    reads: Rc<RefCell<Vec<String>>>,
}

impl Store for Rig {
    fn take_ownership(&mut self, stream: &StreamId) -> Option<u64> {
        self.journal.take_ownership(stream)
    }
    fn rows(&self, stream: &StreamId) -> Option<Vec<StoredEvent>> {
        self.reads.borrow_mut().push(stream.as_str().to_owned());
        match stream.as_str() {
            CONTROL => Some(self.control.borrow().clone()),
            CLOCK => Some(self.clock.borrow().clone()),
            ACCOUNT_STREAM if self.account_down.get() => None,
            ACCOUNT_STREAM => Some(self.account.borrow().clone()),
            OTHER_ACCOUNT_STREAM => panic!("another account's stream is never read"),
            _ => self.journal.rows(stream),
        }
    }
    fn append(
        &mut self,
        stream: &StreamId,
        head: u64,
        epoch: u64,
        recorded_at: UtcNanos,
        drafts: &[Vec<u8>],
    ) -> AppendOutcome {
        self.journal
            .append(stream, head, epoch, recorded_at, drafts)
    }
}

fn row(stream: &str, seq: u64, event_type: &str, actor: &str, payload: &str) -> StoredEvent {
    let space = match stream {
        CONTROL => 'C',
        CLOCK => 'K',
        _ => 'A',
    };
    let event_id = format!("01J8Z3M{space}{seq:018}");
    let body = format!(
        r#"{{"envelope_version":1,"environment":"paper","event_id":"{event_id}","stream_id":"{stream}","event_type":"{event_type}","schema_version":1,"event_time":"2026-10-16T13:00:00.000000000Z","clock_source":"scheduler","causation_id":null,"correlation_id":null,"actor":{{"kind":"{actor}","id":"x","version":"1","build":null}},"config_refs":{{}},"payload":{payload},"artifact_refs":[],"pii_refs":[]}}"#
    );
    StoredEvent {
        stream_id: stream.into(),
        seq,
        event_id,
        event_type: event_type.into(),
        schema_version: 1,
        environment: "paper".into(),
        recorded_at: "2026-10-16T13:00:00.000000000Z".into(),
        prev_hash: Digest::of(b"prev"),
        hash: Digest::of(body.as_bytes()),
        body: body.into_bytes(),
    }
}

fn next_seq(rows: &Rc<RefCell<Vec<StoredEvent>>>) -> u64 {
    u64::try_from(rows.borrow().len()).unwrap() + 1
}

/// The executor's reconciliation of this account, with its result.
fn reconciliation(rig: &Rig, result: &str) {
    let seq = next_seq(&rig.account);
    let payload = format!(r#"{{"result":"{result}","risk_clock":{NOW}}}"#);
    rig.account.borrow_mut().push(row(
        ACCOUNT_STREAM,
        seq,
        "ReconciliationRun",
        "system",
        &payload,
    ));
}

/// The executor's application of a restriction to `agent`'s mode.
fn mode_applied(rig: &Rig, agent: &str, to: &str) {
    let seq = next_seq(&rig.account);
    let payload = format!(
        r#"{{"agent":"{agent}","to":"{to}","restriction":"daily_loss","risk_clock":{NOW}}}"#
    );
    rig.account.borrow_mut().push(row(
        ACCOUNT_STREAM,
        seq,
        "AgentModeApplied",
        "system",
        &payload,
    ));
}

fn kill_switch(rig: &Rig) {
    let seq = next_seq(&rig.control);
    let payload = format!(
        r#"{{"agent":"{AGENT}","command":"kill_switch","scope":"agent","subject":"{AGENT}","bid":null,"bid_size":null,"floor":null,"submitted_at":{NOW},"step_up":null,"user":"user-owner"}}"#
    );
    rig.control
        .borrow_mut()
        .push(row(CONTROL, seq, "OwnerCommandIssued", "user", &payload));
}

fn tick(rig: &Rig, secs: i64) {
    let seq = next_seq(&rig.clock);
    let payload = format!(r#"{{"clock":"{}"}}"#, at(secs));
    rig.clock
        .borrow_mut()
        .push(row(CLOCK, seq, "ClockAdvanced", "system", &payload));
}

fn kinds(rig: &Rig) -> Vec<String> {
    rig.journal
        .rows(&StreamId::parse(AGENT_STREAM).unwrap())
        .unwrap()
        .iter()
        .map(|r| r.event_type.clone())
        .collect()
}

fn count(rig: &Rig, kind: &str) -> usize {
    kinds(rig).iter().filter(|k| *k == kind).count()
}

fn start(rig: &Rig, account: Option<&str>, now: i64) -> RuntimeProcess<Rig> {
    RuntimeProcess::start(rig.clone(), wiring(account), &config(), at(now))
}

/// A pass at `secs` with a fresh tick, so the core settles its mode on an input after the account
/// rows (the core settles the startup hold at the start of each input).
fn pass(rig: &Rig, process: &mut RuntimeProcess<Rig>, secs: i64) {
    tick(rig, secs);
    process.step(at(secs));
}

fn mode(process: &RuntimeProcess<Rig>) -> Mode {
    process.state().effective_mode()
}

/// DEC-2209 item 1: the binary's fixture wiring reads no account stream.
#[test]
fn the_fixture_wiring_names_no_account() {
    assert!(fixture::wiring("user-owner").account.is_none());
}

/// DEC-2209 item 1: with no account named, nothing is read of any account stream and the core keeps
/// its startup hold, however clean the account's reconciliation.
#[test]
fn with_no_account_named_the_runtime_keeps_its_startup_hold() {
    let rig = Rig::default();
    reconciliation(&rig, "clean");
    let mut process = start(&rig, None, NOW);
    pass(&rig, &mut process, NOW + 1);
    assert_eq!(mode(&process), Mode::Paused);
    assert!(
        rig.reads.borrow().iter().all(|s| !s.starts_with("acct:")),
        "{:?}",
        rig.reads.borrow()
    );
}

/// DEC-2209 item 2, DEC-131 item 13: a clean reconciliation folded at start, as the shell folds the
/// account stream before `Input::Started`, means no startup hold is taken at all.
#[test]
fn a_clean_reconciliation_folded_at_start_takes_no_hold() {
    let rig = Rig::default();
    reconciliation(&rig, "clean");
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    assert_eq!(process.lifecycle().state(), State::Ready);
    assert_eq!(mode(&process), Mode::Normal);
    pass(&rig, &mut process, NOW + 1);
    assert_eq!(mode(&process), Mode::Normal);
}

/// DEC-2209 item 3: a reconciliation the executor writes after the runtime started lifts the
/// startup hold at the pass that reads it.
#[test]
fn a_clean_reconciliation_after_start_lifts_the_hold() {
    let rig = Rig::default();
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    pass(&rig, &mut process, NOW + 1);
    assert_eq!(mode(&process), Mode::Paused);
    reconciliation(&rig, "clean");
    pass(&rig, &mut process, NOW + 2);
    assert_eq!(mode(&process), Mode::Normal, "{:?}", kinds(&rig));
    assert_eq!(process.lifecycle().state(), State::Ready);
}

/// The core's own rule: only a clean reconciliation lifts the hold.
#[test]
fn a_reconciliation_that_is_not_clean_keeps_the_hold() {
    let rig = Rig::default();
    reconciliation(&rig, "discrepancy");
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    reconciliation(&rig, "discrepancy");
    pass(&rig, &mut process, NOW + 1);
    assert_eq!(mode(&process), Mode::Paused);
}

/// DEC-2209 item 3: the account's restriction of this agent is honoured; another agent's is not.
#[test]
fn the_account_s_restriction_of_this_agent_is_honoured_and_another_s_is_not() {
    let rig = Rig::default();
    reconciliation(&rig, "clean");
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    mode_applied(&rig, "agent-b", "paused");
    pass(&rig, &mut process, NOW + 1);
    assert_eq!(mode(&process), Mode::Normal);
    mode_applied(&rig, AGENT, "exits_only");
    pass(&rig, &mut process, NOW + 2);
    assert_eq!(mode(&process), Mode::ExitsOnly);
    mode_applied(&rig, AGENT, "normal");
    pass(&rig, &mut process, NOW + 3);
    assert_eq!(mode(&process), Mode::Normal);
}

/// DEC-2209 item 3, DEC-1012 item 4: each pass reads the control stream, then the account stream,
/// then the clock stream.
#[test]
fn each_pass_reads_control_then_account_then_clock() {
    let rig = Rig::default();
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    kill_switch(&rig);
    reconciliation(&rig, "clean");
    tick(&rig, NOW + 1);
    rig.reads.borrow_mut().clear();
    process.step(at(NOW + 1));
    let reads = rig.reads.borrow().clone();
    let position = |stream: &str| reads.iter().position(|s| s == stream).unwrap();
    assert!(position(CONTROL) < position(ACCOUNT_STREAM), "{reads:?}");
    assert!(position(ACCOUNT_STREAM) < position(CLOCK), "{reads:?}");
}

/// DEC-2209 item 5: an account stream that cannot be read ends the pass after the kill switch, and
/// hands no tick on a stale view of the account.
#[test]
fn an_unreadable_account_stream_never_holds_the_kill_switch_and_hands_no_tick() {
    let rig = Rig::default();
    reconciliation(&rig, "clean");
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    rig.account_down.set(true);
    kill_switch(&rig);
    tick(&rig, NOW + 1);
    rig.reads.borrow_mut().clear();
    process.step(at(NOW + 1));
    assert_eq!(count(&rig, "KillSwitchActivated"), 1, "{:?}", kinds(&rig));
    assert!(
        !rig.reads.borrow().iter().any(|s| s == CLOCK),
        "{:?}",
        rig.reads.borrow()
    );
    let clock = || process.state().risk_clock().map(|clock| clock.secs());
    assert_eq!(clock(), Some(NOW), "only the account rows' own risk clock");
    assert_eq!(process.lifecycle().state(), State::Ready);
    rig.account_down.set(false);
    process.step(at(NOW + 2));
    assert_eq!(
        process.state().risk_clock().map(|clock| clock.secs()),
        Some(NOW + 1)
    );
}

/// DEC-2209 item 2: an account stream that cannot be read at start stops the process before the
/// core starts, and the restart replays.
#[test]
fn an_unreadable_account_stream_at_start_fails_the_cycle() {
    let rig = Rig::default();
    rig.account_down.set(true);
    let process = start(&rig, Some(ACCOUNT), NOW);
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
    rig.account_down.set(false);
    reconciliation(&rig, "clean");
    let restarted = start(&rig, Some(ACCOUNT), NOW + 5);
    assert_eq!(restarted.lifecycle().state(), State::Ready);
    assert_eq!(mode(&restarted), Mode::Normal);
}

/// DEC-2209 item 5: an account row the core cannot fold (here, one out of sequence) stops the
/// process rather than being skipped.
#[test]
fn an_account_row_the_core_cannot_fold_stops_the_process() {
    let rig = Rig::default();
    reconciliation(&rig, "clean");
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    let payload = format!(r#"{{"result":"clean","risk_clock":{NOW}}}"#);
    rig.account.borrow_mut().push(row(
        ACCOUNT_STREAM,
        3,
        "ReconciliationRun",
        "system",
        &payload,
    ));
    pass(&rig, &mut process, NOW + 1);
    assert_eq!(
        process.lifecycle().state(),
        State::Exited(Exit::CycleFailed)
    );
}

/// DEC-2209 item 1: an account that makes no stream id refuses the process before it takes the
/// agent stream.
#[test]
fn an_account_that_names_no_stream_is_refused_at_start() {
    let rig = Rig::default();
    let process = start(&rig, Some("not a segment"), NOW);
    assert_eq!(process.lifecycle().state(), State::Exited(Exit::Refused));
    assert!(
        rig.journal
            .rows(&StreamId::parse(AGENT_STREAM).unwrap())
            .unwrap_or_default()
            .is_empty()
    );
}

/// DEC-2209 item 4: each account row is handed once, so passes and restarts that read the same
/// rows write nothing more, and an earlier restriction is never re-applied.
#[test]
fn each_account_row_is_handed_once_across_passes_and_restarts() {
    let rig = Rig::default();
    reconciliation(&rig, "clean");
    let mut process = start(&rig, Some(ACCOUNT), NOW);
    mode_applied(&rig, AGENT, "exits_only");
    mode_applied(&rig, AGENT, "normal");
    pass(&rig, &mut process, NOW + 1);
    let written = kinds(&rig);
    for second in 2..5 {
        pass(&rig, &mut process, NOW + second);
    }
    assert_eq!(kinds(&rig), written, "nothing more for the same rows");
    let mut restarted = start(&rig, Some(ACCOUNT), NOW + 10);
    pass(&rig, &mut restarted, NOW + 11);
    assert_eq!(mode(&restarted), Mode::Normal);
    assert_eq!(
        count(&rig, "AgentModeChanged"),
        written.iter().filter(|k| *k == "AgentModeChanged").count(),
        "{:?}",
        kinds(&rig)
    );
    assert_eq!(
        restarted.state().head(ACCOUNT_STREAM),
        Some(Seq(3)),
        "the core holds the account stream's head"
    );
}

/// What a test does to the account and the process.
#[derive(Debug, Clone)]
enum Op {
    Reconciled(bool),
    Applied { here: bool, to: Mode },
    Pass,
    Restart,
}

fn op() -> impl Strategy<Value = Op> {
    let to = prop_oneof![
        Just(Mode::Normal),
        Just(Mode::ExitsOnly),
        Just(Mode::Paused)
    ];
    prop_oneof![
        any::<bool>().prop_map(Op::Reconciled),
        (any::<bool>(), to).prop_map(|(here, to)| Op::Applied { here, to }),
        Just(Op::Pass),
        Just(Op::Restart),
    ]
}

fn spelling(mode: Mode) -> &'static str {
    match mode {
        Mode::Normal => "normal",
        Mode::ExitsOnly => "exits_only",
        Mode::Paused => "paused",
        Mode::Stopped => "stopped",
    }
}

/// The oracle, from the account rows alone (DEC-131 items 13 and 22): `paused` until a clean
/// reconciliation, and from then the last restriction applied to this agent, `normal` before any.
#[derive(Default)]
struct Oracle {
    reconciled: bool,
    applied: Option<Mode>,
}

impl Oracle {
    fn expected(&self) -> Mode {
        let applied = self.applied.unwrap_or(Mode::Normal);
        if self.reconciled {
            applied
        } else {
            applied.max(Mode::Paused)
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 64,
        failure_persistence: None,
        ..ProptestConfig::default()
    })]

    /// DEC-2209 items 2 to 4 against an oracle that reads the account rows alone: after every pass,
    /// and after every restart, the core's mode is the oracle's, and the process is ready.
    #[test]
    fn the_mode_follows_the_account_stream_across_passes_and_restarts(
        ops in proptest::collection::vec(op(), 1..24)
    ) {
        let rig = Rig::default();
        let mut oracle = Oracle::default();
        let mut process = start(&rig, Some(ACCOUNT), NOW);
        let mut second = NOW;
        for op in ops {
            second += 1;
            match op {
                Op::Reconciled(clean) => {
                    reconciliation(&rig, if clean { "clean" } else { "discrepancy" });
                    oracle.reconciled |= clean;
                }
                Op::Applied { here, to } => {
                    mode_applied(&rig, if here { AGENT } else { "agent-b" }, spelling(to));
                    if here {
                        oracle.applied = Some(to);
                    }
                }
                Op::Pass => {
                    pass(&rig, &mut process, second);
                    prop_assert_eq!(mode(&process), oracle.expected(), "{:?}", kinds(&rig));
                }
                Op::Restart => {
                    process = start(&rig, Some(ACCOUNT), second);
                    pass(&rig, &mut process, second);
                    prop_assert_eq!(mode(&process), oracle.expected(), "{:?}", kinds(&rig));
                }
            }
            prop_assert_eq!(process.lifecycle().state(), State::Ready);
        }
    }
}
