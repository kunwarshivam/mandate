//! The connections read's fold (§4.5, [DEC-2620](../../../docs/project/decisions/DEC-2620.md)),
//! from the control stream's rows alone (API-14). It serves references only (API-11): no member
//! is a credential, the code, the `state`, a vault path, an account number, or the fingerprint,
//! and the journaled `account_ref`, `user`, and `step_up` are left out too (DEC-2620 item 4).
//!
//! Pure: rows in, values out.

use mandate_canon::Value;
use mandate_journal::StoredEvent;

use crate::reads::envelope;

/// Where a connection stands on the control stream (DEC-2620 item 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connecting,
    Established,
    Refused,
    Revoked,
}

impl ConnectionState {
    /// The wire name `read-models/connection` gives it.
    pub fn code(self) -> &'static str {
        match self {
            Self::Connecting => "connecting",
            Self::Established => "established",
            Self::Refused => "refused",
            Self::Revoked => "revoked",
        }
    }
}

/// A connect's `ConnectionRefused`: the first §8.1 check that failed (`None` for a teardown no
/// check caused) and its reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub check: Option<String>,
    pub reason: String,
}

/// One connection, as `read-models/connection` states it (DEC-2620 item 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionView {
    pub connection_id: String,
    pub broker: String,
    pub environment: String,
    pub state: ConnectionState,
    /// The latest establishment's granted scopes; `None` before one.
    pub scopes: Option<Vec<String>>,
    /// The `recorded_at` of the record that set `state`.
    pub since: String,
    /// Set exactly when `state` is [`ConnectionState::Refused`].
    pub refusal: Option<Refusal>,
    /// Version 2's `reason` when `state` is [`ConnectionState::Revoked`]; `None` otherwise, and
    /// for a version-1 revocation, which names none.
    pub revoked_reason: Option<String>,
    /// The agents whose latest `AgentDeployed` is version 2, names this connection, and is not
    /// stopped, in first-deployment order.
    pub agents: Vec<String>,
}

/// The text at `path` (dot-separated) in `value`.
fn text<'v>(value: &'v Value, path: &str) -> Option<&'v str> {
    path.split('.')
        .try_fold(value, |v, key| v.get(key))
        .and_then(Value::as_str)
}

/// The granted scopes of an establishment's payload; `None` unless every one is text.
fn granted(event: &Value) -> Option<Vec<String>> {
    let scopes = event.get("payload")?.get("scopes")?.as_array()?;
    scopes
        .iter()
        .map(|scope| scope.as_str().map(str::to_owned))
        .collect()
}

/// Every connection the control stream names, in the order its first record names it.
///
/// A row that does not parse, or lacks a member the fold reads, is skipped and the rest are
/// folded. The read shows what the stream says and decides nothing. The connect start's fold
/// (`connect::facts`) refuses instead, because a write must not act on a stream it cannot answer
/// for.
pub fn connections(control: &[StoredEvent]) -> Vec<ConnectionView> {
    let mut views: Vec<ConnectionView> = Vec::new();
    let mut deployments: Vec<(String, Option<String>, bool)> = Vec::new();
    for row in control {
        let Some(event) = envelope(row) else {
            continue;
        };
        match row.event_type.as_str() {
            "AgentDeployed" => {
                let Some(agent) = text(&event, "payload.agent_id") else {
                    continue;
                };
                let named = (row.schema_version == 2)
                    .then(|| text(&event, "payload.connection_id").map(str::to_owned))
                    .flatten();
                match deployments.iter_mut().find(|known| known.0 == agent) {
                    Some(known) => *known = (agent.to_owned(), named, false),
                    None => deployments.push((agent.to_owned(), named, false)),
                }
                continue;
            }
            "AgentStopped" => {
                let agent = text(&event, "payload.agent_id");
                if let Some(known) = deployments
                    .iter_mut()
                    .find(|known| Some(known.0.as_str()) == agent)
                {
                    known.2 = true;
                }
                continue;
            }
            _ => {}
        }
        let Some(id) = text(&event, "payload.connection_id") else {
            continue;
        };
        let connect_refusal = row.event_type == "ConnectionRefused"
            && text(&event, "payload.occasion") == Some("connect");
        let names = matches!(
            row.event_type.as_str(),
            "ConnectionRequested" | "ConnectionEstablished"
        ) || connect_refusal;
        let index = match views.iter().position(|view| view.connection_id == id) {
            Some(index) => index,
            None if names => {
                views.push(ConnectionView {
                    connection_id: id.to_owned(),
                    broker: String::new(),
                    environment: String::new(),
                    state: ConnectionState::Connecting,
                    scopes: None,
                    since: row.recorded_at.clone(),
                    refusal: None,
                    revoked_reason: None,
                    agents: Vec::new(),
                });
                views.len().saturating_sub(1)
            }
            None => continue,
        };
        let Some(view) = views.get_mut(index) else {
            continue;
        };
        if matches!(
            row.event_type.as_str(),
            "ConnectionRequested" | "ConnectionEstablished" | "ConnectionRefused"
        ) {
            if let Some(broker) = text(&event, "payload.broker") {
                view.broker = broker.to_owned();
            }
            if let Some(environment) = text(&event, "payload.environment") {
                view.environment = environment.to_owned();
            }
        }
        let (state, refusal, revoked_reason) = match row.event_type.as_str() {
            "ConnectionEstablished" => {
                view.scopes = granted(&event);
                (ConnectionState::Established, None, None)
            }
            "ConnectionRefused" if connect_refusal => {
                let refusal = Refusal {
                    check: text(&event, "payload.check").map(str::to_owned),
                    reason: text(&event, "payload.reason")
                        .unwrap_or_default()
                        .to_owned(),
                };
                (ConnectionState::Refused, Some(refusal), None)
            }
            "ConnectionRevoked" => {
                let reason = (row.schema_version == 2)
                    .then(|| text(&event, "payload.reason").map(str::to_owned))
                    .flatten();
                (ConnectionState::Revoked, None, reason)
            }
            _ => continue,
        };
        view.state = state;
        view.since = row.recorded_at.clone();
        view.refusal = refusal;
        view.revoked_reason = revoked_reason;
    }
    for view in &mut views {
        view.agents = deployments
            .iter()
            .filter(|(_, named, stopped)| {
                !stopped && named.as_deref() == Some(view.connection_id.as_str())
            })
            .map(|(agent, _, _)| agent.clone())
            .collect();
    }
    views
}
