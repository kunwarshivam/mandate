//! The first read models (§4.3, §4.7, DEC-979), folded from journal rows alone (API-14).
//!
//! - An agent is one `AgentDeployed` names on the control stream. Its mode is the last
//!   `AgentModeChanged`'s `to` on its agent stream, `stopped` once `AgentStopped` names it, and
//!   `None` before either: not reported, never read as `normal` (`AGENTS.md` rule 3).
//! - An approval's state is folded from its agent stream's approval events, first terminal event
//!   winning, as the CLI's `approvals list` folds them: an admitted grant awaiting re-validation,
//!   and a `refused` or `counted` answer, leave it pending.
//!
//! Every function here is pure: rows in, values out.

use mandate_canon::Value;
use mandate_journal::StoredEvent;

/// How many resolved approvals the inbox lists, most recently requested first (DEC-979 item 5).
pub const RESOLVED_LISTED: usize = 100;

/// One agent, as `read-models/agent` states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentView {
    pub agent_id: String,
    pub environment: String,
    pub mandate_version: String,
    pub deployed_at: String,
    pub mode: Option<String>,
    pub pending_approvals: u64,
}

/// Where an approval stands (DEC-979 item 4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ApprovalState {
    Pending,
    Acted,
    Skipped,
    TimedOut,
    Canceled,
}

impl ApprovalState {
    /// The wire name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Acted => "acted",
            Self::Skipped => "skipped",
            Self::TimedOut => "timed_out",
            Self::Canceled => "canceled",
        }
    }
}

/// One approval, as `read-models/approval` states it; `content` is served only by the single read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApprovalView {
    pub approval_id: String,
    pub agent_id: String,
    pub state: ApprovalState,
    pub requested_at: String,
    pub deadline: i64,
    pub content_hash: String,
    pub content: Value,
}

/// Which approvals the inbox lists (`?state=`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    All,
    Pending,
    Resolved,
}

/// A row's envelope, parsed; `None` for a body that is not canonical JSON.
pub fn envelope(row: &StoredEvent) -> Option<Value> {
    let _ = row;
    None
}

/// The agents the control stream deploys, in deployment order, each once: `(agent_id,
/// environment, mandate_version, deployed_at, stopped)`.
pub fn deployed(control: &[StoredEvent]) -> Vec<(String, String, String, String, bool)> {
    let _ = control;
    Vec::new()
}

/// The agent's mode from its agent stream's rows: the last `AgentModeChanged`'s `to`.
pub fn mode(agent_rows: &[StoredEvent]) -> Option<String> {
    let _ = agent_rows;
    None
}

/// Every approval on `agent_id`'s stream, in request order, with its folded state.
pub fn approvals(agent_id: &str, agent_rows: &[StoredEvent]) -> Vec<ApprovalView> {
    let _ = (agent_id, agent_rows);
    Vec::new()
}

/// The inbox order: pending by deadline (then request order), then the [`RESOLVED_LISTED`] most
/// recently requested resolved ones, narrowed by `filter`.
pub fn inbox(approvals: Vec<ApprovalView>, filter: Filter) -> Vec<ApprovalView> {
    let _ = (approvals, filter);
    Vec::new()
}

/// One agent's view from its deployment and its stream's rows.
pub fn agent(
    deployment: &(String, String, String, String, bool),
    agent_rows: &[StoredEvent],
) -> AgentView {
    let _ = (deployment, agent_rows);
    AgentView {
        agent_id: String::new(),
        environment: String::new(),
        mandate_version: String::new(),
        deployed_at: String::new(),
        mode: None,
        pending_approvals: 0,
    }
}
