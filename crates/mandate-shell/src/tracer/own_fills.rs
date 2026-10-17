//! The own-fill reconciliation a run performs before its preflight judges the account
//! ([DEC-955](../../../../docs/project/decisions/DEC-955.md)): the account stream folded, the
//! broker asked only about the orders the stream owns, by their own ids, and about its fills, and
//! the fill records that word calls for appended. Read-only at the broker and journal-only: no
//! request here can place, cancel or replace an order.

use mandate_executor::{
    ActivityCursor, BrokerFill, BrokerOrder, BrokerOutcome, BrokerRequest, ClientOrderId, Effect,
    EventDraft, FoldedEvent, RiskClock, WriterEpoch,
};
use mandate_journal::{AppendOutcome, StoredEvent};

use crate::envelope::{DraftFields, Envelope, Writer, account_stream, draft_bytes};
use crate::error::{Cause, ShellError, refused};
use crate::stages::{Connector, Executor, Stage};

use super::{ProductionCycle, body_of};

/// What one own-fill pass booked.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Booked {
    /// The fill records appended, `FillApplied` and `LateFillApplied` together.
    pub fills: usize,
}

/// The most pages of account activities one pass reads before it stops: a broker whose
/// activities never end can never hold the run in a loop (rule 3).
const ACTIVITY_PAGES: usize = 20;

impl ProductionCycle {
    /// Books the deployment's own unbooked fills (DEC-955 items 1 to 5), before anything judges
    /// the account. The account stream is read and folded; when it owns no order with fills it
    /// may not have booked, nothing is read from the broker, owned or written. Otherwise the
    /// stream's ownership is taken, each such order is read back by its own client order id and
    /// the account's fills are read on from the stream's last checkpoint (GETs only), and the
    /// fill records the executor drafts from that word are appended at the head that was read,
    /// all at once or not at all.
    ///
    /// # Errors
    /// Every [`ShellError`] is a stop after which nothing further is read, journaled or sent: a
    /// journal that cannot be read, owned or appended to, a broker read that does not answer as
    /// asked, and the executor's own-fill mismatch, which names the order's opaque id and a
    /// reason code only.
    pub fn book_own_fills(&mut self) -> Result<Booked, ShellError> {
        let stream = account_stream(&self.setup.deployment.workspace.0, &self.setup.account_ref);
        let executor = &mut *self.stages.executor;
        executor.reset().map_err(refused(Stage::Executor))?;
        let rows = self
            .stages
            .journal
            .read(&stream)
            .map_err(refused(Stage::Journal))?;
        fold_rows(executor, &stream, &rows)?;
        let (owed, since) = executor.owed().map_err(refused(Stage::Reconcile))?;
        if owed.is_empty() {
            return Ok(Booked::default());
        }
        let epoch = self
            .stages
            .journal
            .take_ownership(&stream)
            .map_err(refused(Stage::Journal))?;
        let connector = &mut *self.stages.connector;
        let orders = read_orders(connector, owed)?;
        let fills = read_fills(connector, since)?;
        let effects = self
            .stages
            .executor
            .book_own(
                WriterEpoch(epoch),
                RiskClock::from_secs(self.setup.now.secs()),
                &orders,
                &fills,
            )
            .map_err(refused(Stage::Reconcile))?;
        let drafts = journal_only(effects)?;
        if drafts.is_empty() {
            return Ok(Booked::default());
        }
        let bytes = drafts
            .iter()
            .map(|draft| {
                draft_bytes(
                    &Envelope {
                        stream: &stream,
                        writer: Writer::Executor,
                        actor_id: "executor",
                        event_time: self.setup.now,
                    },
                    &DraftFields {
                        event_id: &draft.event_id.0,
                        event_type: &draft.event_type,
                        schema_version: draft.schema_version,
                        causation_id: draft.causation_id.as_ref().map(|id| id.0.as_str()),
                        config_refs: &draft.config_refs,
                        payload: &draft.payload,
                    },
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        let head = rows.last().map_or(0, |row| row.seq);
        match self.stages.journal.append(&stream, head, epoch, &bytes) {
            AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_) => {}
            refusal @ (AppendOutcome::HeadMismatch { .. }
            | AppendOutcome::IdempotencyConflict { .. }
            | AppendOutcome::Fenced { .. }
            | AppendOutcome::Invalid { .. }
            | AppendOutcome::Unavailable
            | AppendOutcome::Ambiguous) => {
                return Err(refused(Stage::Journal)(Cause::Append {
                    outcome: refusal.name(),
                }));
            }
        }
        Ok(Booked {
            fills: drafts
                .iter()
                .filter(|draft| draft.event_type.ends_with("FillApplied"))
                .count(),
        })
    }
}

/// Folds every committed row of the account stream into the executor, in `seq` order.
fn fold_rows(
    executor: &mut dyn Executor,
    stream: &str,
    rows: &[StoredEvent],
) -> Result<(), ShellError> {
    for row in rows {
        let (payload, causation) = body_of(row)?;
        executor
            .committed(&FoldedEvent {
                stream: stream.to_owned(),
                seq: mandate_executor::Seq(row.seq),
                event_id: mandate_executor::EventId(row.event_id.clone()),
                event_type: row.event_type.clone(),
                causation_id: causation.map(mandate_executor::EventId),
                payload,
            })
            .map_err(refused(Stage::Executor))?;
    }
    Ok(())
}

/// The broker's word on each owed order, read back by its own client order id. An order the
/// broker does not have filled nothing, so it owes nothing; any other answer stops the pass.
fn read_orders(
    connector: &mut dyn Connector,
    owed: Vec<ClientOrderId>,
) -> Result<Vec<BrokerOrder>, ShellError> {
    let mut orders = Vec::new();
    for id in owed {
        match connector.call(&BrokerRequest::GetOrderByClientId(id)) {
            Ok(BrokerOutcome::Order(order)) => orders.push(order),
            Ok(BrokerOutcome::Absent { .. }) => {}
            Ok(_) | Err(_) => {
                return Err(refused(Stage::Reconcile)(Cause::Absent {
                    what: "the broker's answer for an owned order",
                }));
            }
        }
    }
    Ok(orders)
}

/// The account's fills from `since` on, page by page, until a page is empty or the cursor stops
/// moving; at most [`ACTIVITY_PAGES`] pages.
fn read_fills(
    connector: &mut dyn Connector,
    mut since: ActivityCursor,
) -> Result<Vec<BrokerFill>, ShellError> {
    let mut fills = Vec::new();
    for _ in 0..ACTIVITY_PAGES {
        let (page, cursor) = match connector.call(&BrokerRequest::ListActivities {
            since: since.clone(),
        }) {
            Ok(BrokerOutcome::Activities { fills, cursor }) => (fills, cursor),
            Ok(_) | Err(_) => {
                return Err(refused(Stage::Reconcile)(Cause::Absent {
                    what: "the broker's account activities",
                }));
            }
        };
        let ended = page.is_empty() || cursor == since;
        fills.extend(page);
        if ended {
            return Ok(fills);
        }
        since = cursor;
    }
    Err(refused(Stage::Reconcile)(Cause::Absent {
        what: "account activities that end",
    }))
}

/// The journal drafts of `effects`, which must be nothing else: the pass sends nothing.
fn journal_only(effects: Vec<Effect>) -> Result<Vec<EventDraft>, ShellError> {
    effects
        .into_iter()
        .map(|effect| match effect {
            Effect::Journal(draft) => Ok(draft),
            Effect::Broker(_) | Effect::Timer(_) | Effect::Notify(_) => {
                Err(refused(Stage::Reconcile)(Cause::Absent {
                    what: "an own-fill reconciliation that only journals",
                }))
            }
        })
        .collect()
}
