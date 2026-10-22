//! The executor's run behind journal rule 68's gate (DEC-2341), and the wait before it on a
//! connection that is still connecting (DEC-2346; connections spec §5.2 step 4).
//!
//! The binary's edge is handed in as closures ([`Edge`]): the host clock, the lease's tick,
//! SIGTERM, and stderr; the probe reads the [`Status`] both publish. Everything the run decides is
//! here, so tests drive it with a fixture wiring that would admit an opening.

use std::time::Duration;

use mandate_process::probe::Status;
use mandate_store::Store;
use mandate_time::UtcNanos;

use crate::credential::Unimplemented;
use crate::{Broker, Config, Wiring};

/// What the run is handed from the binary's edge.
pub struct Edge<'a> {
    /// The host clock, the one place it is read (ES-05).
    pub now: &'a dyn Fn() -> UtcNanos,
    /// The lease's tick at a time: the code that stops the run, or `None` while it goes on and
    /// for a run that holds no lease.
    pub tick: &'a mut dyn FnMut(UtcNanos) -> Option<&'static str>,
    /// Whether SIGTERM has arrived.
    pub terminated: &'a dyn Fn() -> bool,
    /// Where each code goes: one line of stderr.
    pub say: &'a mut dyn FnMut(&str),
    /// The pause between passes.
    pub interval: Duration,
}

/// How a wait ended without refusing the start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waited {
    /// The account stream is established, or due its copy: the process may start.
    Ready,
    /// SIGTERM arrived first: nothing was started, so nothing is owed.
    Stopped,
}

/// DEC-2346: before the process starts, waits until the account stream in `config`'s scope is
/// established or due its copy, reading the journal once a pass and ticking the lease, and
/// publishes on `status` a lifecycle that is live and not ready. The wait appends nothing and asks
/// no broker anything. While it waits no agent can be deployed on the connection, so the platform
/// owes no exit or protective order (`AGENTS.md` rule 13).
///
/// # Errors
/// `connection_closed` for a connection refused or revoked before its copy, the establishment's
/// own code for a record that does not read, `stream_unavailable` for a stream that cannot be
/// read, and the lease's code when its tick stops the run: each exits before anything started.
pub fn wait<S: Store>(
    store: &S,
    config: &Config,
    status: &Status,
    edge: &mut Edge<'_>,
) -> Result<Waited, &'static str> {
    let _ = (store, config, status, edge);
    Err(Unimplemented)
}

/// Runs the process over `store` and `broker` with `wiring` behind rule 68's gate: the gate
/// guards `wiring`'s binding-gate source, and after each step a pass writes the establishment's
/// copy when it is due and opens the gate once the stream holds it. Each gate code is said once
/// per change, and the lifecycle is published on `status` each pass. A lease that stops the run,
/// or SIGTERM, drains it; it answers the exit status of the lifecycle it ended in.
///
/// # Errors
/// `drain_unfinished`, or the lease's code, when a drain does not end the lifecycle.
pub fn serve<S: Store, B: Broker>(
    store: S,
    broker: B,
    wiring: Wiring,
    config: &Config,
    status: &Status,
    edge: Edge<'_>,
) -> Result<u8, &'static str> {
    let _ = (store, broker, wiring, config, status, edge);
    Err(Unimplemented)
}
