//! Heartbeats and the fleet view (design §3.4, E20-5): versions and health only, and a silent
//! deployment only marked `unreachable` (CP-8).

use mandate_canon::Digest;
use mandate_time::UtcNanos;

use crate::directory::{Deployment, Ident};

/// Heartbeats come every 60 seconds (design §3.4); three missed mark a deployment unreachable.
pub const UNREACHABLE_AFTER_SECS: i64 = 180;

/// A component's health (design §3.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Health {
    Ok,
    Degraded,
    Down,
}

/// One component's version and health. `version` is a release's version text,
/// `[0-9A-Za-z.+_-]`, 1 to 64 bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Component {
    pub name: Ident,
    pub version: String,
    pub health: Health,
}

impl Component {
    pub fn new(name: Ident, version: &str, health: Health) -> Option<Self> {
        let grammar = version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'+' | b'_' | b'-'));
        (grammar && !version.is_empty() && version.len() <= 64).then(|| Self {
            name,
            version: version.to_owned(),
            health,
        })
    }
}

/// A deployment's heartbeat: its release digest and its components (design §3.4). Counts arrive
/// in the usage report, never here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heartbeat {
    pub release_digest: Digest,
    pub components: Vec<Component>,
}

/// Whether the plane has heard from a deployment lately.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reachability {
    /// No heartbeat since enrollment.
    NeverSeen,
    Reachable,
    /// No heartbeat for [`UNREACHABLE_AFTER_SECS`]. An alert, never a revocation (CP-8).
    Unreachable,
}

/// One row of the fleet view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FleetRow {
    pub deployment_id: Ident,
    pub enrolled: bool,
    pub last_heartbeat: Option<UtcNanos>,
    pub release_digest: Option<Digest>,
    /// The worst component's health in the last heartbeat.
    pub health: Option<Health>,
    pub reachability: Reachability,
}

pub(crate) fn fleet_row(
    deployment: &Deployment,
    last: Option<(UtcNanos, Heartbeat)>,
    now: UtcNanos,
) -> FleetRow {
    let reachability = match &last {
        None => Reachability::NeverSeen,
        Some((at, _)) => {
            let silent = now.secs().checked_sub(at.secs()).unwrap_or(i64::MAX);
            if silent > UNREACHABLE_AFTER_SECS {
                Reachability::Unreachable
            } else {
                Reachability::Reachable
            }
        }
    };
    FleetRow {
        deployment_id: deployment.deployment_id.clone(),
        enrolled: deployment.public_key.is_some(),
        last_heartbeat: last.as_ref().map(|(at, _)| *at),
        release_digest: last.as_ref().map(|(_, h)| h.release_digest),
        health: last
            .as_ref()
            .and_then(|(_, h)| h.components.iter().map(|c| c.health).max()),
        reachability,
    }
}
