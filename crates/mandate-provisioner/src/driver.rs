//! The provisioner with its ports: the control stream, the vault session, and the operator alerts.

use std::time::Duration;

use mandate_connections::record::ConnectionId;
use mandate_deploy_manager::{Alerts, ControlJournal, Identity, OperatorAlert};
use mandate_journal::AppendOutcome;
use mandate_time::UtcNanos;
use mandate_vault::provisioner::{RoleProvisioner, RoleRefusal};

use crate::plan::{Step, plan};
use crate::record::{Outcome, draft};

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
    /// The vault session is at or near its token's end: no further step is performed with it.
    SessionExpired,
}

/// The provisioner with its ports, for the workspace `namespace` its roles bind.
pub struct Provisioner<J, V, A> {
    identity: Identity,
    namespace: String,
    journal: J,
    vault: V,
    alerts: A,
    deadline: Duration,
    alerted: Vec<Step>,
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
            alerted: Vec::new(),
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
    /// deadline and then journaled. A pass takes at most the steps first owed and
    /// [`APPEND_TRIES`] more, so a stream that never shows a record committed ends it.
    ///
    /// # Errors
    /// [`Halt`], after which every record committed stays committed and the next pass picks up
    /// the rest.
    pub async fn tick(&mut self, now: UtcNanos) -> Result<(), Halt> {
        self.tick_while(now, || true).await
    }

    /// [`Provisioner::tick`], asking `usable` before each step whether the vault session may
    /// still be used: once it may not, the pass stops before the vault is called again.
    ///
    /// # Errors
    /// [`Halt`]; [`Halt::SessionExpired`] when `usable` answers no.
    pub async fn tick_while(
        &mut self,
        now: UtcNanos,
        usable: impl Fn() -> bool,
    ) -> Result<(), Halt> {
        let mut skipped: Vec<Step> = Vec::new();
        let mut moved: usize = 0;
        let mut bound: Option<usize> = None;
        loop {
            let rows = self
                .journal
                .rows_after(0)
                .await
                .map_err(|_| Halt::Journal)?;
            let Ok(owed) = plan(&rows);
            let environment = owed.environment.ok_or(Halt::NotOpened)?;
            let left = bound.get_or_insert(owed.steps.len().saturating_add(APPEND_TRIES));
            if *left == 0 {
                return Err(Halt::Journal);
            }
            *left = left.saturating_sub(1);
            let Some(step) = owed.steps.into_iter().find(|s| !skipped.contains(s)) else {
                return Ok(());
            };
            if !usable() {
                return Err(Halt::SessionExpired);
            }
            let Some(outcome) = self.perform(&step).await else {
                self.alert_once(&step);
                skipped.push(step);
                continue;
            };
            let record = draft(&self.identity, &step, outcome, &environment, now);
            match self
                .journal
                .append(&[&record], now)
                .await
                .map_err(|_| Halt::Journal)?
            {
                AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => {
                    if matches!(outcome, Outcome::Refused(_)) {
                        self.alert_once(&step);
                    }
                }
                AppendOutcome::HeadMismatch { .. }
                | AppendOutcome::IdempotencyConflict { .. }
                | AppendOutcome::Fenced { .. } => {
                    moved = moved.saturating_add(1);
                    if moved >= APPEND_TRIES {
                        return Err(Halt::Journal);
                    }
                }
                AppendOutcome::Invalid { .. } => {
                    return Err(Halt::Refused {
                        event_type: outcome.event_type(),
                    });
                }
                AppendOutcome::Unavailable | AppendOutcome::Ambiguous => return Err(Halt::Journal),
            }
        }
    }

    /// Raises [`ROLES_REFUSED`] for `step` the first time only: a removal the vault keeps
    /// refusing is tried again each pass, and alerts once in this process's life.
    fn alert_once(&mut self, step: &Step) {
        if !self.alerted.contains(step) {
            self.alerted.push(step.clone());
            self.alerts.raise(ROLES_REFUSED);
        }
    }

    /// `step` on the vault within the deadline: the roles made or refused, or the exchange's
    /// removed. A removal the vault does not do in time is `None`: there is no record of a
    /// removal refused, and the next pass removes it again.
    async fn perform(&self, step: &Step) -> Option<Outcome> {
        match step {
            Step::Make { connection, .. } => {
                let made = match ConnectionId::new(connection) {
                    Ok(id) => tokio::time::timeout(
                        self.deadline,
                        self.vault.make_roles(&id, &self.namespace),
                    )
                    .await
                    .unwrap_or(Err(RoleRefusal::VaultUnavailable)),
                    Err(_) => Err(RoleRefusal::RoleRefused),
                };
                Some(made.map_or_else(Outcome::Refused, |()| Outcome::Made))
            }
            Step::Remove { connection, .. } => {
                let id = ConnectionId::new(connection).ok()?;
                tokio::time::timeout(
                    self.deadline,
                    self.vault.remove_exchange_role(&id, &self.namespace),
                )
                .await
                .ok()?
                .ok()
                .map(|()| Outcome::Removed)
            }
        }
    }
}
