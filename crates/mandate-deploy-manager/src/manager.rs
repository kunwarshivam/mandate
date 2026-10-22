//! The manager's fold and its decision (infrastructure design §3.4; DEC-1170). It reads no clock:
//! every time arrives with its input, so the same inputs give the same decisions.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use mandate_process::{CRASH_LOOP_WINDOW, crash_loop};
use mandate_time::UtcNanos;

use crate::control::{Cause, Fact, Process, Scope, Slot, StopCause};
use crate::orchestrator::{
    CRASH_LOOP, DEPLOYMENT_UNCONNECTED, DEPLOYMENT_UNREAD, Effect, Instance, Phase,
};
use crate::record::Record;

/// The first restart's back-off; each restart counted in the crash-loop window doubles it.
pub const BACKOFF_BASE: Duration = Duration::from_secs(1);
/// The longest back-off.
pub const BACKOFF_CAP: Duration = Duration::from_secs(300);

/// The wait before a restart, after `restarts` restarts within the crash-loop window: exponential
/// from [`BACKOFF_BASE`], capped at [`BACKOFF_CAP`] (design §3.4).
pub fn backoff(restarts: usize) -> Duration {
    u32::try_from(restarts)
        .ok()
        .and_then(|n| 1_u32.checked_shl(n))
        .and_then(|factor| BACKOFF_BASE.checked_mul(factor))
        .map_or(BACKOFF_CAP, |wait| wait.min(BACKOFF_CAP))
}

/// One step for one slot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Journal the record, then perform its effect.
    Journal(Record),
    /// Perform an effect whose record is already journaled: a start or stop the orchestrator has
    /// not yet carried out.
    Retry(Effect),
}

/// A connection's OAuth request: its token exchange's deadline, whether a record closed it, its
/// account, whether its executor's `exchanged` start is journaled, and whether a refusal or a
/// revocation ended the connection since (DEC-2335).
#[derive(Debug, Clone)]
struct Request {
    deadline: UtcNanos,
    closed: bool,
    account: String,
    exchanged: bool,
    ended: bool,
}

#[derive(Debug, Clone)]
struct Agent {
    account: String,
    stopped: bool,
}

#[derive(Debug, Clone, Default)]
struct SlotState {
    generation: u64,
    restarts: Vec<UtcNanos>,
    looped: bool,
    kill_switches: BTreeSet<String>,
    /// The kill switches a start already answered, so a row read again never asks for another.
    answered: BTreeSet<String>,
    stopping: BTreeSet<u64>,
    /// The generations whose stop this manager saw carried out: a token exchange's account is
    /// deleted then, listed pod or not, so its stop is not asked again.
    swept: BTreeSet<u64>,
    /// This manager journaled the current generation's start and the orchestrator has not yet
    /// carried it out. Never set by a replay: after a restart, a generation the orchestrator does
    /// not list counts as exited, so its next start is journaled and counted.
    awaiting_launch: bool,
    /// This manager raised the crash-loop alert for the slot.
    alerted: bool,
    /// For an executor: the last record that took a reason to want it away was its connection's
    /// refusal or revocation, not an agent's stop, so its stop's cause is `connection_closed`.
    connection_closed: bool,
    down_since: Option<UtcNanos>,
}

/// One workspace's deployment manager: the control stream folded, and what it has asked of the
/// orchestrator since it started.
#[derive(Debug, Clone, Default)]
pub struct Manager {
    environment: Option<String>,
    agents: BTreeMap<String, Agent>,
    /// Each connection's account, from its first version-2 establishment: a kill switch's scope
    /// reads it whether or not the connection is revoked since.
    connections: BTreeMap<String, String>,
    /// The connections whose latest establishment or revocation is an establishment: the ones a
    /// deployment can start on (journal spec §9.24 rule 201).
    established: BTreeSet<String>,
    /// Every agent an `AgentStopped` names, read or not: a stop is final (§9.21, DEC-1173 item 2).
    stopped: BTreeSet<String>,
    /// The version-1 deployments of agents not stopped and never read, which the manager cannot
    /// run, and whether it has reported each (DEC-1173 item 3).
    unread: BTreeMap<String, bool>,
    /// The version-2 deployments rule 201 refused, of agents not stopped and never run, and
    /// whether this manager has reported each (DEC-2038).
    unconnected: BTreeMap<String, bool>,
    slots: BTreeMap<Slot, SlotState>,
    /// Each connection's OAuth request, by `connection_id` (DEC-1175).
    requests: BTreeMap<String, Request>,
    /// Every connection a record closed a request of, so a request read after one starts nothing.
    closed: BTreeSet<String>,
    /// Every connection refused or revoked, so a request read after one wants no executor.
    ended: BTreeSet<String>,
}

impl Manager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The control stream's environment, once its `StreamOpened` is folded.
    pub fn environment(&self) -> Option<&str> {
        self.environment.as_deref()
    }

    /// Folds one control-stream fact. Each is idempotent, so a record folded when it commits and
    /// again when it is read back changes nothing the second time.
    pub fn fold(&mut self, fact: &Fact) {
        match fact {
            Fact::Opened { environment } => self.environment = Some(environment.clone()),
            Fact::Connected {
                connection,
                account,
            } => {
                self.connections
                    .entry(connection.clone())
                    .or_insert_with(|| account.clone());
                self.established.insert(connection.clone());
                self.close(connection);
            }
            Fact::Revoked { connection } => {
                self.established.remove(connection);
                self.close(connection);
                self.end(connection);
            }
            Fact::Requested {
                connection,
                account,
                deadline,
            } => {
                let closed = self.closed.contains(connection);
                let ended = self.ended.contains(connection);
                self.requests
                    .entry(connection.clone())
                    .or_insert_with(|| Request {
                        deadline: *deadline,
                        closed,
                        account: account.clone(),
                        exchanged: false,
                        ended,
                    });
                self.slots
                    .entry(Slot::token_exchange(connection))
                    .or_default();
            }
            Fact::RequestClosed { connection } => self.close(connection),
            Fact::Refused { connection } => {
                self.close(connection);
                self.end(connection);
            }
            Fact::DeployedUnread { agent } => {
                if !self.agents.contains_key(agent) && !self.stopped.contains(agent) {
                    self.unread.entry(agent.clone()).or_insert(false);
                }
            }
            Fact::Deployed { agent, connection } => {
                let account = self
                    .established
                    .contains(connection)
                    .then(|| self.connections.get(connection).cloned())
                    .flatten();
                let known = self.agents.contains_key(agent) || self.stopped.contains(agent);
                if account.is_none() && !known {
                    self.unconnected.entry(agent.clone()).or_insert(false);
                }
                if let Some(account) = account
                    && !known
                {
                    self.unread.remove(agent);
                    self.unconnected.remove(agent);
                    let executor = Slot::executor(&account);
                    if !self.desired(&executor) {
                        self.resume(&executor);
                    }
                    self.slots.entry(Slot::runtime(agent)).or_default();
                    self.slots.entry(Slot::executor(&account)).or_default();
                    self.agents.insert(
                        agent.clone(),
                        Agent {
                            account,
                            stopped: false,
                        },
                    );
                }
            }
            Fact::Stopped { agent } => {
                self.stopped.insert(agent.clone());
                self.unread.remove(agent);
                self.unconnected.remove(agent);
                let account = self.agents.get_mut(agent).and_then(|known| {
                    let was_running = !known.stopped;
                    known.stopped = true;
                    was_running.then(|| known.account.clone())
                });
                if let Some(account) = account {
                    let executor = Slot::executor(&account);
                    if let Some(state) = self.slots.get_mut(&executor) {
                        state.connection_closed = false;
                    }
                    if !self.desired(&executor) {
                        self.resume(&executor);
                    }
                }
            }
            Fact::KillSwitch { event_id, scope } => {
                let covered: Vec<Slot> = self
                    .slots
                    .keys()
                    .filter(|slot| self.covers(scope, slot))
                    .cloned()
                    .collect();
                for slot in covered {
                    if let Some(state) = self.slots.get_mut(&slot)
                        && !state.answered.contains(event_id)
                    {
                        state.kill_switches.insert(event_id.clone());
                    }
                }
            }
            Fact::Started {
                slot,
                generation,
                cause,
                at,
            } => {
                if *cause == Cause::Exchanged {
                    for request in self.requests.values_mut() {
                        if request.account == slot.subject {
                            request.exchanged = true;
                        }
                    }
                }
                let state = self.slots.entry(slot.clone()).or_default();
                if *generation > state.generation {
                    state.generation = *generation;
                    let answered = std::mem::take(&mut state.kill_switches);
                    state.answered.extend(answered);
                    state.awaiting_launch = false;
                    state.down_since = None;
                    match cause {
                        Cause::Deployed | Cause::Requested | Cause::Exchanged => {
                            state.restarts.clear();
                        }
                        Cause::Restart => state.restarts.push(*at),
                        Cause::KillSwitch => {}
                    }
                }
            }
            Fact::StopIssued { slot, generation } => {
                self.slots
                    .entry(slot.clone())
                    .or_default()
                    .stopping
                    .insert(*generation);
            }
            Fact::CrashLooped { slot } => self.slots.entry(slot.clone()).or_default().looped = true,
        }
    }

    /// Closes `connection`'s request, read or still to come (rule 131).
    fn close(&mut self, connection: &str) {
        self.closed.insert(connection.to_owned());
        if let Some(request) = self.requests.get_mut(connection) {
            request.closed = true;
        }
    }

    /// Ends `connection`'s connect: its executor is no longer wanted for it, and if that executor
    /// was started for it, its stop's cause is `connection_closed` (DEC-2335).
    fn end(&mut self, connection: &str) {
        self.ended.insert(connection.to_owned());
        let Some(request) = self.requests.get_mut(connection) else {
            return;
        };
        request.ended = true;
        if request.exchanged
            && let Some(state) = self.slots.get_mut(&Slot::executor(&request.account))
        {
            state.connection_closed = true;
        }
    }

    /// The `exchanged` start of the executor of `connection`'s account, once its token exchange
    /// succeeded: only while its request is open, and only if that executor never ran, since rule
    /// 167 holds `exchanged` to generation 1.
    fn exchanged(&self, connection: &str) -> Option<Decision> {
        let request = self.requests.get(connection)?;
        let executor = Slot::executor(&request.account);
        if request.closed || request.exchanged || self.slots.contains_key(&executor) {
            return None;
        }
        Some(start(&executor, 1, Cause::Exchanged, None))
    }

    /// What a token exchange's request has left at `now`, which its pod is given to end by; `None`
    /// for every other process, which runs until it is stopped.
    pub fn lifetime(&self, slot: &Slot, now: UtcNanos) -> Option<Duration> {
        match slot.process {
            Process::TokenExchange => self
                .requests
                .get(&slot.subject)
                .map(|request| elapsed(now, request.deadline)),
            Process::Runtime | Process::Executor => None,
        }
    }

    /// The connection `slot` acts for, which an executor runs as the account of (DEC-2060 item 9):
    /// the connect whose exchange started it, else the established connection of its account, as
    /// a deployment on it names. `None` for a runtime and a token exchange, whose subjects say
    /// what they need.
    pub fn connection(&self, slot: &Slot) -> Option<String> {
        if slot.process != Process::Executor {
            return None;
        }
        let exchanged = self
            .requests
            .iter()
            .find(|(_, r)| r.exchanged && r.account == slot.subject);
        let established = self.connections.iter().find(|(_, a)| **a == slot.subject);
        exchanged
            .map(|(connection, _)| connection)
            .or(established.map(|(connection, _)| connection))
            .cloned()
    }

    /// Ends an executor's crash-loop pause once no agent wants it: every agent of its account is
    /// stopped (DEC-1170 item 3).
    fn resume(&mut self, executor: &Slot) {
        if let Some(state) = self.slots.get_mut(executor) {
            state.looped = false;
            state.alerted = false;
            state.restarts.clear();
        }
    }

    /// Records that this manager journaled the start of `generation` of `slot` and is about to ask
    /// the orchestrator for it.
    pub fn issued(&mut self, slot: &Slot, generation: u64) {
        if let Some(state) = self.slots.get_mut(slot)
            && state.generation == generation
        {
            state.awaiting_launch = true;
        }
    }

    /// Records that the orchestrator carried out the stop of `generation` of `slot`.
    pub fn stopped(&mut self, slot: &Slot, generation: u64) {
        if let Some(state) = self.slots.get_mut(slot) {
            state.swept.insert(generation);
        }
    }

    /// Records that the orchestrator carried out the start of `generation` of `slot`.
    pub fn launched(&mut self, slot: &Slot, generation: u64) {
        if let Some(state) = self.slots.get_mut(slot)
            && state.generation == generation
        {
            state.awaiting_launch = false;
        }
    }

    /// Whether the control stream wants `slot` running: a runtime for an agent deployed and not
    /// stopped, an executor for an account with such an agent, and a token exchange for a
    /// connection whose OAuth request is open.
    pub fn desired(&self, slot: &Slot) -> bool {
        match slot.process {
            Process::Runtime => self
                .agents
                .get(&slot.subject)
                .is_some_and(|agent| !agent.stopped),
            Process::Executor => {
                self.agents
                    .values()
                    .any(|agent| !agent.stopped && agent.account == slot.subject)
                    || self.requests.values().any(|request| {
                        request.exchanged && !request.ended && request.account == slot.subject
                    })
            }
            Process::TokenExchange => self
                .requests
                .get(&slot.subject)
                .is_some_and(|request| !request.closed),
        }
    }

    /// Whether a kill switch of `scope` covers `slot`. None covers a token exchange: it holds no
    /// order (DEC-1175 item 3).
    pub fn covers(&self, scope: &Scope, slot: &Slot) -> bool {
        let account_of = |agent: &str| self.agents.get(agent).map(|a| a.account.as_str());
        let connected = |connection: &str| self.connections.get(connection).map(String::as_str);
        match (scope, slot.process) {
            (Scope::Workspace, _) => true,
            (Scope::Agent(agent), Process::Runtime) => *agent == slot.subject,
            (Scope::Agent(agent), Process::Executor) => {
                account_of(agent) == Some(slot.subject.as_str())
            }
            (Scope::Connection(connection), Process::Executor) => {
                connected(connection) == Some(slot.subject.as_str())
                    || self
                        .requests
                        .get(connection)
                        .is_some_and(|request| request.account == slot.subject)
            }
            (Scope::Connection(connection), Process::Runtime) => {
                account_of(&slot.subject).is_some_and(|a| connected(connection) == Some(a))
            }
            (_, Process::TokenExchange) => false,
        }
    }

    /// Whether `slot` is left down after a crash loop.
    pub fn paused(&self, slot: &Slot) -> bool {
        self.slots.get(slot).is_some_and(|state| state.looped)
    }

    /// One operator alert for each unread deployment, and each one rule 201 refused, not yet
    /// reported, then at most one step per slot, from what the orchestrator lists at `now`. The alerts come first: raising one never
    /// fails, so a journal append that halts the pass after them cannot lose one that is already
    /// marked reported. A slot with a live instance is never started again and, while desired,
    /// never stopped: neither readiness nor a kill switch stops a process.
    pub fn decide(&mut self, observed: &[Instance], now: UtcNanos) -> Vec<Decision> {
        let mut decisions = Vec::new();
        for reported in self.unread.values_mut().filter(|reported| !**reported) {
            *reported = true;
            decisions.push(Decision::Retry(Effect::Alert(DEPLOYMENT_UNREAD)));
        }
        for reported in self.unconnected.values_mut().filter(|reported| !**reported) {
            *reported = true;
            decisions.push(Decision::Retry(Effect::Alert(DEPLOYMENT_UNCONNECTED)));
        }
        let succeeded: Vec<String> = observed
            .iter()
            .filter(|i| i.phase == Phase::Succeeded && i.slot.process == Process::TokenExchange)
            .map(|i| i.slot.subject.clone())
            .collect();
        for start in succeeded.iter().filter_map(|c| self.exchanged(c)) {
            if !decisions.contains(&start) {
                decisions.push(start);
            }
        }
        let slots: Vec<Slot> = self.slots.keys().cloned().collect();
        decisions.extend(slots.into_iter().filter_map(|slot| {
            let desired = self.desired(&slot);
            let lifetime = self.lifetime(&slot, now);
            let state = self.slots.get_mut(&slot)?;
            let instances: Vec<&Instance> = observed.iter().filter(|i| i.slot == slot).collect();
            match lifetime {
                Some(lifetime) => exchange_step(&slot, state, desired, lifetime, &instances),
                None => step(&slot, state, desired, &instances, now),
            }
        }));
        decisions
    }
}

fn step(
    slot: &Slot,
    state: &mut SlotState,
    desired: bool,
    instances: &[&Instance],
    now: UtcNanos,
) -> Option<Decision> {
    let live = instances.iter().find(|i| i.phase.live());
    if !desired {
        let generation = live?.generation;
        let cause = match slot.process {
            Process::Executor if state.connection_closed => StopCause::ConnectionClosed,
            _ => StopCause::AgentStopped,
        };
        return Some(if state.stopping.contains(&generation) {
            Decision::Retry(Effect::Stop {
                slot: slot.clone(),
                generation,
            })
        } else {
            Decision::Journal(Record::Stop {
                slot: slot.clone(),
                generation,
                cause,
            })
        });
    }
    if live.is_some() {
        state.down_since = None;
        return None;
    }
    let listed = instances.iter().any(|i| i.generation == state.generation);
    if state.generation == 0 {
        return Some(start(slot, 1, Cause::Deployed, None));
    }
    if !listed && state.awaiting_launch {
        return Some(Decision::Retry(Effect::Start {
            slot: slot.clone(),
            generation: state.generation,
        }));
    }
    let down_since = *state.down_since.get_or_insert(now);
    let next = state.generation.checked_add(1)?;
    if let Some(kill_switch) = state.kill_switches.iter().next() {
        return Some(start(
            slot,
            next,
            Cause::KillSwitch,
            Some(kill_switch.clone()),
        ));
    }
    if state.looped {
        if state.alerted {
            return None;
        }
        state.alerted = true;
        return Some(Decision::Retry(Effect::Alert(CRASH_LOOP)));
    }
    if crash_loop(state.restarts.iter().copied(), now) {
        state.alerted = true;
        let restarts = within_window(&state.restarts, now);
        return Some(Decision::Journal(Record::CrashLooped {
            slot: slot.clone(),
            generation: state.generation,
            restarts: u64::try_from(restarts).unwrap_or(u64::MAX),
        }));
    }
    let wait = backoff(within_window(&state.restarts, now));
    (elapsed(down_since, now) >= wait).then(|| start(slot, next, Cause::Restart, None))
}

/// A token exchange's step (DEC-1175 items 2 to 4, DEC-1177 item 12): one start, at generation 1, while its request
/// is open and has at least a second left, asked of the orchestrator again only while this
/// manager has not seen it carried out; never a restart; and once the request closes, a stop of
/// every instance still listed, exited or not, so its pod and account are deleted, and of the last
/// generation started even when none is listed, until this manager sees one carried out, so an
/// account whose pod never ran is deleted too (DEC-2332 item 3). A connection id is one connect's
/// alone (journal spec rule 131), so a closed request's exchange is never started again. The
/// deadline stops nothing here: the pod ends by it, and the connection manager's teardown journals
/// the request's closing record (connections spec §5.2 step 6), which this step then answers. The deadline
/// stops nothing: the pod ends by it.
fn exchange_step(
    slot: &Slot,
    state: &mut SlotState,
    open: bool,
    lifetime: Duration,
    instances: &[&Instance],
) -> Option<Decision> {
    if !open {
        let listed = instances.iter().map(|i| i.generation).max();
        let generation = listed.or((state.generation >= 1).then_some(state.generation))?;
        if listed.is_none() && state.swept.contains(&generation) {
            return None;
        }
        return Some(if state.stopping.contains(&generation) {
            Decision::Retry(Effect::Stop {
                slot: slot.clone(),
                generation,
            })
        } else {
            Decision::Journal(Record::Stop {
                slot: slot.clone(),
                generation,
                cause: StopCause::RequestClosed,
            })
        });
    }
    if lifetime < Duration::from_secs(1) {
        return None;
    }
    if state.generation == 0 {
        return Some(start(slot, 1, Cause::Requested, None));
    }
    let listed = instances.iter().any(|i| i.generation == state.generation);
    (state.awaiting_launch && !listed).then(|| {
        Decision::Retry(Effect::Start {
            slot: slot.clone(),
            generation: state.generation,
        })
    })
}

fn start(slot: &Slot, generation: u64, cause: Cause, kill_switch: Option<String>) -> Decision {
    Decision::Journal(Record::Start {
        slot: slot.clone(),
        generation,
        cause,
        kill_switch,
    })
}

/// The restarts at or after `now` less the crash-loop window, as `crash_loop` counts them.
fn within_window(restarts: &[UtcNanos], now: UtcNanos) -> usize {
    let window = i128::try_from(CRASH_LOOP_WINDOW.as_nanos()).unwrap_or(i128::MAX);
    let since = nanos(now).saturating_sub(window);
    restarts.iter().filter(|r| nanos(**r) >= since).count()
}

fn elapsed(since: UtcNanos, now: UtcNanos) -> Duration {
    let span = nanos(now).saturating_sub(nanos(since));
    u64::try_from(span).map_or(Duration::ZERO, Duration::from_nanos)
}

fn nanos(at: UtcNanos) -> i128 {
    i128::from(at.secs())
        .saturating_mul(1_000_000_000)
        .saturating_add(i128::from(at.nanos()))
}
