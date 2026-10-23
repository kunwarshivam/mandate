//! What the provisioner has left to do, from the control stream alone: the requests whose roles it
//! has not answered, and the made roles whose request has closed or whose connection was revoked
//! since, with no removal yet. It reads §9.33's rules 262 and 263 the way
//! [`mandate_journal::verify_connection_roles`] does, so every step it plans journals a record that
//! fold accepts.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};
use mandate_journal::StoredEvent;

/// Why a removal is owed: the closer of the request the roles were made for, or a revocation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Occasion {
    RequestClosed,
    Revoked,
}

impl Occasion {
    /// The occasion as `ConnectionRolesRemoved` journals it.
    #[must_use]
    pub fn code(self) -> &'static str {
        match self {
            Self::RequestClosed => "request_closed",
            Self::Revoked => "revoked",
        }
    }
}

/// One step the provisioner owes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Make `connection`'s roles for the OAuth request `request`, by its `event_id`.
    Make { connection: String, request: String },
    /// Remove `connection`'s token exchange's role, for the closer or revocation `cause`.
    Remove {
        connection: String,
        cause: String,
        occasion: Occasion,
    },
}

/// The stream's environment, from its `StreamOpened`, which every record repeats, and the steps
/// owed, in the order the stream gives them.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub environment: Option<String>,
    pub steps: Vec<Step>,
}

/// Why no plan was made: never, since the plan reads every row it can and skips the rest; kept
/// so a caller's match stays the same across the crate's stubs (DEC-77).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanError {}

/// The steps owed over the control stream's `rows`, in commit order from its first event. A body
/// that does not parse is skipped, as the verifier's fold skips it.
///
/// A request whose connection is refused or revoked before its roles are made is owed nothing:
/// its token exchange never starts, so its roles are never needed.
///
/// # Errors
/// None: the plan reads every row it can.
pub fn plan(rows: &[StoredEvent]) -> Result<Plan, PlanError> {
    let mut fold = Fold::default();
    for row in rows {
        if row.event_type == "StreamOpened" {
            fold.environment
                .get_or_insert_with(|| row.environment.clone());
            continue;
        }
        if let Ok(body) = parse(&row.body) {
            fold.step(row, &body);
        }
    }
    Ok(fold.owed())
}

/// The plan's fold of the control stream.
#[derive(Default)]
struct Fold {
    environment: Option<String>,
    /// Each version-2 OAuth request still open, by its `event_id`, with its connection, in order.
    requests: Vec<(String, String)>,
    /// The requests answered by a `ConnectionRolesMade` or `ConnectionRolesRefused`.
    answered: BTreeSet<String>,
    /// The connections whose roles were made, in the order they were made, and not removed since.
    made: Vec<String>,
    /// Each made connection's first closer since the request its latest roles were made for.
    closers: BTreeMap<String, (String, Occasion)>,
    /// Every version-2 OAuth request's connection, by the request's `event_id`, open or closed.
    of: BTreeMap<String, String>,
    /// Each request's first closer: the roles made for it after it closed owe their removal at once,
    /// citing it (journal spec v0.78 rule 263, DEC-2350 item 7).
    first_closers: BTreeMap<String, (String, Occasion)>,
}

impl Fold {
    fn step(&mut self, row: &StoredEvent, body: &Value) {
        let member = |name: &str| {
            body.get("payload")
                .and_then(|p| p.get(name))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let connection = member("connection_id");
        let cause = body
            .get("causation_id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        match row.event_type.as_str() {
            "ConnectionRequested" => {
                if row.schema_version == 2 && member("credential") == "oauth" {
                    self.of.insert(row.event_id.clone(), connection.clone());
                    self.requests.push((row.event_id.clone(), connection));
                }
            }
            "ConnectionRefused" if member("occasion") == "connect" => {
                self.close(&connection, &row.event_id, Occasion::RequestClosed);
            }
            "ConnectionRevoked" => self.close(&connection, &row.event_id, Occasion::Revoked),
            "ConnectionRolesMade" | "ConnectionRolesRefused" => {
                self.answered.insert(cause.clone());
                if row.event_type == "ConnectionRolesMade" {
                    self.made.retain(|made| *made != connection);
                    match self.first_closers.get(&cause) {
                        Some(closer) => self.closers.insert(connection.clone(), closer.clone()),
                        None => self.closers.remove(&connection),
                    };
                    self.made.push(connection);
                }
            }
            "ConnectionRolesRemoved" => {
                self.made.retain(|made| *made != connection);
                self.closers.remove(&connection);
            }
            _ => {}
        }
    }

    /// Closes `connection`'s open requests not yet answered, and is the first closer of its made
    /// roles, if they have none yet.
    fn close(&mut self, connection: &str, closer: &str, occasion: Occasion) {
        for (request, of) in &self.of {
            if of == connection {
                self.first_closers
                    .entry(request.clone())
                    .or_insert_with(|| (closer.to_owned(), occasion));
            }
        }
        let answered = &self.answered;
        self.requests
            .retain(|(request, of)| of != connection || answered.contains(request));
        if self.made.iter().any(|made| made == connection) {
            self.closers
                .entry(connection.to_owned())
                .or_insert_with(|| (closer.to_owned(), occasion));
        }
    }

    fn owed(self) -> Plan {
        let mut steps: Vec<Step> = self
            .requests
            .into_iter()
            .filter(|(request, _)| !self.answered.contains(request))
            .map(|(request, connection)| Step::Make {
                connection,
                request,
            })
            .collect();
        for connection in self.made {
            if let Some((cause, occasion)) = self.closers.get(&connection) {
                steps.push(Step::Remove {
                    connection,
                    cause: cause.clone(),
                    occasion: *occasion,
                });
            }
        }
        Plan {
            environment: self.environment,
            steps,
        }
    }
}
