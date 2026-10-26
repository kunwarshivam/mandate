//! The loop's one pass: read the control stream, read the orchestrator, and for each decision
//! journal its record before its effect (journal spec §5.2, §9.21).

use std::future::Future;

use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_time::UtcNanos;

use crate::control::fact;
use crate::manager::{Decision, Manager};
use crate::orchestrator::{Alerts, Effect, Launch, Orchestrator, Unavailable};

pub use crate::record::Identity;

/// The workspace control stream, as the manager reads and appends to it.
pub trait ControlJournal {
    /// Every row of the control stream after `seq`, in order.
    fn rows_after(
        &mut self,
        seq: u64,
    ) -> impl Future<Output = Result<Vec<StoredEvent>, Unavailable>>;
    /// Appends one draft at the stream's head under a writer epoch it takes for this append, as
    /// workspace control services share the stream (journal spec §5.1).
    fn append(
        &mut self,
        draft: &[u8],
        recorded_at: UtcNanos,
    ) -> impl Future<Output = Result<AppendOutcome, Unavailable>>;
}

/// Why a pass stopped early. Nothing unjournaled was done; the next pass starts again from the
/// journal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Halt {
    /// The journal could not be read or written, or another writer moved the stream: re-read.
    Journal,
    /// The orchestrator could not list its instances: nothing was decided.
    Orchestrator,
    /// The journal refused a record as invalid: a defect, never retried blind.
    Refused { event_type: &'static str },
    /// The control stream has no `StreamOpened` yet, so no record can name its environment.
    NotOpened,
}

/// The manager with its ports.
pub struct Deployer<J, O, A> {
    pub manager: Manager,
    identity: Identity,
    journal: J,
    orchestrator: O,
    alerts: A,
    read_through: u64,
}

impl<J: ControlJournal, O: Orchestrator, A: Alerts> Deployer<J, O, A> {
    pub fn new(identity: Identity, journal: J, orchestrator: O, alerts: A) -> Self {
        Self {
            manager: Manager::new(),
            identity,
            journal,
            orchestrator,
            alerts,
            read_through: 0,
        }
    }

    pub fn orchestrator(&mut self) -> &mut O {
        &mut self.orchestrator
    }

    pub fn journal(&mut self) -> &mut J {
        &mut self.journal
    }

    pub fn alerts(&mut self) -> &mut A {
        &mut self.alerts
    }

    /// The ports back, as a manager that stops hands them to its successor.
    pub fn into_ports(self) -> (J, O, A) {
        (self.journal, self.orchestrator, self.alerts)
    }

    /// One pass at `now`. A record that does not commit stops the pass before its effect; an
    /// effect that fails is retried by a later pass without a second record.
    ///
    /// # Errors
    /// [`Halt`], after which nothing unjournaled has been done.
    pub async fn tick(&mut self, now: UtcNanos) -> Result<(), Halt> {
        self.refresh().await?;
        let environment = self
            .manager
            .environment()
            .ok_or(Halt::NotOpened)?
            .to_owned();
        let observed = self
            .orchestrator
            .observe()
            .await
            .map_err(|Unavailable| Halt::Orchestrator)?;
        for decision in self.manager.decide(&observed, now) {
            let effect = match decision {
                Decision::Retry(effect) => effect,
                Decision::Journal(record) => {
                    let draft = record.draft(&self.identity, &environment, now);
                    let outcome = self
                        .journal
                        .append(&draft, now)
                        .await
                        .map_err(|Unavailable| Halt::Journal)?;
                    match outcome {
                        AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => {}
                        AppendOutcome::Invalid { .. } => {
                            return Err(Halt::Refused {
                                event_type: record.event_type(),
                            });
                        }
                        _ => return Err(Halt::Journal),
                    }
                    self.manager.fold(&record.fact(now));
                    if let Effect::Start { slot, generation } = record.effect() {
                        self.manager.issued(&slot, generation);
                        self.refresh().await?;
                        if !self.manager.wanted(&slot) {
                            continue;
                        }
                    }
                    record.effect()
                }
            };
            self.perform(effect, now).await;
        }
        Ok(())
    }

    /// Folds every control-stream row not yet folded, in order, from the journal alone.
    async fn refresh(&mut self) -> Result<(), Halt> {
        let rows = self
            .journal
            .rows_after(self.read_through)
            .await
            .map_err(|Unavailable| Halt::Journal)?;
        for row in &rows {
            if let Some(fact) = fact(row) {
                self.manager.fold(&fact);
            }
            self.read_through = self.read_through.max(row.seq);
        }
        Ok(())
    }

    async fn perform(&mut self, effect: Effect, now: UtcNanos) {
        match effect {
            Effect::Start { slot, generation } => {
                let launch = Launch {
                    lifetime: self.manager.lifetime(&slot, now),
                    expires_at: self.manager.expires_at(&slot),
                    connection: self.manager.connection(&slot, generation),
                };
                if self
                    .orchestrator
                    .start(&slot, generation, &launch)
                    .await
                    .is_ok()
                {
                    self.manager.launched(&slot, generation);
                }
            }
            Effect::Stop { slot, generation } => {
                let connection = self.manager.connection(&slot, generation);
                if self
                    .orchestrator
                    .stop(&slot, generation, connection.as_deref())
                    .await
                    .is_ok()
                {
                    self.manager.stopped(&slot, generation);
                }
            }
            Effect::Alert(alert) => self.alerts.raise(alert),
        }
    }
}
