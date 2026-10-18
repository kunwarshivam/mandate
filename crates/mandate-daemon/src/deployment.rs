//! A runtime's deployment read from the journal and the mandate document it names (DEC-1016), for
//! a launch that names only the workspace and the agent (DEC-1015 item 5).
//!
//! The latest `AgentDeployed` or `AgentStopped` of the agent in the process's environment decides
//! whether it is deployed, and the deployment names its mandate version: the hash of the stored
//! canonical document (journal spec §9.2). The document gives the connection and the approvers
//! named as users; a `role:` approver is not resolved here, which can only make an approval harder
//! to grant. The version's first `MandateVersionCreated`, journaled by the user who created it,
//! gives the author. Anything missing, corrupt, unreadable, or for another environment refuses
//! rather than guessing, and a deployment record that cannot be read is never skipped for an older
//! one (`AGENTS.md` rules 3 and 8).

use mandate_canon::{Value, parse};
use mandate_journal::{ArtifactRef, ArtifactSource, Environment, StoredEvent, get_artifact};
use mandate_spec::document::Mandate;

/// What a deployment resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub connection: String,
    pub approvers: Vec<String>,
    pub author: String,
}

/// The deployment of `agent` in `environment`, from the workspace's `control` rows and the
/// documents in `store`.
///
/// # Errors
/// `control_unreadable`, `agent_not_deployed`, `mandate_document_unavailable`,
/// `mandate_environment_mismatch`, or `mandate_author_unknown`.
pub fn resolved(
    control: &[StoredEvent],
    agent: &str,
    environment: Environment,
    store: &dyn ArtifactSource,
) -> Result<Resolved, &'static str> {
    let mut latest = None;
    for row in control.iter().rev().filter(|row| {
        row.environment == environment.as_str()
            && matches!(row.event_type.as_str(), "AgentDeployed" | "AgentStopped")
    }) {
        let body = body(row).ok_or("control_unreadable")?;
        let named = text(&body, &["payload", "agent_id"]).ok_or("control_unreadable")?;
        if named == agent {
            latest = Some((row, body));
            break;
        }
    }
    let version = match latest {
        Some((row, body)) if row.event_type == "AgentDeployed" => {
            text(&body, &["payload", "mandate_version"]).map(str::to_owned)
        }
        _ => None,
    }
    .ok_or("agent_not_deployed")?;
    let document = ArtifactRef::parse(&version)
        .and_then(|reference| get_artifact(store, &reference).ok())
        .and_then(|bytes| parse(&bytes).ok())
        .filter(|document| Mandate::parse(document).is_ok())
        .ok_or("mandate_document_unavailable")?;
    if text(&document, &["environment"]) != Some(environment.as_str()) {
        return Err("mandate_environment_mismatch");
    }
    let connection = text(&document, &["connection_id"])
        .ok_or("mandate_document_unavailable")?
        .to_owned();
    let approvers = document
        .get("autonomy")
        .and_then(|v| v.get("approval"))
        .and_then(|v| v.get("approvers"))
        .and_then(Value::as_array)
        .ok_or("mandate_document_unavailable")?
        .iter()
        .filter_map(Value::as_str)
        .filter_map(|approver| approver.strip_prefix("user:"))
        .map(str::to_owned)
        .collect();
    let author = control
        .iter()
        .filter(|row| row.environment == environment.as_str())
        .filter(|row| row.event_type == "MandateVersionCreated")
        .filter_map(body)
        .find(|body| text(body, &["payload", "mandate_version"]) == Some(version.as_str()))
        .filter(|body| text(body, &["actor", "kind"]) == Some("user"))
        .and_then(|body| text(&body, &["actor", "id"]).map(str::to_owned))
        .ok_or("mandate_author_unknown")?;
    Ok(Resolved {
        connection,
        approvers,
        author,
    })
}

fn body(row: &StoredEvent) -> Option<Value> {
    parse(&row.body).ok()
}

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |v, key| v.get(key))
        .and_then(Value::as_str)
}
