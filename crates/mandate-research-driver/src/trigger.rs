//! Which run's trigger goes to the core next (agent harness spec v0.2 §6.9, ADR-0005 §4, DEC-2204).
//!
//! The queue only arbitrates. At most one run is in flight per agent (HI-8). An owner or explain
//! run goes ahead of a queued scheduled run, and never interrupts the one in flight, which never
//! delays trading either: the trading tick never waits on research (HI-7). No run starts while the
//! agent is `paused` or `stopped`, and a pause or stop drops what was queued, so nothing queued
//! before it starts after a resume. Any mode change cancels the run in flight: §6.9 ends it as
//! `mode`, whatever the new mode allows.
//!
//! The core decides the rest from the trigger and its folded state alone, before anything untrusted
//! is read: whether a scheduled run is due, whether it is `ideate` or `monitor`, and its tool menu.
//! Event triggers (news, filings, price moves, fills, refusals) wait for a mandate spec change that
//! lists them as envelope fields with their debounce and rate limit (rule 11, E19-14), so the queue
//! takes none.

use std::collections::VecDeque;

use mandate_runtime::{Mode, RiskClock};

/// What asks for a run. Every trigger is the platform's: none is model output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Trigger {
    /// The scheduler's tick: a scheduled run may be due (§5.3's interval, judged by the core).
    Schedule { at: RiskClock },
    /// The owner asked on the desk; `cause` is the copied question's `event_id` (§15, lane EX).
    OwnerQuestion { at: RiskClock, cause: String },
    /// An approval request was raised; `cause` is its `ApprovalRequested`'s `event_id` (§15).
    Approval { at: RiskClock, cause: String },
}

/// The queue's bound, set by the deployment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueConfig {
    /// Owner and explain runs that may wait at once. A scheduled run has its own slot, so they
    /// never crowd it out.
    pub max_answers: usize,
}

/// What the queue did with a trigger.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Offered {
    Queued,
    /// A run for the same thing is already queued: another schedule tick, or the same question or
    /// approval again.
    Coalesced,
    /// `max_answers` owner and explain runs are waiting already.
    Full,
}

/// The per-agent trigger queue. It reads no clock.
#[derive(Debug, Clone)]
pub struct TriggerQueue {
    config: QueueConfig,
    /// Owner and explain runs, in arrival order.
    answers: VecDeque<Trigger>,
    schedule: Option<Trigger>,
    /// The mode the run in flight started in, if one is in flight.
    in_flight: Option<Mode>,
}

impl TriggerQueue {
    pub fn new(config: QueueConfig) -> Self {
        Self {
            config,
            answers: VecDeque::new(),
            schedule: None,
            in_flight: None,
        }
    }

    /// Takes a trigger into the queue, or says why not.
    pub fn offer(&mut self, trigger: Trigger) -> Offered {
        match &trigger {
            Trigger::Schedule { .. } => {
                if self.schedule.is_some() {
                    return Offered::Coalesced;
                }
                self.schedule = Some(trigger);
                Offered::Queued
            }
            Trigger::OwnerQuestion { .. } | Trigger::Approval { .. } => {
                if self.answers.iter().any(|q| same(q, &trigger)) {
                    return Offered::Coalesced;
                }
                if self.answers.len() >= self.config.max_answers {
                    return Offered::Full;
                }
                self.answers.push_back(trigger);
                Offered::Queued
            }
        }
    }

    /// The trigger whose run starts now in `mode`, if one may start: none while one is in flight,
    /// none in `paused` or `stopped` (which also drops what was queued), and otherwise the first
    /// owner or explain run, then the scheduled one.
    pub fn next(&mut self, mode: Mode) -> Option<Trigger> {
        if matches!(mode, Mode::Paused | Mode::Stopped) {
            self.drop_queued();
            return None;
        }
        if self.in_flight.is_some() {
            return None;
        }
        let trigger = self.answers.pop_front().or_else(|| self.schedule.take())?;
        self.in_flight = Some(mode);
        Some(trigger)
    }

    /// The run in flight ended, by any path.
    pub fn ended(&mut self) {
        self.in_flight = None;
    }

    /// The agent's mode changed. Returns whether the run in flight must be cancelled: any change
    /// from the mode it started in. A change to `paused` or `stopped` also drops what was queued.
    /// The run stays in flight until [`TriggerQueue::ended`].
    pub fn mode_changed(&mut self, mode: Mode) -> bool {
        if matches!(mode, Mode::Paused | Mode::Stopped) {
            self.drop_queued();
        }
        self.in_flight.is_some_and(|started| started != mode)
    }

    pub fn in_flight(&self) -> bool {
        self.in_flight.is_some()
    }

    /// Every run waiting.
    pub fn queued(&self) -> usize {
        self.answers
            .len()
            .saturating_add(usize::from(self.schedule.is_some()))
    }

    fn drop_queued(&mut self) {
        self.answers.clear();
        self.schedule = None;
    }
}

/// Whether two answer triggers ask for the same run: the same kind and the same cause.
fn same(queued: &Trigger, offered: &Trigger) -> bool {
    match (queued, offered) {
        (Trigger::OwnerQuestion { cause: a, .. }, Trigger::OwnerQuestion { cause: b, .. })
        | (Trigger::Approval { cause: a, .. }, Trigger::Approval { cause: b, .. }) => a == b,
        _ => false,
    }
}
