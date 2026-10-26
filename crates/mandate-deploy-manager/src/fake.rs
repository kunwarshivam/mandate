//! An orchestrator in memory, for tests and the local drills: it does exactly what it is asked,
//! keeps every call, and lets a test crash, flap, or lose an instance.

use std::collections::BTreeMap;
use std::time::Duration;

use mandate_time::UtcNanos;

use crate::control::Slot;
use crate::orchestrator::{Instance, Launch, Orchestrator, Phase, Unavailable};

/// One call the manager made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    Start {
        slot: Slot,
        generation: u64,
        lifetime: Option<Duration>,
        expires_at: Option<UtcNanos>,
        connection: Option<String>,
    },
    Stop {
        slot: Slot,
        generation: u64,
        connection: Option<String>,
    },
}

/// The fake. A start creates a running instance that is not ready; a stop removes a running,
/// pending or exited instance at once, as deleting its pod would, and leaves an unknown one
/// unknown, as a lost node would.
#[derive(Debug, Clone, Default)]
pub struct Fake {
    instances: BTreeMap<(Slot, u64), Phase>,
    calls: Vec<Call>,
    /// While set, every call answers [`Unavailable`] and changes nothing.
    pub unavailable: bool,
    /// While set, starts and stops answer [`Unavailable`] and change nothing; listing still works.
    pub refuse_effects: bool,
    /// While set, a start is carried out and then answers [`Unavailable`], as one whose answer was
    /// lost would.
    pub lose_start_answers: bool,
}

impl Fake {
    pub fn new() -> Self {
        Self::default()
    }

    /// Every call so far, in order.
    pub fn calls(&self) -> &[Call] {
        &self.calls
    }

    /// The instances the fake holds, live or exited.
    pub fn instances(&self) -> Vec<Instance> {
        self.instances
            .iter()
            .map(|((slot, generation), phase)| Instance {
                slot: slot.clone(),
                generation: *generation,
                phase: *phase,
            })
            .collect()
    }

    /// The instances of `slot` that may still be running.
    pub fn live(&self, slot: &Slot) -> usize {
        self.instances
            .iter()
            .filter(|((s, _), phase)| s == slot && phase.live())
            .count()
    }

    /// Moves every live instance of `slot` in `from` to `to`.
    fn shift(&mut self, slot: &Slot, from: impl Fn(Phase) -> bool, to: Phase) {
        for ((s, _), phase) in &mut self.instances {
            if s == slot && from(*phase) {
                *phase = to;
            }
        }
    }

    /// The running instance of `slot` exits.
    pub fn crash(&mut self, slot: &Slot) {
        self.shift(
            slot,
            |p| matches!(p, Phase::Running { .. } | Phase::Pending),
            Phase::Exited,
        );
    }

    /// The running instance of `slot` exits successfully, as a token exchange that wrote its
    /// connection's token would.
    pub fn succeed(&mut self, slot: &Slot) {
        self.shift(
            slot,
            |p| matches!(p, Phase::Running { .. } | Phase::Pending),
            Phase::Succeeded,
        );
    }

    /// The running instance of `slot` reports `ready`.
    pub fn ready(&mut self, slot: &Slot, ready: bool) {
        self.shift(
            slot,
            |p| matches!(p, Phase::Running { .. }),
            Phase::Running { ready },
        );
    }

    /// The node of `slot`'s running instance is lost: it may still be running.
    pub fn lose_node(&mut self, slot: &Slot) {
        self.shift(
            slot,
            |p| matches!(p, Phase::Running { .. } | Phase::Pending),
            Phase::Unknown,
        );
    }

    /// Every exited instance, succeeded or not, is removed, as the orchestrator's garbage collection would.
    pub fn collect(&mut self) {
        self.instances.retain(|_, phase| phase.live());
    }

    /// Every unknown instance of `slot` is confirmed gone.
    pub fn vanish(&mut self, slot: &Slot) {
        self.instances
            .retain(|(s, _), phase| s != slot || *phase != Phase::Unknown);
    }
}

impl Orchestrator for Fake {
    async fn observe(&mut self) -> Result<Vec<Instance>, Unavailable> {
        if self.unavailable {
            return Err(Unavailable);
        }
        Ok(self.instances())
    }

    async fn start(
        &mut self,
        slot: &Slot,
        generation: u64,
        launch: &Launch,
    ) -> Result<(), Unavailable> {
        if self.unavailable || self.refuse_effects {
            return Err(Unavailable);
        }
        self.calls.push(Call::Start {
            slot: slot.clone(),
            generation,
            lifetime: launch.lifetime,
            expires_at: launch.expires_at,
            connection: launch.connection.clone(),
        });
        self.instances
            .entry((slot.clone(), generation))
            .or_insert(Phase::Running { ready: false });
        if self.lose_start_answers {
            return Err(Unavailable);
        }
        Ok(())
    }

    async fn stop(
        &mut self,
        slot: &Slot,
        generation: u64,
        connection: Option<&str>,
    ) -> Result<(), Unavailable> {
        if self.unavailable || self.refuse_effects {
            return Err(Unavailable);
        }
        self.calls.push(Call::Stop {
            slot: slot.clone(),
            generation,
            connection: connection.map(str::to_owned),
        });
        let key = (slot.clone(), generation);
        if matches!(
            self.instances.get(&key),
            Some(Phase::Running { .. } | Phase::Pending | Phase::Exited | Phase::Succeeded)
        ) {
            self.instances.remove(&key);
        }
        Ok(())
    }
}
