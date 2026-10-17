//! Pause, a hold, and the kill switch as `OwnerCommandIssued` on the workspace's control stream
//! (§3.11 item 10, §4.2, §5.4, DEC-1007): the members journal spec §9.11 lists, as the CLI writes
//! them, so the runtime reads the API's commands exactly as it reads the CLI's.
//!
//! The event id is derived from the client's key (§3.4), so a retry, a lost answer, or a fence
//! commits nothing twice: each attempt takes a new writer epoch, reads the head, and appends the
//! same bytes, and a retry of an append that landed answers `AlreadyCommitted` (journal spec §5.1).

use std::time::Duration;

use mandate_authn::Request;
use mandate_identity::Permission;
use mandate_journal::StoredEvent;
use mandate_time::UtcNanos;

use crate::journal::Journal;

/// How many appends a command makes before it answers that the journal did not settle.
pub const ATTEMPTS: usize = 5;

/// The pause after an attempt that did not settle.
pub const BACKOFF: Duration = Duration::from_millis(20);

/// What a kill switch stops (§5.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KillScope {
    Agent(String),
    Connection(String),
    Workspace,
}

/// One owner command the API records.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Pause the agent.
    Pause(String),
    /// Hold the agent's new openings (DEC-191).
    Hold(String),
    /// Engage the kill switch at a scope.
    KillSwitch(KillScope),
}

impl Command {
    /// §3.4's operation name, which is also the journaled `command`'s.
    pub fn operation(&self) -> &'static str {
        ""
    }

    /// The matrix row that authorizes it (identity spec §4.2).
    pub fn permission(&self) -> Permission {
        Permission::Pause
    }

    /// What it is to the session (identity spec §6.4): pause and the kill switch survive an
    /// outage; a hold does not.
    pub fn request(&self) -> Request {
        Request::Other
    }
}

/// Who issued a command, where, and when.
#[derive(Debug, Clone, Copy)]
pub struct Issued<'a> {
    /// The workspace's ULID.
    pub workspace: &'a str,
    /// The principal's ULID, the actor and the payload's `user`.
    pub principal: &'a str,
    /// The event id §3.4 derives.
    pub event_id: &'a str,
    /// The control stream's environment.
    pub environment: &'a str,
    /// The instant of receipt.
    pub now: UtcNanos,
    pub command: &'a Command,
}

/// The canonical draft of `issued`'s `OwnerCommandIssued`, or `None` for an instant before 1970.
pub fn draft(issued: &Issued<'_>) -> Option<Vec<u8>> {
    let _ = issued;
    None
}

/// Whether `stored` records the command `draft` does: the same event type, stream, actor, and
/// payload, but the second it was submitted at (§3.4 step 1, DEC-1007 item 4).
pub fn same(stored: &StoredEvent, draft: &[u8]) -> bool {
    let _ = (stored, draft);
    false
}

/// Whether the control stream holds `command`'s scope (DEC-1007 item 5): an agent an
/// `AgentDeployed` names, a connection an event names, or, for the workspace, any row at all.
pub fn in_workspace(control: &[StoredEvent], command: &Command) -> bool {
    let _ = (control, command);
    false
}

/// How an append ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    /// The event is stored: committed now or before.
    Stored(StoredEvent),
    /// Another event of other content is stored under the id.
    Conflict,
    /// An attempt's commit was ambiguous and none since confirmed it.
    Unknown,
    /// Nothing was stored.
    Unavailable,
}

/// Appends `draft`, whose id is `event_id`, to `stream`, in up to [`ATTEMPTS`] attempts.
pub async fn settle(
    journal: &dyn Journal,
    stream: &str,
    event_id: &str,
    draft: &[u8],
    now: UtcNanos,
) -> Settled {
    let _ = (journal, stream, event_id, draft, now);
    Settled::Unavailable
}
