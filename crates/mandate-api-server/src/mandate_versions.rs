//! `GET /v1/workspaces/{ws}/agents/{agent}/mandate-versions` (§4.7,
//! [DEC-2147](../../../docs/project/decisions/DEC-2147.md), E10-27): the agent's mandate versions,
//! oldest first, each with its classification and journaled times, and the version in effect.
//! Read-only.
//!
//! - **The history** is the version the agent's `AgentDeployed` names, then each
//!   `MandateConfirmed` naming the agent recorded after it, in control-stream order.
//! - **The version in effect** is the deployed one, replaced by each `MandateVersionApplied`
//!   `applied` for the agent on its account stream whose `causation_id` is a listed confirmation:
//!   the executor's application, never the confirmation, puts a version in force (§5.1), and
//!   `in_effect` only ever names a version the history lists. Nothing is in effect when that
//!   stream is not found, or once `AgentStopped` names the agent.
//! - **An application agrees with its entry**: it names the entry's version over its base, with
//!   the classification computed here, and a reason exactly when rejected (journal spec rules 30
//!   and 33), and it is the confirmation's only one; one that does not, or a second answering
//!   the same confirmation, fails the read rather than serve a contradiction.
//! - **A classification** is mandate spec §9.2's verdict of the two stored documents, computed
//!   here as the confirm computes it, never read from a record.
//!
//! [`history`] is pure: rows in, the history out. A row of a folded kind whose members do not read
//! fails the whole fold, so no list is ever served short and no version in effect is read past a
//! record it skipped.

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::PathRejection;
use axum::extract::{Path, RawQuery, State};
use axum::http::HeaderMap;
use axum::response::Response;
use mandate_authn::Request;
use mandate_canon::Value;
use mandate_journal::{ArtifactSource, StoredEvent};
use mandate_spec::change::{ChangeClass, classify};

use crate::documents::envelope;
use crate::http::App;
use crate::reads;
use crate::routes::{Refused, account_of, object, path_refused, reader, served, string};
use crate::versions::{CONFIRMED, CREATED, DEPLOYED, stored};

const STOPPED: &str = "AgentStopped";
const APPLIED: &str = "MandateVersionApplied";

/// The deployment's read store, as [`App`] holds it.
pub type Store = dyn ArtifactSource + Send + Sync;

/// The classification of the version an agent was deployed with, which no earlier version bounds.
pub const FIRST_VERSION: &str = "first_version";

/// Why the fold serves no history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unserved {
    /// No `AgentDeployed` names the agent.
    NotFound,
    /// A row of a folded kind whose members do not read.
    Unreadable,
}

/// The executor's answer to a confirmation (journal spec §9.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Application {
    pub result: String,
    pub reason: Option<String>,
    pub recorded_at: String,
}

/// One entry of the history, as `read-models/agent-mandate-versions` states it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub mandate_version: String,
    pub base_version: Option<String>,
    pub classification: Option<&'static str>,
    pub created_at: Option<String>,
    pub confirmed_at: Option<String>,
    pub confirmation_id: Option<String>,
    pub deployed_at: Option<String>,
    pub application: Option<Application>,
}

/// The agent's history, the version in effect, and when it was stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct History {
    pub entries: Vec<Entry>,
    pub in_effect: Option<String>,
    pub stopped_at: Option<String>,
}

/// A row's payload, or `None` for a body that does not parse or holds no payload object.
fn payload(row: &StoredEvent) -> Option<Value> {
    reads::envelope(row)?
        .get("payload")
        .filter(|p| p.as_object().is_some())
        .cloned()
}

/// A member that must be text.
fn required(payload: &Value, name: &str) -> Result<String, Unserved> {
    payload
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(Unserved::Unreadable)
}

/// A member that is text, `null`, or absent; any other value does not read.
fn optional(value: &Value, name: &str) -> Result<Option<String>, Unserved> {
    match value.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Str(text)) => Ok(Some(text.clone())),
        Some(_) => Err(Unserved::Unreadable),
    }
}

/// One confirmation on the control stream: its version, base, id, and time.
#[derive(Clone)]
struct Confirmation {
    version: String,
    base: Option<String>,
    id: String,
    recorded_at: String,
}

/// One application on the account stream for the agent.
struct Applied {
    causation: Option<String>,
    old_version: String,
    new_version: String,
    classification: String,
    application: Application,
}

/// The agent's deployment: its version and time, and the last confirmation with no agent naming
/// that version before it.
struct Deployment {
    version: String,
    recorded_at: String,
    confirmed: Option<Confirmation>,
}

/// What the control stream says of `agent`, folded in stream order: a confirmation naming the
/// agent counts only once a deployment of it is read, so none before it is listed.
#[derive(Default)]
struct Control {
    created: BTreeMap<String, String>,
    agentless: BTreeMap<String, Confirmation>,
    deployment: Option<Deployment>,
    later: Vec<Confirmation>,
    stopped: Option<String>,
}

fn fold_control(control: &[StoredEvent], agent: &str) -> Result<Control, Unserved> {
    let mut folded = Control::default();
    for row in control {
        let kind = row.event_type.as_str();
        if ![CREATED, CONFIRMED, DEPLOYED, STOPPED].contains(&kind) {
            continue;
        }
        let payload = payload(row).ok_or(Unserved::Unreadable)?;
        let recorded_at = row.recorded_at.clone();
        match kind {
            CREATED => {
                let version = required(&payload, "mandate_version")?;
                folded.created.entry(version).or_insert(recorded_at);
            }
            CONFIRMED => {
                let confirmation = Confirmation {
                    version: required(&payload, "mandate_version")?,
                    base: optional(&payload, "base_version")?,
                    id: row.event_id.clone(),
                    recorded_at,
                };
                match optional(&payload, "agent_id")? {
                    None => {
                        folded
                            .agentless
                            .insert(confirmation.version.clone(), confirmation);
                    }
                    Some(named) if named == agent && folded.deployment.is_some() => {
                        folded.later.push(confirmation);
                    }
                    Some(_) => {}
                }
            }
            DEPLOYED => {
                let named = required(&payload, "agent_id")?;
                let version = required(&payload, "mandate_version")?;
                if named == agent {
                    folded.deployment = Some(Deployment {
                        confirmed: folded.agentless.get(&version).cloned(),
                        version,
                        recorded_at,
                    });
                }
            }
            _ => {
                if required(&payload, "agent_id")? == agent {
                    folded.stopped = Some(recorded_at);
                }
            }
        }
    }
    Ok(folded)
}

fn fold_account(account: &[StoredEvent], agent: &str) -> Result<Vec<Applied>, Unserved> {
    let mut applied = Vec::new();
    for row in account.iter().filter(|row| row.event_type == APPLIED) {
        let event = reads::envelope(row).ok_or(Unserved::Unreadable)?;
        let payload = payload(row).ok_or(Unserved::Unreadable)?;
        let named = required(&payload, "agent_id")?;
        let old_version = required(&payload, "old_version")?;
        let new_version = required(&payload, "new_version")?;
        let classification = required(&payload, "classification")?;
        let result = required(&payload, "result")?;
        let reason = optional(&payload, "reason")?;
        let causation = optional(&event, "causation_id")?;
        let rejected = match result.as_str() {
            "applied" => false,
            "rejected" => true,
            _ => return Err(Unserved::Unreadable),
        };
        if reason.is_some() != rejected {
            return Err(Unserved::Unreadable);
        }
        if named == agent {
            applied.push(Applied {
                causation,
                old_version,
                new_version,
                classification,
                application: Application {
                    result,
                    reason,
                    recorded_at: row.recorded_at.clone(),
                },
            });
        }
    }
    Ok(applied)
}

/// Mandate spec §9.2's verdict of `base` → `version` from the two stored documents, `None` when
/// the store cannot give either or they do not classify.
pub fn classification(store: Option<&Store>, base: &str, version: &str) -> Option<&'static str> {
    let store: &dyn ArtifactSource = store?;
    let (_, old) = stored(store, base)?;
    let (_, new) = stored(store, version)?;
    match classify(&old, &new).ok()?.class {
        ChangeClass::RiskIncreasing => Some("risk_increasing"),
        ChangeClass::RiskReducing => Some("risk_reducing"),
        ChangeClass::Neutral => Some("neutral"),
        ChangeClass::Invalid => None,
    }
}

/// `agent`'s history from the control stream and, when found, its account stream; classified
/// from `store`.
///
/// # Errors
/// [`Unserved::Unreadable`] for any row of a folded kind whose members do not read, then
/// [`Unserved::NotFound`] when no `AgentDeployed` names `agent`, then [`Unserved::Unreadable`]
/// for a later confirmation with no base, an application that disagrees with its entry, or a
/// second application answering one confirmation.
pub fn history(
    control: &[StoredEvent],
    account: Option<&[StoredEvent]>,
    agent: &str,
    store: Option<&Store>,
) -> Result<History, Unserved> {
    let folded = fold_control(control, agent)?;
    let applied = account.map(|rows| fold_account(rows, agent)).transpose()?;
    let deployment = folded.deployment.ok_or(Unserved::NotFound)?;
    let created = |version: &str| folded.created.get(version).cloned();
    let applied = applied.unwrap_or_default();
    let answering = |id: &str| {
        let mut answers = applied
            .iter()
            .filter(|a| a.causation.as_deref() == Some(id));
        let first = answers.next();
        match answers.next() {
            None => Ok(first),
            Some(_) => Err(Unserved::Unreadable),
        }
    };
    let deploying = deployment.confirmed.as_ref();
    let mut entries = vec![Entry {
        mandate_version: deployment.version.clone(),
        base_version: None,
        classification: Some(FIRST_VERSION),
        created_at: created(&deployment.version),
        confirmed_at: deploying.map(|c| c.recorded_at.clone()),
        confirmation_id: deploying.map(|c| c.id.clone()),
        deployed_at: Some(deployment.recorded_at.clone()),
        application: None,
    }];
    for confirmation in &folded.later {
        let base = confirmation.base.clone().ok_or(Unserved::Unreadable)?;
        let classified = classification(store, &base, &confirmation.version);
        let answer = answering(&confirmation.id)?;
        if let Some(answer) = answer {
            let agrees = answer.new_version == confirmation.version
                && answer.old_version == base
                && classified.is_none_or(|c| c == answer.classification);
            if !agrees {
                return Err(Unserved::Unreadable);
            }
        }
        entries.push(Entry {
            classification: classified,
            mandate_version: confirmation.version.clone(),
            base_version: Some(base),
            created_at: created(&confirmation.version),
            confirmed_at: Some(confirmation.recorded_at.clone()),
            confirmation_id: Some(confirmation.id.clone()),
            deployed_at: None,
            application: answer.map(|a| a.application.clone()),
        });
    }
    let stopped_at = folded.stopped;
    let deployed = deployment.version;
    let listed: Vec<&str> = folded.later.iter().map(|c| c.id.as_str()).collect();
    let in_effect = match (account, &stopped_at) {
        (Some(_), None) => Some(
            applied
                .iter()
                .rev()
                .find(|a| {
                    a.application.result == "applied"
                        && a.causation
                            .as_deref()
                            .is_some_and(|id| listed.contains(&id))
                })
                .map(|a| a.new_version.clone())
                .unwrap_or(deployed),
        ),
        _ => None,
    };
    Ok(History {
        entries,
        in_effect,
        stopped_at,
    })
}

fn text_or_null(value: Option<&str>) -> Value {
    value.map_or(Value::Null, string)
}

fn entry_value(entry: &Entry) -> Value {
    let application = entry.application.as_ref().map_or(Value::Null, |a| {
        object(vec![
            ("result", string(&a.result)),
            ("reason", text_or_null(a.reason.as_deref())),
            ("recorded_at", string(&a.recorded_at)),
        ])
    });
    object(vec![
        ("mandate_version", string(&entry.mandate_version)),
        ("base_version", text_or_null(entry.base_version.as_deref())),
        ("classification", text_or_null(entry.classification)),
        ("created_at", text_or_null(entry.created_at.as_deref())),
        ("confirmed_at", text_or_null(entry.confirmed_at.as_deref())),
        (
            "confirmation_id",
            text_or_null(entry.confirmation_id.as_deref()),
        ),
        ("deployed_at", text_or_null(entry.deployed_at.as_deref())),
        ("application", application),
    ])
}

/// The route: authorized by the "View agents, positions, decisions" row, then any query refused,
/// then the control stream and the deployed document's account stream folded.
pub async fn agent_mandate_versions(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    path: Result<Path<(String, String)>, PathRejection>,
    RawQuery(query): RawQuery,
) -> Response {
    let Ok(Path((workspace, agent))) = path else {
        return path_refused(&app, &headers, Request::Other).await;
    };
    let reader = match reader(&app, &headers, &workspace).await {
        Ok(reader) => reader,
        Err(refusal) => return refusal.response(),
    };
    if query.is_some() {
        return Refused::Invalid.response();
    }
    let deployed = match fold_control(&reader.control, &agent) {
        Ok(folded) => folded.deployment.map(|d| d.version),
        Err(_) => return Refused::JournalUnavailable.response(),
    };
    let document = deployed
        .as_deref()
        .zip(app.artifacts.as_deref())
        .and_then(|(version, store)| envelope(store, version));
    let account = match account_of(&app, &reader, document.as_ref()).await {
        Ok(account) => account,
        Err(refusal) => return refusal.response(),
    };
    let store = app.artifacts.as_deref();
    let rows = account.as_ref().map(|(_, rows)| rows.as_slice());
    let found = match history(&reader.control, rows, &agent, store) {
        Ok(found) => found,
        Err(Unserved::NotFound) => return Refused::NotFound.response(),
        Err(Unserved::Unreadable) => return Refused::JournalUnavailable.response(),
    };
    let members = vec![
        ("agent_id", string(&agent)),
        (
            "account_ref",
            text_or_null(account.as_ref().map(|(reference, _)| reference.as_str())),
        ),
        ("in_effect", text_or_null(found.in_effect.as_deref())),
        ("stopped_at", text_or_null(found.stopped_at.as_deref())),
        (
            "versions",
            Value::Array(found.entries.iter().map(entry_value).collect()),
        ),
    ];
    let streams: Vec<&[StoredEvent]> = rows.into_iter().collect();
    served(&reader, &streams, members)
}
