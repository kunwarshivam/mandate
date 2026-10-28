//! A runtime's deployment read from the journal and the mandate document it names (DEC-1016), for
//! a launch that names only the workspace and the agent (DEC-1015 item 5).
//!
//! The latest `AgentDeployed` or `AgentStopped` of the agent in the process's environment decides
//! whether it is deployed, and the deployment names its mandate version: the hash of the stored
//! canonical document (journal spec §9.2). The document gives the connection and the approver
//! references, which resolve among the workspace's members as `mandate_identity::approvers`
//! resolves them for the shell and the executor (DEC-953, DEC-2384). The version's first
//! `MandateVersionCreated`, journaled by the user who created it, gives the author. A deployment
//! or document that is missing, corrupt, unreadable, or for another environment refuses rather than
//! guessing, a deployment record that cannot be read is never skipped for an older one, and any
//! control row the approvers' reading cannot read resolves no approver (`AGENTS.md` rules 3 and 8).
//! The approvers are read when the process resolves its deployment, and narrowed at every later read
//! of the control stream to those its rows still resolve to; nobody is added while it runs
//! (DEC-2818).

use std::collections::BTreeSet;

use mandate_canon::{Value, parse};
use mandate_gate_context::gate_rule_set;
use mandate_identity::approvers::{RecordView, members, resolve};
use mandate_journal::{ArtifactRef, ArtifactSource, Environment, StoredEvent, get_artifact};
use mandate_risk::GateConfig;
use mandate_spec::document::Mandate;
use mandate_time::UtcNanos;

/// What a deployment resolves to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub connection: String,
    pub approvers: Vec<String>,
    /// What the approvers are re-read from at each later read of the control stream.
    pub source: ApproverSource,
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
    let (version, document) = document(control, agent, environment, store)?;
    let connection = text(&document, &["connection_id"])
        .ok_or("mandate_document_unavailable")?
        .to_owned();
    let references: Vec<&str> = document
        .get("autonomy")
        .and_then(|v| v.get("approval"))
        .and_then(|v| v.get("approvers"))
        .and_then(Value::as_array)
        .ok_or("mandate_document_unavailable")?
        .iter()
        .filter_map(Value::as_str)
        .collect();
    let approvers = approvers(control, &connection, &references);
    let source = ApproverSource::new(
        approvers.iter().cloned().collect(),
        &connection,
        &references,
    );
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
        source,
        author,
    })
}

/// The mandate version `agent` is deployed with in `environment`, and its stored document: the
/// read [`resolved`] makes, for a caller that needs the document itself.
///
/// # Errors
/// `control_unreadable`, `agent_not_deployed`, `mandate_document_unavailable`, or
/// `mandate_environment_mismatch`.
pub fn document(
    control: &[StoredEvent],
    agent: &str,
    environment: Environment,
    store: &dyn ArtifactSource,
) -> Result<(String, Value), &'static str> {
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
    Ok((version, document))
}

/// The gate configuration of the latest rule set registered in `environment` (DEC-2388 item 3):
/// the newest `ConfigSnapshotRegistered` of kind `rule_set` names its artifact in `store`, whose
/// canonical bytes must read as the reviewed gate rule set. A newest one that is unreadable, not
/// stored, or not a gate's rule set is never skipped for an older one, nor is any registration
/// newer than it whose row does not parse, since that row may be a rule set.
///
/// # Errors
/// `rule_set_unavailable`.
pub fn rule_set(
    control: &[StoredEvent],
    environment: Environment,
    store: &dyn ArtifactSource,
) -> Result<GateConfig, &'static str> {
    control
        .iter()
        .rev()
        .filter(|row| {
            row.environment == environment.as_str() && row.event_type == "ConfigSnapshotRegistered"
        })
        .map(body)
        .find(|body| {
            body.as_ref()
                .is_none_or(|body| text(body, &["payload", "kind"]) == Some("rule_set"))
        })
        .flatten()
        .and_then(|body| text(&body, &["payload", "content_hash"]).and_then(ArtifactRef::parse))
        .and_then(|reference| get_artifact(store, &reference).ok())
        .and_then(|bytes| parse(&bytes).ok())
        .and_then(|rules| gate_rule_set(&rules).ok())
        .ok_or("rule_set_unavailable")
}

/// The approvers a process started with, and what it re-reads them from (DEC-2818): the mandate's
/// approver references and its connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApproverSource {
    admitted: BTreeSet<String>,
    connection: String,
    references: Vec<String>,
}

impl ApproverSource {
    /// The approvers `admitted` at the start, re-read from `references` resolved on `connection`.
    #[must_use]
    pub fn new(admitted: BTreeSet<String>, connection: &str, references: &[&str]) -> Self {
        Self {
            admitted,
            connection: connection.to_owned(),
            references: references.iter().map(|r| (*r).to_owned()).collect(),
        }
    }

    /// The admitted approvers, narrowed to those `control`, a fresh read of the control stream,
    /// still resolves to: a member deactivated, removed, or stripped of the role since the start
    /// never approves, a row that cannot be read resolves no one, and nobody is ever added, as the
    /// shell's `ApproverSource::current` narrows them (DEC-2384 item 6, `AGENTS.md` rule 3).
    #[must_use]
    pub fn current(&self, control: &[StoredEvent]) -> BTreeSet<String> {
        let references: Vec<&str> = self.references.iter().map(String::as_str).collect();
        let fresh: BTreeSet<String> = approvers(control, &self.connection, &references)
            .into_iter()
            .collect();
        self.admitted.intersection(&fresh).cloned().collect()
    }
}

/// The user ids `references` resolve to among the workspace's members, as
/// `mandate_identity::approvers` resolves them for the shell and the executor (DEC-2384): over
/// every control row in `seq` order, with its body's `event_time`. A row the run cannot read, one
/// whose body does not parse, that carries no payload, or that names no environment the run reads,
/// resolves no one, as the shell's reading of the same rows does: skipping it could leave a
/// revoked connection's owner or a removed approver standing (`AGENTS.md` rule 3).
fn approvers(control: &[StoredEvent], connection: &str, references: &[&str]) -> Vec<String> {
    let mut rows: Vec<&StoredEvent> = control.iter().collect();
    rows.sort_by_key(|row| row.seq);
    let mut read = Vec::with_capacity(rows.len());
    for row in rows {
        let Some(body) = body(row) else {
            return Vec::new();
        };
        if body.get("payload").is_none() || !matches!(row.environment.as_str(), "paper" | "live") {
            return Vec::new();
        }
        read.push((row, body));
    }
    let views: Vec<RecordView<'_>> = read
        .iter()
        .filter_map(|(row, body)| {
            Some(RecordView {
                seq: row.seq,
                event_type: &row.event_type,
                event_time: text(body, &["event_time"]).and_then(|at| UtcNanos::parse(at).ok()),
                payload: body.get("payload")?,
            })
        })
        .collect();
    resolve(references, &members(&views, connection))
        .into_iter()
        .collect()
}

fn body(row: &StoredEvent) -> Option<Value> {
    parse(&row.body).ok()
}

fn text<'a>(value: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter()
        .try_fold(value, |v, key| v.get(key))
        .and_then(Value::as_str)
}
