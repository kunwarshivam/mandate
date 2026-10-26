//! The connect's finish pass ([DEC-2342](../../../docs/project/decisions/DEC-2342.md) items 3
//! and 10): each pass reads the workspace's control stream, finishes every open request
//! [`crate::finish::decide`] answers, appending at the head it read, and tears down every connect a
//! refusal or a revoke closed through the [`Teardown`] port.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::io::{self, Write};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::Poll;
use std::time::Duration;

use mandate_canon::{Value, parse};
use mandate_connections::manager::{FinishPlan, ManagerEffect, RequestFold};
use mandate_connections::record::{AccountRef, ConnectionId};
use mandate_journal::{AppendOutcome, StoredEvent};
use mandate_time::UtcNanos;

use crate::clock::Clock;
use crate::finish::{
    CONNECT_DEADLINE_SECS, Fingerprints, Pass, Unreadable, checked, decide, request,
};
use crate::finish_records::drafted;
use crate::journal::{Journal, JournalUnavailable};

/// How long the pass waits between passes while a connect is bound (DEC-2342 item 10).
pub const PASS_SECS: u64 = 5;

/// How long a stopped finish lets the pass in flight run, well inside a pod's 30-second grace.
pub const DRAIN: Duration = Duration::from_secs(5);

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

/// The opaque operator alert a stalled teardown raises (DEC-2348 item 1).
pub const TEARDOWN_STALLED: &str = "connect_teardown_stalled";

/// The runbook [`TEARDOWN_STALLED`] names.
pub const TEARDOWN_RUNBOOK: &str = "docs/runbooks/RB-19-connect-teardown.md";

/// Where the pass raises an operator alert: a code and a runbook, and nothing else (rule 6).
pub trait Alerts: Send + Sync {
    fn raise(&self, code: &'static str, runbook: &'static str);
}

/// Alerts written as one line each, `operator_alert <code> <runbook>`, as the deployment manager
/// writes its own (DEC-2348 item 3); the process gives it stderr.
pub struct LineAlerts<W>(pub Mutex<W>);

impl<W: Write + Send> Alerts for LineAlerts<W> {
    fn raise(&self, code: &'static str, runbook: &'static str) {
        if let Ok(mut writer) = self.0.lock() {
            let _unwritten_alert_is_not_retried =
                writeln!(writer, "operator_alert {code} {runbook}");
        }
    }
}

/// Runs a pass at the clock's time every `every`, for as long as the process runs; a clock that
/// cannot be read skips that pass, and a pass that cannot read the journal waits for the next.
pub async fn passes<F, T, A>(finisher: Finisher<F, T, A>, clock: Arc<dyn Clock>, every: Duration)
where
    F: Fingerprints,
    T: Teardown,
    A: Alerts,
{
    passes_until(finisher, clock, every, std::future::pending()).await;
}

/// [`passes`] until `stop` resolves (DEC-2348 item 3): the wait between passes ends at once, and a
/// pass in flight gets [`DRAIN`] to end. One still running then is abandoned, which is safe, since
/// every pass reads the control stream again, appends at the head it read, and deletes and releases
/// idempotently: the next start's pass does what it left.
pub async fn passes_until<F, T, A>(
    mut finisher: Finisher<F, T, A>,
    clock: Arc<dyn Clock>,
    every: Duration,
    stop: impl Future<Output = ()> + Send,
) where
    F: Fingerprints,
    T: Teardown,
    A: Alerts,
{
    let mut stop = std::pin::pin!(stop);
    loop {
        if let Some(now) = clock.now() {
            let mut pass = std::pin::pin!(finisher.pass(now));
            let stopped = std::future::poll_fn(|cx| {
                if pass.as_mut().poll(cx).is_ready() {
                    Poll::Ready(false)
                } else {
                    stop.as_mut().poll(cx).map(|()| true)
                }
            })
            .await;
            if stopped {
                let _abandoned_past_the_drain = tokio::time::timeout(DRAIN, pass).await;
                return;
            }
        }
        if tokio::time::timeout(every, stop.as_mut()).await.is_ok() {
            return;
        }
    }
}

/// [`passes_until`] on `runtime`, the stop made by `stop` inside it, then `runtime` shut down
/// without waiting (DEC-2348 item 3): a vault call a drained pass abandoned runs on a blocking
/// thread the runtime would otherwise wait for, so this returns within [`DRAIN`] of the stop
/// whatever the vault does.
///
/// # Errors
/// `stop`'s failure to make the stop; no pass runs then.
pub fn run_until_stopped<F, T, A, S>(
    runtime: tokio::runtime::Runtime,
    finisher: Finisher<F, T, A>,
    clock: Arc<dyn Clock>,
    every: Duration,
    stop: impl FnOnce() -> io::Result<S>,
) -> io::Result<()>
where
    F: Fingerprints,
    T: Teardown,
    A: Alerts,
    S: Future<Output = ()> + Send,
{
    let ran = runtime.block_on(async {
        let stop = stop()?;
        passes_until(finisher, clock, every, stop).await;
        io::Result::Ok(())
    });
    runtime.shutdown_background();
    ran
}

/// The pass over one workspace's connects, and what it remembers between passes.
pub struct Finisher<F, T, A> {
    journal: Arc<dyn Journal>,
    fingerprints: F,
    teardown: T,
    alerts: A,
    workspace: String,
    /// The closed requests, by connection id and `account_ref`, whose deletes succeeded since the
    /// process started: DEC-699 lets a refused connection id be requested again.
    cleared: BTreeSet<(ConnectionId, AccountRef)>,
    /// The requests [`TEARDOWN_STALLED`] was raised for since the process started.
    alerted: BTreeSet<(ConnectionId, AccountRef)>,
    /// Whether no pass has read the control stream since the process started.
    restarted: bool,
}

impl<F: Fingerprints, T: Teardown, A: Alerts> Finisher<F, T, A> {
    /// A finisher whose first pass is the restart pass.
    pub fn new(
        journal: Arc<dyn Journal>,
        fingerprints: F,
        teardown: T,
        alerts: A,
        workspace: String,
    ) -> Self {
        Self {
            journal,
            fingerprints,
            teardown,
            alerts,
            workspace,
            cleared: BTreeSet::new(),
            alerted: BTreeSet::new(),
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
        for (connection, account_ref, deadline) in closed(&control) {
            self.clear(connection, account_ref, deadline, now).await;
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
            }) = decide(&request, checked, &self.fingerprints, now, restarted).await
            else {
                continue;
            };
            for effect in &effects {
                let refusal = matches!(effect, ManagerEffect::ConnectRefused { .. });
                if refusal && self.teardown.delete(&connection).await.is_err() {
                    self.stalled(
                        (connection.clone(), members.account_ref.clone()),
                        deadline(request.opened_at),
                        now,
                    );
                    continue;
                }
                let Some(draft) = drafted(effect, causation.as_deref(), &self.workspace, now)
                else {
                    continue;
                };
                let Ok(epoch) = self.journal.take_ownership(&stream).await else {
                    continue;
                };
                let appended = self
                    .journal
                    .append(&stream, head, epoch, now, &[&draft])
                    .await;
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
                    self.clear(
                        connection.clone(),
                        members.account_ref.clone(),
                        deadline(request.opened_at),
                        now,
                    )
                    .await;
                }
            }
        }
        Ok(())
    }

    /// Deletes the entries of `connection`'s request on `account_ref` unless it was cleared since
    /// the process started, then releases its fingerprint; it is cleared once both succeed, and a
    /// delete or release that fails at `now` past `deadline` is [`Self::stalled`].
    async fn clear(
        &mut self,
        connection: ConnectionId,
        account_ref: AccountRef,
        deadline: i64,
        now: UtcNanos,
    ) {
        let closed = (connection, account_ref);
        if self.cleared.contains(&closed) {
            return;
        }
        if self.teardown.delete(&closed.0).await.is_ok()
            && self
                .fingerprints
                .release(&closed.0, &closed.1)
                .await
                .is_ok()
        {
            self.cleared.insert(closed);
        } else {
            self.stalled(closed, deadline, now);
        }
    }

    /// Raises [`TEARDOWN_STALLED`] for `request`, once per process, when its delete failed at
    /// `now`, at or past its `deadline` (DEC-2348 item 1).
    fn stalled(&mut self, request: (ConnectionId, AccountRef), deadline: i64, now: UtcNanos) {
        if now.secs() >= deadline && self.alerted.insert(request) {
            self.alerts.raise(TEARDOWN_STALLED, TEARDOWN_RUNBOOK);
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

/// The second at which a request recorded at `opened_at` reaches its deadline; a time that cannot
/// be read is past it.
fn deadline(opened_at: Option<UtcNanos>) -> i64 {
    opened_at.map_or(i64::MIN, |at| {
        at.secs().saturating_add(CONNECT_DEADLINE_SECS)
    })
}

/// The version-2 `oauth` requests on `control` that a refusal or a revoke closed while open, each
/// by its connection id, its `account_ref` and its [`deadline`]: the connects whose code and token
/// entries the teardown deletes.
fn closed(control: &[StoredEvent]) -> Vec<(ConnectionId, AccountRef, i64)> {
    let mut open = BTreeMap::new();
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
                if let Some(account_ref) = payload
                    .get("account_ref")
                    .and_then(Value::as_str)
                    .and_then(|text| AccountRef::new(text).ok())
                {
                    let opened_at = UtcNanos::parse_rfc3339(&row.recorded_at).ok();
                    open.insert(id, (account_ref, deadline(opened_at)));
                }
            }
            "ConnectionEstablished" => {
                open.remove(&id);
            }
            "ConnectionRefused" | "ConnectionRevoked" => {
                if let Some((account_ref, deadline)) = open.remove(&id) {
                    closed.push((id, account_ref, deadline));
                }
            }
            _ => {}
        }
    }
    closed
}
