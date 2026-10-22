//! The provisioner with its ports: the control stream, the vault session, and the operator alerts.

use std::time::Duration;

use mandate_deploy_manager::{Alerts, ControlJournal, Identity, OperatorAlert};
use mandate_time::UtcNanos;
use mandate_vault::provisioner::RoleProvisioner;

/// How long the vault has to make one connect's roles before the request is refused as
/// `vault_unavailable` (coordinator, 2026-10-22). The connect then ends by its own deadline, and
/// the owner connects again.
pub const DEADLINE: Duration = Duration::from_secs(30);

/// How many times one record is drafted again when another writer moved the stream under it.
pub const APPEND_TRIES: usize = 3;

/// The operator alert a refused connect raises: a fixed code and its runbook, never an id
/// (`AGENTS.md` rule 6; design §8.2).
pub const ROLES_REFUSED: OperatorAlert = OperatorAlert {
    code: "connection_roles_refused",
    runbook: "RB-12",
};

/// Why a pass stopped early. A step the vault performed and the journal did not record is planned
/// again by the next pass, which performs it again with nothing changed and records it once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// The journal could not be read or written, or kept moving under the record.
    Journal,
    /// The journal refused a record as invalid: a defect, never retried blind.
    Refused { event_type: &'static str },
    /// The control stream has no `StreamOpened` yet, so no record can name its environment.
    NotOpened,
    /// The report of a pass E21-33 has not implemented yet (DEC-77); the implementation removes
    /// it.
    Unimplemented { story: &'static str },
}

/// The provisioner with its ports, for the workspace `namespace` its roles bind.
pub struct Provisioner<J, V, A> {
    identity: Identity,
    namespace: String,
    journal: J,
    vault: V,
    alerts: A,
    deadline: Duration,
}

impl<J: ControlJournal, V: RoleProvisioner, A: Alerts> Provisioner<J, V, A> {
    pub fn new(identity: Identity, namespace: &str, journal: J, vault: V, alerts: A) -> Self {
        Self {
            identity,
            namespace: namespace.to_owned(),
            journal,
            vault,
            alerts,
            deadline: DEADLINE,
        }
    }

    /// The same provisioner with `deadline` for each step in place of [`DEADLINE`].
    #[must_use]
    pub fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = deadline;
        self
    }

    pub fn journal(&mut self) -> &mut J {
        &mut self.journal
    }

    pub fn vault(&mut self) -> &mut V {
        &mut self.vault
    }

    pub fn alerts(&mut self) -> &mut A {
        &mut self.alerts
    }

    /// One pass at `now`: every step the stream owes, each performed on the vault within the
    /// deadline and then journaled.
    ///
    /// # Errors
    /// [`Halt`], after which every record committed stays committed and the next pass picks up
    /// the rest.
    pub async fn tick(&mut self, now: UtcNanos) -> Result<(), Halt> {
        let _ = (now, &self.identity, &self.namespace, self.deadline);
        Err(Halt::Unimplemented { story: "E21-33" })
    }
}
