//! A connection's stream rules (journal spec v0.20 §9.8 rules 66 to 68) and §11's two connection
//! checks, `connection_lifecycle_mismatch` and `connection_cause_mismatch` (E7-17, DEC-800 item 9).
//! `append` folds no stream, so these rules are not `append`'s: the connection manager and the
//! executor check them against their own fold before appending, and verification runs them here
//! over the stored rows.
//!
//! Both functions take the rows of the control stream and of the account streams together, in
//! commit order, from each stream's first event: rules 66 and 67 fold the control stream, rule 68
//! folds each account stream on its own, and the cause check follows a `causation_id` from one
//! stream to the other. Rows of other event types are skipped, and the first failing row is
//! reported by its index in `rows`. A body that does not parse is skipped too, since §11's
//! per-event `non_canonical` check reports it.
//!
//! Each account stream and each control stream has its own fold, as §9.8 says. A version-1 first
//! establishment is never re-established (rule 66), and a control-stream establishment of an id
//! established before on that control stream, at either version, is a `reconnect` (§11);
//! `reference/journal/connections.py` takes the same readings since journal spec v0.30
//! ([DEC-696](../../../docs/project/decisions/DEC-696.md)). Streams have no global order (§2), so a
//! cause or an original is looked up among every row given, wherever it sits.

use std::collections::{BTreeMap, BTreeSet};

use mandate_canon::{Value, parse};

use crate::{StoredEvent, StreamId, StreamType};

const ESTABLISHED: &str = "ConnectionEstablished";
const REVOKED: &str = "ConnectionRevoked";
const ROTATED: &str = "ConnectionCredentialRotated";
const REFUSED: &str = "ConnectionRefused";
const CHECKED: &str = "ConnectionChecked";

/// The connection records on the control stream, which the connection manager folds.
const CONTROL_RECORDS: [&str; 4] = [ESTABLISHED, REVOKED, ROTATED, REFUSED];

/// The connection records on an account stream, which its executor folds.
const ACCOUNT_RECORDS: [&str; 5] = [
    CHECKED,
    "ConnectionStateChanged",
    "ConnectionCredentialRefreshed",
    ESTABLISHED,
    ROTATED,
];

/// The occasions an account stream's checks may have before its first binding (rule 68).
const UNBOUND_OCCASIONS: [&str; 3] = ["connect", "reconnect", "reauthorize"];

/// The brokers that connect through MCP (journal spec §9.8), whose checks always list `contract`.
const MCP_BROKERS: [&str; 1] = ["robinhood"];

/// A §9.8 stream rule: 66 (establishment), 67 (rotation and refusal), 68 (the account stream).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ConnectionStreamRule {
    /// A connection is established again only after its revocation, for the same broker,
    /// environment, and `account_ref`, and one `account_ref` belongs to one connection.
    Established,
    /// A rotation or a refused `reauthorize` names a live connection, a refused `reconnect` a
    /// revoked one, a refused `connect` a new id; and a rotation never widens the scopes.
    Rotated,
    /// One account stream, one connection: its binding, contract, rotations, and state changes.
    AccountStream,
}

impl ConnectionStreamRule {
    /// The rule's number in journal spec §9.8.
    pub fn number(self) -> u8 {
        match self {
            Self::Established => 66,
            Self::Rotated => 67,
            Self::AccountStream => 68,
        }
    }
}

/// §11's connection checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionCheck {
    /// A record breaks a stream rule.
    LifecycleMismatch(ConnectionStreamRule),
    /// A version-2 establishment's or a rotation's check is not the passing check of its occasion
    /// on the bound stream for the same connection, or a copy differs from its original.
    CauseMismatch,
}

impl ConnectionCheck {
    /// The check's code as §11 writes it.
    pub fn code(self) -> &'static str {
        match self {
            Self::LifecycleMismatch(_) => "connection_lifecycle_mismatch",
            Self::CauseMismatch => "connection_cause_mismatch",
        }
    }
}

/// The first row, by its index in the rows given, that fails a connection check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionFailure {
    pub index: usize,
    pub check: ConnectionCheck,
}

/// Why a connection check did not pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionVerifyError {
    Mismatch(ConnectionFailure),
}

/// §11's `connection_lifecycle_mismatch`: the first row that breaks rule 66, 67, or 68.
pub fn verify_connection_lifecycle(rows: &[StoredEvent]) -> Result<(), ConnectionVerifyError> {
    let rows = connection_rows(rows);
    let mut controls: BTreeMap<&str, ControlFold<'_>> = BTreeMap::new();
    let mut accounts: BTreeMap<&str, AccountFold<'_>> = BTreeMap::new();
    for row in &rows {
        let stream = row.stored.stream_id.as_str();
        let refused = match row.stream {
            StreamType::Control => controls.entry(stream).or_default().admit(row).err(),
            _ => {
                let admitted = accounts.entry(stream).or_default().admits(row);
                (!admitted).then_some(ConnectionStreamRule::AccountStream)
            }
        };
        if let Some(rule) = refused {
            return Err(mismatch(
                row.index,
                ConnectionCheck::LifecycleMismatch(rule),
            ));
        }
    }
    Ok(())
}

/// §11's `connection_cause_mismatch`: the first version-2 `ConnectionEstablished` or
/// `ConnectionCredentialRotated` on the control stream whose `causation_id` is not the passing
/// `ConnectionChecked` of its occasion (`connect`, `reconnect` for a second establishment, or
/// `reauthorize`) on the stream its `account_ref` names, for the same connection, listing
/// `contract` for an MCP broker; or the first copy on an account stream whose `causation_id`
/// names no control-stream original of its type, or whose payload without `risk_clock` differs
/// from that original's.
pub fn verify_connection_causes(rows: &[StoredEvent]) -> Result<(), ConnectionVerifyError> {
    let rows = connection_rows(rows);
    let by_id: BTreeMap<&str, &Row<'_>> = rows
        .iter()
        .map(|row| (row.stored.event_id.as_str(), row))
        .collect();
    let mut connections: BTreeMap<(&str, &str), (&str, &str)> = BTreeMap::new();
    for row in &rows {
        let kind = row.stored.event_type.as_str();
        let cause = row.causation().and_then(|id| by_id.get(id)).copied();
        let caused = match row.stream {
            _ if kind != ESTABLISHED && kind != ROTATED => true,
            StreamType::Control => {
                let p = row.payload();
                let key = (row.stored.stream_id.as_str(), text(p, "connection_id"));
                let occasion = match connections.contains_key(&key) {
                    _ if kind == ROTATED => "reauthorize",
                    true => "reconnect",
                    false => "connect",
                };
                if kind == ESTABLISHED {
                    connections.insert(key, (text(p, "account_ref"), text(p, "broker")));
                }
                row.stored.schema_version != 2 && kind == ESTABLISHED
                    || connections.get(&key).is_some_and(|&(account_ref, broker)| {
                        checked_for(row, cause, account_ref, broker, occasion)
                    })
            }
            _ => cause.is_some_and(|original| {
                let mut own = row.payload().as_object().cloned().unwrap_or_default();
                own.remove("risk_clock");
                original.stream == StreamType::Control
                    && original.stored.event_type == kind
                    && *original.payload() == Value::Object(own)
            }),
        };
        if !caused {
            return Err(mismatch(row.index, ConnectionCheck::CauseMismatch));
        }
    }
    Ok(())
}

/// Whether `cause` is the passing `ConnectionChecked` of `occasion` that `row`, a control-stream
/// establishment or rotation, rests on: on the account stream of `account_ref` in the control
/// stream's workspace, for the same connection, with `contract` listed for an MCP `broker`.
fn checked_for(
    row: &Row<'_>,
    cause: Option<&Row<'_>>,
    account_ref: &str,
    broker: &str,
    occasion: &str,
) -> bool {
    let workspace = row
        .stored
        .stream_id
        .strip_prefix("ctl:")
        .unwrap_or_default();
    let stream = format!("acct:{workspace}:{account_ref}");
    cause.is_some_and(|check| {
        let p = check.payload();
        check.stored.event_type == CHECKED
            && check.stored.stream_id == stream
            && text(p, "connection_id") == text(row.payload(), "connection_id")
            && text(p, "occasion") == occasion
            && all_passed(p)
            && (listed(p, "contract") || !MCP_BROKERS.contains(&broker))
    })
}

fn mismatch(index: usize, check: ConnectionCheck) -> ConnectionVerifyError {
    ConnectionVerifyError::Mismatch(ConnectionFailure { index, check })
}

/// A connection record on a control or account stream, with its index in the rows given.
struct Row<'a> {
    index: usize,
    stored: &'a StoredEvent,
    stream: StreamType,
    body: Value,
}

impl Row<'_> {
    fn payload(&self) -> &Value {
        self.body.get("payload").unwrap_or(&Value::Null)
    }

    fn causation(&self) -> Option<&str> {
        self.body.get("causation_id").and_then(Value::as_str)
    }
}

/// The rows §9.8's rules read: the connection records of the control and account streams.
fn connection_rows(rows: &[StoredEvent]) -> Vec<Row<'_>> {
    rows.iter()
        .enumerate()
        .filter_map(|(index, stored)| {
            let stream = StreamId::parse(&stored.stream_id)?.stream_type();
            let records: &[&str] = match stream {
                StreamType::Control => &CONTROL_RECORDS,
                StreamType::Account => &ACCOUNT_RECORDS,
                _ => &[],
            };
            records
                .contains(&stored.event_type.as_str())
                .then_some(())?;
            let body = parse(&stored.body).ok()?;
            Some(Row {
                index,
                stored,
                stream,
                body,
            })
        })
        .collect()
}

fn text<'a>(value: &'a Value, member: &str) -> &'a str {
    value
        .get(member)
        .and_then(Value::as_str)
        .unwrap_or_default()
}

fn results(p: &Value) -> &[Value] {
    p.get("results")
        .and_then(Value::as_array)
        .unwrap_or_default()
}

fn all_passed(p: &Value) -> bool {
    results(p).iter().all(|r| text(r, "result") == "passed")
}

fn listed(p: &Value, check: &str) -> bool {
    results(p).iter().any(|r| text(r, "check") == check)
}

/// The connection manager's fold of one control stream (rules 66 and 67).
#[derive(Default)]
struct ControlFold<'a> {
    /// Each connection's first establishment.
    first: BTreeMap<&'a str, &'a Value>,
    /// Each connection's latest `ConnectionEstablished` or `ConnectionRevoked`.
    latest: BTreeMap<&'a str, &'static str>,
    /// Each connection's latest scopes, its establishment's or the latest rotation's.
    scopes: BTreeMap<&'a str, BTreeSet<&'a str>>,
    /// The connection each `account_ref` belongs to (CN-5).
    holders: BTreeMap<&'a str, &'a str>,
}

impl<'a> ControlFold<'a> {
    fn admit(&mut self, row: &'a Row<'_>) -> Result<(), ConnectionStreamRule> {
        let p = row.payload();
        let id = text(p, "connection_id");
        let first = self.first.get(id).copied();
        let latest = self.latest.get(id).copied();
        let same = |member: &str| first.is_none_or(|f| text(f, member) == text(p, member));
        let scopes: BTreeSet<&str> = p
            .get("scopes")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        match row.stored.event_type.as_str() {
            ESTABLISHED => {
                let account_ref = p.get("account_ref").and_then(Value::as_str);
                let again = first.is_some_and(|f| {
                    latest != Some(REVOKED)
                        || !same("broker")
                        || !same("environment")
                        || f.get("account_ref").and_then(Value::as_str).is_none()
                        || f.get("account_ref").and_then(Value::as_str) != account_ref
                });
                let held = account_ref.and_then(|r| self.holders.get(r));
                if again || held.is_some_and(|holder| *holder != id) {
                    return Err(ConnectionStreamRule::Established);
                }
                if let Some(account_ref) = account_ref {
                    self.holders.entry(account_ref).or_insert(id);
                }
                self.first.entry(id).or_insert(p);
                self.latest.insert(id, ESTABLISHED);
                self.scopes.insert(id, scopes);
            }
            REVOKED => {
                self.latest.insert(id, REVOKED);
            }
            ROTATED => {
                let narrower = self
                    .scopes
                    .get(id)
                    .is_some_and(|held| scopes.is_subset(held));
                if latest != Some(ESTABLISHED) || !narrower {
                    return Err(ConnectionStreamRule::Rotated);
                }
                self.scopes.insert(id, scopes);
            }
            _ => {
                let occasion = text(p, "occasion");
                let wanted = match occasion {
                    "connect" => None,
                    "reconnect" => Some(REVOKED),
                    "reauthorize" => Some(ESTABLISHED),
                    _ => return Err(ConnectionStreamRule::Rotated),
                };
                if latest != wanted || !same("broker") || !same("environment") {
                    return Err(ConnectionStreamRule::Rotated);
                }
            }
        }
        Ok(())
    }
}

/// A check as an account stream's fold keeps it: whether it passed every result, whether it
/// listed `contract`, and the stream's latest entry into `suspended` before it.
#[derive(Clone, Copy)]
struct Check {
    passed: bool,
    contract: bool,
    suspension: Option<usize>,
}

/// The executor's fold of one account stream (rule 68).
#[derive(Default)]
struct AccountFold<'a> {
    /// The connection whose record came first; every later one names it.
    owner: Option<&'a str>,
    /// The broker of the latest binding copy; `None` while the stream is `connecting`.
    broker: Option<&'a str>,
    /// The latest check of each occasion.
    checks: BTreeMap<&'a str, Check>,
    /// Each rotation's copy, by event id, with the stream's latest entry into `suspended` before
    /// its `reauthorize` check.
    rotations: BTreeMap<&'a str, Option<usize>>,
    /// The state the latest `ConnectionStateChanged` left, `active` before any.
    state: Option<&'a str>,
    /// Whether the latest `ConnectionStateChanged` was a `condition_cleared`.
    cleared: bool,
    /// The index of the `ConnectionStateChanged` that last entered `suspended`.
    suspension: Option<usize>,
}

impl<'a> AccountFold<'a> {
    fn admits(&mut self, row: &'a Row<'_>) -> bool {
        let p = row.payload();
        let id = text(p, "connection_id");
        if *self.owner.get_or_insert(id) != id {
            return false;
        }
        let mcp = self.broker.is_some_and(|b| MCP_BROKERS.contains(&b));
        match row.stored.event_type.as_str() {
            CHECKED => {
                let occasion = text(p, "occasion");
                let contract = listed(p, "contract");
                let check = Check {
                    passed: all_passed(p),
                    contract,
                    suspension: self.suspension,
                };
                self.checks.insert(occasion, check);
                (self.broker.is_some() || UNBOUND_OCCASIONS.contains(&occasion))
                    && (contract || !mcp)
            }
            ESTABLISHED => {
                let occasion = match self.broker {
                    Some(_) => "reconnect",
                    None => "connect",
                };
                let broker = text(p, "broker");
                let stream_ref = row.stored.stream_id.rsplit(':').next().unwrap_or_default();
                self.broker = Some(broker);
                text(p, "account_ref") == stream_ref
                    && self.checks.get(occasion).is_some_and(|check| {
                        check.passed && (check.contract || !MCP_BROKERS.contains(&broker))
                    })
            }
            _ if self.broker.is_none() => false,
            ROTATED => match self.checks.get("reauthorize") {
                Some(check) if check.passed && (check.contract || !mcp) => {
                    self.rotations
                        .insert(row.stored.event_id.as_str(), check.suspension);
                    true
                }
                _ => false,
            },
            "ConnectionStateChanged" => self.moves(row),
            _ => true,
        }
    }

    /// Rule 68's state clause on a `ConnectionStateChanged`.
    fn moves(&mut self, row: &'a Row<'_>) -> bool {
        let p = row.payload();
        let (from, to, reason) = (text(p, "from"), text(p, "to"), text(p, "reason"));
        let rotation = row.causation().and_then(|id| self.rotations.get(id));
        let admitted = from == self.state.unwrap_or("active")
            && (reason != "acknowledged" || self.cleared)
            && (reason != "condition_cleared"
                || from != "suspended"
                || rotation.is_some_and(|suspension| *suspension == self.suspension));
        if to == "suspended" && from != "suspended" {
            self.suspension = Some(row.index);
        }
        self.cleared = reason == "condition_cleared";
        self.state = Some(to);
        admitted
    }
}
