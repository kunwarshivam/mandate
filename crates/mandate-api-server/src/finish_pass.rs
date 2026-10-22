//! The connect's finish pass ([DEC-2342](../../../docs/project/decisions/DEC-2342.md) items 3
//! and 10): each pass reads the workspace's control stream, finishes every open request
//! [`crate::finish::decide`] answers, appending at the head it read, and tears down every connect a
//! refusal or a revoke closed through the [`Teardown`] port.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mandate_canon::{Value, parse};
use mandate_connections::manager::{FinishPlan, ManagerEffect, RequestFold};
use mandate_connections::record::ConnectionId;
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_time::UtcNanos;

use crate::finish::{Fingerprints, Pass, Unreadable, checked, decide, request};
use crate::finish_records::drafted;
use crate::journal::{Journal, JournalUnavailable};

/// How long the pass waits between passes while a connect is bound (DEC-2342 item 10).
pub const PASS_SECS: u64 = 5;

/// A delete that did not reach the vault, or that the vault refused; the next pass retries it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeleteFailed;

/// One delete's answer.
pub type Deleted<'a> = Pin<Box<dyn Future<Output = Result<(), DeleteFailed>> + Send + 'a>>;

/// The teardown's vault port: deletes a connection's code and token entries as the API's vault
/// identity, which may delete those and nothing else. An absent entry is already deleted.
pub trait Teardown: Send + Sync {
    fn delete<'a>(&'a self, connection: &'a ConnectionId) -> Deleted<'a>;
}

/// The pass over one workspace's connects, and what it remembers between passes.
pub struct Finisher<F, T> {
    journal: Arc<dyn Journal>,
    fingerprints: F,
    teardown: T,
    workspace: String,
    /// The closed connects whose deletes succeeded since the process started.
    cleared: BTreeSet<ConnectionId>,
    /// Whether no pass has read the control stream since the process started.
    restarted: bool,
}

impl<F: Fingerprints, T: Teardown> Finisher<F, T> {
    /// A finisher whose first pass is the restart pass.
    pub fn new(journal: Arc<dyn Journal>, fingerprints: F, teardown: T, workspace: String) -> Self {
        Self {
            journal,
            fingerprints,
            teardown,
            workspace,
            cleared: BTreeSet::new(),
            restarted: true,
        }
    }

    /// One pass at `now`: the deletes still owed, then each open request's finish appended at the
    /// head the pass read, each refusal's deletes before and after it.
    ///
    /// # Errors
    /// [`JournalUnavailable`] when the control stream cannot be read; the restart pass is still to
    /// come.
    pub async fn pass(&mut self, now: UtcNanos) -> Result<(), JournalUnavailable> {
        let stream = format!("ctl:{}", self.workspace);
        let control = self.journal.rows(&stream).await?;
        let restarted = std::mem::replace(&mut self.restarted, false);
        for connection in closed(&control) {
            self.clear(connection).await;
        }
        let mut head = control.last().map_or(0, |row| row.seq);
        for connection in requested(&control) {
            let request = request(&control, &connection);
            let RequestFold::Open(members) = &request.fold else {
                continue;
            };
            let account = format!("acct:{}:{}", self.workspace, members.account_ref.as_str());
            let checked = match self.journal.rows(&account).await {
                Ok(rows) => checked(&rows, &connection),
                Err(JournalUnavailable) => Err(Unreadable),
            };
            let Ok(Pass::Finish {
                plan: FinishPlan::Commit(effects),
                causation,
            }) = decide(&request, checked, &self.fingerprints, now, restarted)
            else {
                continue;
            };
            for effect in &effects {
                let refusal = matches!(effect, ManagerEffect::ConnectRefused { .. });
                if refusal {
                    let _owed_until_a_later_delete = self.teardown.delete(&connection).await;
                }
                let Some(draft) = drafted(effect, causation.as_deref(), &self.workspace, now)
                else {
                    continue;
                };
                let Ok(epoch) = self.journal.take_ownership(&stream).await else {
                    continue;
                };
                let appended = self.journal.append(&stream, head, epoch, now, &draft).await;
                if let Ok(AppendOutcome::Committed(rows)) = &appended
                    && let Some(last) = rows.last()
                {
                    head = last.seq;
                }
                if refusal
                    && matches!(
                        appended,
                        Ok(AppendOutcome::Committed(_) | AppendOutcome::AlreadyCommitted(_))
                    )
                {
                    self.clear(connection.clone()).await;
                }
            }
        }
        Ok(())
    }

    /// Deletes `connection`'s entries unless they were deleted since the process started.
    async fn clear(&mut self, connection: ConnectionId) {
        if !self.cleared.contains(&connection) && self.teardown.delete(&connection).await.is_ok() {
            self.cleared.insert(connection);
        }
    }
}

/// The connection a control row names, and the row's payload.
fn named(row: &StoredEvent) -> Option<(ConnectionId, Value)> {
    let payload = parse(&row.body).ok()?.get("payload").cloned()?;
    let id = ConnectionId::new(payload.get("connection_id")?.as_str()?).ok()?;
    Some((id, payload))
}

/// Each connection a `ConnectionRequested` on `control` names, once, in stream order.
fn requested(control: &[StoredEvent]) -> Vec<ConnectionId> {
    let mut seen = BTreeSet::new();
    control
        .iter()
        .filter(|row| row.event_type == "ConnectionRequested")
        .filter_map(named)
        .map(|(id, _)| id)
        .filter(|id| seen.insert(id.clone()))
        .collect()
}

/// The version-2 `oauth` requests on `control` that a refusal or a revoke closed while open: the
/// connects whose code and token entries the teardown deletes.
fn closed(control: &[StoredEvent]) -> Vec<ConnectionId> {
    let mut open = BTreeSet::new();
    let mut closed = Vec::new();
    for row in control {
        let Some((id, payload)) = named(row) else {
            continue;
        };
        match row.event_type.as_str() {
            "ConnectionRequested"
                if row.schema_version == 2
                    && payload.get("credential").and_then(Value::as_str) == Some("oauth") =>
            {
                open.insert(id);
            }
            "ConnectionEstablished" => {
                open.remove(&id);
            }
            "ConnectionRefused" | "ConnectionRevoked" if open.remove(&id) => closed.push(id),
            _ => {}
        }
    }
    closed
}
