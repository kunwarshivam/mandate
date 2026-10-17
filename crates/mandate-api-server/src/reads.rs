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
    /// The order the approval binds (DEC-1021).
    pub order: Value,
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
    mandate_canon::parse(&row.body).ok()
}

/// The text at `path` (dot-separated) in `value`.
fn text<'v>(value: &'v Value, path: &str) -> Option<&'v str> {
    path.split('.')
        .try_fold(value, |v, key| v.get(key))
        .and_then(Value::as_str)
}

/// A deployed agent: `(agent_id, environment, mandate_version, deployed_at, stopped)`.
pub type Deployment = (String, String, String, String, bool);

/// The agents the control stream deploys, in first-deployment order, each once, with its latest
/// deployment; `stopped` once `AgentStopped` names it after that deployment.
pub fn deployed(control: &[StoredEvent]) -> Vec<Deployment> {
    let mut agents: Vec<Deployment> = Vec::new();
    for row in control {
        let Some(event) = envelope(row) else {
            continue;
        };
        match row.event_type.as_str() {
            "AgentDeployed" => {
                let (Some(agent), Some(version)) = (
                    text(&event, "payload.agent_id"),
                    text(&event, "payload.mandate_version"),
                ) else {
                    continue;
                };
                let deployment = (
                    agent.to_owned(),
                    row.environment.clone(),
                    version.to_owned(),
                    row.recorded_at.clone(),
                    false,
                );
                match agents.iter_mut().find(|known| known.0 == agent) {
                    Some(known) => *known = deployment,
                    None => agents.push(deployment),
                }
            }
            "AgentStopped" => {
                let agent = text(&event, "payload.agent_id");
                if let Some(known) = agents
                    .iter_mut()
                    .find(|known| Some(known.0.as_str()) == agent)
                {
                    known.4 = true;
                }
            }
            _ => {}
        }
    }
    agents
}

/// The agent's mode from its agent stream's rows: the last `AgentModeChanged`'s `to`.
pub fn mode(agent_rows: &[StoredEvent]) -> Option<String> {
    agent_rows
        .iter()
        .rev()
        .filter(|row| row.event_type == "AgentModeChanged")
        .find_map(|row| envelope(row).and_then(|e| text(&e, "payload.to").map(str::to_owned)))
}

/// Every approval on `agent_id`'s stream, in request order, with its folded state. A request
/// without a deadline, a content hash, or a content object is not served.
pub fn approvals(agent_id: &str, agent_rows: &[StoredEvent]) -> Vec<ApprovalView> {
    let mut views: Vec<ApprovalView> = Vec::new();
    for row in agent_rows {
        let Some(event) = envelope(row) else {
            continue;
        };
        if row.event_type == "ApprovalRequested" {
            let deadline = event
                .get("payload")
                .and_then(|p| p.get("deadline"))
                .and_then(Value::as_int)
                .and_then(|d| i64::try_from(d).ok());
            let hash = text(&event, "payload.content_hash");
            let content = event
                .get("payload")
                .and_then(|p| p.get("content"))
                .filter(|c| c.as_object().is_some());
            let bound = event.get("payload").and_then(order);
            if let (Some(deadline), Some(hash), Some(content), Some(bound)) =
                (deadline, hash, content, bound)
            {
                views.push(ApprovalView {
                    approval_id: row.event_id.clone(),
                    agent_id: agent_id.to_owned(),
                    state: ApprovalState::Pending,
                    requested_at: row.recorded_at.clone(),
                    deadline,
                    content_hash: hash.to_owned(),
                    content: content.clone(),
                    order: bound,
                });
            }
            continue;
        }
        let ended = match row.event_type.as_str() {
            "ApprovalTimedOut" => Some(ApprovalState::TimedOut),
            "ApprovalCanceled" => Some(ApprovalState::Canceled),
            "ApprovalRevalidated" => Some(match text(&event, "payload.result") {
                Some("act") => ApprovalState::Acted,
                _ => ApprovalState::Skipped,
            }),
            "ApprovalResponded"
                if text(&event, "payload.result") == Some("admitted")
                    && text(&event, "payload.verdict") == Some("skipped") =>
            {
                Some(ApprovalState::Skipped)
            }
            _ => None,
        };
        let approval = text(&event, "payload.approval");
        if let Some(state) = ended
            && let Some(view) = views.iter_mut().find(|view| {
                Some(view.approval_id.as_str()) == approval && view.state == ApprovalState::Pending
            })
        {
            view.state = state;
        }
    }
    views
}

/// The inbox order: pending by deadline (then request order), then the [`RESOLVED_LISTED`] most
/// recently requested resolved ones (by `requested_at`, then by approval id, a ULID), narrowed
/// by `filter`. Approvals of several agents may be given together.
pub fn inbox(approvals: Vec<ApprovalView>, filter: Filter) -> Vec<ApprovalView> {
    let (mut pending, resolved): (Vec<_>, Vec<_>) = approvals
        .into_iter()
        .partition(|view| view.state == ApprovalState::Pending);
    pending.sort_by_key(|view| view.deadline);
    let mut resolved: Vec<ApprovalView> = resolved.into_iter().rev().collect();
    resolved
        .sort_by(|a, b| (&b.requested_at, &b.approval_id).cmp(&(&a.requested_at, &a.approval_id)));
    let resolved = resolved.into_iter().take(RESOLVED_LISTED);
    match filter {
        Filter::All => pending.into_iter().chain(resolved).collect(),
        Filter::Pending => pending,
        Filter::Resolved => resolved.collect(),
    }
}

/// One agent's view from its deployment and its stream's rows.
pub fn agent(deployment: &Deployment, agent_rows: &[StoredEvent]) -> AgentView {
    let (agent_id, environment, mandate_version, deployed_at, stopped) = deployment;
    let pending = approvals(agent_id, agent_rows)
        .iter()
        .filter(|view| view.state == ApprovalState::Pending)
        .count();
    AgentView {
        agent_id: agent_id.clone(),
        environment: environment.clone(),
        mandate_version: mandate_version.clone(),
        deployed_at: deployed_at.clone(),
        mode: if *stopped {
            Some("stopped".to_owned())
        } else {
            mode(agent_rows)
        },
        pending_approvals: u64::try_from(pending).unwrap_or(u64::MAX),
    }
}

/// The bound order of an `ApprovalRequested` payload (DEC-1021): `content.action`'s members, and
/// version 2's `content.execution` time in force and protection, `null` without one.
pub fn order(payload: &Value) -> Option<Value> {
    let content = payload.get("content")?;
    let action = content.get("action")?;
    let mut members = Vec::with_capacity(9);
    for name in ACTION_MEMBERS {
        members.push((name, action.get(name)?.clone()));
    }
    let execution = content.get("execution");
    for name in ["tif", "protection"] {
        let planned = execution.and_then(|e| e.get(name)).cloned();
        members.push((name, planned.unwrap_or(Value::Null)));
    }
    object(members)
}

/// `content.action`'s members, every one of which an approval binds.
const ACTION_MEMBERS: [&str; 7] = [
    "instrument",
    "asset_class",
    "side",
    "qty",
    "limit",
    "order_usd",
    "purpose",
];

/// An object of `members`.
fn object(members: Vec<(&str, Value)>) -> Option<Value> {
    let mut object = mandate_canon::Object::new();
    for (name, value) in members {
        object.insert(mandate_canon::Key::new(name).ok()?, value);
    }
    Some(Value::Object(object))
}

/// The `account_ref` the control stream binds to `connection_id` (DEC-1020 item 3): the latest
/// `ConnectionEstablished` at schema version 2 naming it, else its latest `ConnectionRequested`.
pub fn account_ref(control: &[StoredEvent], connection_id: &str) -> Option<String> {
    let bound = |event_type: &str, schema_version: u64| {
        control.iter().rev().find_map(|row| {
            if row.event_type != event_type || row.schema_version != schema_version {
                return None;
            }
            let event = envelope(row)?;
            (text(&event, "payload.connection_id") == Some(connection_id))
                .then(|| text(&event, "payload.account_ref").map(str::to_owned))
                .flatten()
        })
    };
    bound("ConnectionEstablished", 2).or_else(|| bound("ConnectionRequested", 1))
}

/// The account's holdings at its last reconciliation (DEC-1020 item 3): the latest
/// `BrokerPositionObserved` per instrument, in instrument order, leaving out an instrument whose
/// model and broker quantities are both zero.
pub fn account_positions(account_rows: &[StoredEvent]) -> Vec<Value> {
    let mut latest: std::collections::BTreeMap<String, Value> = std::collections::BTreeMap::new();
    for row in account_rows {
        if row.event_type != "BrokerPositionObserved" {
            continue;
        }
        let Some(event) = envelope(row) else {
            continue;
        };
        let member = |name: &str| event.get("payload").and_then(|p| p.get(name)).cloned();
        let (Some(Value::Str(instrument)), Some(qty), Some(broker), Some(mismatch)) = (
            member("instrument"),
            member("model_qty"),
            member("broker_qty"),
            member("mismatch"),
        ) else {
            continue;
        };
        let held = [&qty, &broker].iter().any(|q| q.as_str() != Some("0"));
        let position = object(vec![
            ("instrument", Value::Str(instrument.clone())),
            ("qty", qty),
            ("broker_qty", broker),
            ("mismatch", mismatch),
            ("observed_at", Value::Str(row.recorded_at.clone())),
        ]);
        match (held, position) {
            (true, Some(position)) => {
                latest.insert(instrument, position);
            }
            _ => {
                latest.remove(&instrument);
            }
        }
    }
    latest.into_values().collect()
}
