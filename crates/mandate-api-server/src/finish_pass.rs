//! The connect's finish pass ([DEC-2342](../../../docs/project/decisions/DEC-2342.md) items 3
//! and 10): each pass reads the workspace's control stream, finishes every open request
//! [`crate::finish::decide`] answers, appending at the head it read, and tears down every connect a
//! refusal or a revoke closed through the [`Teardown`] port.

use std::collections::BTreeSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mandate_connections::record::ConnectionId;
use mandate_time::UtcNanos;

use crate::finish::Fingerprints;
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
#[expect(
    dead_code,
    reason = "the pass that reads them lands after its tests (DEC-77)"
)]
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
    #[expect(
        clippy::todo,
        reason = "the tests land before the implementation (DEC-77); the stub is todo!(), the form DEC-137 names"
    )]
    pub async fn pass(&mut self, _now: UtcNanos) -> Result<(), JournalUnavailable> {
        todo!()
    }
}
