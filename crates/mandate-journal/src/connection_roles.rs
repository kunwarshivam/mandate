//! The role provisioner's records, journal spec v0.77 §9.33 (DEC-2350): `ConnectionRolesMade`,
//! `ConnectionRolesRefused` and `ConnectionRolesRemoved` on the control stream, and the stream
//! rules 262 and 263 that tie each to its request.
//!
//! The schemas close the three records at schema version 1 with rule 261, which `append` checks
//! on each record: a system actor and a cause. Rules 262 and 263 are the provisioner's: it checks
//! them against its own fold before it appends, and the verifier's fold,
//! [`verify_connection_roles`], fails closed on them. `append` folds no stream, so it never judges
//! them.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};

use crate::schema::Ty;
use crate::{Invalid, InvalidReason, StoredEvent, StreamType};

const MADE: &str = "ConnectionRolesMade";
const REFUSED: &str = "ConnectionRolesRefused";
const REMOVED: &str = "ConnectionRolesRemoved";
const REQUESTED: &str = "ConnectionRequested";
const CONNECTION_REFUSED: &str = "ConnectionRefused";
const REVOKED: &str = "ConnectionRevoked";

/// The control-stream event types §9.33 adds to [`crate::control::governs`].
pub(crate) const CONTROL: [&str; 3] = [MADE, REFUSED, REMOVED];

static CONNECTION_ROLES_MADE: Ty = Ty::Record(&[("connection_id", Ty::Ident)]);

static CONNECTION_ROLES_REFUSED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("reason", Ty::OneOf(&["vault_unavailable", "role_refused"])),
]);

static CONNECTION_ROLES_REMOVED: Ty = Ty::Record(&[
    ("connection_id", Ty::Ident),
    ("role", Ty::OneOf(&["token_exchange"])),
    ("occasion", Ty::OneOf(&["request_closed", "revoked"])),
]);

/// The §9.33 schema of `event_type` at `schema_version` on `stream`, or `None` when this module
/// registers none there.
pub(crate) fn schema(
    event_type: &str,
    schema_version: u64,
    stream: StreamType,
) -> Option<&'static Ty> {
    match (event_type, schema_version, stream) {
        (MADE, 1, StreamType::Control) => Some(&CONNECTION_ROLES_MADE),
        (REFUSED, 1, StreamType::Control) => Some(&CONNECTION_ROLES_REFUSED),
        (REMOVED, 1, StreamType::Control) => Some(&CONNECTION_ROLES_REMOVED),
        _ => None,
    }
}

/// Rule 261 on a §9.33 record: `actor.kind` is `system`, then `causation_id` is non-null. Every
/// other event type passes. `actor` and `causation_id` are the envelope's.
pub(crate) fn rules(
    event_type: &str,
    actor: Option<&Value>,
    causation_id: Option<&Value>,
) -> Result<(), Invalid> {
    if !CONTROL.contains(&event_type) {
        return Ok(());
    }
    let system = actor.and_then(|a| a.get("kind")).and_then(Value::as_str) == Some("system");
    ensure(system, "actor.kind")?;
    ensure(
        causation_id.is_some_and(|c| *c != Value::Null),
        "causation_id",
    )
}

fn ensure(holds: bool, path: &str) -> Result<(), Invalid> {
    if holds {
        Ok(())
    } else {
        Err(Invalid::new(InvalidReason::Schema, path))
    }
}

/// §9.33's stream rules, by the number the spec gives each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionRolesRule {
    /// Rule 262: a `ConnectionRolesMade` or `ConnectionRolesRefused` follows the version-2 OAuth
    /// `ConnectionRequested` its `causation_id` names, of its connection, once per request.
    Answered,
    /// Rule 263: a `ConnectionRolesRemoved` follows its connection's latest roles made, once, and
    /// names that request's own closer or a revocation after them.
    Removed,
}

impl ConnectionRolesRule {
    /// The rule's number in §9.33.
    #[must_use]
    pub fn number(self) -> u16 {
        match self {
            Self::Answered => 262,
            Self::Removed => 263,
        }
    }
}

/// Why [`verify_connection_roles`] refused the rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionRolesError {
    /// The first row, by its index in the rows given, that breaks `rule`.
    Broken {
        index: usize,
        rule: ConnectionRolesRule,
    },
    /// The report of a fold E21-33 has not implemented yet (DEC-77); the implementation removes it.
    Unimplemented { story: &'static str },
}

/// Rules 262 and 263 over a control stream's rows, in commit order from its first event: the
/// first §9.33 record that breaks either, by its index in `rows`. Rows of other event types are
/// folded for the requests, closers and revocations they hold, and a body that does not parse is
/// skipped, since §11's per-event check reports it.
///
/// # Errors
/// [`ConnectionRolesError::Broken`] at the first row that breaks rule 262 or 263.
pub fn verify_connection_roles(rows: &[StoredEvent]) -> Result<(), ConnectionRolesError> {
    let mut fold = Fold::default();
    for (index, row) in rows.iter().enumerate() {
        let Ok(body) = parse(&row.body) else {
            continue;
        };
        fold.step(row, &body)
            .map_err(|rule| ConnectionRolesError::Broken { index, rule })?;
    }
    Ok(())
}

/// How a closer closed its connection's roles: the request's own refusal, or a revocation; and
/// the occasion a removal names that is neither, which no closer is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Closer {
    RequestClosed,
    Revoked,
    Unnamed,
}

/// The provisioner's fold of one control stream, as §9.33 reads it.
#[derive(Default)]
struct Fold {
    /// Each `ConnectionRequested` by its `event_id`: its connection, and whether it is a
    /// version-2 OAuth request (rule 262).
    requests: BTreeMap<String, (String, bool)>,
    /// The requests a `ConnectionRolesMade` or `ConnectionRolesRefused` already answered.
    answered: BTreeSet<String>,
    /// The connections whose roles were made.
    made: BTreeSet<String>,
    /// The connections whose exchange role was removed since their latest roles were made.
    removed: BTreeSet<String>,
    /// Each connection's closers since its latest roles were made, by `event_id`.
    closers: BTreeMap<String, BTreeMap<String, Closer>>,
}

impl Fold {
    /// Folds one row, refusing it with the rule it breaks.
    fn step(&mut self, row: &StoredEvent, body: &Value) -> Result<(), ConnectionRolesRule> {
        let payload = body.get("payload");
        let member = |name: &str| {
            payload
                .and_then(|p| p.get(name))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let cause = body
            .get("causation_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let connection = member("connection_id");
        match row.event_type.as_str() {
            REQUESTED => {
                let oauth = row.schema_version == 2 && member("credential") == "oauth";
                self.requests
                    .insert(row.event_id.clone(), (connection, oauth));
            }
            CONNECTION_REFUSED if member("occasion") == "connect" => {
                self.close(connection, row, Closer::RequestClosed);
            }
            REVOKED => self.close(connection, row, Closer::Revoked),
            MADE | REFUSED => {
                let request = self.requests.get(&cause);
                let oauth = request.is_some_and(|(_, oauth)| *oauth);
                let own = request.is_some_and(|(of, _)| *of == connection);
                if !oauth || !own || !self.answered.insert(cause) {
                    return Err(ConnectionRolesRule::Answered);
                }
                if row.event_type == MADE {
                    self.removed.remove(&connection);
                    self.closers.insert(connection.clone(), BTreeMap::new());
                    self.made.insert(connection);
                }
            }
            REMOVED => {
                let closer = self
                    .closers
                    .get(&connection)
                    .and_then(|closers| closers.get(&cause))
                    .copied();
                let named = match member("occasion").as_str() {
                    "request_closed" => Closer::RequestClosed,
                    "revoked" => Closer::Revoked,
                    _ => Closer::Unnamed,
                };
                let first = !self.removed.contains(&connection);
                if !self.made.contains(&connection) || !first || closer != Some(named) {
                    return Err(ConnectionRolesRule::Removed);
                }
                self.removed.insert(connection);
            }
            _ => {}
        }
        Ok(())
    }

    /// Records `row` as a closer of `connection`'s roles, `how`.
    fn close(&mut self, connection: String, row: &StoredEvent, how: Closer) {
        self.closers
            .entry(connection)
            .or_default()
            .insert(row.event_id.clone(), how);
    }
}
