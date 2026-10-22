//! The orchestrator port: what the manager reads of the processes it runs, and the two things it
//! asks for. It reads probes and changes nothing else (infrastructure design §3.1, DEC-986).

use std::future::Future;
use std::time::Duration;

use crate::control::Slot;

/// What the orchestrator reports of one instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// Created, not yet running.
    Pending,
    /// Running, with its readiness probe's answer (design §3.4). Readiness is reported, never
    /// acted on: a process that is not ready still acts on the kill switch.
    Running { ready: bool },
    /// Exited, and never restarted by the orchestrator itself.
    Exited,
    /// Neither running nor exited as far as the orchestrator can tell (a lost node, or a stop in
    /// progress): it may still be running, so it counts as live.
    Unknown,
}

impl Phase {
    /// Whether the instance may still be running, and so still be its stream's writer.
    pub fn live(self) -> bool {
        !matches!(self, Self::Exited)
    }
}

/// One instance of a slot, by generation. An instance the orchestrator no longer lists is gone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    pub slot: Slot,
    pub generation: u64,
    pub phase: Phase,
}

/// An effect, performed only after its record is committed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Start { slot: Slot, generation: u64 },
    Stop { slot: Slot, generation: u64 },
    Alert(OperatorAlert),
}

/// The orchestrator could not answer or act; nothing is assumed to have happened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unavailable;

/// Starts and stops processes, and lists the ones the manager started. Each call is idempotent:
/// starting an instance that exists, or stopping one that is gone, succeeds.
pub trait Orchestrator {
    /// Every instance this workspace's manager started that the orchestrator still lists. The list
    /// must be current: an instance missing from it counts as gone, and its slot's next generation
    /// may start (the Kubernetes API's list is a quorum read; the writer epoch still fences a copy
    /// a stale list hid).
    fn observe(&mut self) -> impl Future<Output = Result<Vec<Instance>, Unavailable>>;
    /// Creates `generation` of `slot`, which the orchestrator must never restart on its own. A token
    /// exchange is given the `lifetime` its request has left and ends by it; a standing process is
    /// given none.
    fn start(
        &mut self,
        slot: &Slot,
        generation: u64,
        lifetime: Option<Duration>,
    ) -> impl Future<Output = Result<(), Unavailable>>;
    /// Asks `generation` of `slot` to stop, by the process's own drain (design §7.3).
    fn stop(
        &mut self,
        slot: &Slot,
        generation: u64,
    ) -> impl Future<Output = Result<(), Unavailable>>;
}

/// An operator alert (design §8.2, OPS-10): a reason code and a runbook, nothing else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperatorAlert {
    pub code: &'static str,
    pub runbook: &'static str,
}

/// §8.2's "Crash loop; agent left paused".
pub const CRASH_LOOP: OperatorAlert = OperatorAlert {
    code: "crash_loop",
    runbook: "RB-14",
};

/// A version-1 `AgentDeployed` of an agent not stopped: it names no connection, so the manager
/// starts nothing for it until the owner stops it and deploys a new agent id at version 2
/// (DEC-1173 item 3).
pub const DEPLOYMENT_UNREAD: OperatorAlert = OperatorAlert {
    code: "deployment_unread",
    runbook: "RB-14",
};

/// A version-2 `AgentDeployed` that rule 201 refuses, of an agent not stopped: its connection was
/// not established on the control stream, or was revoked, before it, so the manager starts nothing
/// for it, and a later establishment does not revive it, until the owner stops it and deploys a new
/// agent id on an established connection (journal spec §9.24 rule 201, DEC-2038).
pub const DEPLOYMENT_UNCONNECTED: OperatorAlert = OperatorAlert {
    code: "deployment_unconnected",
    runbook: "RB-14",
};

/// Where operator alerts go. Raising one never fails the manager.
pub trait Alerts {
    fn raise(&mut self, alert: OperatorAlert);
}
