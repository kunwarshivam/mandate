//! Which research run starts next, and with which tools (ADR-0005 §4, DEC-2204).
//!
//! A run is one session of the research loop. Its trigger fixes its kind, and its kind and the
//! agent's mode fix its tool classes, before anything untrusted is read: an event or a news item
//! can start a monitoring run, which may read, think, and reduce risk, but never propose a thesis
//! or an entry. Only the mandate's schedule starts an ideation run, the one kind that may propose.
//!
//! The queue holds at most one run in flight per agent (HI-8). Owner and explain runs go ahead of
//! every queued run and never interrupt the one in flight, which never holds up trading either:
//! the trading tick never waits on research (HI-7). Event triggers are debounced per subject and
//! cause, coalesced while a run for the same subject and cause is queued, and rate-limited per
//! agent over a rolling hour, so a news storm cannot buy a run storm. No run starts while the agent
//! is `paused` or `stopped`; a pause or stop cancels the run in flight at its next step and drops
//! what was queued, so nothing triggered before it starts after a resume.

use std::collections::{BTreeMap, VecDeque};

use mandate_runtime::{Mode, RiskClock};

/// A rolling hour, the rate limit's window.
const HOUR_S: i64 = 3600;

/// The kinds of research run (ADR-0005 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RunKind {
    /// From the mandate's research cadence: the one kind that may propose theses and entries.
    Ideation,
    /// From an event on a held or watched instrument: invalidation, review, or nothing.
    Monitoring,
    /// From a desk message: an answer or an explanation.
    Owner,
    /// From an ASK being raised: the plain-language text of its card.
    Explain,
}

/// What a monitoring run was triggered by (ADR-0005 §4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventCause {
    News,
    Filing,
    PriceBand,
    Fill,
    GateRefusal,
    ThesisExpiring,
}

/// What asks for a run. Every trigger is the platform's: none is model output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// The mandate's research cadence came due.
    Schedule { at: RiskClock },
    /// An event on an instrument the agent holds or watches.
    Event {
        at: RiskClock,
        subject: String,
        cause: EventCause,
    },
    /// The owner wrote on the desk.
    OwnerMessage { at: RiskClock, message_id: String },
    /// An approval request was raised and its card needs its text.
    AskRaised { at: RiskClock, approval_id: String },
}

impl Trigger {
    pub fn kind(&self) -> RunKind {
        match self {
            Self::Schedule { .. } => RunKind::Ideation,
            Self::Event { .. } => RunKind::Monitoring,
            Self::OwnerMessage { .. } => RunKind::Owner,
            Self::AskRaised { .. } => RunKind::Explain,
        }
    }
}

/// The classes of tool a run may be given. The tool names and schemas are the agent harness
/// spec's; the class is what the trigger decides.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ToolClass {
    /// Reads inside the run's bounds: market data, the ingested news and filings, memory, records.
    Read,
    /// No side effect: notes and plans, kept in the transcript.
    Think,
    /// Proposals that only reduce risk, which the harness never denies: an invalidation, an exit.
    RiskReducing,
    /// Proposals that may add risk, each judged by the harness, admission, the builder, the gate,
    /// and the autonomy rules: a thesis, a revision, an entry.
    Proposing,
}

const ALL: &[ToolClass] = &[
    ToolClass::Read,
    ToolClass::Think,
    ToolClass::RiskReducing,
    ToolClass::Proposing,
];
const REVIEW: &[ToolClass] = &[ToolClass::Read, ToolClass::Think, ToolClass::RiskReducing];
const ANSWER: &[ToolClass] = &[ToolClass::Read, ToolClass::Think];
const NONE: &[ToolClass] = &[];

/// The tool classes a run of `kind` gets in `mode`. A run's menu follows from these two alone,
/// never from the model. In `exits_only` a run is review-only (agent harness spec §6.7), and in
/// `paused` or `stopped` no run starts at all.
pub fn tool_classes(kind: RunKind, mode: Mode) -> &'static [ToolClass] {
    match (mode, kind) {
        (Mode::Paused | Mode::Stopped, _) => NONE,
        (_, RunKind::Owner | RunKind::Explain) => ANSWER,
        (Mode::ExitsOnly, _) | (_, RunKind::Monitoring) => REVIEW,
        (Mode::Normal, RunKind::Ideation) => ALL,
    }
}

/// The queue's bounds, set by the deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueConfig {
    /// Seconds after an accepted event during which another of the same subject and cause is
    /// dropped.
    pub debounce_s: u32,
    /// Monitoring runs that may start in any rolling hour.
    pub monitoring_per_hour: u32,
    /// Ideation and monitoring runs that may wait at once; owner and explain runs are never
    /// dropped for room.
    pub max_queued: usize,
}

/// What the queue did with a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offered {
    Queued,
    /// The same subject and cause had an event accepted within the debounce window.
    Debounced,
    /// A run for the same thing is already queued, and this trigger joins it.
    Coalesced,
    /// The queue holds `max_queued` ideation and monitoring runs already.
    Full,
}

/// A run the queue started: its kind, the trigger that asked for it, and its tool classes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Started {
    pub kind: RunKind,
    pub trigger: Trigger,
    pub tools: Vec<ToolClass>,
}

/// The per-agent trigger queue. It reads no clock: every time it judges is handed in.
#[derive(Debug, Clone)]
pub struct TriggerQueue {
    config: QueueConfig,
    /// Owner and explain runs, in arrival order.
    answers: VecDeque<Trigger>,
    /// Monitoring runs, in arrival order.
    monitoring: VecDeque<Trigger>,
    ideation: Option<Trigger>,
    in_flight: Option<RunKind>,
    /// The last accepted event of each subject and cause.
    accepted: BTreeMap<(String, EventCause), RiskClock>,
    /// When each monitoring run of the last hour started.
    monitoring_starts: VecDeque<RiskClock>,
}

impl TriggerQueue {
    pub fn new(config: QueueConfig) -> Self {
        Self {
            config,
            answers: VecDeque::new(),
            monitoring: VecDeque::new(),
            ideation: None,
            in_flight: None,
            accepted: BTreeMap::new(),
            monitoring_starts: VecDeque::new(),
        }
    }

    /// Takes a trigger into the queue, or says why not.
    pub fn offer(&mut self, trigger: Trigger) -> Offered {
        match &trigger {
            Trigger::OwnerMessage { .. } | Trigger::AskRaised { .. } => {
                self.answers.push_back(trigger);
                Offered::Queued
            }
            Trigger::Schedule { .. } => {
                if self.ideation.is_some() {
                    return Offered::Coalesced;
                }
                if self.waiting() >= self.config.max_queued {
                    return Offered::Full;
                }
                self.ideation = Some(trigger);
                Offered::Queued
            }
            Trigger::Event { at, subject, cause } => {
                let key = (subject.clone(), *cause);
                let window = i64::from(self.config.debounce_s);
                let recent = self
                    .accepted
                    .get(&key)
                    .is_some_and(|last| at.secs().saturating_sub(last.secs()) < window);
                if recent {
                    return Offered::Debounced;
                }
                if self
                    .monitoring
                    .iter()
                    .any(|queued| same_event(queued, &key))
                {
                    self.accepted.insert(key, *at);
                    return Offered::Coalesced;
                }
                if self.waiting() >= self.config.max_queued {
                    return Offered::Full;
                }
                self.accepted.insert(key, *at);
                self.monitoring.push_back(trigger);
                Offered::Queued
            }
        }
    }

    /// The run to start at `now` in `mode`, if one may start: none while one is in flight, none in
    /// `paused` or `stopped` (which also drops what was queued), and otherwise the first owner or
    /// explain run, then the first monitoring run the rate limit allows, then the ideation run.
    pub fn next(&mut self, now: RiskClock, mode: Mode) -> Option<Started> {
        if matches!(mode, Mode::Paused | Mode::Stopped) {
            self.drop_queued();
            return None;
        }
        if self.in_flight.is_some() {
            return None;
        }
        let trigger = match self.answers.pop_front() {
            Some(answer) => answer,
            None => match self.monitoring_allowed(now) {
                true if !self.monitoring.is_empty() => {
                    self.monitoring_starts.push_back(now);
                    self.monitoring.pop_front()?
                }
                _ => self.ideation.take()?,
            },
        };
        let kind = trigger.kind();
        self.in_flight = Some(kind);
        Some(Started {
            kind,
            trigger,
            tools: tool_classes(kind, mode).to_vec(),
        })
    }

    /// The run in flight ended, by any path: done, bounded, refused, or its cancellation
    /// acknowledged.
    pub fn ended(&mut self) {
        self.in_flight = None;
    }

    /// The agent's mode changed. Returns whether the run in flight must be cancelled at its next
    /// step: in `paused` or `stopped`, which also drop what was queued. The run stays in flight
    /// until [`TriggerQueue::ended`] says its cancellation was acknowledged.
    pub fn mode_changed(&mut self, mode: Mode) -> bool {
        if !matches!(mode, Mode::Paused | Mode::Stopped) {
            return false;
        }
        self.drop_queued();
        self.in_flight.is_some()
    }

    pub fn in_flight(&self) -> Option<RunKind> {
        self.in_flight
    }

    /// Every run waiting, of every kind.
    pub fn queued(&self) -> usize {
        self.answers.len().saturating_add(self.waiting())
    }

    /// The ideation and monitoring runs waiting, which `max_queued` bounds.
    fn waiting(&self) -> usize {
        self.monitoring
            .len()
            .saturating_add(usize::from(self.ideation.is_some()))
    }

    fn monitoring_allowed(&mut self, now: RiskClock) -> bool {
        while self
            .monitoring_starts
            .front()
            .is_some_and(|start| now.secs().saturating_sub(start.secs()) >= HOUR_S)
        {
            self.monitoring_starts.pop_front();
        }
        let limit = usize::try_from(self.config.monitoring_per_hour).unwrap_or(usize::MAX);
        self.monitoring_starts.len() < limit
    }

    fn drop_queued(&mut self) {
        self.answers.clear();
        self.monitoring.clear();
        self.ideation = None;
    }
}

fn same_event(queued: &Trigger, key: &(String, EventCause)) -> bool {
    matches!(queued, Trigger::Event { subject, cause, .. } if *subject == key.0 && *cause == key.1)
}
